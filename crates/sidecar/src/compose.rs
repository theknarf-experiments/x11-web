//! X11 compose key and dead key support.
//!
//! Implements XCompose-style multi-key sequences for producing composed
//! characters (e.g., Compose + ' + e -> e with acute).

use std::collections::HashMap;

/// Compose state machine for handling multi-key sequences.
pub struct ComposeState {
    /// Compose table: sequence of keysyms -> output string.
    table: HashMap<Vec<u32>, String>,
    /// Current compose sequence in progress.
    current: Vec<u32>,
    /// Whether we're in the middle of a compose sequence.
    composing: bool,
}

// XK keysym constants
const XK_MULTI_KEY: u32 = 0xff20;
const XK_DEAD_GRAVE: u32 = 0xfe50;
const XK_DEAD_ACUTE: u32 = 0xfe51;
const XK_DEAD_CIRCUMFLEX: u32 = 0xfe52;
const XK_DEAD_TILDE: u32 = 0xfe53;
const XK_DEAD_CEDILLA: u32 = 0xfe55;
const XK_DEAD_DIAERESIS: u32 = 0xfe57;
const XK_DEAD_RING_ABOVE: u32 = 0xfe58;

impl ComposeState {
    pub fn new() -> Self {
        let mut table = HashMap::new();
        // Build the compose table with common sequences
        build_compose_table(&mut table);
        let mut state = Self {
            table,
            current: Vec::new(),
            composing: false,
        };
        // Load user/system compose files
        state.load_system_compose();
        state
    }

    /// Process a keysym. Returns:
    /// - `ComposeResult::Pass(keysym)` -- not composing, forward the key
    /// - `ComposeResult::Consumed` -- key is part of compose sequence, don't forward
    /// - `ComposeResult::Composed(text)` -- sequence complete, inject this text
    /// - `ComposeResult::Cancelled(keysyms)` -- bad sequence, replay these keys
    pub fn process(&mut self, keysym: u32) -> ComposeResult {
        // Dead keys start a compose sequence
        if is_dead_key(keysym) {
            self.composing = true;
            self.current.clear();
            self.current.push(keysym);
            return ComposeResult::Consumed;
        }

        // Multi_key starts a compose sequence
        if keysym == XK_MULTI_KEY {
            self.composing = true;
            self.current.clear();
            return ComposeResult::Consumed;
        }

        if !self.composing {
            return ComposeResult::Pass(keysym);
        }

        // Add to current sequence
        self.current.push(keysym);

        // Check for exact match
        if let Some(text) = self.table.get(&self.current) {
            let text = text.clone();
            self.composing = false;
            self.current.clear();
            return ComposeResult::Composed(text);
        }

        // Check if any table entry starts with our current sequence
        let has_prefix = self
            .table
            .keys()
            .any(|k| k.len() > self.current.len() && k.starts_with(&self.current));

        if has_prefix {
            // More keys needed
            return ComposeResult::Consumed;
        }

        // No match and no prefix -- cancel compose
        let failed = self.current.clone();
        self.composing = false;
        self.current.clear();
        ComposeResult::Cancelled(failed)
    }

    /// Reset compose state.
    pub fn reset(&mut self) {
        self.composing = false;
        self.current.clear();
    }
}

pub enum ComposeResult {
    /// Not composing -- forward the keysym unchanged.
    Pass(u32),
    /// Key consumed by compose sequence -- don't forward.
    Consumed,
    /// Compose sequence complete -- inject this text.
    Composed(String),
    /// Compose sequence failed -- replay these keysyms.
    Cancelled(Vec<u32>),
}

fn is_dead_key(keysym: u32) -> bool {
    (0xfe50..=0xfe6f).contains(&keysym)
}

