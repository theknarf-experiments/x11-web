use tracing::debug;
use x11rb_protocol::protocol::render::{
    Directformat, PictType, Pictdepth, Pictforminfo, Pictscreen, Pictvisual,
};
use x11rb_protocol::x11_utils::Serialize;

use super::{
    resolve_source_pixels, PictFilter, PictureState, PICTFORMAT_A1, PICTFORMAT_A8,
    PICTFORMAT_ARGB32, PICTFORMAT_RGB24, PICTFORMAT_XBGR32, PICTFORMAT_XRGB32,
};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_i16_bo, read_u16_bo, read_u32_bo, ROOT_VISUAL};
use crate::xserver::reply::ReplyBuf;
use crate::xserver::ClientState;

/// QueryVersion: reply with version 0.11
pub(crate) fn handle_query_version(seq: u16, bo: bool) -> Vec<u8> {
    ReplyBuf::fixed(seq, bo)
        .set_u32(8, 0) // major version
        .set_u32(12, 11) // minor version
        .build()
}

/// QueryPictFormats: reply with ARGB32, RGB24, A8, A1, xRGB32, xBGR32
/// formats + screen info
pub(crate) fn handle_query_pict_formats(seq: u16, bo: bool) -> Vec<u8> {
    // We define 6 formats: ARGB32, RGB24, A8, A1, xRGB32, xBGR32.
    // The two `x*` formats are needed for the rendercheck
    // libreoffice / gtk byte-swap tests; they share the depth-32
    // pixmap layout with ARGB32 but treat the high byte as padding
    // (xRGB) or swap R/B (xBGR).
    let num_subpixel: u32 = 1;

    let formats = [
        // Format 1: ARGB32 (type=PictTypeDirect, depth=32)
        Pictforminfo {
            id: PICTFORMAT_ARGB32,
            type_: PictType::DIRECT,
            depth: 32,
            direct: Directformat {
                red_shift: 16, red_mask: 0xFF,
                green_shift: 8, green_mask: 0xFF,
                blue_shift: 0, blue_mask: 0xFF,
                alpha_shift: 24, alpha_mask: 0xFF,
            },
            colormap: 0,
        },
        // Format 2: RGB24 (type=PictTypeDirect, depth=24)
        Pictforminfo {
            id: PICTFORMAT_RGB24,
            type_: PictType::DIRECT,
            depth: 24,
            direct: Directformat {
                red_shift: 16, red_mask: 0xFF,
                green_shift: 8, green_mask: 0xFF,
                blue_shift: 0, blue_mask: 0xFF,
                alpha_shift: 0, alpha_mask: 0,
            },
            colormap: 0,
        },
        // Format 3: A8 (type=PictTypeDirect, depth=8, alpha only)
        Pictforminfo {
            id: PICTFORMAT_A8,
            type_: PictType::DIRECT,
            depth: 8,
            direct: Directformat {
                red_shift: 0, red_mask: 0,
                green_shift: 0, green_mask: 0,
                blue_shift: 0, blue_mask: 0,
                alpha_shift: 0, alpha_mask: 0xFF,
            },
            colormap: 0,
        },
        // Format 4: A1 (type=PictTypeDirect, depth=1, 1-bit alpha)
        Pictforminfo {
            id: PICTFORMAT_A1,
            type_: PictType::DIRECT,
            depth: 1,
            direct: Directformat {
                red_shift: 0, red_mask: 0,
                green_shift: 0, green_mask: 0,
                blue_shift: 0, blue_mask: 0,
                alpha_shift: 0, alpha_mask: 0x1,
            },
            colormap: 0,
        },
        // Format 5: xRGB32 -- depth 32, R/G/B in the same byte positions
        // as ARGB32 but the high byte is padding (alphaMask = 0). The
        // rendercheck libreoffice test wants this to verify that the
        // server doesn't peek at the unused byte.
        Pictforminfo {
            id: PICTFORMAT_XRGB32,
            type_: PictType::DIRECT,
            depth: 32,
            direct: Directformat {
                red_shift: 16, red_mask: 0xFF,
                green_shift: 8, green_mask: 0xFF,
                blue_shift: 0, blue_mask: 0xFF,
                alpha_shift: 0, alpha_mask: 0,
            },
            colormap: 0,
        },
        // Format 6: xBGR32 -- depth 32 with R/B swapped (R at byte 0, B
        // at byte 2). The rendercheck gtk test exercises this layout
        // against ARGB32 to verify that the server reads each picture
        // through its declared format rather than blindly assuming a
        // canonical byte order.
        Pictforminfo {
            id: PICTFORMAT_XBGR32,
            type_: PictType::DIRECT,
            depth: 32,
            direct: Directformat {
                red_shift: 0, red_mask: 0xFF,
                green_shift: 8, green_mask: 0xFF,
                blue_shift: 16, blue_mask: 0xFF,
                alpha_shift: 0, alpha_mask: 0,
            },
            colormap: 0,
        },
    ];

    let screens = [Pictscreen {
        fallback: PICTFORMAT_RGB24,
        depths: vec![
            Pictdepth {
                depth: 24,
                visuals: vec![Pictvisual {
                    visual: ROOT_VISUAL,
                    format: PICTFORMAT_RGB24,
                }],
            },
            Pictdepth {
                depth: 32,
                visuals: vec![],
            },
        ],
    }];

    // Count totals for the reply header fields
    let num_depths: u32 = screens.iter().map(|s| s.depths.len() as u32).sum();
    let num_visuals: u32 = screens
        .iter()
        .flat_map(|s| &s.depths)
        .map(|d| d.visuals.len() as u32)
        .sum();

    // NOTE: x11rb's Serialize uses native endian (LE on our platform).
    // If `bo` (msb_first) is true, the ReplyBuf header will be BE but
    // the struct data below will still be LE. This is a known limitation --
    // virtually all X11 clients use LE, so this is acceptable for now.
    let mut extra_data: Vec<u8> = Vec::new();
    for f in &formats {
        f.serialize_into(&mut extra_data);
    }
    for s in &screens {
        s.serialize_into(&mut extra_data);
    }
    // Subpixel order: 0 = Unknown
    0u32.serialize_into(&mut extra_data);

    ReplyBuf::with_extra(seq, extra_data.len(), bo)
        .set_u32(8, formats.len() as u32) // num_formats
        .set_u32(12, screens.len() as u32) // num_screens
        .set_u32(16, num_depths) // num_depths (total across screens)
        .set_u32(20, num_visuals) // num_visuals (total across all depths)
        .set_u32(24, num_subpixel) // num_subpixel
        .set_bytes(32, &extra_data)
        .build()
}

