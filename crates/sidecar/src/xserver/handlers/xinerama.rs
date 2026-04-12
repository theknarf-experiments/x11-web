//! XINERAMA extension handler (opcode 158).

use super::super::client::ClientState;

/// Handle XINERAMA extension requests. We report a single screen covering the
/// entire display so that apps querying multi-monitor configurations work.
pub(crate) fn handle_xinerama_request(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1); // major
            state.write_u16(&mut reply, 10, 1); // minor
            reply.to_vec()
        }
        1 => {
            // GetState
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 1; // state = active
            state.write_u16(&mut reply, 2, seq);
            // window ID at bytes 8-11 (from request)
            if data.len() >= 8 {
                reply[8..12].copy_from_slice(&data[4..8]);
            }
            reply.to_vec()
        }
        2 => {
            // GetScreenCount
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 1; // screen_count = 1
            state.write_u16(&mut reply, 2, seq);
            if data.len() >= 8 {
                reply[8..12].copy_from_slice(&data[4..8]);
            }
            reply.to_vec()
        }
        3 => {
            // GetScreenSize
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, state.screen_width as u32); // width
            state.write_u32(&mut reply, 12, state.screen_height as u32); // height
            if data.len() >= 8 {
                reply[16..20].copy_from_slice(&data[4..8]); // window
            }
            state.write_u32(&mut reply, 20, 0); // screen_number
            reply.to_vec()
        }
        4 => {
            // IsActive
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 1); // state = active
            reply.to_vec()
        }
        5 => {
            // QueryScreens - return single screen covering the whole display
            let num_screens: u32 = 1;
            let screen_info_size = 8usize; // x_org(2) + y_org(2) + width(2) + height(2)
            let extra = screen_info_size;
            let padded = (extra + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (padded / 4) as u32);
            state.write_u32(&mut reply, 8, num_screens);
            // Screen 0: x=0, y=0, width=state.screen_width, height=state.screen_height
            let off = 32;
            // x_org = 0, y_org = 0 (already zero)
            state.write_u16(&mut reply, off + 4, state.screen_width);
            state.write_u16(&mut reply, off + 6, state.screen_height);
            reply
        }
        _ => {
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, 0,
                158, minor as u16, state.msb_first,
            )
        }
    }
}