/// Build the compose table with common Latin compose sequences.
/// Based on the X11 Compose file for en_US.UTF-8.
fn build_compose_table(table: &mut HashMap<Vec<u32>, String>) {
    // Helper to add both dead key and Multi_key variants
    macro_rules! compose {
        // dead_key + key => char
        (dead $dead:expr, $key:expr => $result:expr) => {
            table.insert(vec![$dead, $key], $result.to_string());
        };
        // Multi_key + k1 + k2 => char
        ($k1:expr, $k2:expr => $result:expr) => {
            table.insert(vec![$k1, $k2], $result.to_string());
        };
    }

    // Acute accent
    compose!(dead XK_DEAD_ACUTE, 0x61 => "\u{00e1}"); // a -> a with acute
    compose!(dead XK_DEAD_ACUTE, 0x65 => "\u{00e9}"); // e
    compose!(dead XK_DEAD_ACUTE, 0x69 => "\u{00ed}"); // i
    compose!(dead XK_DEAD_ACUTE, 0x6f => "\u{00f3}"); // o
    compose!(dead XK_DEAD_ACUTE, 0x75 => "\u{00fa}"); // u
    compose!(dead XK_DEAD_ACUTE, 0x79 => "\u{00fd}"); // y
    compose!(dead XK_DEAD_ACUTE, 0x41 => "\u{00c1}"); // A
    compose!(dead XK_DEAD_ACUTE, 0x45 => "\u{00c9}"); // E
    compose!(dead XK_DEAD_ACUTE, 0x49 => "\u{00cd}"); // I
    compose!(dead XK_DEAD_ACUTE, 0x4f => "\u{00d3}"); // O
    compose!(dead XK_DEAD_ACUTE, 0x55 => "\u{00da}"); // U
    compose!(dead XK_DEAD_ACUTE, 0x59 => "\u{00dd}"); // Y
    compose!(dead XK_DEAD_ACUTE, 0x63 => "\u{0107}"); // c
    compose!(dead XK_DEAD_ACUTE, 0x43 => "\u{0106}"); // C
    compose!(dead XK_DEAD_ACUTE, 0x6e => "\u{0144}"); // n
    compose!(dead XK_DEAD_ACUTE, 0x4e => "\u{0143}"); // N
    compose!(dead XK_DEAD_ACUTE, 0x73 => "\u{015b}"); // s
    compose!(dead XK_DEAD_ACUTE, 0x53 => "\u{015a}"); // S
    compose!(dead XK_DEAD_ACUTE, 0x7a => "\u{017a}"); // z
    compose!(dead XK_DEAD_ACUTE, 0x5a => "\u{0179}"); // Z

    // Grave accent
    compose!(dead XK_DEAD_GRAVE, 0x61 => "\u{00e0}"); // a
    compose!(dead XK_DEAD_GRAVE, 0x65 => "\u{00e8}"); // e
    compose!(dead XK_DEAD_GRAVE, 0x69 => "\u{00ec}"); // i
    compose!(dead XK_DEAD_GRAVE, 0x6f => "\u{00f2}"); // o
    compose!(dead XK_DEAD_GRAVE, 0x75 => "\u{00f9}"); // u
    compose!(dead XK_DEAD_GRAVE, 0x41 => "\u{00c0}"); // A
    compose!(dead XK_DEAD_GRAVE, 0x45 => "\u{00c8}"); // E
    compose!(dead XK_DEAD_GRAVE, 0x49 => "\u{00cc}"); // I
    compose!(dead XK_DEAD_GRAVE, 0x4f => "\u{00d2}"); // O
    compose!(dead XK_DEAD_GRAVE, 0x55 => "\u{00d9}"); // U

    // Circumflex
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x61 => "\u{00e2}"); // a
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x65 => "\u{00ea}"); // e
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x69 => "\u{00ee}"); // i
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x6f => "\u{00f4}"); // o
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x75 => "\u{00fb}"); // u
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x41 => "\u{00c2}"); // A
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x45 => "\u{00ca}"); // E
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x49 => "\u{00ce}"); // I
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x4f => "\u{00d4}"); // O
    compose!(dead XK_DEAD_CIRCUMFLEX, 0x55 => "\u{00db}"); // U

    // Tilde
    compose!(dead XK_DEAD_TILDE, 0x61 => "\u{00e3}"); // a
    compose!(dead XK_DEAD_TILDE, 0x6e => "\u{00f1}"); // n
    compose!(dead XK_DEAD_TILDE, 0x6f => "\u{00f5}"); // o
    compose!(dead XK_DEAD_TILDE, 0x41 => "\u{00c3}"); // A
    compose!(dead XK_DEAD_TILDE, 0x4e => "\u{00d1}"); // N
    compose!(dead XK_DEAD_TILDE, 0x4f => "\u{00d5}"); // O

    // Diaeresis/umlaut
    compose!(dead XK_DEAD_DIAERESIS, 0x61 => "\u{00e4}"); // a
    compose!(dead XK_DEAD_DIAERESIS, 0x65 => "\u{00eb}"); // e
    compose!(dead XK_DEAD_DIAERESIS, 0x69 => "\u{00ef}"); // i
    compose!(dead XK_DEAD_DIAERESIS, 0x6f => "\u{00f6}"); // o
    compose!(dead XK_DEAD_DIAERESIS, 0x75 => "\u{00fc}"); // u
    compose!(dead XK_DEAD_DIAERESIS, 0x79 => "\u{00ff}"); // y
    compose!(dead XK_DEAD_DIAERESIS, 0x41 => "\u{00c4}"); // A
    compose!(dead XK_DEAD_DIAERESIS, 0x45 => "\u{00cb}"); // E
    compose!(dead XK_DEAD_DIAERESIS, 0x49 => "\u{00cf}"); // I
    compose!(dead XK_DEAD_DIAERESIS, 0x4f => "\u{00d6}"); // O
    compose!(dead XK_DEAD_DIAERESIS, 0x55 => "\u{00dc}"); // U

    // Cedilla
    compose!(dead XK_DEAD_CEDILLA, 0x63 => "\u{00e7}"); // c
    compose!(dead XK_DEAD_CEDILLA, 0x43 => "\u{00c7}"); // C
    compose!(dead XK_DEAD_CEDILLA, 0x73 => "\u{015f}"); // s
    compose!(dead XK_DEAD_CEDILLA, 0x53 => "\u{015e}"); // S

    // Ring above
    compose!(dead XK_DEAD_RING_ABOVE, 0x61 => "\u{00e5}"); // a
    compose!(dead XK_DEAD_RING_ABOVE, 0x41 => "\u{00c5}"); // A
    compose!(dead XK_DEAD_RING_ABOVE, 0x75 => "\u{016f}"); // u

    // Multi_key sequences (from X11 Compose file)
    // apostrophe sequences (acute accent via compose)
    compose!(0x27, 0x61 => "\u{00e1}"); // ' a
    compose!(0x27, 0x65 => "\u{00e9}"); // ' e
    compose!(0x27, 0x69 => "\u{00ed}"); // ' i
    compose!(0x27, 0x6f => "\u{00f3}"); // ' o
    compose!(0x27, 0x75 => "\u{00fa}"); // ' u
    compose!(0x27, 0x79 => "\u{00fd}"); // ' y
    compose!(0x27, 0x41 => "\u{00c1}"); // ' A
    compose!(0x27, 0x45 => "\u{00c9}"); // ' E
    compose!(0x27, 0x49 => "\u{00cd}"); // ' I
    compose!(0x27, 0x4f => "\u{00d3}"); // ' O
    compose!(0x27, 0x55 => "\u{00da}"); // ' U
    compose!(0x27, 0x59 => "\u{00dd}"); // ' Y

    // backtick sequences (grave accent via compose)
    compose!(0x60, 0x61 => "\u{00e0}"); // ` a
    compose!(0x60, 0x65 => "\u{00e8}"); // ` e
    compose!(0x60, 0x69 => "\u{00ec}"); // ` i
    compose!(0x60, 0x6f => "\u{00f2}"); // ` o
    compose!(0x60, 0x75 => "\u{00f9}"); // ` u
    compose!(0x60, 0x41 => "\u{00c0}"); // ` A
    compose!(0x60, 0x45 => "\u{00c8}"); // ` E
    compose!(0x60, 0x49 => "\u{00cc}"); // ` I
    compose!(0x60, 0x4f => "\u{00d2}"); // ` O
    compose!(0x60, 0x55 => "\u{00d9}"); // ` U

    // tilde sequences
    compose!(0x7e, 0x6e => "\u{00f1}"); // ~ n
    compose!(0x7e, 0x4e => "\u{00d1}"); // ~ N
    compose!(0x7e, 0x61 => "\u{00e3}"); // ~ a
    compose!(0x7e, 0x41 => "\u{00c3}"); // ~ A
    compose!(0x7e, 0x6f => "\u{00f5}"); // ~ o
    compose!(0x7e, 0x4f => "\u{00d5}"); // ~ O

    // circumflex sequences
    compose!(0x5e, 0x61 => "\u{00e2}"); // ^ a
    compose!(0x5e, 0x65 => "\u{00ea}"); // ^ e
    compose!(0x5e, 0x69 => "\u{00ee}"); // ^ i
    compose!(0x5e, 0x6f => "\u{00f4}"); // ^ o
    compose!(0x5e, 0x75 => "\u{00fb}"); // ^ u
    compose!(0x5e, 0x41 => "\u{00c2}"); // ^ A
    compose!(0x5e, 0x45 => "\u{00ca}"); // ^ E
    compose!(0x5e, 0x49 => "\u{00ce}"); // ^ I
    compose!(0x5e, 0x4f => "\u{00d4}"); // ^ O
    compose!(0x5e, 0x55 => "\u{00db}"); // ^ U

    // diaeresis sequences
    compose!(0x22, 0x61 => "\u{00e4}"); // " a
    compose!(0x22, 0x65 => "\u{00eb}"); // " e
    compose!(0x22, 0x69 => "\u{00ef}"); // " i
    compose!(0x22, 0x6f => "\u{00f6}"); // " o
    compose!(0x22, 0x75 => "\u{00fc}"); // " u
    compose!(0x22, 0x79 => "\u{00ff}"); // " y
    compose!(0x22, 0x41 => "\u{00c4}"); // " A
    compose!(0x22, 0x45 => "\u{00cb}"); // " E
    compose!(0x22, 0x49 => "\u{00cf}"); // " I
    compose!(0x22, 0x4f => "\u{00d6}"); // " O
    compose!(0x22, 0x55 => "\u{00dc}"); // " U

    // cedilla sequences
    compose!(0x2c, 0x63 => "\u{00e7}"); // , c
    compose!(0x2c, 0x43 => "\u{00c7}"); // , C

    // Special characters
    compose!(0x6f, 0x63 => "\u{00a9}"); // o c -> copyright
    compose!(0x4f, 0x43 => "\u{00a9}"); // O C -> copyright
    compose!(0x6f, 0x72 => "\u{00ae}"); // o r -> registered
    compose!(0x4f, 0x52 => "\u{00ae}"); // O R -> registered
    compose!(0x2d, 0x2d => "\u{2014}"); // - - -> em dash
    compose!(0x2e, 0x2e => "\u{2026}"); // . . -> ellipsis
    compose!(0x21, 0x21 => "\u{00a1}"); // ! ! -> inverted !
    compose!(0x3f, 0x3f => "\u{00bf}"); // ? ? -> inverted ?
    compose!(0x3c, 0x3c => "\u{00ab}"); // < < -> left guillemet
    compose!(0x3e, 0x3e => "\u{00bb}"); // > > -> right guillemet
    compose!(0x73, 0x73 => "\u{00df}"); // s s -> eszett
    compose!(0x61, 0x65 => "\u{00e6}"); // a e -> ae ligature
    compose!(0x41, 0x45 => "\u{00c6}"); // A E -> AE ligature
    compose!(0x6f, 0x65 => "\u{0153}"); // o e -> oe ligature
    compose!(0x4f, 0x45 => "\u{0152}"); // O E -> OE ligature
    compose!(0x2f, 0x6f => "\u{00f8}"); // / o -> o with stroke
    compose!(0x2f, 0x4f => "\u{00d8}"); // / O -> O with stroke
    compose!(0x6d, 0x75 => "\u{00b5}"); // m u -> micro sign
    compose!(0x2b, 0x2d => "\u{00b1}"); // + - -> plus-minus
    compose!(0x31, 0x32 => "\u{00bd}"); // 1 2 -> one half
    compose!(0x31, 0x34 => "\u{00bc}"); // 1 4 -> one quarter
    compose!(0x33, 0x34 => "\u{00be}"); // 3 4 -> three quarters
    compose!(0x78, 0x78 => "\u{00d7}"); // x x -> multiplication sign
    compose!(0x2d, 0x3a => "\u{00f7}"); // - : -> division sign
    compose!(0x3d, 0x65 => "\u{20ac}"); // = e -> euro sign
    compose!(0x3d, 0x45 => "\u{20ac}"); // = E -> euro sign
    compose!(0x3d, 0x4c => "\u{00a3}"); // = L -> pound sign
    compose!(0x3d, 0x59 => "\u{00a5}"); // = Y -> yen sign
    compose!(0x63, 0x2f => "\u{00a2}"); // c / -> cent sign
}

