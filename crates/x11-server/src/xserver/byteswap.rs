//! Small byte-swap primitives that didn't have a natural home elsewhere.
//!
//! Inbound MSB-first requests used to be pre-swapped to LE by a big
//! per-opcode dispatcher in this module before being handed to the
//! parsers; that's gone now. The codegen-emitted
//! `try_parse_endian_request` reads the wire fields in the client's
//! negotiated byte order directly, so no in-place conversion happens
//! before parsing.
//!
//! The four helpers below survive because a couple of hand-rolled
//! reply byteswappers still call them. Once those are migrated to the
//! generator's `SerializeEndian` impls, this module can disappear.

/// Swap a u16 at the given offset in place.
#[inline]
pub(crate) fn swap_u16(buf: &mut [u8], off: usize) {
    if off + 2 <= buf.len() {
        buf[off..off + 2].reverse();
    }
}

/// Swap a u32 at the given offset in place.
#[inline]
pub(crate) fn swap_u32(buf: &mut [u8], off: usize) {
    if off + 4 <= buf.len() {
        buf[off..off + 4].reverse();
    }
}

/// Swap N consecutive u32s starting at `off`.
#[inline]
pub(crate) fn swap_u32_array(buf: &mut [u8], off: usize, count: usize) {
    for i in 0..count {
        swap_u32(buf, off + i * 4);
    }
}

/// Swap N consecutive u16s starting at `off`.
#[inline]
pub(crate) fn swap_u16_array(buf: &mut [u8], off: usize, count: usize) {
    for i in 0..count {
        swap_u16(buf, off + i * 2);
    }
}

// The per-opcode MSB→LE pre-swap dispatcher that used to live here
// (`byteswap_request_in_place` plus ~70 helpers) is gone. Inbound
// request parsing now goes through the codegen-emitted
// `try_parse_endian_request` (see `parse_minor!` in `handlers/mod.rs`),
// which reads multi-byte fields in the client's negotiated byte order
// directly. The four `swap_*` helpers above are kept because dbe.rs
// still uses them for a hand-rolled GetVisualInfoReply byteswap;
// migrating that handler to `SerializeEndian` removes the last user.
