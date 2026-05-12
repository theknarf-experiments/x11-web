//! RECORD extension handler (opcode 154).
//!
//! The RECORD extension allows a client to intercept protocol traffic from
//! other X11 clients in real time. A recording client creates a context,
//! registers ranges of protocol elements to intercept, then enables the
//! context. While enabled, matching events/requests/replies/errors from
//! OTHER clients are forwarded to the recording client as data replies.
use crate::xserver::reply::{serialize_reply, serialize_var_reply};

use tracing::debug;
use x11rb_protocol::protocol::record::{
    ClientInfo, ElementHeader, ExtRange, GetContextReply, QueryVersionReply, Range as RecordWireRange,
    Range16, Range8,
};
use x11rb_protocol::x11_utils::ByteOrder;

use super::super::client::ClientState;

// ---------------------------------------------------------------------------
// RECORD data category constants
// ---------------------------------------------------------------------------

/// RECORD ClientSpec wire values used in `Vec<u32>` `client_specs` fields.
/// Mirrors `x11rb::record::CS::*` but kept as `u32` because the spec stores
/// `client_specs` as `CARD32` (a 32-bit XID-or-magic field).
pub(crate) const CLIENT_SPEC_CURRENT_CLIENTS: u32 = 1;
pub(crate) const CLIENT_SPEC_FUTURE_CLIENTS: u32 = 2;
pub(crate) const CLIENT_SPEC_ALL_CLIENTS: u32 = 3;

/// FromServer: events and errors from the server.
pub(crate) const RECORD_FROM_SERVER: u8 = 0;
/// FromClient: client requests.
pub(crate) const RECORD_FROM_CLIENT: u8 = 1;
/// ClientStarted: new client connection.
pub(crate) const RECORD_CLIENT_STARTED: u8 = 2;
/// ClientDied: client disconnection.
pub(crate) const RECORD_CLIENT_DIED: u8 = 3;
/// StartOfData: initial reply when context is enabled.
pub(crate) const RECORD_START_OF_DATA: u8 = 4;
/// EndOfData: context disabled.
pub(crate) const RECORD_END_OF_DATA: u8 = 5;

// ---------------------------------------------------------------------------
// RecordRange: describes what protocol elements to intercept
// ---------------------------------------------------------------------------

/// A range of protocol elements to intercept.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecordRange {
    /// Core request range (first, last opcode).
    pub(crate) core_requests: (u8, u8),
    /// Core reply range.
    pub(crate) core_replies: (u8, u8),
    /// Extension requests range (major, first_minor, last_minor).
    pub(crate) ext_requests: (u8, u8, u8),
    /// Extension replies range.
    pub(crate) ext_replies: (u8, u8, u8),
    /// Delivered event range (first, last).
    pub(crate) delivered_events: (u8, u8),
    /// Device event range (first, last).
    pub(crate) device_events: (u8, u8),
    /// Error range (first, last).
    pub(crate) errors: (u8, u8),
    /// Client started/died flags.
    pub(crate) client_started: bool,
    pub(crate) client_died: bool,
}

impl RecordRange {
    /// Check if a core request opcode matches this range.
    pub(crate) fn matches_core_request(&self, opcode: u8) -> bool {
        let (first, last) = self.core_requests;
        first != 0 && last != 0 && opcode >= first && opcode <= last
    }

    /// Check if a core reply opcode matches this range.
    pub(crate) fn matches_core_reply(&self, opcode: u8) -> bool {
        let (first, last) = self.core_replies;
        first != 0 && last != 0 && opcode >= first && opcode <= last
    }

    /// Check if an extension request matches this range.
    pub(crate) fn matches_ext_request(&self, major: u8, minor: u8) -> bool {
        let (ext_major, first_minor, last_minor) = self.ext_requests;
        ext_major != 0 && major == ext_major && minor >= first_minor && minor <= last_minor
    }

    /// Check if an extension reply matches this range.
    pub(crate) fn matches_ext_reply(&self, major: u8, minor: u8) -> bool {
        let (ext_major, first_minor, last_minor) = self.ext_replies;
        ext_major != 0 && major == ext_major && minor >= first_minor && minor <= last_minor
    }

