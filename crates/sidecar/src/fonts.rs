use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// FreeType library singleton
// ---------------------------------------------------------------------------

static FREETYPE_LIB: OnceLock<freetype::Library> = OnceLock::new();

fn ft_library() -> &'static freetype::Library {
    FREETYPE_LIB.get_or_init(|| {
        freetype::Library::init().expect("Failed to initialise FreeType library")
    })
}

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
        let lib = ft_library();
        let face = lib.new_face(path, 0).ok()?;
        face.set_pixel_sizes(0, self.scalable_pixel_size).ok()?;
        face.load_char(code as usize, freetype::face::LoadFlag::RENDER).ok()?;
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
        Some((ci, GlyphBitmap { width: w, height: h, bitmap: bmp }))
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
        let lib = ft_library();
        let face = lib.new_face(&self.path, 0).ok()?;
        face.set_pixel_sizes(0, self.pixel_size).ok()?;

        face.load_char(char_code as usize, freetype::face::LoadFlag::RENDER).ok()?;
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

        Some((ci, GlyphBitmap { width: w, height: h, bitmap: bmp }))
    }

    /// Convert this scalable font into a `BitmapFont` covering Latin-1 (0–255).
    /// This lets the existing rendering pipeline work unchanged.
    pub fn to_bitmap_font(&self) -> Option<BitmapFont> {
        let lib = ft_library();
        let face = lib.new_face(&self.path, 0).ok()?;
        face.set_pixel_sizes(0, self.pixel_size).ok()?;

        let min_char: u16 = 0;
        let max_char: u16 = 255;
        let num_chars = 256usize;

        let mut char_infos = vec![CharInfo::default(); num_chars];
        let mut glyphs = vec![GlyphBitmap { width: 0, height: 0, bitmap: Vec::new() }; num_chars];

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
        let lib = ft_library();
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
            if face.load_char(ch as usize, freetype::face::LoadFlag::RENDER).is_ok() {
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
            if face.load_char(ch as usize, freetype::face::LoadFlag::RENDER).is_err() {
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
                        pixels[idx + 1] = ((fg_g as u16 * a + pixels[idx + 1] as u16 * inv_a) / 255) as u8;
                        pixels[idx + 2] = ((fg_r as u16 * a + pixels[idx + 2] as u16 * inv_a) / 255) as u8;
                        pixels[idx + 3] = pixels[idx + 3].saturating_add(alpha);
                    }
                }
            }
            pen_x += (glyph.metrics().horiAdvance >> 6) as i32;
        }

        (total_width, line_height, pixels)
    }
}

// ---------------------------------------------------------------------------
// Fontconfig integration for TTF/OTF font discovery
// ---------------------------------------------------------------------------

