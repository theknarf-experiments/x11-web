use tracing::{debug, info};

use super::{
    composite_pixel, composite_pixel_ca, pict_format_has_alpha, point_in_triangle,
    resolve_source_color, resolve_source_pixels, zero_src_has_no_effect, ClipSnapshot,
};
use crate::xserver::core::require_len;
use crate::xserver::request::request_header;
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::{
    AddTrapsRequest, CompositeRequest, FillRectanglesRequest, Fixed, TrapezoidsRequest,
    TriFanRequest, TriStripRequest, TrianglesRequest,
};

/// The main compositing operation.
pub(crate) fn handle_composite(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 36, seq, 139, data[1] as u16, bo);

    let req = parse_minor!(CompositeRequest, data, state, seq, 139, data[1] as u16);

    let op = u8::from(req.op);
    let src_pic = req.src;
    let mask_pic = req.mask;
    let dst_pic = req.dst;
    let src_x = req.src_x;
    let src_y = req.src_y;
    let mask_x = req.mask_x;
    let mask_y = req.mask_y;
    let dst_x = req.dst_x;
    let dst_y = req.dst_y;
    let width = req.width;
    let height = req.height;

    info!(
        "Render Composite: op={op} src={src_pic:#x} mask={mask_pic:#x} dst={dst_pic:#x} src=({src_x},{src_y}) dst=({dst_x},{dst_y}) {width}x{height}"
    );

    // Resolve source pixels
    let src_pixels: Option<(Vec<u8>, u32, u32)> =
        resolve_source_pixels(state, src_pic, src_x, src_y, width, height);
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
                                if mb == 0 && mg == 0 && mr == 0 && ma == 0 && skip_zero_mask_ok {
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
                            sb,
                            sg,
                            sr,
                            sa,
                            sa_b,
                            sa_g,
                            sa_r,
                            sa_a,
                            dst_has_alpha,
                        );
                    } else {
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

/// Convert a 16.16 fixed-point i32 (x11rb `Fixed`) to f64.
fn fixed_to_f64(f: Fixed) -> f64 {
    f as f64 / 65536.0
}

/// Handle XRender Trapezoids (minor opcode 10).
pub(crate) fn handle_trapezoids(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let req = parse_minor!(TrapezoidsRequest, data, state, seq, 139, minor);

    let op = u8::from(req.op);
    let src_pic = req.src;
    let dst_pic = req.dst;

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
        None => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                dst_pic,
                139,
                minor,
                bo,
            )
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Convert x11rb Trapezoid structs to the (f64, ...) tuples used downstream
    let traps: Vec<_> = req
        .traps
        .iter()
        .map(|t| {
            let top = fixed_to_f64(t.top);
            let bottom = fixed_to_f64(t.bottom);
            let left_x1 = fixed_to_f64(t.left.p1.x);
            let left_y1 = fixed_to_f64(t.left.p1.y);
            let left_x2 = fixed_to_f64(t.left.p2.x);
            let left_y2 = fixed_to_f64(t.left.p2.y);
            let right_x1 = fixed_to_f64(t.right.p1.x);
            let right_y1 = fixed_to_f64(t.right.p1.y);
            let right_x2 = fixed_to_f64(t.right.p2.x);
            let right_y2 = fixed_to_f64(t.right.p2.y);
            (
                top, bottom, left_x1, left_y1, left_x2, left_y2, right_x1, right_y1, right_x2,
                right_y2,
            )
        })
        .collect();

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
                    fb,
                    fb_w,
                    fb_h,
                    op,
                    sr,
                    sg,
                    sb,
                    sa,
                    dst_has_alpha,
                    top,
                    bottom,
                    lx1,
                    ly1,
                    lx2,
                    ly2,
                    rx1,
                    ry1,
                    rx2,
                    ry2,
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

    let req = parse_minor!(TrianglesRequest, data, state, seq, 139, minor);

    let op = u8::from(req.op);
    let src_pic = req.src;
    let dst_pic = req.dst;

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
        None => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                dst_pic,
                139,
                minor,
                bo,
            )
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Convert x11rb Triangle structs to f64 tuples
    let triangles: Vec<_> = req
        .triangles
        .iter()
        .map(|t| {
            (
                fixed_to_f64(t.p1.x),
                fixed_to_f64(t.p1.y),
                fixed_to_f64(t.p2.x),
                fixed_to_f64(t.p2.y),
                fixed_to_f64(t.p3.x),
                fixed_to_f64(t.p3.y),
            )
        })
        .collect();

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        if zero_src_has_no_effect(op) {
            // Standard fast path: only the trapezoid bounding box is
            // touched, scanline-decomposed into trapezoids.
            for &(x1, y1, x2, y2, x3, y3) in &triangles {
                rasterize_triangle(
                    fb,
                    fb_w,
                    fb_h,
                    op,
                    sr,
                    sg,
                    sb,
                    sa,
                    dst_has_alpha,
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    &clip,
                );
            }
        } else {
            // Pixman semantics: ops where a zero source still
            // mutates the destination (Clear, Src, In, InRev, Out,
            // AtopRev) composite over the *entire destination*. We
            // iterate the dst bbox, treating outside-triangle pixels
            // as having a fully transparent source.
            composite_triangles_full_dst(
                fb,
                fb_w,
                fb_h,
                op,
                sr,
                sg,
                sb,
                sa,
                dst_has_alpha,
                &triangles,
                &clip,
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
            let (eb, eg, er, ea) = if inside {
                (sb, sg, sr, sa)
            } else {
                (0, 0, 0, 0)
            };
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
            fb,
            fb_w,
            fb_h,
            op,
            sr,
            sg,
            sb,
            sa,
            dst_has_alpha,
            vy0,
            vy1,
            llx.0,
            llx.1,
            llx.2,
            llx.3,
            rrx.0,
            rrx.1,
            rrx.2,
            rrx.3,
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
            fb,
            fb_w,
            fb_h,
            op,
            sr,
            sg,
            sb,
            sa,
            dst_has_alpha,
            vy1,
            vy2,
            llx.0,
            llx.1,
            llx.2,
            llx.3,
            rrx.0,
            rrx.1,
            rrx.2,
            rrx.3,
            clip,
        );
    }
}

