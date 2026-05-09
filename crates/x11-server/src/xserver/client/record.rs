//! RECORD extension intercept and notification methods on ClientState.

use super::ClientState;
use crate::xserver::core::SEND_EVENT_FLAG;

/// Largest valid major opcode for a core (i.e. non-extension) X11 request.
/// Per the X11 protocol, opcodes 1..=127 are core requests; 128..=255 are
/// dynamically assigned to extensions via QueryExtension.
const CORE_REQUEST_OPCODE_MAX: u8 = 127;

/// Lowest valid X11 wire event code (KeyPress = 2). Codes 0 and 1 are
/// reserved for the Error and Reply response types respectively.
const X11_EVENT_TYPE_MIN: u8 = 2;

/// Highest valid X11 wire event code (GenericEvent = 35 in newer protocol;
/// 34 = MappingNotify is the last "core" type used by RECORD intercept).
const X11_EVENT_TYPE_MAX: u8 = 34;

impl ClientState {
    /// Intercept a request that was just received from this client and generate
    /// RECORD data replies for any enabled recording contexts that match.
    /// The generated replies are appended to `pending_events` for delivery.
    pub(crate) fn record_intercept_request(&mut self, request_data: &[u8]) {
        if request_data.is_empty() {
            return;
        }
        // Local contexts (self-interception)
        if !self.record_contexts.is_empty() {
            let server_time = self.timestamp();
            let seq = self.sequence;
            let intercepts = super::super::handlers::record::intercept_request(
                &self.record_contexts,
                &self.client_id,
                &self.client_id,
                self.resource_id_base,
                request_data,
                server_time,
                seq,
            );
            self.pending_events.extend(intercepts);
        }
        // Shared contexts (cross-client interception)
        if let Ok(shared) = self.shared_record_contexts.lock() {
            if shared.is_empty() {
                return;
            }
            let server_time = self.timestamp();
            let seq = self.sequence;
            for entry in shared.values() {
                if !entry.context.enabled {
                    continue;
                }
                if !entry.context.should_intercept_client(
                    &self.client_id,
                    &entry.recording_client_id,
                    self.resource_id_base,
                ) {
                    continue;
                }
                let major_opcode = request_data[0];
                let minor_opcode = if request_data.len() > 1 {
                    request_data[1]
                } else {
                    0
                };
                let matched = if major_opcode <= CORE_REQUEST_OPCODE_MAX {
                    entry.context.matches_core_request(major_opcode)
                } else {
                    entry
                        .context
                        .matches_ext_request(major_opcode, minor_opcode)
                };
                if matched {
                    let reply = super::super::handlers::record::build_record_data_reply(
                        super::super::handlers::record::RECORD_FROM_CLIENT,
                        entry.context.enable_sequence,
                        entry.context.element_header,
                        request_data,
                        server_time,
                        seq,
                    );
                    let _ = entry.event_tx.send(reply);
                }
            }
        }
    }

    /// Intercept a reply or error that is about to be sent to this client and
    /// generate RECORD data replies for any enabled recording contexts.
    pub(crate) fn record_intercept_response(
        &mut self,
        response: &[u8],
        major_opcode: u8,
        minor_opcode: u8,
    ) {
        if response.is_empty() {
            return;
        }
        // Local contexts (self-interception)
        if !self.record_contexts.is_empty() {
            let server_time = self.timestamp();
            let seq = self.sequence;
            if response[0] == 1 {
                let intercepts = super::super::handlers::record::intercept_reply(
                    &self.record_contexts,
                    &self.client_id,
                    &self.client_id,
                    self.resource_id_base,
                    response,
                    major_opcode,
                    minor_opcode,
                    server_time,
                    seq,
                );
                self.pending_events.extend(intercepts);
            } else if response[0] == 0 && response.len() >= 2 {
                let intercepts = super::super::handlers::record::intercept_error(
                    &self.record_contexts,
                    &self.client_id,
                    &self.client_id,
                    self.resource_id_base,
                    response,
                    server_time,
                    seq,
                );
                self.pending_events.extend(intercepts);
            }
        }
        // Shared contexts (cross-client interception)
        if let Ok(shared) = self.shared_record_contexts.lock() {
            if shared.is_empty() {
                return;
            }
            let server_time = self.timestamp();
            let seq = self.sequence;
            for entry in shared.values() {
                if !entry.context.enabled {
                    continue;
                }
                if !entry.context.should_intercept_client(
                    &self.client_id,
                    &entry.recording_client_id,
                    self.resource_id_base,
                ) {
                    continue;
                }
                if response[0] == 1 {
                    // Reply
                    let matched = if major_opcode <= CORE_REQUEST_OPCODE_MAX {
                        entry.context.matches_core_reply(major_opcode)
                    } else {
                        entry.context.matches_ext_reply(major_opcode, minor_opcode)
                    };
                    if matched {
                        let reply = super::super::handlers::record::build_record_data_reply(
                            super::super::handlers::record::RECORD_FROM_SERVER,
                            entry.context.enable_sequence,
                            entry.context.element_header,
                            response,
                            server_time,
                            seq,
                        );
                        let _ = entry.event_tx.send(reply);
                    }
                } else if response[0] == 0 && response.len() >= 2 {
                    // Error
                    let error_code = response[1];
                    if entry.context.matches_error(error_code) {
                        let reply = super::super::handlers::record::build_record_data_reply(
                            super::super::handlers::record::RECORD_FROM_SERVER,
                            entry.context.enable_sequence,
                            entry.context.element_header,
                            response,
                            server_time,
                            seq,
                        );
                        let _ = entry.event_tx.send(reply);
                    }
                }
            }
        }
    }

