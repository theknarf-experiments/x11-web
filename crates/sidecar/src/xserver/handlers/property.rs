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
    selection: u32,
    target: u32,
    data: Vec<u8>,
) -> bool {
    const INCR_ATOM: u32 = 138;
    let total_size = data.len() as u32;

    // Set the INCR property with the total size estimate (CARDINAL/32).
    let msb = state.msb_first;
    if let Some(win) = state.windows.get_mut(&requestor) {
        let mut size_data = [0u8; 4];
        write_u32_bo(&mut size_data, 0, total_size, msb);
        win.properties.insert(property, PropertyValue {
            prop_type: INCR_ATOM,
            format: 32,
            data: size_data.to_vec(),
        });
    }

    // Generate PropertyNotify(NewValue) so the requestor is notified.
    {
        let mut event = [0u8; 32];
        event[0] = PROPERTY_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        state.write_u32(&mut event, 4, requestor);
        state.write_u32(&mut event, 8, property);
        state.write_u32(&mut event, 12, state.timestamp());
        event[16] = 0; // NewValue

        if let Some(win) = state.windows.get(&requestor) {
            if win.event_mask & PROPERTY_CHANGE_MASK != 0 {
                state.pending_events.push(event.to_vec());
            }
        }
        state.broadcast_event(requestor, PROPERTY_CHANGE_MASK, &event);
    }

    // Default chunk size: ~64KB (standard INCR chunk).
    let chunk_size = 65536;

    state.push_incr_transfer(IncrTransfer {
        requestor,
        property,
        selection,
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
    let idx = state.incr_transfers.iter().position(|t| {
        t.requestor == window && t.property == property
    });
    let Some(idx) = idx else { return };

    // Update last_activity timestamp for timeout tracking.
    state.incr_transfers[idx].last_activity = std::time::Instant::now();

    let transfer = &state.incr_transfers[idx];
    let remaining = transfer.data.len() - transfer.offset;

    if remaining == 0 {
        // All data sent — write zero-length property to signal completion.
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(property, PropertyValue {
                prop_type: transfer.target,
                format: 8,
                data: Vec::new(),
            });
        }
        state.incr_transfers.remove(idx);
    } else {
        // Write the next chunk.
        let chunk_size = remaining.min(state.incr_transfers[idx].chunk_size);
        let offset = state.incr_transfers[idx].offset;
        let chunk = state.incr_transfers[idx].data[offset..offset + chunk_size].to_vec();
        let target = state.incr_transfers[idx].target;
        state.incr_transfers[idx].offset += chunk_size;

        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(property, PropertyValue {
                prop_type: target,
                format: 8,
                data: chunk,
            });
        }
    }

    // Generate PropertyNotify(NewValue) so the requestor knows data is ready.
    // Deliver to all clients that selected PropertyChangeMask on this window.
    {
        let mut event = [0u8; 32];
        event[0] = PROPERTY_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        state.write_u32(&mut event, 4, window);
        state.write_u32(&mut event, 8, property);
        state.write_u32(&mut event, 12, state.timestamp());
        event[16] = 0; // NewValue

        if let Some(win) = state.windows.get(&window) {
            if win.event_mask & PROPERTY_CHANGE_MASK != 0 {
                state.pending_events.push(event.to_vec());
            }
        }

        // Broadcast to other connections that selected PropertyChangeMask
        state.broadcast_event(window, PROPERTY_CHANGE_MASK, &event);
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
    const TARGETS_ATOM: u32 = 135;
    const ATOM_ATOM: u32 = 4;
    const TIMESTAMP_ATOM: u32 = 137;
    const CARDINAL_ATOM: u32 = 6;

    let pc_lock = match state.persistent_clipboard.lock() {
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
    if target == TARGETS_ATOM {
        const STRING_ATOM: u32 = 31;
        const UTF8_STRING_ATOM: u32 = 133;
        const COMPOUND_TEXT_ATOM: u32 = 181;
        const TEXT_ATOM: u32 = 182;
        let text_atoms = [STRING_ATOM, UTF8_STRING_ATOM, COMPOUND_TEXT_ATOM, TEXT_ATOM];

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

        if !atoms.contains(&TARGETS_ATOM) {
            atoms.push(TARGETS_ATOM);
        }
        if !atoms.contains(&TIMESTAMP_ATOM) {
            atoms.push(TIMESTAMP_ATOM);
        }
        let mut data = Vec::with_capacity(atoms.len() * 4);
        for a in &atoms {
            data.extend_from_slice(&a.to_le_bytes());
        }
        drop(pc_lock); // release lock before mutating state

        if let Some(win) = state.windows.get_mut(&requestor) {
            win.properties.insert(property, PropertyValue {
                prop_type: ATOM_ATOM,
                format: 32,
                data,
            });
        }

        let mut event = [0u8; 32];
        event[0] = SELECTION_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        state.write_u32(&mut event, 4, state.timestamp());
        state.write_u32(&mut event, 8, requestor);
        state.write_u32(&mut event, 12, selection);
        state.write_u32(&mut event, 16, TARGETS_ATOM);
        state.write_u32(&mut event, 20, property);
        if !state.event_router.send_event(requestor, event.to_vec()) {
            state.pending_events.push(event.to_vec());
        }
        return true;
    }

    // Handle TIMESTAMP: return the time the persistent data was captured.
    if target == TIMESTAMP_ATOM {
        let ts = entry.timestamp;
        drop(pc_lock);

        let mut ts_data = [0u8; 4];
        state.write_u32(&mut ts_data, 0, ts);
        if let Some(win) = state.windows.get_mut(&requestor) {
            win.properties.insert(property, PropertyValue {
                prop_type: CARDINAL_ATOM,
                format: 32,
                data: ts_data.to_vec(),
            });
        }

        let mut event = [0u8; 32];
        event[0] = SELECTION_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        state.write_u32(&mut event, 4, state.timestamp());
        state.write_u32(&mut event, 8, requestor);
        state.write_u32(&mut event, 12, selection);
        state.write_u32(&mut event, 16, TIMESTAMP_ATOM);
        state.write_u32(&mut event, 20, property);
        if !state.event_router.send_event(requestor, event.to_vec()) {
            state.pending_events.push(event.to_vec());
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
            start_incr_transfer(state, requestor, property, selection, target, data);

            let mut event = [0u8; 32];
            event[0] = SELECTION_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, state.timestamp());
            state.write_u32(&mut event, 8, requestor);
            state.write_u32(&mut event, 12, selection);
            state.write_u32(&mut event, 16, target);
            state.write_u32(&mut event, 20, property);
            if !state.event_router.send_event(requestor, event.to_vec()) {
                state.pending_events.push(event.to_vec());
            }
        } else {
            // Small data: set property inline (normal transfer).
            if let Some(win) = state.windows.get_mut(&requestor) {
                win.properties.insert(property, PropertyValue {
                    prop_type,
                    format: 8,
                    data,
                });
            }

            let mut event = [0u8; 32];
            event[0] = SELECTION_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, state.timestamp());
            state.write_u32(&mut event, 8, requestor);
            state.write_u32(&mut event, 12, selection);
            state.write_u32(&mut event, 16, target);
            state.write_u32(&mut event, 20, property);
            if !state.event_router.send_event(requestor, event.to_vec()) {
                state.pending_events.push(event.to_vec());
            }
        }
        return true;
    }

    // Target not found directly — try automatic text format conversion.
    // Many apps request UTF8_STRING but clipboard may only have STRING, or vice versa.
    const STRING_ATOM: u32 = 31;
    const UTF8_STRING_ATOM: u32 = 133;
    const COMPOUND_TEXT_ATOM: u32 = 181;
    const TEXT_ATOM: u32 = 182;

    let text_targets = [STRING_ATOM, UTF8_STRING_ATOM, COMPOUND_TEXT_ATOM, TEXT_ATOM];
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
                    win.properties.insert(property, PropertyValue {
                        prop_type: target,
                        format: 8,
                        data,
                    });
                }

                let mut event = [0u8; 32];
                event[0] = SELECTION_NOTIFY_EVENT;
                state.write_u16(&mut event, 2, state.sequence);
                state.write_u32(&mut event, 4, state.timestamp());
                state.write_u32(&mut event, 8, requestor);
                state.write_u32(&mut event, 12, selection);
                state.write_u32(&mut event, 16, target);
                state.write_u32(&mut event, 20, property);
                if !state.event_router.send_event(requestor, event.to_vec()) {
                    state.pending_events.push(event.to_vec());
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
        KEY_PRESS_EVENT => KEY_PRESS_MASK,
        KEY_RELEASE_EVENT => KEY_RELEASE_MASK,
        BUTTON_PRESS_EVENT => BUTTON_PRESS_MASK,
        BUTTON_RELEASE_EVENT => BUTTON_RELEASE_MASK,
        MOTION_NOTIFY_EVENT => POINTER_MOTION_MASK,
        ENTER_NOTIFY_EVENT => ENTER_WINDOW_MASK,
        LEAVE_NOTIFY_EVENT => LEAVE_WINDOW_MASK,
        FOCUS_IN_EVENT | FOCUS_OUT_EVENT => FOCUS_CHANGE_MASK,
        KEYMAP_NOTIFY_EVENT => KEYMAP_STATE_MASK,
        EXPOSE_EVENT => EXPOSURE_MASK,
        VISIBILITY_NOTIFY_EVENT => VISIBILITY_CHANGE_MASK,
        CREATE_NOTIFY_EVENT | DESTROY_NOTIFY_EVENT | UNMAP_NOTIFY_EVENT
        | MAP_NOTIFY_EVENT | REPARENT_NOTIFY_EVENT | CONFIGURE_NOTIFY_EVENT
        | GRAVITY_NOTIFY_EVENT => STRUCTURE_NOTIFY_MASK,
        MAP_REQUEST_EVENT | CONFIGURE_REQUEST_EVENT | CIRCULATE_REQUEST_EVENT => {
            SUBSTRUCTURE_REDIRECT_MASK
        }
        RESIZE_REQUEST_EVENT => RESIZE_REDIRECT_MASK,
        CIRCULATE_NOTIFY_EVENT => SUBSTRUCTURE_NOTIFY_MASK,
        PROPERTY_NOTIFY_EVENT => PROPERTY_CHANGE_MASK,
        COLOURMAP_NOTIFY_EVENT => COLOURMAP_CHANGE_MASK,
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
        assert_eq!(event_type_to_mask(KEY_PRESS_EVENT), KEY_PRESS_MASK);
        assert_eq!(event_type_to_mask(KEY_RELEASE_EVENT), KEY_RELEASE_MASK);
        assert_eq!(event_type_to_mask(BUTTON_PRESS_EVENT), BUTTON_PRESS_MASK);
        assert_eq!(event_type_to_mask(BUTTON_RELEASE_EVENT), BUTTON_RELEASE_MASK);
        assert_eq!(event_type_to_mask(MOTION_NOTIFY_EVENT), POINTER_MOTION_MASK);
    }

    #[test]
    fn crossing_events_map_to_correct_masks() {
        assert_eq!(event_type_to_mask(ENTER_NOTIFY_EVENT), ENTER_WINDOW_MASK);
        assert_eq!(event_type_to_mask(LEAVE_NOTIFY_EVENT), LEAVE_WINDOW_MASK);
    }

    #[test]
    fn focus_events_share_mask() {
        assert_eq!(event_type_to_mask(FOCUS_IN_EVENT), FOCUS_CHANGE_MASK);
        assert_eq!(event_type_to_mask(FOCUS_OUT_EVENT), FOCUS_CHANGE_MASK);
    }

    #[test]
    fn keymap_notify_maps_to_keymap_state_mask() {
        assert_eq!(event_type_to_mask(KEYMAP_NOTIFY_EVENT), KEYMAP_STATE_MASK);
    }

    #[test]
    fn expose_maps_to_exposure_mask() {
        assert_eq!(event_type_to_mask(EXPOSE_EVENT), EXPOSURE_MASK);
    }

    #[test]
    fn visibility_notify_maps_to_visibility_change_mask() {
        assert_eq!(event_type_to_mask(VISIBILITY_NOTIFY_EVENT), VISIBILITY_CHANGE_MASK);
    }

    #[test]
    fn structure_events_share_structure_notify_mask() {
        let structure_events = [
            CREATE_NOTIFY_EVENT, DESTROY_NOTIFY_EVENT, UNMAP_NOTIFY_EVENT,
            MAP_NOTIFY_EVENT, REPARENT_NOTIFY_EVENT, CONFIGURE_NOTIFY_EVENT,
            GRAVITY_NOTIFY_EVENT,
        ];
        for ev in structure_events {
            assert_eq!(
                event_type_to_mask(ev), STRUCTURE_NOTIFY_MASK,
                "event type {ev} should map to STRUCTURE_NOTIFY_MASK"
            );
        }
    }

    #[test]
    fn redirect_events_map_to_substructure_redirect_mask() {
        let redirect_events = [
            MAP_REQUEST_EVENT, CONFIGURE_REQUEST_EVENT, CIRCULATE_REQUEST_EVENT,
        ];
        for ev in redirect_events {
            assert_eq!(
                event_type_to_mask(ev), SUBSTRUCTURE_REDIRECT_MASK,
                "event type {ev} should map to SUBSTRUCTURE_REDIRECT_MASK"
            );
        }
    }

    #[test]
    fn resize_request_maps_to_resize_redirect_mask() {
        assert_eq!(event_type_to_mask(RESIZE_REQUEST_EVENT), RESIZE_REDIRECT_MASK);
    }

    #[test]
    fn circulate_notify_maps_to_substructure_notify_mask() {
        assert_eq!(event_type_to_mask(CIRCULATE_NOTIFY_EVENT), SUBSTRUCTURE_NOTIFY_MASK);
    }

    #[test]
    fn property_notify_maps_to_property_change_mask() {
        assert_eq!(event_type_to_mask(PROPERTY_NOTIFY_EVENT), PROPERTY_CHANGE_MASK);
    }

    #[test]
    fn colourmap_notify_maps_to_colourmap_change_mask() {
        assert_eq!(event_type_to_mask(COLOURMAP_NOTIFY_EVENT), COLOURMAP_CHANGE_MASK);
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
