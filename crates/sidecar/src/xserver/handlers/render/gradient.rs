use tracing::debug;

use super::{
    read_fixed_bo, ConicalGradientState, GradientStop, LinearGradientState, PictFilter,
    PictureState, RadialGradientState, SolidFillState, PICTFORMAT_ARGB32,
};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_u16_bo, read_u32_bo};
use crate::xserver::ClientState;

pub(crate) fn handle_create_solid_fill(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 16, seq, 139, data[1] as u16, bo);
    let pid = read_u32_bo(data, 4, bo);
    // Color: 4 x CARD16 (red, green, blue, alpha) at offset 8.
    // XRenderColor is already premultiplied per the X RENDER spec —
    // just truncate 16-bit -> 8-bit. No extra alpha scaling.
    let r = (read_u16_bo(data, 8, bo) >> 8) as u8;
    let g = (read_u16_bo(data, 10, bo) >> 8) as u8;
    let b = (read_u16_bo(data, 12, bo) >> 8) as u8;
    let a = (read_u16_bo(data, 14, bo) >> 8) as u8;

    debug!("Render CreateSolidFill: pid={pid:#x} premul=({r},{g},{b},{a})");

    state
        .render
        .solid_fills
        .insert(pid, SolidFillState { r, g, b, a });
    Vec::new()
}

pub(crate) fn handle_create_gradient_fill(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 8, seq, 139, data[1] as u16, state.msb_first);
    let minor = data[1];
    match minor {
        34 => handle_create_linear_gradient(state, data, seq),
        35 => handle_create_radial_gradient(state, data, seq),
        36 => handle_create_conical_gradient(state, data, seq),
        _ => {
            // Unreachable from dispatch, but return proper error if called directly
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                0,
                minor as u32,
                139,
                minor as u16,
                state.msb_first,
            )
        }
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
fn handle_create_linear_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 32, seq, 139, minor, bo);
    let pid = read_u32_bo(data, 4, bo);
    let p1x = read_fixed_bo(data, 8, bo);
    let p1y = read_fixed_bo(data, 12, bo);
    let p2x = read_fixed_bo(data, 16, bo);
    let p2y = read_fixed_bo(data, 20, bo);
    let num_stops = read_u32_bo(data, 24, bo) as usize;

    // Sanity bound: a typical gradient has 2-8 stops; reject anything
    // absurd before we allocate.
    if num_stops > 1024 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::VALUE_ERROR,
            seq,
            num_stops as u32,
            139,
            minor,
            bo,
        );
    }

    let stops_start = 28;
    let colors_start = stops_start + num_stops * 4;
    if colors_start + num_stops * 8 > data.len() {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            139,
            minor,
            bo,
        );
    }

    let mut stops = Vec::with_capacity(num_stops);
    for i in 0..num_stops {
        let offset = read_fixed_bo(data, stops_start + i * 4, bo);
        let coff = colors_start + i * 8;
        // XRenderColor is already premultiplied per the spec —
        // just truncate 16-bit -> 8-bit.
        let r = (read_u16_bo(data, coff, bo) >> 8) as u8;
        let g = (read_u16_bo(data, coff + 2, bo) >> 8) as u8;
        let b = (read_u16_bo(data, coff + 4, bo) >> 8) as u8;
        let a = (read_u16_bo(data, coff + 6, bo) >> 8) as u8;
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
            clip_mask: None,
            filter: PictFilter::Nearest,
        },
    );
    Vec::new()
}

