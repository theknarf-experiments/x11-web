use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use tracing::{debug, info, warn};

/// Per-character metrics matching X11 CHARINFO
#[derive(Clone, Debug, Default)]
pub struct CharInfo {
    pub left_side_bearing: i16,
    pub right_side_bearing: i16,
    pub character_width: i16,
    pub ascent: i16,
    pub descent: i16,
    pub attributes: u16,
}

/// A loaded bitmap font
#[derive(Clone)]
pub struct BitmapFont {
    pub name: String,
    pub min_bounds: CharInfo,
    pub max_bounds: CharInfo,
    pub min_char: u16,
    pub max_char: u16,
    pub default_char: u16,
    pub font_ascent: i16,
    pub font_descent: i16,
    pub char_infos: Vec<CharInfo>,
    /// Glyph bitmaps indexed by (char_code - min_char). Each bitmap is
    /// rows of bytes, MSB-first, padded to byte boundary per row.
    pub glyphs: Vec<GlyphBitmap>,
}

#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    pub width: u16,
    pub height: u16,
    pub bitmap: Vec<u8>, // row-major, 1 bit per pixel, MSB first, padded to byte boundary
}

impl BitmapFont {
    /// Get the CharInfo for a character code
    pub fn char_info(&self, code: u16) -> &CharInfo {
        if code >= self.min_char && code <= self.max_char {
            let idx = (code - self.min_char) as usize;
            if idx < self.char_infos.len() {
                return &self.char_infos[idx];
            }
        }
        // Return default char info
        if self.default_char >= self.min_char && self.default_char <= self.max_char {
            let idx = (self.default_char - self.min_char) as usize;
            if idx < self.char_infos.len() {
                return &self.char_infos[idx];
            }
        }
        &self.min_bounds
    }

    /// Get the glyph bitmap for a character code
    pub fn glyph(&self, code: u16) -> Option<&GlyphBitmap> {
        if code >= self.min_char && code <= self.max_char {
            let idx = (code - self.min_char) as usize;
            if idx < self.glyphs.len() {
                return Some(&self.glyphs[idx]);
            }
        }
        None
    }

    /// Render a string to a pixel buffer (BGRX format, 4 bytes per pixel).
    /// Returns (width, height, pixels).
    pub fn render_text(&self, text: &[u8], fg: u32, bg: u32) -> (u16, u16, Vec<u8>) {
        // Calculate total width
        let mut total_width: i32 = 0;
        for &ch in text {
            let ci = self.char_info(ch as u16);
            total_width += ci.character_width as i32;
        }
        let total_width = total_width.max(1) as u16;
        let total_height = (self.font_ascent + self.font_descent) as u16;

        let fg_r = ((fg >> 16) & 0xFF) as u8;
        let fg_g = ((fg >> 8) & 0xFF) as u8;
        let fg_b = (fg & 0xFF) as u8;
        let bg_r = ((bg >> 16) & 0xFF) as u8;
        let bg_g = ((bg >> 8) & 0xFF) as u8;
        let bg_b = (bg & 0xFF) as u8;

        let mut pixels = vec![0u8; total_width as usize * total_height as usize * 4];

        // Fill background
        for i in 0..(total_width as usize * total_height as usize) {
            pixels[i * 4] = bg_b;
            pixels[i * 4 + 1] = bg_g;
            pixels[i * 4 + 2] = bg_r;
            pixels[i * 4 + 3] = 0;
        }

        // Render each character
        let mut cursor_x: i32 = 0;
        for &ch in text {
            let ci = self.char_info(ch as u16);
            if let Some(glyph) = self.glyph(ch as u16) {
                let gx = cursor_x + ci.left_side_bearing as i32;
                let gy = self.font_ascent as i32 - ci.ascent as i32;

                let row_bytes = ((glyph.width as usize) + 7) / 8;
                for row in 0..glyph.height as usize {
                    for col in 0..glyph.width as usize {
                        let byte_idx = row * row_bytes + col / 8;
                        let bit_idx = 7 - (col % 8); // MSB first
                        if byte_idx < glyph.bitmap.len()
                            && (glyph.bitmap[byte_idx] >> bit_idx) & 1 != 0
                        {
                            let px = gx as usize + col;
                            let py = gy as usize + row;
                            if px < total_width as usize && py < total_height as usize {
                                let idx = (py * total_width as usize + px) * 4;
                                pixels[idx] = fg_b;
                                pixels[idx + 1] = fg_g;
                                pixels[idx + 2] = fg_r;
                                pixels[idx + 3] = 0;
                            }
                        }
                    }
                }
            }
            cursor_x += ci.character_width as i32;
        }

        (total_width, total_height, pixels)
    }

