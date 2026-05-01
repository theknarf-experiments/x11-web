//! Graphics Context state and bitmap clip mask types.

/// Resolved bitmap clip mask data extracted from a pixmap.
/// When `clip_mask` is set to a 1-bit-depth pixmap, we cache the bitmap
/// here so drawing operations can test individual pixels.
#[derive(Clone, Debug)]
pub(crate) struct ClipMaskBitmap {
    /// Width of the mask in pixels.
    pub(crate) width: u16,
    /// Height of the mask in pixels.
    pub(crate) height: u16,
    /// Row-major bitmap: one bit per pixel, packed 8 pixels per byte,
    /// LSB-first within each byte.  Row stride = (width + 7) / 8.
    pub(crate) bits: Vec<u8>,
}

impl ClipMaskBitmap {
    /// Convert the bitmap mask to a list of clip rectangles (run-length encoded
    /// by row).  The rectangles are offset by (origin_x, origin_y) so they can
    /// be used directly as GC clip rectangles.
    pub(crate) fn to_clip_rects(&self, origin_x: i16, origin_y: i16) -> Vec<(i16, i16, u16, u16)> {
        let stride = (self.width as usize).div_ceil(8);
        let mut rects = Vec::new();
        for y in 0..self.height as usize {
            let mut x = 0usize;
            while x < self.width as usize {
                // Skip 0-bits
                let byte_idx = y * stride + x / 8;
                let bit_idx = x % 8;
                if byte_idx >= self.bits.len() || self.bits[byte_idx] & (1 << bit_idx) == 0 {
                    x += 1;
                    continue;
                }
                // Found a 1-bit, start a run
                let run_start = x;
                while x < self.width as usize {
                    let bi = y * stride + x / 8;
                    let bx = x % 8;
                    if bi >= self.bits.len() || self.bits[bi] & (1 << bx) == 0 {
                        break;
                    }
                    x += 1;
                }
                rects.push((
                    run_start as i16 + origin_x,
                    y as i16 + origin_y,
                    (x - run_start) as u16,
                    1,
                ));
            }
        }
        rects
    }
}

/// Full X11 Graphics Context state per the spec.
#[derive(Clone)]
pub(crate) struct GcState {
    pub(crate) function: u8,
    pub(crate) plane_mask: u32,
    pub(crate) foreground: u32,
    pub(crate) background: u32,
    pub(crate) line_width: u16,
    pub(crate) line_style: u8,
    pub(crate) cap_style: u8,
    pub(crate) join_style: u8,
    pub(crate) fill_style: u8,
    pub(crate) fill_rule: u8,
    pub(crate) tile: u32,
    pub(crate) stipple: u32,
    pub(crate) ts_x: i16,
    pub(crate) ts_y: i16,
    pub(crate) font_id: u32,
    pub(crate) subwindow_mode: u8,
    pub(crate) graphics_exposures: bool,
    pub(crate) clip_x: i16,
    pub(crate) clip_y: i16,
    pub(crate) clip_mask: u32,
    pub(crate) dash_offset: u16,
    pub(crate) dashes: u8,
    pub(crate) arc_mode: u8,
    /// Clip rectangles set by SetClipRectangles (empty = no clipping).
    pub(crate) clip_rects: Vec<(i16, i16, u16, u16)>,
    /// Resolved bitmap clip mask (from `clip_mask` pixmap).
    /// When set, only pixels corresponding to 1-bits are drawn.
    /// The mask is offset by `clip_x`/`clip_y` relative to the drawable.
    pub(crate) clip_mask_bitmap: Option<ClipMaskBitmap>,
    /// Dash pattern set by SetDashes (empty = use `dashes` field as uniform length).
    pub(crate) dash_list: Vec<u8>,
}

impl Default for GcState {
    fn default() -> Self {
        Self {
            function: 3, // GXcopy
            plane_mask: 0xFFFFFFFF,
            foreground: 0x00_00_00,
            background: 0xFF_FF_FF,
            line_width: 0,
            line_style: 0, // Solid
            cap_style: 1,  // Butt
            join_style: 0, // Miter
            fill_style: 0, // Solid
            fill_rule: 0,  // EvenOdd
            tile: 0,
            stipple: 0,
            ts_x: 0,
            ts_y: 0,
            font_id: 0,
            subwindow_mode: 0, // ClipByChildren
            graphics_exposures: true,
            clip_x: 0,
            clip_y: 0,
            clip_mask: 0, // None
            dash_offset: 0,
            dashes: 4,
            arc_mode: 1, // PieSlice
            clip_rects: Vec::new(),
            clip_mask_bitmap: None,
            dash_list: Vec::new(),
        }
    }
}

impl GcState {}
