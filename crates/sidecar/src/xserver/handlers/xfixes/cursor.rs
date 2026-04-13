//! XFIXES cursor operations.

use tracing::debug;

use super::super::super::client::ClientState;

/// 3: SelectCursorInput
pub(crate) fn handle_select_cursor_input(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 12 {
        let window = state.read_u32(data, 4);
        let event_mask = state.read_u32(data, 8);
        debug!("XFIXES SelectCursorInput: window={window:#x} mask={event_mask:#x}");
        state
            .cursor_event_subscribers
            .insert(window, event_mask != 0);
    }
    Vec::new()
}

/// 4: GetCursorImage
pub(crate) fn handle_get_cursor_image(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let _ = data;
    // Try to find current cursor info
    let cursor_id = state.current_cursor;
    let (width, height, hotspot_x, hotspot_y, argb_data) = if cursor_id != 0 {
        if let Some(info) = state.cursor_info.get(&cursor_id) {
            if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                (
                    info.width,
                    info.height,
                    info.hotspot_x,
                    info.hotspot_y,
                    info.argb_data.clone(),
                )
            } else {
                // Cursor exists but no bitmap — return 1x1 transparent
                (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
            }
        } else {
            (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
        }
    } else {
        // Default cursor — return 1x1 transparent
        (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
    };

    let pixels_len = (width as usize) * (height as usize) * 4;
    let extra = 24 + pixels_len;
    let total = 32 + extra;
    let length_units = (extra / 4) as u32;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_units);
    state.write_i16(&mut reply, 8, state.pointer_x); // x
    state.write_i16(&mut reply, 10, state.pointer_y); // y
    state.write_u16(&mut reply, 12, width);
    state.write_u16(&mut reply, 14, height);
    state.write_u16(&mut reply, 16, hotspot_x);
    state.write_u16(&mut reply, 18, hotspot_y);
    state.write_u32(&mut reply, 20, state.cursor_serial);
    // Copy ARGB pixel data
    let copy_len = pixels_len.min(argb_data.len());
    reply[32..32 + copy_len].copy_from_slice(&argb_data[..copy_len]);
    reply
}

/// 23: SetCursorName
pub(crate) fn handle_set_cursor_name(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let cursor_id = state.read_u32(data, 4);
        let name_len = state.read_u16(data, 8) as usize;
        if data.len() >= 12 + name_len {
            let name = String::from_utf8_lossy(&data[12..12 + name_len]).to_string();
            debug!("XFIXES SetCursorName: cursor={cursor_id:#x} name={name:?}");
            // Store name in existing cursor info, or create a minimal entry
            if let Some(info) = state.cursor_info.get_mut(&cursor_id) {
                info.name = name;
            } else {
                use super::super::super::types::CursorInfo;
                state.cursor_info.insert(
                    cursor_id,
                    CursorInfo {
                        css_name: String::new(),
                        source_pixmap: 0,
                        mask_pixmap: 0,
                        fore_red: 0,
                        fore_green: 0,
                        fore_blue: 0,
                        back_red: 0,
                        back_green: 0,
                        back_blue: 0,
                        hotspot_x: 0,
                        hotspot_y: 0,
                        argb_data: Vec::new(),
                        width: 0,
                        height: 0,
                        name,
                        anim_frames: Vec::new(),
                    },
                );
            }
        }
    }
    Vec::new()
}

/// 24: GetCursorName
pub(crate) fn handle_get_cursor_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let cursor_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    let name = state
        .cursor_info
        .get(&cursor_id)
        .map(|info| info.name.clone())
        .unwrap_or_default();
    let atom = if !name.is_empty() {
        let mut atoms = state.atoms.lock().unwrap();
        atoms.intern(&name, true)
    } else {
        0
    };
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    let pad = (4 - (name_len % 4)) % 4;
    let extra = name_len + pad;
    let total = 32 + extra;
    let length_units = (extra / 4) as u32;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_units);
    state.write_u32(&mut reply, 8, atom); // cursor name atom
    state.write_u16(&mut reply, 12, name_len as u16); // nbytes
    if !name_bytes.is_empty() {
        reply[32..32 + name_len].copy_from_slice(name_bytes);
    }
    reply
}

/// 25: GetCursorImageAndName
pub(crate) fn handle_get_cursor_image_and_name(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let _ = data;
    let cursor_id = state.current_cursor;
    let (width, height, hotspot_x, hotspot_y, argb_data, name) = if cursor_id != 0 {
        if let Some(info) = state.cursor_info.get(&cursor_id) {
            if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                (
                    info.width,
                    info.height,
                    info.hotspot_x,
                    info.hotspot_y,
                    info.argb_data.clone(),
                    info.name.clone(),
                )
            } else {
                (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], info.name.clone())
            }
        } else {
            (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], String::new())
        }
    } else {
        (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], String::new())
    };

    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    let name_atom = if !name.is_empty() {
        let mut atoms = state.atoms.lock().unwrap();
        atoms.intern(&name, true)
    } else {
        0
    };
    let pixels_len = (width as usize) * (height as usize) * 4;
    let name_pad = (4 - (name_len % 4)) % 4;
    // Reply body after the 32-byte header:
    //   x(2) y(2) width(2) height(2) hotspot_x(2) hotspot_y(2) serial(4) atom(4) name_len(2) pad(2)
    //   = 24 bytes of fields, then pixels, then name + padding
    let extra = 24 + pixels_len + name_len + name_pad;
    let total = 32 + extra;
    let length_units = (extra / 4) as u32;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_units);
    state.write_i16(&mut reply, 8, state.pointer_x); // x
    state.write_i16(&mut reply, 10, state.pointer_y); // y
    state.write_u16(&mut reply, 12, width);
    state.write_u16(&mut reply, 14, height);
    state.write_u16(&mut reply, 16, hotspot_x);
    state.write_u16(&mut reply, 18, hotspot_y);
    state.write_u32(&mut reply, 20, state.cursor_serial);
    state.write_u32(&mut reply, 24, name_atom);
    state.write_u16(&mut reply, 28, name_len as u16);
    // Pixel data starts at 32
    let copy_len = pixels_len.min(argb_data.len());
    reply[32..32 + copy_len].copy_from_slice(&argb_data[..copy_len]);
    // Name data follows pixels
    let name_offset = 32 + pixels_len;
    if !name_bytes.is_empty() {
        reply[name_offset..name_offset + name_len].copy_from_slice(name_bytes);
    }
    reply
}