    /// Render text with transparent background (for PolyText8).
    /// Foreground pixels get alpha=0xFF, background pixels get alpha=0.
    pub fn render_text_transparent(&self, text: &[u8], fg: u32) -> (u16, u16, Vec<u8>) {
        let mut total_width: i32 = 0;
        for &ch in text {
            let ci = self.char_info(ch as u16);
            total_width += ci.character_width as i32;
        }
        let total_width = total_width.max(1) as u16;
        let total_height = (self.font_ascent + self.font_descent) as u16;

        let fg_r = ((fg >> 16) & 0xFF) as u8;
        let fg_g = ((fg >> 8) & 0xFF) as u8;
        let fg_b = (fg & 0xFF) as u8;

        // Background pixels are fully transparent (alpha=0x00)
        let mut pixels = vec![0u8; total_width as usize * total_height as usize * 4];

        let mut cursor_x: i32 = 0;
        for &ch in text {
            let ci = self.char_info(ch as u16);
            if let Some(glyph) = self.glyph(ch as u16) {
                let gx = cursor_x + ci.left_side_bearing as i32;
                let gy = self.font_ascent as i32 - ci.ascent as i32;

                let row_bytes = ((glyph.width as usize) + 7) / 8;
                for row in 0..glyph.height as usize {
                    for col in 0..glyph.width as usize {
                        let byte_idx = row * row_bytes + col / 8;
                        let bit_idx = 7 - (col % 8);
                        if byte_idx < glyph.bitmap.len()
                            && (glyph.bitmap[byte_idx] >> bit_idx) & 1 != 0
                        {
                            let px = gx as usize + col;
                            let py = gy as usize + row;
                            if px < total_width as usize && py < total_height as usize {
                                let idx = (py * total_width as usize + px) * 4;
                                pixels[idx] = fg_b;
                                pixels[idx + 1] = fg_g;
                                pixels[idx + 2] = fg_r;
                                pixels[idx + 3] = 0xFF; // Opaque foreground
                            }
                        }
                    }
                }
            }
            cursor_x += ci.character_width as i32;
        }

        (total_width, total_height, pixels)
    }
}

/// Font manager that loads and caches fonts
pub struct FontManager {
    /// Known fonts by name (lowercase XLFD or alias)
    fonts: HashMap<String, BitmapFont>,
    /// Font IDs assigned by clients
    font_ids: HashMap<u32, String>,
    /// Search paths for BDF files
    search_paths: Vec<String>,
}

impl FontManager {
    pub fn new() -> Self {
        let search_paths = vec![
            "/usr/share/fonts/X11/misc".to_string(),
            "/usr/share/X11/fonts/misc".to_string(),
            "/usr/local/share/fonts/bdf".to_string(),
        ];

        let mut mgr = Self {
            fonts: HashMap::new(),
            font_ids: HashMap::new(),
            search_paths,
        };

        // Pre-load common fonts
        mgr.scan_font_directories();

        mgr
    }

