use std::collections::HashMap;
use tracing::{debug, info};

use crate::xserver::ClientState;

// PictFormat IDs
const PICTFORMAT_ARGB32: u32 = 0x24;
const PICTFORMAT_RGB24: u32 = 0x25;
const PICTFORMAT_A8: u32 = 0x26;
const PICTFORMAT_A1: u32 = 0x27;

pub struct RenderState {
    pictures: HashMap<u32, PictureState>,
    glyphsets: HashMap<u32, GlyphSetState>,
    solid_fills: HashMap<u32, SolidFillState>,
}

struct PictureState {
    drawable: u32,
    _format_id: u32,
    repeat: u32,
}

struct GlyphSetState {
    format_id: u32,
    glyphs: HashMap<u32, StoredGlyph>,
}

struct StoredGlyph {
    width: u16,
    height: u16,
    x: i16,
    y: i16,
    x_off: i16,
    y_off: i16,
    data: Vec<u8>, // alpha bitmap
}

struct SolidFillState {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            pictures: HashMap::new(),
            glyphsets: HashMap::new(),
            solid_fills: HashMap::new(),
        }
    }
}

/// Composite a single source pixel over a destination pixel using the OVER operator.
fn composite_over_pixel(dst: &mut [u8], src_b: u8, src_g: u8, src_r: u8, src_a: u8) {
    if src_a == 0 {
        return;
    }
    if src_a == 255 {
        dst[0] = src_b;
        dst[1] = src_g;
        dst[2] = src_r;
        dst[3] = 0xFF;
        return;
    }
    let sa = src_a as u32;
    let da = 255 - sa;
    dst[0] = ((src_b as u32 * sa + dst[0] as u32 * da) / 255) as u8;
    dst[1] = ((src_g as u32 * sa + dst[1] as u32 * da) / 255) as u8;
    dst[2] = ((src_r as u32 * sa + dst[2] as u32 * da) / 255) as u8;
    dst[3] = 0xFF;
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_i16(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

pub fn handle_render_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let minor = data[1];
    info!("Render op minor={minor}");

    match minor {
        0 => handle_query_version(seq),
        1 => handle_query_pict_formats(seq),
        4 => handle_create_picture(state, data),
        5 => handle_change_picture(state, data),
        6 => {
            // SetPictureClipRectangles - ignore
            Vec::new()
        }
        7 => handle_free_picture(state, data),
        8 => handle_composite(state, data),
        10 => handle_trapezoids(state, data),
        11 => handle_triangles(state, data),
        12 => handle_tri_strip(state, data),
        13 => handle_tri_fan(state, data),
        17 => handle_create_glyphset(state, data),
        19 => handle_free_glyphset(state, data),
        20 => handle_add_glyphs(state, data),
        22 => handle_free_glyphs(state, data),
        23 => handle_composite_glyphs(state, data, 1), // Glyphs8
        24 => handle_composite_glyphs(state, data, 2), // Glyphs16
        25 => handle_composite_glyphs(state, data, 4), // Glyphs32
        26 => handle_fill_rectangles(state, data),
        27 => {
            // CreateCursor - ignore
            Vec::new()
        }
        28 => {
            // SetPictureTransform - ignore
            Vec::new()
        }
        29 => handle_query_filters(seq),
        30 => {
            // SetPictureFilter - ignore
            Vec::new()
        }
        33 => handle_create_solid_fill(state, data),
        34 | 35 | 36 => handle_create_gradient_fill(state, data),
        _ => {
            debug!("Unhandled RENDER minor opcode: {minor}");
            Vec::new()
        }
    }
}

/// QueryVersion: reply with version 0.11
fn handle_query_version(seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0 (no extra data beyond 32 bytes)
    reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // major version
    reply[12..16].copy_from_slice(&11u32.to_le_bytes()); // minor version
    reply.to_vec()
}

/// QueryPictFormats: reply with ARGB32, RGB24, A8, A1 formats + screen info
fn handle_query_pict_formats(seq: u16) -> Vec<u8> {
    // We define 4 formats: ARGB32, RGB24, A8, A1
    let num_formats: u32 = 4;
    let num_screens: u32 = 1;
    let num_subpixel: u32 = 1;

    // Each PictForminfo is 28 bytes
    // Screen: 8 bytes header + depths
    // We report 2 depths (24 and 32) with visuals
    // Depth 24: 8 bytes header + 1 PictVisual (8 bytes) = 16 bytes
    // Depth 32: 8 bytes header + 0 PictVisuals = 8 bytes
    // Screen total: 8 + 16 + 8 = 32 bytes
    // Subpixel: 4 bytes each

    let num_depths: u32 = 2;
    let formats_bytes = num_formats as usize * 28;
    let screen_header = 8usize;
    let depth24_bytes = 8 + 8; // 8 header + 1 PictVisual(8)
    let depth32_bytes = 8; // 8 header + 0 PictVisuals
    let screen_bytes = screen_header + depth24_bytes + depth32_bytes;
    let subpixel_bytes = num_subpixel as usize * 4;
    let extra = formats_bytes + screen_bytes + subpixel_bytes;
    let total = 32 + extra;

    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra / 4) as u32).to_le_bytes()); // length in 4-byte units
    reply[8..12].copy_from_slice(&num_formats.to_le_bytes()); // num_formats
    reply[12..16].copy_from_slice(&num_screens.to_le_bytes()); // num_screens
    reply[16..20].copy_from_slice(&num_depths.to_le_bytes()); // num_depths
    // reply[20..24] = num_visuals (we have 1 visual across all depths)
    reply[20..24].copy_from_slice(&1u32.to_le_bytes());
    reply[24..28].copy_from_slice(&num_subpixel.to_le_bytes()); // num_subpixel

    let mut off = 32;

    // Format 1: ARGB32 (type=PictTypeDirect=1, depth=32)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_ARGB32,
        1,
        32, // type=Direct, depth=32
        16,
        0xFF,
        8,
        0xFF,
        0,
        0xFF,
        24,
        0xFF, // ARGB shifts/masks
    );

    // Format 2: RGB24 (type=PictTypeDirect=1, depth=24)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_RGB24,
        1,
        24,
        16,
        0xFF,
        8,
        0xFF,
        0,
        0xFF,
        0,
        0, // no alpha
    );

    // Format 3: A8 (type=PictTypeDirect=1, depth=8)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_A8,
        1,
        8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0xFF, // alpha only
    );

    // Format 4: A1 (type=PictTypeDirect=1, depth=1)
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_A1,
        1,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0x1, // 1-bit alpha
    );

    // Screen info (8 bytes header)
    let num_depths_for_screen: u32 = 2;
    // fallback pictformat for the screen
    reply[off..off + 4].copy_from_slice(&num_depths_for_screen.to_le_bytes());
    off += 4;
    reply[off..off + 4].copy_from_slice(&PICTFORMAT_RGB24.to_le_bytes()); // fallback
    off += 4;

    // Depth 24: header (8 bytes) + 1 PictVisual (8 bytes)
    reply[off] = 24; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    reply[off..off + 2].copy_from_slice(&1u16.to_le_bytes()); // num_visuals
    off += 2;
    off += 4; // pad

    // PictVisual for depth 24: visual(4) + format(4)
    reply[off..off + 4].copy_from_slice(&0x00000021u32.to_le_bytes()); // ROOT_VISUAL
    off += 4;
    reply[off..off + 4].copy_from_slice(&PICTFORMAT_RGB24.to_le_bytes());
    off += 4;

    // Depth 32: header (8 bytes) + 0 PictVisuals
    reply[off] = 32; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    reply[off..off + 2].copy_from_slice(&0u16.to_le_bytes()); // num_visuals
    off += 2;
    off += 4; // pad

    // Subpixel order (4 bytes): 0 = Unknown
    reply[off..off + 4].copy_from_slice(&0u32.to_le_bytes());

    reply
}

