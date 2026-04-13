//! Font handlers (opcodes 45-52).

use super::*;

// ---------------------------------------------------------------------------
// Opcode 45: OpenFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_open_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return build_error(BAD_LENGTH, state.sequence, 0, 45, 0);
    }
    let fid = state.read_u32(data, 4);

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(fid) {
        return build_error(BAD_ID_CHOICE, state.sequence, fid, 45, 0);
    }

    let name_len = state.read_u16(data, 8) as usize;
    let name = if 12 + name_len <= data.len() {
        String::from_utf8_lossy(&data[12..12 + name_len]).to_string()
    } else {
        "fixed".to_string()
    };
    debug!("OpenFont: fid={fid:#x} name={name}");
    state.font_manager.open_font(fid, &name);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 46: CloseFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_close_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, state.sequence, 0, 46, 0);
    }
    let fid = state.read_u32(data, 4);
    // Validate font exists
    if state.font_manager.get_font(fid).is_none() {
        return build_error(BAD_FONT, state.sequence, fid, 46, 0);
    }
    state.font_manager.close_font(fid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 47: QueryFont
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_font(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, seq, 0, 47, 0);
    }
    let fontable = state.read_u32(data, 4);

    // fontable can be a font ID or a GC ID (containing a font)
    let is_valid_fontable = state.font_manager.get_font(fontable).is_some()
        || state.gcs.contains_key(&fontable);

    if !is_valid_fontable {
        return build_error(BAD_FONT, seq, fontable, 47, 0);
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
            let reply_len = 60 + char_infos_bytes;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, ((reply_len - 32) / 4) as u32);
            // min_bounds: lsb=0, rsb=6, width=6, ascent=10, descent=3
            state.write_i16(&mut reply, 8, 0);   // min lsb
            state.write_i16(&mut reply, 10, 6);   // min rsb
            state.write_i16(&mut reply, 12, 6);   // min width
            state.write_i16(&mut reply, 14, 10);  // min ascent
            state.write_i16(&mut reply, 16, 3);   // min descent
            // max_bounds (same for monospaced)
            state.write_i16(&mut reply, 24, 0);
            state.write_i16(&mut reply, 26, 6);
            state.write_i16(&mut reply, 28, 6);
            state.write_i16(&mut reply, 30, 10);
            state.write_i16(&mut reply, 32, 3);
            state.write_u16(&mut reply, 40, 32u16);  // min_char_or_byte2
            state.write_u16(&mut reply, 42, 126u16); // max_char_or_byte2
            state.write_u16(&mut reply, 44, 32u16);  // default_char
            state.write_u16(&mut reply, 46, 0u16);   // n_properties
            reply[48] = 0;   // draw_direction = LeftToRight
            reply[51] = 1;   // all_chars_exist
            state.write_i16(&mut reply, 52, 10i16);  // font_ascent
            state.write_i16(&mut reply, 54, 3i16);   // font_descent
            state.write_u32(&mut reply, 56, n_char_infos);
            // Fill per-character info (all same for monospaced fallback)
            let mut off = 60;
            for _ in 0..n_char_infos {
                state.write_i16(&mut reply, off, 0);      // lsb
                state.write_i16(&mut reply, off + 2, 6);   // rsb
                state.write_i16(&mut reply, off + 4, 6);   // width
                state.write_i16(&mut reply, off + 6, 10);  // ascent
                state.write_i16(&mut reply, off + 8, 3);   // descent
                state.write_u16(&mut reply, off + 10, 0);  // attributes
                off += 12;
            }
            return reply;
        }
    };

    // Build font properties: (atom, i32_value) pairs.
    // We derive pixel_size from ascent+descent, point_size = pixel_size * 10.
    let pixel_size = (font.font_ascent + font.font_descent) as i32;
    let point_size = pixel_size * 10;
    let char_width = font.max_bounds.character_width as i32;

    // Intern the property atoms.
    let prop_defs: Vec<(&str, i32)> = vec![
        ("PIXEL_SIZE", pixel_size),
        ("POINT_SIZE", point_size),
        ("RESOLUTION_X", 75),
        ("RESOLUTION_Y", 75),
        ("WEIGHT", 10), // medium weight
        ("X_HEIGHT", (font.font_ascent as i32 * 2) / 3),
        ("QUAD_WIDTH", char_width),
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

    let reply_len = 60 + props_bytes + char_infos_bytes;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    state.write_u16(&mut reply, 2, seq);
    let extra_words = ((reply_len - 32) / 4) as u32;
    state.write_u32(&mut reply, 4, extra_words);

    // min_bounds at offset 8 (12 bytes)
    {
        let ci = &font.min_bounds;
        state.write_i16(&mut reply, 8, ci.left_side_bearing);
        state.write_i16(&mut reply, 10, ci.right_side_bearing);
        state.write_i16(&mut reply, 12, ci.character_width);
        state.write_i16(&mut reply, 14, ci.ascent);
        state.write_i16(&mut reply, 16, ci.descent);
        state.write_u16(&mut reply, 18, ci.attributes);
    }
    // pad at 20..24

    // max_bounds at offset 24 (12 bytes)
    {
        let ci = &font.max_bounds;
        state.write_i16(&mut reply, 24, ci.left_side_bearing);
        state.write_i16(&mut reply, 26, ci.right_side_bearing);
        state.write_i16(&mut reply, 28, ci.character_width);
        state.write_i16(&mut reply, 30, ci.ascent);
        state.write_i16(&mut reply, 32, ci.descent);
        state.write_u16(&mut reply, 34, ci.attributes);
    }
    // pad at 36..40

    state.write_u16(&mut reply, 40, font.min_char);
    state.write_u16(&mut reply, 42, font.max_char);
    state.write_u16(&mut reply, 44, font.default_char);
    state.write_u16(&mut reply, 46, n_properties);
    reply[48] = 0; // draw_direction = LeftToRight
    reply[49] = 0; // min_byte1
    reply[50] = 0; // max_byte1
    reply[51] = if font.char_infos.len() == n_char_infos as usize {
        1
    } else {
        0
    }; // all_chars_exist
    state.write_i16(&mut reply, 52, font.font_ascent);
    state.write_i16(&mut reply, 54, font.font_descent);
    state.write_u32(&mut reply, 56, n_char_infos);

    // Font properties at offset 60 (each FONTPROP = 4-byte atom + 4-byte value)
    let mut off = 60;
    for (atom, value) in &props {
        state.write_u32(&mut reply, off, *atom);
        state.write_u32(&mut reply, off + 4, *value as u32);
        off += 8;
    }

    // Char infos follow properties
    for ci in &font.char_infos {
        if off + 12 <= reply.len() {
            state.write_i16(&mut reply, off, ci.left_side_bearing);
            state.write_i16(&mut reply, off + 2, ci.right_side_bearing);
            state.write_i16(&mut reply, off + 4, ci.character_width);
            state.write_i16(&mut reply, off + 6, ci.ascent);
            state.write_i16(&mut reply, off + 8, ci.descent);
            state.write_u16(&mut reply, off + 10, ci.attributes);
            off += 12;
        }
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 48: QueryTextExtents
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_text_extents(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, seq, 0, 48, 0);
    }

    let fontable = state.read_u32(data, 4);

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
        // Calculate width from the text in the request
        // Text starts at offset 8, each char is 2 bytes (CHAR2B format)
        let odd_length = data[1] != 0;
        let text_bytes = data.len() - 8;
        let char_count = if odd_length {
            (text_bytes - 2) / 2
        } else {
            text_bytes / 2
        };
        let mut width: i32 = 0;
        let mut left: i32 = 0;
        let mut right: i32 = 0;
        let mut pos: i32 = 0;
        for i in 0..char_count {
            let _byte1 = data[8 + i * 2];
            let byte2 = data[8 + i * 2 + 1];
            let ci = font.char_info(byte2 as u16);
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
        (font.font_ascent, font.font_descent, width as i16, left as i16, right as i16)
    } else {
        (12i16, 4i16, 0i16, 0i16, 0i16)
    };

    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_i16(&mut reply, 8, ascent); // font_ascent
    state.write_i16(&mut reply, 10, descent); // font_descent
    state.write_i16(&mut reply, 12, ascent); // overall_ascent
    state.write_i16(&mut reply, 14, descent); // overall_descent
    state.write_u32(&mut reply, 16, overall_width as i32 as u32); // overall_width
    state.write_u32(&mut reply, 20, overall_left as i32 as u32); // overall_left
    state.write_u32(&mut reply, 24, overall_right as i32 as u32); // overall_right
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 49: ListFonts
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_fonts(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, seq, 0, 49, 0);
    }
    // Parse the request: max_names (2 bytes), pattern_len (2 bytes), pattern
    let max_names = state.read_u16(data, 4);
    let pattern_len = state.read_u16(data, 6) as usize;
    let pattern = if pattern_len > 0 && data.len() >= 8 + pattern_len {
        String::from_utf8_lossy(&data[8..8 + pattern_len]).to_string()
    } else {
        "*".to_string()
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

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1; // Reply
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (padded / 4) as u32);
    state.write_u16(&mut reply, 8, names.len() as u16);
    reply[32..32 + str_data.len()].copy_from_slice(&str_data);

    reply
}

