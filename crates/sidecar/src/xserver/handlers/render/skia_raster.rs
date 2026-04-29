//! tiny-skia-based AA rasterization for RENDER geometric primitives.
//!
//! The X RENDER spec requires sub-pixel coverage anti-aliasing for trapezoids,
//! triangles, tristrips and trifans. This module produces an 8-bit coverage
//! mask via tiny-skia, then composites each pixel through the existing
//! `composite_pixel` Porter-Duff path with the source alpha modulated by mask
//! coverage.

use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

use super::composite_pixel;
use crate::framebuffer::Framebuffer;
use crate::xserver::handlers::render::ClipSnapshot;

/// Compute the integer bounding box that fully covers a list of (x, y) points.
fn bbox(points: &[(f64, f64)]) -> Option<(i32, i32, i32, i32)> {
    if points.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x, y) in points {
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
    }
    let x0 = min_x.floor() as i32;
    let y0 = min_y.floor() as i32;
    let x1 = max_x.ceil() as i32;
    let y1 = max_y.ceil() as i32;
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

/// Build a tiny-skia mask covering `points` (in framebuffer coordinates).
/// The mask is offset so its top-left corner aligns with the bounding box.
/// Returns `(mask, x_origin, y_origin)` on success.
fn rasterize_polygon_mask(
    points: &[(f64, f64)],
    fb_w: i32,
    fb_h: i32,
) -> Option<(Mask, i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = bbox(points)?;
    // Clamp to framebuffer bounds.
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(fb_w);
    y1 = y1.min(fb_h);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;
    let mut mask = Mask::new(w, h)?;

    let mut pb = PathBuilder::new();
    let first = points[0];
    pb.move_to(first.0 as f32 - x0 as f32, first.1 as f32 - y0 as f32);
    for &(px, py) in &points[1..] {
        pb.line_to(px as f32 - x0 as f32, py as f32 - y0 as f32);
    }
    pb.close();
    let path = pb.finish()?;

    // anti_alias=true gives sub-pixel coverage in the 0..=255 range.
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some((mask, x0, y0))
}

/// Composite a polygon into the framebuffer using the AA coverage mask
/// produced by tiny-skia. Pixels with zero coverage are skipped; pixels with
/// partial coverage have the source alpha scaled down before being passed to
/// `composite_pixel`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_polygon_aa(
    fb: &mut Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
    dst_has_alpha: bool,
    points: &[(f64, f64)],
    clip: &ClipSnapshot,
) {
    let Some((mask, x_off, y_off)) = rasterize_polygon_mask(points, fb_w, fb_h) else {
        return;
    };
    let mw = mask.width() as i32;
    let mh = mask.height() as i32;
    let mdata = mask.data();

    let fb_stride = fb.stride();
    let fb_data = fb.data_mut();

    for my in 0..mh {
        let fy = y_off + my;
        if fy < 0 || fy >= fb_h {
            continue;
        }
        let mrow = (my * mw) as usize;
        for mx in 0..mw {
            let coverage = mdata[mrow + mx as usize];
            if coverage == 0 {
                continue;
            }
            let fx = x_off + mx;
            if fx < 0 || fx >= fb_w {
                continue;
            }
            if !clip.allows(fx, fy) {
                continue;
            }
            let dst_off = fy as usize * fb_stride + fx as usize * 4;
            if dst_off + 3 >= fb_data.len() {
                continue;
            }
            // Modulate the premultiplied source by coverage. Premultiplied
            // alpha + coverage scaling: every channel including alpha is
            // multiplied by coverage/255.
            let scaled_b = ((sb as u32 * coverage as u32 + 127) / 255) as u8;
            let scaled_g = ((sg as u32 * coverage as u32 + 127) / 255) as u8;
            let scaled_r = ((sr as u32 * coverage as u32 + 127) / 255) as u8;
            let scaled_a = ((sa as u32 * coverage as u32 + 127) / 255) as u8;
            composite_pixel(
                op,
                &mut fb_data[dst_off..dst_off + 4],
                scaled_b,
                scaled_g,
                scaled_r,
                scaled_a,
                dst_has_alpha,
            );
        }
    }

    // Mark dirty region.
    if mw > 0 && mh > 0 {
        fb.mark_dirty(x_off, y_off, mw as u32, mh as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn black_clip() -> ClipSnapshot {
        ClipSnapshot::default()
    }

    #[test]
    fn polygon_renders_some_pixels() {
        let mut fb = Framebuffer::new(32, 32);
        let pts = vec![(4.0, 4.0), (28.0, 4.0), (28.0, 28.0), (4.0, 28.0)];
        // sr/sg/sb=255 (white) with op=1 (Src) and full coverage should
        // write 255 into the B channel of any covered pixel.
        composite_polygon_aa(
            &mut fb, 32, 32, 1, 255, 255, 255, 255, true, &pts, &black_clip(),
        );
        let stride = fb.stride();
        let off = 16 * stride + 16 * 4; // center pixel
        let data = fb.data();
        assert_eq!(data[off], 255, "center pixel should have B=255");
        assert_eq!(data[off + 1], 255, "center pixel should have G=255");
        assert_eq!(data[off + 2], 255, "center pixel should have R=255");
    }

    #[test]
    fn polygon_outside_bbox_no_panic() {
        let mut fb = Framebuffer::new(8, 8);
        // Polygon entirely outside the framebuffer.
        let pts = vec![(100.0, 100.0), (110.0, 100.0), (110.0, 110.0)];
        composite_polygon_aa(
            &mut fb, 8, 8, 1, 255, 255, 255, 255, true, &pts, &black_clip(),
        );
        // No assert beyond "didn't panic"
    }
}
