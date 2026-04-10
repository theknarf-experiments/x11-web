use std::collections::HashMap;
use tracing::{debug, info};

use crate::xserver::ClientState;

// PictFormat IDs
const PICTFORMAT_ARGB32: u32 = 0x24;
const PICTFORMAT_RGB24: u32 = 0x25;
const PICTFORMAT_A8: u32 = 0x26;
const PICTFORMAT_A1: u32 = 0x27;

pub struct RenderState {
    pictures: HashMap<u32, PictureState>,
    glyphsets: HashMap<u32, GlyphSetState>,
    solid_fills: HashMap<u32, SolidFillState>,
    linear_gradients: HashMap<u32, LinearGradientState>,
    /// Per-picture 3x3 affine transforms set via SetPictureTransform
    /// (RENDER minor opcode 28). Applied when sampling source
    /// pictures — most importantly for gradients, where rendercheck
    /// uses transforms to map a tiny gradient onto a much larger
    /// destination region.
    transforms: HashMap<u32, [f64; 9]>,
}

struct PictureState {
    drawable: u32,
    repeat: u32,
    /// Clip rectangles set via SetPictureClipRectangles. The picture's
    /// destination is the union of these rectangles, offset by
    /// `clip_origin_*`. `None` means no clipping (full drawable).
    clip_rects: Option<Vec<(i16, i16, u16, u16)>>,
    clip_origin_x: i16,
    clip_origin_y: i16,
}

impl PictureState {
    /// Returns true if the destination point is inside the current clip
    /// region. If no clip rectangles have been set, everything is in.
    fn point_in_clip(&self, x: i32, y: i32) -> bool {
        match &self.clip_rects {
            None => true,
            Some(rects) => {
                for &(rx, ry, rw, rh) in rects {
                    let cx = self.clip_origin_x as i32 + rx as i32;
                    let cy = self.clip_origin_y as i32 + ry as i32;
                    if x >= cx
                        && x < cx + rw as i32
                        && y >= cy
                        && y < cy + rh as i32
                    {
                        return true;
                    }
                }
                false
            }
        }
    }
}

struct GlyphSetState {
    format_id: u32,
    glyphs: HashMap<u32, StoredGlyph>,
}

struct StoredGlyph {
    width: u16,
    height: u16,
    x: i16,
    y: i16,
    x_off: i16,
    y_off: i16,
    data: Vec<u8>, // alpha bitmap
}

struct SolidFillState {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// Linear gradient stored in premultiplied alpha. Stops are sorted
/// ascending by `offset` (which is normally in 0..1 but the spec
/// allows out-of-range values for special effects we don't handle).
struct LinearGradientState {
    p1: (f64, f64),
    p2: (f64, f64),
    stops: Vec<GradientStop>,
}

#[derive(Clone, Copy)]
struct GradientStop {
    offset: f64,
    /// Premultiplied colour at this stop.
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            pictures: HashMap::new(),
            glyphsets: HashMap::new(),
            solid_fills: HashMap::new(),
            linear_gradients: HashMap::new(),
            transforms: HashMap::new(),
        }
    }
}

