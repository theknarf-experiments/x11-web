//! MIT-SCREEN-SAVER extension handler (opcode 152).

use super::parse_minor;
use tracing::debug;
use x11rb_protocol::protocol::screensaver::{
    Kind, SelectInputRequest, SetAttributesRequest, State, QUERY_INFO_REQUEST,
    QUERY_VERSION_REQUEST, SELECT_INPUT_REQUEST, SET_ATTRIBUTES_REQUEST, SUSPEND_REQUEST,
    UNSET_ATTRIBUTES_REQUEST,
};

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// Resume request opcode (the X11 SCREEN-SAVER protocol pairs Suspend(5)
/// with Resume(6); x11rb only exposes `SUSPEND_REQUEST = 5`, so we
/// declare the sibling here.
const RESUME_REQUEST: u8 = 6;

/// Screen saver window attributes stored by MIT-SCREEN-SAVER SetAttributes.
pub(crate) struct ScreenSaverAttrs {
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

/// MIT-SCREEN-SAVER (opcode 152)
pub(crate) fn handle_screen_saver_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let minor = data[1];
    match minor {
        QUERY_VERSION_REQUEST => ReplyBuf::fixed(seq, state.msb_first)
            .set_u16(8, 1) // server_major
            .set_u16(10, 1) // server_minor
            .build(),
        QUERY_INFO_REQUEST => {
            let saver_state = if state.screen_saver_suspend_count > 0 {
                State::DISABLED
            } else if state.screen_saver.active {
                State::ON
            } else {
                State::OFF
            };
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(u8::from(saver_state))
                .set_u32(8, state.screen_saver_window) // saver_window
                .set_u32(12, 0) // ms_until_server
                .set_u32(16, state.timestamp()) // ms_since_user_input
                .set_u32(20, state.screen_saver_event_mask) // event_mask
                .set_u8(24, u8::from(Kind::BLANKED))
                .build()
        }
        SELECT_INPUT_REQUEST => {
            require_len!(data, 12, seq, 152, minor as u16, state.msb_first);
            let req = parse_minor!(SelectInputRequest, data, state, seq, 152, minor as u16);
            let event_mask = u32::from(req.event_mask);
            state.screen_saver_event_mask = event_mask;
            debug!("ScreenSaver SelectInput: event_mask=0x{event_mask:08x}");
            Vec::new()
        }
        SET_ATTRIBUTES_REQUEST => {
            // Store screen saver window attributes for when the saver activates.
            require_len!(data, 24, seq, 152, minor as u16, state.msb_first);
            let req = parse_minor!(SetAttributesRequest, data, state, seq, 152, minor as u16);
            state.screen_saver_attrs = Some(ScreenSaverAttrs {
                x: req.x,
                y: req.y,
                width: req.width,
                height: req.height,
            });
            debug!(
                "ScreenSaver SetAttributes: {},{} {}x{}",
                req.x, req.y, req.width, req.height
            );
            Vec::new()
        }
        UNSET_ATTRIBUTES_REQUEST => {
            state.screen_saver_attrs = None;
            debug!("ScreenSaver UnsetAttributes");
            Vec::new()
        }
        SUSPEND_REQUEST => {
            // Reference-counted: each Suspend increments, Resume decrements.
            state.screen_saver_suspend_count += 1;
            debug!(
                "ScreenSaver Suspend: count={}",
                state.screen_saver_suspend_count
            );
            Vec::new()
        }
        RESUME_REQUEST => {
            if state.screen_saver_suspend_count > 0 {
                state.screen_saver_suspend_count -= 1;
            }
            debug!(
                "ScreenSaver Resume: count={}",
                state.screen_saver_suspend_count
            );
            Vec::new()
        }
        _ => {
            debug!("ScreenSaver: unhandled minor opcode {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                152,
                minor as u16,
            )
        }
    }
}
