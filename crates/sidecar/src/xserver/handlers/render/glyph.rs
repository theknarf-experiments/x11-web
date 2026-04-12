use std::collections::HashMap;
use tracing::debug;

use crate::xserver::ClientState;
use crate::xserver::core::{read_u16_bo, read_u32_bo, read_i16_bo};
use super::{
    PICTFORMAT_ARGB32, PICTFORMAT_A8, PICTFORMAT_A1,
    pict_format_has_alpha, pad4,
    composite_pixel, ClipSnapshot, resolve_source_color,
    GlyphSetState, StoredGlyph,
};

pub(crate) fn handle_create_glyphset(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let gsid = read_u32_bo(data, 4, bo);
    let format_id = read_u32_bo(data, 8, bo);

    debug!("Render CreateGlyphSet: gsid={gsid:#x} format={format_id:#x}");

    state.render.glyphsets.insert(
        gsid,
        GlyphSetState {
            format_id,
            glyphs: HashMap::new(),
        },
    );
    Vec::new()
}

pub(crate) fn handle_free_glyphset(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let gsid = read_u32_bo(data, 4, bo);
    state.render.glyphsets.remove(&gsid);
    Vec::new()
}

/// ReferenceGlyphSet (RENDER minor opcode 18).
/// Creates a new glyphset that shares glyphs with an existing one.
pub(crate) fn handle_reference_glyphset(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let new_gsid = read_u32_bo(data, 4, bo);
    let existing_gsid = read_u32_bo(data, 8, bo);

    debug!("Render ReferenceGlyphSet: new={new_gsid:#x} existing={existing_gsid:#x}");

    // Clone the existing glyphset
    if let Some(existing) = state.render.glyphsets.get(&existing_gsid) {
        let cloned = GlyphSetState {
            format_id: existing.format_id,
            glyphs: existing.glyphs.clone(),
        };
        state.render.glyphsets.insert(new_gsid, cloned);
    } else {
        // Existing not found - create empty with default format
        state.render.glyphsets.insert(
            new_gsid,
            GlyphSetState {
                format_id: PICTFORMAT_A8,
                glyphs: HashMap::new(),
            },
        );
    }
    Vec::new()
}