/// Discover system TrueType/OpenType fonts via fontconfig.
/// Returns a list of (family, style, path) tuples.
fn fontconfig_list_fonts() -> Vec<(String, String, PathBuf)> {
    use std::process::Command;

    // Use fc-list command (universally available where fontconfig is installed).
    // Output format: path : family : style
    let output = match Command::new("fc-list")
        .args(["--format", "%{file}\t%{family}\t%{style}\n"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            warn!("fc-list not available: {e}");
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let path = PathBuf::from(parts[0]);
        // Only accept TrueType and OpenType fonts
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext != "ttf" && ext != "otf" && ext != "ttc" {
            continue;
        }
        let family = parts[1].split(',').next().unwrap_or(parts[1]).trim().to_string();
        let style = parts[2].split(',').next().unwrap_or(parts[2]).trim().to_string();
        results.push((family, style, path));
    }

    info!("Fontconfig discovered {} scalable fonts", results.len());
    results
}

/// Build an XLFD name for a scalable font.
fn build_xlfd(family: &str, style: &str, pixel_size: u32) -> String {
    let weight = if style.to_lowercase().contains("bold") { "bold" } else { "medium" };
    let slant = if style.to_lowercase().contains("italic") || style.to_lowercase().contains("oblique") {
        "i"
    } else {
        "r"
    };
    // Construct a standard XLFD name
    format!(
        "-misc-{}-{}-{}-normal--{}-{}-75-75-p-0-iso8859-1",
        family.to_lowercase().replace(' ', ""),
        weight,
        slant,
        pixel_size,
        pixel_size * 10, // decipoints
    )
}

/// Font manager that loads and caches fonts
pub struct FontManager {
    /// Known fonts by name (lowercase XLFD or alias)
    fonts: HashMap<String, BitmapFont>,
    /// Font IDs assigned by clients
    font_ids: HashMap<u32, String>,
    /// Search paths for BDF files
    search_paths: Vec<String>,
    /// Discovered scalable fonts (family, style, path).
    /// These are rasterised on-demand when a client opens them at a specific size.
    scalable_fonts: Vec<ScalableFont>,
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
            scalable_fonts: Vec::new(),
        };

        // Pre-load common fonts (BDF/PCF)
        mgr.scan_font_directories();

        // Discover scalable fonts via fontconfig
        mgr.discover_scalable_fonts();

        mgr
    }

    /// Reload fonts from new search paths.
    pub fn reload_paths(&mut self, paths: &[String]) {
        self.search_paths = paths.to_vec();
        // Keep existing font_ids but rescan directories
        self.fonts.clear();
        self.scan_font_directories();
        self.discover_scalable_fonts();
        info!("Font paths reloaded: {} fonts found", self.fonts.len());
    }

    /// Discover scalable TrueType/OpenType fonts via fontconfig.
    fn discover_scalable_fonts(&mut self) {
        let fc_fonts = fontconfig_list_fonts();
        let mut seen_families = std::collections::HashSet::new();

        for (family, style, path) in fc_fonts {
            let key = format!("{}-{}", family.to_lowercase(), style.to_lowercase());
            if !seen_families.insert(key) {
                continue;
            }
            // Pre-rasterise at common X11 pixel sizes (12, 13, 14, 15, 16, 18, 20, 24)
            for &px_size in &[12u32, 13, 14, 15, 16, 18, 20, 24] {
                let xlfd = build_xlfd(&family, &style, px_size);
                let sf = ScalableFont {
                    path: path.clone(),
                    xlfd_name: xlfd.clone(),
                    family: family.clone(),
                    style: style.clone(),
                    pixel_size: px_size,
                };
                self.scalable_fonts.push(sf);
            }
        }

        info!("Discovered {} scalable font variants", self.scalable_fonts.len());
    }

    fn scan_font_directories(&mut self) {
        for dir in &self.search_paths.clone() {
            // Load fonts.alias first (maps short names -> XLFD names)
            let aliases = load_fonts_alias(&format!("{}/fonts.alias", dir));

            // Load fonts.dir (maps filenames -> XLFD names)
            let font_dir = load_fonts_dir(&format!("{}/fonts.dir", dir));

            // Load BDF fonts
            let pattern = format!("{}/*.bdf", dir);
            if let Ok(paths) = glob::glob(&pattern) {
                for entry in paths.flatten() {
                    if let Some(font) = load_bdf_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded BDF font: {}", name);
                        self.fonts.insert(name, font);
                    }
                }
            }

            // Load compressed BDF fonts
            let pattern_gz = format!("{}/*.bdf.gz", dir);
            if let Ok(paths) = glob::glob(&pattern_gz) {
                for entry in paths.flatten() {
                    if let Some(font) = load_bdf_gz_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded BDF.gz font: {}", name);
                        self.fonts.insert(name, font);
                    }
                }
            }

            // Load PCF fonts (compressed)
            let pattern_pcf = format!("{}/*.pcf.gz", dir);
            if let Ok(paths) = glob::glob(&pattern_pcf) {
                for entry in paths.flatten() {
                    if let Some(font) = load_pcf_gz_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded PCF.gz font: {}", name);
                        self.fonts.insert(name, font);
                    }
                }
            }

            // Also try uncompressed PCF
            let pattern_pcf_plain = format!("{}/*.pcf", dir);
            if let Ok(paths) = glob::glob(&pattern_pcf_plain) {
                for entry in paths.flatten() {
                    if let Some(font) = load_pcf_font(&entry) {
                        let name = font.name.to_lowercase();
                        debug!("Loaded PCF font: {}", name);
                        self.fonts.insert(name, font);
                    }
                }
            }

            // Register fonts.dir XLFD names as aliases
            for (filename, xlfd_name) in &font_dir {
                let xlfd_lower = xlfd_name.to_lowercase();
                // Find if we loaded a font from this filename
                let stem = Path::new(filename)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                // Also strip .pcf or .bdf from double extensions
                let stem = stem.strip_suffix(".pcf").or(stem.strip_suffix(".bdf")).unwrap_or(&stem).to_string();
                if self.fonts.contains_key(&stem) && !self.fonts.contains_key(&xlfd_lower) {
                    if let Some(font) = self.fonts.get(&stem).cloned() {
                        self.fonts.insert(xlfd_lower, font);
                    }
                }
            }

            // Register aliases
            for (alias, target) in &aliases {
                let alias_lower = alias.to_lowercase();
                let target_lower = target.to_lowercase();
                if !self.fonts.contains_key(&alias_lower) {
                    if let Some(font) = self.fonts.get(&target_lower).cloned() {
                        self.fonts.insert(alias_lower, font);
                    }
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

        // Try matching by XLFD glob pattern (deterministic: sort keys first).
        // XLFD names use '*' and '?' as wildcards, e.g.
        //   -misc-fixed-*-*-*-*-13-*-*-*-*-*-iso8859-1
        let mut keys: Vec<&String> = self.fonts.keys().collect();
        keys.sort();
        if name_lower.contains('*') || name_lower.contains('?') {
            for key in &keys {
                if glob_match(&name_lower, &key.to_lowercase()) {
                    self.font_ids.insert(font_id, (*key).clone());
                    return true;
                }
            }
        } else {
            // Non-pattern: try substring match
            for key in &keys {
                if key.contains(&name_lower) {
                    self.font_ids.insert(font_id, (*key).clone());
                    return true;
                }
            }
        }

        // Try scalable fonts: match by family name or XLFD pattern
        if let Some(sf) = self.find_scalable_font(&name_lower) {
            debug!("Loading scalable font for '{}': {} ({}pt)", name, sf.xlfd_name, sf.pixel_size);
            if let Some(bf) = sf.to_bitmap_font() {
                let key = bf.name.to_lowercase();
                self.fonts.insert(key.clone(), bf);
                self.font_ids.insert(font_id, key);
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

    /// Get a loaded font by its name (case-insensitive).
    pub fn get_font_by_name(&self, name: &str) -> Option<&BitmapFont> {
        self.fonts.get(&name.to_lowercase())
    }

    /// Find a scalable font that best matches the given name/pattern.
    /// Parses pixel size from XLFD if present, otherwise defaults to 13px.
    fn find_scalable_font(&self, name: &str) -> Option<ScalableFont> {
        let name_lower = name.to_lowercase();

        // Try to extract pixel size from XLFD pattern:
        //   -foundry-family-weight-slant-setwidth--pixelSize-pointSize-...
        let requested_size = if name_lower.starts_with('-') {
            let fields: Vec<&str> = name_lower.split('-').collect();
            // Field 7 (0-indexed) is pixel size in XLFD
            if fields.len() > 7 {
                fields[7].parse::<u32>().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };
        let target_size = if requested_size > 0 { requested_size } else { 13 };

        // Extract family name hints from the pattern
        let family_hint = if name_lower.starts_with('-') {
            let fields: Vec<&str> = name_lower.split('-').collect();
            if fields.len() > 2 { fields[2].to_string() } else { String::new() }
        } else {
            // Might be a short name like "dejavu sans mono"
            name_lower.replace('-', " ")
        };

        // Try exact XLFD match first
        for sf in &self.scalable_fonts {
            if sf.xlfd_name.to_lowercase() == name_lower {
                return Some(sf.clone());
            }
        }

        // Try family name match with closest pixel size
        let mut best: Option<&ScalableFont> = None;
        let mut best_distance = u32::MAX;

        for sf in &self.scalable_fonts {
            let family_lower = sf.family.to_lowercase();
            let matches = if family_hint == "*" || family_hint.is_empty() {
                true
            } else {
                family_lower.contains(&family_hint) || family_hint.contains(&family_lower)
            };
            if !matches {
                continue;
            }
            let dist = (sf.pixel_size as i32 - target_size as i32).unsigned_abs();
            if dist < best_distance {
                best_distance = dist;
                best = Some(sf);
            }
        }

        // If no family match, try any font at the closest size
        if best.is_none() && !self.scalable_fonts.is_empty() {
            for sf in &self.scalable_fonts {
                let dist = (sf.pixel_size as i32 - target_size as i32).unsigned_abs();
                if dist < best_distance {
                    best_distance = dist;
                    best = Some(sf);
                }
            }
        }

        best.map(|sf| {
            // Return a version with the exact requested size
            if sf.pixel_size == target_size {
                sf.clone()
            } else {
                ScalableFont {
                    path: sf.path.clone(),
                    xlfd_name: build_xlfd(&sf.family, &sf.style, target_size),
                    family: sf.family.clone(),
                    style: sf.style.clone(),
                    pixel_size: target_size,
                }
            }
        })
    }

    /// List font names matching an XLFD pattern.
    /// Supports wildcards: `*` matches any sequence, `?` matches any single
    /// character.  Returns at most `max_names` results.
    ///
    /// When the pattern is a full XLFD with a specific pixel size field,
    /// virtual font names are generated from scalable fonts at that size.
    pub fn list_fonts(&self, pattern: &str, max_names: u16) -> Vec<String> {
        // Well-known names that we always advertise, even when no BDF files
        // are loaded.  Sorted so output is deterministic.
        let well_known: &[&str] = &[
            "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1",
            "6x13",
            "9x15",
            "cursor",
            "fixed",
        ];

        // Build a simple glob matcher from the XLFD pattern.
        let pat = pattern.to_lowercase();
        let matches_pattern = |name: &str| -> bool {
            // Empty or pure-wildcard patterns match everything.
            if pat.is_empty() || pat == "*" || pat == "-*-*-*-*-*-*-*-*-*-*-*-*-*-*" {
                return true;
            }
            glob_match(&pat, &name.to_lowercase())
        };

        // Extract requested pixel size from XLFD pattern for scalable font synthesis.
        // XLFD: -foundry-family-weight-slant-setwidth--pixelSize-pointSize-...
        // Field index 7 (0-indexed, first empty field before foundry is index 0).
        let requested_size = if pat.starts_with('-') {
            let fields: Vec<&str> = pat.split('-').collect();
            if fields.len() > 7 {
                fields[7].parse::<u32>().ok().filter(|&s| s > 0)
            } else {
                None
            }
        } else {
            None
        };

        // Collect matching names from loaded fonts + well-known, de-dup.
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        // Well-known first so apps always find the essentials.
        for &wk in well_known {
            if result.len() >= max_names as usize {
                break;
            }
            if matches_pattern(wk) && seen.insert(wk.to_string()) {
                result.push(wk.to_string());
            }
        }

        // Then real loaded fonts (sorted for determinism).
        let mut keys: Vec<&String> = self.fonts.keys().collect();
        keys.sort();
        for key in keys {
            if result.len() >= max_names as usize {
                break;
            }
            if matches_pattern(key) && seen.insert(key.clone()) {
                result.push(key.clone());
            }
        }

        // Include discovered scalable fonts — both at their native sizes and
        // at the specifically requested pixel size (if the XLFD pattern
        // specifies one).  This allows apps like xfontsel to discover
        // scalable fonts at arbitrary sizes.
        let mut sf_names: Vec<String> = Vec::new();
        for sf in &self.scalable_fonts {
            sf_names.push(sf.xlfd_name.to_lowercase());
            // Generate a virtual XLFD at the requested pixel size
            if let Some(size) = requested_size {
                if sf.pixel_size != size {
                    let virtual_xlfd = build_xlfd(&sf.family, &sf.style, size);
                    sf_names.push(virtual_xlfd.to_lowercase());
                }
            }
        }
        sf_names.sort();
        sf_names.dedup();
        for name in sf_names {
            if result.len() >= max_names as usize {
                break;
            }
            if matches_pattern(&name) && seen.insert(name.clone()) {
                result.push(name);
            }
        }

        result
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
        let row_bytes = (w as usize).div_ceil(8);
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
        scalable_path: None,
        scalable_pixel_size: 0,
    })
}

/// Load a fonts.alias file. Format: alias_name target_name (one per line).
fn load_fonts_alias(path: &str) -> Vec<(String, String)> {
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
            line.split('"')
                .filter(|s| !s.trim().is_empty())
                .collect()
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
fn load_fonts_dir(path: &str) -> Vec<(String, String)> {
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

// ============================================================================
// PCF font parser
// ============================================================================

const PCF_MAGIC: u32 = 0x70636601; // "\1fcp" in LE

// PCF table types
const PCF_PROPERTIES: u32 = 1 << 0;
const PCF_ACCELERATORS: u32 = 1 << 1;
const PCF_METRICS: u32 = 1 << 2;
const PCF_BITMAPS: u32 = 1 << 3;
const PCF_BDF_ENCODINGS: u32 = 1 << 5;
const PCF_BDF_ACCELERATORS: u32 = 1 << 8;

// Format flags
const PCF_ACCEL_W_INKBOUNDS: u32 = 0x00000100;
const PCF_COMPRESSED_METRICS: u32 = 0x00000100;
const PCF_BYTE_MASK: u32 = 1 << 2; // MSB byte order
const PCF_BIT_MASK: u32 = 1 << 3;  // MSB bit order
#[allow(dead_code)]
const PCF_GLYPH_PAD_MASK: u32 = 3; // 2 bits for glyph padding

fn pcf_read_u32(data: &[u8], offset: usize, msb: bool) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    if msb {
        u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    } else {
        u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    }
}

fn pcf_read_u16(data: &[u8], offset: usize, msb: bool) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    if msb {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    }
}

fn pcf_read_i16(data: &[u8], offset: usize, msb: bool) -> i16 {
    pcf_read_u16(data, offset, msb) as i16
}

fn pcf_read_i32(data: &[u8], offset: usize, msb: bool) -> i32 {
    pcf_read_u32(data, offset, msb) as i32
}

struct PcfTable {
    table_type: u32,
    #[allow(dead_code)]
    format: u32,
    size: u32,
    offset: u32,
}

fn load_pcf_font(path: &Path) -> Option<BitmapFont> {
    let data = std::fs::read(path).ok()?;
    parse_pcf_data(&data, path)
}

fn load_pcf_gz_font(path: &Path) -> Option<BitmapFont> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;
    parse_pcf_data(&data, path)
}

fn parse_pcf_data(data: &[u8], path: &Path) -> Option<BitmapFont> {
    if data.len() < 8 {
        return None;
    }

    // Check magic (always LE)
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != PCF_MAGIC {
        return None;
    }

    let table_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if data.len() < 8 + table_count * 16 {
        return None;
    }

    let mut tables = Vec::with_capacity(table_count);
    for i in 0..table_count {
        let off = 8 + i * 16;
        tables.push(PcfTable {
            table_type: u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]),
            format: u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]),
            size: u32::from_le_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]]),
            offset: u32::from_le_bytes([data[off + 12], data[off + 13], data[off + 14], data[off + 15]]),
        });
    }

    let find_table = |tt: u32| -> Option<&PcfTable> {
        tables.iter().find(|t| t.table_type == tt)
    };

    // Parse metrics
    let metrics_table = find_table(PCF_METRICS)?;
    let metrics = parse_pcf_metrics(data, metrics_table)?;

    // Parse bitmaps
    let bitmaps_table = find_table(PCF_BITMAPS)?;
    let bitmaps = parse_pcf_bitmaps(data, bitmaps_table, metrics.len())?;

    // Parse encodings
    let encodings_table = find_table(PCF_BDF_ENCODINGS)?;
    let (min_char, max_char, encoding_map) = parse_pcf_encodings(data, encodings_table)?;

    // Parse properties for font name
    let font_name = if let Some(props_table) = find_table(PCF_PROPERTIES) {
        parse_pcf_properties_font_name(data, props_table)
    } else {
        None
    };

    // Parse accelerators for ascent/descent
    let accel_table = find_table(PCF_BDF_ACCELERATORS)
        .or_else(|| find_table(PCF_ACCELERATORS));
    let (font_ascent, font_descent) = if let Some(at) = accel_table {
        parse_pcf_accelerators(data, at)
    } else {
        // Derive from metrics
        let mut max_asc = 0i16;
        let mut max_desc = 0i16;
        for m in &metrics {
            max_asc = max_asc.max(m.ascent);
            max_desc = max_desc.max(m.descent);
        }
        (max_asc, max_desc)
    };

    // Build the BitmapFont
    let num_chars = (max_char - min_char + 1) as usize;
    let mut char_infos = vec![CharInfo::default(); num_chars];
    let mut glyphs_vec = vec![
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

    for encoding in min_char..=max_char {
        let idx = (encoding - min_char) as usize;
        let glyph_idx = match encoding_map.get(&encoding) {
            Some(&gi) => gi,
            None => continue,
        };

        if glyph_idx >= metrics.len() {
            continue;
        }

        let m = &metrics[glyph_idx];
        let ci = CharInfo {
            left_side_bearing: m.left_side_bearing,
            right_side_bearing: m.right_side_bearing,
            character_width: m.character_width,
            ascent: m.ascent,
            descent: m.descent,
            attributes: m.attributes,
        };

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

        // Get bitmap for this glyph
        if glyph_idx < bitmaps.len() {
            let w = (m.right_side_bearing - m.left_side_bearing).max(0) as u16;
            let h = (m.ascent + m.descent).max(0) as u16;
            glyphs_vec[idx] = GlyphBitmap {
                width: w,
                height: h,
                bitmap: bitmaps[glyph_idx].clone(),
            };
        }
    }

    let name = font_name.unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    Some(BitmapFont {
        name,
        min_bounds,
        max_bounds,
        min_char,
        max_char,
        default_char: 32,
        font_ascent,
        font_descent,
        char_infos,
        glyphs: glyphs_vec,
        scalable_path: None,
        scalable_pixel_size: 0,
    })
}

