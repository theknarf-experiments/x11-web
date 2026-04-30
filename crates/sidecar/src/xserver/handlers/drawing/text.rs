//! Text operations (opcodes 74-77).

use super::*;
use x11rb_protocol::protocol::xproto::{
    ImageText16Request, ImageText8Request, PolyText16Request, PolyText8Request,
};

// ---------------------------------------------------------------------------
// Opcode 74: PolyText8
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_text8(state: &mut ClientState, req: &PolyText8Request) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 74, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 74, 0);
    }

    let mut cursor_x = req.x;
    let y = req.y;
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    if font.is_none() {
        return Vec::new();
    }

    // Collect text items first to avoid borrow issues
    let mut items: Vec<(i16, i16, u16, u16, Vec<u8>)> = Vec::new();
    let items_data = &*req.items;
    let mut offset = 0;
    let end = items_data.len();

    while offset < end {
        let item_len = items_data[offset] as usize;

        if item_len == 255 {
            // Font-switch item: length=255, then 4-byte font ID.
            // Per X11 spec, change the active font for subsequent text items.
            if offset + 5 <= end {
                let new_font_id = state.read_u32(items_data, offset + 1);
                if let Some(f) = state.font_manager.get_font(new_font_id) {
                    font = Some(f);
                }
            }
            offset += 5;
            continue;
        }
        if item_len == 0 {
            break;
        }
        if offset + 2 + item_len > end {
            break;
        }

        let delta = items_data[offset + 1] as i8;
        cursor_x += delta as i16;

        if let Some(ref f) = font {
            let text = &items_data[offset + 2..offset + 2 + item_len];
            let (img_w, img_h, pixels) = f.render_text_transparent(text, gc.foreground);

            if img_w > 0 && img_h > 0 {
                items.push((cursor_x, y - f.font_ascent, img_w, img_h, pixels));
            }

            let mut text_advance: i32 = 0;
            for &ch in text {
                text_advance += f.char_info(ch as u16).character_width as i32;
            }
            cursor_x += text_advance as i16;
        }
        offset += 2 + item_len;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, w, h, ref pixels) in &items {
            fb.put_image_over_gc(
                x,
                y,
                w,
                h,
                pixels,
                gc.function,
                gc.plane_mask,
                &gc.clip_rects,
            );
        }
    }
    for &(x, y, w, h, _) in &items {
        state.notify_damage(drawable, x, y, w, h);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 75: PolyText16
// ---------------------------------------------------------------------------

pub(crate) fn handle_poly_text16(state: &mut ClientState, req: &PolyText16Request) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 75, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 75, 0);
    }

    let mut cursor_x = req.x;
    let y = req.y;
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    if font.is_none() {
        return Vec::new();
    }

    let mut items: Vec<(i16, i16, u16, u16, Vec<u8>)> = Vec::new();
    let items_data = &*req.items;
    let mut offset = 0;
    let end = items_data.len();

    while offset < end {
        let item_len = items_data[offset] as usize;

        if item_len == 255 {
            // Font-switch item: change active font for subsequent text items.
            if offset + 5 <= end {
                let new_font_id = state.read_u32(items_data, offset + 1);
                if let Some(f) = state.font_manager.get_font(new_font_id) {
                    font = Some(f);
                }
            }
            offset += 5;
            continue;
        }
        if item_len == 0 {
            break;
        }
        if offset + 2 + item_len * 2 > end {
            break;
        }

        let delta = items_data[offset + 1] as i8;
        cursor_x += delta as i16;

        if let Some(ref font) = font {
            // Extract 2-byte character codes (big-endian per X11 spec)
            let mut char_codes: Vec<u16> = Vec::with_capacity(item_len);
            for i in 0..item_len {
                let char_offset = offset + 2 + i * 2;
                let hi = items_data[char_offset] as u16;
                let lo = items_data[char_offset + 1] as u16;
                char_codes.push((hi << 8) | lo);
            }

            // Render each character, using on-demand glyph rendering for codepoints > 255
            let mut text_advance: i32 = 0;
            for &code in &char_codes {
                if code <= font.max_char {
                    text_advance += font.char_info(code).character_width as i32;
                } else if let Some((ci, _)) = font.render_extended_glyph(code as u32) {
                    text_advance += ci.character_width as i32;
                } else {
                    text_advance += font.char_info(font.default_char).character_width as i32;
                }
            }

            // For characters within the basic range, use the standard renderer
            let basic_chars: Vec<u8> = char_codes
                .iter()
                .filter(|&&c| c <= 255)
                .map(|&c| c as u8)
                .collect();
            let has_extended = char_codes.iter().any(|&c| c > 255);

            if !has_extended {
                // All characters in basic range — use optimized path
                let (img_w, img_h, pixels) =
                    font.render_text_transparent(&basic_chars, gc.foreground);
                if img_w > 0 && img_h > 0 {
                    items.push((cursor_x, y - font.font_ascent, img_w, img_h, pixels));
                }
            } else {
                // Mix of basic and extended characters — render individually
                let mut local_x = cursor_x;
                for &code in &char_codes {
                    let (ci, glyph_opt) = if code <= font.max_char {
                        let ci = font.char_info(code).clone();
                        let g = font.glyph(code).cloned();
                        (ci, g)
                    } else if let Some((ci, g)) = font.render_extended_glyph(code as u32) {
                        (ci, Some(g))
                    } else {
                        let ci = font.char_info(font.default_char).clone();
                        let g = font.glyph(font.default_char).cloned();
                        (ci, g)
                    };

                    if let Some(glyph) = glyph_opt {
                        if glyph.width > 0 && glyph.height > 0 {
                            let fg_r = ((gc.foreground >> 16) & 0xFF) as u8;
                            let fg_g = ((gc.foreground >> 8) & 0xFF) as u8;
                            let fg_b = (gc.foreground & 0xFF) as u8;
                            let gw = glyph.width as usize;
                            let gh = glyph.height as usize;
                            let row_bytes = gw.div_ceil(8);
                            let mut pixels = vec![0u8; gw * gh * 4];
                            for row in 0..gh {
                                for col in 0..gw {
                                    let bit = (glyph.bitmap[row * row_bytes + col / 8]
                                        >> (7 - (col % 8)))
                                        & 1;
                                    if bit != 0 {
                                        let idx = (row * gw + col) * 4;
                                        pixels[idx] = fg_b;
                                        pixels[idx + 1] = fg_g;
                                        pixels[idx + 2] = fg_r;
                                        pixels[idx + 3] = 0xFF;
                                    }
                                }
                            }
                            let gx = local_x + ci.left_side_bearing;
                            let gy = y - ci.ascent;
                            items.push((gx, gy, glyph.width, glyph.height, pixels));
                        }
                    }
                    local_x += ci.character_width;
                }
            }

            cursor_x += text_advance as i16;
        }
        offset += 2 + item_len * 2;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, w, h, ref pixels) in &items {
            fb.put_image_over_gc(
                x,
                y,
                w,
                h,
                pixels,
                gc.function,
                gc.plane_mask,
                &gc.clip_rects,
            );
        }
    }
    for &(x, y, w, h, _) in &items {
        state.notify_damage(drawable, x, y, w, h);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 76: ImageText8
// ---------------------------------------------------------------------------

pub(crate) fn handle_image_text8(state: &mut ClientState, req: &ImageText8Request) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 76, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 76, 0);
    }

    let x = req.x;
    let y = req.y;
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let text = &*req.string;

    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    let (img_w, img_h, pixels) = font.render_text(text, gc.foreground, gc.background);
    if img_w == 0 || img_h == 0 {
        return Vec::new();
    }

    let render_y = y - font.font_ascent;
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.put_image_gc(
            x,
            render_y,
            img_w,
            img_h,
            &pixels,
            gc.function,
            gc.plane_mask,
            &gc.clip_rects,
        );
    }
    state.notify_damage(drawable, x, render_y, img_w, img_h);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 77: ImageText16
