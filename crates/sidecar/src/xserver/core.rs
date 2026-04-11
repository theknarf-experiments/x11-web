//! Core constants, error builders, and event type definitions for the X11 server.

// Screen and root window constants
pub(crate) const ROOT_WINDOW: u32 = 0x00000062;
pub(crate) const ROOT_VISUAL: u32 = 0x00000021;
pub(crate) const ROOT_COLORMAP: u32 = 0x00000020;
pub(crate) const SCREEN_WIDTH: u16 = 1024;
pub(crate) const SCREEN_HEIGHT: u16 = 768;

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
pub(crate) const CIRCULATE_NOTIFY_EVENT: u8 = 26;
pub(crate) const CIRCULATE_REQUEST_EVENT: u8 = 27;
pub(crate) const PROPERTY_NOTIFY_EVENT: u8 = 28;
pub(crate) const SELECTION_CLEAR_EVENT: u8 = 29;
pub(crate) const SELECTION_REQUEST_EVENT: u8 = 30;
pub(crate) const SELECTION_NOTIFY_EVENT: u8 = 31;
pub(crate) const MAPPING_NOTIFY_EVENT: u8 = 34;

// X11 event masks
pub(crate) const KEY_PRESS_MASK: u32 = 0x0000_0001;
pub(crate) const KEY_RELEASE_MASK: u32 = 0x0000_0002;
pub(crate) const BUTTON_PRESS_MASK: u32 = 0x0000_0004;
pub(crate) const BUTTON_RELEASE_MASK: u32 = 0x0000_0008;
pub(crate) const ENTER_WINDOW_MASK: u32 = 0x0000_0010;
pub(crate) const LEAVE_WINDOW_MASK: u32 = 0x0000_0020;
pub(crate) const POINTER_MOTION_MASK: u32 = 0x0000_0040;
pub(crate) const EXPOSURE_MASK: u32 = 0x0000_8000;
pub(crate) const STRUCTURE_NOTIFY_MASK: u32 = 0x0002_0000;
pub(crate) const SUBSTRUCTURE_NOTIFY_MASK: u32 = 0x0008_0000;
pub(crate) const SUBSTRUCTURE_REDIRECT_MASK: u32 = 0x0010_0000;
pub(crate) const PROPERTY_CHANGE_MASK: u32 = 0x0040_0000;

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

/// Build an X11 error reply (32 bytes).
pub(crate) fn build_error(error_code: u8, seq: u16, bad_value: u32, major_opcode: u8, minor_opcode: u16) -> Vec<u8> {
    let mut err = [0u8; 32];
    err[0] = 0; // Error indicator
    err[1] = error_code;
    err[2..4].copy_from_slice(&seq.to_le_bytes());
    err[4..8].copy_from_slice(&bad_value.to_le_bytes());
    err[8..10].copy_from_slice(&minor_opcode.to_le_bytes());
    err[10] = major_opcode;
    err.to_vec()
}
