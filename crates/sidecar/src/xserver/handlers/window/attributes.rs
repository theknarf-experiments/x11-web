//! Window attributes and save-set handlers (opcodes 2, 3, 6).

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 2: ChangeWindowAttributes
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_window_attributes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 2);

    let wid = state.read_u32(data, 4);
    let value_mask = state.read_u32(data, 8);

    // Validate value-list length matches the bitmask
    let n_values = value_mask.count_ones() as usize;
    let required_len = 12 + n_values * 4;
    require_len!(data, required_len, state.sequence, 2);

    if !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, state.sequence, wid, 2, 0);
    }

    // Pre-validate enumerated attributes before mutating state
    let msb_first = state.msb_first;
    {
        let mut voff = 12;
        for bit in 0..15 {
            if value_mask & (1 << bit) != 0
                && voff + 4 <= data.len() {
                    let val = read_u32_bo(data, voff, msb_first);
                    match bit {
                        4 if val > 10 => return build_error(BAD_VALUE, state.sequence, val, 2, 0),
                        5 if val > 10 => return build_error(BAD_VALUE, state.sequence, val, 2, 0),
                        6 if val > 2 => return build_error(BAD_VALUE, state.sequence, val, 2, 0),
                        // bit 11 = event-mask: check SubstructureRedirect/ResizeRedirect
                        // mutual exclusion per X11 spec Section 12.3
                        11 => {
                            if let Some(_conflict) = state.event_broadcaster
                                .check_redirect_conflict(wid, val, &state.client_id)
                            {
                                return build_error(BAD_ACCESS, state.sequence, 0, 2, 0);
                            }
                        }
                        // bit 14 = cursor: validate cursor ID exists
                        14 if val != 0 => {
                            if !state.cursors.contains_key(&val) {
                                return build_error(BAD_CURSOR, state.sequence, val, 2, 0);
                            }
                        }
                        _ => {}
                    }
                    voff += 4;
                }
        }
    }

    let mut cursor_changed = false;
    let mut deferred_event_mask: Option<u32> = None;
    let mut deferred_colormap_notify: Option<(u32, u32)> = None; // (old_cmap, new_cmap)
    if let Some(win) = state.windows.get_mut(&wid) {
        let mut offset = 12;
        for bit in 0..15 {
            if value_mask & (1 << bit) != 0
                && offset + 4 <= data.len() {
                    let val = read_u32_bo(data, offset, msb_first);
                    match bit {
                        0 => {
                            // background-pixmap: 0=None, 1=ParentRelative, else pixmap ID
                            win.background_pixmap = Some(val);
                        }
                        1 => win.background_pixel = val,
                        2 => {
                            // border-pixmap: 0=CopyFromParent, else pixmap ID
                            win.border_pixmap = Some(val);
                        }
                        3 => win.border_pixel = val,
                        4 => { state.bit_gravity.insert(wid, val as u8); }
                        5 => { state.win_gravity.insert(wid, val as u8); }
                        6 => win.backing_store = val as u8,
                        7 => win.backing_planes = val,
                        8 => win.backing_pixel = val,
                        9 => win.override_redirect = val != 0,
                        10 => win.save_under = val != 0,
                        11 => {
                            win.event_mask = val;
                            // SubstructureRedirectMask = bit 20 = 0x0010_0000
                            const SUBSTRUCTURE_REDIRECT_MASK: u32 = 0x0010_0000;
                            if wid == state.root_window && (val & SUBSTRUCTURE_REDIRECT_MASK) != 0 {
                                info!(
                                    "Client {} registering as window manager (SubstructureRedirectMask on root)",
                                    state.client_id
                                );
                                if let Ok(mut wm) = state.wm_state.lock() {
                                    wm.client_id = Some(state.client_id.clone());
                                    wm.event_tx = Some(state.wm_events_tx.clone());
                                }
                            }
                            // Defer cross-connection subscription until after mutable borrow ends
                            deferred_event_mask = Some(val);
                        }
                        12 => win.do_not_propagate_mask = val,
                        13 => {
                            // Colormap: 0 = CopyFromParent
                            let old_cmap = win.colormap;
                            win.colormap = val;
                            if val != old_cmap && (win.event_mask & COLOURMAP_CHANGE_MASK != 0) {
                                deferred_colormap_notify = Some((old_cmap, val));
                            }
                        }
                        14 => {
                            let new_cursor = if val == 0 { None } else { Some(val) };
                            if win.cursor != new_cursor {
                                win.cursor = new_cursor;
                                cursor_changed = true;
                            }
                        }
                        _ => {}
                    }
                    offset += 4;
                }
        }
    }

    // Register cross-connection event subscription (deferred from inside mutable borrow)
    if let Some(mask) = deferred_event_mask {
        state.subscribe_to_window_events(wid, mask);
    }

    // Generate ColormapNotify when the window's colormap attribute changes
    if let Some((_old_cmap, new_cmap)) = deferred_colormap_notify {
        let mut event = [0u8; 32];
        event[0] = COLOURMAP_NOTIFY_EVENT;
        state.write_u32(&mut event, 4, wid);
        state.write_u32(&mut event, 8, new_cmap);
        event[12] = 1; // new = true
        event[13] = 1; // state = Installed
        state.pending_events.push(event.to_vec());
        state.broadcast_event(wid, COLOURMAP_CHANGE_MASK, &event);
    }

    if cursor_changed {
        emit_cursor_changed(state, wid);
    }

    // If border attributes changed, send a WindowConfigured update so the
    // frontend can re-render the border.
    if value_mask & ((1 << 2) | (1 << 3)) != 0 {
        if let Some(win) = state.windows.get(&wid) {
            if let Some(uuid) = state.window_uuid(wid) {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowConfigured {
                        window_id: uuid,
                        x: win.x,
                        y: win.y,
                        width: win.width,
                        height: win.height,
                        border_width: win.border_width,
                        border_pixel: win.border_pixel,
                    },
                ));
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 3: GetWindowAttributes
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_window_attributes(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 3);
    let wid = state.read_u32(data, 4);

    let win = match state.windows.get(&wid) {
        Some(w) => w,
        None => return build_error(BAD_WINDOW, seq, wid, 3, 0),
    };

    let mut reply = vec![0u8; 44];
    reply[0] = 1; // Reply
    reply[1] = win.backing_store;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, 3u32); // length = 3 extra u32s
    state.write_u32(&mut reply, 8, win.visual);     // visual (4 bytes)
    state.write_u16(&mut reply, 12, win.class);     // class (2 bytes)
    reply[14] = state.bit_gravity.get(&wid).copied().unwrap_or(0); // bit_gravity
    reply[15] = state.win_gravity.get(&wid).copied().unwrap_or(1); // win_gravity
    state.write_u32(&mut reply, 16, win.backing_planes); // backing_planes
    state.write_u32(&mut reply, 20, win.backing_pixel); // backing_pixel
    reply[24] = if win.save_under { 1 } else { 0 };
    reply[25] = 1; // map_is_installed = true
    reply[26] = if win.mapped { 2 } else { 0 }; // map_state: Viewable or Unmapped
    reply[27] = if win.override_redirect { 1 } else { 0 };
    let cmap = if win.colormap != 0 { win.colormap } else { ROOT_COLORMAP };
    state.write_u32(&mut reply, 28, cmap); // colormap
    // all_event_masks: union of all clients' event masks (own + remote)
    let remote_masks = state.event_broadcaster.all_event_masks(wid);
    state.write_u32(&mut reply, 32, win.event_mask | remote_masks);
    // your_event_mask: this client's own event mask
    state.write_u32(&mut reply, 36, win.event_mask);
    state.write_u16(&mut reply, 40, win.do_not_propagate_mask as u16); // do_not_propagate_mask
    // bytes 42-43: unused padding

    reply
}

// ---------------------------------------------------------------------------
// Opcode 6: ChangeSaveSet
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_save_set(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 6);
    let mode = data[1]; // 0 = Insert, 1 = Delete
    let window = state.read_u32(data, 4);

    // Per X11 spec, validate the window exists (cannot add root window to save set)
    if !state.windows.contains_key(&window) || window == state.root_window {
        return build_error(BAD_WINDOW, state.sequence, window, 6, 0);
    }

    // Per X11 spec, mode must be 0 (Insert) or 1 (Delete)
    if mode > 1 {
        return build_error(BAD_VALUE, state.sequence, mode as u32, 6, 0);
    }

    match mode {
        0 => {
            // Insert
            if !state.save_set.contains(&window) {
                state.save_set.push(window);
            }
        }
        1 => {
            // Delete
            state.save_set.retain(|&w| w != window);
        }
        _ => {}
    }
    Vec::new()
}