/// Handle XRender TriStrip (minor opcode 12).
pub(crate) fn handle_tri_strip(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let req = parse_minor!(TriStripRequest, data, state, seq, 139, minor);

    let op = u8::from(req.op);
    let src_pic = req.src;
    let dst_pic = req.dst;

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
        None => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                dst_pic,
                139,
                minor,
                bo,
            )
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Convert Pointfix to f64 pairs
    let points: Vec<_> = req
        .points
        .iter()
        .map(|p| (fixed_to_f64(p.x), fixed_to_f64(p.y)))
        .collect();

    if points.len() < 3 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            139,
            minor,
            bo,
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
                    fb,
                    fb_w,
                    fb_h,
                    op,
                    sr,
                    sg,
                    sb,
                    sa,
                    dst_has_alpha,
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    &clip,
                );
            }
        } else {
            composite_triangles_full_dst(
                fb,
                fb_w,
                fb_h,
                op,
                sr,
                sg,
                sb,
                sa,
                dst_has_alpha,
                &triangles,
                &clip,
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

    let req = parse_minor!(TriFanRequest, data, state, seq, 139, minor);

    let op = u8::from(req.op);
    let src_pic = req.src;
    let dst_pic = req.dst;

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
        None => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                dst_pic,
                139,
                minor,
                bo,
            )
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Convert Pointfix to f64 pairs
    let points: Vec<_> = req
        .points
        .iter()
        .map(|p| (fixed_to_f64(p.x), fixed_to_f64(p.y)))
        .collect();

    if points.len() < 3 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::LENGTH_ERROR,
            seq,
            0,
            139,
            minor,
            bo,
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
                    fb,
                    fb_w,
                    fb_h,
                    op,
                    sr,
                    sg,
                    sb,
                    sa,
                    dst_has_alpha,
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    &clip,
                );
            }
        } else {
            composite_triangles_full_dst(
                fb,
                fb_w,
                fb_h,
                op,
                sr,
                sg,
                sb,
                sa,
                dst_has_alpha,
                &triangles,
                &clip,
            );
        }
    }

    Vec::new()
}

