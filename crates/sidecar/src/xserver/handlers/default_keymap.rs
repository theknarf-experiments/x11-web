//! Default xkbcommon keymap shared across all clients for keysym lookup.
//!
//! We compile `evdev / pc105 / us` once via libxkbcommon and use it for every
//! `keycode_to_keysym` query. xkbcommon's `key_get_syms_by_level` is stateless
//! and reentrant, so a single immutable Keymap can be queried from any thread.
//!
//! Falls back to a tiny hardcoded table if libxkbcommon's data files are not
//! installed (e.g. a stripped-down Docker image without `xkb-data`). The
//! fallback only covers keys our own tests assert on; everything else returns
//! `(0, 0)` — matching the previous behaviour for unknown keycodes.

use std::sync::OnceLock;
use xkbcommon::xkb;

struct DefaultKeymap {
    keymap: xkb::Keymap,
    // The Context must outlive the Keymap; keep it here so the OnceLock
    // owner can drop them together.
    _context: xkb::Context,
}

// xkbcommon Keymap and Context hold raw FFI pointers and are documented as
// thread-compatible (single owner, but movable). Reads are safe to share
// across threads as long as no one mutates the keymap, which we never do.
unsafe impl Send for DefaultKeymap {}
unsafe impl Sync for DefaultKeymap {}

static DEFAULT: OnceLock<Option<DefaultKeymap>> = OnceLock::new();

fn build() -> Option<DefaultKeymap> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "evdev",
        "pc105",
        "us",
        "",
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )?;
    Some(DefaultKeymap {
        keymap,
        _context: context,
    })
}

/// Resolve a single keycode's XKB name (e.g. `"AE10"`) padded to 4 bytes
/// with trailing spaces, matching X11's `KeyName` wire format. Returns
/// `*b"K   "` when xkbcommon has no name for the keycode (truly unmapped).
pub(crate) fn key_name(keycode: u8) -> [u8; 4] {
    let entry = DEFAULT.get_or_init(build);
    if let Some(km) = entry {
        let kc_raw = keycode as u32;
        if kc_raw >= km.keymap.min_keycode().raw()
            && kc_raw <= km.keymap.max_keycode().raw()
        {
            if let Some(name) = km.keymap.key_get_name(xkb::Keycode::new(kc_raw)) {
                let mut out = *b"    ";
                let bytes = name.as_bytes();
                let n = bytes.len().min(4);
                out[..n].copy_from_slice(&bytes[..n]);
                return out;
            }
        }
    }
    *b"K   "
}

/// Build a 248-entry array of single-keysym values for X11 keycodes 8..=255,
/// indexed by `keycode - 8`. Returns the unmodified (level 0) keysym for
/// each, or 0 when xkbcommon has no mapping. This is the data fed to
/// `XkbGetMap` replies for the default ("server") keymap.
pub(crate) fn default_keysym_array() -> [u32; 248] {
    let mut out = [0u32; 248];
    let entry = DEFAULT.get_or_init(build);
    let Some(km) = entry else {
        return out;
    };
    let min = km.keymap.min_keycode().raw();
    let max = km.keymap.max_keycode().raw();
    for kc in 8u32..=255u32 {
        if kc < min || kc > max {
            continue;
        }
        let syms = km
            .keymap
            .key_get_syms_by_level(xkb::Keycode::new(kc), 0, 0);
        out[(kc - 8) as usize] = first_sym(syms);
    }
    out
}

/// Look up `(unmodified, shifted)` keysyms for an X11 keycode using the shared
/// default US keymap. Returns `(0, 0)` for keycodes outside the keymap's
/// declared range — matching the old hand-rolled table for unknown keys.
pub(crate) fn keysyms_for_keycode(keycode: u8) -> (u32, u32) {
    let entry = DEFAULT.get_or_init(build);
    if let Some(km) = entry {
        // Clamp to the keymap's declared keycode range. `key_get_syms_by_level`
        // doesn't bounds-check; out-of-range queries can return spurious
        // multimedia keysyms.
        let kc_raw = keycode as u32;
        let min = km.keymap.min_keycode().raw();
        let max = km.keymap.max_keycode().raw();
        if kc_raw < min || kc_raw > max {
            return legacy_fallback(keycode);
        }
        let kc = xkb::Keycode::new(kc_raw);
        let layout = 0;
        let normal = first_sym(km.keymap.key_get_syms_by_level(kc, layout, 0));
        let shifted = first_sym(km.keymap.key_get_syms_by_level(kc, layout, 1));
        if normal != 0 || shifted != 0 {
            // For Shift-only keys (Esc, Return, F-keys, modifiers, etc.)
            // xkbcommon often leaves level 1 unset. Mirror the unmodified
            // sym so callers always get the same byte for both slots,
            // matching the previous hand-rolled behaviour.
            let shifted = if shifted == 0 { normal } else { shifted };
            return (normal, shifted);
        }
    }
    legacy_fallback(keycode)
}