    /// Intercept events that are about to be delivered to this client and
    /// generate RECORD data replies for any matching recording contexts.
    /// Returns the RECORD intercept replies (caller must send them after the events).
    pub(crate) fn record_intercept_events(&self, events: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let mut results = Vec::new();
        // Local contexts (self-interception)
        if !self.record_contexts.is_empty() {
            for event in events {
                if event.len() == 32
                    && event[0] >= X11_EVENT_TYPE_MIN
                    && event[0] <= X11_EVENT_TYPE_MAX
                {
                    let server_time = self.timestamp();
                    let seq = self.sequence;
                    results.extend(super::super::handlers::record::intercept_event(
                        &self.record_contexts,
                        &self.client_id,
                        &self.client_id,
                        self.resource_id_base,
                        event,
                        server_time,
                        seq,
                    ));
                }
            }
        }
        // Shared contexts (cross-client interception)
        if let Ok(shared) = self.shared_record_contexts.lock() {
            if !shared.is_empty() {
                for event in events {
                    if event.len() == 32
                    && event[0] >= X11_EVENT_TYPE_MIN
                    && event[0] <= X11_EVENT_TYPE_MAX
                {
                        let event_code = event[0] & !SEND_EVENT_FLAG;
                        let server_time = self.timestamp();
                        let seq = self.sequence;
                        for entry in shared.values() {
                            if !entry.context.enabled {
                                continue;
                            }
                            if !entry.context.should_intercept_client(
                                &self.client_id,
                                &entry.recording_client_id,
                                self.resource_id_base,
                            ) {
                                continue;
                            }
                            if entry.context.matches_event(event_code) {
                                let reply = super::super::handlers::record::build_record_data_reply(
                                    super::super::handlers::record::RECORD_FROM_SERVER,
                                    entry.context.enable_sequence,
                                    entry.context.element_header,
                                    event,
                                    server_time,
                                    seq,
                                );
                                let _ = entry.event_tx.send(reply);
                            }
                        }
                    }
                }
            }
        }
        results
    }

    /// Generate RECORD ClientStarted notifications for all enabled contexts
    /// that have AllClients or FutureClients in their client_specs and a range
    /// with client_started set.
    pub(crate) fn record_notify_client_started(&mut self) {
        let server_time = self.timestamp();
        // Local contexts
        if !self.record_contexts.is_empty() {
            let notifications: Vec<Vec<u8>> = self
                .record_contexts
                .values()
                .filter(|ctx| ctx.enabled && ctx.wants_client_started())
                .filter(|ctx| ctx.client_specs.iter().any(|&s| s == 2 || s == 3))
                .map(|ctx| {
                    super::super::handlers::record::build_record_status_reply(
                        super::super::handlers::record::RECORD_CLIENT_STARTED,
                        ctx.enable_sequence,
                        ctx.element_header,
                        server_time,
                    )
                })
                .collect();
            self.pending_events.extend(notifications);
        }
        // Shared contexts: notify recording clients about this new client
        if let Ok(shared) = self.shared_record_contexts.lock() {
            for entry in shared.values() {
                if !entry.context.enabled || !entry.context.wants_client_started() {
                    continue;
                }
                if !entry.context.client_specs.iter().any(|&s| s == 2 || s == 3) {
                    continue;
                }
                // Don't notify the recording client about itself
                if entry.recording_client_id == self.client_id {
                    continue;
                }
                let reply = super::super::handlers::record::build_record_status_reply(
                    super::super::handlers::record::RECORD_CLIENT_STARTED,
                    entry.context.enable_sequence,
                    entry.context.element_header,
                    server_time,
                );
                let _ = entry.event_tx.send(reply);
            }
        }
    }

    /// Generate RECORD ClientDied notifications for all enabled contexts
    /// that have AllClients or FutureClients in their client_specs and a range
    /// with client_died set.
    pub(crate) fn record_notify_client_died(&mut self) {
        let server_time = self.timestamp();
        // Local contexts
        if !self.record_contexts.is_empty() {
            let notifications: Vec<Vec<u8>> = self
                .record_contexts
                .values()
                .filter(|ctx| ctx.enabled && ctx.wants_client_died())
                .filter(|ctx| ctx.client_specs.iter().any(|&s| s == 2 || s == 3))
                .map(|ctx| {
                    super::super::handlers::record::build_record_status_reply(
                        super::super::handlers::record::RECORD_CLIENT_DIED,
                        ctx.enable_sequence,
                        ctx.element_header,
                        server_time,
                    )
                })
                .collect();
            self.pending_events.extend(notifications);
        }
        // Shared contexts: notify recording clients about this client dying
        if let Ok(shared) = self.shared_record_contexts.lock() {
            for entry in shared.values() {
                if !entry.context.enabled || !entry.context.wants_client_died() {
                    continue;
                }
                if !entry.context.client_specs.iter().any(|&s| s == 2 || s == 3) {
                    continue;
                }
                if entry.recording_client_id == self.client_id {
                    continue;
                }
                let reply = super::super::handlers::record::build_record_status_reply(
                    super::super::handlers::record::RECORD_CLIENT_DIED,
                    entry.context.enable_sequence,
                    entry.context.element_header,
                    server_time,
                );
                let _ = entry.event_tx.send(reply);
            }
        }
    }
}
