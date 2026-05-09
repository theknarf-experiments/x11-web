use std::collections::HashMap;
use tracing::debug;

use super::super::parse_minor;
use super::{
    composite_pixel, composite_pixel_ca, pad4, pict_format_has_alpha, render_value_err,
    resolve_source_color, ClipSnapshot, GlyphSetState, StoredGlyph, PICTFORMAT_A1, PICTFORMAT_A8,
    PICTFORMAT_ARGB32,
};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_i16_bo, read_u16_bo, read_u32_bo};
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::{
    AddGlyphsRequest, CreateGlyphSetRequest, FreeGlyphSetRequest, FreeGlyphsRequest,
    ReferenceGlyphSetRequest,
};

pub(crate) fn handle_create_glyphset(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateGlyphSetRequest, data, state, seq, 139, data[1] as u16);

    let gsid = req.gsid;
    let format_id = req.format;

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
    let req = parse_minor!(FreeGlyphSetRequest, data, state, seq, 139, data[1] as u16);

    let gsid = req.glyphset;
    state.render.glyphsets.remove(&gsid);
    state.recycle_xid(gsid);
    Vec::new()
}

/// ReferenceGlyphSet (RENDER minor opcode 18).
/// Creates a new glyphset that shares glyphs with an existing one.
pub(crate) fn handle_reference_glyphset(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(
        ReferenceGlyphSetRequest,
        data,
        state,
        seq,
        139,
        data[1] as u16
    );

    let new_gsid = req.gsid;
    let existing_gsid = req.existing;

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
    let minor = data[1] as u16;
    let req = parse_minor!(AddGlyphsRequest, data, state, seq, 139, minor);

    let gsid = req.glyphset;
    let num_glyphs = req.glyphids.len();

    debug!("Render AddGlyphs: gsid={gsid:#x} num={num_glyphs}");

    // Determine the format to know how to read pixel data
    let format_id = state.render.glyphsets.get(&gsid).map(|gs| gs.format_id);

    // The x11rb parser gives us glyphids, glyphs (GlyphInfo), and raw data.
    // We need to slice the raw data according to each glyph's dimensions.
    let mut pixel_off = 0usize;
    let raw_data = &req.data;
    let glyphs_to_store: Vec<(u32, StoredGlyph)> = req
        .glyphids
        .iter()
        .zip(req.glyphs.iter())
        .map(|(&gid, gi)| {
            let width = gi.width;
            let height = gi.height;
            let x = gi.x;
            let y = gi.y;
            let x_off = gi.x_off;
            let y_off = gi.y_off;

            let glyph_data = if width > 0 && height > 0 {
                match format_id {
                    Some(fmt) if fmt == PICTFORMAT_A8 => {
                        let row_bytes = pad4(width as usize);
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= raw_data.len() {
                            raw_data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_A1 => {
                        let row_bytes = pad4((width as usize).div_ceil(8));
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= raw_data.len() {
                            raw_data[pixel_off..pixel_off + total].to_vec()
                        } else {
                            vec![0u8; total]
                        };
                        pixel_off += total;
                        d
                    }
                    Some(fmt) if fmt == PICTFORMAT_ARGB32 => {
                        let row_bytes = width as usize * 4;
                        let total = row_bytes * height as usize;
                        let d = if pixel_off + total <= raw_data.len() {
                            raw_data[pixel_off..pixel_off + total].to_vec()
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
                        let d = if pixel_off + total <= raw_data.len() {
                            raw_data[pixel_off..pixel_off + total].to_vec()
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
/// Note: There is no x11rb AddGlyphsFromPictureRequest struct, so we
/// keep manual parsing here.
pub(crate) fn handle_add_glyphs_from_picture(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 16, seq, 139, minor, bo);

    let gsid = read_u32_bo(data, 4, bo);
    let src_picture = read_u32_bo(data, 8, bo);
    let num_glyphs = read_u32_bo(data, 12, bo) as usize;

    debug!("Render AddGlyphsFromPicture: gsid={gsid:#x} src={src_picture:#x} num={num_glyphs}");

    require_len!(data, 16 + num_glyphs * 4, seq, 139, minor, bo);

    // Read glyph IDs
    let mut glyph_ids = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        glyph_ids.push(read_u32_bo(data, 16 + i * 4, bo));
    }

    let info_start = 16 + num_glyphs * 4;
    require_len!(data, info_start + num_glyphs * 12, seq, 139, minor, bo);

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
    let src_drawable = state.render.pictures.get(&src_picture).and_then(|p| {
        let did = p.drawable;
        if let Some(px) = state.pixmaps.get(&did) {
            Some((
                px.framebuffer.data().to_vec(),
                px.framebuffer.width() as usize,
            ))
        } else {
            state.windows.get(&did).map(|win| {
                (
                    win.framebuffer.data().to_vec(),
                    win.framebuffer.width() as usize,
                )
            })
        }
    });

    let format_id = state.render.glyphsets.get(&gsid).map(|gs| gs.format_id);

    let mut src_x_cursor: usize = 0;
    let mut glyphs_to_store: Vec<(u32, StoredGlyph)> = Vec::with_capacity(num_glyphs);

    for (&gid, &(width, height, x, y, x_off, y_off)) in glyph_ids.iter().zip(glyph_infos.iter()) {
        let glyph_data = if width > 0 && height > 0 {
            if let Some((ref fb_data, fb_stride)) = src_drawable {
                let bpp = 4usize;
                let mut pixels = Vec::new();

                match format_id {
                    Some(fmt) if fmt == PICTFORMAT_A8 => {
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
                            pixels.extend(std::iter::repeat_n(0u8, pad));
                        }
                    }
                    Some(fmt) if fmt == PICTFORMAT_ARGB32 => {
                        // Framebuffer storage is RGBA; glyph data is stored in
                        // ARGB32 wire byte order ([B, G, R, A]) so consumers
                        // (`get_glyph_argb`) read consistently with `AddGlyphs`.
                        for row in 0..height as usize {
                            let sy = row;
                            for col in 0..width as usize {
                                let sx = src_x_cursor + col;
                                let fb_off = sy * fb_stride * bpp + sx * bpp;
                                if fb_off + 4 <= fb_data.len() {
                                    pixels.push(fb_data[fb_off + 2]); // B
                                    pixels.push(fb_data[fb_off + 1]); // G
                                    pixels.push(fb_data[fb_off]); // R
                                    pixels.push(fb_data[fb_off + 3]); // A
                                } else {
                                    pixels.extend_from_slice(&[0, 0, 0, 0]);
                                }
                            }
                        }
                    }
                    _ => {
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
                            pixels.extend(std::iter::repeat_n(0u8, pad));
                        }
                    }
                }
                pixels
            } else {
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
    let req = parse_minor!(FreeGlyphsRequest, data, state, seq, 139, data[1] as u16);

    let gsid = req.glyphset;
    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for &gid in req.glyphs.iter() {
            gs.glyphs.remove(&gid);
        }
    }
    Vec::new()
}

/// Handle CompositeGlyphs8/16/32
///
/// Note: CompositeGlyphs8/16/32Request all have the same structure but
/// the `glyphcmds` field is raw bytes. The actual glyph ID size (1/2/4)
/// is embedded in the glyphcmd parsing, not the x11rb struct, so we parse
/// the header fields from the typed struct and continue to manually parse
/// the glyphcmds using the glyph_id_size parameter.
pub(crate) fn handle_composite_glyphs(
    state: &mut ClientState,
    data: &[u8],
    glyph_id_size: usize,
    seq: u16,
) -> Vec<u8> {
    let bo = state.msb_first;
    let minor = data[1] as u16;
    require_len!(data, 28, seq, 139, minor, bo);

    // All three CompositeGlyphs variants have the same field layout, but
    // x11rb's try_parse_request() rejects mismatched minor opcodes. We
    // parse using CompositeGlyphs8Request with a synthetic header that
    // claims minor=23 — the wire body is identical, only the glyph IDs
    // are 1/2/4 bytes (handled separately by `glyph_id_size`).
    use x11rb_protocol::protocol::render::{CompositeGlyphs8Request, COMPOSITE_GLYPHS8_REQUEST};
    use x11rb_protocol::x11_utils::RequestHeader;

    let real_header = crate::xserver::request::request_header(data);
    let header = RequestHeader {
        major_opcode: real_header.major_opcode,
        minor_opcode: COMPOSITE_GLYPHS8_REQUEST,
        remaining_length: real_header.remaining_length,
    };
    let req = parse_minor!(CompositeGlyphs8Request, data, state, seq, 139, minor, header);

    let pict_op = u8::from(req.op);
    let src_pic = req.src;
    let dst_pic = req.dst;
    let mut current_gsid = req.glyphset;

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
        None => {
            return render_value_err(seq, dst_pic, minor);
        }
    };
    let clip = ClipSnapshot::from_picture(state, dst_pic);

    // Parse glyphcmds from the raw bytes provided by x11rb
    let glyphcmds = &req.glyphcmds;
    let mut off = 0usize;
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

    while off < glyphcmds.len() {
        let len = glyphcmds[off] as usize;

        if len == 0 {
            break;
        }

        if len == 255 {
            // Glyphset switch
            if off + 8 <= glyphcmds.len() {
                // Read the new glyphset ID from the raw glyphcmds bytes (LE)
                current_gsid = u32::from_le_bytes([
                    glyphcmds[off + 4],
                    glyphcmds[off + 5],
                    glyphcmds[off + 6],
                    glyphcmds[off + 7],
                ]);
                off = pad4(off + 8);
            } else {
                break;
            }
            continue;
        }

        // Regular glyph element
        if off + 8 > glyphcmds.len() {
            break;
        }
        // bytes 1..3 = padding
        let delta_x = i16::from_le_bytes([glyphcmds[off + 4], glyphcmds[off + 5]]);
        let delta_y = i16::from_le_bytes([glyphcmds[off + 6], glyphcmds[off + 7]]);

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
        if glyph_data_start + glyph_data_bytes > glyphcmds.len() {
            break;
        }

        // Read glyph IDs from the raw glyphcmds bytes
        let mut glyph_ids = Vec::with_capacity(len);
        for i in 0..len {
            let gid_off = glyph_data_start + i * glyph_id_size;
            let gid = match glyph_id_size {
                1 => glyphcmds[gid_off] as u32,
                2 => u16::from_le_bytes([glyphcmds[gid_off], glyphcmds[gid_off + 1]]) as u32,
                4 => u32::from_le_bytes([
                    glyphcmds[gid_off],
                    glyphcmds[gid_off + 1],
                    glyphcmds[gid_off + 2],
                    glyphcmds[gid_off + 3],
                ]),
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
            let is_argb = op.format_id == PICTFORMAT_ARGB32;

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

                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 >= fb_data.len() {
                        continue;
                    }

                    if is_argb {
                        let (mb, mg, mr, ma) =
                            get_glyph_argb(&op.alpha_data, op.width, col as u16, row as u16);
                        if mb == 0 && mg == 0 && mr == 0 && ma == 0 {
                            continue;
                        }
                        let eff_b = ((sb as u32 * mb as u32 + 127) / 255) as u8;
                        let eff_g = ((sg as u32 * mg as u32 + 127) / 255) as u8;
                        let eff_r = ((sr as u32 * mr as u32 + 127) / 255) as u8;
                        let eff_a = ((sa as u32 * ma as u32 + 127) / 255) as u8;
                        let sa_b = ((sa as u32 * mb as u32 + 127) / 255) as u8;
                        let sa_g = ((sa as u32 * mg as u32 + 127) / 255) as u8;
                        let sa_r = ((sa as u32 * mr as u32 + 127) / 255) as u8;
                        let sa_a = ((sa as u32 * ma as u32 + 127) / 255) as u8;
                        composite_pixel_ca(
                            pict_op,
                            &mut fb_data[dst_off..dst_off + 4],
                            eff_b,
                            eff_g,
                            eff_r,
                            eff_a,
                            sa_b,
                            sa_g,
                            sa_r,
                            sa_a,
                            dst_has_alpha,
                        );
                    } else {
                        let alpha = get_glyph_alpha(
                            &op.alpha_data,
                            op.width,
                            col as u16,
                            row as u16,
                            op.format_id,
                        );
                        if alpha == 0 {
                            continue;
                        }
                        let eff_a = ((sa as u32 * alpha as u32 + 127) / 255) as u8;
                        let eff_r = ((sr as u32 * alpha as u32 + 127) / 255) as u8;
                        let eff_g = ((sg as u32 * alpha as u32 + 127) / 255) as u8;
                        let eff_b = ((sb as u32 * alpha as u32 + 127) / 255) as u8;

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
            let row_bytes = pad4((width as usize).div_ceil(8));
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

/// Extract per-channel BGRA values from an ARGB32-format glyph at a given position.
fn get_glyph_argb(data: &[u8], width: u16, x: u16, y: u16) -> (u8, u8, u8, u8) {
    let off = (y as usize * width as usize + x as usize) * 4;
    if off + 3 < data.len() {
        (data[off], data[off + 1], data[off + 2], data[off + 3])
    } else {
        (0, 0, 0, 0)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Wrapper for get_glyph_argb so parent module tests can call it.
    pub fn get_glyph_argb_wrapper(data: &[u8], width: u16, x: u16, y: u16) -> (u8, u8, u8, u8) {
        get_glyph_argb(data, width, x, y)
    }

    #[test]
    fn glyph_argb_second_pixel() {
        let data = vec![
            10, 20, 30, 200, // pixel (0,0)
            50, 60, 70, 100, // pixel (1,0)
        ];
        let (b, g, r, a) = get_glyph_argb(&data, 2, 1, 0);
        assert_eq!((b, g, r, a), (50, 60, 70, 100));
    }

    #[test]
    fn glyph_a8_alpha_extraction() {
        let data = vec![128, 64, 32, 0];
        let a = get_glyph_alpha(&data, 4, 0, 0, PICTFORMAT_A8);
        assert_eq!(a, 128);
        let a = get_glyph_alpha(&data, 4, 1, 0, PICTFORMAT_A8);
        assert_eq!(a, 64);
    }

    #[test]
    fn glyph_a1_bit_extraction() {
        let data = vec![0b00000101, 0, 0, 0];
        let a = get_glyph_alpha(&data, 8, 0, 0, PICTFORMAT_A1);
        assert_eq!(a, 255);
        let a = get_glyph_alpha(&data, 8, 1, 0, PICTFORMAT_A1);
        assert_eq!(a, 0);
        let a = get_glyph_alpha(&data, 8, 2, 0, PICTFORMAT_A1);
        assert_eq!(a, 255);
    }
}
