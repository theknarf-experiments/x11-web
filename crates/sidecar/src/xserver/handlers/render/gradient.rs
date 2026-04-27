use tracing::debug;

use super::super::parse_minor;
use super::{
    ConicalGradientState, GradientStop, LinearGradientState, PictFilter, PictureState,
    RadialGradientState, SolidFillState, PICTFORMAT_ARGB32,
};
use crate::xserver::core::require_len;
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::{
    CreateConicalGradientRequest, CreateLinearGradientRequest, CreateRadialGradientRequest,
    CreateSolidFillRequest, Fixed,
};

/// Convert a 16.16 fixed-point i32 (x11rb `Fixed`) to f64.
fn fixed_to_f64(f: Fixed) -> f64 {
    f as f64 / 65536.0
}

pub(crate) fn handle_create_solid_fill(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;

    let req = parse_minor!(CreateSolidFillRequest, data, state, seq, 139, data[1] as u16);

    let pid = req.picture;
    // XRenderColor is already premultiplied per the X RENDER spec --
    // just truncate 16-bit -> 8-bit. No extra alpha scaling.
    let r = (req.color.red >> 8) as u8;
    let g = (req.color.green >> 8) as u8;
    let b = (req.color.blue >> 8) as u8;
    let a = (req.color.alpha >> 8) as u8;

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
fn handle_create_linear_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;

    let req = parse_minor!(CreateLinearGradientRequest, data, state, seq, 139, minor);

    let pid = req.picture;
    let p1x = fixed_to_f64(req.p1.x);
    let p1y = fixed_to_f64(req.p1.y);
    let p2x = fixed_to_f64(req.p2.x);
    let p2y = fixed_to_f64(req.p2.y);
    let num_stops = req.stops.len();

    // Sanity bound
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

    let mut stops = Vec::with_capacity(num_stops);
    for (i, &stop_fixed) in req.stops.iter().enumerate() {
        let offset = fixed_to_f64(stop_fixed);
        let color = &req.colors[i];
        let r = (color.red >> 8) as u8;
        let g = (color.green >> 8) as u8;
        let b = (color.blue >> 8) as u8;
        let a = (color.alpha >> 8) as u8;
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
    // actually land somewhere we'll read back.
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

    // Premultiply the lerped straight RGBA.
    let scale = sa / 255.0;
    let pr = (sr * scale).round().clamp(0.0, 255.0) as u8;
    let pg = (sg * scale).round().clamp(0.0, 255.0) as u8;
    let pb = (sb * scale).round().clamp(0.0, 255.0) as u8;
    let pa = sa.round().clamp(0.0, 255.0) as u8;
    (pr, pg, pb, pa)
}

/// Rasterise a region of a linear gradient into a BGRA pixel buffer.
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
            let mut px = (src_x as i32 + col) as f64 + 0.5;
            let mut py = (src_y as i32 + row) as f64 + 0.5;
            if let Some(tx) = transform {
                let (tx_px, tx_py) = super::transform::apply_transform(tx, px, py);
                px = tx_px;
                py = tx_py;
            }
            let t_raw = ((px - p1x) * dx + (py - p1y) * dy) / len_sq;
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
fn handle_create_radial_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;

    let req = parse_minor!(CreateRadialGradientRequest, data, state, seq, 139, minor);

    let pid = req.picture;
    let inner_cx = fixed_to_f64(req.inner.x);
    let inner_cy = fixed_to_f64(req.inner.y);
    let outer_cx = fixed_to_f64(req.outer.x);
    let outer_cy = fixed_to_f64(req.outer.y);
    let inner_r = fixed_to_f64(req.inner_radius);
    let outer_r = fixed_to_f64(req.outer_radius);
    let num_stops = req.stops.len();

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

    let mut stops = Vec::with_capacity(num_stops);
    for (i, &stop_fixed) in req.stops.iter().enumerate() {
        let offset = fixed_to_f64(stop_fixed);
        let color = &req.colors[i];
        let r = (color.red >> 8) as u8;
        let g = (color.green >> 8) as u8;
        let b = (color.blue >> 8) as u8;
        let a = (color.alpha >> 8) as u8;
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

            let pdx = px - c1x;
            let pdy = py - c1y;

            let a = cdx * cdx + cdy * cdy - dr * dr;
            let b = 2.0 * (pdx * cdx + pdy * cdy + r1 * dr);
            let c = pdx * pdx + pdy * pdy - r1 * r1;

            let (r, g, b_val, alpha) = if a.abs() < 1e-9 {
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

                    let valid = |t: f64| r1 + t * dr >= 0.0;
                    let t_raw = if valid(t0) && valid(t1) {
                        t0.max(t1)
                    } else if valid(t0) {
                        t0
                    } else if valid(t1) {
                        t1
                    } else {
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
fn handle_create_conical_gradient(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;

    let req = parse_minor!(CreateConicalGradientRequest, data, state, seq, 139, minor);

    let pid = req.picture;
    let cx = fixed_to_f64(req.center.x);
    let cy = fixed_to_f64(req.center.y);
    let angle_fixed = fixed_to_f64(req.angle);
    // The spec says angle is in degrees (as FIXED). Convert to radians.
    let angle_rad = angle_fixed * std::f64::consts::PI / 180.0;
    let num_stops = req.stops.len();

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

    let mut stops = Vec::with_capacity(num_stops);
    for (i, &stop_fixed) in req.stops.iter().enumerate() {
        let offset = fixed_to_f64(stop_fixed);
        let color = &req.colors[i];
        let r = (color.red >> 8) as u8;
        let g = (color.green >> 8) as u8;
        let b = (color.blue >> 8) as u8;
        let a = (color.alpha >> 8) as u8;
        stops.push(GradientStop { offset, r, g, b, a });
    }

    debug!(
        "CreateConicalGradient: pid={pid:#x} center=({cx:.2},{cy:.2}) angle={angle_fixed:.2}\u{00b0} stops={num_stops}"
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
            let angle = dy.atan2(dx) - start_angle;
            let t_raw = angle.rem_euclid(2.0 * std::f64::consts::PI) / (2.0 * std::f64::consts::PI);

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

/// Apply a pixmap repeat mode to raw coordinates.
pub(crate) fn apply_pixmap_repeat(
    raw_x: i32,
    raw_y: i32,
    w: i32,
    h: i32,
    repeat: u32,
) -> (u32, u32) {
    match repeat {
        2 => {
            let sx = raw_x.clamp(0, w - 1);
            let sy = raw_y.clamp(0, h - 1);
            (sx as u32, sy as u32)
        }
        3 => {
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
            let t = t_raw.rem_euclid(1.0);
            sample_gradient_stops(stops, t)
        }
        3 => {
            let r2 = t_raw.rem_euclid(2.0);
            let t = if r2 > 1.0 { 2.0 - r2 } else { r2 };
            sample_gradient_stops(stops, t)
        }
        2 => {
            sample_gradient_stops(stops, t_raw.clamp(0.0, 1.0))
        }
        _ => {
            if !(0.0..=1.0).contains(&t_raw) {
                (0, 0, 0, 0)
            } else {
                sample_gradient_stops(stops, t_raw)
            }
        }
    }
}