pub(crate) fn handle_add_glyphs(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    let gsid = read_u32_bo(data, 4, bo);
    let num_glyphs = read_u32_bo(data, 8, bo) as usize;

    debug!("Render AddGlyphs: gsid={gsid:#x} num={num_glyphs}");

    if data.len() < 12 + num_glyphs * 4 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    // Read glyph IDs
    let mut glyph_ids = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        glyph_ids.push(read_u32_bo(data, 12 + i * 4, bo));
    }

    let info_start = 12 + num_glyphs * 4;
    if data.len() < info_start + num_glyphs * 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    // Read GlyphInfo entries (12 bytes each)
    let mut glyph_infos = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        let off = info_start + i * 12;
        let width = read_u16_bo(data, off, bo);
        let height = read_u16_bo(data, off + 2, bo);
        let x = read_i16_bo(data, off + 4, bo);
        let y = read_i16_bo(data, off + 6, bo);
        let x_off = read_i16_bo(data, off + 8, bo);
        let y_off = read_i16_bo(data, off + 10, bo);
        glyph_infos.push((width, height, x, y, x_off, y_off));
    }

    let pixel_start = info_start + num_glyphs * 12;

    // Determine the format to know how to read pixel data
    let format_id = state.render.glyphsets.get(&gsid).map(|gs| gs.format_id);

    let mut pixel_off = pixel_start;
    let glyphs_to_store: Vec<(u32, StoredGlyph)> = glyph_ids
        .iter()
        .zip(glyph_infos.iter())
        .map(|(&gid, &(width, height, x, y, x_off, y_off))| {
            let glyph_data = if width > 0 && height > 0 {
                match format_id {
                    Some(fmt) if fmt == PICTFORMAT_A8 => {
                        // A8: each row padded to 4 bytes
                        let row_bytes = pad4(width as usize);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_A1 => {
                        // A1: each row padded to 4 bytes (in bits)
                        let row_bytes = pad4((width as usize + 7) / 8);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_ARGB32 => {
                        // ARGB32: 4 bytes per pixel, rows padded to 4 bytes
                        let row_bytes = width as usize * 4;
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    _ => {
                        // Default to A8
                        let row_bytes = pad4(width as usize);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= data.len() {
                            data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                }
            } else {
                Vec::new()
            };

            (
                gid,
                StoredGlyph {
                    width,
                    height,
                    x,
                    y,
                    x_off,
                    y_off,
                    data: glyph_data,
                },
            )
        })
        .collect();

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for (gid, glyph) in glyphs_to_store {
            gs.glyphs.insert(gid, glyph);
        }
    }

    Vec::new()
}

/// AddGlyphsFromPicture (RENDER minor opcode 21).
///
/// Like AddGlyphs but reads pixel data from a Picture resource at a
/// given (x, y) position rather than from inline data in the request.
/// Each glyph's pixel region is extracted from the source picture's
/// drawable at the offset computed from the glyph metrics.
///
/// Wire format:
///   4-7:  glyphset (GLYPHSET)
///   8-11: src_picture (PICTURE)
///   12-15: num_glyphs (CARD32)
///   Then for each glyph:
///     glyph_id (CARD32)
///   Then for each glyph:
///     GlyphInfo (12 bytes): width(2), height(2), x(2), y(2), x_off(2), y_off(2)
pub(crate) fn handle_add_glyphs_from_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    if data.len() < 16 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    let gsid = read_u32_bo(data, 4, bo);
    let src_picture = read_u32_bo(data, 8, bo);
    let num_glyphs = read_u32_bo(data, 12, bo) as usize;

    debug!("Render AddGlyphsFromPicture: gsid={gsid:#x} src={src_picture:#x} num={num_glyphs}");

    if data.len() < 16 + num_glyphs * 4 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    // Read glyph IDs
    let mut glyph_ids = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        glyph_ids.push(read_u32_bo(data, 16 + i * 4, bo));
    }

    let info_start = 16 + num_glyphs * 4;
    if data.len() < info_start + num_glyphs * 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    // Read GlyphInfo entries (12 bytes each)
    let mut glyph_infos = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        let off = info_start + i * 12;
        let width = read_u16_bo(data, off, bo);
        let height = read_u16_bo(data, off + 2, bo);
        let x = read_i16_bo(data, off + 4, bo);
        let y = read_i16_bo(data, off + 6, bo);
        let x_off = read_i16_bo(data, off + 8, bo);
        let y_off = read_i16_bo(data, off + 10, bo);
        glyph_infos.push((width, height, x, y, x_off, y_off));
    }

    // Resolve the source picture's drawable to extract pixel data.
    // The picture must reference a drawable (window or pixmap).
    let src_drawable = state.render.pictures.get(&src_picture).and_then(|p| {
        let did = p.drawable;
        // Get the framebuffer data from the drawable
        if let Some(px) = state.pixmaps.get(&did) {
            Some((px.framebuffer.data().to_vec(), px.framebuffer.width() as usize))
        } else if let Some(win) = state.windows.get(&did) {
            Some((win.framebuffer.data().to_vec(), win.framebuffer.width() as usize))
        } else {
            None
        }
    });

    let format_id = state.render.glyphsets.get(&gsid).map(|gs| gs.format_id);

    // Extract each glyph's pixels from the source drawable.
    // Glyphs are laid out sequentially in the source picture: glyph i
    // starts at (sum of previous widths, 0) unless the metrics say otherwise.
    let mut src_x_cursor: usize = 0;
    let mut glyphs_to_store: Vec<(u32, StoredGlyph)> = Vec::with_capacity(num_glyphs);

    for (_idx, (&gid, &(width, height, x, y, x_off, y_off))) in
        glyph_ids.iter().zip(glyph_infos.iter()).enumerate()
    {
        let glyph_data = if width > 0 && height > 0 {
            if let Some((ref fb_data, fb_stride)) = src_drawable {
                let bpp = 4usize; // BGRA framebuffer
                let mut pixels = Vec::new();

                // Determine bytes-per-pixel for the glyph format
                match format_id {
                    Some(fmt) if fmt == PICTFORMAT_A8 => {
                        let row_bytes = pad4(width as usize);
                        for row in 0..height as usize {
                            let sy = row;
                            for col in 0..width as usize {
                                let sx = src_x_cursor + col;
                                let fb_off = sy * fb_stride * bpp + sx * bpp;
                                // Extract alpha byte from BGRA
                                let alpha = if fb_off + 3 < fb_data.len() {
                                    fb_data[fb_off + 3]
                                } else {
                                    0
                                };
                                pixels.push(alpha);
                            }
                            // Pad row to 4 bytes
                            let pad = row_bytes - width as usize;
                            pixels.extend(std::iter::repeat(0u8).take(pad));
                        }
                    }
                    Some(fmt) if fmt == PICTFORMAT_ARGB32 => {
                        for row in 0..height as usize {
                            let sy = row;
                            for col in 0..width as usize {
                                let sx = src_x_cursor + col;
                                let fb_off = sy * fb_stride * bpp + sx * bpp;
                                if fb_off + 4 <= fb_data.len() {
                                    pixels.extend_from_slice(&fb_data[fb_off..fb_off + 4]);
                                } else {
                                    pixels.extend_from_slice(&[0, 0, 0, 0]);
                                }
                            }
                        }
                    }
                    _ => {
                        // Default to A8
                        let row_bytes = pad4(width as usize);
                        for row in 0..height as usize {
                            let sy = row;
                            for col in 0..width as usize {
                                let sx = src_x_cursor + col;
                                let fb_off = sy * fb_stride * bpp + sx * bpp;
                                let alpha = if fb_off + 3 < fb_data.len() {
                                    fb_data[fb_off + 3]
                                } else {
                                    0
                                };
                                pixels.push(alpha);
                            }
                            let pad = row_bytes - width as usize;
                            pixels.extend(std::iter::repeat(0u8).take(pad));
                        }
                    }
                }
                pixels
            } else {
                // Source picture not found or has no drawable - store empty
                Vec::new()
            }
        } else {
            Vec::new()
        };

        src_x_cursor += width as usize;

        glyphs_to_store.push((
            gid,
            StoredGlyph {
                width,
                height,
                x,
                y,
                x_off,
                y_off,
                data: glyph_data,
            },
        ));
    }

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for (gid, glyph) in glyphs_to_store {
            gs.glyphs.insert(gid, glyph);
        }
    }

    Vec::new()
}

