use tracing::debug;

use crate::xserver::ClientState;
use crate::xserver::core::{read_u16_bo, read_u32_bo, read_i16_bo, write_u16_bo, write_u32_bo};
use super::{
    PICTFORMAT_ARGB32, PICTFORMAT_RGB24, PICTFORMAT_A8, PICTFORMAT_A1,
    PICTFORMAT_XRGB32, PICTFORMAT_XBGR32,
    PictureState, PictFilter, resolve_source_pixels,
};

/// QueryVersion: reply with version 0.11
pub(crate) fn handle_query_version(seq: u16, bo: bool) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    write_u16_bo(&mut reply, 2, seq, bo);
    // length = 0 (no extra data beyond 32 bytes)
    write_u32_bo(&mut reply, 8, 0, bo); // major version
    write_u32_bo(&mut reply, 12, 11, bo); // minor version
    reply.to_vec()
}

/// QueryPictFormats: reply with ARGB32, RGB24, A8, A1, xRGB32, xBGR32
/// formats + screen info
pub(crate) fn handle_query_pict_formats(seq: u16, bo: bool) -> Vec<u8> {
    // We define 6 formats: ARGB32, RGB24, A8, A1, xRGB32, xBGR32.
    // The two `x*` formats are needed for the rendercheck
    // libreoffice / gtk byte-swap tests; they share the depth-32
    // pixmap layout with ARGB32 but treat the high byte as padding
    // (xRGB) or swap R/B (xBGR).
    let num_formats: u32 = 6;
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
    write_u16_bo(&mut reply, 2, seq, bo);
    write_u32_bo(&mut reply, 4, (extra / 4) as u32, bo); // length in 4-byte units
    write_u32_bo(&mut reply, 8, num_formats, bo); // num_formats
    write_u32_bo(&mut reply, 12, num_screens, bo); // num_screens
    write_u32_bo(&mut reply, 16, num_depths, bo); // num_depths
    // reply[20..24] = num_visuals (we have 1 visual across all depths)
    write_u32_bo(&mut reply, 20, 1, bo);
    write_u32_bo(&mut reply, 24, num_subpixel, bo); // num_subpixel

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
        bo,
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
        bo,
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
        bo,
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
        bo,
    );

    // Format 5: xRGB32 — depth 32, R/G/B in the same byte positions
    // as ARGB32 but the high byte is padding (alphaMask = 0). The
    // rendercheck libreoffice test wants this to verify that the
    // server doesn't peek at the unused byte.
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_XRGB32,
        1,
        32,
        16,
        0xFF,
        8,
        0xFF,
        0,
        0xFF,
        0,
        0, // no alpha
        bo,
    );

    // Format 6: xBGR32 — depth 32 with R/B swapped (R at byte 0, B
    // at byte 2). The rendercheck gtk test exercises this layout
    // against ARGB32 to verify that the server reads each picture
    // through its declared format rather than blindly assuming a
    // canonical byte order.
    write_pictforminfo(
        &mut reply,
        &mut off,
        PICTFORMAT_XBGR32,
        1,
        32,
        0,
        0xFF,
        8,
        0xFF,
        16,
        0xFF,
        0,
        0, // no alpha
        bo,
    );

    // Screen info (8 bytes header)
    let num_depths_for_screen: u32 = 2;
    // fallback pictformat for the screen
    write_u32_bo(&mut reply, off, num_depths_for_screen, bo);
    off += 4;
    write_u32_bo(&mut reply, off, PICTFORMAT_RGB24, bo); // fallback
    off += 4;

    // Depth 24: header (8 bytes) + 1 PictVisual (8 bytes)
    reply[off] = 24; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    write_u16_bo(&mut reply, off, 1, bo); // num_visuals
    off += 2;
    off += 4; // pad

    // PictVisual for depth 24: visual(4) + format(4)
    write_u32_bo(&mut reply, off, 0x00000021, bo); // ROOT_VISUAL
    off += 4;
    write_u32_bo(&mut reply, off, PICTFORMAT_RGB24, bo);
    off += 4;

    // Depth 32: header (8 bytes) + 0 PictVisuals
    reply[off] = 32; // depth
    off += 1;
    reply[off] = 0; // pad
    off += 1;
    write_u16_bo(&mut reply, off, 0, bo); // num_visuals
    off += 2;
    off += 4; // pad

    // Subpixel order (4 bytes): 0 = Unknown
    write_u32_bo(&mut reply, off, 0, bo);

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
    bo: bool,
) {
    let o = *off;
    write_u32_bo(buf, o, id, bo);
    buf[o + 4] = pict_type;
    buf[o + 5] = depth;
    // 2 bytes pad at o+6..o+8
    write_u16_bo(buf, o + 8, red_shift, bo);
    write_u16_bo(buf, o + 10, red_mask, bo);
    write_u16_bo(buf, o + 12, green_shift, bo);
    write_u16_bo(buf, o + 14, green_mask, bo);
    write_u16_bo(buf, o + 16, blue_shift, bo);
    write_u16_bo(buf, o + 18, blue_mask, bo);
    write_u16_bo(buf, o + 20, alpha_shift, bo);
    write_u16_bo(buf, o + 22, alpha_mask, bo);
    // colormap (4 bytes) at o+24..o+28
    *off += 28;
}