/// Sample a sorted stop list at parameter `t`. Lerps in *straight*
/// alpha (matching rendercheck / Cairo) and returns the result in
/// premultiplied form so callers can drop it directly into a picture
/// framebuffer.
pub(crate) fn sample_gradient_stops(stops: &[GradientStop], t: f64) -> (u8, u8, u8, u8) {
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
                let f = if span > 1e-9 {
                    (t - s0.offset) / span
                } else {
                    0.0
                };
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
pub(crate) fn rasterize_linear_gradient(
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
                let (tx_px, tx_py) = super::transform::apply_transform(tx, px, py);
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

/// CreateRadialGradient (RENDER minor opcode 35).
///
/// Wire layout:
///
/// ```text
///   1   opcode (139)
///   1   minor  (35)
///   2   length
///   4   pid
///   8   inner_center  POINTFIX (FIXED x, FIXED y)
///   8   outer_center  POINTFIX
///   4   inner_radius  FIXED
///   4   outer_radius  FIXED
///   4   num_stops
///   4*n stops         (FIXED offsets)
///   8*n colors        (4 CARD16: r, g, b, a)
/// ```
fn handle_create_radial_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 40, seq, 139, minor, bo);
    let pid = read_u32_bo(data, 4, bo);
    let inner_cx = read_fixed_bo(data, 8, bo);
    let inner_cy = read_fixed_bo(data, 12, bo);
    let outer_cx = read_fixed_bo(data, 16, bo);
    let outer_cy = read_fixed_bo(data, 20, bo);
    let inner_r = read_fixed_bo(data, 24, bo);
    let outer_r = read_fixed_bo(data, 28, bo);
    let num_stops = read_u32_bo(data, 32, bo) as usize;

    if num_stops > 1024 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::VALUE_ERROR,
            seq,
            num_stops as u32,
            139,
            minor,
            bo,
        );
    }

    let stops_start = 36;
    let colors_start = stops_start + num_stops * 4;
    if colors_start + num_stops * 8 > data.len() {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            139,
            minor,
            bo,
        );
    }

    let mut stops = Vec::with_capacity(num_stops);
    for i in 0..num_stops {
        let offset = read_fixed_bo(data, stops_start + i * 4, bo);
        let coff = colors_start + i * 8;
        let r = (read_u16_bo(data, coff, bo) >> 8) as u8;
        let g = (read_u16_bo(data, coff + 2, bo) >> 8) as u8;
        let b = (read_u16_bo(data, coff + 4, bo) >> 8) as u8;
        let a = (read_u16_bo(data, coff + 6, bo) >> 8) as u8;
        stops.push(GradientStop { offset, r, g, b, a });
    }

    debug!(
        "CreateRadialGradient: pid={pid:#x} inner=({inner_cx:.2},{inner_cy:.2},r={inner_r:.2}) outer=({outer_cx:.2},{outer_cy:.2},r={outer_r:.2}) stops={num_stops}"
    );

    state.render.radial_gradients.insert(
        pid,
        RadialGradientState {
            inner: (inner_cx, inner_cy, inner_r),
            outer: (outer_cx, outer_cy, outer_r),
            stops,
        },
    );
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
            clip_mask: None,
            filter: PictFilter::Nearest,
        },
    );
    Vec::new()
}

