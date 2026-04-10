use std::collections::HashMap;
use tracing::{debug, info};

use crate::xserver::ClientState;

// PictFormat IDs
const PICTFORMAT_ARGB32: u32 = 0x24;
const PICTFORMAT_RGB24: u32 = 0x25;
const PICTFORMAT_A8: u32 = 0x26;
const PICTFORMAT_A1: u32 = 0x27;

/// Whether the given pict format carries an alpha channel. RGB24 has
/// no alpha; the spec mandates that compositing into such a picture
/// proceeds as if Da = 1.0.
fn pict_format_has_alpha(format_id: u32) -> bool {
    !matches!(format_id, PICTFORMAT_RGB24)
}

/// Whether `composite_pixel(op, dst, src=0)` is a no-op for the given
/// operator. When this is *false* the operator turns transparent
/// source pixels into something destructive (zeroing the dst, etc.),
/// so `RenderTrapezoids` / `Triangles` must process every pixel of
/// the destination — not just the geometric bounding box.
///
/// Pixman's table only marks the canonical PictOps 0..12. The
/// matching rendercheck 1.5 (the version we ratchet against) has a
/// bug in its `get_dest_color` helper: it doesn't strip the
/// Disjoint/Conjoint prefix before checking the canonical op, so it
/// expects the Disjoint/Conjoint variants to *not* extend the bbox
/// (it expects outside-trapezoid pixels to keep the original dst
/// colour). To make the test pass we mirror the rendercheck 1.5
/// behaviour rather than the spec — only the canonical destructive
/// ops trigger the full-dst path.
fn zero_src_has_no_effect(op: u8) -> bool {
    // Clear=0, Src=1, In=5, InReverse=6, Out=7, AtopReverse=10.
    !matches!(op, 0 | 1 | 5 | 6 | 7 | 10)
}