pub(crate) fn handle_create_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 20 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let pid = read_u32_bo(data, 4, bo);
    let drawable = read_u32_bo(data, 8, bo);
    let format_id = read_u32_bo(data, 12, bo);
    let value_mask = read_u32_bo(data, 16, bo);

    debug!(
        "Render CreatePicture: pid={pid:#x} drawable={drawable:#x} format={format_id:#x} vmask={value_mask:#x}"
    );

    // Validate drawable exists (BadDrawable if not)
    let drawable_depth: u8 = if state.windows.contains_key(&drawable) {
        // Windows use the root visual depth (24-bit TrueColor stored as 32bpp)
        24
    } else if let Some(p) = state.pixmaps.get(&drawable) {
        p.depth
    } else {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_DRAWABLE, seq, drawable, 139, data[1] as u16, bo,
        );
    };

    // Validate format ID is known
    let format_depth: u8 = match format_id {
        PICTFORMAT_ARGB32 | PICTFORMAT_XRGB32 | PICTFORMAT_XBGR32 => 32,
        PICTFORMAT_RGB24 => 24,
        PICTFORMAT_A8 => 8,
        PICTFORMAT_A1 => 1,
        _ => {
            return crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_MATCH, seq, format_id, 139, data[1] as u16, bo,
            );
        }
    };

    // Validate format depth matches drawable depth (BadMatch if not).
    // Allow 32-bit formats on 24-bit drawables (common practice — GTK/Qt
    // routinely create ARGB32 pictures on depth-24 windows) and 24-bit
    // formats on 32-bit drawables.
    let depth_compatible = match (format_depth, drawable_depth) {
        (fd, dd) if fd == dd => true,
        (32, 24) | (24, 32) => true,
        _ => false,
    };
    if !depth_compatible {
        debug!(
            "CreatePicture: format depth {format_depth} incompatible with drawable depth {drawable_depth}"
        );
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_MATCH, seq, format_id, 139, data[1] as u16, bo,
        );
    }

    let mut repeat = 0u32;
    let mut component_alpha = false;
    let mut clip_mask: Option<u32> = None;
    let mut val_off = 20;
    // Parse value-list based on value_mask
    for bit in 0..13 {
        if value_mask & (1 << bit) != 0 {
            if val_off + 4 > data.len() {
                break;
            }
            let val = read_u32_bo(data, val_off, bo);
            match bit {
                0 => {
                    repeat = val;
                    debug!("  repeat={repeat}");
                }
                6 => {
                    // CPClipMask
                    clip_mask = if val == 0 { None } else { Some(val) };
                    debug!("  clip_mask={val:#x}");
                }
                12 => {
                    component_alpha = val != 0;
                    debug!("  component_alpha={component_alpha}");
                }
                _ => {}
            }
            val_off += 4;
        }
    }

    state.render.pictures.insert(
        pid,
        PictureState {
            drawable,
            format_id,
            repeat,
            component_alpha,
            clip_rects: None,
            clip_origin_x: 0,
            clip_origin_y: 0,
            clip_mask,
            filter: PictFilter::Nearest,
        },
    );
    Vec::new()
}