/// Rasterise a radial gradient into a BGRA pixel buffer.
///
/// Follows the pixman/Cairo radial gradient model: the gradient is
/// defined by two circles (inner and outer). For each pixel, we solve
/// the quadratic equation to find the parameter `t` where the
/// interpolated circle passes through that point. We pick the
/// largest valid root so the gradient flows from inner (t=0) to
/// outer (t=1).
pub(crate) fn rasterize_radial_gradient(
    grad: &RadialGradientState,
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

    let (c1x, c1y, r1) = grad.inner;
    let (c2x, c2y, r2) = grad.outer;

    // Differences for parametric interpolation: C(t) = C1 + t*(C2-C1),
    // R(t) = R1 + t*(R2-R1). We need to find t such that |P - C(t)|^2 = R(t)^2.
    let cdx = c2x - c1x;
    let cdy = c2y - c1y;
    let dr = r2 - r1;

    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let mut px = (src_x as i32 + col) as f64 + 0.5;
            let mut py = (src_y as i32 + row) as f64 + 0.5;
            if let Some(tx) = transform {
                let (tx_px, tx_py) = super::transform::apply_transform(tx, px, py);
                px = tx_px;
                py = tx_py;
            }

            // Vector from inner center to pixel
            let pdx = px - c1x;
            let pdy = py - c1y;

            // Quadratic: A*t^2 + B*t + C = 0
            // where the circle center moves C1->C2 and radius r1->r2
            let a = cdx * cdx + cdy * cdy - dr * dr;
            let b = 2.0 * (pdx * cdx + pdy * cdy + r1 * dr);
            let c = pdx * pdx + pdy * pdy - r1 * r1;

            let (r, g, b_val, alpha) = if a.abs() < 1e-9 {
                // Linear case (degenerate)
                if b.abs() < 1e-9 {
                    (0u8, 0u8, 0u8, 0u8)
                } else {
                    let t_raw = -c / b;
                    apply_gradient_repeat(&grad.stops, t_raw, repeat)
                }
            } else {
                let discr = b * b - 4.0 * a * c;
                if discr < 0.0 {
                    (0u8, 0u8, 0u8, 0u8)
                } else {
                    let sqrt_d = discr.sqrt();
                    let t0 = (-b + sqrt_d) / (2.0 * a);
                    let t1 = (-b - sqrt_d) / (2.0 * a);

                    // Pick the largest t where radius R(t) >= 0
                    let valid = |t: f64| r1 + t * dr >= 0.0;
                    let t_raw = if valid(t0) && valid(t1) {
                        t0.max(t1)
                    } else if valid(t0) {
                        t0
                    } else if valid(t1) {
                        t1
                    } else {
                        // No valid solution — transparent
                        let off = (row as usize * w as usize + col as usize) * 4;
                        pixels[off] = 0;
                        pixels[off + 1] = 0;
                        pixels[off + 2] = 0;
                        pixels[off + 3] = 0;
                        continue;
                    };
                    apply_gradient_repeat(&grad.stops, t_raw, repeat)
                }
            };

            let off = (row as usize * w as usize + col as usize) * 4;
            pixels[off] = b_val;
            pixels[off + 1] = g;
            pixels[off + 2] = r;
            pixels[off + 3] = alpha;
        }
    }

    (pixels, w, h)
}

/// CreateConicalGradient (RENDER minor opcode 36).
///
/// Wire layout:
///
/// ```text
///   1   opcode (139)
///   1   minor  (36)
///   2   length
///   4   pid
///   8   center   POINTFIX (FIXED x, FIXED y)
///   4   angle    FIXED (degrees, but stored as fixed-point)
///   4   num_stops
///   4*n stops    (FIXED offsets)
///   8*n colors   (4 CARD16: r, g, b, a)
/// ```
fn handle_create_conical_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);
    let pid = read_u32_bo(data, 4, bo);
    let cx = read_fixed_bo(data, 8, bo);
    let cy = read_fixed_bo(data, 12, bo);
    let angle_fixed = read_fixed_bo(data, 16, bo);
    // The spec says angle is in degrees (as FIXED). Convert to radians.
    let angle_rad = angle_fixed * std::f64::consts::PI / 180.0;
    let num_stops = read_u32_bo(data, 20, bo) as usize;

    if num_stops > 1024 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::VALUE_ERROR,
            seq,
            num_stops as u32,
            139,
            minor,
            bo,
        );
    }

    let stops_start = 24;
    let colors_start = stops_start + num_stops * 4;
    if colors_start + num_stops * 8 > data.len() {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            139,
            minor,
            bo,
        );
    }

    let mut stops = Vec::with_capacity(num_stops);
    for i in 0..num_stops {
        let offset = read_fixed_bo(data, stops_start + i * 4, bo);
        let coff = colors_start + i * 8;
        let r = (read_u16_bo(data, coff, bo) >> 8) as u8;
        let g = (read_u16_bo(data, coff + 2, bo) >> 8) as u8;
        let b = (read_u16_bo(data, coff + 4, bo) >> 8) as u8;
        let a = (read_u16_bo(data, coff + 6, bo) >> 8) as u8;
        stops.push(GradientStop { offset, r, g, b, a });
    }

    debug!(
        "CreateConicalGradient: pid={pid:#x} center=({cx:.2},{cy:.2}) angle={angle_fixed:.2}° stops={num_stops}"
    );

    state.render.conical_gradients.insert(
        pid,
        ConicalGradientState {
            center: (cx, cy),
            angle: angle_rad,
            stops,
        },
    );
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
            clip_mask: None,
            filter: PictFilter::Nearest,
        },
    );
    Vec::new()
}

