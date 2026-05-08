//! XVideo notification operations: SelectVideoNotify, SelectPortNotify.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::parse_minor;
use super::XV_MAJOR_OPCODE;
use x11rb_protocol::protocol::xv::{
    SELECT_PORT_NOTIFY_REQUEST, SELECT_VIDEO_NOTIFY_REQUEST, SelectPortNotifyRequest,
    SelectVideoNotifyRequest,
};

pub(crate) fn handle_notify_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
) -> Vec<u8> {
    match minor {
        SELECT_VIDEO_NOTIFY_REQUEST => {
            // Track interest in XvVideoNotify events on a drawable so we can
            // deliver them when video operations complete.
            let req = parse_minor!(
                SelectVideoNotifyRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            if req.onoff {
                state.xv_video_notify_drawables.insert(req.drawable);
            } else {
                state.xv_video_notify_drawables.remove(&req.drawable);
            }
            debug!(
                "XVideo SelectVideoNotify: drawable={:#x} on={}",
                req.drawable, req.onoff,
            );
            Vec::new()
        }
        SELECT_PORT_NOTIFY_REQUEST => {
            // Track interest in XvPortNotify events on a port so we can
            // deliver them when port attributes change.
            let req = parse_minor!(
                SelectPortNotifyRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            if req.onoff {
                state.xv_port_notify_ports.insert(req.port);
            } else {
                state.xv_port_notify_ports.remove(&req.port);
            }
            debug!(
                "XVideo SelectPortNotify: port={} on={}",
                req.port, req.onoff,
            );
            Vec::new()
        }
        _ => {
            debug!("XVideo notify: unhandled minor opcode {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                u32::from(minor),
                XV_MAJOR_OPCODE,
                u16::from(minor),
            )
        }
    }
}
