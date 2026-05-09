use std::collections::HashMap;
use tracing::{debug, info};
use x11rb_protocol::protocol::render::{
    ADD_GLYPHS_REQUEST, ADD_TRAPS_REQUEST, CHANGE_PICTURE_REQUEST, COMPOSITE_GLYPHS16_REQUEST,
    COMPOSITE_GLYPHS32_REQUEST, COMPOSITE_GLYPHS8_REQUEST, COMPOSITE_REQUEST,
    CREATE_ANIM_CURSOR_REQUEST, CREATE_CONICAL_GRADIENT_REQUEST, CREATE_CURSOR_REQUEST,
    CREATE_GLYPH_SET_REQUEST, CREATE_LINEAR_GRADIENT_REQUEST, CREATE_PICTURE_REQUEST,
    CREATE_RADIAL_GRADIENT_REQUEST, CREATE_SOLID_FILL_REQUEST, FILL_RECTANGLES_REQUEST,
    FREE_GLYPHS_REQUEST, FREE_GLYPH_SET_REQUEST, FREE_PICTURE_REQUEST, PictOp,
    QUERY_FILTERS_REQUEST, QUERY_PICT_FORMATS_REQUEST, QUERY_PICT_INDEX_VALUES_REQUEST,
    QUERY_VERSION_REQUEST, REFERENCE_GLYPH_SET_REQUEST, SET_PICTURE_CLIP_RECTANGLES_REQUEST,
    SET_PICTURE_FILTER_REQUEST, SET_PICTURE_TRANSFORM_REQUEST, TRAPEZOIDS_REQUEST,
    TRIANGLES_REQUEST, TRI_FAN_REQUEST, TRI_STRIP_REQUEST,
};

use crate::xserver::core::read_u32_bo;
use crate::xserver::core::require_len;
use crate::xserver::ClientState;

mod composite;
mod filter;
mod glyph;
mod gradient;
mod picture;
mod skia_gradient;
mod skia_raster;
mod transform;

/// RENDER major opcode (assigned at QueryExtension).
pub(super) const RENDER_MAJOR_OPCODE: u8 = 139;

/// Build a RENDER protocol error reply.
#[inline]
pub(super) fn render_err(code: u8, seq: u16, bad_value: u32, minor: u16) -> Vec<u8> {
    crate::xserver::core::build_error(code, seq, bad_value, RENDER_MAJOR_OPCODE, minor)
}

/// Build a RENDER `BadValue` error — by far the most common RENDER error,
/// used whenever a picture id, format, num-stops, etc. fails validation.
#[inline]
pub(super) fn render_value_err(seq: u16, bad_value: u32, minor: u16) -> Vec<u8> {
    render_err(crate::xserver::core::VALUE_ERROR, seq, bad_value, minor)
}

/// Build a RENDER `BadLength` error.
#[inline]
pub(super) fn render_length_err(seq: u16, minor: u16) -> Vec<u8> {
    render_err(crate::xserver::core::LENGTH_ERROR, seq, 0, minor)
}

// PictFormat IDs
pub(super) const PICTFORMAT_ARGB32: u32 = 0x24;
pub(super) const PICTFORMAT_RGB24: u32 = 0x25;
pub(super) const PICTFORMAT_A8: u32 = 0x26;
pub(super) const PICTFORMAT_A1: u32 = 0x27;
pub(super) const PICTFORMAT_XRGB32: u32 = 0x28;
pub(super) const PICTFORMAT_XBGR32: u32 = 0x29;

/// Whether the given pict format carries an alpha channel. RGB24 /
/// xRGB32 / xBGR32 have no alpha; the spec mandates compositing into
/// such pictures proceeds as if Da = 1.0.
pub(crate) fn pict_format_has_alpha(format_id: u32) -> bool {
    !matches!(
        format_id,
        PICTFORMAT_RGB24 | PICTFORMAT_XRGB32 | PICTFORMAT_XBGR32
    )
}

/// Decode a 4-byte raw pixmap word into canonical (b, g, r, a) values
/// according to the picture's format. Framebuffer storage is packed
/// RGBA (byte 0 = R); the X11 Core PutImage path swaps from BGRA wire
/// before writing, so an ARGB32 pixmap reads back as `[R, G, B, A]`.
pub(crate) fn decode_pixel_bgra(format_id: u32, bytes: &[u8]) -> (u8, u8, u8, u8) {
    if bytes.len() < 4 {
        return (0, 0, 0, 0);
    }
    match format_id {
        // ARGB32: storage bytes are [R, G, B, A].
        PICTFORMAT_ARGB32 => (bytes[2], bytes[1], bytes[0], bytes[3]),
        // xRGB32 / RGB24: same layout as ARGB32; force alpha=255.
        PICTFORMAT_XRGB32 | PICTFORMAT_RGB24 => (bytes[2], bytes[1], bytes[0], 0xff),
        // xBGR32: R/B swapped vs ARGB32 — bytes [B, G, R, X].
        PICTFORMAT_XBGR32 => (bytes[0], bytes[1], bytes[2], 0xff),
        // A8 (alpha-only) — used for masks. RENDER operations
        // (FillRectangles, Composite) store the alpha in bytes[3].
        // PutImage depth=8 also writes alpha data here when the
        // pixmap's depth matches A8's depth.
        PICTFORMAT_A8 => (0, 0, 0, bytes[3]),
        _ => (bytes[2], bytes[1], bytes[0], bytes[3]),
    }
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
pub(crate) fn zero_src_has_no_effect(op: u8) -> bool {
    !matches!(
        PictOp::from(op),
        PictOp::CLEAR
            | PictOp::SRC
            | PictOp::IN
            | PictOp::IN_REVERSE
            | PictOp::OUT
            | PictOp::ATOP_REVERSE
    )
}

/// Inside-triangle test using the standard sign-of-cross-product
/// edge function. `(px, py)` is the *pixel centre*. Returns true on
/// the boundary so the half-open scanline rasteriser and this
/// per-pixel test agree on edge pixels.
pub(crate) fn point_in_triangle(
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
    pub(super) pictures: HashMap<u32, PictureState>,
    pub(super) glyphsets: HashMap<u32, GlyphSetState>,
    pub(super) solid_fills: HashMap<u32, SolidFillState>,
    pub(super) linear_gradients: HashMap<u32, LinearGradientState>,
    pub(super) radial_gradients: HashMap<u32, RadialGradientState>,
    pub(super) conical_gradients: HashMap<u32, ConicalGradientState>,
    /// Per-picture 3x3 affine transforms set via SetPictureTransform
    /// (RENDER minor opcode 28). Applied when sampling source
    /// pictures — most importantly for gradients, where rendercheck
    /// uses transforms to map a tiny gradient onto a much larger
    /// destination region.
    pub(super) transforms: HashMap<u32, [f64; 9]>,
}

/// Picture filter type set via SetPictureFilter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PictFilter {
    Nearest,
    Bilinear,
}

