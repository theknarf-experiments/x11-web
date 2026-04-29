//! X11 color name database.
//!
//! Provides [`lookup_color`] which resolves an X11 color specification to a
//! 16-bit RGB triple.  Supported formats:
//!
//! - Named colors from `/usr/share/X11/rgb.txt` (or a built-in fallback)
//! - Hex specs: `#RGB`, `#RRGGBB`, `#RRRRGGGGBBBB`
//! - Floating-point intensity: `rgbi:R/G/B` (each component 0.0..1.0)
//!
//! Named lookups are case-insensitive **and** space-insensitive, so
//! `"Light Gray"`, `"lightgray"`, and `"LightGray"` all resolve identically.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Global color database, lazily initialized on first access.
static COLOR_DB: OnceLock<HashMap<String, (u16, u16, u16)>> = OnceLock::new();

/// Scale an 8-bit color component to 16-bit (0-255 -> 0-65535).
#[inline]
fn scale8(v: u8) -> u16 {
    v as u16 * 257
}

/// Normalize a color name for case-insensitive and space-insensitive lookup.
/// Converts to lowercase and strips all ASCII whitespace.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Parse `/usr/share/X11/rgb.txt` into a map of normalized-name -> (r16, g16, b16).
fn parse_rgb_txt(contents: &str) -> HashMap<String, (u16, u16, u16)> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        // Format: R  G  B  <tab-or-spaces>  color name
        let mut parts = line.split_ascii_whitespace();
        let r: Option<u8> = parts.next().and_then(|s| s.parse().ok());
        let g: Option<u8> = parts.next().and_then(|s| s.parse().ok());
        let b: Option<u8> = parts.next().and_then(|s| s.parse().ok());
        if let (Some(r), Some(g), Some(b)) = (r, g, b) {
            // The rest is the color name (may contain spaces).
            let name: String = parts.collect::<Vec<_>>().join(" ");
            if !name.is_empty() {
                let key = normalize(&name);
                map.entry(key).or_insert((scale8(r), scale8(g), scale8(b)));
            }
        }
    }
    map
}

/// Compile-time copy of `/usr/share/X11/rgb.txt` so the database is always
/// populated even when the system file is missing (CI, minimal containers,
/// non-Linux dev machines).
const EMBEDDED_RGB_TXT: &str = include_str!("../data/rgb.txt");

/// Build the color database.  Prefers the system file (which may have been
/// patched by the distro), falling back to the embedded copy.
fn build_db() -> HashMap<String, (u16, u16, u16)> {
    if let Ok(contents) = std::fs::read_to_string("/usr/share/X11/rgb.txt") {
        let map = parse_rgb_txt(&contents);
        if !map.is_empty() {
            return map;
        }
    }
    parse_rgb_txt(EMBEDDED_RGB_TXT)
}

fn get_db() -> &'static HashMap<String, (u16, u16, u16)> {
    COLOR_DB.get_or_init(build_db)
}

/// Look up an X11 color specification and return 16-bit RGB components.
///
/// Accepts:
/// - Named colors (case/space insensitive): `"LightGray"`, `"light gray"`
/// - Hex: `#RGB`, `#RRGGBB`, `#RRRRGGGGBBBB`
/// - `rgbi:R/G/B` with floating-point intensities in `[0.0, 1.0]`
pub fn lookup_color(name: &str) -> Option<(u16, u16, u16)> {
    let name = name.trim();

    // Try hex format: #RGB, #RRGGBB, #RRRRGGGGBBBB
    if let Some(hex) = name.strip_prefix('#') {
        return parse_hex(hex);
    }

    // Try rgbi:R/G/B format
    if let Some(rest) = name.strip_prefix("rgbi:") {
        return parse_rgbi(rest);
    }

    // Named color lookup (case + space insensitive)
    let key = normalize(name);
    get_db().get(&key).copied()
}

