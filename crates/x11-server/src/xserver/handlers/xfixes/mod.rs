//! XFIXES extension handler.

mod barrier;
mod cursor;
mod region;

use std::collections::HashMap;

use tracing::debug;

use super::super::client::ClientState;
use super::parse_minor;
use crate::xserver::reply::serialize_reply;
use crate::xserver::types::{PointerBarrier, XFixesRegion};
use x11rb_protocol::protocol::xfixes::{
    ChangeSaveSetRequest, QueryVersionReply as XfixesQueryVersionReply, QueryVersionRequest,
    SelectSelectionInputRequest,
};

/// Per-connection XFIXES extension state. Lives on
/// `ClientState::xfixes`; reads and writes happen through
/// `state.xfixes.*`.
#[derive(Default)]
pub(crate) struct XFixesState {
    /// XFIXES regions.
    pub(crate) regions: HashMap<u32, XFixesRegion>,
    /// Clients subscribed to cursor change events (window_id → bool).
    pub(crate) cursor_event_subscribers: HashMap<u32, bool>,
    /// Clients subscribed to selection events (selection_atom → event_mask).
    pub(crate) selection_event_subscribers: HashMap<u32, u32>,
    /// Cursor is hidden (HideCursor/ShowCursor nesting count).
    pub(crate) cursor_hidden: u32,
    /// XFIXES pointer barriers.
    pub(crate) barriers: HashMap<u32, PointerBarrier>,
    /// XFIXES client disconnect mode (0 = default).
    pub(crate) disconnect_mode: u32,
}

pub(crate) fn handle_xfixes_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XFIXES minor opcode: {minor}");
    let xfixes_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 138, minor as u16)
    };

    match minor {
        // 0: QueryVersion
        0 => {
            let _req = parse_minor!(QueryVersionRequest, data, state, seq, 138, 0);
            serialize_reply(
                &XfixesQueryVersionReply {
                    sequence: seq,
                    length: 0,
                    major_version: 5,
                    minor_version: 0,
                },
                state.byte_order(),
            )
        }

        // 1: ChangeSaveSet (extended)
        1 => {
            let req = parse_minor!(ChangeSaveSetRequest, data, state, seq, 138, 1);
            let window = req.window;
            let mode: u8 = req.mode.into();

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
                    return xfixes_err(crate::xserver::core::VALUE_ERROR, mode as u32);
                }
            }
            debug!("XFIXES ChangeSaveSet: window={window:#x} mode={mode}");
            Vec::new()
        }

        // 2: SelectSelectionInput
        2 => {
            let req = parse_minor!(SelectSelectionInputRequest, data, state, seq, 138, 2);
            let window = req.window;
            let selection = req.selection;
            let event_mask = req.event_mask.bits();
            debug!("XFIXES SelectSelectionInput: window={window:#x} selection={selection:#x} mask={event_mask:#x}");
            if event_mask != 0 {
                state
                    .xfixes
                    .selection_event_subscribers
                    .insert(selection, event_mask);
            } else {
                state.xfixes.selection_event_subscribers.remove(&selection);
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
            xfixes_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