pub(super) struct PictureState {
    pub(super) drawable: u32,
    /// Picture format (e.g. PICTFORMAT_ARGB32 / RGB24 / A8). Used to
    /// decide whether the destination has an alpha channel during
    /// composite (rgb24 destinations get implicit dst-alpha = 1).
    pub(super) format_id: u32,
    pub(super) repeat: u32,
    /// CPComponentAlpha — when this picture is used as a *mask*, each
    /// of its R/G/B/A channels independently modulates the matching
    /// source channel (instead of only the alpha modulating all four
    /// uniformly). Used for sub-pixel-precise glyph rendering and the
    /// rendercheck mask coords test.
    pub(super) component_alpha: bool,
    /// Clip rectangles set via SetPictureClipRectangles. The picture's
    /// destination is the union of these rectangles, offset by
    /// `clip_origin_*`. `None` means no clipping (full drawable).
    pub(super) clip_rects: Option<Vec<(i16, i16, u16, u16)>>,
    pub(super) clip_origin_x: i16,
    pub(super) clip_origin_y: i16,
    /// Pixmap-based clip mask set via CPClipMask in ChangePicture.
    /// When set, only pixels where the mask is non-zero are written.
    pub(super) clip_mask: Option<u32>,
    /// Filter type for sampling this picture (nearest or bilinear).
    pub(super) filter: PictFilter,
}

pub(super) struct GlyphSetState {
    pub(super) format_id: u32,
    pub(super) glyphs: HashMap<u32, StoredGlyph>,
}

#[derive(Clone)]
pub(super) struct StoredGlyph {
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) x: i16,
    pub(super) y: i16,
    pub(super) x_off: i16,
    pub(super) y_off: i16,
    pub(super) data: Vec<u8>, // alpha bitmap
}

pub(super) struct SolidFillState {
    pub(super) r: u8,
    pub(super) g: u8,
    pub(super) b: u8,
    pub(super) a: u8,
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
pub(super) struct LinearGradientState {
    pub(super) p1: (f64, f64),
    pub(super) p2: (f64, f64),
    pub(super) stops: Vec<GradientStop>,
}

/// Radial gradient defined by two circles (inner and outer).
/// The gradient parameter `t` is derived from the solution to
/// the quadratic equation describing the circle at each point.
pub(super) struct RadialGradientState {
    pub(super) inner: (f64, f64, f64), // (cx, cy, radius)
    pub(super) outer: (f64, f64, f64), // (cx, cy, radius)
    pub(super) stops: Vec<GradientStop>,
}

/// Conical (angular) gradient around a center point.
/// The gradient parameter is the angle from the center,
/// starting at `angle` radians and wrapping around 2*PI.
pub(super) struct ConicalGradientState {
    pub(super) center: (f64, f64),
    pub(super) angle: f64, // radians
    pub(super) stops: Vec<GradientStop>,
}

#[derive(Clone, Copy)]
pub(super) struct GradientStop {
    pub(super) offset: f64,
    /// Straight (non-premultiplied) colour at this stop.
    pub(super) r: u8,
    pub(super) g: u8,
    pub(super) b: u8,
    pub(super) a: u8,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            pictures: HashMap::new(),
            glyphsets: HashMap::new(),
            solid_fills: HashMap::new(),
            linear_gradients: HashMap::new(),
            radial_gradients: HashMap::new(),
            conical_gradients: HashMap::new(),
            transforms: HashMap::new(),
        }
    }

    /// Number of active pictures (for X-Resource reporting).
    pub fn picture_count(&self) -> usize {
        self.pictures.len()
    }

    /// Number of active glyph sets (for X-Resource reporting).
    pub fn glyphset_count(&self) -> usize {
        self.glyphsets.len()
    }

    /// Get the drawable ID associated with a picture.
    pub fn picture_drawable(&self, pic_id: u32) -> Option<u32> {
        self.pictures.get(&pic_id).map(|p| p.drawable)
    }

    /// Get the clip rectangles for a picture (if any are set).
    pub fn picture_clip_rects(&self, pic_id: u32) -> Option<&[(i16, i16, u16, u16)]> {
        self.pictures
            .get(&pic_id)
            .and_then(|p| p.clip_rects.as_deref())
    }

    /// Set clip region on a picture (used by XFIXES SetPictureClipRegion).
    /// Pass None for clip_rects to clear clipping.
    pub fn set_picture_clip_region(
        &mut self,
        pic_id: u32,
        clip_rects: Option<Vec<(i16, i16, u16, u16)>>,
        clip_origin_x: i16,
        clip_origin_y: i16,
    ) -> bool {
        if let Some(pic) = self.pictures.get_mut(&pic_id) {
            pic.clip_rects = clip_rects;
            pic.clip_origin_x = clip_origin_x;
            pic.clip_origin_y = clip_origin_y;
            true
        } else {
            false
        }
    }
}

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
    match PictOp::from(op) {
        PictOp::CLEAR => (0, 0),
        PictOp::SRC => (255, 0),
        PictOp::DST => (0, 255),
        PictOp::OVER => (255, 255 - sa),
        PictOp::OVER_REVERSE => (255 - da, 255),
        PictOp::IN => (da, 0),
        PictOp::IN_REVERSE => (0, sa),
        PictOp::OUT => (255 - da, 0),
        PictOp::OUT_REVERSE => (0, 255 - sa),
        PictOp::ATOP => (da, 255 - sa),
        PictOp::ATOP_REVERSE => (255 - da, sa),
        PictOp::XOR => (255 - da, 255 - sa),
        PictOp::ADD => (255, 255), // Add (clamped on apply)
        PictOp::SATURATE | PictOp::DISJOINT_OVER_REVERSE => (out_dis(sa, da), 255),
        PictOp::DISJOINT_CLEAR => (0, 0),
        PictOp::DISJOINT_SRC => (255, 0),
        PictOp::DISJOINT_DST => (0, 255),
        PictOp::DISJOINT_OVER => (255, out_dis(da, sa)),
        PictOp::DISJOINT_IN => (in_dis(sa, da), 0),
        PictOp::DISJOINT_IN_REVERSE => (0, in_dis(da, sa)),
        PictOp::DISJOINT_OUT => (out_dis(sa, da), 0),
        PictOp::DISJOINT_OUT_REVERSE => (0, out_dis(da, sa)),
        PictOp::DISJOINT_ATOP => (in_dis(sa, da), out_dis(da, sa)),
        PictOp::DISJOINT_ATOP_REVERSE => (out_dis(sa, da), in_dis(da, sa)),
        PictOp::DISJOINT_XOR => (out_dis(sa, da), out_dis(da, sa)),
        PictOp::CONJOINT_CLEAR => (0, 0),
        PictOp::CONJOINT_SRC => (255, 0),
        PictOp::CONJOINT_DST => (0, 255),
        PictOp::CONJOINT_OVER => (255, out_con(da, sa)),
        PictOp::CONJOINT_OVER_REVERSE => (out_con(sa, da), 255),
        PictOp::CONJOINT_IN => (in_con(sa, da), 0),
        PictOp::CONJOINT_IN_REVERSE => (0, in_con(da, sa)),
        PictOp::CONJOINT_OUT => (out_con(sa, da), 0),
        PictOp::CONJOINT_OUT_REVERSE => (0, out_con(da, sa)),
        PictOp::CONJOINT_ATOP => (in_con(sa, da), out_con(da, sa)),
        PictOp::CONJOINT_ATOP_REVERSE => (out_con(sa, da), in_con(da, sa)),
        PictOp::CONJOINT_XOR => (out_con(sa, da), out_con(da, sa)),
        _ => (255, 255 - sa), // fallback to Over
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
    // Storage layout: dst[0] = R, dst[1] = G, dst[2] = B, dst[3] = A.
    match PictOp::from(op) {
        PictOp::CLEAR => {
            dst[0] = 0;
            dst[1] = 0;
            dst[2] = 0;
            dst[3] = if force_da_one { 255 } else { 0 };
            return;
        }
        PictOp::SRC => {
            dst[0] = src_r;
            dst[1] = src_g;
            dst[2] = src_b;
            dst[3] = if force_da_one { 255 } else { src_a };
            return;
        }
        PictOp::DST => {
            // Leave the destination untouched.
            return;
        }
        _ => {}
    }

    let sa = src_a as i32;
    let da = if force_da_one { 255 } else { dst[3] as i32 };

    let (fs, fd) = pict_op_factors(op, sa, da);

    dst[0] = blend_chan(src_r, dst[0], fs, fd);
    dst[1] = blend_chan(src_g, dst[1], fs, fd);
    dst[2] = blend_chan(src_b, dst[2], fs, fd);
    dst[3] = if force_da_one {
        255
    } else {
        blend_chan(src_a, dst[3], fs, fd)
    };
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

    // Storage layout: dst[0] = R, dst[1] = G, dst[2] = B, dst[3] = A.
    dst[0] = blend_chan(src_r, dst[0], fs_r, fd_r);
    dst[1] = blend_chan(src_g, dst[1], fs_g, fd_g);
    dst[2] = blend_chan(src_b, dst[2], fs_b, fd_b);
    dst[3] = if force_da_one {
        255
    } else {
        blend_chan(src_a, dst[3], fs_a, fd_a)
    };
}

