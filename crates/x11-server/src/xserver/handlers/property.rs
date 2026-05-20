//! Property, atom, and selection handlers (opcodes 16-25).
//!
//! This module is split into submodules:
//! - `atom`      — InternAtom (16), GetAtomName (17)
//! - `crud`      — ChangeProperty (18), DeleteProperty (19), GetProperty (20), ListProperties (21)
//! - `selection` — SetSelectionOwner (22), GetSelectionOwner (23), ConvertSelection (24)
//! - `event`     — SendEvent (25)

mod atom;
mod crud;
mod event;
mod selection;

pub(super) use atom::*;
pub(super) use crud::*;
pub(super) use event::*;
pub(super) use selection::*;

// Re-export the parent scope into this module so submodules can use `use super::*;`
pub(super) use super::*;

use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{PropertyNotifyEvent, SelectionNotifyEvent};

// ---------------------------------------------------------------------------
// INCR transfer helpers (used by crud and selection submodules)
// ---------------------------------------------------------------------------

/// Begin an INCR (incremental) selection transfer per ICCCM spec.
///
/// Sets the property on the requestor window to type=INCR with the total
/// size estimate as CARDINAL/32 data, then registers the transfer for
/// chunk-by-chunk delivery via [`advance_incr_transfer`].
/// Sends PropertyNotify(NewValue) so the requestor knows to begin consuming.
///
/// The caller must send the SelectionNotify event with the INCR target to
/// the requestor after calling this function.
/// INCR threshold: data larger than this triggers incremental transfer.
/// The ICCCM suggests using the server's maximum request size; common
/// implementations use 64 KB.  We use 64 KB to match standard practice.
pub(crate) const INCR_THRESHOLD: usize = 65536;

pub(crate) fn start_incr_transfer(
    state: &mut ClientState,
    requestor: u32,
    property: u32,
    target: u32,
    data: Vec<u8>,
) -> bool {
    use crate::xserver::atoms::predef;
    let total_size = data.len() as u32;

    // Set the INCR property with the total size estimate (CARDINAL/32).
    if let Some(win) = state.windows.get_mut(&requestor) {
        win.properties.insert(
            property,
            PropertyValue {
                prop_type: predef::INCR,
                format: 32,
                data: total_size.to_le_bytes().to_vec(),
            },
        );
    }

    // Generate PropertyNotify(NewValue) so the requestor is notified.
    {
        let event = serialize_event(
            &PropertyNotifyEvent {
                response_type: PROPERTY_NOTIFY_EVENT,
                sequence: state.sequence,
                window: requestor,
                atom: property,
                time: state.timestamp(),
                state: 0u8.into(), // NewValue
            },
            state.msb_first,
        );

        state.deliver_event(requestor, EventMask::PROPERTY_CHANGE, &event);
    }

    // Default chunk size: ~64KB (standard INCR chunk).
    let chunk_size = INCR_THRESHOLD;

    state.push_incr_transfer(IncrTransfer {
        requestor,
        property,
        target,
        data,
        offset: 0,
        chunk_size,
        last_activity: std::time::Instant::now(),
    })
}

