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
pub(crate) const ROOT_VISUAL: u32 = 0x00000021;
pub(crate) const ROOT_COLORMAP: u32 = 0x00000020;
pub(crate) const SCREEN_WIDTH: u16 = 1024;
pub(crate) const SCREEN_HEIGHT: u16 = 768;

/// Map a visual ID to its pixel depth, matching the visual table in setup.rs.
/// Returns the root depth (24) for unknown visuals as a safe fallback.
pub(crate) fn depth_for_visual(visual: u32) -> u8 {
    match visual {
        0x40 => 32,                     // TrueColor ARGB
        0x21 | 0x22 => 24,             // TrueColor / DirectColor 24-bit
        0x24 => 16,                     // TrueColor 16-bit
        0x23 | 0x26 | 0x27 => 8,       // PseudoColor / GrayScale / StaticColor
        0x25 => 4,                      // StaticGray 4-bit
        0 => 0,                         // InputOnly windows
        _ => 24,                        // default to root depth
    }
}

// X11 event type codes
pub(crate) const KEY_PRESS_EVENT: u8 = 2;
pub(crate) const KEY_RELEASE_EVENT: u8 = 3;
pub(crate) const BUTTON_PRESS_EVENT: u8 = 4;
pub(crate) const BUTTON_RELEASE_EVENT: u8 = 5;
pub(crate) const MOTION_NOTIFY_EVENT: u8 = 6;
pub(crate) const ENTER_NOTIFY_EVENT: u8 = 7;
pub(crate) const LEAVE_NOTIFY_EVENT: u8 = 8;
pub(crate) const FOCUS_IN_EVENT: u8 = 9;
pub(crate) const FOCUS_OUT_EVENT: u8 = 10;
pub(crate) const KEYMAP_NOTIFY_EVENT: u8 = 11;
pub(crate) const EXPOSE_EVENT: u8 = 12;
pub(crate) const VISIBILITY_NOTIFY_EVENT: u8 = 15;
pub(crate) const CREATE_NOTIFY_EVENT: u8 = 16;
pub(crate) const DESTROY_NOTIFY_EVENT: u8 = 17;
pub(crate) const UNMAP_NOTIFY_EVENT: u8 = 18;
pub(crate) const MAP_NOTIFY_EVENT: u8 = 19;
pub(crate) const MAP_REQUEST_EVENT: u8 = 20;
pub(crate) const REPARENT_NOTIFY_EVENT: u8 = 21;
pub(crate) const CONFIGURE_NOTIFY_EVENT: u8 = 22;
pub(crate) const CONFIGURE_REQUEST_EVENT: u8 = 23;
pub(crate) const GRAVITY_NOTIFY_EVENT: u8 = 24;
pub(crate) const RESIZE_REQUEST_EVENT: u8 = 25;
pub(crate) const CIRCULATE_NOTIFY_EVENT: u8 = 26;
pub(crate) const CIRCULATE_REQUEST_EVENT: u8 = 27;
pub(crate) const PROPERTY_NOTIFY_EVENT: u8 = 28;
pub(crate) const SELECTION_CLEAR_EVENT: u8 = 29;
pub(crate) const SELECTION_REQUEST_EVENT: u8 = 30;
pub(crate) const SELECTION_NOTIFY_EVENT: u8 = 31;
pub(crate) const COLOURMAP_NOTIFY_EVENT: u8 = 32;
pub(crate) const CLIENT_MESSAGE_EVENT: u8 = 33;
pub(crate) const MAPPING_NOTIFY_EVENT: u8 = 34;

