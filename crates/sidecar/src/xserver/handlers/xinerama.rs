//! XINERAMA extension handler (opcode 158).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;

/// Handle XINERAMA extension requests. We report a single screen covering the
/// entire display so that apps querying multi-monitor configurations work.
pub(crate) fn handle_xinerama_request(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // major
                .set_u16(10, 1) // minor
                .build()
        }
        1 => {
            // GetState
            let mut reply = ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(1); // state = active
            // window ID at bytes 8-11 (from request)
            if data.len() >= 8 {
                reply = reply.set_bytes(8, &data[4..8]);
            }
            reply.build()
        }
        2 => {
            // GetScreenCount
            let mut reply = ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(1); // screen_count = 1
            if data.len() >= 8 {
                reply = reply.set_bytes(8, &data[4..8]);
            }
            reply.build()
        }
        3 => {
            // GetScreenSize
            let mut reply = ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, state.screen_width as u32) // width
                .set_u32(12, state.screen_height as u32) // height
                .set_u32(20, 0); // screen_number
            if data.len() >= 8 {
                reply = reply.set_bytes(16, &data[4..8]); // window
            }
            reply.build()
        }
        4 => {
            // IsActive
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, 1) // state = active
                .build()
        }
        5 => {
            // QueryScreens - return single screen covering the whole display
            let num_screens: u32 = 1;
            let screen_info_size = 8usize; // x_org(2) + y_org(2) + width(2) + height(2)
            let extra = screen_info_size;
            let padded = (extra + 3) & !3;
            // Screen 0: x=0, y=0, width=state.screen_width, height=state.screen_height
            let off = 32;
            // x_org = 0, y_org = 0 (already zero)
            ReplyBuf::with_extra(seq, padded, state.msb_first)
                .set_u32(8, num_screens)
                .set_u16(off + 4, state.screen_width)
                .set_u16(off + 6, state.screen_height)
                .build()
        }
        _ => {
            debug!("XINERAMA: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST,
                seq,
                minor as u32,
                158,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
