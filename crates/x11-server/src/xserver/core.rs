//! Core constants, error builders, and event type definitions for the X11 server.

// Screen and root window constants
pub(crate) const ROOT_WINDOW: u32 = 0x00000062;
pub(crate) const OVERLAY_WINDOW: u32 = 0x00000063;
/// Dedicated window used for _NET_SUPPORTING_WM_CHECK (EWMH spec).
/// Both root and this window carry _NET_SUPPORTING_WM_CHECK pointing to this
/// window; this window also carries _NET_WM_NAME = "x11-web".
pub(crate) const WM_CHECK_WINDOW: u32 = 0x00000064;
/// Dedicated window for the XSETTINGS manager (_XSETTINGS_S0 selection owner).
pub(crate) const XSETTINGS_WINDOW: u32 = 0x00000012;
/// Dedicated window for the built-in XIM (X Input Method) server.
pub(crate) const XIM_WINDOW: u32 = 0x00000013;
pub(crate) const ROOT_VISUAL: u32 = VISUAL_TRUE_COLOR_24;
pub(crate) const ROOT_COLORMAP: u32 = 0x00000020;
pub(crate) const SCREEN_WIDTH: u16 = 1024;
pub(crate) const SCREEN_HEIGHT: u16 = 768;

// Visual IDs. These are server-internal assignments — they don't have to
// match any external table — but each one is referenced by colormap /
// depth-routing logic so they need stable names.
pub(crate) const VISUAL_TRUE_COLOR_24: u32 = 0x21;
pub(crate) const VISUAL_DIRECT_COLOR_24: u32 = 0x22;
pub(crate) const VISUAL_PSEUDO_COLOR_8: u32 = 0x23;
pub(crate) const VISUAL_TRUE_COLOR_16: u32 = 0x24;
pub(crate) const VISUAL_STATIC_GRAY_4: u32 = 0x25;
pub(crate) const VISUAL_GRAY_SCALE_8: u32 = 0x26;
pub(crate) const VISUAL_STATIC_COLOR_8: u32 = 0x27;
pub(crate) const VISUAL_TRUE_COLOR_ARGB_32: u32 = 0x40;

/// Per-client resource-ID base shift. Each connection gets a 22-bit ID
/// space (`(conn_index + 1) << RESOURCE_ID_BASE_SHIFT`); the low 22 bits
/// are owned by the client. Mirrors what is reported in the connection
/// setup `resource_id_mask`.
pub(crate) const RESOURCE_ID_BASE_SHIFT: u32 = 22;
/// Mask of the bits a client may set within its assigned resource-ID
/// space. Equal to `(1 << RESOURCE_ID_BASE_SHIFT) - 1`.
pub(crate) const RESOURCE_ID_MASK: u32 = (1 << RESOURCE_ID_BASE_SHIFT) - 1;

/// Maximum length (in 4-byte words) we advertise for BIG-REQUESTS — equals
/// 16 MiB, the largest power-of-two that fits comfortably under our buffer
/// limits while well above the spec's 256 KiB minimum.
pub(crate) const BIG_REQUESTS_MAX_LEN_WORDS: u32 = (16 * 1024 * 1024) / 4;

/// Wire size in bytes of a single core X11 event (always 32).
pub(crate) const X11_EVENT_SIZE: usize = 32;
/// Wire word size: every X11 wire field group is rounded up to a multiple
/// of this many bytes.
pub(crate) const X11_WORD_SIZE: usize = 4;

/// Round `n` up to the next 4-byte (X11 wire-word) boundary. X11 pads every
/// field group to a multiple of 4 bytes, so this is the canonical alignment
/// helper for request/reply length calculations.
#[inline]
pub(crate) const fn align_to_4(n: usize) -> usize {
    (n + 3) & !3
}

/// Map a visual ID to its pixel depth, matching the visual table in setup.rs.
/// Returns the root depth (24) for unknown visuals as a safe fallback.
pub(crate) fn depth_for_visual(visual: u32) -> u8 {
    match visual {
        VISUAL_TRUE_COLOR_ARGB_32 => 32,
        VISUAL_TRUE_COLOR_24 | VISUAL_DIRECT_COLOR_24 => 24,
        VISUAL_TRUE_COLOR_16 => 16,
        VISUAL_PSEUDO_COLOR_8 | VISUAL_GRAY_SCALE_8 | VISUAL_STATIC_COLOR_8 => 8,
        VISUAL_STATIC_GRAY_4 => 4,
        0 => 0,  // InputOnly windows
        _ => 24, // default to root depth
    }
}