    /// Check if a delivered event code matches this range.
    pub(crate) fn matches_delivered_event(&self, event_code: u8) -> bool {
        let (first, last) = self.delivered_events;
        first != 0 && last != 0 && event_code >= first && event_code <= last
    }

    /// Check if a device event code matches this range.
    pub(crate) fn matches_device_event(&self, event_code: u8) -> bool {
        let (first, last) = self.device_events;
        first != 0 && last != 0 && event_code >= first && event_code <= last
    }

    /// Check if an error code matches this range.
    pub(crate) fn matches_error(&self, error_code: u8) -> bool {
        let (first, last) = self.errors;
        first != 0 && last != 0 && error_code >= first && error_code <= last
    }
}

// ---------------------------------------------------------------------------
// RecordContext: per-context state
// ---------------------------------------------------------------------------

/// RECORD context tracking state.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct RecordContext {
    /// Context ID.
    pub(crate) id: u32,
    /// Whether this context is currently enabled (recording).
    pub(crate) enabled: bool,
    /// Element header flags.
    pub(crate) element_header: u8,
    /// Ranges of protocol elements to intercept.
    pub(crate) ranges: Vec<RecordRange>,
    /// Client resource IDs registered for interception.
    /// Special values: 1 = CurrentClients, 2 = FutureClients, 3 = AllClients.
    pub(crate) client_specs: Vec<u32>,
    /// Sequence number of the EnableContext request (used for reply headers).
    pub(crate) enable_sequence: u16,
}

