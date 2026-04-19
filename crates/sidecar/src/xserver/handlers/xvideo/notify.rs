//! XVideo notification operations: SelectVideoNotify, SelectPortNotify.

use tracing::debug;

use super::super::super::client::ClientState;

pub(crate) fn handle_notify_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
) -> Vec<u8> {
    match minor {
        13 => {
            // SelectVideoNotify — register interest in XvVideoNotify events on a drawable
            // This is a void request. Track the subscription in client state so we can
            // deliver VideoNotify events if/when video operations complete on that drawable.
            if data.len() >= 9 {
                let drawable = state.read_u32(data, 4);
                let on_off = data[8] != 0;
                if on_off {
                    state.xv_video_notify_drawables.insert(drawable);
                } else {
                    state.xv_video_notify_drawables.remove(&drawable);
                }
                debug!("XVideo SelectVideoNotify: drawable={drawable:#x} on={on_off}");
            } else {
                debug!(
                    "XVideo SelectVideoNotify: short request (len={}), ignoring",
                    data.len()
                );
            }
            Vec::new()
        }
        14 => {
            // SelectPortNotify — register interest in XvPortNotify events on a port
            // This is a void request. Track the subscription so we can deliver
            // PortNotify events when port attributes change.
            if data.len() >= 9 {
                let port = state.read_u32(data, 4);
                let on_off = data[8] != 0;
                if on_off {
                    state.xv_port_notify_ports.insert(port);
                } else {
                    state.xv_port_notify_ports.remove(&port);
                }
                debug!("XVideo SelectPortNotify: port={port} on={on_off}");
            } else {
                debug!(
                    "XVideo SelectPortNotify: short request (len={}), ignoring",
                    data.len()
                );
            }
            Vec::new()
        }
        _ => {
            debug!("XVideo notify: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                156,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