// X11 event type codes — re-exported from x11rb-protocol (single source of truth).
pub(crate) use x11rb_protocol::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, CIRCULATE_NOTIFY_EVENT, CIRCULATE_REQUEST_EVENT,
    CLIENT_MESSAGE_EVENT, COLORMAP_NOTIFY_EVENT, CONFIGURE_NOTIFY_EVENT, CONFIGURE_REQUEST_EVENT,
    CREATE_NOTIFY_EVENT, DESTROY_NOTIFY_EVENT, ENTER_NOTIFY_EVENT, EXPOSE_EVENT, FOCUS_IN_EVENT,
    FOCUS_OUT_EVENT, GRAPHICS_EXPOSURE_EVENT, GRAVITY_NOTIFY_EVENT, KEYMAP_NOTIFY_EVENT,
    KEY_PRESS_EVENT, KEY_RELEASE_EVENT, LEAVE_NOTIFY_EVENT, MAPPING_NOTIFY_EVENT, MAP_NOTIFY_EVENT,
    MAP_REQUEST_EVENT, MOTION_NOTIFY_EVENT, NO_EXPOSURE_EVENT, PROPERTY_NOTIFY_EVENT,
    REPARENT_NOTIFY_EVENT, RESIZE_REQUEST_EVENT, SELECTION_CLEAR_EVENT, SELECTION_NOTIFY_EVENT,
    SELECTION_REQUEST_EVENT, UNMAP_NOTIFY_EVENT, VISIBILITY_NOTIFY_EVENT,
};
// Alias for British spelling used throughout codebase
pub(crate) const COLOURMAP_NOTIFY_EVENT: u8 = COLORMAP_NOTIFY_EVENT;

/// Top bit of a wire event-type byte: set on synthetic events delivered via
/// SendEvent. Used both to mark outgoing synthetic events and to strip the
/// flag when matching against the bare event type.
pub(crate) const SEND_EVENT_FLAG: u8 = 0x80;

// X11 event masks — re-exported directly from x11rb.
pub(crate) use x11rb_protocol::protocol::xproto::EventMask;

// X11 error codes — re-exported from x11rb-protocol.
#[cfg(test)]
use x11rb_protocol::protocol::xproto::IMPLEMENTATION_ERROR;
pub(crate) use x11rb_protocol::protocol::xproto::{
    ACCESS_ERROR, ALLOC_ERROR, ATOM_ERROR, COLORMAP_ERROR, CURSOR_ERROR, DRAWABLE_ERROR,
    FONT_ERROR, G_CONTEXT_ERROR, ID_CHOICE_ERROR, LENGTH_ERROR, MATCH_ERROR, NAME_ERROR,
    PIXMAP_ERROR, REQUEST_ERROR, VALUE_ERROR, WINDOW_ERROR,
};

/// Validate minimum request length; returns early with a LENGTH_ERROR error if too short.
///
/// Core handler usage:   `require_len!(data, 8, seq, opcode);`
/// Extension handler:    `require_len!(data, 12, seq, ext_opcode, minor, state.msb_first);`
macro_rules! require_len {
    ($data:expr, $min:expr, $seq:expr, $major:expr) => {
        if $data.len() < $min {
            return $crate::xserver::core::build_error(
                $crate::xserver::core::LENGTH_ERROR,
                $seq,
                0,
                $major,
                0,
            );
        }
    };
    ($data:expr, $min:expr, $seq:expr, $major:expr, $minor:expr, $msb:expr) => {
        if $data.len() < $min {
            let _ = $msb; // back-compat shim — byte order is patched at the write point
            return $crate::xserver::core::build_error(
                $crate::xserver::core::LENGTH_ERROR,
                $seq,
                $data.len() as u32,
                $major,
                $minor as u16,
            );
        }
    };
}
pub(crate) use require_len;

