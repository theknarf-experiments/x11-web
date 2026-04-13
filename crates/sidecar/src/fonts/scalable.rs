use std::path::PathBuf;

use super::types::{BitmapFont, CharInfo, GlyphBitmap};

// ---------------------------------------------------------------------------
// Scalable (TrueType / OpenType) font support via FreeType
// ---------------------------------------------------------------------------

/// A scalable font loaded via FreeType, rasterised at a specific pixel size.
#[derive(Clone)]
pub struct ScalableFont {
    /// Path to the font file on disk.
    pub path: PathBuf,
    /// XLFD-style name we advertise for this font.
    pub xlfd_name: String,
    /// Family name from the font's metadata.
    pub family: String,
    /// Style name (Regular, Bold, Italic, …).
    pub style: String,
    /// Pixel size this instance is rasterised at.
    pub pixel_size: u32,
}

impl ScalableFont {
    /// Rasterise a single glyph at the configured pixel size.
    /// Returns `None` if FreeType cannot load the glyph.
    fn render_glyph(&self, char_code: u32) -> Option<(CharInfo, GlyphBitmap)> {
        let lib = super::ft_library();
        let face = lib.new_face(&self.path, 0).ok()?;
        face.set_pixel_sizes(0, self.pixel_size).ok()?;

        face.load_char(char_code as usize, freetype::face::LoadFlag::RENDER)
            .ok()?;
        let glyph = face.glyph();
        let bitmap = glyph.bitmap();
        let metrics = glyph.metrics();

        let w = bitmap.width() as u16;
        let h = bitmap.rows() as u16;
        let pitch = bitmap.pitch().unsigned_abs() as usize;

        // Convert FreeType bitmap (8-bit gray) to 1-bit MSB bitmap
        let row_bytes = (w as usize).div_ceil(8);
        let mut bmp = vec![0u8; row_bytes * h as usize];
        let buf = bitmap.buffer();
        for row in 0..h as usize {
            for col in 0..w as usize {
                let src_idx = row * pitch + col;
                if src_idx < buf.len() && buf[src_idx] >= 128 {
                    let byte_idx = row * row_bytes + col / 8;
                    let bit_idx = 7 - (col % 8);
                    bmp[byte_idx] |= 1 << bit_idx;
                }
            }
        }

        // FreeType metrics are in 26.6 fixed-point (1/64th of a pixel)
        let ci = CharInfo {
            left_side_bearing: (metrics.horiBearingX >> 6) as i16,
            right_side_bearing: ((metrics.horiBearingX + metrics.width) >> 6) as i16,
            character_width: (metrics.horiAdvance >> 6) as i16,
            ascent: (metrics.horiBearingY >> 6) as i16,
            descent: ((metrics.height - metrics.horiBearingY) >> 6) as i16,
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

    /// Convert this scalable font into a `BitmapFont` covering Latin-1 (0–255).
    /// This lets the existing rendering pipeline work unchanged.
    pub fn to_bitmap_font(&self) -> Option<BitmapFont> {
        let lib = super::ft_library();
        let face = lib.new_face(&self.path, 0).ok()?;
        face.set_pixel_sizes(0, self.pixel_size).ok()?;

        let min_char: u16 = 0;
        let max_char: u16 = 255;
        let num_chars = 256usize;

        let mut char_infos = vec![CharInfo::default(); num_chars];
        let mut glyphs = vec![
            GlyphBitmap {
                width: 0,
                height: 0,
                bitmap: Vec::new()
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

        for code in min_char..=max_char {
            if let Some((ci, glyph)) = self.render_glyph(code as u32) {
                min_bounds.left_side_bearing =
                    min_bounds.left_side_bearing.min(ci.left_side_bearing);
                min_bounds.right_side_bearing =
                    min_bounds.right_side_bearing.min(ci.right_side_bearing);
                min_bounds.character_width = min_bounds.character_width.min(ci.character_width);
                min_bounds.ascent = min_bounds.ascent.min(ci.ascent);
                min_bounds.descent = min_bounds.descent.min(ci.descent);
                max_bounds.left_side_bearing =
                    max_bounds.left_side_bearing.max(ci.left_side_bearing);
                max_bounds.right_side_bearing =
                    max_bounds.right_side_bearing.max(ci.right_side_bearing);
                max_bounds.character_width = max_bounds.character_width.max(ci.character_width);
                max_bounds.ascent = max_bounds.ascent.max(ci.ascent);
                max_bounds.descent = max_bounds.descent.max(ci.descent);
                char_infos[code as usize] = ci;
                glyphs[code as usize] = glyph;
            }
        }

        // Get global metrics
        let size_metrics = face.size_metrics()?;
        let font_ascent = (size_metrics.ascender >> 6) as i16;
        let font_descent = ((-size_metrics.descender) >> 6) as i16;

        Some(BitmapFont {
            name: self.xlfd_name.clone(),
            min_bounds,
            max_bounds,
            min_char,
            max_char,
            default_char: 32,
            font_ascent,
            font_descent,
            char_infos,
            glyphs,
            scalable_path: Some(self.path.clone()),
            scalable_pixel_size: self.pixel_size,
        })
    }

    /// Render text to an ARGB pixel buffer using FreeType's anti-aliased rasteriser.
    /// Returns (width, height, BGRX pixels with alpha).
    #[allow(dead_code)]
    pub fn render_text_aa(&self, text: &[u8], fg: u32) -> (u16, u16, Vec<u8>) {
        let lib = super::ft_library();
        let face = match lib.new_face(&self.path, 0) {
            Ok(f) => f,
            Err(_) => return (0, 0, Vec::new()),
        };
        if face.set_pixel_sizes(0, self.pixel_size).is_err() {
            return (0, 0, Vec::new());
        }

        let size_metrics = match face.size_metrics() {
            Some(m) => m,
            None => return (0, 0, Vec::new()),
        };
        let ascender = (size_metrics.ascender >> 6) as i32;
        let descender = (size_metrics.descender >> 6) as i32;
        let line_height = (ascender - descender) as u16;

        // First pass: measure total width
        let mut total_width: i32 = 0;
        for &ch in text {
            if face
                .load_char(ch as usize, freetype::face::LoadFlag::RENDER)
                .is_ok()
            {
                total_width += (face.glyph().metrics().horiAdvance >> 6) as i32;
            }
        }
        let total_width = total_width.max(1) as u16;

        let fg_r = ((fg >> 16) & 0xFF) as u8;
        let fg_g = ((fg >> 8) & 0xFF) as u8;
        let fg_b = (fg & 0xFF) as u8;

        let mut pixels = vec![0u8; total_width as usize * line_height as usize * 4];

        // Second pass: render
        let mut pen_x: i32 = 0;
        for &ch in text {
            if face
                .load_char(ch as usize, freetype::face::LoadFlag::RENDER)
                .is_err()
            {
                continue;
            }
            let glyph = face.glyph();
            let bitmap = glyph.bitmap();
            let bx = glyph.bitmap_left() as i32;
            let by = ascender - glyph.bitmap_top() as i32;

            let w = bitmap.width() as usize;
            let h = bitmap.rows() as usize;
            let pitch = bitmap.pitch().unsigned_abs() as usize;
            let buf = bitmap.buffer();

            for row in 0..h {
                for col in 0..w {
                    let alpha = buf[row * pitch + col];
                    if alpha == 0 {
                        continue;
                    }
                    let px = (pen_x + bx) as usize + col;
                    let py = by as usize + row;
                    if px < total_width as usize && py < line_height as usize {
                        let idx = (py * total_width as usize + px) * 4;
                        // Alpha-blend over existing pixel
                        let a = alpha as u16;
                        let inv_a = 255 - a;
                        pixels[idx] = ((fg_b as u16 * a + pixels[idx] as u16 * inv_a) / 255) as u8;
                        pixels[idx + 1] =
                            ((fg_g as u16 * a + pixels[idx + 1] as u16 * inv_a) / 255) as u8;
                        pixels[idx + 2] =
                            ((fg_r as u16 * a + pixels[idx + 2] as u16 * inv_a) / 255) as u8;
                        pixels[idx + 3] = pixels[idx + 3].saturating_add(alpha);
                    }
                }
            }
            pen_x += (glyph.metrics().horiAdvance >> 6) as i32;
        }

        (total_width, line_height, pixels)
    }
}