struct PcfMetric {
    left_side_bearing: i16,
    right_side_bearing: i16,
    character_width: i16,
    ascent: i16,
    descent: i16,
    attributes: u16,
}

fn parse_pcf_metrics(data: &[u8], table: &PcfTable) -> Option<Vec<PcfMetric>> {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false); // format is always LE
    let msb = format & PCF_BYTE_MASK != 0;
    let compressed = format & PCF_COMPRESSED_METRICS != 0;

    let mut pos = off + 4;
    let mut metrics = Vec::new();

    if compressed {
        // Compressed: 2-byte count, then 5 bytes per metric
        let count = pcf_read_u16(data, pos, msb) as usize;
        pos += 2;
        for _ in 0..count {
            if pos + 5 > data.len() {
                break;
            }
            metrics.push(PcfMetric {
                left_side_bearing: data[pos] as i16 - 0x80,
                right_side_bearing: data[pos + 1] as i16 - 0x80,
                character_width: data[pos + 2] as i16 - 0x80,
                ascent: data[pos + 3] as i16 - 0x80,
                descent: data[pos + 4] as i16 - 0x80,
                attributes: 0,
            });
            pos += 5;
        }
    } else {
        // Uncompressed: 4-byte count, then 12 bytes per metric
        let count = pcf_read_u32(data, pos, msb) as usize;
        pos += 4;
        for _ in 0..count {
            if pos + 12 > data.len() {
                break;
            }
            metrics.push(PcfMetric {
                left_side_bearing: pcf_read_i16(data, pos, msb),
                right_side_bearing: pcf_read_i16(data, pos + 2, msb),
                character_width: pcf_read_i16(data, pos + 4, msb),
                ascent: pcf_read_i16(data, pos + 6, msb),
                descent: pcf_read_i16(data, pos + 8, msb),
                attributes: pcf_read_u16(data, pos + 10, msb),
            });
            pos += 12;
        }
    }

    Some(metrics)
}