pub(crate) fn handle_create_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 20, seq, 139, data[1] as u16, bo);
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
            crate::xserver::core::DRAWABLE_ERROR,
            seq,
            drawable,
            139,
            data[1] as u16,
            bo,
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
                crate::xserver::core::MATCH_ERROR,
                seq,
                format_id,
                139,
                data[1] as u16,
                bo,
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
            crate::xserver::core::MATCH_ERROR,
            seq,
            format_id,
            139,
            data[1] as u16,
            bo,
        );
    }

    let mut repeat = 0u32;
    let mut component_alpha = false;
    let mut clip_mask: Option<u32> = None;
    let mut clip_origin_x: i16 = 0;
    let mut clip_origin_y: i16 = 0;
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
                4 => {
                    clip_origin_x = val as i16;
                    debug!("  clip_origin_x={clip_origin_x}");
                }
                5 => {
                    clip_origin_y = val as i16;
                    debug!("  clip_origin_y={clip_origin_y}");
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
            clip_origin_x,
            clip_origin_y,
            clip_mask,
            filter: PictFilter::Nearest,
        },
    );
    Vec::new()
}

pub(crate) fn handle_change_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 12, seq, 139, data[1] as u16, bo);
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
                    4 => {
                        pic.clip_origin_x = val as i16;
                        debug!("  clip_origin_x={}", pic.clip_origin_x);
                    }
                    5 => {
                        pic.clip_origin_y = val as i16;
                        debug!("  clip_origin_y={}", pic.clip_origin_y);
                    }
                    6 => {
                        pic.clip_mask = if val == 0 { None } else { Some(val) };
                        // Reset clip rects when clip mask changes
                        if val == 0 {
                            pic.clip_rects = None;
                        }
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

pub(crate) fn handle_set_picture_clip_rectangles(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 12, seq, 139, data[1] as u16, bo);
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
        state.recycle_xid(pid);
    }
    Vec::new()
}

