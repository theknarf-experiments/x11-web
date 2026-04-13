//! XFIXES extension handler.

mod cursor;
mod region;
mod barrier;

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::core::require_len;

pub(crate) fn handle_xfixes_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XFIXES minor opcode: {minor}");

    match minor {
        // 0: QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 5u32);
            state.write_u32(&mut reply, 12, 0u32);
            reply.to_vec()
        }

        // 1: ChangeSaveSet (extended)
        1 => {
            require_len!(data, 12, seq, 138, 1, state.msb_first);
            let window = state.read_u32(data, 4);
            let mode   = data[8];
            let _target = if data.len() > 9 { data[9] } else { 0 };
            let _map    = if data.len() > 10 { data[10] } else { 0 };

            match mode {
                0 => {
                    // SetModeInsert
                    if !state.save_set.contains(&window) {
                        state.save_set.push(window);
                    }
                }
                1 => {
                    // SetModeDelete
                    state.save_set.retain(|&w| w != window);
                }
                _ => {
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::BAD_VALUE, seq, mode as u32,
                        138, 1, state.msb_first,
                    );
                }
            }
            debug!("XFIXES ChangeSaveSet: window={window:#x} mode={mode}");
            Vec::new()
        }

        // 2: SelectSelectionInput
        2 => {
            if data.len() >= 16 {
                let window = state.read_u32(data, 4);
                let selection = state.read_u32(data, 8);
                let event_mask = state.read_u32(data, 12);
                debug!("XFIXES SelectSelectionInput: window={window:#x} selection={selection:#x} mask={event_mask:#x}");
                if event_mask != 0 {
                    state.selection_event_subscribers.insert(selection, event_mask);
                } else {
                    state.selection_event_subscribers.remove(&selection);
                }
            }
            Vec::new()
        }

        // Cursor operations
        3 => cursor::handle_select_cursor_input(state, data, seq),
        4 => cursor::handle_get_cursor_image(state, data, seq),
        23 => cursor::handle_set_cursor_name(state, data, seq),
        24 => cursor::handle_get_cursor_name(state, data, seq),
        25 => cursor::handle_get_cursor_image_and_name(state, data, seq),
        26 => cursor::handle_change_cursor(state, data, seq),
        27 => cursor::handle_change_cursor_by_name(state, data, seq),
        29 => cursor::handle_hide_cursor(state, data, seq),
        30 => cursor::handle_show_cursor(state, data, seq),

        // Region operations
        5 => region::handle_create_region(state, data, seq),
        6 => region::handle_create_region_from_bitmap(state, data, seq),
        7 => region::handle_create_region_from_window(state, data, seq),
        8 => region::handle_create_region_from_gc(state, data, seq),
        9 => region::handle_create_region_from_picture(state, data, seq),
        10 => region::handle_destroy_region(state, data, seq),
        11 => region::handle_set_region(state, data, seq),
        12 => region::handle_copy_region(state, data, seq),
        13 => region::handle_union_region(state, data, seq),
        14 => region::handle_intersect_region(state, data, seq),
        15 => region::handle_subtract_region(state, data, seq),
        16 => region::handle_invert_region(state, data, seq),
        17 => region::handle_translate_region(state, data, seq),
        18 => region::handle_region_extents(state, data, seq),
        19 => region::handle_fetch_region(state, data, seq),
        20 => region::handle_set_gc_clip_region(state, data, seq),
        21 => region::handle_set_window_shape_region(state, data, seq),
        22 => region::handle_set_picture_clip_region(state, data, seq),
        28 => region::handle_expand_region(state, data, seq),

        // Barrier/misc operations
        31 => barrier::handle_create_pointer_barrier(state, data, seq),
        32 => barrier::handle_delete_pointer_barrier(state, data, seq),
        33 => barrier::handle_set_client_disconnect_mode(state, data, seq),
        34 => barrier::handle_get_client_disconnect_mode(state, data, seq),

        _ => {
            debug!("XFIXES: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                138, minor as u16, state.msb_first,
            )
        }
    }
}