// ---------------------------------------------------------------------------

pub(crate) fn handle_image_text16(state: &mut ClientState, req: &ImageText16Request) -> Vec<u8> {
    let drawable = req.drawable;
    let gc_id = req.gc;

    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 77, 0);
    }
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 77, 0);
    }

    let str_len = req.string.len();

    // Check if all characters are in the basic Latin-1 range
    let all_basic = req.string.iter().all(|c| c.byte1 == 0);

    if all_basic {
        // Optimization: all chars have high byte 0, delegate to ImageText8
        let bytes: Vec<u8> = req.string.iter().map(|c| c.byte2).collect();
        let req8 = ImageText8Request {
            drawable: req.drawable,
            gc: req.gc,
            x: req.x,
            y: req.y,
            string: std::borrow::Cow::Owned(bytes),
        };
        let _ = str_len;
        return handle_image_text8(state, &req8);
    }

    // Extended characters present — render with 2-byte codes
    let x = req.x;
    let y = req.y;
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    // Calculate total width for all characters
    let mut total_width: i32 = 0;
    let mut char_codes = Vec::with_capacity(str_len);
    for c in req.string.iter() {
        let code = ((c.byte1 as u16) << 8) | (c.byte2 as u16);
        char_codes.push(code);
        if code <= font.max_char {
            total_width += font.char_info(code).character_width as i32;
        } else if let Some((ci, _)) = font.render_extended_glyph(code as u32) {
            total_width += ci.character_width as i32;
        } else {
            total_width += font.char_info(font.default_char).character_width as i32;
        }
    }

    let total_width = total_width.max(1) as u16;
    let total_height = (font.font_ascent + font.font_descent) as u16;

    // Render with opaque background
    let fg_r = ((gc.foreground >> 16) & 0xFF) as u8;
    let fg_g = ((gc.foreground >> 8) & 0xFF) as u8;
    let fg_b = (gc.foreground & 0xFF) as u8;
    let bg_r = ((gc.background >> 16) & 0xFF) as u8;
    let bg_g = ((gc.background >> 8) & 0xFF) as u8;
    let bg_b = (gc.background & 0xFF) as u8;

    let mut pixels = vec![0u8; total_width as usize * total_height as usize * 4];
    // Fill background
    for i in 0..(total_width as usize * total_height as usize) {
        pixels[i * 4] = bg_b;
        pixels[i * 4 + 1] = bg_g;
        pixels[i * 4 + 2] = bg_r;
        pixels[i * 4 + 3] = 0xFF;
    }

    // Render each glyph
    let mut pen_x: i32 = 0;
    for &code in &char_codes {
        let (ci, glyph_opt) = if code <= font.max_char {
            let ci = font.char_info(code).clone();
            let g = font.glyph(code).cloned();
            (ci, g)
        } else if let Some((ci, g)) = font.render_extended_glyph(code as u32) {
            (ci, Some(g))
        } else {
            let ci = font.char_info(font.default_char).clone();
            let g = font.glyph(font.default_char).cloned();
            (ci, g)
        };

        if let Some(glyph) = glyph_opt {
            let gw = glyph.width as usize;
            let gh = glyph.height as usize;
            let row_bytes = gw.div_ceil(8);
            let gx = pen_x + ci.left_side_bearing as i32;
            let gy = font.font_ascent as i32 - ci.ascent as i32;
            for row in 0..gh {
                for col in 0..gw {
                    let bit = (glyph.bitmap[row * row_bytes + col / 8] >> (7 - (col % 8))) & 1;
                    if bit != 0 {
                        let px = gx + col as i32;
                        let py = gy + row as i32;
                        if px >= 0 && px < total_width as i32 && py >= 0 && py < total_height as i32
                        {
                            let idx = (py as usize * total_width as usize + px as usize) * 4;
                            pixels[idx] = fg_b;
                            pixels[idx + 1] = fg_g;
                            pixels[idx + 2] = fg_r;
                            pixels[idx + 3] = 0xFF;
                        }
                    }
                }
            }
        }
        pen_x += ci.character_width as i32;
    }

    let render_y = y - font.font_ascent;
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.put_image_gc(
            x,
            render_y,
            total_width,
            total_height,
            &pixels,
            gc.function,
            gc.plane_mask,
            &gc.clip_rects,
        );
    }
    state.notify_damage(drawable, x, render_y, total_width, total_height);

    Vec::new()
}
