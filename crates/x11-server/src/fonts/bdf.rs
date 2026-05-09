use std::io::Read;
use std::path::Path;
use tracing::debug;

use super::types::{BitmapFont, CharInfo, GlyphBitmap};

pub(super) fn load_bdf_font(path: &Path) -> Option<BitmapFont> {
    let data = std::fs::read(path).ok()?;
    parse_bdf_data(&data, path)
}

pub(super) fn load_bdf_gz_font(path: &Path) -> Option<BitmapFont> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;
    parse_bdf_data(&data, path)
}

pub(super) fn parse_bdf_data(data: &[u8], path: &Path) -> Option<BitmapFont> {
    let font = bdf_parser::BdfFont::parse(data).ok()?;

    // Try to use FONT property (XLFD name) from BDF if available
    let font_name = {
        // The bdf_parser crate stores the font name from the FONT line
        let bdf_name = font.metadata.name.to_string();
        if bdf_name.starts_with('-') {
            // It's an XLFD name - use it
            bdf_name
        } else if !bdf_name.is_empty() {
            bdf_name
        } else {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        }
    };

    let font_bb = font.metadata.bounding_box;
    let font_ascent = font_bb.offset.y + font_bb.size.y;
    let font_descent = -font_bb.offset.y;

    // Collect all glyph codes
    let mut min_char = u16::MAX;
    let mut max_char = 0u16;

    for glyph in font.glyphs.iter() {
        if let Some(encoding) = glyph.encoding {
            let code = encoding as u16;
            min_char = min_char.min(code);
            max_char = max_char.max(code);
        }
    }

    if min_char > max_char {
        return None;
    }

    let num_chars = (max_char - min_char + 1) as usize;
    let mut char_infos = vec![CharInfo::default(); num_chars];
    let mut glyphs = vec![
        GlyphBitmap {
            width: 0,
            height: 0,
            bitmap: Vec::new(),
        };
        num_chars
    ];

    let mut min_bounds = CharInfo {
        left_side_bearing: i16::MAX,
        right_side_bearing: i16::MAX,
        character_width: i16::MAX,
        ascent: i16::MAX,
        descent: i16::MAX,
        attributes: 0,
    };
    let mut max_bounds = CharInfo::default();

    for glyph in font.glyphs.iter() {
        let encoding = match glyph.encoding {
            Some(c) => c as u16,
            None => continue,
        };

        let idx = (encoding - min_char) as usize;

        let bb = glyph.bounding_box;
        let dw_x = glyph.device_width.x;

        let ci = CharInfo {
            left_side_bearing: bb.offset.x as i16,
            right_side_bearing: (bb.offset.x + bb.size.x) as i16,
            character_width: dw_x as i16,
            ascent: (bb.offset.y + bb.size.y) as i16,
            descent: -bb.offset.y as i16,
            attributes: 0,
        };

        // Update min/max bounds
        min_bounds.left_side_bearing = min_bounds.left_side_bearing.min(ci.left_side_bearing);
        min_bounds.right_side_bearing = min_bounds.right_side_bearing.min(ci.right_side_bearing);
        min_bounds.character_width = min_bounds.character_width.min(ci.character_width);
        min_bounds.ascent = min_bounds.ascent.min(ci.ascent);
        min_bounds.descent = min_bounds.descent.min(ci.descent);

        max_bounds.left_side_bearing = max_bounds.left_side_bearing.max(ci.left_side_bearing);
        max_bounds.right_side_bearing = max_bounds.right_side_bearing.max(ci.right_side_bearing);
        max_bounds.character_width = max_bounds.character_width.max(ci.character_width);
        max_bounds.ascent = max_bounds.ascent.max(ci.ascent);
        max_bounds.descent = max_bounds.descent.max(ci.descent);

        char_infos[idx] = ci;

        // Extract bitmap data
        let w = bb.size.x as u16;
        let h = bb.size.y as u16;
        let row_bytes = super::glyph_bitmap::row_bytes(w as usize);
        let mut bitmap = vec![0u8; row_bytes * h as usize];

        for row in 0..h as usize {
            for col in 0..w as usize {
                if glyph.pixel(col, row) {
                    super::glyph_bitmap::set(&mut bitmap, row, col, row_bytes);
                }
            }
        }

        glyphs[idx] = GlyphBitmap {
            width: w,
            height: h,
            bitmap,
        };
    }

    Some(BitmapFont {
        name: font_name,
        min_bounds,
        max_bounds,
        min_char,
        max_char,
        default_char: 32, // space
        font_ascent: font_ascent as i16,
        font_descent: font_descent as i16,
        char_infos,
        glyphs,
        scalable_path: None,
        scalable_pixel_size: 0,
    })
}

/// Load a fonts.alias file. Format: alias_name target_name (one per line).
pub(super) fn load_fonts_alias(path: &str) -> Vec<(String, String)> {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut aliases = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('#') {
            continue;
        }
        // Format: "alias" "target" OR alias target
        let parts: Vec<&str> = if line.contains('"') {
            // Quoted format
            line.split('"').filter(|s| !s.trim().is_empty()).collect()
        } else {
            line.splitn(2, char::is_whitespace)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect()
        };
        if parts.len() >= 2 {
            aliases.push((parts[0].to_string(), parts[1].to_string()));
        }
    }
    debug!("Loaded {} font aliases from {}", aliases.len(), path);
    aliases
}

/// Load a fonts.dir file. First line is count, rest are "filename xlfd-name".
pub(super) fn load_fonts_dir(path: &str) -> Vec<(String, String)> {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    for (i, line) in data.lines().enumerate() {
        if i == 0 {
            continue; // skip count line
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Format: filename XLFD-name (space-separated)
        if let Some(space_pos) = line.find(' ') {
            let filename = line[..space_pos].to_string();
            let xlfd = line[space_pos + 1..].trim().to_string();
            entries.push((filename, xlfd));
        }
    }
    debug!("Loaded {} fonts.dir entries from {}", entries.len(), path);
    entries
}
