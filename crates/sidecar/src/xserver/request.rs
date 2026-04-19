//! Request parsing helpers.
//!
//! Provides `request_header()` to build the `RequestHeader` that x11rb's
//! `try_parse_request` methods expect, plus `swap_request_to_le()` for
//! the rare MSB-first client case.
//!
//! Usage:
//! ```ignore
//! use x11rb_protocol::protocol::xproto::GetPropertyRequest;
//! let header = request_header(data);
//! let req = GetPropertyRequest::try_parse_request(header, &data[4..])?;
//! // req.window, req.property, req.type_, etc.
//! ```

use x11rb_protocol::x11_utils::RequestHeader;

/// Build a RequestHeader from the first 4 bytes of raw request data.
#[inline]
pub(crate) fn request_header(data: &[u8]) -> RequestHeader {
    RequestHeader {
        major_opcode: data[0],
        minor_opcode: data[1],
        remaining_length: 0,
    }
}

/// Byte-swap a request from MSB-first (big-endian) to native LE.
///
/// X11 requests have a 4-byte header (opcode, minor, length) followed by
/// u32/u16 fields. This does a rough swap treating bytes 2-3 as a u16
/// and bytes 4+ as u32-aligned words. Not perfect for every request type
/// but sufficient for the extremely rare MSB client case.
pub(crate) fn swap_request_to_le(data: &mut [u8]) {
    if data.len() >= 4 {
        data.swap(2, 3); // length u16
    }
    for i in (4..data.len()).step_by(4) {
        if i + 3 < data.len() {
            data.swap(i, i + 3);
            data.swap(i + 1, i + 2);
        }
    }
}