impl ComposeState {
    /// Load additional compose sequences from an XCompose file.
    /// Format per spec:
    ///   <Multi_key> <a> <e> : "æ"
    ///   include "%L"   # include locale default
    /// Lines starting with # are comments.
    pub fn load_xcompose_file(&mut self, path: &std::path::Path) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Handle "include" directives: resolve %L, %H, %S placeholders
            if let Some(rest) = line.strip_prefix("include") {
                let include_path = rest.trim().trim_matches('"');
                if !include_path.is_empty() {
                    let resolved = resolve_include_path(include_path);
                    if let Some(resolved_path) = resolved {
                        let p = std::path::Path::new(&resolved_path);
                        if p.exists() {
                            self.load_xcompose_file(p);
                        }
                    }
                }
                continue;
            }
            // Parse: <KeySym> <KeySym> ... : "output"
            if let Some(colon_pos) = line.find(':') {
                let keys_part = &line[..colon_pos];
                let output_part = line[colon_pos + 1..].trim();

                // Extract output string (between quotes)
                let output = if let (Some(start), Some(end)) =
                    (output_part.find('"'), output_part.rfind('"'))
                {
                    if start < end {
                        output_part[start + 1..end].to_string()
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                // Parse key sequence: <KeySym1> <KeySym2> ...
                let mut keysyms = Vec::new();
                let mut valid = true;
                for token in keys_part.split_whitespace() {
                    if token.starts_with('<') && token.ends_with('>') {
                        let name = &token[1..token.len() - 1];
                        if let Some(ks) = keysym_from_name(name) {
                            keysyms.push(ks);
                        } else {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid && !keysyms.is_empty() {
                    // Remove Multi_key prefix if present (we store sequences without it)
                    if keysyms.first() == Some(&XK_MULTI_KEY) {
                        keysyms.remove(0);
                    }
                    if !keysyms.is_empty() {
                        self.table.insert(keysyms, output);
                    }
                }
            }
        }
    }

    /// Try loading from standard locations: ~/.XCompose, then locale-specific Compose file.
    pub fn load_system_compose(&mut self) {
        // User compose file takes priority
        if let Ok(home) = std::env::var("HOME") {
            let user_compose = std::path::PathBuf::from(&home).join(".XCompose");
            if user_compose.exists() {
                self.load_xcompose_file(&user_compose);
                return; // Per XCompose spec, user file replaces system file
            }
        }

        // Fall back to locale-specific system compose file
        if let Some(locale_compose) = find_locale_compose_file() {
            self.load_xcompose_file(&locale_compose);
        }
    }
}

/// Resolve include path placeholders:
/// %L = full locale (e.g. en_US.UTF-8)
/// %l = language (e.g. en)
/// %H = home directory
/// %S = system compose dir (/usr/share/X11/locale)
fn resolve_include_path(path: &str) -> Option<String> {
    let locale = std::env::var("LC_CTYPE")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en_US.UTF-8".to_string());
    let language = locale.split('_').next().unwrap_or("en");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let sys_dir = "/usr/share/X11/locale";

    let resolved = path
        .replace("%L", &locale)
        .replace("%l", language)
        .replace("%H", &home)
        .replace("%S", sys_dir);

    Some(resolved)
}

/// Find the locale-specific Compose file from /usr/share/X11/locale/<locale>/Compose
fn find_locale_compose_file() -> Option<std::path::PathBuf> {
    let locale = std::env::var("LC_CTYPE")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en_US.UTF-8".to_string());

    let base = std::path::Path::new("/usr/share/X11/locale");

    // Try full locale (e.g. en_US.UTF-8)
    let full = base.join(&locale).join("Compose");
    if full.exists() {
        return Some(full);
    }

    // Try without encoding (e.g. en_US)
    let no_encoding = locale.split('.').next().unwrap_or(&locale);
    let path = base.join(no_encoding).join("Compose");
    if path.exists() {
        return Some(path);
    }

    // Try just language (e.g. en)
    let lang = locale.split('_').next().unwrap_or(&locale);
    let path = base.join(lang).join("Compose");
    if path.exists() {
        return Some(path);
    }

    // Fall back to compose.dir lookup
    let compose_dir = base.join("compose.dir");
    if let Ok(content) = std::fs::read_to_string(&compose_dir) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: compose_file_path    locale_name
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == locale {
                let compose_path = base.join(parts[0]);
                if compose_path.exists() {
                    return Some(compose_path);
                }
            }
        }
    }

    None
}

/// Map a keysym name to its numeric value.
/// Covers the most common keysyms used in XCompose files.
fn keysym_from_name(name: &str) -> Option<u32> {
    // Common keysym names used in compose files
    match name {
        "Multi_key" => Some(XK_MULTI_KEY),
        "dead_grave" => Some(XK_DEAD_GRAVE),
        "dead_acute" => Some(XK_DEAD_ACUTE),
        "dead_circumflex" => Some(XK_DEAD_CIRCUMFLEX),
        "dead_tilde" => Some(XK_DEAD_TILDE),
        "dead_cedilla" => Some(XK_DEAD_CEDILLA),
        "dead_diaeresis" => Some(XK_DEAD_DIAERESIS),
        "dead_abovering" | "dead_ring_above" => Some(XK_DEAD_RING_ABOVE),
        "dead_macron" => Some(0xfe54),
        "dead_breve" => Some(0xfe55),
        "dead_abovedot" => Some(0xfe56),
        "dead_doubleacute" => Some(0xfe59),
        "dead_caron" => Some(0xfe5a),
        "dead_ogonek" => Some(0xfe5b),
        "dead_horn" => Some(0xfe5e),
        "dead_stroke" => Some(0xfe63),
        // Latin letters (lowercase)
        _ if name.len() == 1 => {
            let ch = name.chars().next()?;
            if ch.is_ascii() {
                Some(ch as u32)
            } else {
                // Unicode keysym: 0x01000000 + unicode codepoint
                Some(0x0100_0000 + ch as u32)
            }
        }
        // Common named keys
        "space" => Some(0x20),
        "exclam" => Some(0x21),
        "quotedbl" => Some(0x22),
        "numbersign" => Some(0x23),
        "dollar" => Some(0x24),
        "percent" => Some(0x25),
        "ampersand" => Some(0x26),
        "apostrophe" | "quoteright" => Some(0x27),
        "parenleft" => Some(0x28),
        "parenright" => Some(0x29),
        "asterisk" => Some(0x2a),
        "plus" => Some(0x2b),
        "comma" => Some(0x2c),
        "minus" => Some(0x2d),
        "period" => Some(0x2e),
        "slash" => Some(0x2f),
        "colon" => Some(0x3a),
        "semicolon" => Some(0x3b),
        "less" => Some(0x3c),
        "equal" => Some(0x3d),
        "greater" => Some(0x3e),
        "question" => Some(0x3f),
        "at" => Some(0x40),
        "bracketleft" => Some(0x5b),
        "backslash" => Some(0x5c),
        "bracketright" => Some(0x5d),
        "asciicircum" => Some(0x5e),
        "underscore" => Some(0x5f),
        "grave" | "quoteleft" => Some(0x60),
        "braceleft" => Some(0x7b),
        "bar" => Some(0x7c),
        "braceright" => Some(0x7d),
        "asciitilde" => Some(0x7e),
        // Named uppercase letters
        _ if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => {
            Some(name.chars().next()? as u32)
        }
        // Try Unicode hex keysym: U+XXXX or UXXXX
        _ if name.starts_with("U+") || name.starts_with("U") => {
            let hex = name.trim_start_matches("U+").trim_start_matches('U');
            u32::from_str_radix(hex, 16).ok().map(|cp| 0x0100_0000 + cp)
        }
        _ => None,
    }
}

impl Default for ComposeState {
    fn default() -> Self {
        Self::new()
    }
}