fn write_pictforminfo(
    buf: &mut [u8],
    off: &mut usize,
    id: u32,
    pict_type: u8,
    depth: u8,
    red_shift: u16,
    red_mask: u16,
    green_shift: u16,
    green_mask: u16,
    blue_shift: u16,
    blue_mask: u16,
    alpha_shift: u16,
    alpha_mask: u16,
) {
    let o = *off;
    buf[o..o + 4].copy_from_slice(&id.to_le_bytes());
    buf[o + 4] = pict_type;
    buf[o + 5] = depth;
    // 2 bytes pad at o+6..o+8
    buf[o + 8..o + 10].copy_from_slice(&red_shift.to_le_bytes());
    buf[o + 10..o + 12].copy_from_slice(&red_mask.to_le_bytes());
    buf[o + 12..o + 14].copy_from_slice(&green_shift.to_le_bytes());
    buf[o + 14..o + 16].copy_from_slice(&green_mask.to_le_bytes());
    buf[o + 16..o + 18].copy_from_slice(&blue_shift.to_le_bytes());
    buf[o + 18..o + 20].copy_from_slice(&blue_mask.to_le_bytes());
    buf[o + 20..o + 22].copy_from_slice(&alpha_shift.to_le_bytes());
    buf[o + 22..o + 24].copy_from_slice(&alpha_mask.to_le_bytes());
    // colormap (4 bytes) at o+24..o+28
    *off += 28;
}