/// 26: ChangeCursor
pub(crate) fn handle_change_cursor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let source_cursor = state.read_u32(data, 4);
        let dest_cursor = state.read_u32(data, 8);
        debug!("XFIXES ChangeCursor: source={source_cursor:#x} dest={dest_cursor:#x}");
        // Update all windows that use dest_cursor to use source_cursor instead
        let windows_to_update: Vec<u32> = state
            .windows
            .iter()
            .filter(|(_, w)| w.cursor == Some(dest_cursor))
            .map(|(id, _)| *id)
            .collect();
        for wid in windows_to_update {
            if let Some(w) = state.windows.get_mut(&wid) {
                w.cursor = Some(source_cursor);
            }
        }
        // Copy cursor info from source to dest
        if let Some(info) = state.cursor_info.get(&source_cursor).cloned() {
            state.cursor_info.insert(dest_cursor, info);
        }
        if let Some(css) = state.cursors.get(&source_cursor).cloned() {
            state.cursors.insert(dest_cursor, css);
        }
    }
    Vec::new()
}

/// 27: ChangeCursorByName
pub(crate) fn handle_change_cursor_by_name(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 12 {
        let source_cursor = state.read_u32(data, 4);
        let name_len = state.read_u16(data, 8) as usize;
        if data.len() >= 12 + name_len {
            let name = String::from_utf8_lossy(&data[12..12 + name_len]).to_string();
            debug!("XFIXES ChangeCursorByName: source={source_cursor:#x} name={name:?}");
            // Find all cursors that have the matching name
            let matching_cursor_ids: Vec<u32> = state
                .cursor_info
                .iter()
                .filter(|(_, info)| info.name == name)
                .map(|(id, _)| *id)
                .collect();
            // Replace each matching cursor with source_cursor's info
            if let Some(source_info) = state.cursor_info.get(&source_cursor).cloned() {
                let source_css = state.cursors.get(&source_cursor).cloned();
                for cid in &matching_cursor_ids {
                    state.cursor_info.insert(*cid, source_info.clone());
                    if let Some(ref css) = source_css {
                        state.cursors.insert(*cid, css.clone());
                    }
                    // Update windows using this cursor
                    let windows_to_update: Vec<u32> = state
                        .windows
                        .iter()
                        .filter(|(_, w)| w.cursor == Some(*cid))
                        .map(|(id, _)| *id)
                        .collect();
                    for wid in windows_to_update {
                        if let Some(w) = state.windows.get_mut(&wid) {
                            w.cursor = Some(source_cursor);
                        }
                    }
                }
            }
        }
    }
    Vec::new()
}

/// 29: HideCursor
pub(crate) fn handle_hide_cursor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let window_id = state.read_u32(data, 4);
        state.cursor_hidden = state.cursor_hidden.saturating_add(1);
        debug!(
            "XFIXES HideCursor: window={window_id:#x} nesting={}",
            state.cursor_hidden
        );
        // On first hide, send cursor changed to "none"
        if state.cursor_hidden == 1 {
            if let Some(uuid) = state
                .top_level_uuid_for(window_id)
                .or_else(|| state.window_uuid(window_id))
            {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    x11_web_protocol::DisplayUpdate::CursorChanged {
                        window_id: uuid,
                        cursor: "none".to_string(),
                    },
                ));
            }
        }
    }
    Vec::new()
}

/// 30: ShowCursor
pub(crate) fn handle_show_cursor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let window_id = state.read_u32(data, 4);
        state.cursor_hidden = state.cursor_hidden.saturating_sub(1);
        debug!(
            "XFIXES ShowCursor: window={window_id:#x} nesting={}",
            state.cursor_hidden
        );
        // When nesting reaches 0, restore the real cursor
        if state.cursor_hidden == 0 {
            // Resolve the real cursor name from current_cursor or fall back to "default"
            let real_cursor = if state.current_cursor != 0 {
                state
                    .cursors
                    .get(&state.current_cursor)
                    .cloned()
                    .or_else(|| {
                        state
                            .cursor_info
                            .get(&state.current_cursor)
                            .map(|i| i.css_name.clone())
                    })
                    .unwrap_or_else(|| "default".to_string())
            } else {
                "default".to_string()
            };
            if let Some(uuid) = state
                .top_level_uuid_for(window_id)
                .or_else(|| state.window_uuid(window_id))
            {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    x11_web_protocol::DisplayUpdate::CursorChanged {
                        window_id: uuid,
                        cursor: real_cursor,
                    },
                ));
            }
        }
    }
    Vec::new()
}
