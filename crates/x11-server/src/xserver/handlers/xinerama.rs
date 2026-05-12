//! XINERAMA extension handler (opcode 158).

use tracing::debug;

use super::super::client::ClientState;
use super::parse_minor;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xinerama::{
    GetScreenCountReply, GetScreenCountRequest, GetScreenSizeReply, GetScreenSizeRequest,
    GetStateReply, GetStateRequest, IsActiveReply, IsActiveRequest, QueryScreensReply,
    QueryScreensRequest, QueryVersionReply, QueryVersionRequest, ScreenInfo,
};

/// Handle XINERAMA extension requests. We report a single screen covering the
/// entire display so that apps querying multi-monitor configurations work.
pub(crate) fn handle_xinerama_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            let _req = parse_minor!(QueryVersionRequest, data, state, seq, 158, minor);
            serialize_reply(
                &QueryVersionReply {
                    sequence: seq,
                    length: 0,
                    major: 1,
                    minor: 1,
                },
                state.byte_order(),
            )
        }
        1 => {
            // GetState
            let req = parse_minor!(GetStateRequest, data, state, seq, 158, minor);
            serialize_reply(
                &GetStateReply {
                    state: 1, // active
                    sequence: seq,
                    length: 0,
                    window: req.window,
                },
                state.byte_order(),
            )
        }
        2 => {
            // GetScreenCount
            let req = parse_minor!(GetScreenCountRequest, data, state, seq, 158, minor);
            serialize_reply(
                &GetScreenCountReply {
                    screen_count: 1,
                    sequence: seq,
                    length: 0,
                    window: req.window,
                },
                state.byte_order(),
            )
        }
        3 => {
            // GetScreenSize
            let req = parse_minor!(GetScreenSizeRequest, data, state, seq, 158, minor);
            serialize_reply(
                &GetScreenSizeReply {
                    sequence: seq,
                    length: 0,
                    width: state.screen_width as u32,
                    height: state.screen_height as u32,
                    window: req.window,
                    screen: 0,
                },
                state.byte_order(),
            )
        }
        4 => {
            // IsActive
            let _req = parse_minor!(IsActiveRequest, data, state, seq, 158, minor);
            serialize_reply(
                &IsActiveReply {
                    sequence: seq,
                    length: 0,
                    state: 1,
                },
                state.byte_order(),
            )
        }
        5 => {
            // QueryScreens - return single screen covering the whole display
            let _req = parse_minor!(QueryScreensRequest, data, state, seq, 158, minor);
            serialize_var_reply(
                &QueryScreensReply {
                    sequence: seq,
                    length: 0,
                    screen_info: vec![ScreenInfo {
                        x_org: 0,
                        y_org: 0,
                        width: state.screen_width,
                        height: state.screen_height,
                    }],
                },
                state.byte_order(),
            )
        }
        _ => {
            debug!("XINERAMA: unhandled minor opcode {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                158,
                minor as u16,
            )
        }
    }
}