pub(crate) fn handle_fill_rectangles(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 24, seq, 139, minor, bo);

    let req = parse_minor!(FillRectanglesRequest, data, state, seq, 139, minor);

    let op = u8::from(req.op);
    let dst_pic = req.dst;
    // Color: XRenderColor is *already* premultiplied per the X RENDER spec,
    // so we just truncate 16-bit -> 8-bit; no extra alpha multiply.
    let r = (req.color.red >> 8) as u8;
    let g = (req.color.green >> 8) as u8;
    let b = (req.color.blue >> 8) as u8;
    let a = (req.color.alpha >> 8) as u8;

    let (dst_drawable, dst_has_alpha) = state
        .render
        .pictures
        .get(&dst_pic)
        .map(|p| (Some(p.drawable), pict_format_has_alpha(p.format_id)))
        .unwrap_or((None, true));
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                dst_pic,
                139,
                minor,
                bo,
            )
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Use the rectangles from the parsed request
    let rects: Vec<_> = req
        .rects
        .iter()
        .map(|rect| (rect.x, rect.y, rect.width, rect.height))
        .collect();

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;
        let fb_stride = fb.stride();

        for &(rx, ry, rw, rh) in &rects {
            let fb_data = fb.data_mut();
            for row in 0..rh as i32 {
                let dy = ry as i32 + row;
                if dy < 0 || dy >= fb_h {
                    continue;
                }
                for col in 0..rw as i32 {
                    let dx = rx as i32 + col;
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
            fb.mark_dirty(rx as i32, ry as i32, rw as u32, rh as u32);
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
pub(crate) fn handle_add_traps(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 12, seq, 139, minor, bo);

    let req = parse_minor!(AddTrapsRequest, data, state, seq, 139, minor);

    let pic_id = req.picture;
    let x_off = req.x_off;
    let y_off = req.y_off;

    debug!(
        "Render AddTraps: pic={pic_id:#x} offset=({x_off},{y_off}) traps={}",
        req.traps.len()
    );

    let (target, fb_w) = {
        let pic = match state.render.pictures.get(&pic_id) {
            Some(p) => p,
            None => {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::VALUE_ERROR,
                    seq,
                    pic_id,
                    139,
                    minor,
                    bo,
                )
            }
        };
        let d = pic.drawable;
        let w = if let Some(px) = state.pixmaps.get(&d) {
            px.framebuffer.width()
        } else if let Some(win) = state.windows.get(&d) {
            win.framebuffer.width()
        } else {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::VALUE_ERROR,
                seq,
                d,
                139,
                minor,
                bo,
            );
        };
        (d, w)
    };

    for trap in req.traps.iter() {
        // Each Trap has top: Spanfix and bot: Spanfix
        // Spanfix has { l: Fixed, r: Fixed, y: Fixed }
        let top_y = fixed_to_f64(trap.top.y);
        let top_l = fixed_to_f64(trap.top.l) + x_off as f64;
        let top_r = fixed_to_f64(trap.top.r) + x_off as f64;
        let bot_y = fixed_to_f64(trap.bot.y);
        let bot_l = fixed_to_f64(trap.bot.l) + x_off as f64;
        let bot_r = fixed_to_f64(trap.bot.r) + x_off as f64;

        let y_start = (top_y + y_off as f64).floor().max(0.0) as u32;
        let y_end = (bot_y + y_off as f64).ceil().max(0.0) as u32;

        if let Some(fb) = state.get_framebuffer_mut(target) {
            for y in y_start..y_end {
                let yf = y as f64 + 0.5;
                // Interpolate left edge
                let left_x = if (bot_y - top_y).abs() > 0.001 {
                    top_l + (bot_l - top_l) * (yf - top_y - y_off as f64) / (bot_y - top_y)
                } else {
                    top_l
                };
                // Interpolate right edge
                let right_x = if (bot_y - top_y).abs() > 0.001 {
                    top_r + (bot_r - top_r) * (yf - top_y - y_off as f64) / (bot_y - top_y)
                } else {
                    top_r
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
