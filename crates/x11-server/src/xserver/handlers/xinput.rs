//! XInput / XInputExtension dispatch wrapper.
//!
//! The actual XI 1.x and XI 2.x request handling lives in the
//! `crate::xinput2` module, which is deliberately kept independent of
//! `ClientState` so it can be unit-tested with explicit field
//! references. This file is the thin adapter that lets every extension
//! in `handlers::extensions` expose the same
//! `fn(&mut ClientState, &[u8], u16) -> Vec<u8>` signature.

use super::*;
use crate::xserver::core::{SCREEN_HEIGHT, SCREEN_WIDTH};

pub(crate) fn handle_xinput_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let custom_keymap = state.custom_keymap.lock().unwrap().clone();
    let mut reply = crate::xinput2::handle_request(
        data,
        seq,
        &mut state.xi.valuators,
        &mut state.xi.selections,
        &mut state.xi.pending,
        &mut state.xi.client_pointer,
        &mut state.xi.device_properties,
        &mut state.focus_window,
        &mut state.xi.active_grabs,
        &mut state.xi.passive_grabs,
        &mut state.xi.pointer_frozen,
        &mut state.xi.keyboard_frozen,
        &mut state.xi.frozen_pointer_events,
        &mut state.xi.frozen_keyboard_events,
        &mut state.xi.xi1_dont_propagate,
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        state.root_window,
        state.msb_first,
        &custom_keymap,
    );
    // XIQueryPointer reports the root window the pointer is in. We
    // serve a single screen, so patch the reply to always reference
    // this server's actual root window id — xinput2 doesn't know about
    // it without taking a dependency on ClientState.
    if data.len() >= 2
        && data[1] == x11rb_protocol::protocol::xinput::XI_QUERY_POINTER_REQUEST
        && reply.len() >= 12
    {
        crate::xinput2::patch_query_pointer_root(&mut reply, state.root_window, state.msb_first);
    }
    reply
}
