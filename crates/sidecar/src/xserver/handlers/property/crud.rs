//! Property CRUD operations — ChangeProperty (18), DeleteProperty (19),
//! GetProperty (20), ListProperties (21).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

// ---------------------------------------------------------------------------
// Opcode 18: ChangeProperty
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_property(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 24, state.sequence, 18);

    let mode = data[1]; // 0=Replace, 1=Prepend, 2=Append
    let window = state.read_u32(data, 4);
    let property_atom = state.read_u32(data, 8);
    let prop_type = state.read_u32(data, 12);
    let format = data[16];
    let data_len = state.read_u32(data, 20) as usize;

    // Validate property and type atoms exist
    if property_atom != 0 && state.get_atom_name(property_atom).is_none() {
        return build_error(BAD_ATOM, state.sequence, property_atom, 18, 0);
    }
    if prop_type != 0 && state.get_atom_name(prop_type).is_none() {
        return build_error(BAD_ATOM, state.sequence, prop_type, 18, 0);
    }

    // Validate format is one of the legal values
    if !matches!(format, 8 | 16 | 32) {
        return build_error(BAD_VALUE, state.sequence, format as u32, 18, 0);
    }

    // Calculate actual byte length based on format
    let byte_len = match format {
        8 => data_len,
        16 => data_len * 2,
        32 => data_len * 4,
        _ => data_len,
    };

    // Validate that the declared data fits within the request
    require_len!(data, 24 + byte_len, state.sequence, 18);

    // Validate window exists
    if !state.windows.contains_key(&window) {
        return build_error(BAD_WINDOW, state.sequence, window, 18, 0);
    }

    // Store the property value, honoring Replace/Prepend/Append modes
    {
        let new_data = data[24..24 + byte_len].to_vec();
        if let Some(win) = state.windows.get_mut(&window) {
            match mode {
                1 => {
                    // Prepend: new data before existing data (must match type and format)
                    if let Some(existing) = win.properties.get_mut(&property_atom) {
                        if existing.prop_type == prop_type && existing.format == format {
                            let mut combined = new_data;
                            combined.extend_from_slice(&existing.data);
                            existing.data = combined;
                        } else {
                            // Type/format mismatch: replace per spec (BadMatch would be stricter)
                            win.properties.insert(
                                property_atom,
                                PropertyValue {
                                    prop_type,
                                    format,
                                    data: new_data,
                                },
                            );
                        }
                    } else {
                        win.properties.insert(
                            property_atom,
                            PropertyValue {
                                prop_type,
                                format,
                                data: new_data,
                            },
                        );
                    }
                }
                2 => {
                    // Append: existing data before new data (must match type and format)
                    if let Some(existing) = win.properties.get_mut(&property_atom) {
                        if existing.prop_type == prop_type && existing.format == format {
                            existing.data.extend_from_slice(&new_data);
                        } else {
                            win.properties.insert(
                                property_atom,
                                PropertyValue {
                                    prop_type,
                                    format,
                                    data: new_data,
                                },
                            );
                        }
                    } else {
                        win.properties.insert(
                            property_atom,
                            PropertyValue {
                                prop_type,
                                format,
                                data: new_data,
                            },
                        );
                    }
                }
                _ => {
                    // Replace (mode 0 or any other value)
                    win.properties.insert(
                        property_atom,
                        PropertyValue {
                            prop_type,
                            format,
                            data: new_data,
                        },
                    );
                }
            }
        }
    }

    // Sync property to shared window store for cross-client visibility.
    if let Some(win) = state.windows.get(&window) {
        if let Some(prop_val) = win.properties.get(&property_atom) {
            let pv = prop_val.clone();
            if let Ok(mut shared) = state.shared_windows.lock() {
                if let Some(sw) = shared.get_mut(&window) {
                    sw.properties.insert(property_atom, pv);
                }
            }
        }
    }

    // Generate PropertyNotify event if PropertyChangeMask is set
    let property_change_mask: u32 = 0x0040_0000;
    {
        let mut event = [0u8; 32];
        event[0] = PROPERTY_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        state.write_u32(&mut event, 4, window);
        state.write_u32(&mut event, 8, property_atom);
        state.write_u32(&mut event, 12, state.timestamp());
        event[16] = 0; // NewValue

        // Deliver to local client if it selected PropertyChangeMask
        if let Some(win) = state.windows.get(&window) {
            if win.event_mask & property_change_mask != 0 {
                state.pending_events.push(event.to_vec());
            }
        }

        // Broadcast to other connections that selected PropertyChangeMask
        state.broadcast_event(window, property_change_mask, &event);
    }

    // Check if this is WM_TRANSIENT_FOR — store transient parent in WindowState (ICCCM §4.1.2.6)
    let is_wm_transient_for = property_atom == 68 // WM_TRANSIENT_FOR predefined atom
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "WM_TRANSIENT_FOR")
            .unwrap_or(false);

    if is_wm_transient_for && format == 32 && byte_len >= 4 {
        let transient_data = &data[24..24 + byte_len.min(data.len() - 24)];
        if transient_data.len() >= 4 {
            let parent_wid = state.read_u32_from(transient_data, 0);
            if let Some(win) = state.windows.get_mut(&window) {
                win.transient_for = if parent_wid != 0 {
                    Some(parent_wid)
                } else {
                    None
                };
            }
        }
    }

    // Check if this is _NET_WM_SYNC_REQUEST_COUNTER — store counter ID for tear-free resize
    let is_sync_counter = state
        .get_atom_name(property_atom)
        .map(|n| n == "_NET_WM_SYNC_REQUEST_COUNTER")
        .unwrap_or(false);

    if is_sync_counter && format == 32 && byte_len >= 4 {
        let counter_data = &data[24..24 + byte_len.min(data.len() - 24)];
        if counter_data.len() >= 4 {
            let counter_id = state.read_u32_from(counter_data, 0);
            if let Some(win) = state.windows.get_mut(&window) {
                win.sync_request_counter = if counter_id != 0 {
                    Some(counter_id)
                } else {
                    None
                };
            }
        }
    }

    // Check if this is _NET_WM_WINDOW_TYPE — update window type for stacking layer enforcement
    let is_window_type = property_atom == 79 // _NET_WM_WINDOW_TYPE predefined atom
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "_NET_WM_WINDOW_TYPE")
            .unwrap_or(false);

    if is_window_type && format == 32 && byte_len >= 4 {
        let type_data = &data[24..24 + byte_len.min(data.len() - 24)];
        let mut atom_ids = Vec::new();
        for chunk in type_data.chunks_exact(4) {
            atom_ids.push(state.read_u32_from(chunk, 0));
        }
        let wtype = WindowType::from_atom_ids(&atom_ids);
        if let Some(win) = state.windows.get_mut(&window) {
            win.window_type = wtype;
        }

        // Enforce stacking layer: reposition in parent's children_order
        let parent_id = state.windows.get(&window).map(|w| w.parent);
        if let Some(parent_id) = parent_id {
            restack_by_window_type(state, window, parent_id);
        }
    }

    // Check if this is _NET_WM_STRUT or _NET_WM_STRUT_PARTIAL — update strut and recalculate workarea
    let is_strut = property_atom == 129
        || property_atom == 130
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "_NET_WM_STRUT" || n == "_NET_WM_STRUT_PARTIAL")
            .unwrap_or(false);

    if is_strut && format == 32 && byte_len >= 16 {
        let strut_data = &data[24..24 + byte_len.min(data.len() - 24)];
        if strut_data.len() >= 16 {
            let left = state.read_u32_from(strut_data, 0);
            let right = state.read_u32_from(strut_data, 4);
            let top = state.read_u32_from(strut_data, 8);
            let bottom = state.read_u32_from(strut_data, 12);
            if let Some(win) = state.windows.get_mut(&window) {
                win.strut = Some([left, right, top, bottom]);
            }
            state.recalculate_workarea();
        }
    }

    // Check if this is WM_NAME (atom 39) or _NET_WM_NAME
    let is_wm_name = property_atom == 39
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "_NET_WM_NAME" || n == "WM_NAME")
            .unwrap_or(false);

    if is_wm_name && format == 8 && data.len() >= 24 + byte_len {
        let title = String::from_utf8_lossy(&data[24..24 + byte_len]).to_string();
        if !title.is_empty() {
            if let Some(uuid) = state.window_uuid(window) {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::TitleChanged {
                        window_id: uuid,
                        title,
                    },
                ));
            }
        }
    }

    // Detect GTK application menu export.
    if format == 8 && data.len() >= 24 + byte_len {
        let atom_name = state.get_atom_name(property_atom);
        if let Some(name) = atom_name {
            let is_gtk_menu_atom = matches!(
                name.as_str(),
                "_GTK_UNIQUE_BUS_NAME"
                    | "_GTK_MENUBAR_OBJECT_PATH"
                    | "_GTK_APP_MENU_OBJECT_PATH"
                    | "_GTK_APPLICATION_OBJECT_PATH"
                    | "_GTK_WINDOW_OBJECT_PATH"
            );
            if is_gtk_menu_atom {
                let value = String::from_utf8_lossy(&data[24..24 + byte_len])
                    .trim_end_matches('\0')
                    .to_string();
                let entry = state.gtk_menu_paths.entry(window).or_default();
                match name.as_str() {
                    "_GTK_UNIQUE_BUS_NAME" => entry.bus_name = value,
                    "_GTK_MENUBAR_OBJECT_PATH" => entry.menubar_path = Some(value),
                    "_GTK_APP_MENU_OBJECT_PATH" => entry.app_menu_path = Some(value),
                    "_GTK_APPLICATION_OBJECT_PATH" => entry.app_actions_path = Some(value),
                    "_GTK_WINDOW_OBJECT_PATH" => entry.win_actions_path = Some(value),
                    _ => {}
                }
                if let Some(paths) = state.gtk_menu_paths.get(&window) {
                    if paths.has_menu() {
                        if let Some(uuid) = state.window_uuid(window) {
                            state.menu_tracker.attach_gtk(
                                uuid,
                                state.client_id.clone(),
                                paths.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    // Check if this is WM_HINTS (atom 35) — parse urgency and icon hints.
    let is_wm_hints = property_atom == 35
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "WM_HINTS")
            .unwrap_or(false);

    if is_wm_hints && format == 32 && byte_len >= 4 {
        let hint_data = &data[24..24 + byte_len.min(data.len() - 24)];
        if hint_data.len() >= 4 {
            let flags = state.read_u32_from(hint_data, 0);
            // Bit 8 = UrgencyHint
            let urgent = flags & (1 << 8) != 0;
            if let Some(uuid) = state.window_uuid(window) {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowUrgent {
                        window_id: uuid.clone(),
                        urgent,
                    },
                ));
            }

            // Bit 2 = IconPixmapHint — extract icon pixmap and send to frontend
            if flags & (1 << 2) != 0 && hint_data.len() >= 20 {
                let icon_pixmap_id = state.read_u32_from(hint_data, 12);
                if icon_pixmap_id != 0 {
                    // Try to read the icon pixmap's pixel data
                    if let Some(px) = state.pixmaps.get(&icon_pixmap_id) {
                        let w = px.width;
                        let h = px.height;
                        let pixels = px.framebuffer.extract_rgba();
                        if let Some(uuid) = state.window_uuid(window) {
                            let _ = state.update_tx.send((
                                state.client_id.clone(),
                                DisplayUpdate::WindowIconChanged {
                                    window_id: uuid,
                                    width: w,
                                    height: h,
                                    data: pixels,
                                },
                            ));
                        }
                    }
                }
            }

            // Bit 0 = InputHint — whether the window accepts keyboard focus (ICCCM §4.1.2.4).
            // WM_HINTS layout: flags(4), input(4), initial_state(4), icon_pixmap(4), ...
            if flags & (1 << 0) != 0 && hint_data.len() >= 8 {
                let input_val = state.read_u32_from(hint_data, 4);
                if let Some(win) = state.windows.get_mut(&window) {
                    win.wm_hints_input = Some(input_val != 0);
                }
            }

            // Store initial_state for MapWindow to check later
            if flags & (1 << 1) != 0 && hint_data.len() >= 12 {
                let initial_state = state.read_u32_from(hint_data, 8);
                if let Some(win) = state.windows.get_mut(&window) {
                    win.wm_hints_initial_state = Some(initial_state);
                }
            }

            // Bit 6 = WindowGroupHint — window_group leader (ICCCM §4.1.2.6).
            // WM_HINTS layout: flags(0), input(4), initial_state(8), icon_pixmap(12),
            //   icon_window(16), icon_x(20), icon_y(24), icon_mask(28), window_group(32)
            if flags & (1 << 6) != 0 && hint_data.len() >= 36 {
                let group_leader = state.read_u32_from(hint_data, 32);
                if let Some(win) = state.windows.get_mut(&window) {
                    win.wm_hints_window_group = if group_leader != 0 {
                        Some(group_leader)
                    } else {
                        None
                    };
                }
            }
        }
    }

    // Check if this is _NET_WM_ICON — extract ARGB icon data for frontend
    let is_net_wm_icon = state
        .get_atom_name(property_atom)
        .map(|n| n == "_NET_WM_ICON")
        .unwrap_or(false);

    if is_net_wm_icon && format == 32 && byte_len >= 8 {
        let icon_data = &data[24..24 + byte_len.min(data.len() - 24)];
        // _NET_WM_ICON format: width(CARD32) height(CARD32) ARGB_pixels...
        if icon_data.len() >= 8 {
            let w = state.read_u32_from(icon_data, 0);
            let h = state.read_u32_from(icon_data, 4);
            let pixel_count = (w as usize) * (h as usize);
            let expected = 8 + pixel_count * 4;
            if w > 0 && h > 0 && w <= 256 && h <= 256 && icon_data.len() >= expected {
                // Convert ARGB to RGBA
                let mut rgba = vec![0u8; pixel_count * 4];
                for i in 0..pixel_count {
                    let argb = state.read_u32_from(icon_data, 8 + i * 4);
                    let a = (argb >> 24) as u8;
                    let r = (argb >> 16) as u8;
                    let g = (argb >> 8) as u8;
                    let b = argb as u8;
                    rgba[i * 4] = r;
                    rgba[i * 4 + 1] = g;
                    rgba[i * 4 + 2] = b;
                    rgba[i * 4 + 3] = a;
                }
                if let Some(uuid) = state.window_uuid(window) {
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowIconChanged {
                            window_id: uuid,
                            width: w as u16,
                            height: h as u16,
                            data: rgba,
                        },
                    ));
                }
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 19: DeleteProperty
// ---------------------------------------------------------------------------

pub(crate) fn handle_delete_property(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 19);
    {
        let window = state.read_u32(data, 4);
        let window_exists = state.windows.contains_key(&window)
            || state
                .shared_windows
                .lock()
                .ok()
                .is_some_and(|s| s.contains_key(&window));
        if !window_exists {
            return build_error(BAD_WINDOW, state.sequence, window, 19, 0);
        }
        let property = state.read_u32(data, 8);
        // Validate property atom
        if property != 0 && state.get_atom_name(property).is_none() {
            return build_error(BAD_ATOM, state.sequence, property, 19, 0);
        }
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.remove(&property);
        }
        // Also remove from shared store
        if let Ok(mut shared) = state.shared_windows.lock() {
            if let Some(sw) = shared.get_mut(&window) {
                sw.properties.remove(&property);
            }
        }

        // Generate PropertyNotify event if PropertyChangeMask is set
        {
            let mut event = [0u8; 32];
            event[0] = PROPERTY_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, window);
            state.write_u32(&mut event, 8, property);
            state.write_u32(&mut event, 12, state.timestamp());
            event[16] = 1; // Deleted

            if let Some(win) = state.windows.get(&window) {
                if win.event_mask & PROPERTY_CHANGE_MASK != 0 {
                    state.pending_events.push(event.to_vec());
                }
            }

            // Broadcast to other connections that selected PropertyChangeMask
            state.broadcast_event(window, PROPERTY_CHANGE_MASK, &event);
        }

        // Advance any pending INCR transfer for this window+property.
        advance_incr_transfer(state, window, property);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 20: GetProperty
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_property(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 24, seq, 20);

    let delete = data[1] != 0;
    let window = state.read_u32(data, 4);
    let property_atom = state.read_u32(data, 8);
    let _req_type = state.read_u32(data, 12);
    let long_offset = state.read_u32(data, 16) as usize;
    let long_length = state.read_u32(data, 20) as usize;

    // Validate property atom
    if property_atom != 0 && state.get_atom_name(property_atom).is_none() {
        return build_error(BAD_ATOM, seq, property_atom, 20, 0);
    }

    // Validate window exists (local or shared)
    let window_exists = state.windows.contains_key(&window)
        || state
            .shared_windows
            .lock()
            .ok()
            .is_some_and(|s| s.contains_key(&window));
    if !window_exists {
        return build_error(BAD_WINDOW, seq, window, 20, 0);
    }

    // Check local copy first, then shared store for cross-client access.
    let prop = state
        .windows
        .get(&window)
        .and_then(|w| w.properties.get(&property_atom))
        .cloned()
        .or_else(|| {
            state.shared_windows.lock().ok().and_then(|shared| {
                shared
                    .get(&window)
                    .and_then(|w| w.properties.get(&property_atom).cloned())
            })
        });

    if let Some(prop_val) = prop {
        let byte_offset = long_offset * 4;
        let max_bytes = long_length * 4;
        let total_bytes = prop_val.data.len();
        let available = total_bytes.saturating_sub(byte_offset);
        let return_bytes = available.min(max_bytes);
        let bytes_after = available.saturating_sub(return_bytes);

        let return_data = if byte_offset < total_bytes {
            &prop_val.data[byte_offset..byte_offset + return_bytes]
        } else {
            &[]
        };

        // value_length is in units of format size
        let value_length = match prop_val.format {
            8 => return_data.len() as u32,
            16 => (return_data.len() / 2) as u32,
            32 => (return_data.len() / 4) as u32,
            _ => return_data.len() as u32,
        };

        let padded_len = (return_data.len() + 3) & !3;

        let mut reply = ReplyBuf::with_extra(seq, padded_len, state.msb_first)
            .set_data_byte(prop_val.format)
            .set_u32(8, prop_val.prop_type) // type
            .set_u32(12, bytes_after as u32) // bytes_after
            .set_u32(16, value_length); // value_length
        reply.buf_mut()[32..32 + return_data.len()].copy_from_slice(return_data);

        // Delete property if requested and we returned all of it
        if delete && bytes_after == 0 {
            if let Some(win) = state.windows.get_mut(&window) {
                win.properties.remove(&property_atom);
            }

            // Generate PropertyNotify(Deleted) per spec
            let property_change_mask: u32 = 0x0040_0000;
            if let Some(win) = state.windows.get(&window) {
                if win.event_mask & property_change_mask != 0 {
                    let mut event = [0u8; 32];
                    event[0] = PROPERTY_NOTIFY_EVENT;
                    state.write_u16(&mut event, 2, seq);
                    state.write_u32(&mut event, 4, window);
                    state.write_u32(&mut event, 8, property_atom);
                    state.write_u32(&mut event, 12, state.timestamp());
                    event[16] = 1; // PropertyDelete
                    state.pending_events.push(event.to_vec());
                }
            }

            // Advance INCR transfer if this was an incremental selection
            advance_incr_transfer(state, window, property_atom);
        }

        reply.build()
    } else {
        // Property not found
        ReplyBuf::fixed(seq, state.msb_first)
            // type = 0 (None), format = 0, bytes_after = 0, value_length = 0
            .build()
    }
}

// ---------------------------------------------------------------------------
// Opcode 21: ListProperties
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_properties(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 21);
    let window = state.read_u32(data, 4);

    let window_exists = state.windows.contains_key(&window)
        || state
            .shared_windows
            .lock()
            .ok()
            .is_some_and(|s| s.contains_key(&window));
    if !window_exists {
        return build_error(BAD_WINDOW, seq, window, 21, 0);
    }

    // Merge atoms from local and shared window stores.
    let mut atoms: Vec<u32> = state
        .windows
        .get(&window)
        .map(|w| w.properties.keys().copied().collect())
        .unwrap_or_default();
    // Add atoms from shared store that aren't in local
    if let Ok(shared) = state.shared_windows.lock() {
        if let Some(sw) = shared.get(&window) {
            for &atom in sw.properties.keys() {
                if !atoms.contains(&atom) {
                    atoms.push(atom);
                }
            }
        }
    }

    let n = atoms.len();
    let extra_bytes = n * 4;
    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_u16(8, n as u16); // num_atoms
    for (i, atom) in atoms.iter().enumerate() {
        reply = reply.set_u32(32 + i * 4, *atom);
    }
    reply.build()
}