/// Advance an in-progress INCR (incremental) selection transfer.
///
/// Called when a property is deleted on the requestor window (i.e., the
/// requestor has consumed the previous chunk).  We write the next chunk
/// of data to the property and generate a PropertyNotify(NewValue).
/// When all data has been sent, we write a zero-length property to
/// signal completion.
pub(crate) fn advance_incr_transfer(state: &mut ClientState, window: u32, property: u32) {
    // Find the matching INCR transfer.
    let idx = state
        .selection
        .incr_transfers
        .iter()
        .position(|t| t.requestor == window && t.property == property);
    let Some(idx) = idx else { return };

    // Update last_activity timestamp for timeout tracking.
    state.selection.incr_transfers[idx].last_activity = std::time::Instant::now();

    let transfer = &state.selection.incr_transfers[idx];
    let remaining = transfer.data.len() - transfer.offset;

    if remaining == 0 {
        // All data sent — write zero-length property to signal completion.
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(
                property,
                PropertyValue {
                    prop_type: transfer.target,
                    format: 8,
                    data: Vec::new(),
                },
            );
        }
        state.selection.incr_transfers.remove(idx);
    } else {
        // Write the next chunk.
        let chunk_size = remaining.min(state.selection.incr_transfers[idx].chunk_size);
        let offset = state.selection.incr_transfers[idx].offset;
        let chunk = state.selection.incr_transfers[idx].data[offset..offset + chunk_size].to_vec();
        let target = state.selection.incr_transfers[idx].target;
        state.selection.incr_transfers[idx].offset += chunk_size;

        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(
                property,
                PropertyValue {
                    prop_type: target,
                    format: 8,
                    data: chunk,
                },
            );
        }
    }

    // Generate PropertyNotify(NewValue) so the requestor knows data is ready.
    // Deliver to all clients that selected PropertyChangeMask on this window.
    {
        let event = serialize_event(
            &PropertyNotifyEvent {
                response_type: PROPERTY_NOTIFY_EVENT,
                sequence: state.sequence,
                window,
                atom: property,
                time: state.timestamp(),
                state: 0u8.into(), // NewValue
            },
            state.msb_first,
        );

        state.deliver_event(window, EventMask::PROPERTY_CHANGE, &event);
    }
}

// ---------------------------------------------------------------------------
// Persistent clipboard helpers
// ---------------------------------------------------------------------------