pub(crate) fn handle_free_glyphs(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let gsid = read_u32_bo(data, 4, bo);
    let num_glyphs = (data.len() - 8) / 4;

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for i in 0..num_glyphs {
            let gid = read_u32_bo(data, 8 + i * 4, bo);
            gs.glyphs.remove(&gid);
        }
    }
    Vec::new()
}

/// Handle CompositeGlyphs8/16/32
pub(crate) fn handle_composite_glyphs(state: &mut ClientState, data: &[u8], glyph_id_size: usize, seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    if data.len() < 28 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, minor, bo,
        );
    }

    let pict_op = data[4];
    let src_pic = read_u32_bo(data, 8, bo);
    let dst_pic = read_u32_bo(data, 12, bo);
    let _mask_format = read_u32_bo(data, 16, bo);
    let mut current_gsid = read_u32_bo(data, 20, bo);
    let _src_x = read_i16_bo(data, 24, bo);
    let _src_y = read_i16_bo(data, 26, bo);

    debug!(
        "Render CompositeGlyphs{}: op={pict_op} src={src_pic:#x} dst={dst_pic:#x} gs={current_gsid:#x}",
        glyph_id_size * 8
    );

    // Resolve source color (typically solid fill for text)
    let src_color = resolve_source_color(state, src_pic);

    // Resolve dst drawable + format.
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

    // Parse glyphcmds
    let mut off = 28;
    let mut pen_x: i32 = 0;
    let mut pen_y: i32 = 0;
    let mut first_element = true;

    // Collect all glyph render operations first, then apply
    struct GlyphOp {
        dst_x: i32,
        dst_y: i32,
        width: u16,
        height: u16,
        alpha_data: Vec<u8>,
        format_id: u32,
    }
    let mut ops: Vec<GlyphOp> = Vec::new();

    while off < data.len() {
        if off >= data.len() {
            break;
        }
        let len = data[off] as usize;

        if len == 0 {
            break;
        }

        if len == 255 {
            // Glyphset switch
            if off + 8 <= data.len() {
                current_gsid = read_u32_bo(data, off + 4, bo);
                off = pad4(off + 8);
            } else {
                break;
            }
            continue;
        }

        // Regular glyph element
        if off + 8 > data.len() {
            break;
        }
        // bytes 1..3 = padding
        let delta_x = read_i16_bo(data, off + 4, bo);
        let delta_y = read_i16_bo(data, off + 6, bo);

        if first_element {
            pen_x = delta_x as i32;
            pen_y = delta_y as i32;
            first_element = false;
        } else {
            pen_x += delta_x as i32;
            pen_y += delta_y as i32;
        }

        let glyph_data_start = off + 8;
        let glyph_data_bytes = len * glyph_id_size;
        if glyph_data_start + glyph_data_bytes > data.len() {
            break;
        }

        // Read glyph IDs
        let mut glyph_ids = Vec::with_capacity(len);
        for i in 0..len {
            let gid_off = glyph_data_start + i * glyph_id_size;
            let gid = match glyph_id_size {
                1 => data[gid_off] as u32,
                2 => read_u16_bo(data, gid_off, bo) as u32,
                4 => read_u32_bo(data, gid_off, bo),
                _ => 0,
            };
            glyph_ids.push(gid);
        }

        off = pad4(glyph_data_start + glyph_data_bytes);

        // Look up glyphs and create render operations
        if let Some(gs) = state.render.glyphsets.get(&current_gsid) {
            let format_id = gs.format_id;
            for gid in &glyph_ids {
                if let Some(glyph) = gs.glyphs.get(gid) {
                    if glyph.width > 0 && glyph.height > 0 {
                        ops.push(GlyphOp {
                            dst_x: pen_x - glyph.x as i32,
                            dst_y: pen_y - glyph.y as i32,
                            width: glyph.width,
                            height: glyph.height,
                            alpha_data: glyph.data.clone(),
                            format_id,
                        });
                    }
                    pen_x += glyph.x_off as i32;
                    pen_y += glyph.y_off as i32;
                }
            }
        }
    }

    // Now render all glyph operations to the framebuffer
    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;
        let fb_stride = fb.stride();
        let (sr, sg, sb, sa) = src_color;

        for op in &ops {
            let fb_data = fb.data_mut();
            for row in 0..op.height as i32 {
                let dy = op.dst_y + row;
                if dy < 0 || dy >= fb_h {
                    continue;
                }
                for col in 0..op.width as i32 {
                    let dx = op.dst_x + col;
                    if dx < 0 || dx >= fb_w {
                        continue;
                    }
                    if !clip.allows(dx, dy) {
                        continue;
                    }

                    let alpha = get_glyph_alpha(&op.alpha_data, op.width, col as u16, row as u16, op.format_id);
                    if alpha == 0 {
                        continue;
                    }

                    // Modulate source color by glyph alpha. Both
                    // source and result are premultiplied.
                    let eff_a = ((sa as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_r = ((sr as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_g = ((sg as u32 * alpha as u32 + 127) / 255) as u8;
                    let eff_b = ((sb as u32 * alpha as u32 + 127) / 255) as u8;

                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 < fb_data.len() {
                        composite_pixel(
                            pict_op,
                            &mut fb_data[dst_off..dst_off + 4],
                            eff_b,
                            eff_g,
                            eff_r,
                            eff_a,
                            dst_has_alpha,
                        );
                    }
                }
            }
        }

        // Mark dirty for each op
        for op in &ops {
            fb.mark_dirty(op.dst_x, op.dst_y, op.width as u32, op.height as u32);
        }
    }

    Vec::new()
}

/// Extract alpha value from glyph data at a given position
fn get_glyph_alpha(data: &[u8], width: u16, x: u16, y: u16, format_id: u32) -> u8 {
    match format_id {
        f if f == PICTFORMAT_A8 => {
            let row_bytes = pad4(width as usize);
            let off = y as usize * row_bytes + x as usize;
            if off < data.len() {
                data[off]
            } else {
                0
            }
        }
        f if f == PICTFORMAT_A1 => {
            let row_bytes = pad4((width as usize + 7) / 8);
            let byte_off = y as usize * row_bytes + (x as usize / 8);
            let bit_off = x as usize % 8;
            if byte_off < data.len() {
                // LSB first bit order
                if data[byte_off] & (1 << bit_off) != 0 {
                    255
                } else {
                    0
                }
            } else {
                0
            }
        }
        f if f == PICTFORMAT_ARGB32 => {
            let off = (y as usize * width as usize + x as usize) * 4;
            if off + 3 < data.len() {
                data[off + 3] // alpha channel
            } else {
                0
            }
        }
        _ => {
            // Default to A8
            let row_bytes = pad4(width as usize);
            let off = y as usize * row_bytes + x as usize;
            if off < data.len() {
                data[off]
            } else {
                0
            }
        }
    }
}