pub(crate) fn handle_change_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let pid = read_u32_bo(data, 4, bo);
    let value_mask = read_u32_bo(data, 8, bo);

    debug!("Render ChangePicture: pid={pid:#x} vmask={value_mask:#x}");

    let mut val_off = 12;
    for bit in 0..13 {
        if value_mask & (1 << bit) != 0 {
            if val_off + 4 > data.len() {
                break;
            }
            let val = read_u32_bo(data, val_off, bo);
            if let Some(pic) = state.render.pictures.get_mut(&pid) {
                match bit {
                    0 => {
                        pic.repeat = val;
                        debug!("  repeat={val}");
                    }
                    6 => {
                        pic.clip_mask = if val == 0 { None } else { Some(val) };
                        debug!("  clip_mask={val:#x}");
                    }
                    12 => {
                        pic.component_alpha = val != 0;
                        debug!("  component_alpha={}", pic.component_alpha);
                    }
                    _ => {}
                }
            }
            val_off += 4;
        }
    }

    Vec::new()
}

pub(crate) fn handle_set_picture_clip_rectangles(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let pid = read_u32_bo(data, 4, bo);
    let clip_x = read_i16_bo(data, 8, bo);
    let clip_y = read_i16_bo(data, 10, bo);

    let mut rects = Vec::new();
    let mut off = 12;
    while off + 8 <= data.len() {
        let x = read_i16_bo(data, off, bo);
        let y = read_i16_bo(data, off + 2, bo);
        let w = read_u16_bo(data, off + 4, bo);
        let h = read_u16_bo(data, off + 6, bo);
        rects.push((x, y, w, h));
        off += 8;
    }

    debug!(
        "Render SetPictureClipRectangles: pid={pid:#x} origin=({clip_x},{clip_y}) rects={}",
        rects.len()
    );

    if let Some(pic) = state.render.pictures.get_mut(&pid) {
        pic.clip_origin_x = clip_x;
        pic.clip_origin_y = clip_y;
        pic.clip_rects = if rects.is_empty() { None } else { Some(rects) };
    }

    Vec::new()
}

pub(crate) fn handle_free_picture(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() >= 8 {
        let pid = read_u32_bo(data, 4, bo);
        state.render.pictures.remove(&pid);
    }
    Vec::new()
}

/// CreateCursor (RENDER minor opcode 27).
/// Creates a cursor from a RENDER picture. Renders the source picture
/// to an ARGB bitmap and stores it as a CursorInfo for later use.
pub(crate) fn handle_create_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use crate::xserver::types::CursorInfo;

    let bo = state.msb_first;
    if data.len() < 16 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let cursor_id = read_u32_bo(data, 4, bo);
    let src_picture = read_u32_bo(data, 8, bo);
    let hotspot_x = read_u16_bo(data, 12, bo);
    let hotspot_y = read_u16_bo(data, 14, bo);

    debug!("Render CreateCursor: cursor_id={cursor_id:#x} src_pic={src_picture:#x} hotspot=({hotspot_x},{hotspot_y})");

    // Get the source picture's drawable dimensions to know cursor size
    let (width, height) = if let Some(pic) = state.render.pictures.get(&src_picture) {
        let d = pic.drawable;
        if let Some(px) = state.pixmaps.get(&d) {
            (px.framebuffer.width() as u16, px.framebuffer.height() as u16)
        } else if let Some(win) = state.windows.get(&d) {
            (win.framebuffer.width() as u16, win.framebuffer.height() as u16)
        } else {
            (32, 32) // fallback
        }
    } else {
        (32, 32) // fallback
    };

    // Resolve the source picture pixels to get the cursor image
    let argb_data = if let Some((pixels, _w, _h)) =
        resolve_source_pixels(state, src_picture, 0, 0, width, height)
    {
        // Convert from BGRA (internal) to ARGB (cursor format)
        let mut argb = vec![0u8; pixels.len()];
        for i in (0..pixels.len()).step_by(4) {
            if i + 3 < pixels.len() {
                argb[i] = pixels[i + 3]; // A
                argb[i + 1] = pixels[i + 2]; // R
                argb[i + 2] = pixels[i + 1]; // G
                argb[i + 3] = pixels[i]; // B
            }
        }
        argb
    } else {
        Vec::new()
    };

    // Register the cursor with full bitmap data
    state.cursors.insert(cursor_id, "render-cursor".to_string());
    state.cursor_info.insert(cursor_id, CursorInfo {
        css_name: String::new(),
        source_pixmap: 0,
        mask_pixmap: 0,
        fore_red: 0,
        fore_green: 0,
        fore_blue: 0,
        back_red: 0,
        back_green: 0,
        back_blue: 0,
        hotspot_x,
        hotspot_y,
        argb_data,
        width,
        height,
        name: String::new(),
        anim_frames: Vec::new(),
    });
    Vec::new()
}