/// Serve a ConvertSelection request from the server's persistent clipboard.
/// Returns `true` if the request was handled, `false` if the target is not
/// available in the persistent store (caller should fall through to failure).
pub(crate) fn serve_persistent_clipboard(
    state: &mut ClientState,
    selection: u32,
    target: u32,
    property: u32,
    requestor: u32,
) -> bool {
    use crate::xserver::atoms::predef;
    let pc_lock = match state.selection.persistent_clipboard.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    let entry = match pc_lock.get(&selection) {
        Some(e) => e,
        None => return false,
    };

    // Handle TARGETS: return the list of available target atoms plus
    // TARGETS and TIMESTAMP (standard practice).
    // Also advertise text format variants if any text format is stored.
    if target == predef::TARGETS {
        let text_atoms = [predef::STRING, predef::UTF8_STRING, predef::COMPOUND_TEXT, predef::TEXT];

        let mut atoms: Vec<u32> = entry.targets.keys().copied().collect();

        // If any text format is available, advertise all text formats
        let has_text = atoms.iter().any(|a| text_atoms.contains(a));
        if has_text {
            for &ta in &text_atoms {
                if !atoms.contains(&ta) {
                    atoms.push(ta);
                }
            }
        }

        if !atoms.contains(&predef::TARGETS) {
            atoms.push(predef::TARGETS);
        }
        if !atoms.contains(&predef::TIMESTAMP) {
            atoms.push(predef::TIMESTAMP);
        }
        let mut data = Vec::with_capacity(atoms.len() * 4);
        for a in &atoms {
            data.extend_from_slice(&a.to_le_bytes());
        }
        drop(pc_lock); // release lock before mutating state

        if let Some(win) = state.windows.get_mut(&requestor) {
            win.properties.insert(
                property,
                PropertyValue {
                    prop_type: predef::ATOM,
                    format: 32,
                    data,
                },
            );
        }

        let event = serialize_event(
            &SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                requestor,
                selection,
                target: predef::TARGETS,
                property,
            },
            state.msb_first,
        );
        if !state.event_router.send_event(requestor, event.clone()) {
            state.pending_events.push(event);
        }
        return true;
    }

    // Handle TIMESTAMP: return the time the persistent data was captured.
    if target == predef::TIMESTAMP {
        let ts = entry.timestamp;
        drop(pc_lock);

        let mut ts_data = [0u8; 4];
        state.write_u32(&mut ts_data, 0, ts);
        if let Some(win) = state.windows.get_mut(&requestor) {
            win.properties.insert(
                property,
                PropertyValue {
                    prop_type: predef::CARDINAL,
                    format: 32,
                    data: ts_data.to_vec(),
                },
            );
        }

        let event = serialize_event(
            &SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: state.sequence,
                time: state.timestamp(),
                requestor,
                selection,
                target: predef::TIMESTAMP,
                property,
            },
            state.msb_first,
        );
        if !state.event_router.send_event(requestor, event.clone()) {
            state.pending_events.push(event);
        }
        return true;
    }

    // Handle specific data targets (e.g., UTF8_STRING, STRING, etc.).
    if let Some(data) = entry.targets.get(&target) {
        let data = data.clone();
        drop(pc_lock);

        // The property type is the target atom itself (standard practice).
        let prop_type = target;

        if data.len() > INCR_THRESHOLD {
            // Large data: use INCR (incremental) transfer per ICCCM §2.7.2.
            // Set INCR property, register transfer, then send SelectionNotify
            // with the property so the requestor knows to begin consuming.
            start_incr_transfer(state, requestor, property, target, data);

            let event = serialize_event(
                &SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: state.sequence,
                    time: state.timestamp(),
                    requestor,
                    selection,
                    target,
                    property,
                },
                state.msb_first,
            );
            if !state.event_router.send_event(requestor, event.clone()) {
                state.pending_events.push(event);
            }
        } else {
            // Small data: set property inline (normal transfer).
            if let Some(win) = state.windows.get_mut(&requestor) {
                win.properties.insert(
                    property,
                    PropertyValue {
                        prop_type,
                        format: 8,
                        data,
                    },
                );
            }

            let event = serialize_event(
                &SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: state.sequence,
                    time: state.timestamp(),
                    requestor,
                    selection,
                    target,
                    property,
                },
                state.msb_first,
            );
            if !state.event_router.send_event(requestor, event.clone()) {
                state.pending_events.push(event);
            }
        }
        return true;
    }

    // Target not found directly — try automatic text format conversion.
    // Many apps request UTF8_STRING but clipboard may only have STRING, or vice versa.
    let text_targets = [predef::STRING, predef::UTF8_STRING, predef::COMPOUND_TEXT, predef::TEXT];
    if text_targets.contains(&target) {
        // Look for any available text format and convert
        for &alt in &text_targets {
            if let Some(data) = entry.targets.get(&alt) {
                let data = data.clone();
                drop(pc_lock);

                // For STRING <-> UTF8_STRING, the data is typically compatible
                // (ASCII subset). For COMPOUND_TEXT, we just pass through as-is
                // since modern apps generally handle UTF-8.
                if let Some(win) = state.windows.get_mut(&requestor) {
                    win.properties.insert(
                        property,
                        PropertyValue {
                            prop_type: target,
                            format: 8,
                            data,
                        },
                    );
                }

                let event = serialize_event(
                    &SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: state.sequence,
                        time: state.timestamp(),
                        requestor,
                        selection,
                        target,
                        property,
                    },
                    state.msb_first,
                );
                if !state.event_router.send_event(requestor, event.clone()) {
                    state.pending_events.push(event);
                }
                return true;
            }
        }
    }

    drop(pc_lock);
    // Target not available in persistent store.
    false
}