    fn scan_font_directories(&mut self) {
        for dir in &self.search_paths.clone() {
            let pattern = format!("{}/*.bdf", dir);
            if let Ok(paths) = glob::glob(&pattern) {
                for entry in paths.flatten() {
                    if let Some(font) = load_bdf_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded font: {}", name);
                        self.fonts.insert(name, font);
                    }
                }
            }
            // Also try .bdf.gz
            let pattern_gz = format!("{}/*.bdf.gz", dir);
            if let Ok(paths) = glob::glob(&pattern_gz) {
                for entry in paths.flatten() {
                    if let Some(font) = load_bdf_gz_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded font: {} (gzipped)", name);
                        self.fonts.insert(name, font);
                    }
                }
            }
            // Try PCF fonts too
            let pattern_pcf = format!("{}/*.pcf.gz", dir);
            if let Ok(paths) = glob::glob(&pattern_pcf) {
                for entry in paths.flatten() {
                    // We can't parse PCF directly, but note them
                    debug!("Found PCF font (not loaded, need BDF): {}", entry.display());
                }
            }
        }

        info!("Loaded {} fonts", self.fonts.len());
    }

    /// Open a font by name, assign it to a font ID.
    /// Font matching is deterministic: exact match first, then substring,
    /// then well-known fallbacks, then alphabetically-first loaded font.
    pub fn open_font(&mut self, font_id: u32, name: &str) -> bool {
        let name_lower = name.to_lowercase();

        // Try exact match
        if self.fonts.contains_key(&name_lower) {
            self.font_ids.insert(font_id, name_lower);
            return true;
        }

        // "fixed" is an alias for the default font
        if name_lower == "fixed" {
            if let Some(f) = self.get_default_font() {
                let key = f.name.to_lowercase();
                self.font_ids.insert(font_id, key);
                return true;
            }
        }

        // Try matching by XLFD pattern or substring (deterministic: sort keys first)
        let mut keys: Vec<&String> = self.fonts.keys().collect();
        keys.sort();
        for key in &keys {
            if key.contains(&name_lower) {
                self.font_ids.insert(font_id, (*key).clone());
                return true;
            }
        }

        // Fallback: alphabetically-first font for determinism
        if let Some(key) = keys.first() {
            self.font_ids.insert(font_id, (*key).clone());
            return true;
        }

        warn!("No font found for: {}", name);
        false
    }

    pub fn close_font(&mut self, font_id: u32) {
        self.font_ids.remove(&font_id);
    }

    /// Get a loaded font by its font ID
    pub fn get_font(&self, font_id: u32) -> Option<&BitmapFont> {
        let name = self.font_ids.get(&font_id)?;
        self.fonts.get(name)
    }

    /// Get the default font. Deterministic: tries well-known names,
    /// then falls back to alphabetically-first loaded font.
    pub fn get_default_font(&self) -> Option<&BitmapFont> {
        for name in &["fixed", "6x13", "cursor"] {
            if let Some(f) = self.fonts.get(*name) {
                return Some(f);
            }
        }
        // Deterministic fallback: alphabetically first
        let mut keys: Vec<&String> = self.fonts.keys().collect();
        keys.sort();
        keys.first().and_then(|k| self.fonts.get(*k))
    }
}

fn load_bdf_font(path: &Path) -> Option<BitmapFont> {
    let data = std::fs::read(path).ok()?;
    parse_bdf_data(&data, path)
}

fn load_bdf_gz_font(path: &Path) -> Option<BitmapFont> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;
    parse_bdf_data(&data, path)
}

fn parse_bdf_data(data: &[u8], path: &Path) -> Option<BitmapFont> {
    let font = bdf_parser::BdfFont::parse(data).ok()?;

    let font_name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let font_bb = font.metadata.bounding_box;
    let font_ascent = font_bb.offset.y + font_bb.size.y as i32;
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
            right_side_bearing: (bb.offset.x + bb.size.x as i32) as i16,
            character_width: dw_x as i16,
            ascent: (bb.offset.y + bb.size.y as i32) as i16,
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
        let row_bytes = ((w as usize) + 7) / 8;
        let mut bitmap = vec![0u8; row_bytes * h as usize];

        for row in 0..h as usize {
            for col in 0..w as usize {
                if glyph.pixel(col, row) {
                    let byte_idx = row * row_bytes + col / 8;
                    let bit_idx = 7 - (col % 8);
                    bitmap[byte_idx] |= 1 << bit_idx;
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glyph_rendering() {
        let data = std::fs::read("fonts/6x13.bdf").unwrap();
        let font = parse_bdf_data(&data, std::path::Path::new("6x13.bdf")).unwrap();

        // Check H glyph
        let h_info = font.char_info(72); // 'H'
        println!(
            "H: cw={} lsb={} rsb={} asc={} desc={}",
            h_info.character_width,
            h_info.left_side_bearing,
            h_info.right_side_bearing,
            h_info.ascent,
            h_info.descent
        );

        let h_glyph = font.glyph(72).unwrap();
        println!(
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
                if byte_idx < h_glyph.bitmap.len() && (h_glyph.bitmap[byte_idx] >> bit_idx) & 1 != 0
                {
                    line.push('#');
                    has_pixel = true;
                } else {
                    line.push('.');
                }
            }
            println!("  {}", line);
        }
        assert!(has_pixel, "H glyph should have pixels");

        // Test render
        let (w, h, pixels) = font.render_text_transparent(b"H", 0xFFFFFF);
        println!("\nrender_text_transparent H: {}x{}", w, h);
        let mut fg_count = 0;
        for i in 0..(w as usize * h as usize) {
            if pixels[i * 4 + 3] == 0xFF {
                fg_count += 1;
            }
        }
        println!("Foreground pixels: {}", fg_count);
        assert!(fg_count > 0, "Rendered H should have foreground pixels");
    }
}