// X11 event masks (complete per X11 protocol spec)
pub(crate) const KEY_PRESS_MASK: u32 = 0x0000_0001;
pub(crate) const KEY_RELEASE_MASK: u32 = 0x0000_0002;
pub(crate) const BUTTON_PRESS_MASK: u32 = 0x0000_0004;
pub(crate) const BUTTON_RELEASE_MASK: u32 = 0x0000_0008;
pub(crate) const ENTER_WINDOW_MASK: u32 = 0x0000_0010;
pub(crate) const LEAVE_WINDOW_MASK: u32 = 0x0000_0020;
pub(crate) const POINTER_MOTION_MASK: u32 = 0x0000_0040;
pub(crate) const POINTER_MOTION_HINT_MASK: u32 = 0x0000_0080;
pub(crate) const BUTTON1_MOTION_MASK: u32 = 0x0000_0100;
pub(crate) const BUTTON2_MOTION_MASK: u32 = 0x0000_0200;
pub(crate) const BUTTON3_MOTION_MASK: u32 = 0x0000_0400;
pub(crate) const BUTTON4_MOTION_MASK: u32 = 0x0000_0800;
pub(crate) const BUTTON5_MOTION_MASK: u32 = 0x0000_1000;
pub(crate) const BUTTON_MOTION_MASK: u32 = 0x0000_2000;
pub(crate) const KEYMAP_STATE_MASK: u32 = 0x0000_4000;
pub(crate) const EXPOSURE_MASK: u32 = 0x0000_8000;
pub(crate) const VISIBILITY_CHANGE_MASK: u32 = 0x0001_0000;
pub(crate) const STRUCTURE_NOTIFY_MASK: u32 = 0x0002_0000;
pub(crate) const RESIZE_REDIRECT_MASK: u32 = 0x0004_0000;
pub(crate) const SUBSTRUCTURE_NOTIFY_MASK: u32 = 0x0008_0000;
pub(crate) const SUBSTRUCTURE_REDIRECT_MASK: u32 = 0x0010_0000;
pub(crate) const FOCUS_CHANGE_MASK: u32 = 0x0020_0000;
pub(crate) const PROPERTY_CHANGE_MASK: u32 = 0x0040_0000;
pub(crate) const COLOURMAP_CHANGE_MASK: u32 = 0x0080_0000;
pub(crate) const OWNER_GRAB_BUTTON_MASK: u32 = 0x0100_0000;

// X11 error codes
pub(crate) const BAD_REQUEST: u8 = 1;
pub(crate) const BAD_VALUE: u8 = 2;
pub(crate) const BAD_WINDOW: u8 = 3;
pub(crate) const BAD_PIXMAP: u8 = 4;
pub(crate) const BAD_ATOM: u8 = 5;
pub(crate) const BAD_CURSOR: u8 = 6;
pub(crate) const BAD_FONT: u8 = 7;
pub(crate) const BAD_MATCH: u8 = 8;
pub(crate) const BAD_DRAWABLE: u8 = 9;
pub(crate) const BAD_ACCESS: u8 = 10;
pub(crate) const BAD_ALLOC: u8 = 11;
pub(crate) const BAD_COLOR: u8 = 12;
pub(crate) const BAD_GC: u8 = 13;
pub(crate) const BAD_ID_CHOICE: u8 = 14;
pub(crate) const BAD_NAME: u8 = 15;
pub(crate) const BAD_LENGTH: u8 = 16;
pub(crate) const BAD_IMPLEMENTATION: u8 = 17;

/// Validate minimum request length; returns early with a BAD_LENGTH error if too short.
///
/// Core handler usage:   `require_len!(data, 8, seq, opcode);`
/// Extension handler:    `require_len!(data, 12, seq, ext_opcode, minor, state.msb_first);`
macro_rules! require_len {
    ($data:expr, $min:expr, $seq:expr, $major:expr) => {
        if $data.len() < $min {
            return $crate::xserver::core::build_error(
                $crate::xserver::core::BAD_LENGTH, $seq, 0, $major, 0,
            );
        }
    };
    ($data:expr, $min:expr, $seq:expr, $major:expr, $minor:expr, $msb:expr) => {
        if $data.len() < $min {
            return $crate::xserver::core::build_error_bo(
                $crate::xserver::core::BAD_LENGTH, $seq, $data.len() as u32,
                $major, $minor as u16, $msb,
            );
        }
    };
}
pub(crate) use require_len;

/// Build an X11 error reply (32 bytes) in little-endian byte order.
pub(crate) fn build_error(error_code: u8, seq: u16, bad_value: u32, major_opcode: u8, minor_opcode: u16) -> Vec<u8> {
    build_error_bo(error_code, seq, bad_value, major_opcode, minor_opcode, false)
}