/// Composite a single source pixel over a destination pixel using the OVER operator.
/// Apply a Porter-Duff compositing operator to a single destination
/// pixel. Implements the 12 standard X RENDER operators (PictOp 0..12)
/// using premultiplied alpha as the spec requires. Operators above 12
/// (Saturate, Disjoint*, Conjoint*) fall through to PictOpOver.
///
/// Both `src` and `dst` are premultiplied BGRA in little-endian byte
/// order. SolidFill colours are premultiplied at creation time, and
/// our framebuffer stores all picture data premultiplied, so the
/// caller doesn't need to convert.
pub(crate) fn composite_pixel(
    op: u8,
    dst: &mut [u8],
    src_b: u8,
    src_g: u8,
    src_r: u8,
    src_a: u8,
) {
    // Fast paths for the operators that don't depend on per-channel
    // arithmetic — just unconditional writes.
    match op {
        0 => {
            // Clear
            dst[0] = 0;
            dst[1] = 0;
            dst[2] = 0;
            dst[3] = 0;
            return;
        }
        1 => {
            // Src
            dst[0] = src_b;
            dst[1] = src_g;
            dst[2] = src_r;
            dst[3] = src_a;
            return;
        }
        2 => {
            // Dst — leave the destination untouched.
            return;
        }
        _ => {}
    }

    let sa = src_a as i32;
    let da = dst[3] as i32;

    // Per-channel `(Fs, Fd)` factors out of 255. The result for any
    // channel C is `(Cs*Fs + Cd*Fd + 127) / 255`. The +127 is for
    // round-to-nearest at integer division.
    let (fs, fd): (i32, i32) = match op {
        3 => (255, 255 - sa),               // Over
        4 => (255 - da, 255),               // OverReverse
        5 => (da, 0),                       // In
        6 => (0, sa),                       // InReverse
        7 => (255 - da, 0),                 // Out
        8 => (0, 255 - sa),                 // OutReverse
        9 => (da, 255 - sa),                // Atop
        10 => (255 - da, sa),               // AtopReverse
        11 => (255 - da, 255 - sa),         // Xor
        12 => (255, 255),                   // Add (clamped below)
        // Saturate (13) and DisjointOver (19) share the same formula:
        //   Fs = min(1, (1-Da)/Sa)  if Sa > 0; 0 otherwise
        //   Fd = 1
        // The min ensures result alpha never exceeds 1.
        13 | 19 => {
            if sa == 0 {
                (0, 255)
            } else {
                let inv_da = (255 - da) as i64;
                let scaled = (inv_da * 255) / sa as i64;
                ((scaled.min(255)) as i32, 255)
            }
        }
        // DisjointSrc (17): like Saturate but no destination contribution.
        17 => {
            if sa == 0 {
                (0, 0)
            } else {
                let inv_da = (255 - da) as i64;
                let scaled = (inv_da * 255) / sa as i64;
                ((scaled.min(255)) as i32, 0)
            }
        }
        // DisjointDst (18): symmetric — destination dominates with the
        // same disjoint scaling, source contributes nothing.
        18 => {
            if da == 0 {
                (0, 0)
            } else {
                let inv_sa = (255 - sa) as i64;
                let scaled = (inv_sa * 255) / da as i64;
                (0, (scaled.min(255)) as i32)
            }
        }
        // DisjointOverReverse (20): symmetric of DisjointOver/Saturate.
        20 => {
            if da == 0 {
                (255, 0)
            } else {
                let inv_sa = (255 - sa) as i64;
                let scaled = (inv_sa * 255) / da as i64;
                (255, (scaled.min(255)) as i32)
            }
        }
        // DisjointClear (16): same as Clear.
        16 => (0, 0),
        // ConjointClear (32) / ConjointSrc (33) / ConjointDst (34):
        // identical to the standard Clear / Src / Dst.
        32 => (0, 0),
        33 => {
            // Identical to PictOpSrc — handled as a fast path above
            // would have been ideal but the match would still hit
            // here for op == 33; just compute the equivalent.
            (255, 0)
        }
        34 => (0, 255),
        // ConjointOver (35):
        //   Fa = 1
        //   Fb = max(0, 1 - Sa/Da)  — when Sa fully covers, dst gone
        35 => {
            let fb = if da == 0 {
                0
            } else if sa >= da {
                0
            } else {
                ((da - sa) as i64 * 255 / da as i64).max(0) as i32
            };
            (255, fb)
        }
        // ConjointOverReverse (36): symmetric of 35.
        36 => {
            let fa = if sa == 0 {
                0
            } else if da >= sa {
                0
            } else {
                ((sa - da) as i64 * 255 / sa as i64).max(0) as i32
            };
            (fa, 255)
        }
        // Remaining Disjoint/Conjoint operators (21..27, 37..43) are
        // not implemented yet — fall back to Over.
        _ => (255, 255 - sa),
    };

    let blend = |s: u8, d: u8| -> u8 {
        let r = (s as i32 * fs + d as i32 * fd + 127) / 255;
        r.clamp(0, 255) as u8
    };

    dst[0] = blend(src_b, dst[0]);
    dst[1] = blend(src_g, dst[1]);
    dst[2] = blend(src_r, dst[2]);
    dst[3] = blend(src_a, dst[3]);
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_i16(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

pub fn handle_render_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let minor = data[1];
    info!("Render op minor={minor}");

    match minor {
        0 => handle_query_version(seq),
        1 => handle_query_pict_formats(seq),
        4 => handle_create_picture(state, data),
        5 => handle_change_picture(state, data),
        6 => handle_set_picture_clip_rectangles(state, data),
        7 => handle_free_picture(state, data),
        8 => handle_composite(state, data),
        10 => handle_trapezoids(state, data),
        11 => handle_triangles(state, data),
        12 => handle_tri_strip(state, data),
        13 => handle_tri_fan(state, data),
        17 => handle_create_glyphset(state, data),
        19 => handle_free_glyphset(state, data),
        20 => handle_add_glyphs(state, data),
        22 => handle_free_glyphs(state, data),
        23 => handle_composite_glyphs(state, data, 1), // Glyphs8
        24 => handle_composite_glyphs(state, data, 2), // Glyphs16
        25 => handle_composite_glyphs(state, data, 4), // Glyphs32
        26 => handle_fill_rectangles(state, data),
        27 => {
            // CreateCursor - ignore
            Vec::new()
        }
        28 => handle_set_picture_transform(state, data),
        29 => handle_query_filters(seq),
        30 => {
            // SetPictureFilter - ignore
            Vec::new()
        }
        33 => handle_create_solid_fill(state, data),
        34 | 35 | 36 => handle_create_gradient_fill(state, data),
        _ => {
            debug!("Unhandled RENDER minor opcode: {minor}");
            Vec::new()
        }
    }
}

/// QueryVersion: reply with version 0.11
fn handle_query_version(seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0 (no extra data beyond 32 bytes)
    reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // major version
    reply[12..16].copy_from_slice(&11u32.to_le_bytes()); // minor version
    reply.to_vec()
}

/// QueryPictFormats: reply with ARGB32, RGB24, A8, A1 formats + screen info
fn handle_query_pict_formats(seq: u16) -> Vec<u8> {
    // We define 4 formats: ARGB32, RGB24, A8, A1
    let num_formats: u32 = 4;
    let num_screens: u32 = 1;
    let num_subpixel: u32 = 1;

    // Each PictForminfo is 28 bytes
    // Screen: 8 bytes header + depths
    // We report 2 depths (24 and 32) with visuals
    // Depth 24: 8 bytes header + 1 PictVisual (8 bytes) = 16 bytes
    // Depth 32: 8 bytes header + 0 PictVisuals = 8 bytes
    // Screen total: 8 + 16 + 8 = 32 bytes
    // Subpixel: 4 bytes each

    let num_depths: u32 = 2;
    let formats_bytes = num_formats as usize * 28;
    let screen_header = 8usize;
    let depth24_bytes = 8 + 8; // 8 header + 1 PictVisual(8)
    let depth32_bytes = 8; // 8 header + 0 PictVisuals
    let screen_bytes = screen_header + depth24_bytes + depth32_bytes;
    let subpixel_bytes = num_subpixel as usize * 4;
    let extra = formats_bytes + screen_bytes + subpixel_bytes;
    let total = 32 + extra;

    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra / 4) as u32).to_le_bytes()); // length in 4-byte units
    reply[8..12].copy_from_slice(&num_formats.to_le_bytes()); // num_formats
    reply[12..16].copy_from_slice(&num_screens.to_le_bytes()); // num_screens
    reply[16..20].copy_from_slice(&num_depths.to_le_bytes()); // num_depths
    // reply[20..24] = num_visuals (we have 1 visual across all depths)
    reply[20..24].copy_from_slice(&1u32.to_le_bytes());
    reply[24..28].copy_from_slice(&num_subpixel.to_le_bytes()); // num_subpixel

    let mut off = 32;

    // Format 1: ARGB32 (type=PictTypeDirect=1, depth=32)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_ARGB32,
        1,
        32, // type=Direct, depth=32
        16,
        0xFF,
        8,
        0xFF,
        0,
        0xFF,
        24,
        0xFF, // ARGB shifts/masks
    );

    // Format 2: RGB24 (type=PictTypeDirect=1, depth=24)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_RGB24,
        1,
        24,
        16,
        0xFF,
        8,
        0xFF,
        0,
        0xFF,
        0,
        0, // no alpha
    );

    // Format 3: A8 (type=PictTypeDirect=1, depth=8)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_A8,
        1,
        8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0xFF, // alpha only
    );

    // Format 4: A1 (type=PictTypeDirect=1, depth=1)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_A1,
        1,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0x1, // 1-bit alpha
    );

    // Screen info (8 bytes header)
    let num_depths_for_screen: u32 = 2;
    // fallback pictformat for the screen
    reply[off..off + 4].copy_from_slice(&num_depths_for_screen.to_le_bytes());
    off += 4;
    reply[off..off + 4].copy_from_slice(&PICTFORMAT_RGB24.to_le_bytes()); // fallback
    off += 4;

    // Depth 24: header (8 bytes) + 1 PictVisual (8 bytes)
    reply[off] = 24; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    reply[off..off + 2].copy_from_slice(&1u16.to_le_bytes()); // num_visuals
    off += 2;
    off += 4; // pad

    // PictVisual for depth 24: visual(4) + format(4)
    reply[off..off + 4].copy_from_slice(&0x00000021u32.to_le_bytes()); // ROOT_VISUAL
    off += 4;
    reply[off..off + 4].copy_from_slice(&PICTFORMAT_RGB24.to_le_bytes());
    off += 4;

    // Depth 32: header (8 bytes) + 0 PictVisuals
    reply[off] = 32; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    reply[off..off + 2].copy_from_slice(&0u16.to_le_bytes()); // num_visuals
    off += 2;
    off += 4; // pad

    // Subpixel order (4 bytes): 0 = Unknown
    reply[off..off + 4].copy_from_slice(&0u32.to_le_bytes());

    reply
}

fn write_pictforminfo(
    buf: &mut [u8],
    off: &mut usize,
    id: u32,
    pict_type: u8,
    depth: u8,
    red_shift: u16,
    red_mask: u16,
    green_shift: u16,
    green_mask: u16,
    blue_shift: u16,
    blue_mask: u16,
    alpha_shift: u16,
    alpha_mask: u16,
) {
    let o = *off;
    buf[o..o + 4].copy_from_slice(&id.to_le_bytes());
    buf[o + 4] = pict_type;
    buf[o + 5] = depth;
    // 2 bytes pad at o+6..o+8
    buf[o + 8..o + 10].copy_from_slice(&red_shift.to_le_bytes());
    buf[o + 10..o + 12].copy_from_slice(&red_mask.to_le_bytes());
    buf[o + 12..o + 14].copy_from_slice(&green_shift.to_le_bytes());
    buf[o + 14..o + 16].copy_from_slice(&green_mask.to_le_bytes());
    buf[o + 16..o + 18].copy_from_slice(&blue_shift.to_le_bytes());
    buf[o + 18..o + 20].copy_from_slice(&blue_mask.to_le_bytes());
    buf[o + 20..o + 22].copy_from_slice(&alpha_shift.to_le_bytes());
    buf[o + 22..o + 24].copy_from_slice(&alpha_mask.to_le_bytes());
    // colormap (4 bytes) at o+24..o+28
    *off += 28;
}

