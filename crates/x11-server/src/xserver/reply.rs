//! Type-safe X11 reply builder.
//!
//! Eliminates manual byte-offset construction for X11 reply headers.
//! Enforces the invariant that `reply.length` matches the actual extra data.
//!
//! # Wire layout (xReply)
//!
//! ```text
//!   [0]      type = 1 (X_Reply)
//!   [1]      data byte (varies by reply type)
//!   [2..4]   sequence number
//!   [4..8]   length — extra 4-byte words beyond the 32-byte header
//!   [8..32]  reply-specific fields
//!   [32..]   extra data (only if length > 0)
//! ```

use super::core::{X11_EVENT_SIZE as REPLY_HEADER_SIZE, X11_WORD_SIZE as BYTES_PER_WORD};
use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

/// Reply type byte: X11 reply messages always have byte 0 == 1.
const REPLY_TYPE_BYTE: u8 = 1;

/// Convert a `msb_first` bool flag into the matching x11rb `ByteOrder`.
/// Used by callers that have a raw bool but not a full `ClientState`
/// (e.g. the GLX / XInput dispatchers that pass `msb_first` as a parameter).
#[inline]
pub(crate) fn byte_order_of(msb_first: bool) -> ByteOrder {
    if msb_first {
        ByteOrder::Msb
    } else {
        ByteOrder::Lsb
    }
}

/// Serialise an x11rb reply struct via the codegen-emitted
/// `SerializeEndian` impl, padding short fixed-size types to the
/// 32-byte wire minimum and stamping the X11 length field from the
/// actual buffer size. Use this in preference to `ReplyBuf` whenever
/// the reply struct exists in `x11rb_protocol::protocol::*` — the
/// codegen owns the wire layout so call sites stay short and any field
/// reordering / addition flows through one regeneration.
///
/// The codegen serializer emits `self.length` verbatim. Fixed replies
/// larger than 32 bytes (e.g. `GetWindowAttributesReply` at 44, or
/// `QueryKeymapReply` at 40) would otherwise advertise the wrong wire
/// length, causing clients to drop the trailing bytes and desync.
pub(crate) fn serialize_reply<R: SerializeEndian>(reply: &R, byte_order: ByteOrder) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(REPLY_HEADER_SIZE);
    reply.serialize_endian_into(&mut bytes, byte_order);
    if bytes.len() < REPLY_HEADER_SIZE {
        bytes.resize(REPLY_HEADER_SIZE, 0);
    }
    let pad = (BYTES_PER_WORD - (bytes.len() % BYTES_PER_WORD)) % BYTES_PER_WORD;
    bytes.resize(bytes.len() + pad, 0);
    let length = ((bytes.len() - REPLY_HEADER_SIZE) / BYTES_PER_WORD) as u32;
    let length_bytes = match byte_order {
        ByteOrder::Lsb => length.to_le_bytes(),
        ByteOrder::Msb => length.to_be_bytes(),
    };
    bytes[offset::LENGTH..offset::LENGTH + BYTES_PER_WORD].copy_from_slice(&length_bytes);
    bytes
}

/// Serialise a variable-length reply struct (one whose payload extends
/// past the 32-byte header) and stamp the X11 length field from the
/// actual buffer size, byte-order-aware. The caller is responsible for
/// building the reply struct with whatever fields it wants — `length`
/// can be 0 and gets overwritten here.
///
/// If the codegen-emitted serializer produces a payload that isn't a
/// multiple of 4 bytes (e.g. a trailing `Vec<u8>` whose length is not
/// 4-aligned), we zero-pad to the next 4-byte boundary so the wire
/// length field is a valid word count. The X11 protocol requires
/// reply length to be expressed in 4-byte words.
pub(crate) fn serialize_var_reply<R: SerializeEndian>(
    reply: &R,
    byte_order: ByteOrder,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    reply.serialize_endian_into(&mut bytes, byte_order);
    debug_assert!(bytes.len() >= REPLY_HEADER_SIZE);
    let pad = (BYTES_PER_WORD - (bytes.len() % BYTES_PER_WORD)) % BYTES_PER_WORD;
    bytes.resize(bytes.len() + pad, 0);
    let length = ((bytes.len() - REPLY_HEADER_SIZE) / BYTES_PER_WORD) as u32;
    let length_bytes = match byte_order {
        ByteOrder::Lsb => length.to_le_bytes(),
        ByteOrder::Msb => length.to_be_bytes(),
    };
    bytes[offset::LENGTH..offset::LENGTH + BYTES_PER_WORD].copy_from_slice(&length_bytes);
    bytes
}

mod offset {
    /// Sequence number (u16) — bytes 2..4.
    pub(super) const SEQUENCE: usize = 2;
    /// Length in 4-byte words of extra data beyond the 32-byte header (u32).
    pub(super) const LENGTH: usize = 4;
}

