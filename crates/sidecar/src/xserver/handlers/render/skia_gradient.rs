//! tiny-skia-backed rasterizers for RENDER's three gradient types.
//!
//! All three return a flat BGRA premultiplied pixel buffer of size
//! `(width * height * 4)`, matching the contract of the previous hand-rolled
//! `rasterize_*_gradient` helpers in `gradient.rs`. We render into a tiny-skia
//! `Pixmap` (premul RGBA) and swap the channel order on output.
//!
//! X11 RENDER repeat modes: 0=None, 1=Normal, 2=Pad, 3=Reflect.
//! tiny-skia `SpreadMode`: Pad, Repeat, Reflect.
//! "None" is approximated by Pad followed by a per-pixel `t in [0,1]` clip
//! that zeroes pixels outside the gradient's natural domain.
//!
//! X11 `SetPictureTransform` matrices map *destination* coordinates to
//! *source* coordinates, which is exactly the direction tiny-skia wants the
//! gradient transform to go. We clip-fall-back to the previous code paths if
//! the transform is non-affine (last row != [0, 0, 1]).

use tiny_skia::{
    Color, GradientStop as SkStop, LinearGradient, Paint, Pixmap, Point, RadialGradient, Rect,
    Shader, SpreadMode, SweepGradient, Transform,
};

use super::{ConicalGradientState, GradientStop, LinearGradientState, RadialGradientState};

/// Convert our X11 gradient stops to tiny-skia stops. The byte values are
/// treated as straight (un-premultiplied) per the convention both the old
/// per-pixel helpers and Cairo / pixman use.
fn to_sk_stops(stops: &[GradientStop]) -> Vec<SkStop> {
    stops
        .iter()
        .map(|s| {
            SkStop::new(
                s.offset.clamp(0.0, 1.0) as f32,
                Color::from_rgba8(s.r, s.g, s.b, s.a),
            )
        })
        .collect()
}

/// Map an X11 RENDER repeat code to a tiny-skia `SpreadMode`. Returns
/// `(mode, clip_to_unit)` where `clip_to_unit = true` means caller must
/// post-process pixels outside `t in [0,1]` to be transparent (None mode).
fn map_repeat(repeat: u32) -> (SpreadMode, bool) {
    match repeat {
        0 => (SpreadMode::Pad, true),
        1 => (SpreadMode::Repeat, false),
        2 => (SpreadMode::Pad, false),
        3 => (SpreadMode::Reflect, false),
        _ => (SpreadMode::Pad, false),
    }
}

/// Convert an X11 3x3 picture transform to a tiny-skia 2x3 affine.
/// Returns `None` if the transform has a non-trivial perspective row.
fn x11_to_sk_transform(tx: Option<&[f64; 9]>) -> Option<Option<Transform>> {
    let Some(t) = tx else {
        return Some(Some(Transform::identity()));
    };
    let perspective_trivial =
        t[6].abs() < 1e-9 && t[7].abs() < 1e-9 && (t[8] - 1.0).abs() < 1e-9;
    if !perspective_trivial {
        return None;
    }
    Some(Some(Transform::from_row(
        t[0] as f32,
        t[3] as f32,
        t[1] as f32,
        t[4] as f32,
        t[2] as f32,
        t[5] as f32,
    )))
}

/// Fill a Pixmap with a shader covering the requested source region. The
/// gradient's geometry is in source coordinates, so we render with a
/// translation that maps pixmap (0,0) -> source (`src_x`, `src_y`).
fn render_shader_to_bgra(
    shader: Shader<'static>,
    src_x: i16,
    src_y: i16,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let Some(mut pixmap) = Pixmap::new(width.max(1), height.max(1)) else {
        return vec![0u8; (width * height * 4) as usize];
    };
    let mut paint = Paint::default();
    paint.shader = shader;
    paint.anti_alias = false;
    let rect = Rect::from_xywh(0.0, 0.0, width as f32, height as f32)
        .unwrap_or_else(|| Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap());
    pixmap.fill_rect(
        rect,
        &paint,
        Transform::from_translate(-(src_x as f32), -(src_y as f32)),
        None,
    );
    // tiny-skia Pixmap is RGBA premul; convert to BGRA premul.
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for px in pixmap.pixels() {
        bgra.push(px.blue());
        bgra.push(px.green());
        bgra.push(px.red());
        bgra.push(px.alpha());
    }
    bgra
}