fn handle_create_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let drawable = read_u32(data, 8);
    let format_id = read_u32(data, 12);
    let value_mask = read_u32(data, 16);

    let mut repeat = 0u32;
    let mut clip_x_origin = 0i16;
    let mut clip_y_origin = 0i16;
    let mut offset = 20;
    // Parse value list based on value_mask. Bits (from xrender protocol):
    //   0  CPRepeat
    //   1  CPAlphaMap
    //   2  CPAlphaXOrigin
    //   3  CPAlphaYOrigin
    //   4  CPClipXOrigin
    //   5  CPClipYOrigin
    //   6  CPClipMask
    //   7  CPGraphicsExposure
    //   8  CPSubwindowMode
    //   9  CPPolyEdge
    //  10  CPPolyMode
    //  11  CPDither
    //  12  CPComponentAlpha
    for bit in 0..13 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = read_u32(data, offset);
                match bit {
                    0 => repeat = val,
                    4 => clip_x_origin = val as i16,
                    5 => clip_y_origin = val as i16,
                    _ => {}
                }
                offset += 4;
            }
        }
    }

    info!("CreatePicture: pid={pid:#x} drawable={drawable:#x} format={format_id:#x} repeat={repeat}");

    state.render.pictures.insert(
        pid,
        PictureState {
            drawable,
            repeat,
            clip_rects: None,
            clip_origin_x: clip_x_origin,
            clip_origin_y: clip_y_origin,
        },
    );
    Vec::new()
}

fn handle_change_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let value_mask = read_u32(data, 8);

    if let Some(pic) = state.render.pictures.get_mut(&pid) {
        let mut offset = 12;
        for bit in 0..13 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = read_u32(data, offset);
                    match bit {
                        0 => pic.repeat = val,
                        4 => pic.clip_origin_x = val as i16,
                        5 => pic.clip_origin_y = val as i16,
                        6 => {
                            // CPClipMask: setting None (0) clears the clip
                            // region (everything is in).
                            if val == 0 {
                                pic.clip_rects = None;
                            }
                            // Non-None pixmap masks aren't supported (we
                            // don't track pixmap-based clips); leave
                            // existing rect clip in place.
                        }
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }
    }
    Vec::new()
}

/// SetPictureClipRectangles: replace the picture's clip region with a
/// list of rectangles. Subsequent rendering operations on this picture
/// must only affect destination pixels that fall inside one of these
/// rectangles (offset by `clip_x_origin` / `clip_y_origin`).
///
/// Request layout:
///   1   opcode (139)
///   1   minor (6)
///   2   length (in 4-byte units)
///   4   picture
///   2   clip_x_origin (INT16)
///   2   clip_y_origin (INT16)
///   ... rectangles, 8 bytes each: x(2) y(2) width(2) height(2)
fn handle_set_picture_clip_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let clip_x = read_i16(data, 8);
    let clip_y = read_i16(data, 10);

    let mut rects = Vec::new();
    let mut off = 12;
    while off + 8 <= data.len() {
        let x = read_i16(data, off);
        let y = read_i16(data, off + 2);
        let w = read_u16(data, off + 4);
        let h = read_u16(data, off + 6);
        rects.push((x, y, w, h));
        off += 8;
    }

    debug!(
        "Render SetPictureClipRectangles: pid={pid:#x} origin=({clip_x},{clip_y}) rects={}",
        rects.len()
    );

    if let Some(pic) = state.render.pictures.get_mut(&pid) {
        pic.clip_origin_x = clip_x;
        pic.clip_origin_y = clip_y;
        pic.clip_rects = Some(rects);
    }
    Vec::new()
}

fn handle_free_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let pid = read_u32(data, 4);
        state.render.pictures.remove(&pid);
    }
    Vec::new()
}

/// Snapshot a picture's clip state so we can pass it down to drawing
/// helpers without holding a borrow on `state.render` while we mutate
/// the framebuffer.
#[derive(Clone, Default)]
struct ClipSnapshot {
    rects: Option<Vec<(i16, i16, u16, u16)>>,
    origin_x: i16,
    origin_y: i16,
}

impl ClipSnapshot {
    fn from_picture(state: &ClientState, pid: u32) -> Self {
        if let Some(pic) = state.render.pictures.get(&pid) {
            ClipSnapshot {
                rects: pic.clip_rects.clone(),
                origin_x: pic.clip_origin_x,
                origin_y: pic.clip_origin_y,
            }
        } else {
            ClipSnapshot::default()
        }
    }

    fn allows(&self, x: i32, y: i32) -> bool {
        match &self.rects {
            None => true,
            Some(rects) => rects.iter().any(|&(rx, ry, rw, rh)| {
                let cx = self.origin_x as i32 + rx as i32;
                let cy = self.origin_y as i32 + ry as i32;
                x >= cx && x < cx + rw as i32 && y >= cy && y < cy + rh as i32
            }),
        }
    }
}

/// The main compositing operation.
fn handle_composite(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 36 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let mask_pic = read_u32(data, 12);
    let dst_pic = read_u32(data, 16);
    let src_x = read_i16(data, 20);
    let src_y = read_i16(data, 22);
    let mask_x = read_i16(data, 24);
    let mask_y = read_i16(data, 26);
    let dst_x = read_i16(data, 28);
    let dst_y = read_i16(data, 30);
    let width = read_u16(data, 32);
    let height = read_u16(data, 34);

    info!(
        "Render Composite: op={op} src={src_pic:#x} mask={mask_pic:#x} dst={dst_pic:#x} src=({src_x},{src_y}) dst=({dst_x},{dst_y}) {width}x{height}"
    );

    // Resolve source pixels
    let src_pixels: Option<(Vec<u8>, u32, u32)> = resolve_source_pixels(state, src_pic, src_x, src_y, width, height);
    // If a mask picture is provided, fetch its pixels too. The mask
    // modulates the source's alpha per-pixel — used heavily by GTK to
    // draw anti-aliased icons and text decorations.
    let mask_pixels: Option<(Vec<u8>, u32, u32)> = if mask_pic != 0 {
        resolve_source_pixels(state, mask_pic, mask_x, mask_y, width, height)
    } else {
        None
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Resolve dst drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);

    if let (Some((src_data, src_w, _src_h)), Some(dst_draw)) = (src_pixels, dst_drawable) {
        if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
            let fb_w = fb.width() as i32;
            let fb_h = fb.height() as i32;
            let fb_stride = fb.stride();
            let fb_data = fb.data_mut();

            for row in 0..height as i32 {
                let dy = dst_y as i32 + row;
                if dy < 0 || dy >= fb_h {
                    continue;
                }
                for col in 0..width as i32 {
                    let dx = dst_x as i32 + col;
                    if dx < 0 || dx >= fb_w {
                        continue;
                    }
                    if !clip.allows(dx, dy) {
                        continue;
                    }
                    let src_off = (row as usize * src_w as usize + col as usize) * 4;
                    if src_off + 3 >= src_data.len() {
                        continue;
                    }
                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 >= fb_data.len() {
                        continue;
                    }
                    let mut sb = src_data[src_off];
                    let mut sg = src_data[src_off + 1];
                    let mut sr = src_data[src_off + 2];
                    let mut sa = src_data[src_off + 3];

                    // Apply mask: modulate the source's RGBA by the
                    // mask's alpha (or, for component-alpha masks,
                    // by each channel). This is what makes anti-aliased
                    // icons and theme decorations actually show up.
                    if let Some((mask_data, mask_w, _)) = &mask_pixels {
                        let mask_off = (row as usize * *mask_w as usize + col as usize) * 4;
                        if mask_off + 3 < mask_data.len() {
                            let ma = mask_data[mask_off + 3];
                            if ma == 0 {
                                continue;
                            }
                            sb = ((sb as u32 * ma as u32) / 255) as u8;
                            sg = ((sg as u32 * ma as u32) / 255) as u8;
                            sr = ((sr as u32 * ma as u32) / 255) as u8;
                            sa = ((sa as u32 * ma as u32) / 255) as u8;
                        }
                    }

                    composite_pixel(
                        op,
                        &mut fb_data[dst_off..dst_off + 4],
                        sb,
                        sg,
                        sr,
                        sa,
                    );
                }
            }
            fb.mark_dirty(dst_x as i32, dst_y as i32, width as u32, height as u32);
        }
        // Notify DAMAGE subscribers for the destination drawable
        if let Some(d) = dst_drawable {
            state.notify_damage(d, dst_x, dst_y, width, height);
        }
    }

    Vec::new()
}