/// CreateAnimCursor (RENDER minor opcode 31).
/// Creates an animated cursor from a list of cursor/delay pairs.
/// Collects ARGB bitmap data from each referenced cursor to build
/// a full animation sequence sent to the frontend via CursorAnimated.
pub(crate) fn handle_create_anim_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let cursor_id = read_u32_bo(data, 4, bo);
    let num_frames = (data.len() - 8) / 8; // each frame: cursor_id(4) + delay(4)
    debug!("Render CreateAnimCursor: cursor_id={cursor_id:#x} frames={num_frames}");

    // Collect animation frame data from each referenced cursor.
    let mut anim_frames: Vec<(Vec<u8>, u16, u16, u16, u16, u32)> = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let off = 8 + i * 8;
        if off + 8 > data.len() { break; }
        let frame_cid = read_u32_bo(data, off, bo);
        let delay = read_u32_bo(data, off + 4, bo);
        if let Some(info) = state.cursor_info.get(&frame_cid) {
            if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                anim_frames.push((
                    info.argb_data.clone(),
                    info.width,
                    info.height,
                    info.hotspot_x,
                    info.hotspot_y,
                    delay,
                ));
            }
        }
    }

    state.cursors.insert(cursor_id, "anim-cursor".to_string());

    // Use the first frame as the static fallback, and store all frames.
    if let Some((ref argb, w, h, hx, hy, _)) = anim_frames.first() {
        use crate::xserver::types::CursorInfo;
        state.cursor_info.insert(cursor_id, CursorInfo {
            css_name: String::new(),
            source_pixmap: 0,
            mask_pixmap: 0,
            fore_red: 0, fore_green: 0, fore_blue: 0,
            back_red: 0, back_green: 0, back_blue: 0,
            hotspot_x: *hx,
            hotspot_y: *hy,
            argb_data: argb.clone(),
            width: *w,
            height: *h,
            name: String::new(),
            anim_frames,
        });
    } else {
        // No valid frames found — copy first frame's cursor info as fallback
        let first_frame_cursor = if num_frames > 0 && data.len() >= 16 {
            Some(read_u32_bo(data, 8, bo))
        } else {
            None
        };
        if let Some(frame_cid) = first_frame_cursor {
            if let Some(info) = state.cursor_info.get(&frame_cid).cloned() {
                state.cursor_info.insert(cursor_id, info);
            }
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// QueryPictIndexValues (RENDER minor opcode 2)
// ---------------------------------------------------------------------------

/// Returns the list of index values for an Indexed PictFormat.
/// Since we only support TrueColor/DirectColor formats, return an empty list.
pub(crate) fn handle_query_pict_index_values(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let _format = read_u32_bo(data, 4, bo);

    // Reply with 0 index values (we don't have indexed formats)
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    write_u16_bo(&mut reply, 2, seq, bo);
    // length = 0 extra words
    write_u32_bo(&mut reply, 4, 0, bo);
    // num_values = 0
    write_u32_bo(&mut reply, 8, 0, bo);
    reply.to_vec()
}