fn parse_pcf_bitmaps(data: &[u8], table: &PcfTable, glyph_count: usize) -> Option<Vec<Vec<u8>>> {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;
    let msb_bits = format & PCF_BIT_MASK != 0;
    let _glyph_pad = 1 << (format & PCF_GLYPH_PAD_MASK);

    let mut pos = off + 4;

    // Glyph count
    let count = pcf_read_u32(data, pos, msb) as usize;
    pos += 4;

    if count != glyph_count || count > 100_000 {
        // Mismatch - try to proceed anyway
    }

    // Offsets into bitmap data (one per glyph)
    let mut offsets = Vec::with_capacity(count);
    for _ in 0..count {
        offsets.push(pcf_read_u32(data, pos, msb) as usize);
        pos += 4;
    }

    // 4 bitmap sizes (for different padding)
    let _sizes: [u32; 4] = [
        pcf_read_u32(data, pos, msb),
        pcf_read_u32(data, pos + 4, msb),
        pcf_read_u32(data, pos + 8, msb),
        pcf_read_u32(data, pos + 12, msb),
    ];
    pos += 16;

    let bitmap_data_start = pos;

    // Extract bitmaps
    let mut bitmaps = Vec::with_capacity(count);
    for i in 0..count {
        let bm_off = bitmap_data_start + offsets[i];
        // We need to figure out the size from metrics (height * stride)
        // For now, just copy the raw bitmap data between this offset and the next
        let next_off = if i + 1 < count {
            bitmap_data_start + offsets[i + 1]
        } else {
            data.len().min(off + table.size as usize)
        };

        let size = next_off.saturating_sub(bm_off).min(data.len() - bm_off.min(data.len()));
        let mut bitmap = if bm_off + size <= data.len() {
            data[bm_off..bm_off + size].to_vec()
        } else {
            Vec::new()
        };

        // If bit order is LSB-first, reverse bits in each byte
        if !msb_bits {
            for byte in &mut bitmap {
                *byte = byte.reverse_bits();
            }
        }

        bitmaps.push(bitmap);
    }

    Some(bitmaps)
}

