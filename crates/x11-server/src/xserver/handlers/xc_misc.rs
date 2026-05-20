//! XC-MISC extension handler.
//!
//! Provides protocol-version negotiation and resource-ID allocation
//! helpers (GetXIDRange / GetXIDList).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::xc_misc::{
    GetVersionReply, GetXIDListReply, GetXIDListRequest, GetXIDRangeReply, GET_VERSION_REQUEST,
    GET_XID_LIST_REQUEST, GET_XID_RANGE_REQUEST,
};

pub(crate) fn handle_xc_misc_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XC-MISC minor opcode: {minor}");

    match minor {
        GET_VERSION_REQUEST => serialize_reply(
            &GetVersionReply {
                sequence: seq,
                length: 0,
                server_major_version: 1,
                server_minor_version: 1,
            },
            state.byte_order(),
        ),
        GET_XID_RANGE_REQUEST => {
            // Reply with a contiguous range of resource IDs.
            // Per the XC-MISC spec, first try to return recycled (freed) IDs
            // as individual IDs wouldn't form a contiguous range; fall back
            // to allocating new IDs from the client's ID space.
            let mask: u32 = crate::xserver::core::RESOURCE_ID_MASK;
            let current_offset = state.next_xid.wrapping_sub(state.resource_id_base) & mask;
            let remaining = mask.saturating_sub(current_offset) + 1;
            let range_size = remaining.min(65536);
            let start_id = state.resource_id_base | (current_offset & mask);
            // Advance the counter
            state.next_xid = state.resource_id_base | ((current_offset + range_size) & mask);

            serialize_reply(
                &GetXIDRangeReply {
                    sequence: seq,
                    length: 0,
                    start_id,
                    count: range_size,
                },
                state.byte_order(),
            )
        }
        GET_XID_LIST_REQUEST => {
            // Return the requested number of individual resource IDs.
            // Prefer recycled (freed) IDs over allocating new sequential ones.
            let count = GetXIDListRequest::try_parse_request(request_header(data), &data[4..])
                .map(|r| r.count)
                .unwrap_or(0);
            let ids_to_return = count.min(4096) as usize;

            let mut ids: Vec<u32> = Vec::with_capacity(ids_to_return);

            while ids.len() < ids_to_return && !state.freed_xids.is_empty() {
                ids.push(state.freed_xids.pop().unwrap());
            }

            if ids.len() < ids_to_return {
                let mask: u32 = crate::xserver::core::RESOURCE_ID_MASK;
                let current_offset = state.next_xid.wrapping_sub(state.resource_id_base) & mask;
                let remaining = mask.saturating_sub(current_offset) + 1;
                let sequential_count = ((ids_to_return - ids.len()) as u32).min(remaining);
                for i in 0..sequential_count {
                    let id = state.resource_id_base | ((current_offset + i) & mask);
                    ids.push(id);
                }
                state.next_xid =
                    state.resource_id_base | ((current_offset + sequential_count) & mask);
            }

            serialize_var_reply(
                &GetXIDListReply {
                    sequence: seq,
                    length: 0,
                    ids,
                },
                state.byte_order(),
            )
        }
        _ => {
            debug!("Unhandled XC-MISC minor opcode: {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                141,
                minor as u16,
            )
        }
    }
}
