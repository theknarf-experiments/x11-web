//! Drawing primitive operations (opcodes 61-71).

use super::*;
use crate::xserver::core::{require_len, GRAPHICS_EXPOSURE_EVENT, NO_EXPOSURE_EVENT};

// ---------------------------------------------------------------------------
// Opcode 61: ClearArea
// ---------------------------------------------------------------------------

pub(crate) fn handle_clear_area(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 61);

    let exposures = data[1] != 0;
    let wid = state.read_u32(data, 4);

    if !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, state.sequence, wid, 61, 0);
    }
    // Per X11 spec: ClearArea on an InputOnly window generates BadMatch.
    if state.windows.get(&wid).is_some_and(|w| w.class == 2) {
        return build_error(BAD_MATCH, state.sequence, wid, 61, 0);
    }

    let x = state.read_i16(data, 8);
    let y = state.read_i16(data, 10);
    let mut width = state.read_u16(data, 12);
    let mut height = state.read_u16(data, 14);

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
        let mut event = [0u8; 32];
        event[0] = EXPOSE_EVENT;
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, wid, bo);
        write_i16_bo(&mut event, 8, x, bo);
        write_i16_bo(&mut event, 10, y, bo);
        write_u16_bo(&mut event, 12, width, bo);
        write_u16_bo(&mut event, 14, height, bo);
        // count = 0 (last in sequence)
        if state
            .windows
            .get(&wid)
            .is_some_and(|w| w.event_mask & EXPOSURE_MASK != 0)
        {
            state.pending_events.push(event.to_vec());
        }
        // Broadcast to other clients that selected ExposureMask on this window
        state.broadcast_event(wid, EXPOSURE_MASK, &event);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 62: CopyArea
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_area(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 28, state.sequence, 62);

    let src = state.read_u32(data, 4);
    let dst = state.read_u32(data, 8);
    let gc_id = state.read_u32(data, 12);
    let src_x = state.read_i16(data, 16);
    let src_y = state.read_i16(data, 18);
    let dst_x = state.read_i16(data, 20);
    let dst_y = state.read_i16(data, 22);
    let width = state.read_u16(data, 24);
    let height = state.read_u16(data, 26);

    // Validate resources
    let has_src = state.windows.contains_key(&src) || state.pixmaps.contains_key(&src);
    let has_dst = state.windows.contains_key(&dst) || state.pixmaps.contains_key(&dst);
    if !has_src {
        return build_error(BAD_DRAWABLE, state.sequence, src, 62, 0);
    }
    if !has_dst {
        return build_error(BAD_DRAWABLE, state.sequence, dst, 62, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 62, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Sync SHM-backed pixmap data before reading from src
    state.sync_shm_pixmap(src);

    // Check if source is a 1-bit depth pixmap (used for clip masks)
    let src_depth = state.pixmaps.get(&src).map(|p| p.depth).unwrap_or(24);

    let has_clip = !gc.clip_rects.is_empty();
    // subwindow_mode: 0=ClipByChildren, 1=IncludeInferiors
    let include_inferiors = gc.subwindow_mode == 1 && state.windows.contains_key(&src);

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
            if src_depth <= 1 && gc.function != 3 {
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
                            let src_pixel = pixels[src_off] as u32
                                | (pixels[src_off + 1] as u32) << 8
                                | (pixels[src_off + 2] as u32) << 16;
                            let color = if src_pixel != 0 { ca_fg } else { ca_bg };
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else if gc.function != 3 || has_clip {
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
            let mut event = [0u8; 32];
            event[0] = NO_EXPOSURE_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, dst);
            state.write_u16(&mut event, 8, 0u16); // minor_opcode: 0 for core protocol
            event[10] = 62; // major_opcode: CopyArea
            state.pending_events.push(event.to_vec());
        } else {
            // GraphicsExposure events (type 13) for each exposed region
            let last_idx = exposed_rects.len() - 1;
            for (i, &(ex, ey, ew, eh)) in exposed_rects.iter().enumerate() {
                let mut event = [0u8; 32];
                event[0] = GRAPHICS_EXPOSURE_EVENT;
                state.write_u16(&mut event, 2, state.sequence);
                state.write_u32(&mut event, 4, dst);
                state.write_u16(&mut event, 8, ex as u16);
                state.write_u16(&mut event, 10, ey as u16);
                state.write_u16(&mut event, 12, ew);
                state.write_u16(&mut event, 14, eh);
                state.write_u16(&mut event, 16, 0u16); // minor_opcode: 0 for core protocol
                let count = (last_idx - i) as u16; // remaining events
                state.write_u16(&mut event, 18, count);
                event[20] = 62; // major_opcode: CopyArea
                state.pending_events.push(event.to_vec());
            }
        }
    }
    state.notify_damage(dst, dst_x, dst_y, width, height);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 63: CopyPlane
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_plane(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 32, state.sequence, 63);

    let src = state.read_u32(data, 4);
    let dst = state.read_u32(data, 8);
    let gc_id = state.read_u32(data, 12);
    let src_x = state.read_i16(data, 16);
    let src_y = state.read_i16(data, 18);
    let dst_x = state.read_i16(data, 20);
    let dst_y = state.read_i16(data, 22);
    let width = state.read_u16(data, 24);
    let height = state.read_u16(data, 26);
    let bit_plane = state.read_u32(data, 28);

    // Validate: bit_plane must have exactly one bit set
    if bit_plane == 0 || (bit_plane & (bit_plane - 1)) != 0 {
        return build_error(BAD_VALUE, state.sequence, bit_plane, 63, 0);
    }

    // Validate resources
    let has_src = state.windows.contains_key(&src) || state.pixmaps.contains_key(&src);
    let has_dst = state.windows.contains_key(&dst) || state.pixmaps.contains_key(&dst);
    if !has_src {
        return build_error(BAD_DRAWABLE, state.sequence, src, 63, 0);
    }
    if !has_dst {
        return build_error(BAD_DRAWABLE, state.sequence, dst, 63, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 63, 0);
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
                    let src_pixel = pixels[src_off] as u32
                        | (pixels[src_off + 1] as u32) << 8
                        | (pixels[src_off + 2] as u32) << 16
                        | (pixels[src_off + 3] as u32) << 24;
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
            let mut event = [0u8; 32];
            event[0] = NO_EXPOSURE_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, dst);
            state.write_u16(&mut event, 8, 0u16); // minor_opcode: 0 for core protocol
            event[10] = 63; // major_opcode: CopyPlane
            state.pending_events.push(event.to_vec());
        } else {
            let last_idx = exposed_rects.len() - 1;
            for (i, &(ex, ey, ew, eh)) in exposed_rects.iter().enumerate() {
                let mut event = [0u8; 32];
                event[0] = GRAPHICS_EXPOSURE_EVENT;
                state.write_u16(&mut event, 2, state.sequence);
                state.write_u32(&mut event, 4, dst);
                state.write_u16(&mut event, 8, ex as u16);
                state.write_u16(&mut event, 10, ey as u16);
                state.write_u16(&mut event, 12, ew);
                state.write_u16(&mut event, 14, eh);
                state.write_u16(&mut event, 16, 0u16); // minor_opcode: 0 for core protocol
                let count = (last_idx - i) as u16;
                state.write_u16(&mut event, 18, count);
                event[20] = 63; // major_opcode: CopyPlane
                state.pending_events.push(event.to_vec());
            }
        }
    }
    state.notify_damage(dst, dst_x, dst_y, width, height);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 64: PolyPoint
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_point(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 64);

    let coord_mode = data[1];
    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 64, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 64, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points = Vec::new();
    let mut last_x: i16 = 0;
    let mut last_y: i16 = 0;
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let mut x = state.read_i16(data, offset);
        let mut y = state.read_i16(data, offset + 2);
        if coord_mode == 1 {
            x += last_x;
            y += last_y;
        }
        last_x = x;
        last_y = y;
        points.push((x, y));
        offset += 4;
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

