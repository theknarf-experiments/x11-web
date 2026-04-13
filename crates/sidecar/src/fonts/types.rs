use std::path::PathBuf;

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
    /// Optional scalable font path for on-demand rendering of codepoints
    /// beyond the pre-rendered range (e.g. CJK, Arabic, Cyrillic extended).
    pub scalable_path: Option<PathBuf>,
    /// Pixel size used for scalable font rendering (needed for on-demand glyphs).
    pub scalable_pixel_size: u32,
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

    /// Render a glyph on-demand for codepoints beyond the pre-rendered range.
    /// Uses the scalable font path if available (for FreeType-backed fonts).
    /// Returns (CharInfo, GlyphBitmap) if the glyph exists in the font.
    pub fn render_extended_glyph(&self, code: u32) -> Option<(CharInfo, GlyphBitmap)> {
        let path = self.scalable_path.as_ref()?;
        let lib = super::ft_library();
        let face = lib.new_face(path, 0).ok()?;
        face.set_pixel_sizes(0, self.scalable_pixel_size).ok()?;
        face.load_char(code as usize, freetype::face::LoadFlag::RENDER)
            .ok()?;
        let glyph = face.glyph();
        let bitmap = glyph.bitmap();
        let metrics = glyph.metrics();

        let w = bitmap.width() as u16;
        let h = bitmap.rows() as u16;
        let lsb = (metrics.horiBearingX >> 6) as i16;
        let ascent = (metrics.horiBearingY >> 6) as i16;
        let descent = h as i16 - ascent;
        let char_width = (metrics.horiAdvance >> 6) as i16;
        let rsb = lsb + w as i16;

        // Convert FreeType bitmap (8-bit grayscale) to 1-bit MSB-first
        let row_bytes = (w as usize).div_ceil(8);
        let mut bmp = vec![0u8; row_bytes * h as usize];
        let buffer = bitmap.buffer();
        let pitch = bitmap.pitch().unsigned_abs() as usize;
        for row in 0..h as usize {
            for col in 0..w as usize {
                let gray = if row * pitch + col < buffer.len() {
                    buffer[row * pitch + col]
                } else {
                    0
                };
                if gray >= 128 {
                    bmp[row * row_bytes + col / 8] |= 0x80 >> (col % 8);
                }
            }
        }

        let ci = CharInfo {
            left_side_bearing: lsb,
            right_side_bearing: rsb,
            character_width: char_width,
            ascent,
            descent,
            attributes: 0,
        };
        Some((
            ci,
            GlyphBitmap {
                width: w,
                height: h,
                bitmap: bmp,
            },
        ))
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

        // Fill background (opaque — ImageText fills the bounding box)
        for i in 0..(total_width as usize * total_height as usize) {
            pixels[i * 4] = bg_b;
            pixels[i * 4 + 1] = bg_g;
            pixels[i * 4 + 2] = bg_r;
            pixels[i * 4 + 3] = 0xFF;
        }

        // Render each character
        let mut cursor_x: i32 = 0;
        for &ch in text {
            let ci = self.char_info(ch as u16);
            if let Some(glyph) = self.glyph(ch as u16) {
                let gx = cursor_x + ci.left_side_bearing as i32;
                let gy = self.font_ascent as i32 - ci.ascent as i32;

                let row_bytes = (glyph.width as usize).div_ceil(8);
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
                                pixels[idx + 3] = 0xFF;
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

                let row_bytes = (glyph.width as usize).div_ceil(8);
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