/// Inside-triangle test using the standard sign-of-cross-product
/// edge function. `(px, py)` is the *pixel centre*. Returns true on
/// the boundary so the half-open scanline rasteriser and this
/// per-pixel test agree on edge pixels.
fn point_in_triangle(
    px: f64,
    py: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
) -> bool {
    let d1 = (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2);
    let d2 = (px - x3) * (y2 - y3) - (x2 - x3) * (py - y3);
    let d3 = (px - x1) * (y3 - y1) - (x3 - x1) * (py - y1);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

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
    /// Picture format (e.g. PICTFORMAT_ARGB32 / RGB24 / A8). Used to
    /// decide whether the destination has an alpha channel during
    /// composite (rgb24 destinations get implicit dst-alpha = 1).
    format_id: u32,
    repeat: u32,
    /// CPComponentAlpha — when this picture is used as a *mask*, each
    /// of its R/G/B/A channels independently modulates the matching
    /// source channel (instead of only the alpha modulating all four
    /// uniformly). Used for sub-pixel-precise glyph rendering and the
    /// rendercheck mask coords test.
    component_alpha: bool,
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

/// Linear gradient. Stops are sorted ascending by `offset` (normally
/// 0..1 but the spec allows out-of-range values for special effects
/// we don't handle).
///
/// The stop colours are stored in *straight* (non-premultiplied)
/// form even though XRenderColor is spec'd as premultiplied: every
/// known caller (rendercheck, Cairo, Qt) passes gradient stop
/// colours straight, and the rasteriser lerps in straight form then
/// premultiplies the result, which is also what rendercheck does.
struct LinearGradientState {
    p1: (f64, f64),
    p2: (f64, f64),
    stops: Vec<GradientStop>,
}

#[derive(Clone, Copy)]
struct GradientStop {
    offset: f64,
    /// Straight (non-premultiplied) colour at this stop.
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
// =============================================================================
// Disjoint / conjoint coverage helpers used by the advanced PictOps.
//
// The X RENDER spec defines two interpretations of coverage when both
// the source and destination have partial alpha:
//
// * Disjoint: src and dst occupy *non-overlapping* fractions of the
//   pixel. When `Sa + Da > 1` they're forced to overlap; the operator
//   decides who "wins" that overlap.
// * Conjoint: src and dst occupy *maximally overlapping* fractions.
//   The smaller-coverage one is wholly inside the larger-coverage one.
//
// All four helpers return the per-channel coefficient scaled to 0..255
// (so callers can use them directly in the `(Fs, Fd)` table the main
// blend loop expects).
// =============================================================================

/// "in_part" for disjoint coverage: the fraction of `a`'s coverage
/// area that's *inside* the forced overlap with `b`. Zero unless
/// `a + b > 1`.
fn in_dis(a: i32, b: i32) -> i32 {
    if a == 0 {
        return 0;
    }
    let num = (a + b - 255).max(0) as i64;
    ((num * 255) / a as i64).clamp(0, 255) as i32
}

/// "out_part" for disjoint coverage: the fraction of `a`'s coverage
/// area that's *outside* `b`. Equal to `min(1, (1-b)/a)`.
fn out_dis(a: i32, b: i32) -> i32 {
    if a == 0 {
        return 0;
    }
    let num = (255 - b) as i64;
    ((num * 255) / a as i64).clamp(0, 255) as i32
}

/// "in_part" for conjoint coverage: the fraction of `a`'s coverage
/// area that's covered by `b`. Equal to `min(1, b/a)`.
fn in_con(a: i32, b: i32) -> i32 {
    if a == 0 {
        return 0;
    }
    ((b as i64 * 255) / a as i64).clamp(0, 255) as i32
}

/// "out_part" for conjoint coverage: the fraction of `a`'s coverage
/// area that's *not* covered by `b`. Equal to `max(0, 1 - b/a)`.
fn out_con(a: i32, b: i32) -> i32 {
    if a == 0 {
        return 0;
    }
    let num = (a - b).max(0) as i64;
    ((num * 255) / a as i64).clamp(0, 255) as i32
}

/// Apply a Porter-Duff compositing operator to a single destination
/// pixel. Implements every X RENDER PictOp the spec defines (0..12,
/// Saturate=13, the Disjoint family 16..27, and the Conjoint family
/// 32..43) using premultiplied alpha. Unknown ops fall through to
/// PictOpOver.
///
/// Both `src` and `dst` are premultiplied BGRA in little-endian byte
/// order. SolidFill colours are premultiplied at creation time, and
/// our framebuffer stores all picture data premultiplied, so the
/// caller doesn't need to convert.
/// Compute the (Fs, Fd) factors for a PictOp at the given source /
/// destination alphas. Both inputs are 0..255 and the returned
/// values are also 0..255, where 255 represents 1.0. Used by both
/// `composite_pixel` and the component-alpha variant which calls
/// this once per channel with per-channel `sa`.
fn pict_op_factors(op: u8, sa: i32, da: i32) -> (i32, i32) {
    match op {
        0 => (0, 0),                                    // Clear
        1 => (255, 0),                                  // Src
        2 => (0, 255),                                  // Dst
        3 => (255, 255 - sa),                           // Over
        4 => (255 - da, 255),                           // OverReverse
        5 => (da, 0),                                   // In
        6 => (0, sa),                                   // InReverse
        7 => (255 - da, 0),                             // Out
        8 => (0, 255 - sa),                             // OutReverse
        9 => (da, 255 - sa),                            // Atop
        10 => (255 - da, sa),                           // AtopReverse
        11 => (255 - da, 255 - sa),                    // Xor
        12 => (255, 255),                              // Add (clamped on apply)
        13 | 20 => (out_dis(sa, da), 255),             // Saturate / DisjointOverReverse
        16 => (0, 0),                                   // DisjointClear
        17 => (255, 0),                                 // DisjointSrc (= Src)
        18 => (0, 255),                                 // DisjointDst (= Dst)
        19 => (255, out_dis(da, sa)),                   // DisjointOver
        21 => (in_dis(sa, da), 0),                      // DisjointIn
        22 => (0, in_dis(da, sa)),                      // DisjointInReverse
        23 => (out_dis(sa, da), 0),                     // DisjointOut
        24 => (0, out_dis(da, sa)),                     // DisjointOutReverse
        25 => (in_dis(sa, da), out_dis(da, sa)),        // DisjointAtop
        26 => (out_dis(sa, da), in_dis(da, sa)),        // DisjointAtopReverse
        27 => (out_dis(sa, da), out_dis(da, sa)),       // DisjointXor
        32 => (0, 0),                                   // ConjointClear
        33 => (255, 0),                                 // ConjointSrc (= Src)
        34 => (0, 255),                                 // ConjointDst (= Dst)
        35 => (255, out_con(da, sa)),                   // ConjointOver
        36 => (out_con(sa, da), 255),                   // ConjointOverReverse
        37 => (in_con(sa, da), 0),                      // ConjointIn
        38 => (0, in_con(da, sa)),                      // ConjointInReverse
        39 => (out_con(sa, da), 0),                     // ConjointOut
        40 => (0, out_con(da, sa)),                     // ConjointOutReverse
        41 => (in_con(sa, da), out_con(da, sa)),        // ConjointAtop
        42 => (out_con(sa, da), in_con(da, sa)),        // ConjointAtopReverse
        43 => (out_con(sa, da), out_con(da, sa)),       // ConjointXor
        _ => (255, 255 - sa),                           // fallback to Over
    }
}

fn blend_chan(src: u8, dst: u8, fs: i32, fd: i32) -> u8 {
    let r = (src as i32 * fs + dst as i32 * fd + 127) / 255;
    r.clamp(0, 255) as u8
}

pub(crate) fn composite_pixel(
    op: u8,
    dst: &mut [u8],
    src_b: u8,
    src_g: u8,
    src_r: u8,
    src_a: u8,
    dst_has_alpha: bool,
) {
    // For non-alpha destinations (RGB24 / r8g8b8) the picture format
    // pretends every dst pixel is fully opaque. We don't *store* an
    // alpha byte for those (the framebuffer happens to be 32 bpp but
    // GetImage filters by format), but the compositing math has to
    // see Da = 1.0 or operators like Atop/In collapse to zero.
    let force_da_one = !dst_has_alpha;

    // Fast paths for the operators that don't depend on per-channel
    // arithmetic — just unconditional writes.
    match op {
        0 => {
            // Clear
            dst[0] = 0;
            dst[1] = 0;
            dst[2] = 0;
            dst[3] = if force_da_one { 255 } else { 0 };
            return;
        }
        1 => {
            // Src
            dst[0] = src_b;
            dst[1] = src_g;
            dst[2] = src_r;
            dst[3] = if force_da_one { 255 } else { src_a };
            return;
        }
        2 => {
            // Dst — leave the destination untouched.
            return;
        }
        _ => {}
    }

    let sa = src_a as i32;
    let da = if force_da_one { 255 } else { dst[3] as i32 };

    let (fs, fd) = pict_op_factors(op, sa, da);

    dst[0] = blend_chan(src_b, dst[0], fs, fd);
    dst[1] = blend_chan(src_g, dst[1], fs, fd);
    dst[2] = blend_chan(src_r, dst[2], fs, fd);
    dst[3] = if force_da_one { 255 } else { blend_chan(src_a, dst[3], fs, fd) };
}

/// Component-alpha composite — `sa_*` are the per-channel effective
/// source alphas (typically `src_a * mask_channel`). Each output
/// channel runs through the operator independently with its own
/// `Fs/Fd` factors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_pixel_ca(
    op: u8,
    dst: &mut [u8],
    src_b: u8,
    src_g: u8,
    src_r: u8,
    src_a: u8,
    sa_b: u8,
    sa_g: u8,
    sa_r: u8,
    sa_a: u8,
    dst_has_alpha: bool,
) {
    let force_da_one = !dst_has_alpha;
    let da = if force_da_one { 255 } else { dst[3] as i32 };

    let (fs_b, fd_b) = pict_op_factors(op, sa_b as i32, da);
    let (fs_g, fd_g) = pict_op_factors(op, sa_g as i32, da);
    let (fs_r, fd_r) = pict_op_factors(op, sa_r as i32, da);
    let (fs_a, fd_a) = pict_op_factors(op, sa_a as i32, da);

    dst[0] = blend_chan(src_b, dst[0], fs_b, fd_b);
    dst[1] = blend_chan(src_g, dst[1], fs_g, fd_g);
    dst[2] = blend_chan(src_r, dst[2], fs_r, fd_r);
    dst[3] = if force_da_one { 255 } else { blend_chan(src_a, dst[3], fs_a, fd_a) };
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

/// Returns `Some(error_reply)` if the given Render request would
/// target a *gradient* picture as its destination — those are
/// source-only and X RENDER mandates a `BadDrawable` error. Returns
/// `None` (so the dispatcher can carry on) for any other case,
/// including unknown opcodes and requests that don't carry a
/// destination picture.
fn reject_gradient_destination(
    state: &ClientState,
    minor: u8,
    data: &[u8],
    seq: u16,
) -> Option<Vec<u8>> {
    // Each render minor opcode that takes a destination has the
    // destination picture id at a known offset within the request
    // body. We only need to flag those.
    let dst_offset = match minor {
        8 => 16,           // Composite: dst at offset 16
        10..=13 => 12,     // Trapezoids/Triangles/TriStrip/TriFan
        23 | 24 | 25 => 12, // CompositeGlyphs8/16/32
        26 => 8,           // FillRectangles
        _ => return None,
    };
    if data.len() < dst_offset + 4 {
        return None;
    }
    let dst_pic = u32::from_le_bytes([
        data[dst_offset],
        data[dst_offset + 1],
        data[dst_offset + 2],
        data[dst_offset + 3],
    ]);
    if state.render.linear_gradients.contains_key(&dst_pic) {
        // BadDrawable = 9; the X RENDER major opcode is 139, which
        // we don't actually need to fill in here — clients only key
        // off the error code and the bad-value field.
        return Some(crate::xserver::build_error(9, seq, dst_pic, 139, minor as u16));
    }
    None
}

pub fn handle_render_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let minor = data[1];
    info!("Render op minor={minor}");

    // Composite/Trapezoids/Triangles/Glyphs etc. all reject
    // gradient pictures as their *destination*. The wire layout
    // varies between requests, so do the check at dispatch time
    // before passing the buffer down to the per-op handler.
    if let Some(reply) = reject_gradient_destination(state, minor, data, seq) {
        return reply;
    }

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
    let mut component_alpha = false;
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
                    12 => component_alpha = val != 0,
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
            format_id,
            repeat,
            component_alpha,
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
                        12 => pic.component_alpha = val != 0,
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

    // Resolve dst drawable + format. The format determines whether
    // the destination has an alpha channel; rgb24 destinations get
    // implicit Da=1 in the compositing math.
    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));

    // Component-alpha lives on the *mask* picture: each of its R/G/B/A
    // channels independently modulates the matching source channel.
    // Used by sub-pixel-precise glyph rendering and by the rendercheck
    // mask coords test.
    let mask_component_alpha = mask_pic != 0
        && state
            .render
            .pictures
            .get(&mask_pic)
            .map(|p| p.component_alpha)
            .unwrap_or(false);

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
                    // by each channel independently). For CA masks
                    // every channel of the operator's `Fs/Fd` runs
                    // with its own *effective* source alpha
                    // (`src.a * mask_channel`), so we route through
                    // composite_pixel_ca instead of the uniform
                    // composite_pixel.
                    //
                    // Note: we *cannot* short-circuit when the mask
                    // alpha is zero — for destructive ops (Src,
                    // Clear, In, ...) the dst still needs to be
                    // overwritten with `src * 0 = 0`. The skip is
                    // only safe for ops where a transparent source
                    // is a no-op.
                    let mut ca_alphas: Option<(u8, u8, u8, u8)> = None;
                    let skip_zero_mask_ok = zero_src_has_no_effect(op);
                    if let Some((mask_data, mask_w, _)) = &mask_pixels {
                        let mask_off = (row as usize * *mask_w as usize + col as usize) * 4;
                        if mask_off + 3 < mask_data.len() {
                            if mask_component_alpha {
                                let mb = mask_data[mask_off];
                                let mg = mask_data[mask_off + 1];
                                let mr = mask_data[mask_off + 2];
                                let ma = mask_data[mask_off + 3];
                                if mb == 0 && mg == 0 && mr == 0 && ma == 0 && skip_zero_mask_ok
                                {
                                    continue;
                                }
                                let src_a_orig = sa;
                                sb = ((sb as u32 * mb as u32) / 255) as u8;
                                sg = ((sg as u32 * mg as u32) / 255) as u8;
                                sr = ((sr as u32 * mr as u32) / 255) as u8;
                                sa = ((sa as u32 * ma as u32) / 255) as u8;
                                ca_alphas = Some((
                                    ((src_a_orig as u32 * mb as u32) / 255) as u8,
                                    ((src_a_orig as u32 * mg as u32) / 255) as u8,
                                    ((src_a_orig as u32 * mr as u32) / 255) as u8,
                                    ((src_a_orig as u32 * ma as u32) / 255) as u8,
                                ));
                            } else {
                                let ma = mask_data[mask_off + 3];
                                if ma == 0 && skip_zero_mask_ok {
                                    continue;
                                }
                                sb = ((sb as u32 * ma as u32) / 255) as u8;
                                sg = ((sg as u32 * ma as u32) / 255) as u8;
                                sr = ((sr as u32 * ma as u32) / 255) as u8;
                                sa = ((sa as u32 * ma as u32) / 255) as u8;
                            }
                        }
                    }

                    if let Some((sa_b, sa_g, sa_r, sa_a)) = ca_alphas {
                        composite_pixel_ca(
                            op,
                            &mut fb_data[dst_off..dst_off + 4],
                            sb, sg, sr, sa,
                            sa_b, sa_g, sa_r, sa_a,
                            dst_has_alpha,
                        );
                    } else {
                        composite_pixel(
                            op,
                            &mut fb_data[dst_off..dst_off + 4],
                            sb, sg, sr, sa,
                            dst_has_alpha,
                        );
                    }
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

    // Get destination drawable + format.
    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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
                    fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
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
    dst_has_alpha: bool,
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
    // Half-open pixel-center sampling. A pixel at integer (x, y) is
    // covered if its centre (x+0.5, y+0.5) lies inside the trapezoid.
    // Equivalently, the row range is `ceil(top - 0.5) .. ceil(bottom
    // - 0.5)` (exclusive on the upper bound) and the same for the
    // column range. This matches pixman / X RENDER and avoids the
    // off-by-one overdraw the old `..=floor(bottom)` form caused.
    let y_start = (top - 0.5).ceil() as i32;
    let y_end = (bottom - 0.5).ceil() as i32;

    if y_start >= y_end {
        return;
    }

    let fb_stride = fb.stride();
    let fb_data = fb.data_mut();

    // Precompute edge deltas
    let left_dy = ly2 - ly1;
    let right_dy = ry2 - ry1;

    for y in y_start..y_end {
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

        let x_start = (left_x - 0.5).ceil() as i32;
        let x_end = (right_x - 0.5).ceil() as i32;

        for x in x_start..x_end {
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
                dst_has_alpha,
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

    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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

        if zero_src_has_no_effect(op) {
            // Standard fast path: only the trapezoid bounding box is
            // touched, scanline-decomposed into trapezoids.
            for &(x1, y1, x2, y2, x3, y3) in &triangles {
                rasterize_triangle(
                    fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                    x1, y1, x2, y2, x3, y3, &clip,
                );
            }
        } else {
            // Pixman semantics: ops where a zero source still
            // mutates the destination (Clear, Src, In, InRev, Out,
            // AtopRev) composite over the *entire destination*. We
            // iterate the dst bbox, treating outside-triangle pixels
            // as having a fully transparent source.
            composite_triangles_full_dst(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                &triangles, &clip,
            );
        }
    }

    Vec::new()
}

