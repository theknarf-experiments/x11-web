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

    let row_bytes = super::glyph_bitmap::row_bytes(h_glyph.width as usize);
    let mut has_pixel = false;
    for row in 0..h_glyph.height as usize {
        let mut line = String::new();
        for col in 0..h_glyph.width as usize {
            if super::glyph_bitmap::get(&h_glyph.bitmap, row, col, row_bytes) {
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

// -----------------------------------------------------------------------
// PCF glyph padding repack test
// -----------------------------------------------------------------------

/// Build a minimal synthetic PCF file with 4-byte glyph padding and verify
/// that parse_pcf_data correctly repacks bitmaps to 1-byte row stride.
#[test]
fn pcf_4byte_padding_repack() {
    use std::path::Path;

    // Build a tiny PCF file with one glyph: 6 pixels wide, 3 rows tall.
    // With glyph_pad=4, each row is 4 bytes in the PCF bitmap data.
    // With glyph_pad=1 (internal), each row should be 1 byte (ceil(6/8)=1).
    let glyph_w: usize = 6;
    let glyph_h: usize = 3;
    let pcf_row_bytes: usize = 4; // 4-byte padding

    // Bitmap data: a simple pattern (MSB-first bits)
    // Row 0: 0b11111100 = 0xFC (6 set pixels), then 3 pad bytes
    // Row 1: 0b10000100 = 0x84 (pixels at col 0 and 5), then 3 pad bytes
    // Row 2: 0b11111100 = 0xFC (6 set pixels), then 3 pad bytes
    let bitmap_rows: Vec<Vec<u8>> = vec![
        vec![0xFC, 0x00, 0x00, 0x00],
        vec![0x84, 0x00, 0x00, 0x00],
        vec![0xFC, 0x00, 0x00, 0x00],
    ];

    // Construct the PCF file in memory
    let mut pcf = Vec::new();

    // Magic (LE)
    pcf.extend_from_slice(&0x70636601u32.to_le_bytes());

    // We need 4 tables: METRICS, BITMAPS, BDF_ENCODINGS, BDF_ACCELERATORS
    let table_count: u32 = 4;
    pcf.extend_from_slice(&table_count.to_le_bytes());

    // Placeholder for table entries (4 tables * 16 bytes each = 64 bytes)
    let tables_offset = pcf.len();
    pcf.extend(vec![0u8; 64]);

    // --- Table 0: METRICS (compressed) ---
    let metrics_offset = pcf.len() as u32;
    // Format: LE, compressed (bit 8 set), LSB byte order (bit 2 clear), MSB bit order (bit 3 set)
    let metrics_format: u32 = 0x00000108; // compressed | MSB bits
    pcf.extend_from_slice(&metrics_format.to_le_bytes());
    // Compressed count (2 bytes, LE since byte bit not set)
    let glyph_count: u16 = 1;
    pcf.extend_from_slice(&glyph_count.to_le_bytes());
    // One compressed metric: 5 bytes each, values biased by 0x80
    let lsb: u8 = (0i16 + 0x80) as u8; // left_side_bearing = 0
    let rsb: u8 = (glyph_w as i16 + 0x80) as u8; // right_side_bearing = 6
    let cw: u8 = (glyph_w as i16 + 0x80) as u8; // character_width = 6
    let asc: u8 = (glyph_h as i16 + 0x80) as u8; // ascent = 3
    let desc: u8 = 0x80; // descent = 0
    pcf.extend_from_slice(&[lsb, rsb, cw, asc, desc]);
    let metrics_size = (pcf.len() as u32) - metrics_offset;

    // --- Table 1: BITMAPS ---
    let bitmaps_offset = pcf.len() as u32;
    // Format: glyph_pad=2 (meaning 1<<2=4 bytes), MSB bit order (bit 3 set)
    let bitmaps_format: u32 = 0x00000002 | 0x00000008; // pad=4, MSB bits
    pcf.extend_from_slice(&bitmaps_format.to_le_bytes());
    // Glyph count (LE)
    pcf.extend_from_slice(&(1u32).to_le_bytes());
    // Offsets: one glyph at offset 0
    pcf.extend_from_slice(&(0u32).to_le_bytes());
    // 4 bitmap sizes (for pad 1,2,4,8)
    let bm_size = (pcf_row_bytes * glyph_h) as u32;
    for _ in 0..4 {
        pcf.extend_from_slice(&bm_size.to_le_bytes());
    }
    // Bitmap data
    for row in &bitmap_rows {
        pcf.extend_from_slice(row);
    }
    let bitmaps_size = (pcf.len() as u32) - bitmaps_offset;

    // --- Table 2: BDF_ENCODINGS ---
    let encodings_offset = pcf.len() as u32;
    // Format: LE
    let encodings_format: u32 = 0x00000000;
    pcf.extend_from_slice(&encodings_format.to_le_bytes());
    // min_byte2=65, max_byte2=65 (char 'A'), min_byte1=0, max_byte1=0, default_char=65
    pcf.extend_from_slice(&65u16.to_le_bytes()); // min_byte2
    pcf.extend_from_slice(&65u16.to_le_bytes()); // max_byte2
    pcf.extend_from_slice(&0u16.to_le_bytes()); // min_byte1
    pcf.extend_from_slice(&0u16.to_le_bytes()); // max_byte1
    pcf.extend_from_slice(&65u16.to_le_bytes()); // default_char
                                                 // Encoding: glyph index 0 for char 65
    pcf.extend_from_slice(&0u16.to_le_bytes());
    let encodings_size = (pcf.len() as u32) - encodings_offset;

    // --- Table 3: BDF_ACCELERATORS ---
    let accel_offset = pcf.len() as u32;
    let accel_format: u32 = 0x00000000; // LE
    pcf.extend_from_slice(&accel_format.to_le_bytes());
    // 8 bytes of flags/padding
    pcf.extend(vec![0u8; 8]);
    // fontAscent (i32 LE) = 3
    pcf.extend_from_slice(&3i32.to_le_bytes());
    // fontDescent (i32 LE) = 0
    pcf.extend_from_slice(&0i32.to_le_bytes());
    let accel_size = (pcf.len() as u32) - accel_offset;

    // Fill in the table directory
    let table_entries: [(u32, u32, u32, u32); 4] = [
        (1 << 2, metrics_format, metrics_size, metrics_offset), // PCF_METRICS
        (1 << 3, bitmaps_format, bitmaps_size, bitmaps_offset), // PCF_BITMAPS
        (1 << 5, encodings_format, encodings_size, encodings_offset), // PCF_BDF_ENCODINGS
        (1 << 8, accel_format, accel_size, accel_offset),       // PCF_BDF_ACCELERATORS
    ];
    for (i, (tt, fmt, sz, off)) in table_entries.iter().enumerate() {
        let base = tables_offset + i * 16;
        pcf[base..base + 4].copy_from_slice(&tt.to_le_bytes());
        pcf[base + 4..base + 8].copy_from_slice(&fmt.to_le_bytes());
        pcf[base + 8..base + 12].copy_from_slice(&sz.to_le_bytes());
        pcf[base + 12..base + 16].copy_from_slice(&off.to_le_bytes());
    }

    // Parse the synthetic PCF
    let font =
        pcf::parse_pcf_data(&pcf, Path::new("test.pcf")).expect("Failed to parse synthetic PCF");

    // Verify the glyph for char 65 ('A') was repacked correctly
    let glyph = font.glyph(65).expect("Glyph for 'A' should exist");
    assert_eq!(glyph.width, 6);
    assert_eq!(glyph.height, 3);

    // Internal format should be 1-byte row stride (ceil(6/8) = 1)
    assert_eq!(
        glyph.bitmap.len(),
        3,
        "Should be 3 bytes (1 byte per row * 3 rows)"
    );
    assert_eq!(glyph.bitmap[0], 0xFC, "Row 0 should be 0xFC");
    assert_eq!(glyph.bitmap[1], 0x84, "Row 1 should be 0x84");
    assert_eq!(glyph.bitmap[2], 0xFC, "Row 2 should be 0xFC");
}
