//! XFIXES cursor operations.
use super::super::parse_minor;
use crate::xserver::reply::serialize_var_reply;

use tracing::debug;

use super::super::super::client::ClientState;
use x11rb_protocol::protocol::xfixes::{
    ChangeCursorByNameRequest, ChangeCursorRequest, GetCursorImageAndNameReply,
    GetCursorImageAndNameRequest, GetCursorImageReply, GetCursorImageRequest, GetCursorNameReply,
    GetCursorNameRequest, HideCursorRequest, SelectCursorInputRequest, SetCursorNameRequest,
    ShowCursorRequest,
};

/// Pack the byte-stream ARGB pixel buffer into the `Vec<u32>` that
/// `GetCursorImageReply::cursor_image` expects. We treat the bytes as
/// little-endian so the serializer reproduces the same wire layout the
/// hand-rolled implementation emitted on LE clients (which is virtually
/// every real-world X11 client).
fn argb_bytes_to_u32(bytes: &[u8], width: u16, height: u16) -> Vec<u32> {
    let count = width as usize * height as usize;
    let mut out = Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(4).take(count) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    while out.len() < count {
        out.push(0);
    }
    out
}

/// 3: SelectCursorInput
pub(crate) fn handle_select_cursor_input(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(SelectCursorInputRequest, data, state, seq, 138, 3);
    let window = req.window;
    let event_mask = req.event_mask;
    debug!("XFIXES SelectCursorInput: window={window:#x} mask={event_mask:?}");
    state
        .cursor_event_subscribers
        .insert(window, event_mask.bits() != 0);
    Vec::new()
}

/// 4: GetCursorImage
pub(crate) fn handle_get_cursor_image(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let _req = parse_minor!(GetCursorImageRequest, data, state, seq, 138, 4);
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

    serialize_var_reply(
        &GetCursorImageReply {
            sequence: seq,
            length: 0,
            x: state.pointer_x,
            y: state.pointer_y,
            width,
            height,
            xhot: hotspot_x,
            yhot: hotspot_y,
            cursor_serial: state.cursor_serial,
            cursor_image: argb_bytes_to_u32(&argb_data, width, height),
        },
        state.byte_order(),
    )
}

/// 23: SetCursorName
pub(crate) fn handle_set_cursor_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SetCursorNameRequest, data, state, seq, 138, 23);
    let cursor_id = req.cursor;
    let name = String::from_utf8_lossy(&req.name).to_string();
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
    Vec::new()
}

/// 24: GetCursorName
pub(crate) fn handle_get_cursor_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(GetCursorNameRequest, data, state, seq, 138, 24);
    let cursor_id = req.cursor;
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
    serialize_var_reply(
        &GetCursorNameReply {
            sequence: seq,
            length: 0,
            atom,
            name: name.into_bytes(),
        },
        state.byte_order(),
    )
}

/// 25: GetCursorImageAndName
pub(crate) fn handle_get_cursor_image_and_name(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let _req = parse_minor!(GetCursorImageAndNameRequest, data, state, seq, 138, 25);
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

    let name_atom = if !name.is_empty() {
        let mut atoms = state.atoms.lock().unwrap();
        atoms.intern(&name, true)
    } else {
        0
    };
    serialize_var_reply(
        &GetCursorImageAndNameReply {
            sequence: seq,
            length: 0,
            x: state.pointer_x,
            y: state.pointer_y,
            width,
            height,
            xhot: hotspot_x,
            yhot: hotspot_y,
            cursor_serial: state.cursor_serial,
            cursor_atom: name_atom,
            cursor_image: argb_bytes_to_u32(&argb_data, width, height),
            name: name.into_bytes(),
        },
        state.byte_order(),
    )
}

/// 26: ChangeCursor
pub(crate) fn handle_change_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ChangeCursorRequest, data, state, seq, 138, 26);
    let source_cursor = req.source;
    let dest_cursor = req.destination;
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
    Vec::new()
}

/// 27: ChangeCursorByName
pub(crate) fn handle_change_cursor_by_name(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(ChangeCursorByNameRequest, data, state, seq, 138, 27);
    let source_cursor = req.src;
    let name = String::from_utf8_lossy(&req.name).to_string();
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
    Vec::new()
}

/// 29: HideCursor
///
/// XFIXES nesting counter is still maintained — XFIXES clients
/// observe a coherent state — but the browser-side cursor
/// notification was removed when frontend cursor delivery was
/// dropped. See `emit_cursor_changed` for the same context.
pub(crate) fn handle_hide_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(HideCursorRequest, data, state, seq, 138, 29);
    let window_id = req.window;
    state.cursor_hidden = state.cursor_hidden.saturating_add(1);
    debug!(
        "XFIXES HideCursor: window={window_id:#x} nesting={}",
        state.cursor_hidden
    );
    Vec::new()
}

/// 30: ShowCursor — see `handle_hide_cursor` for the dropped-frontend note.
pub(crate) fn handle_show_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ShowCursorRequest, data, state, seq, 138, 30);
    let window_id = req.window;
    state.cursor_hidden = state.cursor_hidden.saturating_sub(1);
    debug!(
        "XFIXES ShowCursor: window={window_id:#x} nesting={}",
        state.cursor_hidden
    );
    Vec::new()
}