fn parse_pcf_encodings(
    data: &[u8],
    table: &PcfTable,
) -> Option<(u16, u16, HashMap<u16, usize>)> {
    let off = table.offset as usize;
    if off + 14 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;

    let min_byte2 = pcf_read_u16(data, off + 4, msb);
    let max_byte2 = pcf_read_u16(data, off + 6, msb);
    let min_byte1 = pcf_read_u16(data, off + 8, msb);
    let max_byte1 = pcf_read_u16(data, off + 10, msb);
    let _default_char = pcf_read_u16(data, off + 12, msb);

    let mut pos = off + 14;
    let mut encoding_map = HashMap::new();

    // For single-byte fonts, min_byte1 == max_byte1 == 0
    for b1 in min_byte1..=max_byte1 {
        for b2 in min_byte2..=max_byte2 {
            if pos + 2 > data.len() {
                break;
            }
            let glyph_idx = pcf_read_u16(data, pos, msb);
            pos += 2;
            if glyph_idx != 0xFFFF {
                let encoding = if min_byte1 == 0 && max_byte1 == 0 {
                    b2
                } else {
                    (b1 << 8) | b2
                };
                encoding_map.insert(encoding, glyph_idx as usize);
            }
        }
    }

    let min_char = min_byte2;
    let max_char = if min_byte1 == 0 && max_byte1 == 0 {
        max_byte2
    } else {
        (max_byte1 << 8) | max_byte2
    };

    Some((min_char, max_char, encoding_map))
}