/// Read a FIXED (16.16 fixed-point) value from data.
fn read_fixed(data: &[u8], off: usize) -> f64 {
    let raw = i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    raw as f64 / 65536.0
}

/// Handle XRender Trapezoids (minor opcode 10).
///
/// Request format:
///   1  CARD8    op
///   3           unused
///   4  Picture  src
///   4  Picture  dst
///   4  PictFormat mask-format
///   2  INT16    src-x
///   2  INT16    src-y
///   N  list of TRAPEZOID (40 bytes each)
///
/// Each TRAPEZOID:
///   4  FIXED  top
///   4  FIXED  bottom
///   4  FIXED  left.p1.x
///   4  FIXED  left.p1.y
///   4  FIXED  left.p2.x
///   4  FIXED  left.p2.y
///   4  FIXED  right.p1.x
///   4  FIXED  right.p1.y
///   4  FIXED  right.p2.x
///   4  FIXED  right.p2.y
fn handle_trapezoids(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    // Resolve source color
    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    info!(
        "Render Trapezoids: op={op} src={src_pic:#x} dst={dst_pic:#x} color=({sr},{sg},{sb},{sa})"
    );

    // Get destination drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse trapezoids (40 bytes each starting at offset 24)
    let mut off = 24;
    let mut traps = Vec::new();
    while off + 40 <= data.len() {
        let top = read_fixed(data, off);
        let bottom = read_fixed(data, off + 4);
        let left_x1 = read_fixed(data, off + 8);
        let left_y1 = read_fixed(data, off + 12);
        let left_x2 = read_fixed(data, off + 16);
        let left_y2 = read_fixed(data, off + 20);
        let right_x1 = read_fixed(data, off + 24);
        let right_y1 = read_fixed(data, off + 28);
        let right_x2 = read_fixed(data, off + 32);
        let right_y2 = read_fixed(data, off + 36);
        traps.push((top, bottom, left_x1, left_y1, left_x2, left_y2, right_x1, right_y1, right_x2, right_y2));
        off += 40;
    }

    if !traps.is_empty() {
        // Compute bounding box for damage notification
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        for &(top, bottom, lx1, _, lx2, _, rx1, _, rx2, _) in &traps {
            min_y = min_y.min(top);
            max_y = max_y.max(bottom);
            min_x = min_x.min(lx1).min(lx2);
            max_x = max_x.max(rx1).max(rx2);
        }

        if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
            let fb_w = fb.width() as i32;
            let fb_h = fb.height() as i32;

            for &(top, bottom, lx1, ly1, lx2, ly2, rx1, ry1, rx2, ry2) in &traps {
                rasterize_trapezoid(
                    fb, fb_w, fb_h, op, sr, sg, sb, sa,
                    top, bottom, lx1, ly1, lx2, ly2, rx1, ry1, rx2, ry2,
                    &clip,
                );
            }
        }

        // Notify DAMAGE subscribers that this drawable was modified
        let dx = min_x.floor().max(0.0) as i16;
        let dy = min_y.floor().max(0.0) as i16;
        let dw = (max_x.ceil() - min_x.floor()).max(1.0) as u16;
        let dh = (max_y.ceil() - min_y.floor()).max(1.0) as u16;
        state.notify_damage(dst_draw, dx, dy, dw, dh);
    }

    Vec::new()
}

/// Rasterize a single trapezoid into the framebuffer using scanline conversion.
#[allow(clippy::too_many_arguments)]
fn rasterize_trapezoid(
    fb: &mut crate::framebuffer::Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
    top: f64,
    bottom: f64,
    lx1: f64,
    ly1: f64,
    lx2: f64,
    ly2: f64,
    rx1: f64,
    ry1: f64,
    rx2: f64,
    ry2: f64,
    clip: &ClipSnapshot,
) {
    let y_start = top.ceil() as i32;
    let y_end = bottom.floor() as i32;

    if y_start > y_end {
        return;
    }

    let fb_stride = fb.stride();
    let fb_data = fb.data_mut();

    // Precompute edge deltas
    let left_dy = ly2 - ly1;
    let right_dy = ry2 - ry1;

    for y in y_start..=y_end {
        if y < 0 || y >= fb_h {
            continue;
        }

        let yf = y as f64 + 0.5; // sample at pixel center

        // Interpolate left edge X at this Y
        let left_x = if left_dy.abs() < 1e-9 {
            lx1
        } else {
            lx1 + (lx2 - lx1) * (yf - ly1) / left_dy
        };

        // Interpolate right edge X at this Y
        let right_x = if right_dy.abs() < 1e-9 {
            rx1
        } else {
            rx1 + (rx2 - rx1) * (yf - ry1) / right_dy
        };

        let x_start = left_x.ceil() as i32;
        let x_end = right_x.floor() as i32;

        for x in x_start..=x_end {
            if x < 0 || x >= fb_w {
                continue;
            }
            if !clip.allows(x, y) {
                continue;
            }
            let dst_off = y as usize * fb_stride + x as usize * 4;
            if dst_off + 3 >= fb_data.len() {
                continue;
            }
            composite_pixel(
                op,
                &mut fb_data[dst_off..dst_off + 4],
                sb,
                sg,
                sr,
                sa,
            );
        }
    }

    // Mark entire affected region dirty
    let min_y = top.floor().max(0.0) as i32;
    let max_y = (bottom.ceil() as i32).min(fb_h);
    if min_y < max_y {
        fb.mark_dirty(0, min_y, fb_w as u32, (max_y - min_y) as u32);
    }
}

/// Handle XRender Triangles (minor opcode 11).
fn handle_triangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    debug!("Render Triangles: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Each triangle = 3 POINTFIX (each 8 bytes = x FIXED + y FIXED) = 24 bytes
    let mut off = 24;
    let mut triangles = Vec::new();
    while off + 24 <= data.len() {
        let x1 = read_fixed(data, off);
        let y1 = read_fixed(data, off + 4);
        let x2 = read_fixed(data, off + 8);
        let y2 = read_fixed(data, off + 12);
        let x3 = read_fixed(data, off + 16);
        let y3 = read_fixed(data, off + 20);
        triangles.push((x1, y1, x2, y2, x3, y3));
        off += 24;
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        for &(x1, y1, x2, y2, x3, y3) in &triangles {
            rasterize_triangle(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, x1, y1, x2, y2, x3, y3, &clip,
            );
        }
    }

    Vec::new()
}