/// Rasterise a conical (angular) gradient into a BGRA pixel buffer.
///
/// For each pixel, compute the angle from the center and use that
/// as the gradient parameter `t`, offset by the gradient's start angle.
pub(crate) fn rasterize_conical_gradient(
    grad: &ConicalGradientState,
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

    let (cx, cy) = grad.center;
    let start_angle = grad.angle;

    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let mut px = (src_x as i32 + col) as f64 + 0.5;
            let mut py = (src_y as i32 + row) as f64 + 0.5;
            if let Some(tx) = transform {
                let (tx_px, tx_py) = super::transform::apply_transform(tx, px, py);
                px = tx_px;
                py = tx_py;
            }

            let dx = px - cx;
            let dy = py - cy;
            // atan2 gives angle in [-PI, PI]; normalize to [0, 1]
            let angle = dy.atan2(dx) - start_angle;
            let t_raw = angle.rem_euclid(2.0 * std::f64::consts::PI) / (2.0 * std::f64::consts::PI);

            // Conical gradients always repeat (they're inherently cyclic)
            let (r, g, b_val, a) = apply_gradient_repeat(&grad.stops, t_raw, repeat);

            let off = (row as usize * w as usize + col as usize) * 4;
            pixels[off] = b_val;
            pixels[off + 1] = g;
            pixels[off + 2] = r;
            pixels[off + 3] = a;
        }
    }

    (pixels, w, h)
}

/// Apply a pixmap repeat mode to raw coordinates, returning clamped/wrapped
/// coordinates suitable for indexing into a (w x h) pixmap.
/// repeat: 1=Normal (tile), 2=Pad (clamp), 3=Reflect.
pub(crate) fn apply_pixmap_repeat(
    raw_x: i32,
    raw_y: i32,
    w: i32,
    h: i32,
    repeat: u32,
) -> (u32, u32) {
    match repeat {
        2 => {
            // Pad: clamp to edges
            let sx = raw_x.clamp(0, w - 1);
            let sy = raw_y.clamp(0, h - 1);
            (sx as u32, sy as u32)
        }
        3 => {
            // Reflect: mirror at boundaries
            let reflect = |v: i32, size: i32| -> u32 {
                if size <= 0 {
                    return 0;
                }
                let v2 = ((v % (2 * size)) + 2 * size) % (2 * size);
                if v2 < size {
                    v2 as u32
                } else {
                    (2 * size - 1 - v2) as u32
                }
            };
            (reflect(raw_x, w), reflect(raw_y, h))
        }
        _ => {
            // Normal (1) or any other: tile/wrap
            let sx = ((raw_x % w) + w) % w;
            let sy = ((raw_y % h) + h) % h;
            (sx as u32, sy as u32)
        }
    }
}

/// Apply repeat mode to a gradient parameter `t` and sample the stops.
fn apply_gradient_repeat(stops: &[GradientStop], t_raw: f64, repeat: u32) -> (u8, u8, u8, u8) {
    match repeat {
        1 => {
            // Normal (repeat/wrap)
            let t = t_raw.rem_euclid(1.0);
            sample_gradient_stops(stops, t)
        }
        3 => {
            // Reflect
            let r2 = t_raw.rem_euclid(2.0);
            let t = if r2 > 1.0 { 2.0 - r2 } else { r2 };
            sample_gradient_stops(stops, t)
        }
        2 => {
            // Pad (clamp)
            sample_gradient_stops(stops, t_raw.clamp(0.0, 1.0))
        }
        _ => {
            // None — outside [0,1] is transparent
            if !(0.0..=1.0).contains(&t_raw) {
                (0, 0, 0, 0)
            } else {
                sample_gradient_stops(stops, t_raw)
            }
        }
    }
}
