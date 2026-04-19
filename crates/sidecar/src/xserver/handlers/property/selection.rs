//! Selection operations — SetSelectionOwner (22), GetSelectionOwner (23),
//! ConvertSelection (24).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::event::serialize_event;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::xproto::{
    ConvertSelectionRequest, GetSelectionOwnerRequest, SelectionClearEvent, SelectionNotifyEvent,
    SelectionRequestEvent, SetSelectionOwnerRequest,
};

// ---------------------------------------------------------------------------
// Opcode 22: SetSelectionOwner
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_selection_owner(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 22);
    {
        let req = match SetSelectionOwnerRequest::try_parse_request(request_header(data), &data[4..]) {
            Ok(r) => r,
            Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 22, 0),
        };
        let owner = req.owner;
        let selection = req.selection;

        // Validate selection atom
        if selection != 0 && state.get_atom_name(selection).is_none() {
            return build_error(ATOM_ERROR, state.sequence, selection, 22, 0);
        }

        // Check if there was a previous owner (local or cross-connection).
        let prev_owner_local = state.selections.get(&selection).copied();
        let prev_owner_remote = if prev_owner_local.is_none() {
            state
                .shared_selections
                .lock()
                .ok()
                .and_then(|sels| sels.get(&selection).map(|e| e.owner))
        } else {
            None
        };
        let prev_owner = prev_owner_local.or(prev_owner_remote).unwrap_or(0);

        // If there was a previous owner that differs from the new one, send
        // SelectionClear to the old owner (ICCCM requirement).
        // Skip sending SelectionClear to the clipboard manager window since
        // it's a server-internal pseudo-window with no real client.
        if prev_owner != 0 && prev_owner != owner && prev_owner != CLIPBOARD_MANAGER_WINDOW {
            let event = serialize_event(&SelectionClearEvent {
                response_type: SELECTION_CLEAR_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                owner: prev_owner,
                selection,
            }, state.msb_first);
            // Deliver to the previous owner — may be on another connection.
            if state.x11_to_uuid.contains_key(&prev_owner) {
                state.pending_events.push(event);
            } else {
                state.event_router.send_event(prev_owner, event);
            }
        }

        // A real client is taking ownership — clear any persistent clipboard
        // data for this selection since it's now superseded.
        if owner != 0 {
            if let Ok(mut pc) = state.persistent_clipboard.lock() {
                pc.remove(&selection);
            }
        }

        let timestamp = state.timestamp();

        if owner == 0 {
            state.selections.remove(&selection);
            state.selection_timestamps.remove(&selection);
            if let Ok(mut sels) = state.shared_selections.lock() {
                sels.remove(&selection);
            }
        } else {
            state.selections.insert(selection, owner);
            state.selection_timestamps.insert(selection, timestamp);
            // Register in shared selections so other connections can find us.
            if let Ok(mut sels) = state.shared_selections.lock() {
                sels.insert(
                    selection,
                    SelectionEntry {
                        owner,
                        event_tx: state.wm_events_tx.clone(),
                        timestamp,
                    },
                );
            }
        }

        // XFIXES: emit XFixesSelectionNotify (event base 87 + 0) to subscribers.
        // This is required for GTK/Qt clipboard monitoring (e.g., clipboard managers).
        if let Some(&event_mask) = state.selection_event_subscribers.get(&selection) {
            // Bit 0 = SetSelectionOwnerNotifyMask
            if event_mask & 1 != 0 {
                // Find the subscribing window (we stored it as the key in the subscriber)
                // Actually, selection_event_subscribers maps selection_atom -> event_mask.
                // The subscribing window was passed in SelectSelectionInput but we only
                // stored the mask. We need to deliver to all subscribers. For now, deliver
                // as a pending event (the subscribing client is this connection).
                // XFixesSelectionNotify is an extension-specific event (XFIXES) —
                // no x11rb struct available, keep as raw bytes.
                const XFIXES_SELECTION_NOTIFY: u8 = 87; // first_event + 0
                let mut event = [0u8; 32];
                event[0] = XFIXES_SELECTION_NOTIFY;
                event[1] = 0; // subtype: SetSelectionOwner
                state.write_u16(&mut event, 2, state.sequence);
                state.write_u32(&mut event, 4, state.root_window); // window (subscriber)
                state.write_u32(&mut event, 8, owner); // new owner
                state.write_u32(&mut event, 12, selection); // selection atom
                state.write_u32(&mut event, 16, timestamp); // timestamp
                state.write_u32(&mut event, 20, timestamp); // selection_timestamp
                state.pending_events.push(event.to_vec());
            }
        }

        // Notify the clipboard bridge about ownership changes so the backend
        // can offer clipboard data to the frontend.
        if let Some(ref clipboard_tx) = state.clipboard_notify_tx {
            let selection_name = state.get_atom_name(selection).unwrap_or_default();
            let _ = clipboard_tx.send(super::super::super::types::ClipboardEvent::OwnerChanged {
                selection: selection_name,
                owner,
            });
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 23: GetSelectionOwner
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_selection_owner(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 8, seq, 23);
    let req = match GetSelectionOwnerRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 23, 0),
    };
    let selection = req.selection;
    let mut reply_buf = ReplyBuf::fixed(seq, state.msb_first);
    // Check local selections first, then shared (cross-connection).
    let owner = state
        .selections
        .get(&selection)
        .copied()
        .or_else(|| {
            state
                .shared_selections
                .lock()
                .ok()
                .and_then(|sels| sels.get(&selection).map(|e| e.owner))
        })
        .unwrap_or(0);
    reply_buf = reply_buf.set_u32(8, owner);
    reply_buf.build()
}