/// Builder for core X11 replies.
///
/// Handles the 32-byte header automatically. Callers set fields by name.
/// The `length` field at [4..8] is computed from extra data, not set manually.
pub(crate) struct ReplyBuf {
    buf: Vec<u8>,
    msb_first: bool,
}

impl ReplyBuf {
    /// Create a fixed 32-byte reply (no extra data, length=0).
    pub(crate) fn fixed(seq: u16, msb_first: bool) -> Self {
        let mut buf = vec![0u8; REPLY_HEADER_SIZE];
        buf[0] = REPLY_TYPE_BYTE;
        write_u16(&mut buf, offset::SEQUENCE, seq, msb_first);
        Self { buf, msb_first }
    }

    /// Create a reply with extra data. `length` is set to `extra_bytes / 4`.
    /// `extra_bytes` must be a multiple of 4.
    pub(crate) fn with_extra(seq: u16, extra_bytes: usize, msb_first: bool) -> Self {
        debug_assert!(
            extra_bytes.is_multiple_of(BYTES_PER_WORD),
            "extra_bytes must be a multiple of 4"
        );
        let extra_words = (extra_bytes / BYTES_PER_WORD) as u32;
        let mut buf = vec![0u8; REPLY_HEADER_SIZE + extra_bytes];
        buf[0] = REPLY_TYPE_BYTE;
        write_u16(&mut buf, offset::SEQUENCE, seq, msb_first);
        write_u32(&mut buf, offset::LENGTH, extra_words, msb_first);
        Self { buf, msb_first }
    }

    /// Set byte[1] (the "data byte" — used for format, depth, is_direct, etc.)
    pub(crate) fn set_data_byte(mut self, val: u8) -> Self {
        self.buf[1] = val;
        self
    }

    /// Set a u8 at the given offset.
    pub(crate) fn set_u8(mut self, offset: usize, val: u8) -> Self {
        self.buf[offset] = val;
        self
    }

    /// Set a u16 at the given offset (byte-order aware).
    pub(crate) fn set_u16(mut self, offset: usize, val: u16) -> Self {
        write_u16(&mut self.buf, offset, val, self.msb_first);
        self
    }

    /// Set a u32 at the given offset (byte-order aware).
    pub(crate) fn set_u32(mut self, offset: usize, val: u32) -> Self {
        write_u32(&mut self.buf, offset, val, self.msb_first);
        self
    }

    /// Copy raw bytes into the buffer at the given offset.
    pub(crate) fn set_bytes(mut self, offset: usize, data: &[u8]) -> Self {
        self.buf[offset..offset + data.len()].copy_from_slice(data);
        self
    }

    /// Get a mutable reference to the underlying buffer for bulk writes.
    pub(crate) fn buf_mut(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// Consume the builder and return the wire bytes.
    pub(crate) fn build(self) -> Vec<u8> {
        self.buf
    }
}

// Standalone byte-order-aware write functions (don't need &self).
#[inline]
fn write_u16(buf: &mut [u8], offset: usize, val: u16, msb_first: bool) {
    let bytes = if msb_first {
        val.to_be_bytes()
    } else {
        val.to_le_bytes()
    };
    buf[offset..offset + 2].copy_from_slice(&bytes);
}

#[inline]
fn write_u32(buf: &mut [u8], offset: usize, val: u32, msb_first: bool) {
    let bytes = if msb_first {
        val.to_be_bytes()
    } else {
        val.to_le_bytes()
    };
    buf[offset..offset + 4].copy_from_slice(&bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_reply_has_correct_header() {
        let reply = ReplyBuf::fixed(42, false).build();
        assert_eq!(reply.len(), 32);
        assert_eq!(reply[0], 1); // type = Reply
        assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 42); // seq
        assert_eq!(
            u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]),
            0
        ); // length
    }

    #[test]
    fn extra_reply_sets_length() {
        let reply = ReplyBuf::with_extra(1, 16, false).build();
        assert_eq!(reply.len(), 48); // 32 + 16
        assert_eq!(
            u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]),
            4
        ); // 16/4
    }

    #[test]
    fn set_u32_respects_byte_order() {
        let le = ReplyBuf::fixed(0, false).set_u32(8, 0x12345678).build();
        assert_eq!(&le[8..12], &[0x78, 0x56, 0x34, 0x12]);

        let be = ReplyBuf::fixed(0, true).set_u32(8, 0x12345678).build();
        assert_eq!(&be[8..12], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn data_byte_at_offset_1() {
        let reply = ReplyBuf::fixed(0, false).set_data_byte(24).build();
        assert_eq!(reply[1], 24);
    }
}
