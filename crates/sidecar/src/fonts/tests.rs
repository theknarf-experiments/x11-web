use super::*;
use std::sync::Mutex;

/// FreeType library is not thread-safe; serialize tests that create FontManager.
static FT_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_glyph_rendering() {
    let data = std::fs::read("fonts/6x13.bdf").unwrap();
    let font = bdf::parse_bdf_data(&data, std::path::Path::new("6x13.bdf")).unwrap();

    // Check H glyph
    let h_info = font.char_info(72); // 'H'
    debug!(
        "H: cw={} lsb={} rsb={} asc={} desc={}",
        h_info.character_width,
        h_info.left_side_bearing,
        h_info.right_side_bearing,
        h_info.ascent,
        h_info.descent
    );

    let h_glyph = font.glyph(72).unwrap();
    debug!(
        "H glyph: {}x{} bitmap_len={}",
        h_glyph.width,
        h_glyph.height,
        h_glyph.bitmap.len()
    );

    let row_bytes = ((h_glyph.width as usize) + 7) / 8;
    let mut has_pixel = false;
    for row in 0..h_glyph.height as usize {
        let mut line = String::new();
        for col in 0..h_glyph.width as usize {
            let byte_idx = row * row_bytes + col / 8;
            let bit_idx = 7 - (col % 8);
            if byte_idx < h_glyph.bitmap.len() && (h_glyph.bitmap[byte_idx] >> bit_idx) & 1 != 0 {
                line.push('#');
                has_pixel = true;
            } else {
                line.push('.');
            }
        }
        debug!("  {}", line);
    }
    assert!(has_pixel, "H glyph should have pixels");

    // Test render
    let (w, h, pixels) = font.render_text_transparent(b"H", 0xFFFFFF);
    debug!("\nrender_text_transparent H: {}x{}", w, h);
    let mut fg_count = 0;
    for i in 0..(w as usize * h as usize) {
        if pixels[i * 4 + 3] == 0xFF {
            fg_count += 1;
        }
    }
    debug!("Foreground pixels: {}", fg_count);
    assert!(fg_count > 0, "Rendered H should have foreground pixels");
}

// -----------------------------------------------------------------------
// glob_match tests
// -----------------------------------------------------------------------

#[test]
fn glob_match_exact() {
    assert!(glob_match("hello", "hello"));
    assert!(!glob_match("hello", "world"));
}

#[test]
fn glob_match_star() {
    assert!(glob_match("*", "anything"));
    assert!(glob_match("hel*", "hello"));
    assert!(glob_match("*llo", "hello"));
    assert!(glob_match("h*o", "hello"));
    assert!(!glob_match("h*x", "hello"));
}

#[test]
fn glob_match_question() {
    assert!(glob_match("h?llo", "hello"));
    assert!(!glob_match("h?lo", "hello"));
}

#[test]
fn glob_match_xlfd_pattern() {
    // Standard XLFD: 14 hyphen-separated fields
    let font = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    assert!(glob_match("-misc-fixed-*-*-*-*-*-*-*-*-*-*-*-*", font));
    assert!(glob_match("-*-fixed-*-*-*-*-13-*-*-*-*-*-*-*", font));
    assert!(glob_match("-*-*-*-*-*-*-*-*-*-*-*-*-iso8859-1", font));
    assert!(!glob_match("-*-helvetica-*-*-*-*-*-*-*-*-*-*-*-*", font));
}

#[test]
fn glob_match_xlfd_pixel_size() {
    let font = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    // Specific pixel size
    assert!(glob_match("-*-*-*-*-*-*-13-*-*-*-*-*-*-*", font));
    assert!(!glob_match("-*-*-*-*-*-*-12-*-*-*-*-*-*-*", font));
}

// -----------------------------------------------------------------------
// list_fonts tests
// -----------------------------------------------------------------------

#[test]
fn list_fonts_wildcard_returns_well_known() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("*", 100);
    assert!(result.contains(&"fixed".to_string()));
    assert!(result.contains(&"cursor".to_string()));
}

#[test]
fn list_fonts_max_names_limit() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("*", 2);
    assert!(result.len() <= 2);
}

#[test]
fn list_fonts_specific_pattern() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("fixed", 10);
    assert!(result.contains(&"fixed".to_string()));
}

#[test]
fn list_fonts_no_match() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("nonexistent-font-xyz", 10);
    assert!(result.is_empty());
}

// -----------------------------------------------------------------------
// XLFD glob pattern edge cases
// -----------------------------------------------------------------------

#[test]
fn glob_match_empty_pattern_matches_nothing() {
    // An empty pattern should NOT match anything (empty != wildcard)
    assert!(!glob_match("", "hello"));
}

#[test]
fn glob_match_empty_both() {
    assert!(glob_match("", ""));
}

#[test]
fn glob_match_star_matches_empty() {
    assert!(glob_match("*", ""));
}

#[test]
fn glob_match_question_requires_char() {
    assert!(!glob_match("?", ""));
    assert!(glob_match("?", "a"));
}