/// Build an X11 error reply (32 bytes) in canonical little-endian byte
/// order. The connection write loop calls [`byteswap_error_in_place`] on
/// the response before sending if the client negotiated MSB-first byte
/// order, so handlers don't need to know the client's byte order.
pub(crate) fn build_error(
    error_code: u8,
    seq: u16,
    bad_value: u32,
    major_opcode: u8,
    minor_opcode: u16,
) -> Vec<u8> {
    let mut err = [0u8; 32];
    err[0] = 0; // Error indicator
    err[1] = error_code;
    err[2..4].copy_from_slice(&seq.to_le_bytes());
    err[4..8].copy_from_slice(&bad_value.to_le_bytes());
    err[8..10].copy_from_slice(&minor_opcode.to_le_bytes());
    err[10] = major_opcode;
    err.to_vec()
}

/// Convert an LE-formatted X11 error reply (built by [`build_error`])
/// into MSB-first byte order in place. Safe to call on a 32-byte error
/// buffer; field offsets follow the X11 wire format (seq @ 2..4,
/// bad_value @ 4..8, minor_opcode @ 8..10).
pub(crate) fn byteswap_error_in_place(err: &mut [u8]) {
    if err.len() < 32 || err[0] != 0 {
        return;
    }
    err[2..4].reverse();
    err[4..8].reverse();
    err[8..10].reverse();
}

/// Helper to read a u16 from a buffer in the specified byte order.
#[inline]
pub(crate) fn read_u16_bo(data: &[u8], offset: usize, msb_first: bool) -> u16 {
    if msb_first {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    }
}

/// Helper to read a u32 from a buffer in the specified byte order.
#[inline]
pub(crate) fn read_u32_bo(data: &[u8], offset: usize, msb_first: bool) -> u32 {
    if msb_first {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    } else {
        u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }
}

/// Helper to read an i16 from a buffer in the specified byte order.
#[inline]
pub(crate) fn read_i16_bo(data: &[u8], offset: usize, msb_first: bool) -> i16 {
    if msb_first {
        i16::from_be_bytes([data[offset], data[offset + 1]])
    } else {
        i16::from_le_bytes([data[offset], data[offset + 1]])
    }
}

