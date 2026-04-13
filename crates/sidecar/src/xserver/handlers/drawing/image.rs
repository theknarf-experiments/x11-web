//! Image operations (opcodes 72-73).

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 72: PutImage
// ---------------------------------------------------------------------------

pub(crate) fn handle_put_image(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 24, state.sequence, 72);

    let format = data[1]; // 0=Bitmap, 1=XYPixmap, 2=ZPixmap
    let drawable = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 72, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 72, 0);
    }
    if format > 2 {
        return build_error(BAD_VALUE, state.sequence, format as u32, 72, 0);
    }
    let width = state.read_u16(data, 12);
    let height = state.read_u16(data, 14);
    let dst_x = state.read_i16(data, 16);
    let dst_y = state.read_i16(data, 18);
    let left_pad = data[20] as usize;
    let depth = data[21];

    // Validate image dimensions are within reasonable bounds
    if width > 32767 || height > 32767 {
        return build_error(BAD_VALUE, state.sequence, width as u32, 72, 0);
    }

    let pixel_data = &data[24..];

    // Validate that pixel data is present (at least 1 byte for non-zero images)
    if width > 0 && height > 0 && pixel_data.is_empty() {
        return build_error(BAD_LENGTH, state.sequence, 0, 72, 0);
    }

    debug!("PutImage: fmt={format} depth={depth} drawable={drawable:#x} {width}x{height} at ({dst_x},{dst_y}) data={}", pixel_data.len());

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    if format == 2 && depth >= 24 {
        // ZPixmap depth 24/32: direct BGRA/BGRX pixel data
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            fb.put_image_gc(dst_x, dst_y, width, height, pixel_data, gc.function, gc.plane_mask, &gc.clip_rects);
        }
    } else if format == 2 && depth == 16 {
        // ZPixmap depth 16: RGB565 packed pixels → BGRA32 framebuffer
        let gc_func = gc.function;
        let plane_mask = gc.plane_mask;
        let has_clip = !gc.clip_rects.is_empty();
        let clip_rects = gc.clip_rects.clone();
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            let w = width as usize;
            let h = height as usize;
            let fb_w = fb.width() as usize;
            let fb_h = fb.height() as usize;
            let row_bytes = w * 2;
            let padded_row = (row_bytes + 3) & !3;
            let fb_data = fb.data_mut();
            for row in 0..h {
                let dy = dst_y as i32 + row as i32;
                if dy < 0 || dy >= fb_h as i32 { continue; }
                for col in 0..w {
                    let dx = dst_x as i32 + col as i32;
                    if dx < 0 || dx >= fb_w as i32 { continue; }
                    if has_clip && !should_draw_pixel(dx, dy, &clip_rects) { continue; }
                    let src_off = row * padded_row + col * 2;
                    if src_off + 1 >= pixel_data.len() { continue; }
                    let val = u16::from_le_bytes([pixel_data[src_off], pixel_data[src_off + 1]]);
                    let r = ((val >> 11) & 0x1F) as u8;
                    let g = ((val >> 5) & 0x3F) as u8;
                    let b = (val & 0x1F) as u8;
                    let src_color = ((((r << 3) | (r >> 2)) as u32) << 16)
                        | ((((g << 2) | (g >> 4)) as u32) << 8)
                        | (((b << 3) | (b >> 2)) as u32);
                    let fb_off = (dy as usize * fb_w + dx as usize) * 4;
                    if fb_off + 3 < fb_data.len() {
                        let dst_color = (fb_data[fb_off + 2] as u32) << 16
                            | (fb_data[fb_off + 1] as u32) << 8
                            | fb_data[fb_off] as u32;
                        let result = apply_rop(gc_func, src_color, dst_color);
                        let masked = (result & plane_mask) | (dst_color & !plane_mask);
                        fb_data[fb_off] = (masked & 0xFF) as u8;
                        fb_data[fb_off + 1] = ((masked >> 8) & 0xFF) as u8;
                        fb_data[fb_off + 2] = ((masked >> 16) & 0xFF) as u8;
                        fb_data[fb_off + 3] = 0xFF;
                    }
                }
            }
        }
    } else if format == 2 && depth == 8 {
        // ZPixmap depth 8: 8-bit pixel indices → look up via colormap for display.
        // Store the colormap-resolved RGB in B/G/R channels for rendering, and
        // preserve the original colormap index in the A channel so that GetImage
        // can return the correct palette index (not the resolved RGB).
        let drawable_cmap = state.windows.get(&drawable)
            .and_then(|w| if w.colormap != 0 { Some(w.colormap) } else { None })
            .unwrap_or(ROOT_COLORMAP);
        // Pre-build a 256-entry lookup table from the colormap
        let mut lut = [[0u8; 4]; 256];
        if let Some(cmap) = state.colormaps.get(&drawable_cmap) {
            for i in 0..256u32 {
                let (r16, g16, b16) = cmap.lookup(i);
                // [B, G, R, index] — alpha channel stores the original index
                lut[i as usize] = [(b16 >> 8) as u8, (g16 >> 8) as u8, (r16 >> 8) as u8, i as u8];
            }
        } else {
            // Fallback: treat index as grayscale, store index in alpha
            for i in 0..256u32 {
                let v = i as u8;
                lut[i as usize] = [v, v, v, i as u8];
            }
        }

        let gc_func = gc.function;
        let plane_mask = gc.plane_mask;
        let has_clip = !gc.clip_rects.is_empty();
        let clip_rects = gc.clip_rects.clone();
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            let w = width as usize;
            let h = height as usize;
            let fb_w = fb.width() as usize;
            let fb_h = fb.height() as usize;
            let padded_row = (w + 3) & !3;
            let fb_data = fb.data_mut();
            for row in 0..h {
                let dy = dst_y as i32 + row as i32;
                if dy < 0 || dy >= fb_h as i32 { continue; }
                for col in 0..w {
                    let dx = dst_x as i32 + col as i32;
                    if dx < 0 || dx >= fb_w as i32 { continue; }
                    if has_clip && !should_draw_pixel(dx, dy, &clip_rects) { continue; }
                    let src_off = row * padded_row + col;
                    if src_off >= pixel_data.len() { continue; }
                    let idx = pixel_data[src_off] as usize;
                    let pixel = &lut[idx];
                    let src_color = (pixel[2] as u32) << 16 | (pixel[1] as u32) << 8 | pixel[0] as u32;
                    let fb_off = (dy as usize * fb_w + dx as usize) * 4;
                    if fb_off + 3 < fb_data.len() {
                        let dst_color = (fb_data[fb_off + 2] as u32) << 16
                            | (fb_data[fb_off + 1] as u32) << 8
                            | fb_data[fb_off] as u32;
                        let result = apply_rop(gc_func, src_color, dst_color);
                        let masked = (result & plane_mask) | (dst_color & !plane_mask);
                        fb_data[fb_off] = (masked & 0xFF) as u8;
                        fb_data[fb_off + 1] = ((masked >> 8) & 0xFF) as u8;
                        fb_data[fb_off + 2] = ((masked >> 16) & 0xFF) as u8;
                        // Preserve colormap index in alpha channel for GetImage
                        fb_data[fb_off + 3] = pixel[3];
                    }
                }
            }
        }
    } else if format == 2 && depth == 4 {
        // ZPixmap depth 4: nibble-packed pixels
        let gc_func = gc.function;
        let plane_mask = gc.plane_mask;
        let has_clip = !gc.clip_rects.is_empty();
        let clip_rects = gc.clip_rects.clone();
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            let w = width as usize;
            let h = height as usize;
            let fb_w = fb.width() as usize;
            let fb_h = fb.height() as usize;
            let row_bytes = w.div_ceil(2);
            let padded_row = (row_bytes + 3) & !3;
            let fb_data = fb.data_mut();
            for row in 0..h {
                let dy = dst_y as i32 + row as i32;
                if dy < 0 || dy >= fb_h as i32 { continue; }
                for col in 0..w {
                    let dx = dst_x as i32 + col as i32;
                    if dx < 0 || dx >= fb_w as i32 { continue; }
                    if has_clip && !should_draw_pixel(dx, dy, &clip_rects) { continue; }
                    let byte_off = row * padded_row + col / 2;
                    if byte_off >= pixel_data.len() { continue; }
                    let nibble = if col % 2 == 0 {
                        pixel_data[byte_off] & 0x0F
                    } else {
                        (pixel_data[byte_off] >> 4) & 0x0F
                    };
                    // Scale 4-bit to 8-bit grayscale
                    let v = nibble | (nibble << 4);
                    let src_color = (v as u32) << 16 | (v as u32) << 8 | v as u32;
                    let fb_off = (dy as usize * fb_w + dx as usize) * 4;
                    if fb_off + 3 < fb_data.len() {
                        let dst_color = (fb_data[fb_off + 2] as u32) << 16
                            | (fb_data[fb_off + 1] as u32) << 8
                            | fb_data[fb_off] as u32;
                        let result = apply_rop(gc_func, src_color, dst_color);
                        let masked = (result & plane_mask) | (dst_color & !plane_mask);
                        fb_data[fb_off] = (masked & 0xFF) as u8;
                        fb_data[fb_off + 1] = ((masked >> 8) & 0xFF) as u8;
                        fb_data[fb_off + 2] = ((masked >> 16) & 0xFF) as u8;
                        fb_data[fb_off + 3] = 0xFF;
                    }
                }
            }
        }
    } else if format == 2 && depth == 1 {
        // 1-bit depth ZPixmap: write bitmap data into the depth-1 pixmap
        // framebuffer. Each row is padded to 32-bit boundary; each byte
        // holds 8 pixels in LSB-first order.  We store 1-bit values as
        // 0x00000000 (zero) or 0x00FFFFFF (one) in the ARGB32 framebuffer
        // so that CreateCursor can read them back.
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            let fb_w = fb.width() as usize;
            let fb_h = fb.height() as usize;
            let w = width as usize;
            let h = height as usize;
            let dx = dst_x as usize;
            let dy = dst_y as usize;
            let row_bytes = w.div_ceil(8);
            let padded_row = (row_bytes + 3) & !3;
            let fb_data = fb.data_mut();
            for row in 0..h {
                if dy + row >= fb_h { break; }
                let src_row_start = row * padded_row;
                for col in 0..w {
                    if dx + col >= fb_w { break; }
                    let byte_idx = src_row_start + col / 8;
                    let bit_idx = col % 8;
                    let bit = if byte_idx < pixel_data.len() {
                        (pixel_data[byte_idx] >> bit_idx) & 1
                    } else {
                        0
                    };
                    let fb_off = ((dy + row) * fb_w + (dx + col)) * 4;
                    if fb_off + 3 < fb_data.len() {
                        let val = if bit != 0 { 0xFF } else { 0x00 };
                        fb_data[fb_off] = val;     // B
                        fb_data[fb_off + 1] = val; // G
                        fb_data[fb_off + 2] = val; // R
                        fb_data[fb_off + 3] = 0;   // A (unused, cursor reads RGB only)
                    }
                }
            }
        }
    } else if format == 0 || format == 1 {
        let mapped_fg = state.map_color_for_drawable(drawable, gc.foreground);
        let mapped_bg = state.map_color_for_drawable(drawable, gc.background);
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            let w = width as usize;
            let h = height as usize;
            let fb_w = fb.width() as usize;
            let fb_h = fb.height() as usize;
            // Each scanline is (left_pad + w) bits, rounded up to 32-bit boundary
            let scanline_bits = left_pad + w;
            let scanline_bytes = scanline_bits.div_ceil(8);
            let padded_scanline = (scanline_bytes + 3) & !3;

            if format == 0 {
                let fg = mapped_fg & gc.plane_mask;
                let bg = mapped_bg & gc.plane_mask;
                let has_clip = !gc.clip_rects.is_empty();
                let gc_func = gc.function;
                let plane_mask = gc.plane_mask;
                let fb_data = fb.data_mut();
                for row in 0..h {
                    let dy = dst_y as i32 + row as i32;
                    if dy < 0 || dy >= fb_h as i32 { continue; }
                    for col in 0..w {
                        let dx = dst_x as i32 + col as i32;
                        if dx < 0 || dx >= fb_w as i32 { continue; }
                        if has_clip && !should_draw_pixel(dx, dy, &gc.clip_rects) { continue; }
                        let bit_pos = left_pad + col;
                        let byte_idx = row * padded_scanline + bit_pos / 8;
                        let bit = if byte_idx < pixel_data.len() {
                            (pixel_data[byte_idx] >> (7 - (bit_pos % 8))) & 1
                        } else {
                            0
                        };
                        let src_color = if bit != 0 { fg } else { bg };
                        let fb_off = (dy as usize * fb_w + dx as usize) * 4;
                        if fb_off + 3 < fb_data.len() {
                            if gc_func == 3 && plane_mask == 0xFFFFFFFF {
                                fb_data[fb_off] = (src_color & 0xFF) as u8;
                                fb_data[fb_off + 1] = ((src_color >> 8) & 0xFF) as u8;
                                fb_data[fb_off + 2] = ((src_color >> 16) & 0xFF) as u8;
                                fb_data[fb_off + 3] = 0xFF;
                            } else {
                                let dst_color = (fb_data[fb_off + 2] as u32) << 16
                                    | (fb_data[fb_off + 1] as u32) << 8
                                    | fb_data[fb_off] as u32;
                                let result = (apply_rop(gc_func, src_color, dst_color) & plane_mask)
                                    | (dst_color & !plane_mask);
                                fb_data[fb_off] = (result & 0xFF) as u8;
                                fb_data[fb_off + 1] = ((result >> 8) & 0xFF) as u8;
                                fb_data[fb_off + 2] = ((result >> 16) & 0xFF) as u8;
                                fb_data[fb_off + 3] = 0xFF;
                            }
                        }
                    }
                }
            } else {
                // XYPixmap: one bitmap plane per depth bit, MSB plane first
                let num_planes = depth as usize;
                let plane_size = padded_scanline * h;
                let has_clip = !gc.clip_rects.is_empty();
                let gc_func = gc.function;
                let plane_mask = gc.plane_mask;
                let fb_data = fb.data_mut();
                for row in 0..h {
                    let dy = dst_y as i32 + row as i32;
                    if dy < 0 || dy >= fb_h as i32 { continue; }
                    for col in 0..w {
                        let dx = dst_x as i32 + col as i32;
                        if dx < 0 || dx >= fb_w as i32 { continue; }
                        if has_clip && !should_draw_pixel(dx, dy, &gc.clip_rects) { continue; }
                        let bit_pos = left_pad + col;
                        let fb_off = (dy as usize * fb_w + dx as usize) * 4;
                        if fb_off + 3 >= fb_data.len() { continue; }
                        let dst_color = (fb_data[fb_off + 2] as u32) << 16
                            | (fb_data[fb_off + 1] as u32) << 8
                            | fb_data[fb_off] as u32;
                        let mut pixel_val = dst_color;
                        for plane in 0..num_planes {
                            let plane_bit = 1u32 << plane;
                            if plane_mask & plane_bit == 0 { continue; }
                            let plane_offset = (num_planes - 1 - plane) * plane_size;
                            let byte_idx = plane_offset + row * padded_scanline + bit_pos / 8;
                            if byte_idx < pixel_data.len() {
                                let bit = (pixel_data[byte_idx] >> (7 - (bit_pos % 8))) & 1;
                                if bit != 0 {
                                    pixel_val |= plane_bit;
                                } else {
                                    pixel_val &= !plane_bit;
                                }
                            }
                        }
                        let result = if gc_func == 3 {
                            pixel_val
                        } else {
                            (apply_rop(gc_func, pixel_val, dst_color) & plane_mask)
                                | (dst_color & !plane_mask)
                        };
                        fb_data[fb_off] = (result & 0xFF) as u8;
                        fb_data[fb_off + 1] = ((result >> 8) & 0xFF) as u8;
                        fb_data[fb_off + 2] = ((result >> 16) & 0xFF) as u8;
                        fb_data[fb_off + 3] = 0xFF;
                    }
                }
            }
        }
    } else {
        debug!(
            "PutImage: unsupported format={format} depth={depth} {}x{} data_len={}",
            width,
            height,
            pixel_data.len()
        );
    }
    state.notify_damage(drawable, dst_x, dst_y, width, height);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 73: GetImage
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_image(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 20, seq, 73);

    let format = data[1]; // 1=XYPixmap, 2=ZPixmap
    let drawable = state.read_u32(data, 4);

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, seq, drawable, 73, 0);
    }
    if format != 1 && format != 2 {
        return build_error(BAD_VALUE, seq, format as u32, 73, 0);
    }
    let x = state.read_i16(data, 8);
    let y = state.read_i16(data, 10);
    let width = state.read_u16(data, 12);
    let height = state.read_u16(data, 14);
    let plane_mask = state.read_u32(data, 16);

    // Sync SHM pixmaps before reading
    state.sync_shm_pixmap(drawable);

    let depth: u8 = state
        .pixmaps
        .get(&drawable)
        .map(|p| p.depth)
        .unwrap_or(24);

    let visual = if state.pixmaps.contains_key(&drawable) {
        0u32
    } else {
        ROOT_VISUAL
    };

    // Per X11 spec, GetImage on a window returns the screen contents at
    // that location, which includes composited child window content.
    let is_window = state.windows.contains_key(&drawable);
    let pixels = if is_window {
        state.extract_pixels_include_inferiors(drawable, x, y, width, height)
    } else if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.extract_pixels(x, y, width, height)
    } else {
        vec![0u8; width as usize * height as usize * 4]
    };

    let w = width as usize;
    let h = height as usize;

    let image_data = match format {
        2 => {
            // ZPixmap: packed pixel data, apply plane_mask to each pixel
            let bpp: usize = if depth <= 8 { 1 } else if depth <= 16 { 2 } else { 4 };
            let row_bytes = w * bpp;
            let padded_row = (row_bytes + 3) & !3;
            let mut out = vec![0u8; padded_row * h];
            for row in 0..h {
                let src_row_start = row * w * 4;
                let dst_row_start = row * padded_row;
                for col in 0..w {
                    let src_off = src_row_start + col * 4;
                    let dst_off = dst_row_start + col * bpp;
                    if src_off + 4 <= pixels.len() && dst_off + bpp <= out.len() {
                        if bpp == 4 {
                            let pixel_val = u32::from_le_bytes([
                                pixels[src_off],
                                pixels[src_off + 1],
                                pixels[src_off + 2],
                                pixels[src_off + 3],
                            ]);
                            let masked = (pixel_val & plane_mask).to_le_bytes();
                            out[dst_off..dst_off + 4].copy_from_slice(&masked);
                        } else if bpp == 2 {
                            let b = pixels[src_off] as u16;
                            let g = pixels[src_off + 1] as u16;
                            let r = pixels[src_off + 2] as u16;
                            let val = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                            let masked = val & (plane_mask as u16);
                            out[dst_off..dst_off + 2].copy_from_slice(&masked.to_le_bytes());
                        } else {
                            // For depth 8 colormapped visuals, the original
                            // palette index is stored in the A channel (offset +3).
                            out[dst_off] = pixels[src_off + 3] & (plane_mask as u8);
                        }
                    }
                }
            }
            out
        }
        1 => {
            // XYPixmap: planar format, one bitmap per plane in plane_mask, MSB plane first.
            // Only planes with bits set in plane_mask are included in the output.
            let scanline_bytes = w.div_ceil(8);
            let padded_scanline = (scanline_bytes + 3) & !3;
            let plane_size = padded_scanline * h;
            // Collect which planes are active (in descending order for MSB-first output)
            let mut active_planes: Vec<usize> = Vec::new();
            for plane in (0..depth as usize).rev() {
                if plane_mask & (1u32 << plane) != 0 {
                    active_planes.push(plane);
                }
            }
            let num_active = active_planes.len();
            let mut out = vec![0u8; plane_size * num_active];
            for (out_idx, &plane) in active_planes.iter().enumerate() {
                let plane_offset = out_idx * plane_size;
                for row in 0..h {
                    let row_offset = plane_offset + row * padded_scanline;
                    for col in 0..w {
                        let src_off = (row * w + col) * 4;
                        if src_off + 4 <= pixels.len() {
                            let pixel_val = u32::from_le_bytes([
                                pixels[src_off],
                                pixels[src_off + 1],
                                pixels[src_off + 2],
                                pixels[src_off + 3],
                            ]);
                            if (pixel_val >> plane) & 1 != 0 {
                                let byte_idx = row_offset + col / 8;
                                if byte_idx < out.len() {
                                    out[byte_idx] |= 1 << (7 - (col % 8));
                                }
                            }
                        }
                    }
                }
            }
            out
        }
        _ => {
            let scanline_bytes = w.div_ceil(8);
            let padded_scanline = (scanline_bytes + 3) & !3;
            let mut out = vec![0u8; padded_scanline * h];
            for row in 0..h {
                let row_offset = row * padded_scanline;
                for col in 0..w {
                    let src_off = (row * w + col) * 4;
                    if src_off + 4 <= pixels.len() {
                        let pixel_val = u32::from_le_bytes([
                            pixels[src_off],
                            pixels[src_off + 1],
                            pixels[src_off + 2],
                            pixels[src_off + 3],
                        ]);
                        if pixel_val & 1 != 0 {
                            let byte_idx = row_offset + col / 8;
                            if byte_idx < out.len() {
                                out[byte_idx] |= 1 << (7 - (col % 8));
                            }
                        }
                    }
                }
            }
            out
        }
    };

    let data_len = image_data.len();
    let length_field = (data_len / 4) as u32;

    let mut reply = vec![0u8; 32 + data_len];
    reply[0] = 1; // Reply
    reply[1] = if format == 0 { 1 } else { depth };
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field);
    state.write_u32(&mut reply, 8, visual);
    reply[32..32 + data_len].copy_from_slice(&image_data);

    reply
}