/// Map an X11 event type code to the corresponding event selection mask bit.
/// Returns 0 for event types that have no corresponding mask (e.g., ClientMessage,
/// SelectionNotify, MappingNotify).
pub(crate) fn event_type_to_mask(event_type: u8) -> u32 {
    match event_type {
        KEY_PRESS_EVENT => u32::from(EventMask::KEY_PRESS),
        KEY_RELEASE_EVENT => u32::from(EventMask::KEY_RELEASE),
        BUTTON_PRESS_EVENT => u32::from(EventMask::BUTTON_PRESS),
        BUTTON_RELEASE_EVENT => u32::from(EventMask::BUTTON_RELEASE),
        MOTION_NOTIFY_EVENT => u32::from(EventMask::POINTER_MOTION),
        ENTER_NOTIFY_EVENT => u32::from(EventMask::ENTER_WINDOW),
        LEAVE_NOTIFY_EVENT => u32::from(EventMask::LEAVE_WINDOW),
        FOCUS_IN_EVENT | FOCUS_OUT_EVENT => u32::from(EventMask::FOCUS_CHANGE),
        KEYMAP_NOTIFY_EVENT => u32::from(EventMask::KEYMAP_STATE),
        EXPOSE_EVENT => u32::from(EventMask::EXPOSURE),
        VISIBILITY_NOTIFY_EVENT => u32::from(EventMask::VISIBILITY_CHANGE),
        CREATE_NOTIFY_EVENT
        | DESTROY_NOTIFY_EVENT
        | UNMAP_NOTIFY_EVENT
        | MAP_NOTIFY_EVENT
        | REPARENT_NOTIFY_EVENT
        | CONFIGURE_NOTIFY_EVENT
        | GRAVITY_NOTIFY_EVENT => u32::from(EventMask::STRUCTURE_NOTIFY),
        MAP_REQUEST_EVENT | CONFIGURE_REQUEST_EVENT | CIRCULATE_REQUEST_EVENT => {
            u32::from(EventMask::SUBSTRUCTURE_REDIRECT)
        }
        RESIZE_REQUEST_EVENT => u32::from(EventMask::RESIZE_REDIRECT),
        CIRCULATE_NOTIFY_EVENT => u32::from(EventMask::SUBSTRUCTURE_NOTIFY),
        PROPERTY_NOTIFY_EVENT => u32::from(EventMask::PROPERTY_CHANGE),
        COLOURMAP_NOTIFY_EVENT => u32::from(EventMask::COLOR_MAP_CHANGE),
        _ => 0, // ClientMessage, Selection*, MappingNotify, etc.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // event_type_to_mask — correct mask for each event type
    // -----------------------------------------------------------------------

    #[test]
    fn device_events_map_to_correct_masks() {
        assert_eq!(
            event_type_to_mask(KEY_PRESS_EVENT),
            u32::from(EventMask::KEY_PRESS)
        );
        assert_eq!(
            event_type_to_mask(KEY_RELEASE_EVENT),
            u32::from(EventMask::KEY_RELEASE)
        );
        assert_eq!(
            event_type_to_mask(BUTTON_PRESS_EVENT),
            u32::from(EventMask::BUTTON_PRESS)
        );
        assert_eq!(
            event_type_to_mask(BUTTON_RELEASE_EVENT),
            u32::from(EventMask::BUTTON_RELEASE)
        );
        assert_eq!(
            event_type_to_mask(MOTION_NOTIFY_EVENT),
            u32::from(EventMask::POINTER_MOTION)
        );
    }

    #[test]
    fn crossing_events_map_to_correct_masks() {
        assert_eq!(
            event_type_to_mask(ENTER_NOTIFY_EVENT),
            u32::from(EventMask::ENTER_WINDOW)
        );
        assert_eq!(
            event_type_to_mask(LEAVE_NOTIFY_EVENT),
            u32::from(EventMask::LEAVE_WINDOW)
        );
    }

    #[test]
    fn focus_events_share_mask() {
        assert_eq!(
            event_type_to_mask(FOCUS_IN_EVENT),
            u32::from(EventMask::FOCUS_CHANGE)
        );
        assert_eq!(
            event_type_to_mask(FOCUS_OUT_EVENT),
            u32::from(EventMask::FOCUS_CHANGE)
        );
    }

    #[test]
    fn keymap_notify_maps_to_keymap_state_mask() {
        assert_eq!(
            event_type_to_mask(KEYMAP_NOTIFY_EVENT),
            u32::from(EventMask::KEYMAP_STATE)
        );
    }

    #[test]
    fn expose_maps_to_exposure_mask() {
        assert_eq!(
            event_type_to_mask(EXPOSE_EVENT),
            u32::from(EventMask::EXPOSURE)
        );
    }

    #[test]
    fn visibility_notify_maps_to_visibility_change_mask() {
        assert_eq!(
            event_type_to_mask(VISIBILITY_NOTIFY_EVENT),
            u32::from(EventMask::VISIBILITY_CHANGE)
        );
    }

    #[test]
    fn structure_events_share_structure_notify_mask() {
        let structure_events = [
            CREATE_NOTIFY_EVENT,
            DESTROY_NOTIFY_EVENT,
            UNMAP_NOTIFY_EVENT,
            MAP_NOTIFY_EVENT,
            REPARENT_NOTIFY_EVENT,
            CONFIGURE_NOTIFY_EVENT,
            GRAVITY_NOTIFY_EVENT,
        ];
        for ev in structure_events {
            assert_eq!(
                event_type_to_mask(ev),
                u32::from(EventMask::STRUCTURE_NOTIFY),
                "event type {ev} should map to STRUCTURE_NOTIFY_MASK"
            );
        }
    }

    #[test]
    fn redirect_events_map_to_substructure_redirect_mask() {
        let redirect_events = [
            MAP_REQUEST_EVENT,
            CONFIGURE_REQUEST_EVENT,
            CIRCULATE_REQUEST_EVENT,
        ];
        for ev in redirect_events {
            assert_eq!(
                event_type_to_mask(ev),
                u32::from(EventMask::SUBSTRUCTURE_REDIRECT),
                "event type {ev} should map to SUBSTRUCTURE_REDIRECT_MASK"
            );
        }
    }

    #[test]
    fn resize_request_maps_to_resize_redirect_mask() {
        assert_eq!(
            event_type_to_mask(RESIZE_REQUEST_EVENT),
            u32::from(EventMask::RESIZE_REDIRECT)
        );
    }

    #[test]
    fn circulate_notify_maps_to_substructure_notify_mask() {
        assert_eq!(
            event_type_to_mask(CIRCULATE_NOTIFY_EVENT),
            u32::from(EventMask::SUBSTRUCTURE_NOTIFY)
        );
    }

    #[test]
    fn property_notify_maps_to_property_change_mask() {
        assert_eq!(
            event_type_to_mask(PROPERTY_NOTIFY_EVENT),
            u32::from(EventMask::PROPERTY_CHANGE)
        );
    }

    #[test]
    fn colourmap_notify_maps_to_colourmap_change_mask() {
        assert_eq!(
            event_type_to_mask(COLOURMAP_NOTIFY_EVENT),
            u32::from(EventMask::COLOR_MAP_CHANGE)
        );
    }

    #[test]
    fn maskless_events_return_zero() {
        // These events have no corresponding event mask per X11 spec
        assert_eq!(event_type_to_mask(CLIENT_MESSAGE_EVENT), 0);
        assert_eq!(event_type_to_mask(SELECTION_CLEAR_EVENT), 0);
        assert_eq!(event_type_to_mask(SELECTION_REQUEST_EVENT), 0);
        assert_eq!(event_type_to_mask(SELECTION_NOTIFY_EVENT), 0);
        assert_eq!(event_type_to_mask(MAPPING_NOTIFY_EVENT), 0);
    }

    #[test]
    fn graphics_exposure_and_no_exposure_return_zero() {
        // GraphicsExposure/NoExposure are controlled by GC graphics_exposures, not event masks
        assert_eq!(event_type_to_mask(GRAPHICS_EXPOSURE_EVENT), 0);
        assert_eq!(event_type_to_mask(NO_EXPOSURE_EVENT), 0);
    }

    #[test]
    fn invalid_event_types_return_zero() {
        assert_eq!(event_type_to_mask(0), 0);
        assert_eq!(event_type_to_mask(1), 0);
        assert_eq!(event_type_to_mask(35), 0);
        assert_eq!(event_type_to_mask(255), 0);
    }
}