// ---------------------------------------------------------------------------
// Opcode 50: ListFontsWithInfo
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_fonts_with_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Parse request: max_names(2) + pattern_length(2) + pattern(variable)
    if data.len() < 8 {
        return build_error(BAD_LENGTH, seq, 0, 50, 0);
    }
    let max_names = state.read_u16(data, 4);
    let pattern_len = state.read_u16(data, 6) as usize;
    let pattern = if pattern_len > 0 && data.len() >= 8 + pattern_len {
        std::str::from_utf8(&data[8..8 + pattern_len]).unwrap_or("*")
    } else {
        "*"
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
        let extra = props_bytes + name_len + name_pad;
        let reply_len_units = (extra / 4) as u32 + 7; // 7 for the 28 bytes after header

        let total = 32 + 28 + extra;
        let mut reply = vec![0u8; total];
        reply[0] = 1; // Reply
        reply[1] = name_len as u8;
        state.write_u16(&mut reply, 2, seq);
        state.write_u32(&mut reply, 4, reply_len_units);

        if let Some(f) = &font {
            // min_bounds at offset 8
            state.write_i16(&mut reply, 8, f.min_bounds.left_side_bearing);
            state.write_i16(&mut reply, 10, f.min_bounds.right_side_bearing);
            state.write_i16(&mut reply, 12, f.min_bounds.character_width);
            state.write_i16(&mut reply, 14, f.min_bounds.ascent);
            state.write_i16(&mut reply, 16, f.min_bounds.descent);
            // pad at 20..24
            // max_bounds at offset 24
            state.write_i16(&mut reply, 24, f.max_bounds.left_side_bearing);
            state.write_i16(&mut reply, 26, f.max_bounds.right_side_bearing);
            state.write_i16(&mut reply, 28, f.max_bounds.character_width);
            state.write_i16(&mut reply, 30, f.max_bounds.ascent);
            // Continue after header
            state.write_i16(&mut reply, 32, f.max_bounds.descent);
            // pad at 36..40
            state.write_u16(&mut reply, 40, f.min_char);
            state.write_u16(&mut reply, 42, f.max_char);
            state.write_u16(&mut reply, 44, f.default_char);
            state.write_u16(&mut reply, 46, n_properties);
            reply[48] = 0; // draw_direction
            state.write_i16(&mut reply, 52, f.font_ascent);
            state.write_i16(&mut reply, 54, f.font_descent);
        }

        let replies_remaining = (remaining as usize - i - 1) as u32;
        state.write_u32(&mut reply, 56, replies_remaining);

        // Font properties at offset 60 (each FONTPROP = 4-byte atom + 4-byte value)
        let mut off = 60;
        for (atom, value) in &props {
            state.write_u32(&mut reply, off, *atom);
            state.write_u32(&mut reply, off + 4, *value as u32);
            off += 8;
        }

        // Name after properties
        let name_off = 60 + props_bytes;
        if name_off + name_len <= reply.len() {
            reply[name_off..name_off + name_len].copy_from_slice(&name_bytes[..name_len]);
        }

        all_replies.extend_from_slice(&reply);
    }

    // Terminator reply: name_length=0
    let mut term = vec![0u8; 60];
    term[0] = 1; // Reply
    term[1] = 0; // name_length = 0 → last reply
    state.write_u16(&mut term, 2, seq);
    state.write_u32(&mut term, 4, 7u32);
    all_replies.extend_from_slice(&term);

    all_replies
}

// ---------------------------------------------------------------------------
// Opcode 52: GetFontPath
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_font_path(state: &ClientState, seq: u16) -> Vec<u8> {
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
    let extra_words = (padded_len / 4) as u32;

    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, extra_words);
    state.write_u16(&mut reply, 8, state.font_path.len() as u16);
    reply[32..32 + path_data.len()].copy_from_slice(&path_data);
    reply
}

pub(crate) fn handle_set_font_path(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, state.sequence, 0, 51, 0);
    }
    let num_paths = state.read_u16(data, 4) as usize;
    let mut paths = Vec::with_capacity(num_paths);
    let mut off = 8;
    for _ in 0..num_paths {
        if off >= data.len() {
            break;
        }
        let len = data[off] as usize;
        off += 1;
        if off + len > data.len() {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[off..off + len]) {
            paths.push(s.to_string());
        }
        off += len;
    }
    debug!("SetFontPath: {} paths", paths.len());
    state.font_path = paths;
    // Reload fonts from new paths
    state.font_manager.reload_paths(&state.font_path);
    Vec::new()
}
