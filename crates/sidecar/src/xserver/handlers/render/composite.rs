use tracing::{debug, info};

use crate::xserver::ClientState;
use crate::xserver::core::{read_u16_bo, read_u32_bo, read_i16_bo};
use crate::xserver::core::require_len;
use super::{
    pict_format_has_alpha, zero_src_has_no_effect, point_in_triangle,
    composite_pixel, composite_pixel_ca, read_fixed_bo,
    ClipSnapshot, resolve_source_pixels, resolve_source_color,
};

/// The main compositing operation.
pub(crate) fn handle_composite(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 36, seq, 139, data[1] as u16, bo);

    let op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let mask_pic = read_u32_bo(data, 12, bo);
    let dst_pic = read_u32_bo(data, 16, bo);
    let src_x = read_i16_bo(data, 20, bo);
    let src_y = read_i16_bo(data, 22, bo);
    let mask_x = read_i16_bo(data, 24, bo);
    let mask_y = read_i16_bo(data, 26, bo);
    let dst_x = read_i16_bo(data, 28, bo);
    let dst_y = read_i16_bo(data, 30, bo);
    let width = read_u16_bo(data, 32, bo);
    let height = read_u16_bo(data, 34, bo);

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
pub(crate) fn handle_trapezoids(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let dst_pic = read_u32_bo(data, 12, bo);
    let _mask_format = read_u32_bo(data, 16, bo);
    let _src_x = read_i16_bo(data, 20, bo);
    let _src_y = read_i16_bo(data, 22, bo);

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
        None => return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_VALUE, seq, dst_pic, 139, minor, bo,
        ),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse trapezoids (40 bytes each starting at offset 24)
    let mut off = 24;
    let mut traps = Vec::new();
    while off + 40 <= data.len() {
        let top = read_fixed_bo(data, off, bo);
        let bottom = read_fixed_bo(data, off + 4, bo);
        let left_x1 = read_fixed_bo(data, off + 8, bo);
        let left_y1 = read_fixed_bo(data, off + 12, bo);
        let left_x2 = read_fixed_bo(data, off + 16, bo);
        let left_y2 = read_fixed_bo(data, off + 20, bo);
        let right_x1 = read_fixed_bo(data, off + 24, bo);
        let right_y1 = read_fixed_bo(data, off + 28, bo);
        let right_x2 = read_fixed_bo(data, off + 32, bo);
        let right_y2 = read_fixed_bo(data, off + 36, bo);
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
pub(crate) fn handle_triangles(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let dst_pic = read_u32_bo(data, 12, bo);
    let _mask_format = read_u32_bo(data, 16, bo);
    let _src_x = read_i16_bo(data, 20, bo);
    let _src_y = read_i16_bo(data, 22, bo);

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
        None => return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_VALUE, seq, dst_pic, 139, minor, bo,
        ),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Each triangle = 3 POINTFIX (each 8 bytes = x FIXED + y FIXED) = 24 bytes
    let mut off = 24;
    let mut triangles = Vec::new();
    while off + 24 <= data.len() {
        let x1 = read_fixed_bo(data, off, bo);
        let y1 = read_fixed_bo(data, off + 4, bo);
        let x2 = read_fixed_bo(data, off + 8, bo);
        let y2 = read_fixed_bo(data, off + 12, bo);
        let x3 = read_fixed_bo(data, off + 16, bo);
        let y3 = read_fixed_bo(data, off + 20, bo);
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
pub(crate) fn handle_tri_strip(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let dst_pic = read_u32_bo(data, 12, bo);
    let _mask_format = read_u32_bo(data, 16, bo);
    let _src_x = read_i16_bo(data, 20, bo);
    let _src_y = read_i16_bo(data, 22, bo);

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
        None => return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_VALUE, seq, dst_pic, 139, minor, bo,
        ),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Points: 8 bytes each (FIXED x + FIXED y)
    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed_bo(data, off, bo);
        let y = read_fixed_bo(data, off + 4, bo);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
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
pub(crate) fn handle_tri_fan(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let dst_pic = read_u32_bo(data, 12, bo);
    let _mask_format = read_u32_bo(data, 16, bo);
    let _src_x = read_i16_bo(data, 20, bo);
    let _src_y = read_i16_bo(data, 22, bo);

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
        None => return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_VALUE, seq, dst_pic, 139, minor, bo,
        ),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed_bo(data, off, bo);
        let y = read_fixed_bo(data, off + 4, bo);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
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

pub(crate) fn handle_fill_rectangles(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let op = data[4];
    let dst_pic = read_u32_bo(data, 8, bo);
    // Color is in the request at offset 12..20 (CARD16 each: red,
    // green, blue, alpha). XRenderColor is *already* premultiplied
    // per the X RENDER spec, so we just truncate 16-bit -> 8-bit;
    // no extra alpha multiply.
    let red = read_u16_bo(data, 12, bo);
    let green = read_u16_bo(data, 14, bo);
    let blue = read_u16_bo(data, 16, bo);
    let alpha = read_u16_bo(data, 18, bo);

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
        None => return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_VALUE, seq, dst_pic, 139, minor, bo,
        ),
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse rectangles (8 bytes each: x(2) y(2) w(2) h(2))
    let mut off = 20;
    let mut rects = Vec::new();
    while off + 8 <= data.len() {
        let x = read_i16_bo(data, off, bo);
        let y = read_i16_bo(data, off + 2, bo);
        let w = read_u16_bo(data, off + 4, bo);
        let h = read_u16_bo(data, off + 6, bo);
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

// ---------------------------------------------------------------------------
// AddTraps (RENDER minor opcode 32)
// ---------------------------------------------------------------------------

/// AddTraps adds trapezoids to an existing picture's geometry.
/// Each trap is: top(Fixed), bottom(Fixed), left_x1(Fixed), left_y1(Fixed),
/// left_x2(Fixed), left_y2(Fixed), right_x1(Fixed), right_y1(Fixed),
/// right_x2(Fixed), right_y2(Fixed) = 40 bytes.
pub(crate) fn handle_add_traps(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 12, seq, 139, minor, bo);
    let pic_id = read_u32_bo(data, 4, bo);
    let x_off = read_i16_bo(data, 8, bo);
    let y_off = read_i16_bo(data, 10, bo);

    let trap_data = &data[12..];
    let num_traps = trap_data.len() / 40;

    debug!("Render AddTraps: pic={pic_id:#x} offset=({x_off},{y_off}) traps={num_traps}");

    let (target, fb_w) = {
        let pic = match state.render.pictures.get(&pic_id) {
            Some(p) => p,
            None => return crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_VALUE, seq, pic_id, 139, minor, bo,
            ),
        };
        let d = pic.drawable;
        let w = if let Some(px) = state.pixmaps.get(&d) {
            px.framebuffer.width()
        } else if let Some(win) = state.windows.get(&d) {
            win.framebuffer.width()
        } else {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_VALUE, seq, d, 139, minor, bo,
            );
        };
        (d, w)
    };

    for i in 0..num_traps {
        let base = i * 40;
        if base + 40 > trap_data.len() {
            break;
        }

        let top = read_fixed_bo(trap_data, base, bo);
        let bottom = read_fixed_bo(trap_data, base + 4, bo);
        let l_x1 = read_fixed_bo(trap_data, base + 8, bo) + x_off as f64;
        let l_y1 = read_fixed_bo(trap_data, base + 12, bo) + y_off as f64;
        let l_x2 = read_fixed_bo(trap_data, base + 16, bo) + x_off as f64;
        let l_y2 = read_fixed_bo(trap_data, base + 20, bo) + y_off as f64;
        let r_x1 = read_fixed_bo(trap_data, base + 24, bo) + x_off as f64;
        let r_y1 = read_fixed_bo(trap_data, base + 28, bo) + y_off as f64;
        let r_x2 = read_fixed_bo(trap_data, base + 32, bo) + x_off as f64;
        let r_y2 = read_fixed_bo(trap_data, base + 36, bo) + y_off as f64;

        let y_start = (top + y_off as f64).floor().max(0.0) as u32;
        let y_end = (bottom + y_off as f64).ceil().max(0.0) as u32;

        if let Some(fb) = state.get_framebuffer_mut(target) {
            for y in y_start..y_end {
                let yf = y as f64 + 0.5;
                // Interpolate left edge
                let left_x = if (l_y2 - l_y1).abs() > 0.001 {
                    l_x1 + (l_x2 - l_x1) * (yf - l_y1) / (l_y2 - l_y1)
                } else {
                    l_x1
                };
                // Interpolate right edge
                let right_x = if (r_y2 - r_y1).abs() > 0.001 {
                    r_x1 + (r_x2 - r_x1) * (yf - r_y1) / (r_y2 - r_y1)
                } else {
                    r_x1
                };

                let x_start = (left_x.floor() as i32).max(0) as u32;
                let x_end = (right_x.ceil() as i32).min(fb_w as i32) as u32;

                for x in x_start..x_end {
                    // Set pixel to opaque white (alpha=0xFF) for mask pictures
                    let off = (y * fb.stride() as u32 + x * 4) as usize;
                    let fb_data = fb.data_mut();
                    if off + 4 <= fb_data.len() {
                        fb_data[off] = 0xFF;
                        fb_data[off + 1] = 0xFF;
                        fb_data[off + 2] = 0xFF;
                        fb_data[off + 3] = 0xFF;
                    }
                }
            }
            fb.mark_dirty(0, y_start as i32, fb_w, y_end.saturating_sub(y_start));
        }
    }

    Vec::new()
}