/// Helper to write u32 into a buffer in the specified byte order.
#[inline]
pub(crate) fn write_u32_bo(buf: &mut [u8], offset: usize, val: u32, msb_first: bool) {
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

    // -----------------------------------------------------------------------
    // read_u16_bo
    // -----------------------------------------------------------------------

    #[test]
    fn read_u16_bo_little_endian() {
        let buf = [0x34, 0x12, 0x00, 0x00];
        assert_eq!(read_u16_bo(&buf, 0, false), 0x1234);
    }

    #[test]
    fn read_u16_bo_big_endian() {
        let buf = [0x12, 0x34, 0x00, 0x00];
        assert_eq!(read_u16_bo(&buf, 0, true), 0x1234);
    }

    #[test]
    fn read_u16_bo_le_max() {
        let buf = [0xFF, 0xFF];
        assert_eq!(read_u16_bo(&buf, 0, false), u16::MAX);
    }

    #[test]
    fn read_u16_bo_be_max() {
        let buf = [0xFF, 0xFF];
        assert_eq!(read_u16_bo(&buf, 0, true), u16::MAX);
    }

    #[test]
    fn read_u16_bo_offset() {
        // Bytes at offset 2
        let buf = [0x00, 0x00, 0xAB, 0xCD];
        assert_eq!(read_u16_bo(&buf, 2, false), 0xCDAB);
        assert_eq!(read_u16_bo(&buf, 2, true), 0xABCD);
    }

    // -----------------------------------------------------------------------
    // read_u32_bo
    // -----------------------------------------------------------------------

    #[test]
    fn read_u32_bo_little_endian() {
        let buf = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_u32_bo(&buf, 0, false), 0x12345678);
    }

    #[test]
    fn read_u32_bo_big_endian() {
        let buf = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(read_u32_bo(&buf, 0, true), 0x12345678);
    }

    #[test]
    fn read_u32_bo_le_max() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_u32_bo(&buf, 0, false), u32::MAX);
    }

    #[test]
    fn read_u32_bo_be_max() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_u32_bo(&buf, 0, true), u32::MAX);
    }

    #[test]
    fn read_u32_bo_offset() {
        let buf = [0x00, 0x00, 0x04, 0x03, 0x02, 0x01];
        assert_eq!(read_u32_bo(&buf, 2, false), 0x01020304);
        assert_eq!(read_u32_bo(&buf, 2, true), 0x04030201);
    }

    // -----------------------------------------------------------------------
    // read_i16_bo
    // -----------------------------------------------------------------------

    #[test]
    fn read_i16_bo_little_endian_positive() {
        let buf = [0x2A, 0x00];
        assert_eq!(read_i16_bo(&buf, 0, false), 42i16);
    }

    #[test]
    fn read_i16_bo_big_endian_positive() {
        let buf = [0x00, 0x2A];
        assert_eq!(read_i16_bo(&buf, 0, true), 42i16);
    }

    #[test]
    fn read_i16_bo_le_min() {
        // i16::MIN = -32768 = 0x8000; in LE: [0x00, 0x80]
        let buf = [0x00, 0x80];
        assert_eq!(read_i16_bo(&buf, 0, false), i16::MIN);
    }

    #[test]
    fn read_i16_bo_be_min() {
        // i16::MIN = -32768 = 0x8000; in BE: [0x80, 0x00]
        let buf = [0x80, 0x00];
        assert_eq!(read_i16_bo(&buf, 0, true), i16::MIN);
    }

    #[test]
    fn read_i16_bo_le_max() {
        // i16::MAX = 32767 = 0x7FFF; in LE: [0xFF, 0x7F]
        let buf = [0xFF, 0x7F];
        assert_eq!(read_i16_bo(&buf, 0, false), i16::MAX);
    }

    #[test]
    fn read_i16_bo_be_max() {
        // i16::MAX = 32767 = 0x7FFF; in BE: [0x7F, 0xFF]
        let buf = [0x7F, 0xFF];
        assert_eq!(read_i16_bo(&buf, 0, true), i16::MAX);
    }

    #[test]
    fn read_i16_bo_negative() {
        // -1 = 0xFFFF; both LE and BE are [0xFF, 0xFF]
        let buf = [0xFF, 0xFF];
        assert_eq!(read_i16_bo(&buf, 0, false), -1i16);
        assert_eq!(read_i16_bo(&buf, 0, true), -1i16);
    }

    // -----------------------------------------------------------------------
    // write_u16_bo
    // -----------------------------------------------------------------------

    #[test]
    fn write_u16_bo_little_endian() {
        let mut buf = [0u8; 4];
        write_u16_bo(&mut buf, 0, 0x1234, false);
        assert_eq!(&buf[0..2], &[0x34, 0x12]);
    }

    #[test]
    fn write_u16_bo_big_endian() {
        let mut buf = [0u8; 4];
        write_u16_bo(&mut buf, 0, 0x1234, true);
        assert_eq!(&buf[0..2], &[0x12, 0x34]);
    }

    #[test]
    fn write_u16_bo_le_max() {
        let mut buf = [0u8; 2];
        write_u16_bo(&mut buf, 0, u16::MAX, false);
        assert_eq!(&buf, &[0xFF, 0xFF]);
    }

    #[test]
    fn write_u16_bo_be_max() {
        let mut buf = [0u8; 2];
        write_u16_bo(&mut buf, 0, u16::MAX, true);
        assert_eq!(&buf, &[0xFF, 0xFF]);
    }

    #[test]
    fn write_u16_bo_offset() {
        let mut buf = [0u8; 4];
        write_u16_bo(&mut buf, 2, 0xABCD, false);
        assert_eq!(&buf[2..4], &[0xCD, 0xAB]);
        write_u16_bo(&mut buf, 2, 0xABCD, true);
        assert_eq!(&buf[2..4], &[0xAB, 0xCD]);
    }

    // -----------------------------------------------------------------------
    // write_u32_bo
    // -----------------------------------------------------------------------

    #[test]
    fn write_u32_bo_little_endian() {
        let mut buf = [0u8; 4];
        write_u32_bo(&mut buf, 0, 0x12345678, false);
        assert_eq!(&buf, &[0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn write_u32_bo_big_endian() {
        let mut buf = [0u8; 4];
        write_u32_bo(&mut buf, 0, 0x12345678, true);
        assert_eq!(&buf, &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn write_u32_bo_le_max() {
        let mut buf = [0u8; 4];
        write_u32_bo(&mut buf, 0, u32::MAX, false);
        assert_eq!(&buf, &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn write_u32_bo_be_max() {
        let mut buf = [0u8; 4];
        write_u32_bo(&mut buf, 0, u32::MAX, true);
        assert_eq!(&buf, &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    // -----------------------------------------------------------------------
    // write_i16_bo
    // -----------------------------------------------------------------------

    #[test]
    fn write_i16_bo_little_endian_positive() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, 42i16, false);
        assert_eq!(&buf, &[0x2A, 0x00]);
    }

    #[test]
    fn write_i16_bo_big_endian_positive() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, 42i16, true);
        assert_eq!(&buf, &[0x00, 0x2A]);
    }

    #[test]
    fn write_i16_bo_le_min() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, i16::MIN, false);
        assert_eq!(&buf, &[0x00, 0x80]);
    }

    #[test]
    fn write_i16_bo_be_min() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, i16::MIN, true);
        assert_eq!(&buf, &[0x80, 0x00]);
    }

    #[test]
    fn write_i16_bo_le_max() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, i16::MAX, false);
        assert_eq!(&buf, &[0xFF, 0x7F]);
    }

    #[test]
    fn write_i16_bo_be_max() {
        let mut buf = [0u8; 2];
        write_i16_bo(&mut buf, 0, i16::MAX, true);
        assert_eq!(&buf, &[0x7F, 0xFF]);
    }

    // -----------------------------------------------------------------------
    // Round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_u16_le() {
        let values: &[u16] = &[0, 1, 0x1234, u16::MAX - 1, u16::MAX];
        let mut buf = [0u8; 2];
        for &v in values {
            write_u16_bo(&mut buf, 0, v, false);
            assert_eq!(
                read_u16_bo(&buf, 0, false),
                v,
                "round-trip LE u16 failed for {v}"
            );
        }
    }

    #[test]
    fn round_trip_u16_be() {
        let values: &[u16] = &[0, 1, 0x1234, u16::MAX - 1, u16::MAX];
        let mut buf = [0u8; 2];
        for &v in values {
            write_u16_bo(&mut buf, 0, v, true);
            assert_eq!(
                read_u16_bo(&buf, 0, true),
                v,
                "round-trip BE u16 failed for {v}"
            );
        }
    }

    #[test]
    fn round_trip_u32_le() {
        let values: &[u32] = &[0, 1, 0x12345678, u32::MAX - 1, u32::MAX];
        let mut buf = [0u8; 4];
        for &v in values {
            write_u32_bo(&mut buf, 0, v, false);
            assert_eq!(
                read_u32_bo(&buf, 0, false),
                v,
                "round-trip LE u32 failed for {v}"
            );
        }
    }

    #[test]
    fn round_trip_u32_be() {
        let values: &[u32] = &[0, 1, 0x12345678, u32::MAX - 1, u32::MAX];
        let mut buf = [0u8; 4];
        for &v in values {
            write_u32_bo(&mut buf, 0, v, true);
            assert_eq!(
                read_u32_bo(&buf, 0, true),
                v,
                "round-trip BE u32 failed for {v}"
            );
        }
    }

    #[test]
    fn round_trip_i16_le() {
        let values: &[i16] = &[0, 1, -1, i16::MIN, i16::MAX, 100, -100];
        let mut buf = [0u8; 2];
        for &v in values {
            write_i16_bo(&mut buf, 0, v, false);
            assert_eq!(
                read_i16_bo(&buf, 0, false),
                v,
                "round-trip LE i16 failed for {v}"
            );
        }
    }

    #[test]
    fn round_trip_i16_be() {
        let values: &[i16] = &[0, 1, -1, i16::MIN, i16::MAX, 100, -100];
        let mut buf = [0u8; 2];
        for &v in values {
            write_i16_bo(&mut buf, 0, v, true);
            assert_eq!(
                read_i16_bo(&buf, 0, true),
                v,
                "round-trip BE i16 failed for {v}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // build_error
    // -----------------------------------------------------------------------

    #[test]
    fn build_error_length() {
        let err = build_error(REQUEST_ERROR, 1, 0, 1, 0);
        assert_eq!(err.len(), 32, "error reply must be exactly 32 bytes");
    }

    #[test]
    fn build_error_byte0_is_zero() {
        let err = build_error(WINDOW_ERROR, 5, 42, 10, 3);
        assert_eq!(err[0], 0, "byte 0 must be 0 (error indicator)");
    }

    #[test]
    fn build_error_byte1_is_error_code() {
        let err = build_error(WINDOW_ERROR, 5, 42, 10, 3);
        assert_eq!(err[1], WINDOW_ERROR, "byte 1 must be the error code");
    }

    #[test]
    fn build_error_seq_bytes_2_3_little_endian() {
        // seq = 0x1234; LE: bytes [2..4] = [0x34, 0x12]
        let err = build_error(REQUEST_ERROR, 0x1234, 0, 1, 0);
        assert_eq!(err[2], 0x34);
        assert_eq!(err[3], 0x12);
    }

    #[test]
    fn build_error_bad_value_bytes_4_7_little_endian() {
        // bad_value = 0xDEADBEEF; LE: bytes [4..8] = [0xEF, 0xBE, 0xAD, 0xDE]
        let err = build_error(VALUE_ERROR, 1, 0xDEADBEEF, 2, 0);
        assert_eq!(&err[4..8], &[0xEF, 0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn build_error_minor_opcode_bytes_8_9_little_endian() {
        // minor_opcode = 0x0102; LE: bytes [8..10] = [0x02, 0x01]
        let err = build_error(REQUEST_ERROR, 1, 0, 5, 0x0102);
        assert_eq!(err[8], 0x02);
        assert_eq!(err[9], 0x01);
    }

    #[test]
    fn build_error_major_opcode_byte10() {
        let err = build_error(REQUEST_ERROR, 1, 0, 42, 0);
        assert_eq!(err[10], 42, "byte 10 must be the major opcode");
    }

    #[test]
    fn build_error_remaining_bytes_are_zero() {
        let err = build_error(REQUEST_ERROR, 1, 0, 1, 0);
        for i in 11..32 {
            assert_eq!(err[i], 0, "padding byte {i} must be zero");
        }
    }

    #[test]
    fn build_error_full_structure() {
        let err = build_error(ATOM_ERROR, 0xABCD, 0x11223344, 16, 7);
        assert_eq!(err[0], 0);
        assert_eq!(err[1], ATOM_ERROR);
        // seq 0xABCD LE
        assert_eq!(err[2], 0xCD);
        assert_eq!(err[3], 0xAB);
        // bad_value 0x11223344 LE
        assert_eq!(&err[4..8], &[0x44, 0x33, 0x22, 0x11]);
        // minor_opcode 7 LE
        assert_eq!(err[8], 0x07);
        assert_eq!(err[9], 0x00);
        // major_opcode 16
        assert_eq!(err[10], 16);
    }

    // -----------------------------------------------------------------------
    // build_error always emits LE; MSB clients get byteswap_error_in_place
    // applied at the connection write point. See those tests below.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Error codes are unique and in range 1–17
    // -----------------------------------------------------------------------

    #[test]
    fn error_codes_are_unique_and_in_range() {
        let codes: &[u8] = &[
            REQUEST_ERROR,
            VALUE_ERROR,
            WINDOW_ERROR,
            PIXMAP_ERROR,
            ATOM_ERROR,
            CURSOR_ERROR,
            FONT_ERROR,
            MATCH_ERROR,
            DRAWABLE_ERROR,
            ACCESS_ERROR,
            ALLOC_ERROR,
            COLORMAP_ERROR,
            G_CONTEXT_ERROR,
            ID_CHOICE_ERROR,
            NAME_ERROR,
            LENGTH_ERROR,
            IMPLEMENTATION_ERROR,
        ];
        // All 17 codes
        assert_eq!(codes.len(), 17);
        // Each is in [1, 17]
        for &c in codes {
            assert!(c >= 1 && c <= 17, "error code {c} is out of range [1, 17]");
        }
        // All unique
        let mut seen = std::collections::HashSet::new();
        for &c in codes {
            assert!(seen.insert(c), "error code {c} is duplicated");
        }
    }

    #[test]
    fn error_codes_match_x11_spec_values() {
        assert_eq!(REQUEST_ERROR, 1);
        assert_eq!(VALUE_ERROR, 2);
        assert_eq!(WINDOW_ERROR, 3);
        assert_eq!(PIXMAP_ERROR, 4);
        assert_eq!(ATOM_ERROR, 5);
        assert_eq!(CURSOR_ERROR, 6);
        assert_eq!(FONT_ERROR, 7);
        assert_eq!(MATCH_ERROR, 8);
        assert_eq!(DRAWABLE_ERROR, 9);
        assert_eq!(ACCESS_ERROR, 10);
        assert_eq!(ALLOC_ERROR, 11);
        assert_eq!(COLORMAP_ERROR, 12);
        assert_eq!(G_CONTEXT_ERROR, 13);
        assert_eq!(ID_CHOICE_ERROR, 14);
        assert_eq!(NAME_ERROR, 15);
        assert_eq!(LENGTH_ERROR, 16);
        assert_eq!(IMPLEMENTATION_ERROR, 17);
    }

    // -----------------------------------------------------------------------
    // depth_for_visual
    // -----------------------------------------------------------------------

    #[test]
    fn depth_for_visual_argb_32bit() {
        assert_eq!(depth_for_visual(0x40), 32);
    }

    #[test]
    fn depth_for_visual_root_truecolor_24bit() {
        assert_eq!(depth_for_visual(ROOT_VISUAL), 24);
        assert_eq!(depth_for_visual(0x21), 24);
    }

    #[test]
    fn depth_for_visual_directcolor_24bit() {
        assert_eq!(depth_for_visual(0x22), 24);
    }

    #[test]
    fn depth_for_visual_truecolor_16bit() {
        assert_eq!(depth_for_visual(0x24), 16);
    }

    #[test]
    fn depth_for_visual_pseudocolor_8bit() {
        assert_eq!(depth_for_visual(0x23), 8);
    }

    #[test]
    fn depth_for_visual_grayscale_8bit() {
        assert_eq!(depth_for_visual(0x26), 8);
    }

    #[test]
    fn depth_for_visual_staticcolor_8bit() {
        assert_eq!(depth_for_visual(0x27), 8);
    }

    #[test]
    fn depth_for_visual_staticgray_4bit() {
        assert_eq!(depth_for_visual(0x25), 4);
    }

    #[test]
    fn depth_for_visual_input_only_zero() {
        assert_eq!(depth_for_visual(0), 0);
    }

    #[test]
    fn depth_for_visual_unknown_defaults_to_24() {
        assert_eq!(depth_for_visual(0xFF), 24);
        assert_eq!(depth_for_visual(0x99), 24);
    }

    // -----------------------------------------------------------------------
    // build_error — validates X11 error packet format
    // -----------------------------------------------------------------------

    #[test]
    fn build_error_format() {
        let err = build_error(WINDOW_ERROR, 42, 0x12345678, 12, 0);
        assert_eq!(err.len(), 32);
        assert_eq!(err[0], 0); // Error indicator
        assert_eq!(err[1], WINDOW_ERROR); // Error code
        assert_eq!(u16::from_le_bytes([err[2], err[3]]), 42); // Sequence
        assert_eq!(
            u32::from_le_bytes([err[4], err[5], err[6], err[7]]),
            0x12345678
        ); // Bad value
        assert_eq!(err[10], 12); // Major opcode
    }

    #[test]
    fn build_error_bad_length() {
        let err = build_error(LENGTH_ERROR, 100, 0, 28, 0);
        assert_eq!(err[1], LENGTH_ERROR);
        assert_eq!(u16::from_le_bytes([err[2], err[3]]), 100);
        assert_eq!(err[10], 28);
    }

    // -----------------------------------------------------------------------
    // Event type codes — X11 protocol spec compliance
    // -----------------------------------------------------------------------

    #[test]
    fn event_types_match_x11_spec() {
        assert_eq!(KEY_PRESS_EVENT, 2);
        assert_eq!(KEY_RELEASE_EVENT, 3);
        assert_eq!(BUTTON_PRESS_EVENT, 4);
        assert_eq!(BUTTON_RELEASE_EVENT, 5);
        assert_eq!(MOTION_NOTIFY_EVENT, 6);
        assert_eq!(ENTER_NOTIFY_EVENT, 7);
        assert_eq!(LEAVE_NOTIFY_EVENT, 8);
        assert_eq!(FOCUS_IN_EVENT, 9);
        assert_eq!(FOCUS_OUT_EVENT, 10);
        assert_eq!(KEYMAP_NOTIFY_EVENT, 11);
        assert_eq!(EXPOSE_EVENT, 12);
        assert_eq!(GRAPHICS_EXPOSURE_EVENT, 13);
        assert_eq!(NO_EXPOSURE_EVENT, 14);
        assert_eq!(VISIBILITY_NOTIFY_EVENT, 15);
        assert_eq!(CREATE_NOTIFY_EVENT, 16);
        assert_eq!(DESTROY_NOTIFY_EVENT, 17);
        assert_eq!(UNMAP_NOTIFY_EVENT, 18);
        assert_eq!(MAP_NOTIFY_EVENT, 19);
        assert_eq!(MAP_REQUEST_EVENT, 20);
        assert_eq!(REPARENT_NOTIFY_EVENT, 21);
        assert_eq!(CONFIGURE_NOTIFY_EVENT, 22);
        assert_eq!(CONFIGURE_REQUEST_EVENT, 23);
        assert_eq!(GRAVITY_NOTIFY_EVENT, 24);
        assert_eq!(RESIZE_REQUEST_EVENT, 25);
        assert_eq!(CIRCULATE_NOTIFY_EVENT, 26);
        assert_eq!(CIRCULATE_REQUEST_EVENT, 27);
        assert_eq!(PROPERTY_NOTIFY_EVENT, 28);
        assert_eq!(SELECTION_CLEAR_EVENT, 29);
        assert_eq!(SELECTION_REQUEST_EVENT, 30);
        assert_eq!(SELECTION_NOTIFY_EVENT, 31);
        assert_eq!(COLOURMAP_NOTIFY_EVENT, 32);
        assert_eq!(CLIENT_MESSAGE_EVENT, 33);
        assert_eq!(MAPPING_NOTIFY_EVENT, 34);
    }

    #[test]
    fn all_33_event_types_are_contiguous() {
        // X11 spec defines event codes 2-34 (33 events)
        let events = [
            KEY_PRESS_EVENT,
            KEY_RELEASE_EVENT,
            BUTTON_PRESS_EVENT,
            BUTTON_RELEASE_EVENT,
            MOTION_NOTIFY_EVENT,
            ENTER_NOTIFY_EVENT,
            LEAVE_NOTIFY_EVENT,
            FOCUS_IN_EVENT,
            FOCUS_OUT_EVENT,
            KEYMAP_NOTIFY_EVENT,
            EXPOSE_EVENT,
            GRAPHICS_EXPOSURE_EVENT,
            NO_EXPOSURE_EVENT,
            VISIBILITY_NOTIFY_EVENT,
            CREATE_NOTIFY_EVENT,
            DESTROY_NOTIFY_EVENT,
            UNMAP_NOTIFY_EVENT,
            MAP_NOTIFY_EVENT,
            MAP_REQUEST_EVENT,
            REPARENT_NOTIFY_EVENT,
            CONFIGURE_NOTIFY_EVENT,
            CONFIGURE_REQUEST_EVENT,
            GRAVITY_NOTIFY_EVENT,
            RESIZE_REQUEST_EVENT,
            CIRCULATE_NOTIFY_EVENT,
            CIRCULATE_REQUEST_EVENT,
            PROPERTY_NOTIFY_EVENT,
            SELECTION_CLEAR_EVENT,
            SELECTION_REQUEST_EVENT,
            SELECTION_NOTIFY_EVENT,
            COLOURMAP_NOTIFY_EVENT,
            CLIENT_MESSAGE_EVENT,
            MAPPING_NOTIFY_EVENT,
        ];
        assert_eq!(events.len(), 33);
        for (i, &ev) in events.iter().enumerate() {
            assert_eq!(
                ev,
                (i as u8) + 2,
                "event at index {i} should be {}",
                (i as u8) + 2
            );
        }
    }

    // -----------------------------------------------------------------------
    // Root window and visual constants
    // -----------------------------------------------------------------------

    #[test]
    fn root_window_constants_are_distinct() {
        let ids = [
            ROOT_WINDOW,
            OVERLAY_WINDOW,
            WM_CHECK_WINDOW,
            XSETTINGS_WINDOW,
            XIM_WINDOW,
        ];
        let mut seen = std::collections::HashSet::new();
        for &id in &ids {
            assert!(seen.insert(id), "window ID {id:#x} is duplicated");
        }
    }

    #[test]
    fn root_visual_and_colormap_are_non_zero() {
        assert_ne!(ROOT_VISUAL, 0);
        assert_ne!(ROOT_COLORMAP, 0);
    }
}