fn parse_pcf_properties_font_name(data: &[u8], table: &PcfTable) -> Option<String> {
    let off = table.offset as usize;
    if off + 8 > data.len() {
        return None;
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;

    let num_props = pcf_read_u32(data, off + 4, msb) as usize;
    if num_props > 10_000 {
        return None;
    }

    let props_start = off + 8;
    // Each property: name_offset(4), is_string(1), value(4) = 9 bytes
    let strings_start = props_start + num_props * 9;
    // Align to 4 bytes
    let strings_start = (strings_start + 3) & !3;

    if strings_start + 4 > data.len() {
        return None;
    }
    let string_size = pcf_read_u32(data, strings_start, msb) as usize;
    let string_data_start = strings_start + 4;

    if string_data_start + string_size > data.len() {
        return None;
    }

    let strings = &data[string_data_start..string_data_start + string_size];

    // Look for FONT property
    for i in 0..num_props {
        let poff = props_start + i * 9;
        if poff + 9 > data.len() {
            break;
        }
        let name_offset = pcf_read_u32(data, poff, msb) as usize;
        let is_string = data[poff + 4];
        let value = pcf_read_u32(data, poff + 5, msb);

        if name_offset < strings.len() {
            let name_end = strings[name_offset..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(strings.len() - name_offset);
            let name = std::str::from_utf8(&strings[name_offset..name_offset + name_end]).ok()?;

            if name == "FONT" && is_string != 0 {
                let val_offset = value as usize;
                if val_offset < strings.len() {
                    let val_end = strings[val_offset..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(strings.len() - val_offset);
                    return std::str::from_utf8(&strings[val_offset..val_offset + val_end])
                        .ok()
                        .map(|s| s.to_string());
                }
            }
        }
    }

    None
}

fn parse_pcf_accelerators(data: &[u8], table: &PcfTable) -> (i16, i16) {
    let off = table.offset as usize;
    if off + 4 > data.len() {
        return (10, 3);
    }
    let format = pcf_read_u32(data, off, false);
    let msb = format & PCF_BYTE_MASK != 0;
    let _has_ink = format & PCF_ACCEL_W_INKBOUNDS != 0;

    // Layout: format(4), noOverlap(1), constantMetrics(1),
    //         terminalFont(1), constantWidth(1), inkInside(1),
    //         inkMetrics(1), drawDirection(1), padding(1),
    //         fontAscent(4), fontDescent(4), maxOverlap(4),
    //         then minbounds(12) and maxbounds(12)
    //         then optionally ink_minbounds(12) and ink_maxbounds(12)

    let ascent_off = off + 12;
    let descent_off = off + 16;

    if descent_off + 4 > data.len() {
        return (10, 3);
    }

    let font_ascent = pcf_read_i32(data, ascent_off, msb) as i16;
    let font_descent = pcf_read_i32(data, descent_off, msb) as i16;

    (font_ascent, font_descent)
}

/// Simple glob matching for XLFD patterns.  Supports `*` (any sequence)
/// and `?` (any single char).  Both inputs should be lowercased already.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (pn, tn) = (p.len(), t.len());
    // DP with two rows.
    let mut prev = vec![false; tn + 1];
    prev[0] = true;
    for i in 1..=pn {
        let mut cur = vec![false; tn + 1];
        if p[i - 1] == '*' {
            // '*' can match empty or extend.
            cur[0] = prev[0];
            for j in 1..=tn {
                cur[j] = prev[j] || cur[j - 1];
            }
        } else {
            for j in 1..=tn {
                if p[i - 1] == '?' || p[i - 1] == t[j - 1] {
                    cur[j] = prev[j - 1];
                }
            }
        }
        prev = cur;
    }
    prev[tn]
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
                if byte_idx < h_glyph.bitmap.len() && (h_glyph.bitmap[byte_idx] >> bit_idx) & 1 != 0
                {
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
        let fm = FontManager::new();
        let result = fm.list_fonts("*", 100);
        assert!(result.contains(&"fixed".to_string()));
        assert!(result.contains(&"cursor".to_string()));
    }

    #[test]
    fn list_fonts_max_names_limit() {
        let fm = FontManager::new();
        let result = fm.list_fonts("*", 2);
        assert!(result.len() <= 2);
    }

    #[test]
    fn list_fonts_specific_pattern() {
        let fm = FontManager::new();
        let result = fm.list_fonts("fixed", 10);
        assert!(result.contains(&"fixed".to_string()));
    }

    #[test]
    fn list_fonts_no_match() {
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
        let mut fm = FontManager::new();
        // If any fonts are loaded, "fixed" should succeed
        if !fm.fonts.is_empty() {
            let ok = fm.open_font(1, "fixed");
            assert!(ok);
        }
    }

    #[test]
    fn open_font_with_xlfd_pattern_if_available() {
        let mut fm = FontManager::new();
        if !fm.fonts.is_empty() {
            let ok = fm.open_font(3, "-*-*-*-*-*-*-*-*-*-*-*-*-*-*");
            assert!(ok);
        }
    }

    #[test]
    fn close_font_removes_id() {
        let mut fm = FontManager::new();
        if fm.open_font(10, "fixed") {
            assert!(fm.get_font(10).is_some());
            fm.close_font(10);
            assert!(fm.get_font(10).is_none());
        }
    }

    #[test]
    fn open_and_close_font_id_lifecycle() {
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
            min_bounds: CharInfo { character_width: 6, ..Default::default() },
            max_bounds: CharInfo { character_width: 6, ..Default::default() },
            min_char: 32,
            max_char: 126,
            default_char: 32,
            font_ascent: 10,
            font_descent: 3,
            char_infos: vec![CharInfo { character_width: 6, ..Default::default() }; 95],
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
            min_bounds: CharInfo { character_width: 5, ..Default::default() },
            max_bounds: CharInfo { character_width: 8, ..Default::default() },
            min_char: 32,
            max_char: 126,
            default_char: 32,
            font_ascent: 10,
            font_descent: 3,
            char_infos: {
                let mut v = vec![CharInfo { character_width: 6, ..Default::default() }; 95];
                v[0].character_width = 5;  // char 32 (space)
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
        let fm = FontManager::new();
        let result = fm.list_fonts("*", 0);
        assert!(result.is_empty());
    }

    #[test]
    fn list_fonts_max_names_one() {
        let fm = FontManager::new();
        let result = fm.list_fonts("*", 1);
        assert!(result.len() <= 1);
    }
}
