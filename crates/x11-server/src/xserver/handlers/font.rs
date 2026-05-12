//! Font handlers (opcodes 45-52).

use super::*;
use crate::fonts::types::CharInfo as FontCharInfo;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xproto::{
    Charinfo, CloseFontRequest, FontDraw, Fontprop, GetFontPathReply, GetFontPathRequest,
    ListFontsReply, ListFontsRequest, ListFontsWithInfoReply, ListFontsWithInfoRequest,
    OpenFontRequest, QueryFontReply, QueryFontRequest, QueryTextExtentsReply,
    QueryTextExtentsRequest, SetFontPathRequest, Str,
};

fn charinfo_from(ci: &FontCharInfo) -> Charinfo {
    Charinfo {
        left_side_bearing: ci.left_side_bearing,
        right_side_bearing: ci.right_side_bearing,
        character_width: ci.character_width,
        ascent: ci.ascent,
        descent: ci.descent,
        attributes: ci.attributes,
    }
}

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
            // Font ID or GC is valid but no font data available — return
            // reasonable defaults that approximate a 6x13 fixed font so apps
            // can still lay out text correctly.
            let bounds = Charinfo {
                left_side_bearing: 0,
                right_side_bearing: 6,
                character_width: 6,
                ascent: 10,
                descent: 3,
                attributes: 0,
            };
            let char_infos = (32..=126u16).map(|_| bounds).collect();
            return serialize_var_reply(
                &QueryFontReply {
                    sequence: seq,
                    length: 0,
                    min_bounds: bounds,
                    max_bounds: bounds,
                    min_char_or_byte2: 32,
                    max_char_or_byte2: 126,
                    default_char: 32,
                    draw_direction: FontDraw::LEFT_TO_RIGHT,
                    min_byte1: 0,
                    max_byte1: 0,
                    all_chars_exist: true,
                    font_ascent: 10,
                    font_descent: 3,
                    properties: Vec::new(),
                    char_infos,
                },
                state.byte_order(),
            );
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

    let properties: Vec<Fontprop> = {
        let mut atoms = state.atoms.lock().unwrap();
        prop_defs
            .iter()
            .map(|(name, val)| Fontprop {
                name: atoms.intern(name, false),
                value: *val as u32,
            })
            .collect()
    };

    let n_char_infos = (font.max_char - font.min_char + 1) as usize;
    let all_chars = font.char_infos.len() == n_char_infos;

    let char_infos: Vec<Charinfo> = font.char_infos.iter().map(charinfo_from).collect();

    serialize_var_reply(
        &QueryFontReply {
            sequence: seq,
            length: 0,
            min_bounds: charinfo_from(&font.min_bounds),
            max_bounds: charinfo_from(&font.max_bounds),
            min_char_or_byte2: font.min_char,
            max_char_or_byte2: font.max_char,
            default_char: font.default_char,
            draw_direction: FontDraw::LEFT_TO_RIGHT,
            min_byte1: 0,
            max_byte1: 0,
            all_chars_exist: all_chars,
            font_ascent: font.font_ascent,
            font_descent: font.font_descent,
            properties,
            char_infos,
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 48: QueryTextExtents
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_text_extents(
    state: &mut ClientState,
    req: &QueryTextExtentsRequest,
) -> Vec<u8> {
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

    serialize_reply(
        &QueryTextExtentsReply {
            draw_direction: FontDraw::LEFT_TO_RIGHT,
            sequence: seq,
            length: 0,
            font_ascent: ascent,
            font_descent: descent,
            overall_ascent: ascent,
            overall_descent: descent,
            overall_width: overall_width as i32,
            overall_left: overall_left as i32,
            overall_right: overall_right as i32,
        },
        state.byte_order(),
    )
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

    let names: Vec<Str> = state
        .font_manager
        .list_fonts(&pattern, max_names)
        .into_iter()
        .map(|n| Str {
            name: n.into_bytes(),
        })
        .collect();

    serialize_var_reply(
        &ListFontsReply {
            sequence: seq,
            length: 0,
            names,
        },
        state.byte_order(),
    )
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
    let total = font_names.len() as u32;
    let mut all_replies = Vec::new();
    let bo = state.byte_order();

    for (i, name) in font_names.iter().enumerate() {
        let font = state.font_manager.get_font_by_name(name);

        let properties: Vec<Fontprop> = if let Some(f) = &font {
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
                .map(|(pname, val)| Fontprop {
                    name: atoms.intern(pname, false),
                    value: *val as u32,
                })
                .collect()
        } else {
            Vec::new()
        };

        let name_bytes: Vec<u8> = name.as_bytes().iter().take(255).copied().collect();
        let replies_hint = (total as usize - i - 1) as u32;

        let reply = if let Some(f) = &font {
            ListFontsWithInfoReply {
                sequence: seq,
                length: 0,
                min_bounds: charinfo_from(&f.min_bounds),
                max_bounds: charinfo_from(&f.max_bounds),
                min_char_or_byte2: f.min_char,
                max_char_or_byte2: f.max_char,
                default_char: f.default_char,
                draw_direction: FontDraw::LEFT_TO_RIGHT,
                min_byte1: 0,
                max_byte1: 0,
                all_chars_exist: false,
                font_ascent: f.font_ascent,
                font_descent: f.font_descent,
                replies_hint,
                properties,
                name: name_bytes,
            }
        } else {
            ListFontsWithInfoReply {
                sequence: seq,
                length: 0,
                min_bounds: Charinfo::default(),
                max_bounds: Charinfo::default(),
                min_char_or_byte2: 0,
                max_char_or_byte2: 0,
                default_char: 0,
                draw_direction: FontDraw::LEFT_TO_RIGHT,
                min_byte1: 0,
                max_byte1: 0,
                all_chars_exist: false,
                font_ascent: 0,
                font_descent: 0,
                replies_hint,
                properties,
                name: name_bytes,
            }
        };

        all_replies.extend_from_slice(&serialize_var_reply(&reply, bo));
    }

    // Terminator reply: name is empty → name_len = 0 signals end-of-list.
    let term = ListFontsWithInfoReply {
        sequence: seq,
        length: 0,
        min_bounds: Charinfo::default(),
        max_bounds: Charinfo::default(),
        min_char_or_byte2: 0,
        max_char_or_byte2: 0,
        default_char: 0,
        draw_direction: FontDraw::LEFT_TO_RIGHT,
        min_byte1: 0,
        max_byte1: 0,
        all_chars_exist: false,
        font_ascent: 0,
        font_descent: 0,
        replies_hint: 0,
        properties: Vec::new(),
        name: Vec::new(),
    };
    all_replies.extend_from_slice(&serialize_var_reply(&term, bo));

    all_replies
}

// ---------------------------------------------------------------------------
// Opcode 52: GetFontPath
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_font_path(state: &ClientState, _req: &GetFontPathRequest) -> Vec<u8> {
    let seq = state.sequence;
    let path: Vec<Str> = state
        .font_path
        .iter()
        .filter(|p| p.len() <= 255)
        .map(|p| Str {
            name: p.as_bytes().to_vec(),
        })
        .collect();

    serialize_var_reply(
        &GetFontPathReply {
            sequence: seq,
            length: 0,
            path,
        },
        state.byte_order(),
    )
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