// ---------------------------------------------------------------------------
// Opcode 24: ConvertSelection
// ---------------------------------------------------------------------------

pub(crate) fn handle_convert_selection(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    require_len!(data, 24, _seq, 24);
    {
        let req = match ConvertSelectionRequest::try_parse_request(request_header(data), &data[4..]) {
            Ok(r) => r,
            Err(_) => return build_error(LENGTH_ERROR, _seq, 0, 24, 0),
        };
        let requestor = req.requestor;
        let selection = req.selection;
        let target = req.target;
        let property = req.property;

        // Validate selection and target atoms
        if selection != 0 && state.get_atom_name(selection).is_none() {
            return build_error(ATOM_ERROR, _seq, selection, 24, 0);
        }
        if target != 0 && state.get_atom_name(target).is_none() {
            return build_error(ATOM_ERROR, _seq, target, 24, 0);
        }

        // Use property = target if property is None (per ICCCM convention).
        let effective_property = if property == 0 { target } else { property };

        // --- DELETE target: owner should delete the selection data (ICCCM §2.6.3.1) ---
        const DELETE_ATOM: u32 = 190;
        if target == DELETE_ATOM {
            // If we're the owner, remove the selection.
            state.selections.remove(&selection);
            state.selection_timestamps.remove(&selection);
            if let Ok(mut sels) = state.shared_selections.lock() {
                sels.remove(&selection);
            }
            if let Ok(mut pc) = state.persistent_clipboard.lock() {
                pc.remove(&selection);
            }

            // Send SelectionNotify with property set (success).
            let event = serialize_event(&SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                requestor,
                selection,
                target,
                property: effective_property,
            }, state.msb_first);
            if !state.event_router.send_event(requestor, event.clone()) {
                state.pending_events.push(event);
            }
            return Vec::new();
        }

        // --- TIMESTAMP target: respond with the selection acquisition timestamp ---
        const TIMESTAMP_ATOM: u32 = 137;
        const CARDINAL_ATOM: u32 = 6;
        if target == TIMESTAMP_ATOM {
            // Look up the timestamp when this selection was acquired.
            let sel_ts = state
                .selection_timestamps
                .get(&selection)
                .copied()
                .or_else(|| {
                    state
                        .shared_selections
                        .lock()
                        .ok()
                        .and_then(|sels| sels.get(&selection).map(|e| e.timestamp))
                });

            let reply_property = if let Some(ts) = sel_ts {
                // Set the property on the requestor window with the timestamp.
                let mut ts_data = [0u8; 4];
                state.write_u32(&mut ts_data, 0, ts);
                if let Some(win) = state.windows.get_mut(&requestor) {
                    win.properties.insert(
                        effective_property,
                        PropertyValue {
                            prop_type: CARDINAL_ATOM,
                            format: 32,
                            data: ts_data.to_vec(),
                        },
                    );
                }
                effective_property
            } else {
                0 // None — conversion failed
            };

            let event = serialize_event(&SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                requestor,
                selection,
                target,
                property: reply_property,
            }, state.msb_first);
            if !state.event_router.send_event(requestor, event.clone()) {
                state.pending_events.push(event);
            }
            return Vec::new();
        }

        // --- MULTIPLE target: convert multiple targets at once per ICCCM ---
        const MULTIPLE_ATOM: u32 = 136;
        const ATOM_ATOM: u32 = 4;
        if target == MULTIPLE_ATOM && effective_property != 0 {
            // Read the ATOM_PAIR list from the requestor's property.
            // Format is pairs of (target, property) atoms, each u32.
            let pairs: Vec<(u32, u32)> = state
                .windows
                .get(&requestor)
                .and_then(|w| w.properties.get(&effective_property))
                .filter(|pv| pv.format == 32)
                .map(|pv| {
                    pv.data
                        .chunks_exact(8)
                        .map(|chunk| {
                            let t = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                            let p = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                            (t, p)
                        })
                        .collect()
                })
                .unwrap_or_default();

            if pairs.is_empty() {
                // No pairs found — send failure notification.
                let event = serialize_event(&SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: state.sequence,
                    time: state.timestamp(),
                    requestor,
                    selection,
                    target,
                    property: 0, // None
                }, state.msb_first);
                if !state.event_router.send_event(requestor, event.clone()) {
                    state.pending_events.push(event);
                }
                return Vec::new();
            }

            // For each (target, property) pair, synthesize a ConvertSelection.
            // We handle TIMESTAMP inline; others get forwarded to the selection owner.
            // Track which pairs failed so we can replace them with None.
            let mut result_pairs: Vec<(u32, u32)> = Vec::with_capacity(pairs.len());
            let owner_local = state.selections.get(&selection).copied();
            let remote_entry = if owner_local.is_none() {
                state.shared_selections.lock().ok().and_then(|sels| {
                    sels.get(&selection)
                        .filter(|e| e.owner != CLIPBOARD_MANAGER_WINDOW)
                        .map(|e| (e.owner, e.event_tx.clone(), e.timestamp))
                })
            } else {
                None
            };
            // Check if the server's clipboard manager owns this selection.
            let server_owned_multi = owner_local.is_none()
                && remote_entry.is_none()
                && state
                    .shared_selections
                    .lock()
                    .ok()
                    .map(|sels| {
                        sels.get(&selection)
                            .map(|e| e.owner == CLIPBOARD_MANAGER_WINDOW)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);

            for (pair_target, pair_property) in &pairs {
                let pt = *pair_target;
                let pp = *pair_property;

                if pt == TIMESTAMP_ATOM {
                    // Handle TIMESTAMP inline.
                    let sel_ts = state
                        .selection_timestamps
                        .get(&selection)
                        .copied()
                        .or_else(|| remote_entry.as_ref().map(|(_, _, ts)| *ts))
                        .or_else(|| {
                            if server_owned_multi {
                                state
                                    .persistent_clipboard
                                    .lock()
                                    .ok()
                                    .and_then(|pc| pc.get(&selection).map(|e| e.timestamp))
                            } else {
                                None
                            }
                        });
                    if let Some(ts) = sel_ts {
                        let mut ts_data = [0u8; 4];
                        state.write_u32(&mut ts_data, 0, ts);
                        if let Some(win) = state.windows.get_mut(&requestor) {
                            win.properties.insert(
                                pp,
                                PropertyValue {
                                    prop_type: CARDINAL_ATOM,
                                    format: 32,
                                    data: ts_data.to_vec(),
                                },
                            );
                        }
                        result_pairs.push((pt, pp));
                    } else {
                        // Failed — replace property with None
                        result_pairs.push((pt, 0));
                    }
                } else if server_owned_multi {
                    // Serve from persistent clipboard inline.
                    if serve_persistent_clipboard(state, selection, pt, pp, requestor) {
                        result_pairs.push((pt, pp));
                    } else {
                        result_pairs.push((pt, 0));
                    }
                } else if let Some(owner) = owner_local {
                    // Forward as individual SelectionRequest to local owner.
                    let sel_request = serialize_event(&SelectionRequestEvent {
                        response_type: SELECTION_REQUEST_EVENT,
                        sequence: state.sequence,
                        time: state.timestamp(),
                        owner,
                        requestor,
                        selection,
                        target: pt,
                        property: pp,
                    }, state.msb_first);
                    state.pending_events.push(sel_request);
                    result_pairs.push((pt, pp));
                } else if let Some((owner, ref event_tx, _)) = remote_entry {
                    // Forward to remote owner.
                    let sel_request = serialize_event(&SelectionRequestEvent {
                        response_type: SELECTION_REQUEST_EVENT,
                        sequence: state.sequence,
                        time: state.timestamp(),
                        owner,
                        requestor,
                        selection,
                        target: pt,
                        property: pp,
                    }, state.msb_first);
                    let _ = event_tx.send(sel_request);
                    result_pairs.push((pt, pp));
                } else {
                    // No owner — mark as failed.
                    result_pairs.push((pt, 0));
                }
            }

            // Update the MULTIPLE property with results (replacing failed conversions with None).
            let mut result_data = Vec::with_capacity(result_pairs.len() * 8);
            for (t, p) in &result_pairs {
                result_data.extend_from_slice(&t.to_le_bytes());
                result_data.extend_from_slice(&p.to_le_bytes());
            }
            if let Some(win) = state.windows.get_mut(&requestor) {
                win.properties.insert(
                    effective_property,
                    PropertyValue {
                        prop_type: ATOM_ATOM,
                        format: 32,
                        data: result_data,
                    },
                );
            }

            // Send SelectionNotify with target=MULTIPLE
            let event = serialize_event(&SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                requestor,
                selection,
                target: MULTIPLE_ATOM,
                property: effective_property,
            }, state.msb_first);
            if !state.event_router.send_event(requestor, event.clone()) {
                state.pending_events.push(event);
            }
            return Vec::new();
        }

        // --- Standard single-target conversion ---

        // Check if the server's clipboard manager owns this selection
        // (persistent clipboard after the original owner disconnected).
        let is_server_owned = state
            .shared_selections
            .lock()
            .ok()
            .map(|sels| {
                sels.get(&selection)
                    .map(|e| e.owner == CLIPBOARD_MANAGER_WINDOW)
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if is_server_owned {
            // Serve data directly from the persistent clipboard store.
            let served =
                serve_persistent_clipboard(state, selection, target, effective_property, requestor);
            if served {
                return Vec::new();
            }
            // Fall through to failure if we don't have the requested target.
        }

        // Check local selections first.
        if let Some(&owner) = state.selections.get(&selection) {
            let sel_request = serialize_event(&SelectionRequestEvent {
                response_type: SELECTION_REQUEST_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                owner,
                requestor,
                selection,
                target,
                property,
            }, state.msb_first);
            state.pending_events.push(sel_request);
        } else {
            // Check shared (cross-connection) selections.
            let remote_entry = state.shared_selections.lock().ok().and_then(|sels| {
                sels.get(&selection)
                    // Skip the clipboard manager window — already handled above.
                    .filter(|e| e.owner != CLIPBOARD_MANAGER_WINDOW)
                    .map(|e| (e.owner, e.event_tx.clone()))
            });

            if let Some((owner, event_tx)) = remote_entry {
                // Owner is on another connection — forward SelectionRequest
                // via the owner's event channel.
                let sel_request = serialize_event(&SelectionRequestEvent {
                    response_type: SELECTION_REQUEST_EVENT,
                    sequence: state.sequence,
                    time: state.timestamp(),
                    owner,
                    requestor,
                    selection,
                    target,
                    property,
                }, state.msb_first);
                let _ = event_tx.send(sel_request);
            } else {
                // No owner: send SelectionNotify with property=None to the
                // requestor to indicate conversion failed.
                let event = serialize_event(&SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: state.sequence,
                    time: state.timestamp(),
                    requestor,
                    selection,
                    target,
                    property: 0, // None
                }, state.msb_first);
                // Try cross-connection delivery first, fall back to local
                if !state.event_router.send_event(requestor, event.clone()) {
                    state.pending_events.push(event);
                }
            }
        }
    }
    Vec::new()
}