/// Parse hex color: `RGB` (4-bit), `RRGGBB` (8-bit), `RRRRGGGGBBBB` (16-bit).
fn parse_hex(hex: &str) -> Option<(u16, u16, u16)> {
    match hex.len() {
        3 => {
            // #RGB -> expand each nibble to 16-bit
            let r = u16::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u16::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u16::from_str_radix(&hex[2..3], 16).ok()?;
            // 4-bit -> 16-bit: multiply by 0x1111
            Some((r * 0x1111, g * 0x1111, b * 0x1111))
        }
        6 => {
            // #RRGGBB -> expand 8-bit to 16-bit
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((scale8(r), scale8(g), scale8(b)))
        }
        12 => {
            // #RRRRGGGGBBBB -> already 16-bit
            let r = u16::from_str_radix(&hex[0..4], 16).ok()?;
            let g = u16::from_str_radix(&hex[4..8], 16).ok()?;
            let b = u16::from_str_radix(&hex[8..12], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Parse `rgbi:R/G/B` where each component is a float in [0.0, 1.0].
fn parse_rgbi(spec: &str) -> Option<(u16, u16, u16)> {
    let mut parts = spec.split('/');
    let r: f64 = parts.next()?.parse().ok()?;
    let g: f64 = parts.next()?.parse().ok()?;
    let b: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None; // too many components
    }
    let clamp = |v: f64| -> u16 { (v.clamp(0.0, 1.0) * 65535.0).round() as u16 };
    Some((clamp(r), clamp(g), clamp(b)))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_lookup() {
        assert_eq!(lookup_color("white"), Some((65535, 65535, 65535)));
        assert_eq!(lookup_color("black"), Some((0, 0, 0)));
        assert_eq!(lookup_color("red"), Some((65535, 0, 0)));
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(lookup_color("WHITE"), lookup_color("white"));
        assert_eq!(lookup_color("LightGray"), lookup_color("light gray"));
        assert_eq!(lookup_color("lightgray"), lookup_color("Light Gray"));
    }

    #[test]
    fn test_space_insensitive() {
        assert_eq!(
            lookup_color("DarkSlateGray"),
            lookup_color("dark slate gray")
        );
        assert_eq!(
            lookup_color("cornflowerblue"),
            lookup_color("cornflower blue")
        );
    }

    #[test]
    fn test_hex_rgb() {
        assert_eq!(lookup_color("#FFF"), Some((0xFFFF, 0xFFFF, 0xFFFF)));
        assert_eq!(lookup_color("#000"), Some((0, 0, 0)));
        assert_eq!(lookup_color("#F00"), Some((0xFFFF, 0, 0)));
    }

    #[test]
    fn test_hex_rrggbb() {
        assert_eq!(lookup_color("#FF0000"), Some((65535, 0, 0)));
        assert_eq!(lookup_color("#00FF00"), Some((0, 65535, 0)));
        assert_eq!(
            lookup_color("#808080"),
            Some((128 * 257, 128 * 257, 128 * 257))
        );
    }

    #[test]
    fn test_hex_rrrrggggbbbb() {
        assert_eq!(lookup_color("#FFFF00000000"), Some((65535, 0, 0)));
        assert_eq!(
            lookup_color("#800080008000"),
            Some((0x8000, 0x8000, 0x8000))
        );
    }

    #[test]
    fn test_rgbi() {
        assert_eq!(lookup_color("rgbi:1.0/0.0/0.0"), Some((65535, 0, 0)));
        assert_eq!(lookup_color("rgbi:0.0/1.0/0.0"), Some((0, 65535, 0)));
        assert_eq!(
            lookup_color("rgbi:0.5/0.5/0.5"),
            Some((32768, 32768, 32768))
        );
    }

    #[test]
    fn test_gray_numbered() {
        // gray0 = black
        assert_eq!(lookup_color("gray0"), Some((0, 0, 0)));
        // gray100 = white
        assert_eq!(lookup_color("gray100"), Some((65535, 65535, 65535)));
        // grey50 = (127, 127, 127)
        assert_eq!(
            lookup_color("grey50"),
            Some((127 * 257, 127 * 257, 127 * 257))
        );
    }

    #[test]
    fn test_numbered_variants() {
        // snow1 = snow = (255, 250, 250)
        assert_eq!(lookup_color("snow1"), Some((65535, 64250, 64250)));
        // blue4 = (0, 0, 139)
        assert_eq!(lookup_color("blue4"), Some((0, 0, 139 * 257)));
    }

    #[test]
    fn test_unknown_returns_none() {
        assert_eq!(lookup_color("notacolor"), None);
    }

    #[test]
    fn test_dark_colors() {
        assert_eq!(lookup_color("DarkGreen"), Some((0, 100 * 257, 0)));
        assert_eq!(lookup_color("DarkBlue"), Some((0, 0, 139 * 257)));
        assert_eq!(lookup_color("DarkRed"), Some((139 * 257, 0, 0)));
    }

    #[test]
    fn test_scale8() {
        assert_eq!(scale8(0), 0);
        assert_eq!(scale8(255), 65535);
        assert_eq!(scale8(128), 32896);
    }
}