/// Build an X11 error reply (32 bytes) with specified byte order.
pub(crate) fn build_error_bo(error_code: u8, seq: u16, bad_value: u32, major_opcode: u8, minor_opcode: u16, msb_first: bool) -> Vec<u8> {
    let mut err = [0u8; 32];
    err[0] = 0; // Error indicator
    err[1] = error_code;
    if msb_first {
        err[2..4].copy_from_slice(&seq.to_be_bytes());
        err[4..8].copy_from_slice(&bad_value.to_be_bytes());
        err[8..10].copy_from_slice(&minor_opcode.to_be_bytes());
    } else {
        err[2..4].copy_from_slice(&seq.to_le_bytes());
        err[4..8].copy_from_slice(&bad_value.to_le_bytes());
        err[8..10].copy_from_slice(&minor_opcode.to_le_bytes());
    }
    err[10] = major_opcode;
    err.to_vec()
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
        u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    } else {
        u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
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

/// Helper to write u16 into a buffer in the specified byte order.
#[inline]
pub(crate) fn write_u16_bo(buf: &mut [u8], offset: usize, val: u16, msb_first: bool) {
    let bytes = if msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
    buf[offset..offset + 2].copy_from_slice(&bytes);
}

/// Helper to write u32 into a buffer in the specified byte order.
#[inline]
pub(crate) fn write_u32_bo(buf: &mut [u8], offset: usize, val: u32, msb_first: bool) {
    let bytes = if msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
    buf[offset..offset + 4].copy_from_slice(&bytes);
}

/// Helper to write i16 into a buffer in the specified byte order.
#[inline]
pub(crate) fn write_i16_bo(buf: &mut [u8], offset: usize, val: i16, msb_first: bool) {
    let bytes = if msb_first { val.to_be_bytes() } else { val.to_le_bytes() };
    buf[offset..offset + 2].copy_from_slice(&bytes);
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
            assert_eq!(read_u16_bo(&buf, 0, false), v, "round-trip LE u16 failed for {v}");
        }
    }

    #[test]
    fn round_trip_u16_be() {
        let values: &[u16] = &[0, 1, 0x1234, u16::MAX - 1, u16::MAX];
        let mut buf = [0u8; 2];
        for &v in values {
            write_u16_bo(&mut buf, 0, v, true);
            assert_eq!(read_u16_bo(&buf, 0, true), v, "round-trip BE u16 failed for {v}");
        }
    }

    #[test]
    fn round_trip_u32_le() {
        let values: &[u32] = &[0, 1, 0x12345678, u32::MAX - 1, u32::MAX];
        let mut buf = [0u8; 4];
        for &v in values {
            write_u32_bo(&mut buf, 0, v, false);
            assert_eq!(read_u32_bo(&buf, 0, false), v, "round-trip LE u32 failed for {v}");
        }
    }

    #[test]
    fn round_trip_u32_be() {
        let values: &[u32] = &[0, 1, 0x12345678, u32::MAX - 1, u32::MAX];
        let mut buf = [0u8; 4];
        for &v in values {
            write_u32_bo(&mut buf, 0, v, true);
            assert_eq!(read_u32_bo(&buf, 0, true), v, "round-trip BE u32 failed for {v}");
        }
    }

    #[test]
    fn round_trip_i16_le() {
        let values: &[i16] = &[0, 1, -1, i16::MIN, i16::MAX, 100, -100];
        let mut buf = [0u8; 2];
        for &v in values {
            write_i16_bo(&mut buf, 0, v, false);
            assert_eq!(read_i16_bo(&buf, 0, false), v, "round-trip LE i16 failed for {v}");
        }
    }

    #[test]
    fn round_trip_i16_be() {
        let values: &[i16] = &[0, 1, -1, i16::MIN, i16::MAX, 100, -100];
        let mut buf = [0u8; 2];
        for &v in values {
            write_i16_bo(&mut buf, 0, v, true);
            assert_eq!(read_i16_bo(&buf, 0, true), v, "round-trip BE i16 failed for {v}");
        }
    }

    // -----------------------------------------------------------------------
    // build_error
    // -----------------------------------------------------------------------

    #[test]
    fn build_error_length() {
        let err = build_error(BAD_REQUEST, 1, 0, 1, 0);
        assert_eq!(err.len(), 32, "error reply must be exactly 32 bytes");
    }

    #[test]
    fn build_error_byte0_is_zero() {
        let err = build_error(BAD_WINDOW, 5, 42, 10, 3);
        assert_eq!(err[0], 0, "byte 0 must be 0 (error indicator)");
    }

    #[test]
    fn build_error_byte1_is_error_code() {
        let err = build_error(BAD_WINDOW, 5, 42, 10, 3);
        assert_eq!(err[1], BAD_WINDOW, "byte 1 must be the error code");
    }

    #[test]
    fn build_error_seq_bytes_2_3_little_endian() {
        // seq = 0x1234; LE: bytes [2..4] = [0x34, 0x12]
        let err = build_error(BAD_REQUEST, 0x1234, 0, 1, 0);
        assert_eq!(err[2], 0x34);
        assert_eq!(err[3], 0x12);
    }

    #[test]
    fn build_error_bad_value_bytes_4_7_little_endian() {
        // bad_value = 0xDEADBEEF; LE: bytes [4..8] = [0xEF, 0xBE, 0xAD, 0xDE]
        let err = build_error(BAD_VALUE, 1, 0xDEADBEEF, 2, 0);
        assert_eq!(&err[4..8], &[0xEF, 0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn build_error_minor_opcode_bytes_8_9_little_endian() {
        // minor_opcode = 0x0102; LE: bytes [8..10] = [0x02, 0x01]
        let err = build_error(BAD_REQUEST, 1, 0, 5, 0x0102);
        assert_eq!(err[8], 0x02);
        assert_eq!(err[9], 0x01);
    }

    #[test]
    fn build_error_major_opcode_byte10() {
        let err = build_error(BAD_REQUEST, 1, 0, 42, 0);
        assert_eq!(err[10], 42, "byte 10 must be the major opcode");
    }

    #[test]
    fn build_error_remaining_bytes_are_zero() {
        let err = build_error(BAD_REQUEST, 1, 0, 1, 0);
        for i in 11..32 {
            assert_eq!(err[i], 0, "padding byte {i} must be zero");
        }
    }

    #[test]
    fn build_error_full_structure() {
        let err = build_error(BAD_ATOM, 0xABCD, 0x11223344, 16, 7);
        assert_eq!(err[0], 0);
        assert_eq!(err[1], BAD_ATOM);
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
    // build_error_bo (MSB / big-endian mode)
    // -----------------------------------------------------------------------

    #[test]
    fn build_error_bo_msb_length() {
        let err = build_error_bo(BAD_REQUEST, 1, 0, 1, 0, true);
        assert_eq!(err.len(), 32);
    }

    #[test]
    fn build_error_bo_msb_byte0_zero() {
        let err = build_error_bo(BAD_WINDOW, 5, 42, 10, 3, true);
        assert_eq!(err[0], 0);
    }

    #[test]
    fn build_error_bo_msb_seq_big_endian() {
        // seq = 0x1234; BE: bytes [2..4] = [0x12, 0x34]
        let err = build_error_bo(BAD_REQUEST, 0x1234, 0, 1, 0, true);
        assert_eq!(err[2], 0x12);
        assert_eq!(err[3], 0x34);
    }

    #[test]
    fn build_error_bo_msb_bad_value_big_endian() {
        // bad_value = 0x11223344; BE: bytes [4..8] = [0x11, 0x22, 0x33, 0x44]
        let err = build_error_bo(BAD_VALUE, 1, 0x11223344, 2, 0, true);
        assert_eq!(&err[4..8], &[0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn build_error_bo_msb_minor_opcode_big_endian() {
        // minor_opcode = 0x0102; BE: bytes [8..10] = [0x01, 0x02]
        let err = build_error_bo(BAD_REQUEST, 1, 0, 5, 0x0102, true);
        assert_eq!(err[8], 0x01);
        assert_eq!(err[9], 0x02);
    }

    #[test]
    fn build_error_bo_msb_major_opcode_byte10() {
        let err = build_error_bo(BAD_REQUEST, 1, 0, 77, 0, true);
        assert_eq!(err[10], 77);
    }

    // -----------------------------------------------------------------------
    // Error codes are unique and in range 1–17
    // -----------------------------------------------------------------------

    #[test]
    fn error_codes_are_unique_and_in_range() {
        let codes: &[u8] = &[
            BAD_REQUEST, BAD_VALUE, BAD_WINDOW, BAD_PIXMAP, BAD_ATOM,
            BAD_CURSOR, BAD_FONT, BAD_MATCH, BAD_DRAWABLE, BAD_ACCESS,
            BAD_ALLOC, BAD_COLOR, BAD_GC, BAD_ID_CHOICE, BAD_NAME,
            BAD_LENGTH, BAD_IMPLEMENTATION,
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
        assert_eq!(BAD_REQUEST, 1);
        assert_eq!(BAD_VALUE, 2);
        assert_eq!(BAD_WINDOW, 3);
        assert_eq!(BAD_PIXMAP, 4);
        assert_eq!(BAD_ATOM, 5);
        assert_eq!(BAD_CURSOR, 6);
        assert_eq!(BAD_FONT, 7);
        assert_eq!(BAD_MATCH, 8);
        assert_eq!(BAD_DRAWABLE, 9);
        assert_eq!(BAD_ACCESS, 10);
        assert_eq!(BAD_ALLOC, 11);
        assert_eq!(BAD_COLOR, 12);
        assert_eq!(BAD_GC, 13);
        assert_eq!(BAD_ID_CHOICE, 14);
        assert_eq!(BAD_NAME, 15);
        assert_eq!(BAD_LENGTH, 16);
        assert_eq!(BAD_IMPLEMENTATION, 17);
    }
}