fn handle_create_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let drawable = read_u32(data, 8);
    let format_id = read_u32(data, 12);
    let value_mask = read_u32(data, 16);

    let mut repeat = 0u32;
    let mut offset = 20;
    // Parse value list based on value_mask
    for bit in 0..13 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = read_u32(data, offset);
                if bit == 0 {
                    // CPRepeat
                    repeat = val;
                }
                offset += 4;
            }
        }
    }

    info!("CreatePicture: pid={pid:#x} drawable={drawable:#x} format={format_id:#x} repeat={repeat}");

    state.render.pictures.insert(
        pid,
        PictureState {
            drawable,
            _format_id: format_id,
            repeat,
        },
    );
    Vec::new()
}

fn handle_change_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    let value_mask = read_u32(data, 8);

    if let Some(pic) = state.render.pictures.get_mut(&pid) {
        let mut offset = 12;
        for bit in 0..13 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = read_u32(data, offset);
                    if bit == 0 {
                        pic.repeat = val;
                    }
                    offset += 4;
                }
            }
        }
    }
    Vec::new()
}

fn handle_free_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let pid = read_u32(data, 4);
        state.render.pictures.remove(&pid);
    }
    Vec::new()
}

/// The main compositing operation.
fn handle_composite(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 36 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let _mask_pic = read_u32(data, 12);
    let dst_pic = read_u32(data, 16);
    let src_x = read_i16(data, 20);
    let src_y = read_i16(data, 22);
    let _mask_x = read_i16(data, 24);
    let _mask_y = read_i16(data, 26);
    let dst_x = read_i16(data, 28);
    let dst_y = read_i16(data, 30);
    let width = read_u16(data, 32);
    let height = read_u16(data, 34);

    info!(
        "Render Composite: op={op} src={src_pic:#x} dst={dst_pic:#x} src=({src_x},{src_y}) dst=({dst_x},{dst_y}) {width}x{height}"
    );

    // Resolve source pixels
    let src_pixels: Option<(Vec<u8>, u32, u32)> = resolve_source_pixels(state, src_pic, src_x, src_y, width, height);

    // Resolve dst drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);

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
                    let src_off = (row as usize * src_w as usize + col as usize) * 4;
                    if src_off + 3 >= src_data.len() {
                        continue;
                    }
                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 >= fb_data.len() {
                        continue;
                    }
                    let sb = src_data[src_off];
                    let sg = src_data[src_off + 1];
                    let sr = src_data[src_off + 2];
                    let sa = src_data[src_off + 3];

                    match op {
                        3 => {
                            // PictOpOver
                            composite_over_pixel(&mut fb_data[dst_off..dst_off + 4], sb, sg, sr, sa);
                        }
                        1 => {
                            // PictOpSrc
                            fb_data[dst_off] = sb;
                            fb_data[dst_off + 1] = sg;
                            fb_data[dst_off + 2] = sr;
                            fb_data[dst_off + 3] = sa;
                        }
                        0 => {
                            // PictOpClear
                            fb_data[dst_off] = 0;
                            fb_data[dst_off + 1] = 0;
                            fb_data[dst_off + 2] = 0;
                            fb_data[dst_off + 3] = 0;
                        }
                        _ => {
                            // Default to Over
                            composite_over_pixel(&mut fb_data[dst_off..dst_off + 4], sb, sg, sr, sa);
                        }
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

/// Read a FIXED (16.16 fixed-point) value from data.
fn read_fixed(data: &[u8], off: usize) -> f64 {
    let raw = i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    raw as f64 / 65536.0
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
fn handle_trapezoids(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    // Resolve source color
    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    info!(
        "Render Trapezoids: op={op} src={src_pic:#x} dst={dst_pic:#x} color=({sr},{sg},{sb},{sa})"
    );

    // Get destination drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

    // Parse trapezoids (40 bytes each starting at offset 24)
    let mut off = 24;
    let mut traps = Vec::new();
    while off + 40 <= data.len() {
        let top = read_fixed(data, off);
        let bottom = read_fixed(data, off + 4);
        let left_x1 = read_fixed(data, off + 8);
        let left_y1 = read_fixed(data, off + 12);
        let left_x2 = read_fixed(data, off + 16);
        let left_y2 = read_fixed(data, off + 20);
        let right_x1 = read_fixed(data, off + 24);
        let right_y1 = read_fixed(data, off + 28);
        let right_x2 = read_fixed(data, off + 32);
        let right_y2 = read_fixed(data, off + 36);
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
                    fb, fb_w, fb_h, op, sr, sg, sb, sa,
                    top, bottom, lx1, ly1, lx2, ly2, rx1, ry1, rx2, ry2,
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
fn rasterize_trapezoid(
    fb: &mut crate::framebuffer::Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
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
) {
    let y_start = top.ceil() as i32;
    let y_end = bottom.floor() as i32;

    if y_start > y_end {
        return;
    }

    let fb_stride = fb.stride();
    let fb_data = fb.data_mut();

    // Precompute edge deltas
    let left_dy = ly2 - ly1;
    let right_dy = ry2 - ry1;

    for y in y_start..=y_end {
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

        let x_start = left_x.ceil() as i32;
        let x_end = right_x.floor() as i32;

        for x in x_start..=x_end {
            if x < 0 || x >= fb_w {
                continue;
            }
            let dst_off = y as usize * fb_stride + x as usize * 4;
            if dst_off + 3 >= fb_data.len() {
                continue;
            }
            match op {
                0 => {
                    // PictOpClear
                    fb_data[dst_off] = 0;
                    fb_data[dst_off + 1] = 0;
                    fb_data[dst_off + 2] = 0;
                    fb_data[dst_off + 3] = 0;
                }
                1 => {
                    // PictOpSrc
                    fb_data[dst_off] = sb;
                    fb_data[dst_off + 1] = sg;
                    fb_data[dst_off + 2] = sr;
                    fb_data[dst_off + 3] = sa;
                }
                _ => {
                    // PictOpOver (3) and default
                    composite_over_pixel(
                        &mut fb_data[dst_off..dst_off + 4],
                        sb, sg, sr, sa,
                    );
                }
            }
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
fn handle_triangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    info!("Render Triangles: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

    // Each triangle = 3 POINTFIX (each 8 bytes = x FIXED + y FIXED) = 24 bytes
    let mut off = 24;
    let mut triangles = Vec::new();
    while off + 24 <= data.len() {
        let x1 = read_fixed(data, off);
        let y1 = read_fixed(data, off + 4);
        let x2 = read_fixed(data, off + 8);
        let y2 = read_fixed(data, off + 12);
        let x3 = read_fixed(data, off + 16);
        let y3 = read_fixed(data, off + 20);
        triangles.push((x1, y1, x2, y2, x3, y3));
        off += 24;
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        for &(x1, y1, x2, y2, x3, y3) in &triangles {
            rasterize_triangle(fb, fb_w, fb_h, op, sr, sg, sb, sa, x1, y1, x2, y2, x3, y3);
        }
    }

    Vec::new()
}

/// Rasterize a single triangle using scanline conversion.
fn rasterize_triangle(
    fb: &mut crate::framebuffer::Framebuffer,
    fb_w: i32,
    fb_h: i32,
    op: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
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
            fb, fb_w, fb_h, op, sr, sg, sb, sa,
            vy0, vy1, llx.0, llx.1, llx.2, llx.3, rrx.0, rrx.1, rrx.2, rrx.3,
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
            fb, fb_w, fb_h, op, sr, sg, sb, sa,
            vy1, vy2, llx.0, llx.1, llx.2, llx.3, rrx.0, rrx.1, rrx.2, rrx.3,
        );
    }
}

/// Handle XRender TriStrip (minor opcode 12).
fn handle_tri_strip(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    info!("Render TriStrip: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

    // Points: 8 bytes each (FIXED x + FIXED y)
    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed(data, off);
        let y = read_fixed(data, off + 4);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return Vec::new();
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        for i in 0..points.len() - 2 {
            let (x1, y1) = points[i];
            let (x2, y2) = points[i + 1];
            let (x3, y3) = points[i + 2];
            rasterize_triangle(fb, fb_w, fb_h, op, sr, sg, sb, sa, x1, y1, x2, y2, x3, y3);
        }
    }

    Vec::new()
}

/// Handle XRender TriFan (minor opcode 13).
fn handle_tri_fan(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let _src_x = read_i16(data, 20);
    let _src_y = read_i16(data, 22);

    info!("Render TriFan: op={op} src={src_pic:#x} dst={dst_pic:#x}");

    let (sr, sg, sb, sa) = resolve_source_color(state, src_pic);

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

    let mut points = Vec::new();
    let mut off = 24;
    while off + 8 <= data.len() {
        let x = read_fixed(data, off);
        let y = read_fixed(data, off + 4);
        points.push((x, y));
        off += 8;
    }

    if points.len() < 3 {
        return Vec::new();
    }

    if let Some(fb) = state.get_framebuffer_mut(dst_draw) {
        let fb_w = fb.width() as i32;
        let fb_h = fb.height() as i32;

        let (cx, cy) = points[0];
        for i in 1..points.len() - 1 {
            let (x2, y2) = points[i];
            let (x3, y3) = points[i + 1];
            rasterize_triangle(fb, fb_w, fb_h, op, sr, sg, sb, sa, cx, cy, x2, y2, x3, y3);
        }
    }

    Vec::new()
}

/// Resolve source picture to pixel data. Returns (pixels, width, height) in BGRA format.
fn resolve_source_pixels(
    state: &mut ClientState,
    src_pic: u32,
    src_x: i16,
    src_y: i16,
    width: u16,
    height: u16,
) -> Option<(Vec<u8>, u32, u32)> {
    // Check if it's a solid fill
    if let Some(fill) = state.render.solid_fills.get(&src_pic) {
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let off = i * 4;
            pixels[off] = fill.b;
            pixels[off + 1] = fill.g;
            pixels[off + 2] = fill.r;
            pixels[off + 3] = fill.a;
        }
        return Some((pixels, w, h));
    }

    // Check if it's a picture wrapping a drawable
    let (drawable, repeat) = {
        let pic = state.render.pictures.get(&src_pic)?;
        (pic.drawable, pic.repeat)
    };

    // Check if the drawable's picture is actually a solid fill
    if let Some(fill) = state.render.solid_fills.get(&drawable) {
        let w = width as u32;
        let h = height as u32;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let off = i * 4;
            pixels[off] = fill.b;
            pixels[off + 1] = fill.g;
            pixels[off + 2] = fill.r;
            pixels[off + 3] = fill.a;
        }
        return Some((pixels, w, h));
    }

    // Sync SHM-backed pixmap data before reading
    state.sync_shm_pixmap(drawable);

    // Extract pixels from the drawable's framebuffer
    let fb = state.get_framebuffer_mut(drawable)?;

    if repeat != 0 {
        // Repeat mode: tile the source
        let fb_w = fb.width();
        let fb_h = fb.height();
        if fb_w == 0 || fb_h == 0 {
            return None;
        }
        let w = width as u32;
        let h = height as u32;
        let fb_stride = fb.stride();
        let fb_data = fb.data();
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for row in 0..h {
            for col in 0..w {
                let sy = ((src_y as i32 + row as i32) % fb_h as i32 + fb_h as i32) as u32 % fb_h;
                let sx = ((src_x as i32 + col as i32) % fb_w as i32 + fb_w as i32) as u32 % fb_w;
                let src_off = sy as usize * fb_stride + sx as usize * 4;
                let dst_off = (row * w + col) as usize * 4;
                if src_off + 3 < fb_data.len() && dst_off + 3 < pixels.len() {
                    pixels[dst_off..dst_off + 4].copy_from_slice(&fb_data[src_off..src_off + 4]);
                }
            }
        }
        Some((pixels, w, h))
    } else {
        let pixels = fb.extract_pixels(src_x, src_y, width, height);
        Some((pixels, width as u32, height as u32))
    }
}

fn handle_create_glyphset(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let gsid = read_u32(data, 4);
    let format_id = read_u32(data, 8);

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

fn handle_free_glyphset(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let gsid = read_u32(data, 4);
        state.render.glyphsets.remove(&gsid);
    }
    Vec::new()
}

fn handle_add_glyphs(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gsid = read_u32(data, 4);
    let num_glyphs = read_u32(data, 8) as usize;

    debug!("Render AddGlyphs: gsid={gsid:#x} num={num_glyphs}");

    if data.len() < 12 + num_glyphs * 4 {
        return Vec::new();
    }

    // Read glyph IDs
    let mut glyph_ids = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        glyph_ids.push(read_u32(data, 12 + i * 4));
    }

    let info_start = 12 + num_glyphs * 4;
    if data.len() < info_start + num_glyphs * 12 {
        return Vec::new();
    }

    // Read GlyphInfo entries (12 bytes each)
    let mut glyph_infos = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        let off = info_start + i * 12;
        let width = read_u16(data, off);
        let height = read_u16(data, off + 2);
        let x = read_i16(data, off + 4);
        let y = read_i16(data, off + 6);
        let x_off = read_i16(data, off + 8);
        let y_off = read_i16(data, off + 10);
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

fn handle_free_glyphs(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let gsid = read_u32(data, 4);
    let num_glyphs = (data.len() - 8) / 4;

    if let Some(gs) = state.render.glyphsets.get_mut(&gsid) {
        for i in 0..num_glyphs {
            let gid = read_u32(data, 8 + i * 4);
            gs.glyphs.remove(&gid);
        }
    }
    Vec::new()
}

/// Handle CompositeGlyphs8/16/32
fn handle_composite_glyphs(state: &mut ClientState, data: &[u8], glyph_id_size: usize) -> Vec<u8> {
    if data.len() < 28 {
        return Vec::new();
    }

    let _op = data[4];
    let src_pic = read_u32(data, 8);
    let dst_pic = read_u32(data, 12);
    let _mask_format = read_u32(data, 16);
    let mut current_gsid = read_u32(data, 20);
    let _src_x = read_i16(data, 24);
    let _src_y = read_i16(data, 26);

    debug!(
        "Render CompositeGlyphs{}: src={src_pic:#x} dst={dst_pic:#x} gs={current_gsid:#x}",
        glyph_id_size * 8
    );

    // Resolve source color (typically solid fill for text)
    let src_color = resolve_source_color(state, src_pic);

    // Resolve dst drawable
    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

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
                current_gsid = read_u32(data, off + 4);
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
        let delta_x = read_i16(data, off + 4);
        let delta_y = read_i16(data, off + 6);

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
                2 => read_u16(data, gid_off) as u32,
                4 => read_u32(data, gid_off),
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

                    let alpha = get_glyph_alpha(&op.alpha_data, op.width, col as u16, row as u16, op.format_id);
                    if alpha == 0 {
                        continue;
                    }

                    // Modulate source color by glyph alpha
                    let eff_a = ((sa as u32 * alpha as u32) / 255) as u8;
                    let eff_r = ((sr as u32 * alpha as u32) / 255) as u8;
                    let eff_g = ((sg as u32 * alpha as u32) / 255) as u8;
                    let eff_b = ((sb as u32 * alpha as u32) / 255) as u8;

                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 < fb_data.len() {
                        composite_over_pixel(&mut fb_data[dst_off..dst_off + 4], eff_b, eff_g, eff_r, eff_a);
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

/// Resolve a source picture to a solid RGBA color.
fn resolve_source_color(state: &ClientState, src_pic: u32) -> (u8, u8, u8, u8) {
    // Check if it's a solid fill directly
    if let Some(fill) = state.render.solid_fills.get(&src_pic) {
        return (fill.r, fill.g, fill.b, fill.a);
    }

    // Check if it's a picture wrapping a solid fill
    if let Some(pic) = state.render.pictures.get(&src_pic) {
        if let Some(fill) = state.render.solid_fills.get(&pic.drawable) {
            return (fill.r, fill.g, fill.b, fill.a);
        }
    }

    // Default: opaque white
    (0xFF, 0xFF, 0xFF, 0xFF)
}

fn handle_fill_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let op = data[4];
    let dst_pic = read_u32(data, 8);
    // Color is in the request at offset 12..20 (CARD16 each: red, green, blue, alpha)
    let red = read_u16(data, 12);
    let green = read_u16(data, 14);
    let blue = read_u16(data, 16);
    let alpha = read_u16(data, 18);

    let r = (red >> 8) as u8;
    let g = (green >> 8) as u8;
    let b = (blue >> 8) as u8;
    let a = (alpha >> 8) as u8;

    let dst_drawable = state.render.pictures.get(&dst_pic).map(|p| p.drawable);
    let dst_draw = match dst_drawable {
        Some(d) => d,
        None => return Vec::new(),
    };

    // Parse rectangles (8 bytes each: x(2) y(2) w(2) h(2))
    let mut off = 20;
    let mut rects = Vec::new();
    while off + 8 <= data.len() {
        let x = read_i16(data, off);
        let y = read_i16(data, off + 2);
        let w = read_u16(data, off + 4);
        let h = read_u16(data, off + 6);
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
                    let dst_off = dy as usize * fb_stride + dx as usize * 4;
                    if dst_off + 3 >= fb_data.len() {
                        continue;
                    }
                    match op {
                        1 => {
                            // PictOpSrc
                            fb_data[dst_off] = b;
                            fb_data[dst_off + 1] = g;
                            fb_data[dst_off + 2] = r;
                            fb_data[dst_off + 3] = a;
                        }
                        3 => {
                            // PictOpOver
                            composite_over_pixel(&mut fb_data[dst_off..dst_off + 4], b, g, r, a);
                        }
                        0 => {
                            // PictOpClear
                            fb_data[dst_off] = 0;
                            fb_data[dst_off + 1] = 0;
                            fb_data[dst_off + 2] = 0;
                            fb_data[dst_off + 3] = 0;
                        }
                        _ => {
                            composite_over_pixel(&mut fb_data[dst_off..dst_off + 4], b, g, r, a);
                        }
                    }
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

fn handle_query_filters(seq: u16) -> Vec<u8> {
    // Return ["nearest", "bilinear"]
    let filter1 = b"nearest";
    let filter2 = b"bilinear";

    // Each alias is 2 bytes (CARD16), num_aliases first
    // Each filter is: 1-byte length + name bytes, padded to 4 bytes
    let num_aliases: u32 = 0;
    let num_filters: u32 = 2;

    let aliases_bytes = 0usize;
    let filter1_bytes = 1 + filter1.len(); // length byte + name
    let filter2_bytes = 1 + filter2.len();
    let filters_bytes = pad4(filter1_bytes) + pad4(filter2_bytes);
    let extra = aliases_bytes + filters_bytes;
    let total = 32 + extra;

    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra / 4) as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&num_aliases.to_le_bytes());
    reply[12..16].copy_from_slice(&num_filters.to_le_bytes());

    let mut off = 32;
    // Filter 1: "nearest"
    reply[off] = filter1.len() as u8;
    off += 1;
    reply[off..off + filter1.len()].copy_from_slice(filter1);
    off = 32 + pad4(filter1_bytes);

    // Filter 2: "bilinear"
    reply[off] = filter2.len() as u8;
    off += 1;
    reply[off..off + filter2.len()].copy_from_slice(filter2);

    reply
}

fn handle_create_solid_fill(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);
    // Color: 4 x CARD16 (red, green, blue, alpha) at offset 8
    let red = read_u16(data, 8);
    let green = read_u16(data, 10);
    let blue = read_u16(data, 12);
    let alpha = read_u16(data, 14);

    debug!("Render CreateSolidFill: pid={pid:#x} rgba=({red},{green},{blue},{alpha})");

    state.render.solid_fills.insert(
        pid,
        SolidFillState {
            r: (red >> 8) as u8,
            g: (green >> 8) as u8,
            b: (blue >> 8) as u8,
            a: (alpha >> 8) as u8,
        },
    );
    Vec::new()
}

fn handle_create_gradient_fill(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let pid = read_u32(data, 4);

    // Approximate gradient as a solid fill using first stop color if available
    // Gradient requests have varying layouts, just store a neutral color
    debug!("Render CreateGradientFill (approx): pid={pid:#x}");

    state.render.solid_fills.insert(
        pid,
        SolidFillState {
            r: 128,
            g: 128,
            b: 128,
            a: 255,
        },
    );
    Vec::new()
}
