//! Cursor metadata and pointer barrier types.

/// Cursor metadata stored for RecolorCursor and bitmap cursor rendering.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct CursorInfo {
    pub(crate) css_name: String,
    pub(crate) source_pixmap: u32,
    pub(crate) mask_pixmap: u32,
    pub(crate) fore_red: u16,
    pub(crate) fore_green: u16,
    pub(crate) fore_blue: u16,
    pub(crate) back_red: u16,
    pub(crate) back_green: u16,
    pub(crate) back_blue: u16,
    pub(crate) hotspot_x: u16,
    pub(crate) hotspot_y: u16,
    /// Pre-rendered ARGB pixel data for bitmap cursors (empty for glyph cursors).
    pub(crate) argb_data: Vec<u8>,
    /// Width of the cursor bitmap (0 for glyph cursors).
    pub(crate) width: u16,
    /// Height of the cursor bitmap (0 for glyph cursors).
    pub(crate) height: u16,
    /// XFIXES cursor name (SetCursorName).
    pub(crate) name: String,
    /// Animation frames for animated cursors (from CreateAnimCursor).
    /// Each entry is (argb_data, width, height, hotspot_x, hotspot_y, delay_ms).
    pub(crate) anim_frames: Vec<(Vec<u8>, u16, u16, u16, u16, u32)>,
}

/// XFIXES pointer barrier (CreatePointerBarrier).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct PointerBarrier {
    pub(crate) barrier_id: u32,
    pub(crate) window: u32,
    pub(crate) x1: i16,
    pub(crate) y1: i16,
    pub(crate) x2: i16,
    pub(crate) y2: i16,
    pub(crate) directions: u32,
    pub(crate) device_ids: Vec<u16>,
}
