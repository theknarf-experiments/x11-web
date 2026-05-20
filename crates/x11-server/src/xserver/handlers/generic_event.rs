//! Generic Event Extension (GE) handler.
//!
//! XGE is the stub negotiation extension for `GenericEvent` (response_type
//! 35). All it does is report its version; the actual XGE-wrapped events
//! belong to other extensions (XInput2, Present, …).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::ge::{
    QueryVersionReply as GeQueryVersionReply, QUERY_VERSION_REQUEST as GE_QUERY_VERSION_REQUEST,
};

pub(crate) fn handle_ge_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Generic Event Extension minor opcode: {minor}");

    match minor {
        GE_QUERY_VERSION_REQUEST => serialize_reply(
            &GeQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: 1,
                minor_version: 0,
            },
            state.byte_order(),
        ),
        _ => {
            debug!("Unhandled GE minor opcode: {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                135,
                minor as u16,
            )
        }
    }
}