#[test]
fn glob_match_multiple_stars() {
    assert!(glob_match("**", "anything"));
    assert!(glob_match("a*b*c", "axbxc"));
    assert!(glob_match("a*b*c", "abc"));
    assert!(!glob_match("a*b*c", "axc"));
}

#[test]
fn glob_match_mixed_star_question() {
    assert!(glob_match("a?c*", "abcdef"));
    assert!(!glob_match("a?c*", "adef"));
}

#[test]
fn glob_match_xlfd_full_wildcard() {
    let font = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    assert!(glob_match("-*-*-*-*-*-*-*-*-*-*-*-*-*-*", font));
}

#[test]
fn glob_match_xlfd_partial_fields() {
    let font = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    // Match by foundry and family
    assert!(glob_match("-misc-fixed-*", font));
    // Match by encoding
    assert!(glob_match("*iso8859-1", font));
    // Match by pixel size
    assert!(glob_match("*--13-*", font));
}

#[test]
fn glob_match_case_sensitive() {
    // glob_match is case-sensitive; callers must lowercase
    assert!(!glob_match("HELLO", "hello"));
    assert!(glob_match("hello", "hello"));
}

// -----------------------------------------------------------------------
// Font manager: open_font edge cases
// -----------------------------------------------------------------------

#[test]
fn open_font_succeeds_when_fonts_available() {
    let _lock = FT_LOCK.lock().unwrap();
    let mut fm = FontManager::new();
    // If any fonts are loaded, "fixed" should succeed
    if !fm.fonts.is_empty() {
        let ok = fm.open_font(1, "fixed");
        assert!(ok);
    }
}

#[test]
fn open_font_with_xlfd_pattern_if_available() {
    let _lock = FT_LOCK.lock().unwrap();
    let mut fm = FontManager::new();
    if !fm.fonts.is_empty() {
        let ok = fm.open_font(3, "-*-*-*-*-*-*-*-*-*-*-*-*-*-*");
        assert!(ok);
    }
}

#[test]
fn close_font_removes_id() {
    let _lock = FT_LOCK.lock().unwrap();
    let mut fm = FontManager::new();
    if fm.open_font(10, "fixed") {
        assert!(fm.get_font(10).is_some());
        fm.close_font(10);
        assert!(fm.get_font(10).is_none());
    }
}

#[test]
fn open_and_close_font_id_lifecycle() {
    let _lock = FT_LOCK.lock().unwrap();
    let mut fm = FontManager::new();
    // Opening a font twice with same ID should work (last wins)
    if fm.open_font(100, "fixed") {
        assert!(fm.get_font(100).is_some());
        fm.close_font(100);
        assert!(fm.get_font(100).is_none());
        // Closing again should be harmless
        fm.close_font(100);
        assert!(fm.get_font(100).is_none());
    }
}

// -----------------------------------------------------------------------
// BitmapFont: char_info edge cases
// -----------------------------------------------------------------------

#[test]
fn char_info_out_of_range_uses_default() {
    let font = BitmapFont {
        name: "test".to_string(),
        min_bounds: CharInfo {
            character_width: 6,
            ..Default::default()
        },
        max_bounds: CharInfo {
            character_width: 6,
            ..Default::default()
        },
        min_char: 32,
        max_char: 126,
        default_char: 32,
        font_ascent: 10,
        font_descent: 3,
        char_infos: vec![
            CharInfo {
                character_width: 6,
                ..Default::default()
            };
            95
        ],
        glyphs: Vec::new(),
        scalable_path: None,
        scalable_pixel_size: 13,
    };
    // Code 200 is out of range — should fall back to default_char
    let ci = font.char_info(200);
    assert_eq!(ci.character_width, 6);
}

#[test]
fn char_info_at_boundaries() {
    let font = BitmapFont {
        name: "test".to_string(),
        min_bounds: CharInfo {
            character_width: 5,
            ..Default::default()
        },
        max_bounds: CharInfo {
            character_width: 8,
            ..Default::default()
        },
        min_char: 32,
        max_char: 126,
        default_char: 32,
        font_ascent: 10,
        font_descent: 3,
        char_infos: {
            let mut v = vec![
                CharInfo {
                    character_width: 6,
                    ..Default::default()
                };
                95
            ];
            v[0].character_width = 5; // char 32 (space)
            v[94].character_width = 8; // char 126 (~)
            v
        },
        glyphs: Vec::new(),
        scalable_path: None,
        scalable_pixel_size: 13,
    };
    assert_eq!(font.char_info(32).character_width, 5);
    assert_eq!(font.char_info(126).character_width, 8);
    assert_eq!(font.char_info(80).character_width, 6);
}

// -----------------------------------------------------------------------
// list_fonts: max_names boundary
// -----------------------------------------------------------------------

#[test]
fn list_fonts_max_names_zero() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("*", 0);
    assert!(result.is_empty());
}

#[test]
fn list_fonts_max_names_one() {
    let _lock = FT_LOCK.lock().unwrap();
    let fm = FontManager::new();
    let result = fm.list_fonts("*", 1);
    assert!(result.len() <= 1);
}