pub(crate) fn handle_poly_line(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 65);

    let coord_mode = data[1];
    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 65, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 65, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points: Vec<(i16, i16)> = Vec::new();
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
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
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
        match gc.fill_style {
            1 => {
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
            2 | 3 => {
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
                            gc.fill_style == 3,
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

pub(crate) fn handle_poly_segment(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 66);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 66, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 66, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut segments = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x1 = state.read_i16(data, offset);
        let y1 = state.read_i16(data, offset + 2);
        let x2 = state.read_i16(data, offset + 4);
        let y2 = state.read_i16(data, offset + 6);
        segments.push((x1, y1, x2, y2));
        offset += 8;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
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
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
            match gc.fill_style {
                1 => {
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
                2 | 3 => {
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
                            gc.fill_style == 3,
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

pub(crate) fn handle_poly_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 67);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 67, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 67, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        let width = state.read_u16(data, offset + 4);
        let height = state.read_u16(data, offset + 6);
        rects.push((x, y, width, height));
        offset += 8;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
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
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
            match gc.fill_style {
                1 => {
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
                2 | 3 => {
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
                                gc.fill_style == 3,
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

pub(crate) fn handle_poly_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 68);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 68, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 68, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        let width = state.read_u16(data, offset + 4);
        let height = state.read_u16(data, offset + 6);
        let angle1 = state.read_i16(data, offset + 8);
        let angle2 = state.read_i16(data, offset + 10);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

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

pub(crate) fn handle_fill_poly(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 69);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 69, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 69, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();
    let coord_mode = data[13]; // 0 = Origin, 1 = Previous

    let mut points = Vec::new();
    let mut offset = 16;
    while offset + 4 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py): (i16, i16) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
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
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
            match gc.fill_style {
                1 => {
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
                2 | 3 => {
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
                            gc.fill_style == 3,
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

pub(crate) fn handle_poly_fill_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 70);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 70, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 70, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        let width = state.read_u16(data, offset + 4);
        let height = state.read_u16(data, offset + 6);
        rects.push((x, y, width, height));
        offset += 8;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);
    info!(
        "PolyFillRect: draw={drawable:#x} fg={fg:#x} gc={gc_id:#x} rects={} fn={} fill_style={}",
        rects.len(),
        gc.function,
        gc.fill_style
    );

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
        // Tiled: use tile pixmap
        state.pixmaps.get(&gc.tile).map(|p| {
            let d = p.framebuffer.data().to_vec();
            (d, p.width as u32, p.height as u32)
        })
    } else {
        None
    };
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
            match gc.fill_style {
                1 => {
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
                2 | 3 => {
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
                                gc.fill_style == 3,
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
                                        gc.fill_style == 3,
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

pub(crate) fn handle_poly_fill_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 71);

    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 71, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 71, 0);
    }

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        let width = state.read_u16(data, offset + 4);
        let height = state.read_u16(data, offset + 6);
        let angle1 = state.read_i16(data, offset + 8);
        let angle2 = state.read_i16(data, offset + 10);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    let bg = state.map_color_for_drawable(drawable, gc.background);
    info!("PolyFillArc: gc={gc_id:#x} func={} fg_raw={:#x} fg_mapped={fg:#x} draw={drawable:#x} fill_style={}", gc.function, gc.foreground, gc.fill_style);

    // Extract tile/stipple data before borrowing framebuffer
    let tile_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 1 {
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
    let stipple_data: Option<(Vec<u8>, u32, u32)> = if gc.fill_style == 2 || gc.fill_style == 3 {
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
            match gc.fill_style {
                1 => {
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
                2 | 3 => {
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
                            gc.fill_style == 3,
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
