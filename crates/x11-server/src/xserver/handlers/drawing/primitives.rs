//! Drawing primitive operations (opcodes 61-71).

use super::*;
use crate::xserver::core::{GRAPHICS_EXPOSURE_EVENT, NO_EXPOSURE_EVENT};
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{
    ClearAreaRequest, CoordMode, CopyAreaRequest, CopyPlaneRequest, ExposeEvent, FillPolyRequest,
    FillStyle, GraphicsExposureEvent, NoExposureEvent, PolyArcRequest, PolyFillArcRequest,
    PolyFillRectangleRequest, PolyLineRequest, PolyPointRequest, PolyRectangleRequest,
    PolySegmentRequest, SubwindowMode, WindowClass, GX,
};

// ---------------------------------------------------------------------------
// Opcode 61: ClearArea
// ---------------------------------------------------------------------------

pub(crate) fn handle_clear_area(state: &mut ClientState, req: &ClearAreaRequest) -> Vec<u8> {
    let exposures = req.exposures;
    let wid = req.window;

    if !state.windows.contains_key(&wid) {
        return build_error(WINDOW_ERROR, state.sequence, wid, 61, 0);
    }
    // Per X11 spec: ClearArea on an InputOnly window generates BadMatch.
    if state
        .windows
        .get(&wid)
        .is_some_and(|w| w.class == u16::from(WindowClass::INPUT_ONLY))
    {
        return build_error(MATCH_ERROR, state.sequence, wid, 61, 0);
    }

    let x = req.x;
    let y = req.y;
    let mut width = req.width;
    let mut height = req.height;

    let bg = state.windows.get(&wid).map(|w| {
        if width == 0 {
            width = w.width;
        }
        if height == 0 {
            height = w.height;
        }
        w.background_pixel
    });

    let bg_pixel = bg.unwrap_or(0);
    if let Some(fb) = state.get_framebuffer_mut(wid) {
        fb.fill_rect(x, y, width, height, bg_pixel);
    }
    state.notify_damage(wid, x, y, width, height);

    // Generate Expose event if exposures=true and window selects ExposureMask
    if exposures {
        let bo = state.msb_first;
        let seq = state.sequence;
        let event = serialize_event(
            &ExposeEvent {
                response_type: EXPOSE_EVENT,
                sequence: seq,
                window: wid,
                x: x as u16,
                y: y as u16,
                width,
                height,
                count: 0,
            },
            bo,
        );
        state.deliver_event(wid, EventMask::EXPOSURE, &event);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 62: CopyArea
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_area(state: &mut ClientState, req: &CopyAreaRequest) -> Vec<u8> {
    let src = req.src_drawable;
    let dst = req.dst_drawable;
    let gc_id = req.gc;
    let src_x = req.src_x;
    let src_y = req.src_y;
    let dst_x = req.dst_x;
    let dst_y = req.dst_y;
    let width = req.width;
    let height = req.height;

    // Validate resources
    let has_src = state.windows.contains_key(&src) || state.pixmaps.contains_key(&src);
    let has_dst = state.windows.contains_key(&dst) || state.pixmaps.contains_key(&dst);
    if !has_src {
        return build_error(DRAWABLE_ERROR, state.sequence, src, 62, 0);
    }
    if !has_dst {
        return build_error(DRAWABLE_ERROR, state.sequence, dst, 62, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 62, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Sync SHM-backed pixmap data before reading from src
    state.sync_shm_pixmap(src);

    // Check if source is a 1-bit depth pixmap (used for clip masks)
    let src_depth = state.pixmaps.get(&src).map(|p| p.depth).unwrap_or(24);

    let has_clip = !gc.clip_rects.is_empty();
    let include_inferiors = SubwindowMode::from(gc.subwindow_mode)
        == SubwindowMode::INCLUDE_INFERIORS
        && state.windows.contains_key(&src);

    if src == dst && !include_inferiors {
        if let Some(fb) = state.get_framebuffer_mut(src) {
            if has_clip {
                // Self-copy with clipping: extract then put back through clipped path
                let pixels = fb.extract_pixels(src_x, src_y, width, height);
                fb.put_image_gc(
                    dst_x,
                    dst_y,
                    width,
                    height,
                    &pixels,
                    gc.function,
                    gc.plane_mask,
                    &gc.clip_rects,
                );
            } else {
                fb.copy_area_self(src_x, src_y, dst_x, dst_y, width, height);
            }
        }
    } else {
        let pixels = if include_inferiors {
            Some(state.extract_pixels_include_inferiors(src, src_x, src_y, width, height))
        } else {
            state
                .get_framebuffer_mut(src)
                .map(|fb| fb.extract_pixels(src_x, src_y, width, height))
        };
        if let Some(pixels) = pixels {
            if src_depth <= 1 && GX::from(gc.function) != GX::COPY {
                let ca_fg = state.map_color_for_drawable(dst, gc.foreground);
                let ca_bg = state.map_color_for_drawable(dst, gc.background);
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    let fb_w = fb.width() as i32;
                    let fb_h = fb.height() as i32;
                    let src_stride = width as usize * 4;
                    for row in 0..height as usize {
                        let dy = dst_y as i32 + row as i32;
                        if dy < 0 || dy >= fb_h {
                            continue;
                        }
                        for col in 0..width as usize {
                            let dx = dst_x as i32 + col as i32;
                            if dx < 0 || dx >= fb_w {
                                continue;
                            }
                            if has_clip && !should_draw_pixel(dx, dy, &gc.clip_rects) {
                                continue;
                            }
                            let src_off = row * src_stride + col * 4;
                            if src_off + 3 >= pixels.len() {
                                continue;
                            }
                            let src_pixel = crate::framebuffer::read_pixel(&pixels, src_off);
                            let color = if src_pixel != 0 { ca_fg } else { ca_bg };
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else if GX::from(gc.function) != GX::COPY || has_clip {
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    fb.put_image_gc(
                        dst_x,
                        dst_y,
                        width,
                        height,
                        &pixels,
                        gc.function,
                        gc.plane_mask,
                        &gc.clip_rects,
                    );
                }
            } else {
                // GXcopy, no clip -- fast path
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    fb.put_image(dst_x, dst_y, width, height, &pixels);
                }
            }
        }
    }

    // Generate GraphicsExposure or NoExposure events per spec
    if gc.graphics_exposures {
        // Check if source region extends beyond source drawable bounds
        let (src_w, src_h) = state.get_drawable_size(src).unwrap_or((0, 0));
        let mut exposed_rects: Vec<(i16, i16, u16, u16)> = Vec::new();

        // Source clipping: regions requested that fall outside the source drawable
        if src_x < 0 {
            let clip_w = (-src_x).min(width as i16) as u16;
            exposed_rects.push((dst_x, dst_y, clip_w, height));
        }
        if src_y < 0 {
            let clip_h = (-src_y).min(height as i16) as u16;
            exposed_rects.push((dst_x, dst_y, width, clip_h));
        }
        if src_x + width as i16 > src_w as i16 {
            let overshoot = (src_x + width as i16 - src_w as i16).max(0);
            let clip_w = overshoot.min(width as i16) as u16;
            let clip_x = dst_x + width as i16 - overshoot;
            exposed_rects.push((clip_x, dst_y, clip_w, height));
        }
        if src_y + height as i16 > src_h as i16 {
            let overshoot = (src_y + height as i16 - src_h as i16).max(0);
            let clip_h = overshoot.min(height as i16) as u16;
            let clip_y = dst_y + height as i16 - overshoot;
            exposed_rects.push((dst_x, clip_y, width, clip_h));
        }

        if exposed_rects.is_empty() {
            // NoExposure event (type 14)
            let event = serialize_event(
                &NoExposureEvent {
                    response_type: NO_EXPOSURE_EVENT,
                    sequence: state.sequence,
                    drawable: dst,
                    minor_opcode: 0,  // core protocol
                    major_opcode: 62, // CopyArea
                },
                state.msb_first,
            );
            state.pending_events.push(event);
        } else {
            // GraphicsExposure events (type 13) for each exposed region
            let last_idx = exposed_rects.len() - 1;
            for (i, &(ex, ey, ew, eh)) in exposed_rects.iter().enumerate() {
                let count = (last_idx - i) as u16;
                let event = serialize_event(
                    &GraphicsExposureEvent {
                        response_type: GRAPHICS_EXPOSURE_EVENT,
                        sequence: state.sequence,
                        drawable: dst,
                        x: ex as u16,
                        y: ey as u16,
                        width: ew,
                        height: eh,
                        minor_opcode: 0, // core protocol
                        count,
                        major_opcode: 62, // CopyArea
                    },
                    state.msb_first,
                );
                state.pending_events.push(event);
            }
        }
    }
    state.notify_damage(dst, dst_x, dst_y, width, height);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 63: CopyPlane
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_plane(state: &mut ClientState, req: &CopyPlaneRequest) -> Vec<u8> {
    let src = req.src_drawable;
    let dst = req.dst_drawable;
    let gc_id = req.gc;
    let src_x = req.src_x;
    let src_y = req.src_y;
    let dst_x = req.dst_x;
    let dst_y = req.dst_y;
    let width = req.width;
    let height = req.height;
    let bit_plane = req.bit_plane;

    // Validate: bit_plane must have exactly one bit set
    if bit_plane == 0 || (bit_plane & (bit_plane - 1)) != 0 {
        return build_error(VALUE_ERROR, state.sequence, bit_plane, 63, 0);
    }

    // Validate resources
    let has_src = state.windows.contains_key(&src) || state.pixmaps.contains_key(&src);
    let has_dst = state.windows.contains_key(&dst) || state.pixmaps.contains_key(&dst);
    if !has_src {
        return build_error(DRAWABLE_ERROR, state.sequence, src, 63, 0);
    }
    if !has_dst {
        return build_error(DRAWABLE_ERROR, state.sequence, dst, 63, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 63, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Extract source pixels
    state.sync_shm_pixmap(src);
    let pixels = state
        .get_framebuffer_mut(src)
        .map(|fb| fb.extract_pixels(src_x, src_y, width, height));

    let cp_fg = state.map_color_for_drawable(dst, gc.foreground);
    let cp_bg = state.map_color_for_drawable(dst, gc.background);
    if let Some(pixels) = pixels {
        if let Some(fb) = state.get_framebuffer_mut(dst) {
            let src_stride = width as usize * 4;
            let has_clip = !gc.clip_rects.is_empty();
            for row in 0..height as usize {
                for col in 0..width as usize {
                    let dx = dst_x as i32 + col as i32;
                    let dy = dst_y as i32 + row as i32;
                    if has_clip && !should_draw_pixel(dx, dy, &gc.clip_rects) {
                        continue;
                    }
                    let src_off = row * src_stride + col * 4;
                    if src_off + 3 >= pixels.len() {
                        continue;
                    }
                    // bit_plane refers to the X11 visual's pixel plane bits
                    // (R = 16-23, G = 8-15, B = 0-7); read_pixel returns the
                    // pixel as 0x00RRGGBB regardless of storage byte order.
                    let src_pixel = crate::framebuffer::read_pixel(&pixels, src_off);
                    let color = if (src_pixel & bit_plane) != 0 {
                        cp_fg
                    } else {
                        cp_bg
                    };
                    fb.draw_point_with_func(dx, dy, color, gc.function);
                }
            }
        }
    }

    // Generate GraphicsExposure or NoExposure events per spec
    if gc.graphics_exposures {
        let (src_w, src_h) = state.get_drawable_size(src).unwrap_or((0, 0));
        let mut exposed_rects: Vec<(i16, i16, u16, u16)> = Vec::new();

        if src_x < 0 {
            let clip_w = (-src_x).min(width as i16) as u16;
            exposed_rects.push((dst_x, dst_y, clip_w, height));
        }
        if src_y < 0 {
            let clip_h = (-src_y).min(height as i16) as u16;
            exposed_rects.push((dst_x, dst_y, width, clip_h));
        }
        if src_x + width as i16 > src_w as i16 {
            let overshoot = (src_x + width as i16 - src_w as i16).max(0);
            let clip_w = overshoot.min(width as i16) as u16;
            let clip_x = dst_x + width as i16 - overshoot;
            exposed_rects.push((clip_x, dst_y, clip_w, height));
        }
        if src_y + height as i16 > src_h as i16 {
            let overshoot = (src_y + height as i16 - src_h as i16).max(0);
            let clip_h = overshoot.min(height as i16) as u16;
            let clip_y = dst_y + height as i16 - overshoot;
            exposed_rects.push((dst_x, clip_y, width, clip_h));
        }

        if exposed_rects.is_empty() {
            let event = serialize_event(
                &NoExposureEvent {
                    response_type: NO_EXPOSURE_EVENT,
                    sequence: state.sequence,
                    drawable: dst,
                    minor_opcode: 0,  // core protocol
                    major_opcode: 63, // CopyPlane
                },
                state.msb_first,
            );
            state.pending_events.push(event);
        } else {
            let last_idx = exposed_rects.len() - 1;
            for (i, &(ex, ey, ew, eh)) in exposed_rects.iter().enumerate() {
                let count = (last_idx - i) as u16;
                let event = serialize_event(
                    &GraphicsExposureEvent {
                        response_type: GRAPHICS_EXPOSURE_EVENT,
                        sequence: state.sequence,
                        drawable: dst,
                        x: ex as u16,
                        y: ey as u16,
                        width: ew,
                        height: eh,
                        minor_opcode: 0, // core protocol
                        count,
                        major_opcode: 63, // CopyPlane
                    },
                    state.msb_first,
                );
                state.pending_events.push(event);
            }
        }
    }
    state.notify_damage(dst, dst_x, dst_y, width, height);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 64: PolyPoint
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_point(state: &mut ClientState, req: &PolyPointRequest) -> Vec<u8> {
    let coord_mode = CoordMode::from(req.coordinate_mode);
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 64, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 64, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points = Vec::new();
    let mut last_x: i16 = 0;
    let mut last_y: i16 = 0;
    for pt in req.points.iter() {
        let mut x = pt.x;
        let mut y = pt.y;
        if coord_mode == CoordMode::PREVIOUS {
            x += last_x;
            y += last_y;
        }
        last_x = x;
        last_y = y;
        points.push((x, y));
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y) in &points {
            fb.draw_point_gc(
                x as i32,
                y as i32,
                fg,
                gc.function,
                gc.plane_mask,
                &gc.clip_rects,
            );
        }
    }
    if !points.is_empty() {
        let (mut min_x, mut min_y) = points[0];
        let (mut max_x, mut max_y) = points[0];
        for &(x, y) in &points[1..] {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        state.notify_damage(
            drawable,
            min_x,
            min_y,
            (max_x - min_x + 1) as u16,
            (max_y - min_y + 1) as u16,
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 65: PolyLine
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_line(state: &mut ClientState, req: &PolyLineRequest) -> Vec<u8> {
    let coord_mode = CoordMode::from(req.coordinate_mode);
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 65, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 65, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points: Vec<(i16, i16)> = Vec::new();
    for pt in req.points.iter() {
        let x = pt.x;
        let y = pt.y;
        if coord_mode == CoordMode::PREVIOUS && !points.is_empty() {
            let (px, py) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            state.pixmaps.get(&gc.tile).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        state.pixmaps.get(&gc.stipple).map(|p| {
            (
                p.framebuffer.data().to_vec(),
                p.width as u32,
                p.height as u32,
            )
        })
    } else {
        None
    };

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        let dashes = &gc.dash_list;
        let pts: Vec<(i32, i32)> = points.iter().map(|&(x, y)| (x as i32, y as i32)).collect();
        match FillStyle::from(gc.fill_style) {
            FillStyle::TILED => {
                // Tiled: draw each segment with tile pattern
                if let Some((ref tdata, tw, th)) = tile_data {
                    for w in pts.windows(2) {
                        fb.draw_line_tiled(
                            w[0].0,
                            w[0].1,
                            w[1].0,
                            w[1].1,
                            tdata,
                            tw,
                            th,
                            gc.ts_x,
                            gc.ts_y,
                            gc.function,
                            gc.plane_mask,
                            gc.cap_style,
                            &gc.clip_rects,
                        );
                    }
                } else {
                    fb.draw_polyline_gc(
                        &pts,
                        fg,
                        gc.line_width,
                        gc.function,
                        gc.plane_mask,
                        gc.line_style,
                        gc.cap_style,
                        gc.join_style,
                        gc.dash_offset,
                        dashes,
                        bg,
                        &gc.clip_rects,
                    );
                }
            }
            FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                // Stippled/OpaqueStippled: draw each segment with stipple pattern
                if let Some((ref sdata, sw, sh)) = stipple_data {
                    for w in pts.windows(2) {
                        fb.draw_line_stippled(
                            w[0].0,
                            w[0].1,
                            w[1].0,
                            w[1].1,
                            fg,
                            bg,
                            sdata,
                            sw,
                            sh,
                            gc.ts_x,
                            gc.ts_y,
                            FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                            gc.function,
                            gc.plane_mask,
                            gc.cap_style,
                            &gc.clip_rects,
                        );
                    }
                } else {
                    fb.draw_polyline_gc(
                        &pts,
                        fg,
                        gc.line_width,
                        gc.function,
                        gc.plane_mask,
                        gc.line_style,
                        gc.cap_style,
                        gc.join_style,
                        gc.dash_offset,
                        dashes,
                        bg,
                        &gc.clip_rects,
                    );
                }
            }
            _ => {
                // Solid (0)
                fb.draw_polyline_gc(
                    &pts,
                    fg,
                    gc.line_width,
                    gc.function,
                    gc.plane_mask,
                    gc.line_style,
                    gc.cap_style,
                    gc.join_style,
                    gc.dash_offset,
                    dashes,
                    bg,
                    &gc.clip_rects,
                );
            }
        }
    }
    if !points.is_empty() {
        let lw = gc.line_width.max(1) as i16;
        let (mut min_x, mut min_y) = points[0];
        let (mut max_x, mut max_y) = points[0];
        for &(x, y) in &points[1..] {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        state.notify_damage(
            drawable,
            min_x - lw,
            min_y - lw,
            (max_x - min_x + 2 * lw) as u16,
            (max_y - min_y + 2 * lw) as u16,
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 66: PolySegment
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_segment(state: &mut ClientState, req: &PolySegmentRequest) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 66, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 66, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let segments: Vec<(i16, i16, i16, i16)> = req
        .segments
        .iter()
        .map(|s| (s.x1, s.y1, s.x2, s.y2))
        .collect();

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            state.pixmaps.get(&gc.tile).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        state.pixmaps.get(&gc.stipple).map(|p| {
            (
                p.framebuffer.data().to_vec(),
                p.width as u32,
                p.height as u32,
            )
        })
    } else {
        None
    };

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        let dashes = &gc.dash_list;
        for &(x1, y1, x2, y2) in &segments {
            match FillStyle::from(gc.fill_style) {
                FillStyle::TILED => {
                    if let Some((ref tdata, tw, th)) = tile_data {
                        fb.draw_line_tiled(
                            x1 as i32,
                            y1 as i32,
                            x2 as i32,
                            y2 as i32,
                            tdata,
                            tw,
                            th,
                            gc.ts_x,
                            gc.ts_y,
                            gc.function,
                            gc.plane_mask,
                            gc.cap_style,
                            &gc.clip_rects,
                        );
                    } else {
                        fb.draw_line_gc(
                            x1 as i32,
                            y1 as i32,
                            x2 as i32,
                            y2 as i32,
                            fg,
                            gc.line_width,
                            gc.function,
                            gc.plane_mask,
                            gc.line_style,
                            gc.cap_style,
                            gc.join_style,
                            gc.dash_offset,
                            dashes,
                            bg,
                            &gc.clip_rects,
                        );
                    }
                }
                FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                    if let Some((ref sdata, sw, sh)) = stipple_data {
                        fb.draw_line_stippled(
                            x1 as i32,
                            y1 as i32,
                            x2 as i32,
                            y2 as i32,
                            fg,
                            bg,
                            sdata,
                            sw,
                            sh,
                            gc.ts_x,
                            gc.ts_y,
                            FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                            gc.function,
                            gc.plane_mask,
                            gc.cap_style,
                            &gc.clip_rects,
                        );
                    } else {
                        fb.draw_line_gc(
                            x1 as i32,
                            y1 as i32,
                            x2 as i32,
                            y2 as i32,
                            fg,
                            gc.line_width,
                            gc.function,
                            gc.plane_mask,
                            gc.line_style,
                            gc.cap_style,
                            gc.join_style,
                            gc.dash_offset,
                            dashes,
                            bg,
                            &gc.clip_rects,
                        );
                    }
                }
                _ => {
                    fb.draw_line_gc(
                        x1 as i32,
                        y1 as i32,
                        x2 as i32,
                        y2 as i32,
                        fg,
                        gc.line_width,
                        gc.function,
                        gc.plane_mask,
                        gc.line_style,
                        gc.cap_style,
                        gc.join_style,
                        gc.dash_offset,
                        dashes,
                        bg,
                        &gc.clip_rects,
                    );
                }
            }
        }
    }
    if !segments.is_empty() {
        let lw = gc.line_width.max(1) as i16;
        let mut min_x = i16::MAX;
        let mut min_y = i16::MAX;
        let mut max_x = i16::MIN;
        let mut max_y = i16::MIN;
        for &(x1, y1, x2, y2) in &segments {
            min_x = min_x.min(x1).min(x2);
            min_y = min_y.min(y1).min(y2);
            max_x = max_x.max(x1).max(x2);
            max_y = max_y.max(y1).max(y2);
        }
        state.notify_damage(
            drawable,
            min_x - lw,
            min_y - lw,
            (max_x - min_x + 2 * lw) as u16,
            (max_y - min_y + 2 * lw) as u16,
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 67: PolyRectangle
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_rectangle(
    state: &mut ClientState,
    req: &PolyRectangleRequest,
) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 67, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 67, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let rects: Vec<(i16, i16, u16, u16)> = req
        .rectangles
        .iter()
        .map(|r| (r.x, r.y, r.width, r.height))
        .collect();

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            state.pixmaps.get(&gc.tile).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        state.pixmaps.get(&gc.stipple).map(|p| {
            (
                p.framebuffer.data().to_vec(),
                p.width as u32,
                p.height as u32,
            )
        })
    } else {
        None
    };

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        let dashes = &gc.dash_list;
        for &(x, y, width, height) in &rects {
            let x2 = x as i32 + width as i32;
            let y2 = y as i32 + height as i32;
            let lines = [
                (x as i32, y as i32, x2, y as i32),
                (x2, y as i32, x2, y2),
                (x2, y2, x as i32, y2),
                (x as i32, y2, x as i32, y as i32),
            ];
            match FillStyle::from(gc.fill_style) {
                FillStyle::TILED => {
                    if let Some((ref tdata, tw, th)) = tile_data {
                        for &(lx0, ly0, lx1, ly1) in &lines {
                            fb.draw_line_tiled(
                                lx0,
                                ly0,
                                lx1,
                                ly1,
                                tdata,
                                tw,
                                th,
                                gc.ts_x,
                                gc.ts_y,
                                gc.function,
                                gc.plane_mask,
                                gc.cap_style,
                                &gc.clip_rects,
                            );
                        }
                    } else {
                        for &(lx0, ly0, lx1, ly1) in &lines {
                            fb.draw_line_gc(
                                lx0,
                                ly0,
                                lx1,
                                ly1,
                                fg,
                                gc.line_width,
                                gc.function,
                                gc.plane_mask,
                                gc.line_style,
                                gc.cap_style,
                                gc.join_style,
                                gc.dash_offset,
                                dashes,
                                bg,
                                &gc.clip_rects,
                            );
                        }
                    }
                }
                FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                    if let Some((ref sdata, sw, sh)) = stipple_data {
                        for &(lx0, ly0, lx1, ly1) in &lines {
                            fb.draw_line_stippled(
                                lx0,
                                ly0,
                                lx1,
                                ly1,
                                fg,
                                bg,
                                sdata,
                                sw,
                                sh,
                                gc.ts_x,
                                gc.ts_y,
                                FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                                gc.function,
                                gc.plane_mask,
                                gc.cap_style,
                                &gc.clip_rects,
                            );
                        }
                    } else {
                        for &(lx0, ly0, lx1, ly1) in &lines {
                            fb.draw_line_gc(
                                lx0,
                                ly0,
                                lx1,
                                ly1,
                                fg,
                                gc.line_width,
                                gc.function,
                                gc.plane_mask,
                                gc.line_style,
                                gc.cap_style,
                                gc.join_style,
                                gc.dash_offset,
                                dashes,
                                bg,
                                &gc.clip_rects,
                            );
                        }
                    }
                }
                _ => {
                    for &(lx0, ly0, lx1, ly1) in &lines {
                        fb.draw_line_gc(
                            lx0,
                            ly0,
                            lx1,
                            ly1,
                            fg,
                            gc.line_width,
                            gc.function,
                            gc.plane_mask,
                            gc.line_style,
                            gc.cap_style,
                            gc.join_style,
                            gc.dash_offset,
                            dashes,
                            bg,
                            &gc.clip_rects,
                        );
                    }
                }
            }
        }
    }
    let lw = gc.line_width.max(1) as i16;
    for &(x, y, width, height) in &rects {
        state.notify_damage(
            drawable,
            x - lw,
            y - lw,
            width.saturating_add(2 * lw as u16),
            height.saturating_add(2 * lw as u16),
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 68: PolyArc
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_arc(state: &mut ClientState, req: &PolyArcRequest) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 68, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 68, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let arcs: Vec<(i16, i16, u16, u16, i16, i16)> = req
        .arcs
        .iter()
        .map(|a| (a.x, a.y, a.width, a.height, a.angle1, a.angle2))
        .collect();

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, width, height, angle1, angle2) in &arcs {
            fb.draw_arc_gc(
                x,
                y,
                width,
                height,
                angle1,
                angle2,
                false,
                fg,
                bg,
                gc.line_width,
                gc.function,
                gc.plane_mask,
                gc.line_style,
                gc.dash_offset,
                &gc.dash_list,
                &gc.clip_rects,
                gc.arc_mode,
            );
        }
    }
    let lw = gc.line_width.max(1) as i16;
    for &(x, y, width, height, _, _) in &arcs {
        state.notify_damage(
            drawable,
            x - lw,
            y - lw,
            width.saturating_add(2 * lw as u16),
            height.saturating_add(2 * lw as u16),
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 69: FillPoly
// ---------------------------------------------------------------------------

pub(crate) fn handle_fill_poly(state: &mut ClientState, req: &FillPolyRequest) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 69, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 69, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();
    let coord_mode = CoordMode::from(req.coordinate_mode); // 0 = Origin, 1 = Previous

    let mut points = Vec::new();
    for pt in req.points.iter() {
        let x = pt.x;
        let y = pt.y;
        if coord_mode == CoordMode::PREVIOUS && !points.is_empty() {
            let (px, py): (i16, i16) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            state.pixmaps.get(&gc.tile).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        state.pixmaps.get(&gc.stipple).map(|p| {
            (
                p.framebuffer.data().to_vec(),
                p.width as u32,
                p.height as u32,
            )
        })
    } else {
        None
    };

    if points.len() >= 3 {
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            match FillStyle::from(gc.fill_style) {
                FillStyle::TILED => {
                    // Tiled: rasterize polygon using tile pattern
                    if let Some((ref tdata, tw, th)) = tile_data {
                        fb.fill_polygon_tiled(
                            &points,
                            tdata,
                            tw,
                            th,
                            gc.ts_x,
                            gc.ts_y,
                            gc.fill_rule,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    } else {
                        fb.fill_polygon_gc(
                            &points,
                            fg,
                            gc.fill_rule,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    }
                }
                FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                    // Stippled/OpaqueStippled: rasterize with stipple pattern
                    if let Some((ref sdata, sw, sh)) = stipple_data {
                        fb.fill_polygon_stippled(
                            &points,
                            fg,
                            bg,
                            sdata,
                            sw,
                            sh,
                            gc.ts_x,
                            gc.ts_y,
                            FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                            gc.fill_rule,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    } else {
                        fb.fill_polygon_gc(
                            &points,
                            fg,
                            gc.fill_rule,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    }
                }
                _ => {
                    // Solid (0)
                    fb.fill_polygon_gc(
                        &points,
                        fg,
                        gc.fill_rule,
                        gc.function,
                        gc.plane_mask,
                        &gc.clip_rects,
                    );
                }
            }
        }
        let (mut min_x, mut min_y) = points[0];
        let (mut max_x, mut max_y) = points[0];
        for &(x, y) in &points[1..] {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        state.notify_damage(
            drawable,
            min_x,
            min_y,
            (max_x - min_x + 1) as u16,
            (max_y - min_y + 1) as u16,
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 70: PolyFillRectangle
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_fill_rectangle(
    state: &mut ClientState,
    req: &PolyFillRectangleRequest,
) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 70, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 70, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let rects: Vec<(i16, i16, u16, u16)> = req
        .rectangles
        .iter()
        .map(|r| (r.x, r.y, r.width, r.height))
        .collect();

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);
    info!(
        "PolyFillRect: draw={drawable:#x} fg={fg:#x} gc={gc_id:#x} rects={} fn={} fill_style={}",
        rects.len(),
        gc.function,
        gc.fill_style
    );

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            // Tiled: use tile pixmap
            state.pixmaps.get(&gc.tile).map(|p| {
                let d = p.framebuffer.data().to_vec();
                (d, p.width as u32, p.height as u32)
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        // Stippled or OpaqueStippled: use stipple pixmap
        state.pixmaps.get(&gc.stipple).map(|p| {
            let d = p.framebuffer.data().to_vec();
            (d, p.width as u32, p.height as u32)
        })
    } else {
        None
    };

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, width, height) in &rects {
            match FillStyle::from(gc.fill_style) {
                FillStyle::TILED => {
                    // Tiled
                    if let Some((ref data, tw, th)) = tile_data {
                        if gc.clip_rects.is_empty() {
                            fb.fill_rect_tiled(
                                x,
                                y,
                                width,
                                height,
                                data,
                                tw,
                                th,
                                gc.ts_x,
                                gc.ts_y,
                                gc.function,
                                gc.plane_mask,
                            );
                        } else {
                            // Apply clip: intersect each clip rect with the fill rect
                            for &(cx, cy, cw, ch) in &gc.clip_rects {
                                let (ix, iy, iw, ih) =
                                    intersect_rects(x, y, width, height, cx, cy, cw, ch);
                                if iw > 0 && ih > 0 {
                                    fb.fill_rect_tiled(
                                        ix,
                                        iy,
                                        iw,
                                        ih,
                                        data,
                                        tw,
                                        th,
                                        gc.ts_x,
                                        gc.ts_y,
                                        gc.function,
                                        gc.plane_mask,
                                    );
                                }
                            }
                        }
                    } else {
                        fb.fill_rect_rop_clipped(
                            x,
                            y,
                            width,
                            height,
                            fg,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    }
                }
                FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                    // Stippled (2) or OpaqueStippled (3)
                    if let Some((ref data, sw, sh)) = stipple_data {
                        if gc.clip_rects.is_empty() {
                            fb.fill_rect_stippled(
                                x,
                                y,
                                width,
                                height,
                                fg,
                                bg,
                                data,
                                sw,
                                sh,
                                gc.ts_x,
                                gc.ts_y,
                                FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                                gc.function,
                                gc.plane_mask,
                            );
                        } else {
                            for &(cx, cy, cw, ch) in &gc.clip_rects {
                                let (ix, iy, iw, ih) =
                                    intersect_rects(x, y, width, height, cx, cy, cw, ch);
                                if iw > 0 && ih > 0 {
                                    fb.fill_rect_stippled(
                                        ix,
                                        iy,
                                        iw,
                                        ih,
                                        fg,
                                        bg,
                                        data,
                                        sw,
                                        sh,
                                        gc.ts_x,
                                        gc.ts_y,
                                        FillStyle::from(gc.fill_style)
                                            == FillStyle::OPAQUE_STIPPLED,
                                        gc.function,
                                        gc.plane_mask,
                                    );
                                }
                            }
                        }
                    } else {
                        fb.fill_rect_rop_clipped(
                            x,
                            y,
                            width,
                            height,
                            fg,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    }
                }
                _ => {
                    // Solid (0) or fallback
                    fb.fill_rect_rop_clipped(
                        x,
                        y,
                        width,
                        height,
                        fg,
                        gc.function,
                        gc.plane_mask,
                        &gc.clip_rects,
                    );
                }
            }
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height) in &rects {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 71: PolyFillArc
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_fill_arc(state: &mut ClientState, req: &PolyFillArcRequest) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 71, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 71, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let arcs: Vec<(i16, i16, u16, u16, i16, i16)> = req
        .arcs
        .iter()
        .map(|a| (a.x, a.y, a.width, a.height, a.angle1, a.angle2))
        .collect();

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);
    info!("PolyFillArc: gc={gc_id:#x} func={} fg_raw={:#x} fg_mapped={fg:#x} draw={drawable:#x} fill_style={}", gc.function, gc.foreground, gc.fill_style);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> =
        if FillStyle::from(gc.fill_style) == FillStyle::TILED {
            state.pixmaps.get(&gc.tile).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            })
        } else {
            None
        };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if matches!(
        FillStyle::from(gc.fill_style),
        FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED
    ) {
        state.pixmaps.get(&gc.stipple).map(|p| {
            (
                p.framebuffer.data().to_vec(),
                p.width as u32,
                p.height as u32,
            )
        })
    } else {
        None
    };

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, width, height, angle1, angle2) in &arcs {
            match FillStyle::from(gc.fill_style) {
                FillStyle::TILED => {
                    // Tiled: fill arc region with tile pattern (per-pixel within arc shape)
                    if let Some((ref tdata, tw, th)) = tile_data {
                        fb.fill_arc_tiled(
                            x,
                            y,
                            width,
                            height,
                            angle1,
                            angle2,
                            tdata,
                            tw,
                            th,
                            gc.ts_x,
                            gc.ts_y,
                            gc.arc_mode,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    } else {
                        // Fallback to solid fill if tile pixmap is missing
                        fb.draw_arc_gc(
                            x,
                            y,
                            width,
                            height,
                            angle1,
                            angle2,
                            true,
                            fg,
                            bg,
                            gc.line_width,
                            gc.function,
                            gc.plane_mask,
                            gc.line_style,
                            gc.dash_offset,
                            &gc.dash_list,
                            &gc.clip_rects,
                            gc.arc_mode,
                        );
                    }
                }
                FillStyle::STIPPLED | FillStyle::OPAQUE_STIPPLED => {
                    // Stippled/OpaqueStippled: fill arc region with stipple pattern
                    if let Some((ref sdata, sw, sh)) = stipple_data {
                        fb.fill_arc_stippled(
                            x,
                            y,
                            width,
                            height,
                            angle1,
                            angle2,
                            fg,
                            bg,
                            sdata,
                            sw,
                            sh,
                            gc.ts_x,
                            gc.ts_y,
                            FillStyle::from(gc.fill_style) == FillStyle::OPAQUE_STIPPLED,
                            gc.arc_mode,
                            gc.function,
                            gc.plane_mask,
                            &gc.clip_rects,
                        );
                    } else {
                        // Fallback to solid fill if stipple pixmap is missing
                        fb.draw_arc_gc(
                            x,
                            y,
                            width,
                            height,
                            angle1,
                            angle2,
                            true,
                            fg,
                            bg,
                            gc.line_width,
                            gc.function,
                            gc.plane_mask,
                            gc.line_style,
                            gc.dash_offset,
                            &gc.dash_list,
                            &gc.clip_rects,
                            gc.arc_mode,
                        );
                    }
                }
                _ => {
                    // Solid (0)
                    fb.draw_arc_gc(
                        x,
                        y,
                        width,
                        height,
                        angle1,
                        angle2,
                        true,
                        fg,
                        bg,
                        gc.line_width,
                        gc.function,
                        gc.plane_mask,
                        gc.line_style,
                        gc.dash_offset,
                        &gc.dash_list,
                        &gc.clip_rects,
                        gc.arc_mode,
                    );
                }
            }
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height, _, _) in &arcs {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}
