//! MIT-SCREEN-SAVER extension handler (opcode 152).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// Screen saver window attributes stored by MIT-SCREEN-SAVER SetAttributes.
#[allow(dead_code)]
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
        0 => {
            // QueryVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // server_major
                .set_u16(10, 1) // server_minor
                .build()
        }
        1 => {
            // QueryInfo
            // state: 0=Off, 1=On, 2=Cycle, 3=Disabled
            let saver_state = if state.screen_saver_suspend_count > 0 {
                3u8 // Disabled
            } else if state.screen_saver.active {
                1u8 // On
            } else {
                0u8 // Off
            };
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(saver_state)
                .set_u32(8, state.screen_saver_window) // saver_window
                .set_u32(12, 0) // ms_until_server
                .set_u32(16, state.timestamp()) // ms_since_user_input
                .set_u32(20, state.screen_saver_event_mask) // event_mask
                .set_u8(24, 0) // kind = Blanked
                .build()
        }
        2 => {
            // SelectInput
            require_len!(data, 12, seq, 152, minor as u16, state.msb_first);
            let _drawable = state.read_u32(data, 4);
            let event_mask = state.read_u32(data, 8);
            state.screen_saver_event_mask = event_mask;
            debug!("ScreenSaver SelectInput: event_mask=0x{event_mask:08x}");
            Vec::new()
        }
        3 => {
            // SetAttributes
            // Store screen saver window attributes for when the saver activates.
            // Parse the same value-list as CreateWindow.
            require_len!(data, 24, seq, 152, minor as u16, state.msb_first);
            {
                let _drawable = state.read_u32(data, 4);
                let x = state.read_i16(data, 8);
                let y = state.read_i16(data, 10);
                let width = state.read_u16(data, 12);
                let height = state.read_u16(data, 14);
                let _border_width = state.read_u16(data, 16);
                let _class = data[18];
                let _depth = data[19];
                let _visual = state.read_u32(data, 20);
                let _value_mask = state.read_u32(data, 24);
                state.screen_saver_attrs = Some(ScreenSaverAttrs {
                    x,
                    y,
                    width,
                    height,
                });
                debug!("ScreenSaver SetAttributes: {x},{y} {width}x{height}");
            }
            Vec::new()
        }
        4 => {
            // UnsetAttributes
            state.screen_saver_attrs = None;
            debug!("ScreenSaver UnsetAttributes");
            Vec::new()
        }
        5 => {
            // Suspend
            // Reference-counted suspend: each Suspend increments, each Resume decrements.
            state.screen_saver_suspend_count += 1;
            debug!(
                "ScreenSaver Suspend: count={}",
                state.screen_saver_suspend_count
            );
            Vec::new()
        }
        6 => {
            // Resume
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
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                152,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