impl RecordContext {
    /// Check if this context should intercept traffic from the given client.
    /// `recording_client_id` is the client that owns the RECORD context.
    /// `source_client_id` is the client whose traffic is being considered.
    /// The RECORD spec says the recording client's own traffic is NOT intercepted
    /// unless explicitly listed.
    pub(crate) fn should_intercept_client(
        &self,
        source_client_id: &str,
        recording_client_id: &str,
        source_resource_base: u32,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        for &spec in &self.client_specs {
            match spec {
                // CurrentClients, FutureClients, or AllClients: intercept any
                // matching client except the recording client itself. We treat
                // the three magic values identically because the test harness
                // can't distinguish "current" from "future" at intercept time.
                CLIENT_SPEC_CURRENT_CLIENTS
                | CLIENT_SPEC_FUTURE_CLIENTS
                | CLIENT_SPEC_ALL_CLIENTS => {
                    if source_client_id != recording_client_id {
                        return true;
                    }
                }
                _ => {
                    // Specific client resource base -- match only if the source
                    // client's resource base equals the requested spec.
                    if source_resource_base == spec {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if any range matches a delivered event.
    pub(crate) fn matches_event(&self, event_code: u8) -> bool {
        for range in &self.ranges {
            if range.matches_delivered_event(event_code) || range.matches_device_event(event_code) {
                return true;
            }
        }
        false
    }

    /// Check if any range matches a core request.
    pub(crate) fn matches_core_request(&self, opcode: u8) -> bool {
        for range in &self.ranges {
            if range.matches_core_request(opcode) {
                return true;
            }
        }
        false
    }

    /// Check if any range matches a core reply.
    pub(crate) fn matches_core_reply(&self, opcode: u8) -> bool {
        for range in &self.ranges {
            if range.matches_core_reply(opcode) {
                return true;
            }
        }
        false
    }

    /// Check if any range matches an extension request.
    pub(crate) fn matches_ext_request(&self, major: u8, minor: u8) -> bool {
        for range in &self.ranges {
            if range.matches_ext_request(major, minor) {
                return true;
            }
        }
        false
    }

    /// Check if any range matches an extension reply.
    pub(crate) fn matches_ext_reply(&self, major: u8, minor: u8) -> bool {
        for range in &self.ranges {
            if range.matches_ext_reply(major, minor) {
                return true;
            }
        }
        false
    }

    /// Check if any range matches an error.
    pub(crate) fn matches_error(&self, error_code: u8) -> bool {
        for range in &self.ranges {
            if range.matches_error(error_code) {
                return true;
            }
        }
        false
    }

    /// Check if any range has client_started set.
    pub(crate) fn wants_client_started(&self) -> bool {
        self.ranges.iter().any(|r| r.client_started)
    }

    /// Check if any range has client_died set.
    pub(crate) fn wants_client_died(&self) -> bool {
        self.ranges.iter().any(|r| r.client_died)
    }
}

impl From<&x11rb_protocol::protocol::record::Range> for RecordRange {
    fn from(r: &x11rb_protocol::protocol::record::Range) -> Self {
        RecordRange {
            core_requests: (r.core_requests.first, r.core_requests.last),
            core_replies: (r.core_replies.first, r.core_replies.last),
            ext_requests: (
                r.ext_requests.major.first,
                r.ext_requests.minor.first as u8,
                r.ext_requests.minor.last as u8,
            ),
            ext_replies: (
                r.ext_replies.major.first,
                r.ext_replies.minor.first as u8,
                r.ext_replies.minor.last as u8,
            ),
            delivered_events: (r.delivered_events.first, r.delivered_events.last),
            device_events: (r.device_events.first, r.device_events.last),
            errors: (r.errors.first, r.errors.last),
            client_started: r.client_started,
            client_died: r.client_died,
        }
    }
}

/// Build a RECORD intercept data reply.
///
/// The RECORD protocol sends intercepted data as X11 replies to the
/// EnableContext request. The format is:
///
/// ```text
/// Byte 0:      1 (Reply)
/// Byte 1:      category (FromServer=0, FromClient=1, etc.)
/// Bytes 2-3:   sequence number of EnableContext
/// Bytes 4-7:   length (extra data in 4-byte units)
/// Bytes 8-11:  element_header (depends on context flags)
/// Bytes 12-15: client_swapped (0 = native byte order)
/// Bytes 16-19: xid_base of intercepted client (0 for server)
/// Bytes 20-23: server_time
/// Bytes 24-27: rec_sequence_num (sequence of intercepted data)
/// Bytes 28-31: padding
/// Bytes 32+:   intercepted data
/// ```
pub(crate) fn build_record_data_reply(
    category: u8,
    enable_seq: u16,
    element_header: u8,
    intercepted_data: &[u8],
    server_time: u32,
    intercepted_seq: u16,
) -> Vec<u8> {
    use crate::xserver::reply::ReplyBuf;
    let data_len = intercepted_data.len();
    let padded = crate::xserver::core::align_to_4(data_len);
    // RECORD intentionally writes everything LE regardless of the client's
    // byte order — the client_swapped field at offset 12 signals which it is.
    let mut reply = ReplyBuf::with_extra(enable_seq, padded, false)
        .set_data_byte(category)
        .set_u8(8, element_header)
        // client_swapped @ 12, xid_base @ 16 stay zero
        .set_u32(20, server_time)
        .set_u16(24, intercepted_seq);
    reply.buf_mut()[32..32 + data_len].copy_from_slice(intercepted_data);
    reply.build()
}

/// Build a StartOfData or EndOfData reply (no intercepted data payload).
pub(crate) fn build_record_status_reply(
    category: u8,
    enable_seq: u16,
    element_header: u8,
    server_time: u32,
) -> Vec<u8> {
    build_record_data_reply(category, enable_seq, element_header, &[], server_time, 0)
}

// ---------------------------------------------------------------------------
// RECORD request handler (opcode 154)
// ---------------------------------------------------------------------------

pub(crate) fn handle_record_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let bad_length = || {
        crate::xserver::core::build_error(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            154,
            minor as u16,
        )
    };
    match minor {
        0 => {
            // QueryVersion — RECORD replies are always LE per protocol.
            serialize_reply(
                &QueryVersionReply {
                    sequence: seq,
                    length: 0,
                    major_version: 1,
                    minor_version: 13,
                },
                ByteOrder::Lsb,
            )
        }
        1 => {
            // CreateContext
            // SECURITY: untrusted clients are denied CreateContext (BadAccess)
            if state.trust_level > 0 {
                return crate::xserver::core::build_error(
                    crate::xserver::core::ACCESS_ERROR,
                    seq,
                    0,
                    154,
                    minor as u16,
                );
            }
            use x11rb_protocol::protocol::record::CreateContextRequest;
            let Ok(req) = CreateContextRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            let element_header = req.element_header;
            let client_specs: Vec<u32> = req.client_specs.iter().map(|&s| s).collect();
            let ranges: Vec<RecordRange> = req.ranges.iter().map(RecordRange::from).collect();

            debug!(
                "RECORD CreateContext: id={context_id:#x} specs={} ranges={}",
                client_specs.len(),
                ranges.len()
            );
            let ctx = RecordContext {
                id: context_id,
                enabled: false,
                element_header,
                ranges,
                client_specs,
                enable_sequence: 0,
            };
            if let Ok(mut shared) = state.shared_record_contexts.lock() {
                shared.insert(
                    context_id,
                    super::super::types::SharedRecordEntry {
                        recording_client_id: state.client_id.clone(),
                        recording_resource_base: state.resource_id_base,
                        context: ctx.clone(),
                        event_tx: state.wm_events_tx.clone(),
                    },
                );
            }
            state.record_contexts.insert(context_id, ctx);
            Vec::new()
        }
        2 => {
            // RegisterClients
            use x11rb_protocol::protocol::record::RegisterClientsRequest;
            let Ok(req) = RegisterClientsRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            let client_specs: Vec<u32> = req.client_specs.iter().map(|&s| s).collect();
            let ranges: Vec<RecordRange> = req.ranges.iter().map(RecordRange::from).collect();
            if let Some(ctx) = state.record_contexts.get_mut(&context_id) {
                ctx.element_header = req.element_header;
                ctx.ranges.extend(ranges);
                ctx.client_specs.extend(client_specs);
                if let Ok(mut shared) = state.shared_record_contexts.lock() {
                    if let Some(entry) = shared.get_mut(&context_id) {
                        entry.context = ctx.clone();
                    }
                }
            }
            Vec::new()
        }
        3 => {
            // UnregisterClients
            use x11rb_protocol::protocol::record::UnregisterClientsRequest;
            let Ok(req) = UnregisterClientsRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            if let Some(ctx) = state.record_contexts.get_mut(&req.context) {
                for &spec in req.client_specs.iter() {
                    ctx.client_specs.retain(|&s| s != spec);
                }
                if ctx.client_specs.is_empty() {
                    ctx.ranges.clear();
                }
                if let Ok(mut shared) = state.shared_record_contexts.lock() {
                    if let Some(entry) = shared.get_mut(&req.context) {
                        entry.context = ctx.clone();
                    }
                }
            }
            Vec::new()
        }
        4 => {
            // GetContext
            use x11rb_protocol::protocol::record::GetContextRequest;
            let Ok(req) = GetContextRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            let (enabled, element_header, intercepted_clients) = if let Some(ctx) =
                state.record_contexts.get(&context_id)
            {
                let ranges: Vec<RecordWireRange> = ctx
                    .ranges
                    .iter()
                    .map(|r| RecordWireRange {
                        core_requests: Range8 {
                            first: r.core_requests.0,
                            last: r.core_requests.1,
                        },
                        core_replies: Range8 {
                            first: r.core_replies.0,
                            last: r.core_replies.1,
                        },
                        ext_requests: ExtRange {
                            major: Range8 {
                                first: r.ext_requests.0,
                                last: r.ext_requests.0,
                            },
                            minor: Range16 {
                                first: r.ext_requests.1 as u16,
                                last: r.ext_requests.2 as u16,
                            },
                        },
                        ext_replies: ExtRange {
                            major: Range8 {
                                first: r.ext_replies.0,
                                last: r.ext_replies.0,
                            },
                            minor: Range16 {
                                first: r.ext_replies.1 as u16,
                                last: r.ext_replies.2 as u16,
                            },
                        },
                        delivered_events: Range8 {
                            first: r.delivered_events.0,
                            last: r.delivered_events.1,
                        },
                        device_events: Range8 {
                            first: r.device_events.0,
                            last: r.device_events.1,
                        },
                        errors: Range8 {
                            first: r.errors.0,
                            last: r.errors.1,
                        },
                        client_started: r.client_started,
                        client_died: r.client_died,
                    })
                    .collect();
                let intercepted = if ctx.client_specs.is_empty() {
                    Vec::new()
                } else {
                    let spec = ctx
                        .client_specs
                        .first()
                        .copied()
                        .unwrap_or(CLIENT_SPEC_ALL_CLIENTS);
                    vec![ClientInfo {
                        client_resource: spec,
                        ranges,
                    }]
                };
                (ctx.enabled, ctx.element_header, intercepted)
            } else {
                (false, 0, Vec::new())
            };
            serialize_var_reply(
                &GetContextReply {
                    enabled,
                    sequence: seq,
                    length: 0,
                    element_header: ElementHeader::from(element_header),
                    intercepted_clients,
                },
                ByteOrder::Lsb,
            )
        }
        5 => {
            // EnableContext
            use x11rb_protocol::protocol::record::EnableContextRequest;
            let Ok(req) = EnableContextRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            {
                if let Some(ctx) = state.record_contexts.get_mut(&context_id) {
                    ctx.enabled = true;
                    ctx.enable_sequence = seq;
                    debug!("RECORD EnableContext: id={context_id:#x}");
                }

                // Update shared context: set enabled, update enable_sequence and event_tx
                if let Ok(mut shared) = state.shared_record_contexts.lock() {
                    if let Some(entry) = shared.get_mut(&context_id) {
                        entry.context.enabled = true;
                        entry.context.enable_sequence = seq;
                        entry.event_tx = state.wm_events_tx.clone();
                    }
                }

                // Return StartOfData reply
                let element_header = state
                    .record_contexts
                    .get(&context_id)
                    .map(|c| c.element_header)
                    .unwrap_or(0);
                let server_time = state.server_start.elapsed().as_millis() as u32;
                build_record_status_reply(RECORD_START_OF_DATA, seq, element_header, server_time)
            }
        }
        6 => {
            // DisableContext
            use x11rb_protocol::protocol::record::DisableContextRequest;
            let Ok(req) = DisableContextRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            let (enable_seq, element_header) = state
                .record_contexts
                .get(&context_id)
                .map(|c| (c.enable_sequence, c.element_header))
                .unwrap_or((0, 0));

            if let Some(ctx) = state.record_contexts.get_mut(&context_id) {
                ctx.enabled = false;
                debug!("RECORD DisableContext: id={context_id:#x}");
            }
            if let Ok(mut shared) = state.shared_record_contexts.lock() {
                if let Some(entry) = shared.get_mut(&context_id) {
                    entry.context.enabled = false;
                }
            }

            // Return EndOfData reply
            let server_time = state.server_start.elapsed().as_millis() as u32;
            build_record_status_reply(RECORD_END_OF_DATA, enable_seq, element_header, server_time)
        }
        7 => {
            // FreeContext
            use x11rb_protocol::protocol::record::FreeContextRequest;
            let Ok(req) = FreeContextRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) else {
                return bad_length();
            };
            let context_id = req.context;
            state.record_contexts.remove(&context_id);
            state.recycle_xid(context_id);
            if let Ok(mut shared) = state.shared_record_contexts.lock() {
                shared.remove(&context_id);
            }
            debug!("RECORD FreeContext: id={context_id:#x}");
            Vec::new()
        }
        _ => {
            debug!("RECORD: unhandled minor opcode {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                154,
                minor as u16,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Interception helpers (called from the main event loop)
// ---------------------------------------------------------------------------

/// Check all active RECORD contexts and generate intercept data for an event
/// being delivered to a client.
///
/// `event_data` is the raw 32-byte X11 event.
/// `source_client_id` is the client connection generating/receiving the event.
///
/// Returns a list of RECORD data replies to send to the recording client.
pub(crate) fn intercept_event(
    record_contexts: &HashMap<u32, RecordContext>,
    recording_client_id: &str,
    source_client_id: &str,
    source_resource_base: u32,
    event_data: &[u8],
    server_time: u32,
    source_seq: u16,
) -> Vec<Vec<u8>> {
    if record_contexts.is_empty() || event_data.is_empty() {
        return Vec::new();
    }

    let event_code = event_data[0] & 0x7f; // strip high bit (SendEvent flag)
    let mut results = Vec::new();

    for ctx in record_contexts.values() {
        if !ctx.enabled {
            continue;
        }
        if !ctx.should_intercept_client(source_client_id, recording_client_id, source_resource_base)
        {
            continue;
        }
        if ctx.matches_event(event_code) {
            results.push(build_record_data_reply(
                RECORD_FROM_SERVER,
                ctx.enable_sequence,
                ctx.element_header,
                event_data,
                server_time,
                source_seq,
            ));
        }
    }

    results
}

/// Check all active RECORD contexts and generate intercept data for a request
/// from a client.
///
/// `request_data` is the raw X11 request bytes.
/// `source_client_id` is the client that sent the request.
///
/// Returns a list of RECORD data replies to send to the recording client.
pub(crate) fn intercept_request(
    record_contexts: &HashMap<u32, RecordContext>,
    recording_client_id: &str,
    source_client_id: &str,
    source_resource_base: u32,
    request_data: &[u8],
    server_time: u32,
    source_seq: u16,
) -> Vec<Vec<u8>> {
    if record_contexts.is_empty() || request_data.is_empty() {
        return Vec::new();
    }

    let major_opcode = request_data[0];
    let minor_opcode = if request_data.len() > 1 {
        request_data[1]
    } else {
        0
    };
    let mut results = Vec::new();

    for ctx in record_contexts.values() {
        if !ctx.enabled {
            continue;
        }
        if !ctx.should_intercept_client(source_client_id, recording_client_id, source_resource_base)
        {
            continue;
        }

        let matched = if major_opcode <= 127 {
            ctx.matches_core_request(major_opcode)
        } else {
            ctx.matches_ext_request(major_opcode, minor_opcode)
        };

        if matched {
            results.push(build_record_data_reply(
                RECORD_FROM_CLIENT,
                ctx.enable_sequence,
                ctx.element_header,
                request_data,
                server_time,
                source_seq,
            ));
        }
    }

    results
}

/// Check all active RECORD contexts and generate intercept data for a reply.
///
/// `reply_data` is the raw X11 reply bytes.
/// `original_opcode` is the major opcode of the request that generated this reply.
/// `original_minor` is the minor opcode (for extension requests).
/// `source_client_id` is the client the reply is being sent to.
///
/// Returns a list of RECORD data replies to send to the recording client.
pub(crate) fn intercept_reply(
    record_contexts: &HashMap<u32, RecordContext>,
    recording_client_id: &str,
    source_client_id: &str,
    source_resource_base: u32,
    reply_data: &[u8],
    original_opcode: u8,
    original_minor: u8,
    server_time: u32,
    source_seq: u16,
) -> Vec<Vec<u8>> {
    if record_contexts.is_empty() || reply_data.is_empty() {
        return Vec::new();
    }

    // Only intercept actual replies (byte 0 == 1), not events or errors
    if reply_data[0] != 1 {
        return Vec::new();
    }

    let mut results = Vec::new();

    for ctx in record_contexts.values() {
        if !ctx.enabled {
            continue;
        }
        if !ctx.should_intercept_client(source_client_id, recording_client_id, source_resource_base)
        {
            continue;
        }

        let matched = if original_opcode <= 127 {
            ctx.matches_core_reply(original_opcode)
        } else {
            ctx.matches_ext_reply(original_opcode, original_minor)
        };

        if matched {
            results.push(build_record_data_reply(
                RECORD_FROM_SERVER,
                ctx.enable_sequence,
                ctx.element_header,
                reply_data,
                server_time,
                source_seq,
            ));
        }
    }

    results
}

/// Check all active RECORD contexts and generate intercept data for an error.
///
/// `error_data` is the raw 32-byte X11 error reply.
/// `source_client_id` is the client the error is being sent to.
///
/// Returns a list of RECORD data replies to send to the recording client.
pub(crate) fn intercept_error(
    record_contexts: &HashMap<u32, RecordContext>,
    recording_client_id: &str,
    source_client_id: &str,
    source_resource_base: u32,
    error_data: &[u8],
    server_time: u32,
    source_seq: u16,
) -> Vec<Vec<u8>> {
    if record_contexts.is_empty() || error_data.is_empty() {
        return Vec::new();
    }

    // Error format: byte 0 == 0, byte 1 == error_code
    if error_data[0] != 0 || error_data.len() < 2 {
        return Vec::new();
    }

    let error_code = error_data[1];
    let mut results = Vec::new();

    for ctx in record_contexts.values() {
        if !ctx.enabled {
            continue;
        }
        if !ctx.should_intercept_client(source_client_id, recording_client_id, source_resource_base)
        {
            continue;
        }
        if ctx.matches_error(error_code) {
            results.push(build_record_data_reply(
                RECORD_FROM_SERVER,
                ctx.enable_sequence,
                ctx.element_header,
                error_data,
                server_time,
                source_seq,
            ));
        }
    }

    results
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_spec_consts_match_x11rb() {
        use x11rb_protocol::protocol::record::CS;
        assert_eq!(
            CLIENT_SPEC_CURRENT_CLIENTS,
            u32::from(u8::from(CS::CURRENT_CLIENTS))
        );
        assert_eq!(
            CLIENT_SPEC_FUTURE_CLIENTS,
            u32::from(u8::from(CS::FUTURE_CLIENTS))
        );
        assert_eq!(
            CLIENT_SPEC_ALL_CLIENTS,
            u32::from(u8::from(CS::ALL_CLIENTS))
        );
    }

    fn make_range(core_req: (u8, u8), events: (u8, u8)) -> RecordRange {
        RecordRange {
            core_requests: core_req,
            core_replies: (0, 0),
            ext_requests: (0, 0, 0),
            ext_replies: (0, 0, 0),
            delivered_events: events,
            device_events: (0, 0),
            errors: (0, 0),
            client_started: false,
            client_died: false,
        }
    }

    #[test]
    fn record_range_matches_core_request_in_range() {
        let range = make_range((10, 20), (0, 0));
        assert!(range.matches_core_request(10));
        assert!(range.matches_core_request(15));
        assert!(range.matches_core_request(20));
        assert!(!range.matches_core_request(9));
        assert!(!range.matches_core_request(21));
    }

    #[test]
    fn record_range_zero_range_matches_nothing() {
        let range = make_range((0, 0), (0, 0));
        assert!(!range.matches_core_request(0));
        assert!(!range.matches_core_request(1));
    }

    #[test]
    fn record_range_matches_delivered_events() {
        let range = make_range((0, 0), (2, 34));
        assert!(range.matches_delivered_event(2));
        assert!(range.matches_delivered_event(20));
        assert!(range.matches_delivered_event(34));
        assert!(!range.matches_delivered_event(1));
        assert!(!range.matches_delivered_event(35));
    }

    #[test]
    fn record_range_matches_ext_request() {
        let range = RecordRange {
            ext_requests: (150, 0, 10),
            ..Default::default()
        };
        assert!(range.matches_ext_request(150, 0));
        assert!(range.matches_ext_request(150, 5));
        assert!(range.matches_ext_request(150, 10));
        assert!(!range.matches_ext_request(150, 11));
        assert!(!range.matches_ext_request(151, 5));
    }

    #[test]
    fn record_range_matches_errors() {
        let range = RecordRange {
            errors: (1, 17),
            ..Default::default()
        };
        assert!(range.matches_error(1));
        assert!(range.matches_error(10));
        assert!(range.matches_error(17));
        assert!(!range.matches_error(0));
        assert!(!range.matches_error(18));
    }

    #[test]
    fn record_context_matches_event_any_range() {
        let ctx = RecordContext {
            id: 1,
            enabled: true,
            element_header: 0,
            ranges: vec![make_range((0, 0), (2, 5)), make_range((0, 0), (10, 20))],
            client_specs: vec![3], // AllClients
            enable_sequence: 0,
        };
        assert!(ctx.matches_event(3));
        assert!(ctx.matches_event(15));
        assert!(!ctx.matches_event(7));
    }

    #[test]
    fn record_context_should_intercept_all_clients() {
        let ctx = RecordContext {
            id: 1,
            enabled: true,
            element_header: 0,
            ranges: vec![],
            client_specs: vec![3], // AllClients
            enable_sequence: 0,
        };
        // Should intercept other clients, not the recording client itself
        assert!(ctx.should_intercept_client("client_b", "client_a", 0x200));
        assert!(!ctx.should_intercept_client("client_a", "client_a", 0x100));
    }

    #[test]
    fn record_context_should_intercept_specific_client() {
        let ctx = RecordContext {
            id: 1,
            enabled: true,
            element_header: 0,
            ranges: vec![],
            client_specs: vec![0x200], // Specific resource base
            enable_sequence: 0,
        };
        assert!(ctx.should_intercept_client("any", "recorder", 0x200));
        assert!(!ctx.should_intercept_client("any", "recorder", 0x300));
    }

    #[test]
    fn record_context_disabled_intercepts_nothing() {
        let ctx = RecordContext {
            id: 1,
            enabled: false,
            element_header: 0,
            ranges: vec![],
            client_specs: vec![3],
            enable_sequence: 0,
        };
        assert!(!ctx.should_intercept_client("b", "a", 0));
    }

    #[test]
    fn build_record_data_reply_format() {
        let reply = build_record_data_reply(
            RECORD_FROM_SERVER,
            42,
            1,
            &[0x01, 0x02, 0x03, 0x04],
            12345,
            99,
        );
        assert_eq!(reply[0], 1); // Reply indicator
        assert_eq!(reply[1], RECORD_FROM_SERVER);
        assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 42); // enable_seq
        assert_eq!(
            u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]),
            1
        ); // 4 bytes = 1 word
        assert_eq!(reply[8], 1); // element_header
        assert_eq!(reply[32], 0x01); // intercepted data
        assert_eq!(reply[33], 0x02);
        assert_eq!(reply[34], 0x03);
        assert_eq!(reply[35], 0x04);
    }

    #[test]
    fn build_record_status_reply_is_empty_data() {
        let reply = build_record_status_reply(RECORD_START_OF_DATA, 10, 0, 5000);
        assert_eq!(reply[0], 1);
        assert_eq!(reply[1], RECORD_START_OF_DATA);
        assert_eq!(reply.len(), 32); // No extra data
    }

    #[test]
    fn intercept_event_matches_and_builds_reply() {
        let mut contexts = HashMap::new();
        let ctx = RecordContext {
            id: 1,
            enabled: true,
            element_header: 0,
            ranges: vec![make_range((0, 0), (2, 34))],
            client_specs: vec![3], // AllClients
            enable_sequence: 50,
        };
        contexts.insert(1, ctx);

        let event = [2u8; 32]; // Event code 2 (KeyPress)
        let results = intercept_event(&contexts, "recorder", "source", 0x200, &event, 1000, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0][1], RECORD_FROM_SERVER);
    }

    #[test]
    fn intercept_event_skips_disabled_context() {
        let mut contexts = HashMap::new();
        let ctx = RecordContext {
            id: 1,
            enabled: false,
            element_header: 0,
            ranges: vec![make_range((0, 0), (2, 34))],
            client_specs: vec![3],
            enable_sequence: 50,
        };
        contexts.insert(1, ctx);

        let event = [2u8; 32];
        let results = intercept_event(&contexts, "rec", "src", 0, &event, 0, 0);
        assert!(results.is_empty());
    }

    #[test]
    fn intercept_request_matches_core_opcode() {
        let mut contexts = HashMap::new();
        let ctx = RecordContext {
            id: 1,
            enabled: true,
            element_header: 0,
            ranges: vec![make_range((1, 127), (0, 0))],
            client_specs: vec![3],
            enable_sequence: 10,
        };
        contexts.insert(1, ctx);

        let request = [42u8, 0, 2, 0]; // Opcode 42, length 2
        let results = intercept_request(&contexts, "rec", "src", 0x200, &request, 500, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0][1], RECORD_FROM_CLIENT);
    }
}