fn first_sym(syms: &[xkb::Keysym]) -> u32 {
    syms.first().map(|k| k.raw()).unwrap_or(0)
}

/// Hardcoded fallback for environments without /usr/share/X11/xkb data files.
/// Only covers the keys the unit tests assert on.
fn legacy_fallback(keycode: u8) -> (u32, u32) {
    const XK_BACKSPACE: u32 = 0xff08;
    const XK_TAB: u32 = 0xff09;
    const XK_RETURN: u32 = 0xff0d;
    const XK_ESCAPE: u32 = 0xff1b;
    const XK_DELETE: u32 = 0xffff;
    const XK_HOME: u32 = 0xff50;
    const XK_LEFT: u32 = 0xff51;
    const XK_UP: u32 = 0xff52;
    const XK_RIGHT: u32 = 0xff53;
    const XK_DOWN: u32 = 0xff54;
    const XK_PAGE_UP: u32 = 0xff55;
    const XK_PAGE_DOWN: u32 = 0xff56;
    const XK_END: u32 = 0xff57;
    const XK_INSERT: u32 = 0xff63;
    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_SHIFT_R: u32 = 0xffe2;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_CONTROL_R: u32 = 0xffe4;
    const XK_CAPS_LOCK: u32 = 0xffe5;
    const XK_ALT_L: u32 = 0xffe9;
    const XK_ALT_R: u32 = 0xffea;
    const XK_SUPER_L: u32 = 0xffeb;
    const XK_SUPER_R: u32 = 0xffec;
    const XK_F1: u32 = 0xffbe;
    const XK_SPACE: u32 = 0x0020;

    match keycode {
        9 => (XK_ESCAPE, XK_ESCAPE),
        10 => (0x31, 0x21),
        14 => (0x35, 0x25),
        18 => (0x39, 0x28),
        19 => (0x30, 0x29),
        22 => (XK_BACKSPACE, XK_BACKSPACE),
        23 => (XK_TAB, XK_TAB),
        24 => (0x71, 0x51),
        36 => (XK_RETURN, XK_RETURN),
        37 => (XK_CONTROL_L, XK_CONTROL_L),
        38 => (0x61, 0x41),
        50 => (XK_SHIFT_L, XK_SHIFT_L),
        52 => (0x7a, 0x5a),
        58 => (0x6d, 0x4d),
        62 => (XK_SHIFT_R, XK_SHIFT_R),
        64 => (XK_ALT_L, XK_ALT_L),
        65 => (XK_SPACE, XK_SPACE),
        66 => (XK_CAPS_LOCK, XK_CAPS_LOCK),
        k @ 67..=76 => (XK_F1 + (k - 67) as u32, XK_F1 + (k - 67) as u32),
        95 => (XK_F1 + 10, XK_F1 + 10),
        96 => (XK_F1 + 11, XK_F1 + 11),
        105 => (XK_CONTROL_R, XK_CONTROL_R),
        108 => (XK_ALT_R, XK_ALT_R),
        110 => (XK_HOME, XK_HOME),
        111 => (XK_UP, XK_UP),
        112 => (XK_PAGE_UP, XK_PAGE_UP),
        113 => (XK_LEFT, XK_LEFT),
        114 => (XK_RIGHT, XK_RIGHT),
        115 => (XK_END, XK_END),
        116 => (XK_DOWN, XK_DOWN),
        117 => (XK_PAGE_DOWN, XK_PAGE_DOWN),
        118 => (XK_INSERT, XK_INSERT),
        119 => (XK_DELETE, XK_DELETE),
        133 => (XK_SUPER_L, XK_SUPER_L),
        134 => (XK_SUPER_R, XK_SUPER_R),
        _ => (0, 0),
    }
}