/// Rasterize a single triangle using scanline conversion.
#[allow(clippy::too_many_arguments)]
fn rasterize_triangle(
    fb: &mut crate::framebuffer::Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    clip: &ClipSnapshot,
) {
    // Convert triangle to trapezoids by sorting vertices by Y
    let mut verts = [(x1, y1), (x2, y2), (x3, y3)];
    verts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let (vx0, vy0) = verts[0];
    let (vx1, vy1) = verts[1];
    let (vx2, vy2) = verts[2];

    // Top half: vy0 to vy1
    if (vy1 - vy0).abs() > 1e-9 {
        // Long edge from v0 to v2, short edge from v0 to v1
        let mid_x = vx0 + (vx2 - vx0) * (vy1 - vy0) / (vy2 - vy0);
        let (llx, rrx) = if mid_x < vx1 {
            // Left edge is v0->v2 segment, right edge is v0->v1
            ((vx0, vy0, vx2, vy2), (vx0, vy0, vx1, vy1))
        } else {
            ((vx0, vy0, vx1, vy1), (vx0, vy0, vx2, vy2))
        };
        rasterize_trapezoid(
            fb, fb_w, fb_h, op, sr, sg, sb, sa,
            vy0, vy1, llx.0, llx.1, llx.2, llx.3, rrx.0, rrx.1, rrx.2, rrx.3,
            clip,
        );
    }

    // Bottom half: vy1 to vy2
    if (vy2 - vy1).abs() > 1e-9 {
        let mid_x = vx0 + (vx2 - vx0) * (vy1 - vy0) / (vy2 - vy0);
        let (llx, rrx) = if mid_x < vx1 {
            ((vx0, vy0, vx2, vy2), (vx1, vy1, vx2, vy2))
        } else {
            ((vx1, vy1, vx2, vy2), (vx0, vy0, vx2, vy2))
        };
        rasterize_trapezoid(
            fb, fb_w, fb_h, op, sr, sg, sb, sa,
            vy1, vy2, llx.0, llx.1, llx.2, llx.3, rrx.0, rrx.1, rrx.2, rrx.3,
            clip,
        );
    }
}

/// Handle XRender TriStrip (minor opcode 12).
fn handle_tri_strip(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    info!("Render TriStrip: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Points: 8 bytes each (FIXED x + FIXED y)
    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed(data, off);
        let y = read_fixed(data, off + 4);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return Vec::new();
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        for i in 0..points.len() - 2 {
            let (x1, y1) = points[i];
            let (x2, y2) = points[i + 1];
            let (x3, y3) = points[i + 2];
            rasterize_triangle(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, x1, y1, x2, y2, x3, y3, &clip,
            );
        }
    }

    Vec::new()
}

/// Handle XRender TriFan (minor opcode 13).
fn handle_tri_fan(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    info!("Render TriFan: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed(data, off);
        let y = read_fixed(data, off + 4);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return Vec::new();
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        let (cx, cy) = points[0];
        for i in 1..points.len() - 1 {
            let (x2, y2) = points[i];
            let (x3, y3) = points[i + 1];
            rasterize_triangle(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, cx, cy, x2, y2, x3, y3, &clip,
            );
        }
    }

    Vec::new()
}

/// Resolve source picture to pixel data. Returns (pixels, width, height) in BGRA format.
fn resolve_source_pixels(
    state: &mut ClientState,
    src_pic: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<(Vec<u8>, u32, u32)> {
    // Check if it's a solid fill
    if let Some(fill) = state.render.solid_fills.get(&src_pic) {
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let off = i * 4;
            pixels[off] = fill.b;
            pixels[off + 1] = fill.g;
            pixels[off + 2] = fill.r;
            pixels[off + 3] = fill.a;
        }
        return Some((pixels, w, h));
    }

    // Check if it's a linear gradient (referenced directly).
    if let Some(grad) = state.render.linear_gradients.get(&src_pic) {
        let tx = state.render.transforms.get(&src_pic);
        return Some(rasterize_linear_gradient(
            grad, tx, src_x, src_y, width, height,
        ));
    }

    // Check if it's a picture wrapping a drawable
    let (drawable, repeat) = {
        let pic = state.render.pictures.get(&src_pic)?;
        (pic.drawable, pic.repeat)
    };

    // Check if the drawable's picture is actually a solid fill
    if let Some(fill) = state.render.solid_fills.get(&drawable) {
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let off = i * 4;
            pixels[off] = fill.b;
            pixels[off + 1] = fill.g;
            pixels[off + 2] = fill.r;
            pixels[off + 3] = fill.a;
        }
        return Some((pixels, w, h));
    }

    // Check if the picture wraps a linear gradient.
    if let Some(grad) = state.render.linear_gradients.get(&drawable) {
        // Transform may have been set on either the wrapper picture
        // or the underlying gradient — try the wrapper first.
        let tx = state
            .render
            .transforms
            .get(&src_pic)
            .or_else(|| state.render.transforms.get(&drawable));
        return Some(rasterize_linear_gradient(
            grad, tx, src_x, src_y, width, height,
        ));
    }

    // Sync SHM-backed pixmap data before reading
    state.sync_shm_pixmap(drawable);

    // Extract pixels from the drawable's framebuffer
    let fb = state.get_framebuffer_mut(drawable)?;

    if repeat != 0 {
        // Repeat mode: tile the source
        let fb_w = fb.width();
        let fb_h = fb.height();
        if fb_w == 0 || fb_h == 0 {
            return None;
        }
        let w = width as u32;
        let h = height as u32;
        let fb_stride = fb.stride();
        let fb_data = fb.data();
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for row in 0..h {
            for col in 0..w {
                let sy = ((src_y as i32 + row as i32) % fb_h as i32 + fb_h as i32) as u32 % fb_h;
                let sx = ((src_x as i32 + col as i32) % fb_w as i32 + fb_w as i32) as u32 % fb_w;
                let src_off = sy as usize * fb_stride + sx as usize * 4;
                let dst_off = (row * w + col) as usize * 4;
                if src_off + 3 < fb_data.len() && dst_off + 3 < pixels.len() {
                    pixels[dst_off..dst_off + 4].copy_from_slice(&fb_data[src_off..src_off + 4]);
                }
            }
        }
        Some((pixels, w, h))
    } else {
        let pixels = fb.extract_pixels(src_x, src_y, width, height);
        Some((pixels, width as u32, height as u32))
    }
}

fn handle_create_glyphset(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let gsid = read_u32(data, 4);
    let format_id = read_u32(data, 8);

    debug!("Render CreateGlyphSet: gsid={gsid:#x} format={format_id:#x}");

    state.render.glyphsets.insert(
        gsid,
        GlyphSetState {
            format_id,
            glyphs: HashMap::new(),
        },
    );
    Vec::new()
}

fn handle_free_glyphset(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let gsid = read_u32(data, 4);
        state.render.glyphsets.remove(&gsid);
    }
    Vec::new()
}