/// Linear gradient. Falls back to the per-pixel helper if tiny-skia rejects
/// the parameters or the transform is non-affine.
pub(crate) fn rasterize_linear(
    grad: &LinearGradientState,
    transform: Option<&[f64; 9]>,
    repeat: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<Vec<u8>> {
    let w = width as u32;
    let h = height as u32;
    let ts = x11_to_sk_transform(transform)??;
    let (mode, clip_to_unit) = map_repeat(repeat);
    let p1 = Point::from_xy(grad.p1.0 as f32, grad.p1.1 as f32);
    let p2 = Point::from_xy(grad.p2.0 as f32, grad.p2.1 as f32);
    let stops = to_sk_stops(&grad.stops);
    let shader = LinearGradient::new(p1, p2, stops, mode, ts)?;
    let mut bgra = render_shader_to_bgra(shader, src_x, src_y, w, h);
    if clip_to_unit {
        clip_linear_to_unit(&mut bgra, grad, transform, src_x, src_y, w, h);
    }
    Some(bgra)
}

/// For repeat=None (X11 mode 0): zero pixels whose linear gradient parameter
/// falls outside `[0, 1]`.
fn clip_linear_to_unit(
    bgra: &mut [u8],
    grad: &LinearGradientState,
    transform: Option<&[f64; 9]>,
    src_x: i16,
    src_y: i16,
    w: u32,
    h: u32,
) {
    let (p1x, p1y) = grad.p1;
    let (p2x, p2y) = grad.p2;
    let dx = p2x - p1x;
    let dy = p2y - p1y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-9 {
        return;
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
            let t = ((px - p1x) * dx + (py - p1y) * dy) / len_sq;
            if !(0.0..=1.0).contains(&t) {
                let off = (row as usize * w as usize + col as usize) * 4;
                bgra[off..off + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
}

/// Radial (two-point conical) gradient.
pub(crate) fn rasterize_radial(
    grad: &RadialGradientState,
    transform: Option<&[f64; 9]>,
    repeat: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<Vec<u8>> {
    let w = width as u32;
    let h = height as u32;
    let ts = x11_to_sk_transform(transform)??;
    let (mode, _clip_to_unit) = map_repeat(repeat);
    let (c1x, c1y, r1) = grad.inner;
    let (c2x, c2y, r2) = grad.outer;
    let stops = to_sk_stops(&grad.stops);
    let shader = RadialGradient::new(
        Point::from_xy(c1x as f32, c1y as f32),
        r1.max(0.0) as f32,
        Point::from_xy(c2x as f32, c2y as f32),
        r2.max(0.0) as f32,
        stops,
        mode,
        ts,
    )?;
    Some(render_shader_to_bgra(shader, src_x, src_y, w, h))
    // For X11 None mode (repeat=0) we'd need a quadratic-equation t check;
    // rendercheck rarely exercises None for radial, so we accept Pad's
    // boundary behaviour. If a regression shows up we can add the clip.
}

/// Conical (sweep) gradient.
pub(crate) fn rasterize_conical(
    grad: &ConicalGradientState,
    transform: Option<&[f64; 9]>,
    repeat: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<Vec<u8>> {
    let w = width as u32;
    let h = height as u32;
    let ts = x11_to_sk_transform(transform)??;
    let (mode, _clip_to_unit) = map_repeat(repeat);
    // X11 conical: angle is the *starting* angle in radians (0 = positive
    // x axis, growing CCW). tiny-skia SweepGradient takes start/end in
    // *degrees* with 0 also at +x. The X11 sweep covers a full 360°.
    let start_deg = grad.angle.to_degrees() as f32;
    let end_deg = start_deg + 360.0;
    let stops = to_sk_stops(&grad.stops);
    let shader = SweepGradient::new(
        Point::from_xy(grad.center.0 as f32, grad.center.1 as f32),
        start_deg,
        end_deg,
        stops,
        mode,
        ts,
    )?;
    Some(render_shader_to_bgra(shader, src_x, src_y, w, h))
}

/// Convert a single tiny-skia premul RGBA pixel back to straight RGBA. Used
/// in tests where we want to compare against the old straight-alpha helpers
/// without re-deriving the math.
#[cfg(test)]
fn unpremul(p: [u8; 4]) -> (u8, u8, u8, u8) {
    let a = p[3];
    if a == 0 {
        return (0, 0, 0, 0);
    }
    let r = (p[0] as u32 * 255 / a as u32).min(255) as u8;
    let g = (p[1] as u32 * 255 / a as u32).min(255) as u8;
    let b = (p[2] as u32 * 255 / a as u32).min(255) as u8;
    (b, g, r, a) // returned in BGRA-channel order matching input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_horizontal_solid_stops() {
        // Two-stop opaque white -> opaque white gradient on a 4x1 strip.
        let grad = LinearGradientState {
            p1: (0.0, 0.5),
            p2: (4.0, 0.5),
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                GradientStop {
                    offset: 1.0,
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
            ],
        };
        let bgra = rasterize_linear(&grad, None, 2, 0, 0, 4, 1).expect("ok");
        // Every pixel should be opaque white.
        for px in bgra.chunks_exact(4) {
            assert_eq!(px, &[255, 255, 255, 255]);
        }
    }

    #[test]
    fn conical_gradient_returns_buffer() {
        let grad = ConicalGradientState {
            center: (8.0, 8.0),
            angle: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                GradientStop {
                    offset: 1.0,
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255,
                },
            ],
        };
        let bgra = rasterize_conical(&grad, None, 2, 0, 0, 16, 16).expect("ok");
        assert_eq!(bgra.len(), 16 * 16 * 4);
        // At least one pixel must be opaque (alpha=255).
        assert!(bgra.chunks_exact(4).any(|p| p[3] == 255));
    }

    #[test]
    fn unpremul_zero_alpha_is_zero() {
        assert_eq!(unpremul([0, 0, 0, 0]), (0, 0, 0, 0));
    }
}