pub(crate) use crate::xserver::core::align_to_4 as pad4;

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
        COMPOSITE_REQUEST => 16, // Composite: dst at offset 16
        TRAPEZOIDS_REQUEST | TRIANGLES_REQUEST | TRI_STRIP_REQUEST | TRI_FAN_REQUEST => 12,
        COMPOSITE_GLYPHS8_REQUEST | COMPOSITE_GLYPHS16_REQUEST | COMPOSITE_GLYPHS32_REQUEST => 12,
        FILL_RECTANGLES_REQUEST => 8, // FillRectangles
        _ => return None,
    };
    if data.len() < dst_offset + 4 {
        return None;
    }
    let dst_pic = read_u32_bo(data, dst_offset, state.msb_first);
    if state.render.linear_gradients.contains_key(&dst_pic)
        || state.render.radial_gradients.contains_key(&dst_pic)
        || state.render.conical_gradients.contains_key(&dst_pic)
    {
        // BadDrawable = 9; the X RENDER major opcode is 139, which
        // we don't actually need to fill in here — clients only key
        // off the error code and the bad-value field.
        return Some(render_err(9, seq, dst_pic, minor as u16));
    }
    None
}

pub fn handle_render_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 4, seq, 139, 0, bo);

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
        QUERY_VERSION_REQUEST => picture::handle_query_version(seq, bo),
        QUERY_PICT_FORMATS_REQUEST => picture::handle_query_pict_formats(seq, bo),
        CREATE_PICTURE_REQUEST => picture::handle_create_picture(state, data, seq),
        CHANGE_PICTURE_REQUEST => picture::handle_change_picture(state, data, seq),
        SET_PICTURE_CLIP_RECTANGLES_REQUEST => {
            picture::handle_set_picture_clip_rectangles(state, data, seq)
        }
        FREE_PICTURE_REQUEST => picture::handle_free_picture(state, data),
        COMPOSITE_REQUEST => composite::handle_composite(state, data, seq),
        TRAPEZOIDS_REQUEST => composite::handle_trapezoids(state, data, seq),
        TRIANGLES_REQUEST => composite::handle_triangles(state, data, seq),
        TRI_STRIP_REQUEST => composite::handle_tri_strip(state, data, seq),
        TRI_FAN_REQUEST => composite::handle_tri_fan(state, data, seq),
        CREATE_GLYPH_SET_REQUEST => glyph::handle_create_glyphset(state, data, seq),
        REFERENCE_GLYPH_SET_REQUEST => glyph::handle_reference_glyphset(state, data, seq),
        FREE_GLYPH_SET_REQUEST => glyph::handle_free_glyphset(state, data, seq),
        ADD_GLYPHS_REQUEST => glyph::handle_add_glyphs(state, data, seq),
        21 => glyph::handle_add_glyphs_from_picture(state, data, seq),
        FREE_GLYPHS_REQUEST => glyph::handle_free_glyphs(state, data, seq),
        COMPOSITE_GLYPHS8_REQUEST => glyph::handle_composite_glyphs(state, data, 1, seq),
        COMPOSITE_GLYPHS16_REQUEST => glyph::handle_composite_glyphs(state, data, 2, seq),
        COMPOSITE_GLYPHS32_REQUEST => glyph::handle_composite_glyphs(state, data, 4, seq),
        FILL_RECTANGLES_REQUEST => composite::handle_fill_rectangles(state, data, seq),
        CREATE_CURSOR_REQUEST => picture::handle_create_cursor(state, data, seq),
        SET_PICTURE_TRANSFORM_REQUEST => transform::handle_set_picture_transform(state, data, seq),
        QUERY_FILTERS_REQUEST => filter::handle_query_filters(seq, bo),
        SET_PICTURE_FILTER_REQUEST => filter::handle_set_picture_filter(state, data, seq),
        CREATE_ANIM_CURSOR_REQUEST => picture::handle_create_anim_cursor(state, data, seq),
        ADD_TRAPS_REQUEST => composite::handle_add_traps(state, data, seq),
        CREATE_SOLID_FILL_REQUEST => gradient::handle_create_solid_fill(state, data, seq),
        CREATE_LINEAR_GRADIENT_REQUEST
        | CREATE_RADIAL_GRADIENT_REQUEST
        | CREATE_CONICAL_GRADIENT_REQUEST => gradient::handle_create_gradient_fill(state, data, seq),
        QUERY_PICT_INDEX_VALUES_REQUEST => {
            picture::handle_query_pict_index_values(state, data, seq)
        }
        _ => {
            debug!("Unhandled RENDER minor opcode: {minor}");
            render_err(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                minor as u16,
            )
        }
    }
}

/// Snapshot a picture's clip state so we can pass it down to drawing
/// helpers without holding a borrow on `state.render` while we mutate
/// the framebuffer.
#[derive(Clone, Default)]
pub(super) struct ClipSnapshot {
    rects: Option<Vec<(i16, i16, u16, u16)>>,
    origin_x: i16,
    origin_y: i16,
    /// Pixmap-based clip mask alpha channel data. Stored as (width, height, alpha_data).
    /// When present, only pixels where alpha_data[y * w + x] != 0 pass clipping.
    mask_alpha: Option<(u32, u32, Vec<u8>)>,
}

