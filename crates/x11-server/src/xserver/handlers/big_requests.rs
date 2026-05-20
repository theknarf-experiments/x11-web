//! BIG-REQUESTS extension handler.
//!
//! BIG-REQUESTS only defines a single request, `BigReqEnable`, that
//! flips a per-connection flag and reports the maximum request length
//! the server will accept. There's no error/event surface.

use super::super::client::ClientState;
use crate::xserver::reply::serialize_reply;

pub(crate) fn handle_big_requests_request(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    state.big_requests_enabled = true;
    serialize_reply(
        &x11rb_protocol::protocol::bigreq::EnableReply {
            sequence: seq,
            length: 0,
            maximum_request_length: crate::xserver::core::BIG_REQUESTS_MAX_LEN_WORDS,
        },
        state.byte_order(),
    )
}
