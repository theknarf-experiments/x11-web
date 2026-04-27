//! Font handlers (opcodes 45-52).

use super::*;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xproto::{
    CloseFontRequest, GetFontPathRequest, ListFontsRequest, ListFontsWithInfoRequest,
    OpenFontRequest, QueryFontRequest, QueryTextExtentsRequest, SetFontPathRequest,
};

// ---------------------------------------------------------------------------
// Opcode 45: OpenFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_open_font(state: &mut ClientState, req: &OpenFontRequest) -> Vec<u8> {
    let fid = req.fid;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(fid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, fid, 45, 0);
    }
    // Per X11 spec: ID_CHOICE_ERROR if the font ID is already in use
    if state.font_manager.get_font(fid).is_some() {
        return build_error(ID_CHOICE_ERROR, state.sequence, fid, 45, 0);
    }

    let name = String::from_utf8_lossy(&req.name).to_string();
    debug!("OpenFont: fid={fid:#x} name={name}");
    state.font_manager.open_font(fid, &name);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 46: CloseFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_close_font(state: &mut ClientState, req: &CloseFontRequest) -> Vec<u8> {
    let fid = req.font;
    // Validate font exists
    if state.font_manager.get_font(fid).is_none() {
        return build_error(FONT_ERROR, state.sequence, fid, 46, 0);
    }
    state.font_manager.close_font(fid);
    state.recycle_xid(fid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 47: QueryFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_font(state: &mut ClientState, req: &QueryFontRequest) -> Vec<u8> {
    let seq = state.sequence;
    let fontable = req.font;

    // fontable can be a font ID or a GC ID (containing a font)
    let is_valid_fontable =
        state.font_manager.get_font(fontable).is_some() || state.gcs.contains_key(&fontable);

    if !is_valid_fontable {
        return build_error(FONT_ERROR, seq, fontable, 47, 0);
    }

    let font = state
        .font_manager
        .get_font(fontable)
        .or_else(|| {
            let gc = state.gcs.get(&fontable)?;
            state.font_manager.get_font(gc.font_id)
        })
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => {
            // Font ID or GC is valid but no font data available -- return
            // reasonable defaults that approximate a 6x13 fixed font so apps
            // can still lay out text correctly.
            let n_char_infos: u32 = 95; // 32..126
            let char_infos_bytes = n_char_infos as usize * 12;
            let extra = 28 + char_infos_bytes; // 60 - 32 = 28 header extra + char infos
            let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
                // min_bounds: lsb=0, rsb=6, width=6, ascent=10, descent=3
                .set_i16(8, 0)   // min lsb
                .set_i16(10, 6)  // min rsb
                .set_i16(12, 6)  // min width
                .set_i16(14, 10) // min ascent
                .set_i16(16, 3)  // min descent
                // max_bounds (same for monospaced)
                .set_i16(24, 0)
                .set_i16(26, 6)
                .set_i16(28, 6)
                .set_i16(30, 10)
                .set_i16(32, 3)
                .set_u16(40, 32u16)  // min_char_or_byte2
                .set_u16(42, 126u16) // max_char_or_byte2
                .set_u16(44, 32u16)  // default_char
                .set_u16(46, 0u16)   // n_properties
                .set_u8(48, 0)       // draw_direction = LeftToRight
                .set_u8(51, 1)       // all_chars_exist
                .set_i16(52, 10i16)  // font_ascent
                .set_i16(54, 3i16)   // font_descent
                .set_u32(56, n_char_infos);
            // Fill per-character info (all same for monospaced fallback)
            let mut off = 60;
            for _ in 0..n_char_infos {
                reply = reply
                    .set_i16(off, 0)      // lsb
                    .set_i16(off + 2, 6)  // rsb
                    .set_i16(off + 4, 6)  // width
                    .set_i16(off + 6, 10) // ascent
                    .set_i16(off + 8, 3)  // descent
                    .set_u16(off + 10, 0); // attributes
                off += 12;
            }
            return reply.build();
        }
    };

    // Build font properties: (atom, i32_value) pairs.
    // We derive pixel_size from ascent+descent, point_size = pixel_size * 10.
    let pixel_size = (font.font_ascent + font.font_descent) as i32;
    let point_size = pixel_size * 10;
    let char_width = font.max_bounds.character_width as i32;

    // Parse XLFD name components for additional properties.
    // XLFD format: -foundry-family-weight-slant-setwidth-addstyle-pixel-point-resx-resy-spacing-avgwidth-registry-encoding
    let xlfd_parts: Vec<&str> = font.name.split('-').collect();
    let (xlfd_weight, xlfd_spacing, xlfd_avg_width) = if xlfd_parts.len() >= 15 {
        let w = match xlfd_parts[3].to_lowercase().as_str() {
            "bold" | "demibold" => 200,
            "medium" | "regular" | "" => 10,
            "light" => 5,
            _ => 10,
        };
        let spacing = match xlfd_parts[11].to_uppercase().as_str() {
            "M" | "C" => 1, // Monospaced / Cell
            "P" => 2,       // Proportional
            _ => 1,
        };
        let avg_w = xlfd_parts[12].parse::<i32>().unwrap_or(char_width * 10);
        (w, spacing, avg_w)
    } else {
        (10, 1, char_width * 10)
    };

    // Intern the property atoms. Per X11 spec, these match standard BDF/PCF properties.
    let prop_defs: Vec<(&str, i32)> = vec![
        ("PIXEL_SIZE", pixel_size),
        ("POINT_SIZE", point_size),
        ("RESOLUTION_X", 75),
        ("RESOLUTION_Y", 75),
        ("WEIGHT", xlfd_weight),
        ("X_HEIGHT", (font.font_ascent as i32 * 2) / 3),
        ("QUAD_WIDTH", char_width),
        ("CAP_HEIGHT", font.font_ascent as i32),
        ("FONT_ASCENT", font.font_ascent as i32),
        ("FONT_DESCENT", font.font_descent as i32),
        ("AVERAGE_WIDTH", xlfd_avg_width),
        ("SPACING", xlfd_spacing),
        ("MIN_SPACE", char_width),
        ("NORM_SPACE", char_width),
        ("MAX_SPACE", char_width),
        ("UNDERLINE_POSITION", -(font.font_descent as i32 / 2).max(1)),
        ("UNDERLINE_THICKNESS", 1),
    ];

    let props: Vec<(u32, i32)> = {
        let mut atoms = state.atoms.lock().unwrap();
        prop_defs
            .iter()
            .map(|(name, val)| (atoms.intern(name, false), *val))
            .collect()
    };

    let n_properties = props.len() as u16;
    let props_bytes = props.len() * 8; // each FONTPROP is 8 bytes

    let n_char_infos = (font.max_char - font.min_char + 1) as u32;
    let char_infos_bytes = n_char_infos as usize * 12;

    let extra = 28 + props_bytes + char_infos_bytes; // 60 - 32 = 28 header extra
    let all_chars = if font.char_infos.len() == n_char_infos as usize { 1u8 } else { 0u8 };
    let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
        // min_bounds at offset 8 (12 bytes)
        .set_i16(8, font.min_bounds.left_side_bearing)
        .set_i16(10, font.min_bounds.right_side_bearing)
        .set_i16(12, font.min_bounds.character_width)
        .set_i16(14, font.min_bounds.ascent)
        .set_i16(16, font.min_bounds.descent)
        .set_u16(18, font.min_bounds.attributes)
        // pad at 20..24
        // max_bounds at offset 24 (12 bytes)
        .set_i16(24, font.max_bounds.left_side_bearing)
        .set_i16(26, font.max_bounds.right_side_bearing)
        .set_i16(28, font.max_bounds.character_width)
        .set_i16(30, font.max_bounds.ascent)
        .set_i16(32, font.max_bounds.descent)
        .set_u16(34, font.max_bounds.attributes)
        // pad at 36..40
        .set_u16(40, font.min_char)
        .set_u16(42, font.max_char)
        .set_u16(44, font.default_char)
        .set_u16(46, n_properties)
        .set_u8(48, 0)           // draw_direction = LeftToRight
        .set_u8(49, 0)           // min_byte1
        .set_u8(50, 0)           // max_byte1
        .set_u8(51, all_chars)   // all_chars_exist
        .set_i16(52, font.font_ascent)
        .set_i16(54, font.font_descent)
        .set_u32(56, n_char_infos);

    // Font properties at offset 60 (each FONTPROP = 4-byte atom + 4-byte value)
    let mut off = 60;
    for (atom, value) in &props {
        reply = reply
            .set_u32(off, *atom)
            .set_u32(off + 4, *value as u32);
        off += 8;
    }

    // Char infos follow properties
    let buf = reply.buf_mut();
    for ci in &font.char_infos {
        if off + 12 <= buf.len() {
            state.write_i16(buf, off, ci.left_side_bearing);
            state.write_i16(buf, off + 2, ci.right_side_bearing);
            state.write_i16(buf, off + 4, ci.character_width);
            state.write_i16(buf, off + 6, ci.ascent);
            state.write_i16(buf, off + 8, ci.descent);
            state.write_u16(buf, off + 10, ci.attributes);
            off += 12;
        }
    }

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 48: QueryTextExtents
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_text_extents(state: &mut ClientState, req: &QueryTextExtentsRequest) -> Vec<u8> {
    let seq = state.sequence;
    let fontable = req.font;

    // Try to get actual font metrics
    let font = state
        .font_manager
        .get_font(fontable)
        .or_else(|| {
            let gc = state.gcs.get(&fontable)?;
            state.font_manager.get_font(gc.font_id)
        })
        .or_else(|| state.font_manager.get_default_font());

    let (ascent, descent, overall_width, overall_left, overall_right) = if let Some(font) = &font {
        // Each char is 2 bytes (CHAR2B format) — req.string is &[Char2b].
        let mut width: i32 = 0;
        let mut left: i32 = 0;
        let mut right: i32 = 0;
        let mut pos: i32 = 0;
        for (i, ch) in req.string.iter().enumerate() {
            let ci = font.char_info(ch.byte2 as u16);
            let lbearing = ci.left_side_bearing as i32;
            let rbearing = ci.right_side_bearing as i32;
            let char_w = ci.character_width as i32;
            if i == 0 {
                left = lbearing;
            }
            let char_right = pos + rbearing;
            if i == 0 || char_right > right {
                right = char_right;
            }
            pos += char_w;
            width += char_w;
        }
        // If width extends past rightmost rbearing, use width
        if pos > right {
            right = pos;
        }
        (
            font.font_ascent,
            font.font_descent,
            width as i16,
            left as i16,
            right as i16,
        )
    } else {
        (12i16, 4i16, 0i16, 0i16, 0i16)
    };

    ReplyBuf::fixed(seq, state.msb_first)
        .set_i16(8, ascent)                            // font_ascent
        .set_i16(10, descent)                          // font_descent
        .set_i16(12, ascent)                           // overall_ascent
        .set_i16(14, descent)                          // overall_descent
        .set_u32(16, overall_width as i32 as u32)      // overall_width
        .set_u32(20, overall_left as i32 as u32)       // overall_left
        .set_u32(24, overall_right as i32 as u32)      // overall_right
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 49: ListFonts
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_fonts(state: &mut ClientState, req: &ListFontsRequest) -> Vec<u8> {
    let seq = state.sequence;
    let max_names = req.max_names;
    let pattern = if req.pattern.is_empty() {
        "*".to_string()
    } else {
        String::from_utf8_lossy(&req.pattern).to_string()
    };

    let names = state.font_manager.list_fonts(&pattern, max_names);

    // Build the STR list: each entry is 1-byte length + name bytes.
    let mut str_data: Vec<u8> = Vec::new();
    for name in &names {
        let nb = name.as_bytes();
        str_data.push(nb.len() as u8);
        str_data.extend_from_slice(nb);
    }
    // Pad to 4-byte boundary.
    let padded = (str_data.len() + 3) & !3;
    str_data.resize(padded, 0);

    ReplyBuf::with_extra(seq, padded, state.msb_first)
        .set_u16(8, names.len() as u16)
        .set_bytes(32, &str_data)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 50: ListFontsWithInfo
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_fonts_with_info(
    state: &mut ClientState,
    req: &ListFontsWithInfoRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let max_names = req.max_names;
    let pattern = if req.pattern.is_empty() {
        "*"
    } else {
        std::str::from_utf8(&req.pattern).unwrap_or("*")
    };

    let font_names = state.font_manager.list_fonts(pattern, max_names);

    // Build concatenated replies: one per font + terminator
    let mut all_replies = Vec::new();

    let remaining = font_names.len() as u32;
    for (i, name) in font_names.iter().enumerate() {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(255);

        // Try to get font info for this name
        let font = state.font_manager.get_font_by_name(name);

        // Build font properties (same as QueryFont) when font data is available
        let props: Vec<(u32, i32)> = if let Some(f) = &font {
            let pixel_size = (f.font_ascent + f.font_descent) as i32;
            let point_size = pixel_size * 10;
            let char_width = f.max_bounds.character_width as i32;
            let prop_defs: Vec<(&str, i32)> = vec![
                ("PIXEL_SIZE", pixel_size),
                ("POINT_SIZE", point_size),
                ("RESOLUTION_X", 75),
                ("RESOLUTION_Y", 75),
                ("WEIGHT", 10),
                ("X_HEIGHT", (f.font_ascent as i32 * 2) / 3),
                ("QUAD_WIDTH", char_width),
            ];
            let mut atoms = state.atoms.lock().unwrap();
            prop_defs
                .iter()
                .map(|(pname, val)| (atoms.intern(pname, false), *val))
                .collect()
        } else {
            Vec::new()
        };
        let n_properties = props.len() as u16;
        let props_bytes = props.len() * 8; // each FONTPROP is 8 bytes

        let name_pad = (4 - (name_len % 4)) % 4;
        let extra = 28 + props_bytes + name_len + name_pad; // 28 for header bytes at 32..60

        let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
            .set_data_byte(name_len as u8);

        if let Some(f) = &font {
            // min_bounds at offset 8
            reply = reply
                .set_i16(8, f.min_bounds.left_side_bearing)
                .set_i16(10, f.min_bounds.right_side_bearing)
                .set_i16(12, f.min_bounds.character_width)
                .set_i16(14, f.min_bounds.ascent)
                .set_i16(16, f.min_bounds.descent)
                // pad at 20..24
                // max_bounds at offset 24
                .set_i16(24, f.max_bounds.left_side_bearing)
                .set_i16(26, f.max_bounds.right_side_bearing)
                .set_i16(28, f.max_bounds.character_width)
                .set_i16(30, f.max_bounds.ascent)
                // Continue after header
                .set_i16(32, f.max_bounds.descent)
                // pad at 36..40
                .set_u16(40, f.min_char)
                .set_u16(42, f.max_char)
                .set_u16(44, f.default_char)
                .set_u16(46, n_properties)
                .set_u8(48, 0) // draw_direction
                .set_i16(52, f.font_ascent)
                .set_i16(54, f.font_descent);
        }

        let replies_remaining = (remaining as usize - i - 1) as u32;
        reply = reply.set_u32(56, replies_remaining);

        // Font properties at offset 60 (each FONTPROP = 4-byte atom + 4-byte value)
        let mut off = 60;
        for (atom, value) in &props {
            reply = reply
                .set_u32(off, *atom)
                .set_u32(off + 4, *value as u32);
            off += 8;
        }

        // Name after properties
        let name_off = 60 + props_bytes;
        if name_off + name_len <= reply.buf_mut().len() {
            reply.buf_mut()[name_off..name_off + name_len].copy_from_slice(&name_bytes[..name_len]);
        }

        all_replies.extend_from_slice(&reply.build());
    }

    // Terminator reply: name_length=0
    let term = ReplyBuf::with_extra(seq, 28, state.msb_first)
        .set_data_byte(0) // name_length = 0 → last reply
        .build();
    all_replies.extend_from_slice(&term);

    all_replies
}

// ---------------------------------------------------------------------------
// Opcode 52: GetFontPath
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_font_path(state: &ClientState, _req: &GetFontPathRequest) -> Vec<u8> {
    let seq = state.sequence;
    // Build STR8 list of font paths
    let mut path_data = Vec::new();
    for path in &state.font_path {
        let bytes = path.as_bytes();
        if bytes.len() <= 255 {
            path_data.push(bytes.len() as u8);
            path_data.extend_from_slice(bytes);
        }
    }
    let padded_len = (path_data.len() + 3) & !3;
    path_data.resize(padded_len, 0);

    ReplyBuf::with_extra(seq, padded_len, state.msb_first)
        .set_u16(8, state.font_path.len() as u16)
        .set_bytes(32, &path_data)
        .build()
}

pub(crate) fn handle_set_font_path(state: &mut ClientState, req: &SetFontPathRequest) -> Vec<u8> {
    let mut paths = Vec::with_capacity(req.font.len());
    for s in req.font.iter() {
        if let Ok(name) = std::str::from_utf8(&s.name) {
            paths.push(name.to_string());
        }
    }
    debug!("SetFontPath: {} paths", paths.len());
    state.font_path = paths;
    // Reload fonts from new paths
    state.font_manager.reload_paths(&state.font_path);
    Vec::new()
}