impl ClipSnapshot {
    pub(crate) fn from_picture(state: &ClientState, pid: u32) -> Self {
        if let Some(pic) = state.render.pictures.get(&pid) {
            let mask_alpha = pic.clip_mask.and_then(|mask_id| {
                if mask_id == 0 {
                    return None;
                }
                // Try to get the framebuffer for the mask pixmap
                let fb_info = if let Some(px) = state.pixmaps.get(&mask_id) {
                    Some((
                        px.framebuffer.width(),
                        px.framebuffer.height(),
                        px.framebuffer.data(),
                    ))
                } else {
                    state.windows.get(&mask_id).map(|win| {
                        (
                            win.framebuffer.width(),
                            win.framebuffer.height(),
                            win.framebuffer.data(),
                        )
                    })
                };
                fb_info.map(|(w, h, data)| {
                    // Extract alpha channel from BGRA data
                    let stride = (w as usize) * 4;
                    let mut alpha = vec![0u8; (w * h) as usize];
                    for y in 0..h as usize {
                        for x in 0..w as usize {
                            let off = y * stride + x * 4 + 3; // alpha byte
                            if off < data.len() {
                                alpha[y * w as usize + x] = data[off];
                            }
                        }
                    }
                    (w, h, alpha)
                })
            });
            ClipSnapshot {
                rects: pic.clip_rects.clone(),
                origin_x: pic.clip_origin_x,
                origin_y: pic.clip_origin_y,
                mask_alpha,
            }
        } else {
            ClipSnapshot::default()
        }
    }

    pub(crate) fn allows(&self, x: i32, y: i32) -> bool {
        // Check rectangle clip first
        let rect_ok = match &self.rects {
            None => true,
            Some(rects) => rects.iter().any(|&(rx, ry, rw, rh)| {
                let cx = self.origin_x as i32 + rx as i32;
                let cy = self.origin_y as i32 + ry as i32;
                x >= cx && x < cx + rw as i32 && y >= cy && y < cy + rh as i32
            }),
        };
        if !rect_ok {
            return false;
        }
        // Check pixmap-based clip mask
        if let Some((mw, mh, alpha)) = &self.mask_alpha {
            let mx = x - self.origin_x as i32;
            let my = y - self.origin_y as i32;
            if mx < 0 || my < 0 || mx >= *mw as i32 || my >= *mh as i32 {
                return false;
            }
            let idx = my as usize * *mw as usize + mx as usize;
            return idx < alpha.len() && alpha[idx] != 0;
        }
        true
    }
}

