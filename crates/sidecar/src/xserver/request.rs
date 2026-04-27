//! Build the `RequestHeader` x11rb's `try_parse_request` methods expect.
//! Used by the `typed!` and `parse_minor!` dispatch macros in
//! `xserver::handlers`.

use x11rb_protocol::x11_utils::RequestHeader;

#[inline]
pub(crate) fn request_header(data: &[u8]) -> RequestHeader {
    RequestHeader {
        major_opcode: data[0],
        minor_opcode: data[1],
        remaining_length: 0,
    }
}