/// CreateCursor (RENDER minor opcode 27).
/// Creates a cursor from a RENDER picture. Renders the source picture
/// to an ARGB bitmap and stores it as a CursorInfo for later use.
pub(crate) fn handle_create_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use crate::xserver::types::CursorInfo;

    let bo = state.msb_first;
    require_len!(data, 16, seq, 139, data[1] as u16, bo);
    let cursor_id = read_u32_bo(data, 4, bo);
    let src_picture = read_u32_bo(data, 8, bo);
    let hotspot_x = read_u16_bo(data, 12, bo);
    let hotspot_y = read_u16_bo(data, 14, bo);

    debug!("Render CreateCursor: cursor_id={cursor_id:#x} src_pic={src_picture:#x} hotspot=({hotspot_x},{hotspot_y})");

    // Get the source picture's drawable dimensions to know cursor size
    let (width, height) = if let Some(pic) = state.render.pictures.get(&src_picture) {
        let d = pic.drawable;
        if let Some(px) = state.pixmaps.get(&d) {
            (
                px.framebuffer.width() as u16,
                px.framebuffer.height() as u16,
            )
        } else if let Some(win) = state.windows.get(&d) {
            (
                win.framebuffer.width() as u16,
                win.framebuffer.height() as u16,
            )
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
    state.cursor_info.insert(
        cursor_id,
        CursorInfo {
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
        },
    );
    Vec::new()
}

/// CreateAnimCursor (RENDER minor opcode 31).
/// Creates an animated cursor from a list of cursor/delay pairs.
/// Collects ARGB bitmap data from each referenced cursor to build
/// a full animation sequence sent to the frontend via CursorAnimated.
pub(crate) fn handle_create_anim_cursor(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 8, seq, 139, data[1] as u16, bo);
    let cursor_id = read_u32_bo(data, 4, bo);
    let num_frames = (data.len() - 8) / 8; // each frame: cursor_id(4) + delay(4)
    debug!("Render CreateAnimCursor: cursor_id={cursor_id:#x} frames={num_frames}");

    // Collect animation frame data from each referenced cursor.
    let mut anim_frames: Vec<(Vec<u8>, u16, u16, u16, u16, u32)> = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let off = 8 + i * 8;
        if off + 8 > data.len() {
            break;
        }
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
        state.cursor_info.insert(
            cursor_id,
            CursorInfo {
                css_name: String::new(),
                source_pixmap: 0,
                mask_pixmap: 0,
                fore_red: 0,
                fore_green: 0,
                fore_blue: 0,
                back_red: 0,
                back_green: 0,
                back_blue: 0,
                hotspot_x: *hx,
                hotspot_y: *hy,
                argb_data: argb.clone(),
                width: *w,
                height: *h,
                name: String::new(),
                anim_frames,
            },
        );
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
pub(crate) fn handle_query_pict_index_values(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 8, seq, 139, data[1] as u16, bo);
    let _format = read_u32_bo(data, 4, bo);

    // Reply with 0 index values (we don't have indexed formats)
    ReplyBuf::fixed(seq, bo)
        .set_u32(8, 0) // num_values = 0
        .build()
}