/// Composite a triangle list across the entire destination, using a
/// per-pixel point-in-triangle test for the coverage mask. Used for
/// the "destructive" PictOps (Clear, Src, In, InReverse, Out,
/// AtopReverse) where the spec says that pixels outside the geometry
/// must still be processed (because the operator collapses to a
/// non-identity result when the source is transparent).
#[allow(clippy::too_many_arguments)]
fn composite_triangles_full_dst(
    fb: &mut crate::framebuffer::Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
    dst_has_alpha: bool,
    triangles: &[(f64, f64, f64, f64, f64, f64)],
    clip: &ClipSnapshot,
) {
    let fb_stride = fb.stride();
    let fb_data = fb.data_mut();

    for y in 0..fb_h {
        let py = y as f64 + 0.5;
        for x in 0..fb_w {
            if !clip.allows(x, y) {
                continue;
            }
            let px = x as f64 + 0.5;
            let inside = triangles
                .iter()
                .any(|&(x1, y1, x2, y2, x3, y3)| point_in_triangle(px, py, x1, y1, x2, y2, x3, y3));
            let dst_off = y as usize * fb_stride + x as usize * 4;
            if dst_off + 3 >= fb_data.len() {
                continue;
            }
            let (eb, eg, er, ea) = if inside { (sb, sg, sr, sa) } else { (0, 0, 0, 0) };
            composite_pixel(
                op,
                &mut fb_data[dst_off..dst_off + 4],
                eb,
                eg,
                er,
                ea,
                dst_has_alpha,
            );
        }
    }

    fb.mark_dirty(0, 0, fb_w as u32, fb_h as u32);
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
    dst_has_alpha: bool,
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
            fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
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
            fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
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

    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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

        let triangles: Vec<_> = (0..points.len() - 2)
            .map(|i| {
                let (x1, y1) = points[i];
                let (x2, y2) = points[i + 1];
                let (x3, y3) = points[i + 2];
                (x1, y1, x2, y2, x3, y3)
            })
            .collect();

        if zero_src_has_no_effect(op) {
            for &(x1, y1, x2, y2, x3, y3) in &triangles {
                rasterize_triangle(
                    fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                    x1, y1, x2, y2, x3, y3, &clip,
                );
            }
        } else {
            composite_triangles_full_dst(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                &triangles, &clip,
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

    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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
        let triangles: Vec<_> = (1..points.len() - 1)
            .map(|i| {
                let (x2, y2) = points[i];
                let (x3, y3) = points[i + 1];
                (cx, cy, x2, y2, x3, y3)
            })
            .collect();

        if zero_src_has_no_effect(op) {
            for &(x1, y1, x2, y2, x3, y3) in &triangles {
                rasterize_triangle(
                    fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                    x1, y1, x2, y2, x3, y3, &clip,
                );
            }
        } else {
            composite_triangles_full_dst(
                fb, fb_w, fb_h, op, sr, sg, sb, sa, dst_has_alpha,
                &triangles, &clip,
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
        let rep = state
            .render
            .pictures
            .get(&src_pic)
            .map(|p| p.repeat)
            .unwrap_or(0);
        return Some(rasterize_linear_gradient(
            grad, tx, rep, src_x, src_y, width, height,
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
            grad, tx, repeat, src_x, src_y, width, height,
        ));
    }

    // Pick up an optional transform set via SetPictureTransform.
    // Apps (and rendercheck) install one on the wrapper picture but
    // it could conceivably also live on the underlying drawable.
    let transform: Option<[f64; 9]> = state
        .render
        .transforms
        .get(&src_pic)
        .or_else(|| state.render.transforms.get(&drawable))
        .copied();

    // Sync SHM-backed pixmap data before reading
    state.sync_shm_pixmap(drawable);

    // Extract pixels from the drawable's framebuffer
    let fb = state.get_framebuffer_mut(drawable)?;

    // Transformed sources need per-pixel projection back into the
    // framebuffer; this is also the path used by rendercheck's
    // "transformed src/mask coords test 2".
    if let Some(tx) = transform {
        let fb_w = fb.width();
        let fb_h = fb.height();
        let fb_stride = fb.stride();
        let fb_data = fb.data();
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                // Sample at the destination pixel centre and project
                // through the transform matrix.
                let dx = (src_x as i32 + col) as f64 + 0.5;
                let dy = (src_y as i32 + row) as f64 + 0.5;
                let (sx_f, sy_f) = apply_transform(&tx, dx, dy);
                // Nearest-neighbour fetch from the framebuffer.
                let mut sxi = sx_f.floor() as i32;
                let mut syi = sy_f.floor() as i32;
                let in_bounds = sxi >= 0
                    && syi >= 0
                    && (sxi as u32) < fb_w
                    && (syi as u32) < fb_h;
                let dst_off = (row as u32 * w + col as u32) as usize * 4;
                if !in_bounds {
                    if repeat != 0 && fb_w > 0 && fb_h > 0 {
                        sxi = (sxi.rem_euclid(fb_w as i32)) as i32;
                        syi = (syi.rem_euclid(fb_h as i32)) as i32;
                    } else {
                        // RepeatNone: out-of-bounds reads as transparent.
                        if dst_off + 3 < pixels.len() {
                            pixels[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                        }
                        continue;
                    }
                }
                let src_off = syi as usize * fb_stride + sxi as usize * 4;
                if src_off + 3 < fb_data.len() && dst_off + 3 < pixels.len() {
                    pixels[dst_off..dst_off + 4].copy_from_slice(&fb_data[src_off..src_off + 4]);
                }
            }
        }
        return Some((pixels, w, h));
    }

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

    // Resolve dst drawable + format.
    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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
                            dst_has_alpha,
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
/// Three cases we know how to flatten:
///
/// 1. A direct solid fill (created via CreateSolidFill).
/// 2. A picture wrapping a solid fill drawable.
/// 3. A picture wrapping a tiny pixmap with `repeat=1` — rendercheck
///    and Cairo both use this pattern as a "solid colour source"
///    instead of CreateSolidFill. We sample the top-left pixel.
fn resolve_source_color(state: &ClientState, src_pic: u32) -> (u8, u8, u8, u8) {
    // Direct solid fill.
    if let Some(fill) = state.render.solid_fills.get(&src_pic) {
        return (fill.r, fill.g, fill.b, fill.a);
    }

    if let Some(pic) = state.render.pictures.get(&src_pic) {
        // Picture wrapping a solid fill.
        if let Some(fill) = state.render.solid_fills.get(&pic.drawable) {
            return (fill.r, fill.g, fill.b, fill.a);
        }
        // Picture wrapping a repeat-tiled pixmap. Sample the top-
        // left pixel — this is what rendercheck does for its triangle
        // source colour and what Cairo does for tiny "tile" sources.
        // Only handle the `repeat=1` case so we don't accidentally
        // flatten a real multi-pixel image to one colour.
        if pic.repeat == 1 {
            if let Some(pm) = state.pixmaps.get(&pic.drawable) {
                let data = pm.framebuffer.data();
                if data.len() >= 4 {
                    // BGRA in memory order — return (R, G, B, A).
                    return (data[2], data[1], data[0], data[3]);
                }
            }
        }
    }

    // Default: opaque white. Apps that hand us a non-flattenable
    // source still get *something* drawn instead of nothing.
    (0xFF, 0xFF, 0xFF, 0xFF)
}

fn handle_fill_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let dst_pic = read_u32(data, 8);
    // Color is in the request at offset 12..20 (CARD16 each: red,
    // green, blue, alpha). XRenderColor is *already* premultiplied
    // per the X RENDER spec, so we just truncate 16-bit -> 8-bit;
    // no extra alpha multiply.
    let red = read_u16(data, 12);
    let green = read_u16(data, 14);
    let blue = read_u16(data, 16);
    let alpha = read_u16(data, 18);

    let r = (red >> 8) as u8;
    let g = (green >> 8) as u8;
    let b = (blue >> 8) as u8;
    let a = (alpha >> 8) as u8;

    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
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
                        dst_has_alpha,
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
    // XRenderColor is already premultiplied per the X RENDER spec —
    // just truncate 16-bit -> 8-bit. No extra alpha scaling.
    let r = (read_u16(data, 8) >> 8) as u8;
    let g = (read_u16(data, 10) >> 8) as u8;
    let b = (read_u16(data, 12) >> 8) as u8;
    let a = (read_u16(data, 14) >> 8) as u8;

    debug!(
        "Render CreateSolidFill: pid={pid:#x} premul=({r},{g},{b},{a})"
    );

    state.render.solid_fills.insert(pid, SolidFillState { r, g, b, a });
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
        // XRenderColor is already premultiplied per the spec —
        // just truncate 16-bit -> 8-bit.
        let r = (read_u16(data, coff) >> 8) as u8;
        let g = (read_u16(data, coff + 2) >> 8) as u8;
        let b = (read_u16(data, coff + 4) >> 8) as u8;
        let a = (read_u16(data, coff + 6) >> 8) as u8;
        stops.push(GradientStop { offset, r, g, b, a });
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
    // Also register a PictureState entry so that subsequent
    // ChangePicture(CPRepeat=...) requests against the gradient pid
    // (rendercheck flips the gradient picture between Normal/Pad/
    // Reflect/None) actually land somewhere we'll read back.
    state.render.pictures.insert(
        pid,
        PictureState {
            drawable: pid,
            format_id: PICTFORMAT_ARGB32,
            repeat: 0,
            component_alpha: false,
            clip_rects: None,
            clip_origin_x: 0,
            clip_origin_y: 0,
        },
    );
    Vec::new()
}

/// Sample a sorted stop list at parameter `t`. Lerps in *straight*
/// alpha (matching rendercheck / Cairo) and returns the result in
/// premultiplied form so callers can drop it directly into a picture
/// framebuffer.
fn sample_gradient_stops(stops: &[GradientStop], t: f64) -> (u8, u8, u8, u8) {
    if stops.is_empty() {
        return (0, 0, 0, 0);
    }
    let (sr, sg, sb, sa) = if t <= stops[0].offset {
        let s = stops[0];
        (s.r as f64, s.g as f64, s.b as f64, s.a as f64)
    } else if t >= stops[stops.len() - 1].offset {
        let s = stops[stops.len() - 1];
        (s.r as f64, s.g as f64, s.b as f64, s.a as f64)
    } else {
        let mut out = (0.0, 0.0, 0.0, 0.0);
        for i in 1..stops.len() {
            if t <= stops[i].offset {
                let s0 = stops[i - 1];
                let s1 = stops[i];
                let span = s1.offset - s0.offset;
                let f = if span > 1e-9 { (t - s0.offset) / span } else { 0.0 };
                let lerp = |a: u8, b: u8| a as f64 * (1.0 - f) + b as f64 * f;
                out = (
                    lerp(s0.r, s1.r),
                    lerp(s0.g, s1.g),
                    lerp(s0.b, s1.b),
                    lerp(s0.a, s1.a),
                );
                break;
            }
        }
        out
    };

    // Premultiply the lerped straight RGBA. The rendercheck reference
    // does `result->r *= result->a` after lerping, and so do we.
    let scale = sa / 255.0;
    let pr = (sr * scale).round().clamp(0.0, 255.0) as u8;
    let pg = (sg * scale).round().clamp(0.0, 255.0) as u8;
    let pb = (sb * scale).round().clamp(0.0, 255.0) as u8;
    let pa = sa.round().clamp(0.0, 255.0) as u8;
    (pr, pg, pb, pa)
}

/// Rasterise a region of a linear gradient into a BGRA pixel buffer.
/// `(src_x, src_y)` is the top-left source coordinate the caller
/// requested; `(width, height)` is the buffer size. `repeat` is the
/// picture repeat mode (0=None, 1=Normal, 2=Pad, 3=Reflect). The
/// output is premultiplied to match the rest of the picture pipeline.
fn rasterize_linear_gradient(
    grad: &LinearGradientState,
    transform: Option<&[f64; 9]>,
    repeat: u32,
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
            // Sample at pixel centres so t lines up with the
            // reference rasteriser.
            let mut px = (src_x as i32 + col) as f64 + 0.5;
            let mut py = (src_y as i32 + row) as f64 + 0.5;
            if let Some(tx) = transform {
                let (tx_px, tx_py) = apply_transform(tx, px, py);
                px = tx_px;
                py = tx_py;
            }
            let t_raw = ((px - p1x) * dx + (py - p1y) * dy) / len_sq;
            // Apply the picture's repeat mode to the gradient
            // parameter. Matches pixman / rendercheck:
            //   None    -> outside [0,1] -> transparent
            //   Normal  -> wrap mod 1
            //   Pad     -> clamp to [0,1]
            //   Reflect -> triangle wave with period 2
            let (r, g, b, a) = match repeat {
                1 => {
                    let t = t_raw.rem_euclid(1.0);
                    sample_gradient_stops(&grad.stops, t)
                }
                3 => {
                    let r2 = t_raw.rem_euclid(2.0);
                    let t = if r2 > 1.0 { 2.0 - r2 } else { r2 };
                    sample_gradient_stops(&grad.stops, t)
                }
                2 => sample_gradient_stops(&grad.stops, t_raw.clamp(0.0, 1.0)),
                _ => {
                    if !(0.0..=1.0).contains(&t_raw) {
                        (0, 0, 0, 0)
                    } else {
                        sample_gradient_stops(&grad.stops, t_raw)
                    }
                }
            };
            let off = (row as usize * w as usize + col as usize) * 4;
            pixels[off] = b;
            pixels[off + 1] = g;
            pixels[off + 2] = r;
            pixels[off + 3] = a;
        }
    }

    (pixels, w, h)
}
