mod bdf;
mod pcf;
pub mod scalable;
pub mod types;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

pub use scalable::ScalableFont;
pub use types::BitmapFont;
// Re-exported for tests and downstream consumers
#[allow(unused_imports)]
pub use types::{CharInfo, GlyphBitmap};

// ---------------------------------------------------------------------------
// FreeType library singleton
// ---------------------------------------------------------------------------

static FREETYPE_LIB: OnceLock<freetype::Library> = OnceLock::new();

fn ft_library() -> &'static freetype::Library {
    FREETYPE_LIB.get_or_init(|| {
        freetype::Library::init().expect("Failed to initialise FreeType library")
    })
}

// ---------------------------------------------------------------------------
// Fontconfig integration for TTF/OTF font discovery
// ---------------------------------------------------------------------------

/// Discover system TrueType/OpenType fonts via fontconfig.
/// Returns a list of (family, style, path) tuples.
fn fontconfig_list_fonts() -> Vec<(String, String, std::path::PathBuf)> {
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
        let path = std::path::PathBuf::from(parts[0]);
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

// ---------------------------------------------------------------------------
// FontManager
// ---------------------------------------------------------------------------

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
            let aliases = bdf::load_fonts_alias(&format!("{}/fonts.alias", dir));

            // Load fonts.dir (maps filenames -> XLFD names)
            let font_dir = bdf::load_fonts_dir(&format!("{}/fonts.dir", dir));

            // Load BDF fonts
            let pattern = format!("{}/*.bdf", dir);
            if let Ok(paths) = glob::glob(&pattern) {
                for entry in paths.flatten() {
                    if let Some(font) = bdf::load_bdf_font(&entry) {
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
                    if let Some(font) = bdf::load_bdf_gz_font(&entry) {
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
                    if let Some(font) = pcf::load_pcf_gz_font(&entry) {
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
                    if let Some(font) = pcf::load_pcf_font(&entry) {
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