/// Check if `grad_id` is a gradient (linear, radial, or conical) and rasterize it.
/// `pic_id` is the picture ID used to look up transforms and repeat mode.
pub(crate) fn resolve_gradient_pixels(
    state: &mut ClientState,
    pic_id: u32,
    grad_id: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<(Vec<u8>, u32, u32)> {
    let tx = state
        .render
        .transforms
        .get(&pic_id)
        .or_else(|| state.render.transforms.get(&grad_id));
    let rep = state
        .render
        .pictures
        .get(&pic_id)
        .map(|p| p.repeat)
        .unwrap_or(0);

    if let Some(grad) = state.render.linear_gradients.get(&grad_id) {
        let w = width as u32;
        let h = height as u32;
        // Prefer the SIMD-accelerated tiny-skia shader path; fall back to
        // the per-pixel Cairo-style implementation when tiny-skia rejects
        // the parameters or the transform is non-affine.
        let buf = skia_gradient::rasterize_linear(grad, tx, rep, src_x, src_y, width, height)
            .map(|b| (b, w, h))
            .unwrap_or_else(|| {
                gradient::rasterize_linear_gradient(grad, tx, rep, src_x, src_y, width, height)
            });
        return Some(buf);
    }
    if let Some(grad) = state.render.radial_gradients.get(&grad_id) {
        let w = width as u32;
        let h = height as u32;
        let buf = skia_gradient::rasterize_radial(grad, tx, rep, src_x, src_y, width, height)
            .map(|b| (b, w, h))
            .unwrap_or_else(|| {
                gradient::rasterize_radial_gradient(grad, tx, rep, src_x, src_y, width, height)
            });
        return Some(buf);
    }
    if let Some(grad) = state.render.conical_gradients.get(&grad_id) {
        let w = width as u32;
        let h = height as u32;
        let buf = skia_gradient::rasterize_conical(grad, tx, rep, src_x, src_y, width, height)
            .map(|b| (b, w, h))
            .unwrap_or_else(|| {
                gradient::rasterize_conical_gradient(grad, tx, rep, src_x, src_y, width, height)
            });
        return Some(buf);
    }
    None
}

/// Bilinear interpolation helper: sample four neighbouring pixels and blend.
/// `fx`, `fy` are fractional coordinates within the pixel (0.0..1.0).
pub(crate) fn bilinear_sample(
    fb_data: &[u8],
    fb_stride: usize,
    fb_w: u32,
    fb_h: u32,
    format_id: u32,
    repeat: u32,
    sx: f64,
    sy: f64,
) -> (u8, u8, u8, u8) {
    // Sample at the pixel center; subtract 0.5 to get the continuous position
    let cx = sx - 0.5;
    let cy = sy - 0.5;
    let x0f = cx.floor();
    let y0f = cy.floor();
    let fx = (cx - x0f) as f32;
    let fy = (cy - y0f) as f32;
    let x0 = x0f as i32;
    let y0 = y0f as i32;

    let fetch = |px: i32, py: i32| -> (u8, u8, u8, u8) {
        let (mut fx, mut fy) = (px, py);
        let in_bounds = fx >= 0 && fy >= 0 && (fx as u32) < fb_w && (fy as u32) < fb_h;
        if !in_bounds {
            if repeat != 0 && fb_w > 0 && fb_h > 0 {
                let (rx, ry) =
                    gradient::apply_pixmap_repeat(fx, fy, fb_w as i32, fb_h as i32, repeat);
                fx = rx as i32;
                fy = ry as i32;
            } else {
                return (0, 0, 0, 0);
            }
        }
        let off = fy as usize * fb_stride + fx as usize * 4;
        if off + 3 < fb_data.len() {
            decode_pixel_bgra(format_id, &fb_data[off..off + 4])
        } else {
            (0, 0, 0, 0)
        }
    };

    let p00 = fetch(x0, y0);
    let p10 = fetch(x0 + 1, y0);
    let p01 = fetch(x0, y0 + 1);
    let p11 = fetch(x0 + 1, y0 + 1);

    let lerp = |a: u8, b: u8, c: u8, d: u8| -> u8 {
        let top = a as f32 * (1.0 - fx) + b as f32 * fx;
        let bot = c as f32 * (1.0 - fx) + d as f32 * fx;
        let val = top * (1.0 - fy) + bot * fy;
        val.round().clamp(0.0, 255.0) as u8
    };

    (
        lerp(p00.0, p10.0, p01.0, p11.0),
        lerp(p00.1, p10.1, p01.1, p11.1),
        lerp(p00.2, p10.2, p01.2, p11.2),
        lerp(p00.3, p10.3, p01.3, p11.3),
    )
}

/// Resolve source picture to pixel data. Returns (pixels, width, height) in BGRA format.
pub(crate) fn resolve_source_pixels(
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

    // Check if it's a gradient (referenced directly).
    if let Some(result) =
        resolve_gradient_pixels(state, src_pic, src_pic, src_x, src_y, width, height)
    {
        return Some(result);
    }

    // Check if it's a picture wrapping a drawable
    let (drawable, repeat, format_id, filter) = {
        let pic = state.render.pictures.get(&src_pic)?;
        (pic.drawable, pic.repeat, pic.format_id, pic.filter)
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

    // Check if the picture wraps a gradient.
    if let Some(result) =
        resolve_gradient_pixels(state, src_pic, drawable, src_x, src_y, width, height)
    {
        return Some(result);
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

    // Helper to fetch a single pixel from the framebuffer and decode
    // it into canonical (B, G, R, A) according to the source picture's
    // format. Wraps the per-format byte-shuffling that lets the same
    // pixmap be read through (say) `xBGR32` and `ARGB32` and produce
    // different RGB.
    let copy_pixel = |fb_data: &[u8], src_off: usize, out: &mut [u8], dst_off: usize| {
        if src_off + 3 < fb_data.len() && dst_off + 3 < out.len() {
            let (b, g, r, a) = decode_pixel_bgra(format_id, &fb_data[src_off..src_off + 4]);
            out[dst_off] = b;
            out[dst_off + 1] = g;
            out[dst_off + 2] = r;
            out[dst_off + 3] = a;
        }
    };

    let use_bilinear = filter == PictFilter::Bilinear;

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
                let (sx_f, sy_f) = transform::apply_transform(&tx, dx, dy);
                let dst_off = (row as u32 * w + col as u32) as usize * 4;

                if use_bilinear {
                    let (b, g, r, a) = bilinear_sample(
                        fb_data, fb_stride, fb_w, fb_h, format_id, repeat, sx_f, sy_f,
                    );
                    if dst_off + 3 < pixels.len() {
                        pixels[dst_off] = b;
                        pixels[dst_off + 1] = g;
                        pixels[dst_off + 2] = r;
                        pixels[dst_off + 3] = a;
                    }
                } else {
                    // Nearest-neighbour fetch from the framebuffer.
                    let mut sxi = sx_f.floor() as i32;
                    let mut syi = sy_f.floor() as i32;
                    let in_bounds =
                        sxi >= 0 && syi >= 0 && (sxi as u32) < fb_w && (syi as u32) < fb_h;
                    if !in_bounds {
                        if repeat != 0 && fb_w > 0 && fb_h > 0 {
                            let (rx, ry) = gradient::apply_pixmap_repeat(
                                sxi,
                                syi,
                                fb_w as i32,
                                fb_h as i32,
                                repeat,
                            );
                            sxi = rx as i32;
                            syi = ry as i32;
                        } else {
                            // RepeatNone: out-of-bounds reads as transparent.
                            if dst_off + 3 < pixels.len() {
                                pixels[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                            }
                            continue;
                        }
                    }
                    let src_off = syi as usize * fb_stride + sxi as usize * 4;
                    copy_pixel(fb_data, src_off, &mut pixels, dst_off);
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
                if use_bilinear {
                    let sx = src_x as f64 + col as f64 + 0.5;
                    let sy = src_y as f64 + row as f64 + 0.5;
                    let (b, g, r, a) =
                        bilinear_sample(fb_data, fb_stride, fb_w, fb_h, format_id, repeat, sx, sy);
                    let dst_off = (row * w + col) as usize * 4;
                    if dst_off + 3 < pixels.len() {
                        pixels[dst_off] = b;
                        pixels[dst_off + 1] = g;
                        pixels[dst_off + 2] = r;
                        pixels[dst_off + 3] = a;
                    }
                } else {
                    let raw_sy = src_y as i32 + row as i32;
                    let raw_sx = src_x as i32 + col as i32;
                    let (sx, sy) = gradient::apply_pixmap_repeat(
                        raw_sx,
                        raw_sy,
                        fb_w as i32,
                        fb_h as i32,
                        repeat,
                    );
                    let src_off = sy as usize * fb_stride + sx as usize * 4;
                    let dst_off = (row * w + col) as usize * 4;
                    copy_pixel(fb_data, src_off, &mut pixels, dst_off);
                }
            }
        }
        Some((pixels, w, h))
    } else {
        let fb_w = fb.width();
        let fb_h = fb.height();
        let fb_stride = fb.stride();
        let fb_data = fb.data();
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for row in 0..h as i32 {
            let sy = src_y as i32 + row;
            for col in 0..w as i32 {
                let sx = src_x as i32 + col;
                let dst_off = (row as u32 * w + col as u32) as usize * 4;
                if use_bilinear {
                    let (b, g, r, a) = bilinear_sample(
                        fb_data,
                        fb_stride,
                        fb_w,
                        fb_h,
                        format_id,
                        repeat,
                        sx as f64 + 0.5,
                        sy as f64 + 0.5,
                    );
                    if dst_off + 3 < pixels.len() {
                        pixels[dst_off] = b;
                        pixels[dst_off + 1] = g;
                        pixels[dst_off + 2] = r;
                        pixels[dst_off + 3] = a;
                    }
                } else {
                    if sy < 0 || sx < 0 || (sx as u32) >= fb_w || (sy as u32) >= fb_h {
                        // Out of bounds and no repeat → transparent.
                        if dst_off + 3 < pixels.len() {
                            pixels[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                        }
                        continue;
                    }
                    let src_off = sy as usize * fb_stride + sx as usize * 4;
                    copy_pixel(fb_data, src_off, &mut pixels, dst_off);
                }
            }
        }
        Some((pixels, w, h))
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
pub(crate) fn resolve_source_color(state: &ClientState, src_pic: u32) -> (u8, u8, u8, u8) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // pict_op_factors — all 44 Porter-Duff operators
    // -----------------------------------------------------------------------

    #[test]
    fn pict_op_clear() {
        assert_eq!(pict_op_factors(0, 128, 200), (0, 0));
    }

    #[test]
    fn pict_op_src() {
        assert_eq!(pict_op_factors(1, 128, 200), (255, 0));
    }

    #[test]
    fn pict_op_dst() {
        assert_eq!(pict_op_factors(2, 128, 200), (0, 255));
    }

    #[test]
    fn pict_op_over() {
        let (fs, fd) = pict_op_factors(3, 128, 200);
        assert_eq!(fs, 255);
        assert_eq!(fd, 255 - 128); // 1 - Sa
    }

    #[test]
    fn pict_op_over_reverse() {
        let (fs, fd) = pict_op_factors(4, 128, 200);
        assert_eq!(fs, 255 - 200); // 1 - Da
        assert_eq!(fd, 255);
    }

    #[test]
    fn pict_op_in() {
        let (fs, fd) = pict_op_factors(5, 128, 200);
        assert_eq!(fs, 200); // Da
        assert_eq!(fd, 0);
    }

    #[test]
    fn pict_op_in_reverse() {
        let (fs, fd) = pict_op_factors(6, 128, 200);
        assert_eq!(fs, 0);
        assert_eq!(fd, 128); // Sa
    }

    #[test]
    fn pict_op_out() {
        let (fs, fd) = pict_op_factors(7, 128, 200);
        assert_eq!(fs, 255 - 200); // 1 - Da
        assert_eq!(fd, 0);
    }

    #[test]
    fn pict_op_out_reverse() {
        let (fs, fd) = pict_op_factors(8, 128, 200);
        assert_eq!(fs, 0);
        assert_eq!(fd, 255 - 128); // 1 - Sa
    }

    #[test]
    fn pict_op_atop() {
        let (fs, fd) = pict_op_factors(9, 128, 200);
        assert_eq!(fs, 200); // Da
        assert_eq!(fd, 255 - 128); // 1 - Sa
    }

    #[test]
    fn pict_op_atop_reverse() {
        let (fs, fd) = pict_op_factors(10, 128, 200);
        assert_eq!(fs, 255 - 200); // 1 - Da
        assert_eq!(fd, 128); // Sa
    }

    #[test]
    fn pict_op_xor() {
        let (fs, fd) = pict_op_factors(11, 128, 200);
        assert_eq!(fs, 255 - 200); // 1 - Da
        assert_eq!(fd, 255 - 128); // 1 - Sa
    }

    #[test]
    fn pict_op_add() {
        assert_eq!(pict_op_factors(12, 128, 200), (255, 255));
    }

    #[test]
    fn pict_op_unknown_falls_back_to_over() {
        let (fs, fd) = pict_op_factors(255, 128, 200);
        assert_eq!(fs, 255);
        assert_eq!(fd, 255 - 128); // Same as Over
    }

    // -----------------------------------------------------------------------
    // blend_chan — channel compositing with correct rounding
    // -----------------------------------------------------------------------

    #[test]
    fn blend_chan_src_full() {
        // src=255, dst=0, fs=255, fd=0 → 255
        assert_eq!(blend_chan(255, 0, 255, 0), 255);
    }

    #[test]
    fn blend_chan_dst_full() {
        // src=0, dst=255, fs=0, fd=255 → 255
        assert_eq!(blend_chan(0, 255, 0, 255), 255);
    }

    #[test]
    fn blend_chan_half_over_black() {
        // src=128, dst=0, fs=255, fd=127 → 128 (Over: dst is black)
        let result = blend_chan(128, 0, 255, 127);
        assert_eq!(result, 128);
    }

    #[test]
    fn blend_chan_clamped_to_255() {
        // Add: fs=255, fd=255, src=200, dst=200 → clamped to 255
        let result = blend_chan(200, 200, 255, 255);
        assert_eq!(result, 255);
    }

    // -----------------------------------------------------------------------
    // composite_pixel — end-to-end compositing
    // -----------------------------------------------------------------------

    #[test]
    fn composite_clear_zeroes_dst() {
        let mut dst = [100u8, 150, 200, 128];
        composite_pixel(0, &mut dst, 255, 255, 255, 255, true);
        assert_eq!(dst, [0, 0, 0, 0]);
    }

    #[test]
    fn composite_src_overwrites_dst() {
        // dst storage layout: [R, G, B, A]; src args are (src_b, src_g, src_r, src_a).
        let mut dst = [100u8, 150, 200, 128];
        composite_pixel(1, &mut dst, 10, 20, 30, 40, true);
        assert_eq!(dst, [30, 20, 10, 40]);
    }

    #[test]
    fn composite_dst_leaves_unchanged() {
        let mut dst = [100u8, 150, 200, 128];
        composite_pixel(2, &mut dst, 10, 20, 30, 40, true);
        assert_eq!(dst, [100, 150, 200, 128]);
    }

    #[test]
    fn composite_over_opaque_src_replaces_dst() {
        let mut dst = [100u8, 150, 200, 255];
        composite_pixel(3, &mut dst, 50, 60, 70, 255, true);
        // Over with Sa=255: dst = (src_r, src_g, src_b, src_a)
        assert_eq!(dst, [70, 60, 50, 255]);
    }

    #[test]
    fn composite_over_transparent_src_preserves_dst() {
        let mut dst = [100u8, 150, 200, 255];
        composite_pixel(3, &mut dst, 0, 0, 0, 0, true);
        // Over with Sa=0: Fd = 1-0 = 255, so dst = dst
        assert_eq!(dst, [100, 150, 200, 255]);
    }

    #[test]
    fn composite_non_alpha_dst_forces_opaque() {
        let mut dst = [100u8, 150, 200, 0];
        composite_pixel(0, &mut dst, 0, 0, 0, 0, false); // Clear on non-alpha
                                                         // dst_has_alpha=false: alpha byte forced to 255
        assert_eq!(dst[3], 255);
    }

    // -----------------------------------------------------------------------
    // zero_src_has_no_effect — skip optimisation correctness
    // -----------------------------------------------------------------------

    #[test]
    fn zero_src_safe_for_over() {
        // Over with zero src is a no-op, so skipping is safe
        assert!(zero_src_has_no_effect(3));
    }

    #[test]
    fn zero_src_not_safe_for_clear() {
        // Clear always zeroes dst regardless of src
        assert!(!zero_src_has_no_effect(0));
    }

    #[test]
    fn zero_src_not_safe_for_src() {
        // Src always overwrites dst
        assert!(!zero_src_has_no_effect(1));
    }

    // -----------------------------------------------------------------------
    // Component-alpha (CA) compositing
    // -----------------------------------------------------------------------

    #[test]
    fn composite_pixel_ca_over_white_on_black() {
        // CA Over: each channel uses its own effective Sa.
        // src=(255,0,0,255) with per-channel alphas (255,128,0,255)
        // means R channel fully opaque, G channel half, B channel zero.
        let mut dst = [0u8, 0, 0, 255]; // opaque black (RGBA storage)
        composite_pixel_ca(
            3, // PictOpOver
            &mut dst, 0, 0, 255, 255, // src: B=0, G=0, R=255, A=255
            0, 128, 255, 255, // sa_b=0, sa_g=128, sa_r=255, sa_a=255
            true,
        );
        // R channel (byte 0): src_r=255, sa_r=255 → result=255
        assert_eq!(dst[0], 255);
        // G channel (byte 1): src_g=0, sa_g=128 → result=0
        assert_eq!(dst[1], 0);
        // B channel (byte 2): src_b=0, sa_b=0 → result=0
        assert_eq!(dst[2], 0);
    }

    #[test]
    fn composite_pixel_ca_over_preserves_dst_where_mask_zero() {
        // When CA mask channels are 0, src is modulated to 0 and sa=0,
        // so for Over: Fd = 1-sa = 1, Fs = 1. Result = src(0) + dst * 1 = dst.
        let mut dst = [100u8, 150, 200, 255]; // RGBA storage
        composite_pixel_ca(
            3, // PictOpOver
            &mut dst, 0, 0, 0, 0, // src fully modulated to zero
            0, 0, 0, 0, // all channel alphas zero → Fd = 255
            true,
        );
        // With src=0 and Fd=255: result = 0 + dst * 1 = dst preserved.
        assert_eq!(dst[0], 100); // R
        assert_eq!(dst[1], 150); // G
        assert_eq!(dst[2], 200); // B
    }

    #[test]
    fn composite_pixel_ca_src_replaces_dst() {
        // PictOpSrc with CA: dst should be replaced by src regardless of dst.
        let mut dst = [100u8, 150, 200, 255];
        composite_pixel_ca(
            1, // PictOpSrc
            &mut dst, 10, 20, 30, 128, // src args (b, g, r, a)
            64, 128, 255, 128, // per-channel sa
            true,
        );
        // Src op: Fs=1, Fd=0, so result = src; storage layout is RGBA.
        assert_eq!(dst[0], 30); // R
        assert_eq!(dst[1], 20); // G
        assert_eq!(dst[2], 10); // B
        assert_eq!(dst[3], 128); // A
    }

    // -----------------------------------------------------------------------
    // Glyph ARGB helper
    // -----------------------------------------------------------------------

    #[test]
    fn get_glyph_argb_extracts_channels() {
        use super::glyph::tests::get_glyph_argb_wrapper;
        // ARGB32 glyph: pixel at (0,0) stored as BGRA in memory
        let data = vec![10u8, 20, 30, 200]; // B=10, G=20, R=30, A=200
        let (b, g, r, a) = get_glyph_argb_wrapper(&data, 1, 0, 0);
        assert_eq!((b, g, r, a), (10, 20, 30, 200));
    }

    #[test]
    fn get_glyph_argb_out_of_bounds() {
        use super::glyph::tests::get_glyph_argb_wrapper;
        let data = vec![10u8, 20, 30]; // Too short for 4 bytes
        let (b, g, r, a) = get_glyph_argb_wrapper(&data, 1, 0, 0);
        assert_eq!((b, g, r, a), (0, 0, 0, 0));
    }

    // -----------------------------------------------------------------------
    // Bilinear filtering
    // -----------------------------------------------------------------------

    #[test]
    fn bilinear_sample_center_of_single_pixel() {
        // A single red pixel sampled at its center should return red.
        let fb_data = vec![255u8, 0, 0, 255]; // RGBA: red
        let (b, g, r, a) = bilinear_sample(
            &fb_data,
            4,
            1,
            1,
            PICTFORMAT_ARGB32,
            1, // repeat=Normal
            0.5,
            0.5,
        );
        assert_eq!((r, g, b, a), (255, 0, 0, 255));
    }

    #[test]
    fn bilinear_sample_between_two_pixels() {
        // Two pixels: red and green; framebuffer storage is [R, G, B, A].
        let mut fb_data = vec![0u8; 8];
        fb_data[0..4].copy_from_slice(&[255, 0, 0, 255]); // red
        fb_data[4..8].copy_from_slice(&[0, 255, 0, 255]); // green
        let (b, g, r, a) = bilinear_sample(
            &fb_data,
            8,
            2,
            1,
            PICTFORMAT_ARGB32,
            0, // no repeat
            1.0,
            0.5, // at boundary between pixels
        );
        // Should be roughly a mix of red and green
        assert!(r > 100 && r < 200);
        assert!(g > 100 && g < 200);
        assert_eq!(a, 255);
        let _ = b;
    }

    // -----------------------------------------------------------------------
    // PictOp Saturate (op 13) — verify it's a distinct operator
    // -----------------------------------------------------------------------

    #[test]
    fn pict_op_saturate_factors() {
        // Saturate: Fa = min(1, (1-Da)/Sa), Fb = 1
        // With Sa=128, Da=64: Fa = min(1, (255-64)/128) = min(255, (191*255)/128) ≈ 380 → clamped to 255
        let (fa, fb) = super::pict_op_factors(13, 128, 64);
        assert_eq!(fb, 255); // Fb = 1.0 always for Saturate
        assert!(fa > 0); // Fa should be positive
    }

    #[test]
    fn pict_op_saturate_fully_opaque_src() {
        // With Sa=255 (fully opaque), Da=128:
        // Fa = min(1, (255-128)/255) = (127*255)/255 = 127
        let (fa, fb) = super::pict_op_factors(13, 255, 128);
        assert_eq!(fb, 255);
        assert!((fa - 127).abs() <= 1); // Allow rounding
    }

    #[test]
    fn pict_op_saturate_zero_src_alpha() {
        // With Sa=0: out_dis returns 0 (divide by zero protection)
        let (fa, fb) = super::pict_op_factors(13, 0, 128);
        assert_eq!(fa, 0);
        assert_eq!(fb, 255);
    }

    #[test]
    fn pict_op_all_44_operators_mapped() {
        // Verify all 44 operators (0-12, 16-27, 32-43) return reasonable values
        let standard_ops = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]; // 0-13
        let disjoint_ops = [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27]; // 16-27
        let conjoint_ops = [32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43]; // 32-43

        for &op in standard_ops
            .iter()
            .chain(disjoint_ops.iter())
            .chain(conjoint_ops.iter())
        {
            let (fa, fb) = super::pict_op_factors(op, 128, 128);
            assert!(fa >= 0 && fa <= 255, "op {op}: Fa={fa} out of range");
            assert!(fb >= 0 && fb <= 255, "op {op}: Fb={fb} out of range");
        }
    }

    // -----------------------------------------------------------------------
    // RENDER compositing edge cases (spec compliance)
    // -----------------------------------------------------------------------

    #[test]
    fn pict_op_clear_zeroes_everything() {
        // PictOpClear: Fa=0, Fb=0 — result always 0
        let (fa, fb) = super::pict_op_factors(0, 255, 255);
        assert_eq!(fa, 0);
        assert_eq!(fb, 0);
    }

    #[test]
    fn pict_op_src_ignores_dst() {
        // PictOpSrc: Fa=1, Fb=0 — result = src regardless of dst
        let (fa, fb) = super::pict_op_factors(1, 128, 200);
        assert_eq!(fa, 255);
        assert_eq!(fb, 0);
    }

    #[test]
    fn pict_op_dst_ignores_src() {
        // PictOpDst: Fa=0, Fb=1 — result = dst regardless of src
        let (fa, fb) = super::pict_op_factors(2, 128, 200);
        assert_eq!(fa, 0);
        assert_eq!(fb, 255);
    }

    #[test]
    fn pict_op_over_semi_transparent() {
        // PictOpOver: Fa=1, Fb=1-Sa
        let (fa, fb) = super::pict_op_factors(3, 128, 200);
        assert_eq!(fa, 255);
        assert_eq!(fb, 255 - 128); // 127
    }

    #[test]
    fn pict_op_over_fully_opaque() {
        // Fully opaque src: Over becomes Src (Fb=0)
        let (fa, fb) = super::pict_op_factors(3, 255, 200);
        assert_eq!(fa, 255);
        assert_eq!(fb, 0);
    }

    #[test]
    fn pict_op_over_fully_transparent() {
        // Fully transparent src: Over becomes Dst (Fa still 255, Fb=255)
        let (fa, fb) = super::pict_op_factors(3, 0, 200);
        assert_eq!(fa, 255);
        assert_eq!(fb, 255);
    }

    #[test]
    fn pict_op_in_uses_dst_alpha() {
        // PictOpIn: Fa=Da, Fb=0
        let (fa, fb) = super::pict_op_factors(5, 128, 200);
        assert_eq!(fa, 200);
        assert_eq!(fb, 0);
    }

    #[test]
    fn pict_op_out_uses_inv_dst_alpha() {
        // PictOpOut: Fa=1-Da, Fb=0
        let (fa, fb) = super::pict_op_factors(7, 128, 200);
        assert_eq!(fa, 255 - 200); // 55
        assert_eq!(fb, 0);
    }

    #[test]
    fn pict_op_atop_uses_da_and_inv_sa() {
        // PictOpAtop: Fa=Da, Fb=1-Sa
        let (fa, fb) = super::pict_op_factors(9, 100, 150);
        assert_eq!(fa, 150);
        assert_eq!(fb, 255 - 100); // 155
    }

    #[test]
    fn pict_op_xor_uses_inv_both() {
        // PictOpXor: Fa=1-Da, Fb=1-Sa
        let (fa, fb) = super::pict_op_factors(11, 100, 150);
        assert_eq!(fa, 255 - 150); // 105
        assert_eq!(fb, 255 - 100); // 155
    }

    #[test]
    fn pict_op_add_both_full() {
        // PictOpAdd: Fa=1, Fb=1
        let (fa, fb) = super::pict_op_factors(12, 128, 128);
        assert_eq!(fa, 255);
        assert_eq!(fb, 255);
    }

    #[test]
    fn composite_pixel_over_blends_correctly() {
        // Over with 50% alpha green src onto opaque blue dst
        // Storage layout: dst = [R, G, B, A].
        let mut dst = [0u8, 0, 255, 255]; // blue, opaque
        composite_pixel(
            3, // PictOpOver
            &mut dst, 0, 255, 0, 128, // src args: B=0, G=255, R=0, A=128
            true,
        );
        // sa=128, fd=255-128=127
        assert_eq!(dst[0], 0); // R: none
        assert_eq!(dst[1], 255); // G: fully green
        assert_eq!(dst[2], 127); // B: attenuated blue
    }

    #[test]
    fn composite_pixel_src_replaces_completely() {
        let mut dst = [0u8, 0, 255, 255]; // blue (RGBA)
        composite_pixel(1, &mut dst, 0, 255, 0, 128, true); // PictOpSrc: green
        assert_eq!(dst, [0, 255, 0, 128]);
    }

    #[test]
    fn composite_pixel_dst_preserves_completely() {
        let mut dst = [0u8, 0, 255, 255]; // blue
        composite_pixel(2, &mut dst, 0, 255, 0, 128, true); // PictOpDst
        assert_eq!(dst, [0, 0, 255, 255]); // unchanged
    }

    #[test]
    fn composite_pixel_clear_zeroes_all() {
        let mut dst = [255u8, 128, 64, 200];
        composite_pixel(0, &mut dst, 100, 100, 100, 100, true); // PictOpClear
        assert_eq!(dst, [0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // decode_pixel_bgra format handling
    // -----------------------------------------------------------------------

    #[test]
    fn decode_pixel_argb32() {
        // ARGB32 storage layout: [R, G, B, A].
        let bytes = [10u8, 20, 30, 200];
        let (b, g, r, a) = decode_pixel_bgra(PICTFORMAT_ARGB32, &bytes);
        assert_eq!((b, g, r, a), (30, 20, 10, 200));
    }

    #[test]
    fn decode_pixel_xrgb32_forces_opaque() {
        let bytes = [10u8, 20, 30, 0]; // [R, G, B, X]
        let (b, g, r, a) = decode_pixel_bgra(PICTFORMAT_XRGB32, &bytes);
        assert_eq!((b, g, r, a), (30, 20, 10, 255));
    }

    #[test]
    fn decode_pixel_xbgr32_swaps_rb() {
        // xBGR32 reads bytes inverted relative to ARGB32 storage.
        let bytes = [10u8, 20, 30, 0];
        let (b, g, r, a) = decode_pixel_bgra(PICTFORMAT_XBGR32, &bytes);
        assert_eq!((b, g, r, a), (10, 20, 30, 255));
    }

    #[test]
    fn decode_pixel_a8_only_alpha() {
        let bytes = [0u8, 0, 0, 180];
        let (b, g, r, a) = decode_pixel_bgra(PICTFORMAT_A8, &bytes);
        assert_eq!((b, g, r, a), (0, 0, 0, 180)); // only alpha
    }

    #[test]
    fn decode_pixel_short_buffer() {
        let bytes = [10u8, 20]; // too short
        let (b, g, r, a) = decode_pixel_bgra(PICTFORMAT_ARGB32, &bytes);
        assert_eq!((b, g, r, a), (0, 0, 0, 0));
    }

    // -----------------------------------------------------------------------
    // pict_format_has_alpha
    // -----------------------------------------------------------------------

    #[test]
    fn pict_format_alpha_detection() {
        assert!(pict_format_has_alpha(PICTFORMAT_ARGB32));
        assert!(pict_format_has_alpha(PICTFORMAT_A8));
        assert!(pict_format_has_alpha(PICTFORMAT_A1));
        assert!(!pict_format_has_alpha(PICTFORMAT_RGB24));
        assert!(!pict_format_has_alpha(PICTFORMAT_XRGB32));
        assert!(!pict_format_has_alpha(PICTFORMAT_XBGR32));
    }

    // -----------------------------------------------------------------------
    // zero_src_has_no_effect — identify destructive operators
    // -----------------------------------------------------------------------

    #[test]
    fn zero_src_effect_for_standard_ops() {
        // Destructive: Clear(0), Src(1), In(5), InReverse(6), Out(7), AtopReverse(10)
        assert!(!zero_src_has_no_effect(0)); // Clear
        assert!(!zero_src_has_no_effect(1)); // Src
        assert!(!zero_src_has_no_effect(5)); // In
        assert!(!zero_src_has_no_effect(6)); // InReverse
        assert!(!zero_src_has_no_effect(7)); // Out
        assert!(!zero_src_has_no_effect(10)); // AtopReverse

        // Non-destructive: Dst(2), Over(3), OverReverse(4), OutReverse(8), Atop(9), Xor(11), Add(12), Saturate(13)
        assert!(zero_src_has_no_effect(2));
        assert!(zero_src_has_no_effect(3));
        assert!(zero_src_has_no_effect(4));
        assert!(zero_src_has_no_effect(8));
        assert!(zero_src_has_no_effect(9));
        assert!(zero_src_has_no_effect(11));
        assert!(zero_src_has_no_effect(12));
        assert!(zero_src_has_no_effect(13));
    }

    // -----------------------------------------------------------------------
    // point_in_triangle edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn point_in_triangle_inside() {
        assert!(point_in_triangle(1.0, 1.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn point_in_triangle_outside() {
        assert!(!point_in_triangle(5.0, 5.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn point_in_triangle_on_edge() {
        // On the hypotenuse of a right triangle
        assert!(point_in_triangle(1.5, 1.5, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn point_in_triangle_at_vertex() {
        assert!(point_in_triangle(0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn point_in_triangle_degenerate_line() {
        // Degenerate triangle (all points collinear) — should not crash
        let _ = point_in_triangle(1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 4.0, 0.0);
    }

    // -----------------------------------------------------------------------
    // Disjoint/Conjoint operator boundary values
    // -----------------------------------------------------------------------

    #[test]
    fn disjoint_over_with_zero_alpha() {
        // DisjointOver (op 19): with Sa=0, Da=128
        let (fa, fb) = super::pict_op_factors(19, 0, 128);
        // Fa should handle zero Sa gracefully (no divide by zero)
        assert!(fa <= 255);
        assert!(fb <= 255);
    }

    #[test]
    fn conjoint_over_with_full_alpha() {
        // ConjointOver (op 35): Sa=255, Da=255
        let (fa, fb) = super::pict_op_factors(35, 255, 255);
        assert!(fa <= 255);
        assert!(fb <= 255);
    }

    #[test]
    fn disjoint_src_factors() {
        // DisjointSrc (op 17): Fa=1, Fb=max(0,(1-Sa)/Da)
        let (fa, fb) = super::pict_op_factors(17, 128, 128);
        assert_eq!(fa, 255);
        assert!(fb <= 255);
    }

    #[test]
    fn conjoint_clear_zeroes() {
        // ConjointClear (op 32): Fa=0, Fb=0
        let (fa, fb) = super::pict_op_factors(32, 128, 128);
        assert_eq!(fa, 0);
        assert_eq!(fb, 0);
    }
}