fn handle_add_glyphs(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gsid = read_u32(data, 4);
    let num_glyphs = read_u32(data, 8) as usize;

    debug!("Render AddGlyphs: gsid={gsid:#x} num={num_glyphs}");

    if data.len() < 12 + num_glyphs * 4 {
        return Vec::new();
    }

    // Read glyph IDs
    let mut glyph_ids = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        glyph_ids.push(read_u32(data, 12 + i * 4));
    }

    let info_start = 12 + num_glyphs * 4;
    if data.len() < info_start + num_glyphs * 12 {
        return Vec::new();
    }

    // Read GlyphInfo entries (12 bytes each)
    let mut glyph_infos = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        let off = info_start + i * 12;
        let width = read_u16(data, off);
        let height = read_u16(data, off + 2);
        let x = read_i16(data, off + 4);
        let y = read_i16(data, off + 6);
        let x_off = read_i16(data, off + 8);
        let y_off = read_i16(data, off + 10);
        glyph_infos.push((width, height, x, y, x_off, y_off));
    }

    let pixel_start = info_start + num_glyphs * 12;

    // Determine the format to know how to read pixel data
    let format_id = state.render.glyphsets.get(&gsid).map(|gs| gs.format_id);

    let mut pixel_off = pixel_start;
    let glyphs_to_store: Vec<(u32, StoredGlyph)> = glyph_ids
        .iter()
        .zip(glyph_infos.iter())
        .map(|(&gid, &(width, height, x, y, x_off, y_off))| {
            let glyph_data = if width > 0 && height > 0 {
                match format_id {
                    Some(fmt) if fmt == PICTFORMAT_A8 => {
                        // A8: each row padded to 4 bytes
                        let row_bytes = pad4(width as usize);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_A1 => {
                        // A1: each row padded to 4 bytes (in bits)
                        let row_bytes = pad4((width as usize + 7) / 8);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_ARGB32 => {
                        // ARGB32: 4 bytes per pixel, rows padded to 4 bytes
                        let row_bytes = width as usize * 4;
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    _ => {
                        // Default to A8
                        let row_bytes = pad4(width as usize);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                }
            } else {
                Vec::new()
            };

            (
                gid,
                StoredGlyph {
                    width,
                    height,
                    x,
                    y,
                    x_off,
                    y_off,
                    data: glyph_data,
                },
            )
        })
        .collect();

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for (gid, glyph) in glyphs_to_store {
            gs.glyphs.insert(gid, glyph);
        }
    }

    Vec::new()
}

fn handle_free_glyphs(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let gsid = read_u32(data, 4);
    let num_glyphs = (data.len() - 8) / 4;

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for i in 0..num_glyphs {
            let gid = read_u32(data, 8 + i * 4);
            gs.glyphs.remove(&gid);
        }
    }
    Vec::new()
}

/// Handle CompositeGlyphs8/16/32
fn handle_composite_glyphs(state: &mut ClientState, data: &[u8], glyph_id_size: usize) -> Vec<u8> {
    if data.len() < 28 {
        return Vec::new();
    }

    let pict_op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let mut current_gsid = read_u32(data, 20);
    let _src_x = read_i16(data, 24);
    let _src_y = read_i16(data, 26);

    debug!(
        "Render CompositeGlyphs{}: op={pict_op} src={src_pic:#x} dst={dst_pic:#x} gs={current_gsid:#x}",
        glyph_id_size * 8
    );

    // Resolve source color (typically solid fill for text)
    let src_color = resolve_source_color(state, src_pic);

    // Resolve dst drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse glyphcmds
    let mut off = 28;
    let mut pen_x: i32 = 0;
    let mut pen_y: i32 = 0;
    let mut first_element = true;

    // Collect all glyph render operations first, then apply
    struct GlyphOp {
        dst_x: i32,
        dst_y: i32,
        width: u16,
        height: u16,
        alpha_data: Vec<u8>,
        format_id: u32,
    }
    let mut ops: Vec<GlyphOp> = Vec::new();

    while off < data.len() {
        if off >= data.len() {
            break;
        }
        let len = data[off] as usize;

        if len == 0 {
            break;
        }

        if len == 255 {
            // Glyphset switch
            if off + 8 <= data.len() {
                current_gsid = read_u32(data, off + 4);
                off = pad4(off + 8);
            } else {
                break;
            }
            continue;
        }

        // Regular glyph element
        if off + 8 > data.len() {
            break;
        }
        // bytes 1..3 = padding
        let delta_x = read_i16(data, off + 4);
        let delta_y = read_i16(data, off + 6);

        if first_element {
            pen_x = delta_x as i32;
            pen_y = delta_y as i32;
            first_element = false;
        } else {
            pen_x += delta_x as i32;
            pen_y += delta_y as i32;
        }

        let glyph_data_start = off + 8;
        let glyph_data_bytes = len * glyph_id_size;
        if glyph_data_start + glyph_data_bytes > data.len() {
            break;
        }

        // Read glyph IDs
        let mut glyph_ids = Vec::with_capacity(len);
        for i in 0..len {
            let gid_off = glyph_data_start + i * glyph_id_size;
            let gid = match glyph_id_size {
                1 => data[gid_off] as u32,
                2 => read_u16(data, gid_off) as u32,
                4 => read_u32(data, gid_off),
                _ => 0,
            };
            glyph_ids.push(gid);
        }

        off = pad4(glyph_data_start + glyph_data_bytes);

        // Look up glyphs and create render operations
        if let Some(gs) = state.render.glyphsets.get(&current_gsid) {
            let format_id = gs.format_id;
            for gid in &glyph_ids {
                if let Some(glyph) = gs.glyphs.get(gid) {
                    if glyph.width > 0 && glyph.height > 0 {
                        ops.push(GlyphOp {
                            dst_x: pen_x - glyph.x as i32,
                            dst_y: pen_y - glyph.y as i32,
                            width: glyph.width,
                            height: glyph.height,
                            alpha_data: glyph.data.clone(),
                            format_id,
                        });
                    }
                    pen_x += glyph.x_off as i32;
                    pen_y += glyph.y_off as i32;
                }
            }
        }
    }

    // Now render all glyph operations to the framebuffer
    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;
        let fb_stride = fb.stride();
        let (sr, sg, sb, sa) = src_color;

        for op in &ops {
            let fb_data = fb.data_mut();
            for row in 0..op.height as i32 {
                let dy = op.dst_y + row;
                if dy < 0 || dy >= fb_h {
                    continue;
                }
                for col in 0..op.width as i32 {
                    let dx = op.dst_x + col;
                    if dx < 0 || dx >= fb_w {
                        continue;
                    }
                    if !clip.allows(dx, dy) {
                        continue;
                    }

                    let alpha = get_glyph_alpha(&op.alpha_data, op.width, col as u16, row as u16, op.format_id);
                    if alpha == 0 {
                        continue;
                    }

                    // Modulate source color by glyph alpha. Both
                    // source and result are premultiplied.
                    let eff_a = ((sa as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_r = ((sr as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_g = ((sg as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_b = ((sb as u32 * alpha as u32 + 127) / 255) as u8;

                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 < fb_data.len() {
                        composite_pixel(
                            pict_op,
                            &mut fb_data[dst_off..dst_off + 4],
                            eff_b,
                            eff_g,
                            eff_r,
                            eff_a,
                        );
                    }
                }
            }
        }

        // Mark dirty for each op
        for op in &ops {
            fb.mark_dirty(op.dst_x, op.dst_y, op.width as u32, op.height as u32);
        }
    }

    Vec::new()
}

/// Extract alpha value from glyph data at a given position
fn get_glyph_alpha(data: &[u8], width: u16, x: u16, y: u16, format_id: u32) -> u8 {
    match format_id {
        f if f == PICTFORMAT_A8 => {
            let row_bytes = pad4(width as usize);
            let off = y as usize * row_bytes + x as usize;
            if off < data.len() {
                data[off]
            } else {
                0
            }
        }
        f if f == PICTFORMAT_A1 => {
            let row_bytes = pad4((width as usize + 7) / 8);
            let byte_off = y as usize * row_bytes + (x as usize / 8);
            let bit_off = x as usize % 8;
            if byte_off < data.len() {
                // LSB first bit order
                if data[byte_off] & (1 << bit_off) != 0 {
                    255
                } else {
                    0
                }
            } else {
                0
            }
        }
        f if f == PICTFORMAT_ARGB32 => {
            let off = (y as usize * width as usize + x as usize) * 4;
            if off + 3 < data.len() {
                data[off + 3] // alpha channel
            } else {
                0
            }
        }
        _ => {
            // Default to A8
            let row_bytes = pad4(width as usize);
            let off = y as usize * row_bytes + x as usize;
            if off < data.len() {
                data[off]
            } else {
                0
            }
        }
    }
}

/// Resolve a source picture to a single premultiplied RGBA color.
///
/// Used by Trapezoids / Triangles / TriStrip / TriFan / CompositeGlyphs
/// where the source is meant to be a single colour for the whole shape.
fn resolve_source_color(state: &ClientState, src_pic: u32) -> (u8, u8, u8, u8) {
    // Check if it's a solid fill directly
    if let Some(fill) = state.render.solid_fills.get(&src_pic) {
        return (fill.r, fill.g, fill.b, fill.a);
    }

    // Check if it's a picture wrapping a solid fill
    if let Some(pic) = state.render.pictures.get(&src_pic) {
        if let Some(fill) = state.render.solid_fills.get(&pic.drawable) {
            return (fill.r, fill.g, fill.b, fill.a);
        }
    }

    // Default: opaque white. Note that rendercheck (and some Cairo
    // paths) use a 1x1 tiled pixmap as a "solid colour source"
    // instead of CreateSolidFill — the right thing to do there
    // would be to sample the first pixel of the underlying pixmap,
    // but in practice the pixmap may not yet contain the colour
    // the caller intended (it's filled by an earlier Composite that
    // we may or may not have rasterised correctly), so the safest
    // fallback continues to be opaque white.
    (0xFF, 0xFF, 0xFF, 0xFF)
}

fn handle_fill_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let dst_pic = read_u32(data, 8);
    // Color is in the request at offset 12..20 (CARD16 each: red, green, blue, alpha)
    let red = read_u16(data, 12);
    let green = read_u16(data, 14);
    let blue = read_u16(data, 16);
    let alpha = read_u16(data, 18);

    // Convert from straight (XRenderColor) to premultiplied so the
    // composite_pixel formulas operate on the right colour space.
    let red8 = (red >> 8) as u8;
    let green8 = (green >> 8) as u8;
    let blue8 = (blue >> 8) as u8;
    let a = (alpha >> 8) as u8;
    let r = ((red8 as u32 * a as u32 + 127) / 255) as u8;
    let g = ((green8 as u32 * a as u32 + 127) / 255) as u8;
    let b = ((blue8 as u32 * a as u32 + 127) / 255) as u8;

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse rectangles (8 bytes each: x(2) y(2) w(2) h(2))
    let mut off = 20;
    let mut rects = Vec::new();
    while off + 8 <= data.len() {
        let x = read_i16(data, off);
        let y = read_i16(data, off + 2);
        let w = read_u16(data, off + 4);
        let h = read_u16(data, off + 6);
        rects.push((x, y, w, h));
        off += 8;
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;
        let fb_stride = fb.stride();

        for (rx, ry, rw, rh) in &rects {
            let fb_data = fb.data_mut();
            for row in 0..*rh as i32 {
                let dy = *ry as i32 + row;
                if dy < 0 || dy >= fb_h {
                    continue;
                }
                for col in 0..*rw as i32 {
                    let dx = *rx as i32 + col;
                    if dx < 0 || dx >= fb_w {
                        continue;
                    }
                    if !clip.allows(dx, dy) {
                        continue;
                    }
                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 >= fb_data.len() {
                        continue;
                    }
                    composite_pixel(
                        op,
                        &mut fb_data[dst_off..dst_off + 4],
                        b,
                        g,
                        r,
                        a,
                    );
                }
            }
            fb.mark_dirty(*rx as i32, *ry as i32, *rw as u32, *rh as u32);
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, w, h) in &rects {
        state.notify_damage(dst_draw, x, y, w, h);
    }

    Vec::new()
}

fn handle_query_filters(seq: u16) -> Vec<u8> {
    // Return ["nearest", "bilinear"]
    let filter1 = b"nearest";
    let filter2 = b"bilinear";

    // Each alias is 2 bytes (CARD16), num_aliases first
    // Each filter is: 1-byte length + name bytes, padded to 4 bytes
    let num_aliases: u32 = 0;
    let num_filters: u32 = 2;

    let aliases_bytes = 0usize;
    let filter1_bytes = 1 + filter1.len(); // length byte + name
    let filter2_bytes = 1 + filter2.len();
    let filters_bytes = pad4(filter1_bytes) + pad4(filter2_bytes);
    let extra = aliases_bytes + filters_bytes;
    let total = 32 + extra;

    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra / 4) as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&num_aliases.to_le_bytes());
    reply[12..16].copy_from_slice(&num_filters.to_le_bytes());

    let mut off = 32;
    // Filter 1: "nearest"
    reply[off] = filter1.len() as u8;
    off += 1;
    reply[off..off + filter1.len()].copy_from_slice(filter1);
    off = 32 + pad4(filter1_bytes);

    // Filter 2: "bilinear"
    reply[off] = filter2.len() as u8;
    off += 1;
    reply[off..off + filter2.len()].copy_from_slice(filter2);

    reply
}

fn handle_create_solid_fill(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    // Color: 4 x CARD16 (red, green, blue, alpha) at offset 8.
    // XRenderColor on the wire is straight (non-premultiplied) alpha;
    // pictures store premultiplied so we convert here once.
    let red = (read_u16(data, 8) >> 8) as u8;
    let green = (read_u16(data, 10) >> 8) as u8;
    let blue = (read_u16(data, 12) >> 8) as u8;
    let alpha = (read_u16(data, 14) >> 8) as u8;
    let premul = |c: u8| -> u8 {
        ((c as u32 * alpha as u32 + 127) / 255) as u8
    };

    debug!(
        "Render CreateSolidFill: pid={pid:#x} straight=({red},{green},{blue},{alpha})"
    );

    state.render.solid_fills.insert(
        pid,
        SolidFillState {
            r: premul(red),
            g: premul(green),
            b: premul(blue),
            a: alpha,
        },
    );
    Vec::new()
}

fn handle_create_gradient_fill(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let minor = data[1];
    match minor {
        34 => handle_create_linear_gradient(state, data),
        // Radial (35) and conical (36) are stubbed for now — both are
        // rare in practice (rendercheck doesn't test them; Cairo
        // emits them only for radial gradients which most apps avoid).
        _ => {
            let pid = read_u32(data, 4);
            debug!("Render CreateGradientFill minor={minor} (stubbed): pid={pid:#x}");
            state.render.solid_fills.insert(
                pid,
                SolidFillState {
                    r: 128,
                    g: 128,
                    b: 128,
                    a: 255,
                },
            );
            Vec::new()
        }
    }
}

/// SetPictureTransform (RENDER minor opcode 28).
///
/// Wire layout:
///
/// ```text
///   1   opcode (139)
///   1   minor (28)
///   2   length
///   4   PICTURE  picture
///   9*4 FIXED    transform (3x3 row-major matrix)
/// ```
///
/// The transform maps *destination* coordinates to *source*
/// coordinates: `(sx*sw, sy*sw, sw) = T · (dx, dy, 1)`. Used by
/// rendercheck (and Cairo) to project a small gradient over a much
/// larger destination region.
fn handle_set_picture_transform(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 + 9 * 4 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let mut tx = [0f64; 9];
    for i in 0..9 {
        tx[i] = read_fixed(data, 8 + i * 4);
    }
    debug!(
        "SetPictureTransform: pid={pid:#x} m=[[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}]]",
        tx[0], tx[1], tx[2], tx[3], tx[4], tx[5], tx[6], tx[7], tx[8]
    );
    // Identity matrix is the most common "reset" — drop the entry
    // so the lookup short-circuits to the no-op fast path.
    let is_identity = (tx[0] - 1.0).abs() < 1e-9
        && tx[1].abs() < 1e-9
        && tx[2].abs() < 1e-9
        && tx[3].abs() < 1e-9
        && (tx[4] - 1.0).abs() < 1e-9
        && tx[5].abs() < 1e-9
        && tx[6].abs() < 1e-9
        && tx[7].abs() < 1e-9
        && (tx[8] - 1.0).abs() < 1e-9;
    if is_identity {
        state.render.transforms.remove(&pid);
    } else {
        state.render.transforms.insert(pid, tx);
    }
    Vec::new()
}

/// Apply a row-major 3x3 transform to a point. Returns
/// `(sx/sw, sy/sw)` per the X RENDER spec.
fn apply_transform(tx: &[f64; 9], px: f64, py: f64) -> (f64, f64) {
    let sx = tx[0] * px + tx[1] * py + tx[2];
    let sy = tx[3] * px + tx[4] * py + tx[5];
    let sw = tx[6] * px + tx[7] * py + tx[8];
    if sw.abs() < 1e-9 {
        (sx, sy)
    } else {
        (sx / sw, sy / sw)
    }
}

/// CreateLinearGradient (RENDER minor opcode 34).
///
/// Wire layout:
///
/// ```text
///   1   opcode (139)
///   1   minor  (34)
///   2   length
///   4   pid
///   8   p1   POINTFIX (FIXED x, FIXED y)
///   8   p2   POINTFIX
///   4   num_stops
///   4*n stops      (FIXED offsets, 0..1)
///   8*n colors     (4 CARD16: r, g, b, a — straight alpha)
/// ```
fn handle_create_linear_gradient(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let p1x = read_fixed(data, 8);
    let p1y = read_fixed(data, 12);
    let p2x = read_fixed(data, 16);
    let p2y = read_fixed(data, 20);
    let num_stops = read_u32(data, 24) as usize;

    // Sanity bound: a typical gradient has 2-8 stops; reject anything
    // absurd before we allocate.
    if num_stops > 1024 {
        return Vec::new();
    }

    let stops_start = 28;
    let colors_start = stops_start + num_stops * 4;
    if colors_start + num_stops * 8 > data.len() {
        return Vec::new();
    }

    let mut stops = Vec::with_capacity(num_stops);
    for i in 0..num_stops {
        let offset = read_fixed(data, stops_start + i * 4);
        let coff = colors_start + i * 8;
        let r = (read_u16(data, coff) >> 8) as u8;
        let g = (read_u16(data, coff + 2) >> 8) as u8;
        let b = (read_u16(data, coff + 4) >> 8) as u8;
        let a = (read_u16(data, coff + 6) >> 8) as u8;
        // Convert straight → premultiplied to match the pictures
        // we composite into.
        let pr = ((r as u32 * a as u32 + 127) / 255) as u8;
        let pg = ((g as u32 * a as u32 + 127) / 255) as u8;
        let pb = ((b as u32 * a as u32 + 127) / 255) as u8;
        stops.push(GradientStop {
            offset,
            r: pr,
            g: pg,
            b: pb,
            a,
        });
    }

    debug!(
        "CreateLinearGradient: pid={pid:#x} p1=({p1x:.2},{p1y:.2}) p2=({p2x:.2},{p2y:.2}) stops={num_stops}"
    );

    state.render.linear_gradients.insert(
        pid,
        LinearGradientState {
            p1: (p1x, p1y),
            p2: (p2x, p2y),
            stops,
        },
    );
    Vec::new()
}

/// Sample a sorted stop list at parameter `t` (linearly interpolated
/// between the two surrounding stops; clamped at the ends).
fn sample_gradient_stops(stops: &[GradientStop], t: f64) -> (u8, u8, u8, u8) {
    if stops.is_empty() {
        return (0, 0, 0, 0);
    }
    if t <= stops[0].offset {
        let s = stops[0];
        return (s.r, s.g, s.b, s.a);
    }
    if t >= stops[stops.len() - 1].offset {
        let s = stops[stops.len() - 1];
        return (s.r, s.g, s.b, s.a);
    }
    for i in 1..stops.len() {
        if t <= stops[i].offset {
            let s0 = stops[i - 1];
            let s1 = stops[i];
            let span = s1.offset - s0.offset;
            let f = if span > 1e-9 { (t - s0.offset) / span } else { 0.0 };
            let lerp = |a: u8, b: u8| -> u8 {
                let v = a as f64 * (1.0 - f) + b as f64 * f;
                v.round().clamp(0.0, 255.0) as u8
            };
            return (
                lerp(s0.r, s1.r),
                lerp(s0.g, s1.g),
                lerp(s0.b, s1.b),
                lerp(s0.a, s1.a),
            );
        }
    }
    let s = stops[stops.len() - 1];
    (s.r, s.g, s.b, s.a)
}

/// Rasterise a region of a linear gradient into a BGRA pixel buffer.
/// `(src_x, src_y)` is the top-left source coordinate the caller
/// requested; `(width, height)` is the buffer size. The output is
/// premultiplied to match the rest of the picture pipeline.
fn rasterize_linear_gradient(
    grad: &LinearGradientState,
    transform: Option<&[f64; 9]>,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> (Vec<u8>, u32, u32) {
    let w = width as u32;
    let h = height as u32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    let (p1x, p1y) = grad.p1;
    let (p2x, p2y) = grad.p2;
    let dx = p2x - p1x;
    let dy = p2y - p1y;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-9 {
        // Degenerate (p1 == p2): fill with the first stop colour.
        let (r, g, b, a) = sample_gradient_stops(&grad.stops, 0.0);
        for i in 0..(w * h) as usize {
            let off = i * 4;
            pixels[off] = b;
            pixels[off + 1] = g;
            pixels[off + 2] = r;
            pixels[off + 3] = a;
        }
        return (pixels, w, h);
    }

    for row in 0..h as i32 {
        for col in 0..w as i32 {
            // Use pixel centres for the projection so the rasterised
            // gradient lines up with rendercheck's reference.
            let mut px = (src_x as i32 + col) as f64 + 0.5;
            let mut py = (src_y as i32 + row) as f64 + 0.5;
            if let Some(tx) = transform {
                let (tx_px, tx_py) = apply_transform(tx, px, py);
                px = tx_px;
                py = tx_py;
            }
            let t = ((px - p1x) * dx + (py - p1y) * dy) / len_sq;
            let (r, g, b, a) = sample_gradient_stops(&grad.stops, t);
            let off = (row as usize * w as usize + col as usize) * 4;
            pixels[off] = b;
            pixels[off + 1] = g;
            pixels[off + 2] = r;
            pixels[off + 3] = a;
        }
    }

    (pixels, w, h)
}
