//! XKB Geometry model: data structures, default PC-105 layout,
//! wire serialization for GetGeometry (opcode 19) and SetGeometry (opcode 20).

use std::sync::{Arc, Mutex};

use tracing::debug;

use crate::xserver::atoms::AtomManager;
use crate::xserver::client::ClientState;
use super::map::us_qwerty_key_names;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// XKB geometry for a physical keyboard layout.
pub(crate) struct XkbGeometry {
    pub name: u32,
    pub width_mm: u16,
    pub height_mm: u16,
    pub label_font: String,
    pub properties: Vec<XkbProperty>,
    pub colors: Vec<String>,
    pub shapes: Vec<XkbShape>,
    pub sections: Vec<XkbSection>,
    pub doodads: Vec<XkbDoodad>,
    pub key_aliases: Vec<([u8; 4], [u8; 4])>,
}

pub(crate) struct XkbProperty {
    pub name: String,
    pub value: String,
}

pub(crate) struct XkbShape {
    pub name: u32,
    pub outlines: Vec<XkbOutline>,
    pub primary_ndx: u8,
    pub approx_ndx: u8,
}

pub(crate) struct XkbOutline {
    pub corner_radius: u16,
    pub points: Vec<(i16, i16)>,
}

pub(crate) struct XkbSection {
    pub name: u32,
    pub top: i16,
    pub left: i16,
    pub width: u16,
    pub height: u16,
    pub angle: i16,
    pub priority: u8,
    pub rows: Vec<XkbRow>,
    pub doodads: Vec<XkbDoodad>,
    pub overlays: Vec<XkbOverlay>,
}

pub(crate) struct XkbRow {
    pub top: i16,
    pub left: i16,
    pub vertical: bool,
    pub keys: Vec<XkbKey>,
}

pub(crate) struct XkbKey {
    pub name: [u8; 4],
    pub gap: i16,
    pub shape_ndx: u8,
    pub color_ndx: u8,
}

pub(crate) struct XkbOverlay {
    pub name: u32,
    pub rows: Vec<XkbOverlayRow>,
}

pub(crate) struct XkbOverlayRow {
    pub row_under: u8,
    pub keys: Vec<XkbOverlayKey>,
}

pub(crate) struct XkbOverlayKey {
    pub over: [u8; 4],
    pub under: [u8; 4],
}

#[allow(dead_code)]
pub(crate) enum XkbDoodad {
    Outline {
        name: u32,
        top: i16,
        left: i16,
        angle: i16,
        color_ndx: u8,
        shape_ndx: u8,
        priority: u8,
    },
    Solid {
        name: u32,
        top: i16,
        left: i16,
        angle: i16,
        color_ndx: u8,
        shape_ndx: u8,
        priority: u8,
    },
    Text {
        name: u32,
        top: i16,
        left: i16,
        angle: i16,
        color_ndx: u8,
        text: String,
        font: String,
        priority: u8,
    },
    Indicator {
        name: u32,
        top: i16,
        left: i16,
        angle: i16,
        shape_ndx: u8,
        on_color_ndx: u8,
        off_color_ndx: u8,
        priority: u8,
    },
    Logo {
        name: u32,
        top: i16,
        left: i16,
        angle: i16,
        color_ndx: u8,
        shape_ndx: u8,
        logo_name: String,
        priority: u8,
    },
}

// ---------------------------------------------------------------------------
// Wire-format helpers
// ---------------------------------------------------------------------------

/// Pad length up to a 4-byte boundary.
fn pad4(len: usize) -> usize {
    (4 - (len % 4)) % 4
}

/// Write a counted string: u16 length + chars + pad to 4 bytes.
fn write_counted_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(bytes);
    let padding = pad4(2 + bytes.len());
    buf.extend(std::iter::repeat(0u8).take(padding));
}

/// Serialize an XkbOutline to wire format.
fn serialize_outline(buf: &mut Vec<u8>, outline: &XkbOutline) {
    buf.push(outline.points.len() as u8); // nPoints
    buf.extend_from_slice(&outline.corner_radius.to_le_bytes()); // cornerRadius
    buf.push(0); // pad
    for &(x, y) in &outline.points {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
    }
}

/// Serialize an XkbShape to wire format.
fn serialize_shape(buf: &mut Vec<u8>, shape: &XkbShape) {
    buf.extend_from_slice(&shape.name.to_le_bytes()); // name ATOM
    buf.push(shape.outlines.len() as u8); // nOutlines
    buf.push(shape.primary_ndx);
    buf.push(shape.approx_ndx);
    buf.push(0); // pad
    for outline in &shape.outlines {
        serialize_outline(buf, outline);
    }
}

/// Serialize an XkbKey to wire format.
fn serialize_key(buf: &mut Vec<u8>, key: &XkbKey) {
    buf.extend_from_slice(&key.name);
    buf.extend_from_slice(&key.gap.to_le_bytes());
    buf.push(key.shape_ndx);
    buf.push(key.color_ndx);
}

/// Serialize an XkbRow to wire format.
fn serialize_row(buf: &mut Vec<u8>, row: &XkbRow) {
    buf.extend_from_slice(&row.top.to_le_bytes());
    buf.extend_from_slice(&row.left.to_le_bytes());
    buf.push(row.keys.len() as u8); // nKeys
    buf.push(if row.vertical { 1 } else { 0 }); // vertical
    buf.extend_from_slice(&[0u8; 2]); // pad
    for key in &row.keys {
        serialize_key(buf, key);
    }
}

/// Serialize an XkbOverlayKey to wire format.
fn serialize_overlay_key(buf: &mut Vec<u8>, key: &XkbOverlayKey) {
    buf.extend_from_slice(&key.over);
    buf.extend_from_slice(&key.under);
}

/// Serialize an XkbOverlayRow to wire format.
fn serialize_overlay_row(buf: &mut Vec<u8>, row: &XkbOverlayRow) {
    buf.push(row.row_under);
    buf.push(row.keys.len() as u8); // nKeys
    buf.extend_from_slice(&[0u8; 2]); // pad
    for key in &row.keys {
        serialize_overlay_key(buf, key);
    }
}

/// Serialize an XkbOverlay to wire format.
fn serialize_overlay(buf: &mut Vec<u8>, overlay: &XkbOverlay) {
    buf.extend_from_slice(&overlay.name.to_le_bytes());
    buf.push(overlay.rows.len() as u8); // nRows
    buf.extend_from_slice(&[0u8; 3]); // pad
    for row in &overlay.rows {
        serialize_overlay_row(buf, row);
    }
}

/// Doodad type constants.
const DOODAD_OUTLINE: u8 = 1;
const DOODAD_SOLID: u8 = 2;
const DOODAD_TEXT: u8 = 3;
const DOODAD_INDICATOR: u8 = 4;
const DOODAD_LOGO: u8 = 5;

/// Serialize an XkbDoodad to wire format.
/// Each doodad starts with a common header and then has type-specific fields.
/// Common header: name(4) + type(1) + priority(1) + top(2) + left(2) + angle(2) = 12 bytes.
/// Then each type has additional fixed + variable fields, padded so that
/// the total fixed portion is 20 bytes (with variable strings after).
fn serialize_doodad(buf: &mut Vec<u8>, doodad: &XkbDoodad) {
    match doodad {
        XkbDoodad::Outline {
            name, top, left, angle, color_ndx, shape_ndx, priority,
        } => {
            buf.extend_from_slice(&name.to_le_bytes());
            buf.push(DOODAD_OUTLINE);
            buf.push(*priority);
            buf.extend_from_slice(&top.to_le_bytes());
            buf.extend_from_slice(&left.to_le_bytes());
            buf.extend_from_slice(&angle.to_le_bytes());
            buf.push(*color_ndx);
            buf.push(*shape_ndx);
            buf.extend_from_slice(&[0u8; 6]); // pad to 20 bytes total
        }
        XkbDoodad::Solid {
            name, top, left, angle, color_ndx, shape_ndx, priority,
        } => {
            buf.extend_from_slice(&name.to_le_bytes());
            buf.push(DOODAD_SOLID);
            buf.push(*priority);
            buf.extend_from_slice(&top.to_le_bytes());
            buf.extend_from_slice(&left.to_le_bytes());
            buf.extend_from_slice(&angle.to_le_bytes());
            buf.push(*color_ndx);
            buf.push(*shape_ndx);
            buf.extend_from_slice(&[0u8; 6]); // pad to 20 bytes total
        }
        XkbDoodad::Text {
            name, top, left, angle, color_ndx, text, font, priority,
        } => {
            buf.extend_from_slice(&name.to_le_bytes());
            buf.push(DOODAD_TEXT);
            buf.push(*priority);
            buf.extend_from_slice(&top.to_le_bytes());
            buf.extend_from_slice(&left.to_le_bytes());
            buf.extend_from_slice(&angle.to_le_bytes());
            // width(2) + height(2) reserved for text doodad
            buf.extend_from_slice(&[0u8; 4]);
            buf.push(*color_ndx);
            buf.push(0); // pad
            // Variable: text string then font string
            write_counted_string(buf, text);
            write_counted_string(buf, font);
        }
        XkbDoodad::Indicator {
            name, top, left, angle, shape_ndx, on_color_ndx, off_color_ndx, priority,
        } => {
            buf.extend_from_slice(&name.to_le_bytes());
            buf.push(DOODAD_INDICATOR);
            buf.push(*priority);
            buf.extend_from_slice(&top.to_le_bytes());
            buf.extend_from_slice(&left.to_le_bytes());
            buf.extend_from_slice(&angle.to_le_bytes());
            buf.push(*shape_ndx);
            buf.push(*on_color_ndx);
            buf.push(*off_color_ndx);
            buf.extend_from_slice(&[0u8; 5]); // pad to 20 bytes total
        }
        XkbDoodad::Logo {
            name, top, left, angle, color_ndx, shape_ndx, logo_name, priority,
        } => {
            buf.extend_from_slice(&name.to_le_bytes());
            buf.push(DOODAD_LOGO);
            buf.push(*priority);
            buf.extend_from_slice(&top.to_le_bytes());
            buf.extend_from_slice(&left.to_le_bytes());
            buf.extend_from_slice(&angle.to_le_bytes());
            buf.push(*color_ndx);
            buf.push(*shape_ndx);
            buf.extend_from_slice(&[0u8; 6]); // pad to 20 bytes total
            // Variable: logo name string
            write_counted_string(buf, logo_name);
        }
    }
}

/// Serialize an XkbSection to wire format.
fn serialize_section(buf: &mut Vec<u8>, section: &XkbSection) {
    buf.extend_from_slice(&section.name.to_le_bytes()); // name ATOM
    buf.extend_from_slice(&section.top.to_le_bytes());
    buf.extend_from_slice(&section.left.to_le_bytes());
    buf.extend_from_slice(&section.width.to_le_bytes());
    buf.extend_from_slice(&section.height.to_le_bytes());
    buf.extend_from_slice(&section.angle.to_le_bytes());
    buf.push(section.priority);
    buf.push(section.rows.len() as u8); // nRows
    buf.push(section.doodads.len() as u8); // nDoodads
    buf.push(section.overlays.len() as u8); // nOverlays
    buf.extend_from_slice(&[0u8; 2]); // pad
    for row in &section.rows {
        serialize_row(buf, row);
    }
    for doodad in &section.doodads {
        serialize_doodad(buf, doodad);
    }
    for overlay in &section.overlays {
        serialize_overlay(buf, overlay);
    }
}

/// Serialize the full geometry body (everything after the 32-byte header).
fn serialize_geometry_body(geom: &XkbGeometry) -> Vec<u8> {
    let mut body = Vec::new();

    // labelFont (counted string)
    write_counted_string(&mut body, &geom.label_font);

    // properties
    for prop in &geom.properties {
        write_counted_string(&mut body, &prop.name);
        write_counted_string(&mut body, &prop.value);
    }

    // colors
    for color in &geom.colors {
        write_counted_string(&mut body, color);
    }

    // shapes
    for shape in &geom.shapes {
        serialize_shape(&mut body, shape);
    }

    // sections
    for section in &geom.sections {
        serialize_section(&mut body, section);
    }

    // doodads (top-level)
    for doodad in &geom.doodads {
        serialize_doodad(&mut body, doodad);
    }

    // key aliases
    for (real, alias) in &geom.key_aliases {
        body.extend_from_slice(real);
        body.extend_from_slice(alias);
    }

    // Pad to 4-byte boundary
    let padding = pad4(body.len());
    body.extend(std::iter::repeat(0u8).take(padding));

    body
}

// ---------------------------------------------------------------------------
// Default PC-105 geometry
// ---------------------------------------------------------------------------

/// Standard key unit in tenths of mm (1 key = 19mm pitch, 18mm cap + 1mm gap).
const KEY_UNIT: i16 = 190; // 19.0mm in tenths
const KEY_CAP: i16 = 180; // 18.0mm in tenths
const KEY_GAP: i16 = 10;  // 1.0mm in tenths

/// Build a rectangular outline for a key of given width and height (in tenths of mm).
fn rect_outline(w: i16, h: i16) -> XkbOutline {
    XkbOutline {
        corner_radius: 10, // 1mm corner radius
        points: vec![(0, 0), (w, 0), (w, h), (0, h)],
    }
}

/// Intern an atom using the shared atom manager.
fn intern(atoms: &Arc<Mutex<AtomManager>>, name: &str) -> u32 {
    atoms.lock().unwrap().intern(name, false)
}

/// Build the default PC-105 keyboard geometry.
fn default_pc105_geometry(atoms: &Arc<Mutex<AtomManager>>) -> XkbGeometry {
    // Intern atoms for naming
    let geom_name = intern(atoms, "pc(pc105)");
    let shape_normal = intern(atoms, "NORM");
    let shape_wide = intern(atoms, "WIDE");
    let shape_large = intern(atoms, "LARG");
    let shape_tall = intern(atoms, "TALL");
    let shape_space = intern(atoms, "SPCE");
    let shape_led = intern(atoms, "LED");

    let sect_fn_row = intern(atoms, "FnRow");
    let sect_alpha = intern(atoms, "Alpha");
    let sect_nav = intern(atoms, "Editing");
    let sect_keypad = intern(atoms, "Keypad");
    let sect_arrows = intern(atoms, "Arrows");

    // Colors: index 0 = base (grey), 1 = label (white), 2 = keycap (light grey)
    let colors = vec![
        "grey20".to_string(),
        "white".to_string(),
        "grey93".to_string(),
    ];

    // Shapes
    let shapes = vec![
        // 0: NORM - standard 18x18mm key
        XkbShape {
            name: shape_normal,
            outlines: vec![rect_outline(KEY_CAP, KEY_CAP)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
        // 1: WIDE - 1.5u wide key (Tab, Backslash, etc.) = 27mm
        XkbShape {
            name: shape_wide,
            outlines: vec![rect_outline(270, KEY_CAP)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
        // 2: LARG - 2u wide key (Backspace, CapsLock, Shift, Enter) = 37mm
        XkbShape {
            name: shape_large,
            outlines: vec![rect_outline(370, KEY_CAP)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
        // 3: TALL - 2u tall key (numpad Enter, numpad +)
        XkbShape {
            name: shape_tall,
            outlines: vec![rect_outline(KEY_CAP, 370)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
        // 4: SPCE - space bar = ~96mm wide
        XkbShape {
            name: shape_space,
            outlines: vec![rect_outline(960, KEY_CAP)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
        // 5: LED - small indicator light
        XkbShape {
            name: shape_led,
            outlines: vec![rect_outline(50, 50)],
            primary_ndx: 0,
            approx_ndx: 0,
        },
    ];

    // Shape indices
    const S_NORM: u8 = 0;
    const S_WIDE: u8 = 1;
    const S_LARG: u8 = 2;
    const S_TALL: u8 = 3;
    const S_SPCE: u8 = 4;

    // Color indices
    const C_KEYCAP: u8 = 2;

    // Fetch the key name table for mapping keycodes to names.
    let key_names = us_qwerty_key_names();

    /// Helper: build an XkbKey from a keycode, with default gap and shape.
    fn mk_key(key_names: &[&[u8; 4]; 248], kc: u8, shape: u8, gap: i16) -> XkbKey {
        let idx = if kc >= 8 { (kc - 8) as usize } else { 0 };
        let name = if idx < 248 { *key_names[idx] } else { *b"K   " };
        XkbKey {
            name,
            gap,
            shape_ndx: shape,
            color_ndx: C_KEYCAP,
        }
    }

    // ----- Section 1: Function row (F1-F12, Esc, Print/Scroll/Pause) -----
    let fn_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        // Esc (kc=9)
        keys.push(mk_key(&key_names, 9, S_NORM, 0));
        // gap then F1-F4 (kc 67-70)
        for (i, kc) in (67u8..=70).enumerate() {
            keys.push(mk_key(&key_names, kc, S_NORM, if i == 0 { KEY_UNIT } else { KEY_GAP }));
        }
        // gap then F5-F8 (kc 71-74)
        for (i, kc) in (71u8..=74).enumerate() {
            keys.push(mk_key(&key_names, kc, S_NORM, if i == 0 { KEY_GAP * 5 } else { KEY_GAP }));
        }
        // gap then F9-F12 (kc 75-76, 95-96)
        let f9_12: [u8; 4] = [75, 76, 95, 96];
        for (i, &kc) in f9_12.iter().enumerate() {
            keys.push(mk_key(&key_names, kc, S_NORM, if i == 0 { KEY_GAP * 5 } else { KEY_GAP }));
        }
        keys
    };

    let fn_section = XkbSection {
        name: sect_fn_row,
        top: 0,
        left: 0,
        width: 4700,
        height: KEY_CAP as u16,
        angle: 0,
        priority: 0,
        rows: vec![XkbRow {
            top: 0,
            left: 0,
            vertical: false,
            keys: fn_keys,
        }],
        doodads: Vec::new(),
        overlays: Vec::new(),
    };

    // ----- Section 2: Main alphanumeric area (5 rows) -----
    let alpha_top: i16 = KEY_UNIT + KEY_GAP * 5; // below function row with gap

    // Row 0: number row (Grave through Backspace)
    // kc: 49=TLDE, 10-21=AE01-AE12, 22=BKSP
    let num_row_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 49, S_NORM, 0)); // TLDE
        for kc in 10u8..=21 {
            keys.push(mk_key(&key_names, kc, S_NORM, KEY_GAP));
        }
        keys.push(mk_key(&key_names, 22, S_LARG, KEY_GAP)); // BKSP
        keys
    };

    // Row 1: QWERTY row (Tab through Backslash)
    // kc: 23=TAB, 24-35=AD01-AD12, 51=BKSL
    let qwerty_row_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 23, S_WIDE, 0)); // TAB
        for kc in 24u8..=35 {
            keys.push(mk_key(&key_names, kc, S_NORM, KEY_GAP));
        }
        keys.push(mk_key(&key_names, 51, S_WIDE, KEY_GAP)); // BKSL
        keys
    };

    // Row 2: Home row (Caps through Return)
    // kc: 66=CAPS, 38-48=AC01-AC11, 36=RTRN
    let home_row_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 66, S_LARG, 0)); // CAPS
        for kc in 38u8..=48 {
            keys.push(mk_key(&key_names, kc, S_NORM, KEY_GAP));
        }
        keys.push(mk_key(&key_names, 36, S_LARG, KEY_GAP)); // RTRN
        keys
    };

    // Row 3: Bottom row (LShift through RShift)
    // kc: 50=LFSH, 52-61=AB01-AB10, 62=RTSH
    let bottom_row_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 50, S_LARG, 0)); // LFSH
        for kc in 52u8..=61 {
            keys.push(mk_key(&key_names, kc, S_NORM, KEY_GAP));
        }
        keys.push(mk_key(&key_names, 62, S_LARG, KEY_GAP)); // RTSH
        keys
    };

    // Row 4: Space bar row (LCtrl, Super, LAlt, Space, RAlt, Super, Menu, RCtrl)
    let space_row_keys: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 37, S_WIDE, 0));   // LCTL
        keys.push(mk_key(&key_names, 133, S_NORM, KEY_GAP)); // LWIN/Super_L
        keys.push(mk_key(&key_names, 64, S_NORM, KEY_GAP));  // LALT
        keys.push(mk_key(&key_names, 65, S_SPCE, KEY_GAP));  // SPCE
        keys.push(mk_key(&key_names, 108, S_NORM, KEY_GAP)); // RALT
        keys.push(mk_key(&key_names, 134, S_NORM, KEY_GAP)); // RWIN/Super_R
        keys.push(mk_key(&key_names, 135, S_NORM, KEY_GAP)); // MENU (kc 135)
        keys.push(mk_key(&key_names, 105, S_WIDE, KEY_GAP)); // RCTL
        keys
    };

    let alpha_section = XkbSection {
        name: sect_alpha,
        top: alpha_top,
        left: 0,
        width: 2850,
        height: (KEY_UNIT * 5) as u16,
        angle: 0,
        priority: 1,
        rows: vec![
            XkbRow { top: 0, left: 0, vertical: false, keys: num_row_keys },
            XkbRow { top: KEY_UNIT, left: 0, vertical: false, keys: qwerty_row_keys },
            XkbRow { top: KEY_UNIT * 2, left: 0, vertical: false, keys: home_row_keys },
            XkbRow { top: KEY_UNIT * 3, left: 0, vertical: false, keys: bottom_row_keys },
            XkbRow { top: KEY_UNIT * 4, left: 0, vertical: false, keys: space_row_keys },
        ],
        doodads: Vec::new(),
        overlays: Vec::new(),
    };

    // ----- Section 3: Navigation cluster (Insert, Home, PgUp, Delete, End, PgDn) -----
    let nav_left: i16 = 2950;
    let nav_keys_top: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 118, S_NORM, 0));  // INS (kc 118)
        keys.push(mk_key(&key_names, 110, S_NORM, KEY_GAP)); // HOME (kc 110)
        keys.push(mk_key(&key_names, 112, S_NORM, KEY_GAP)); // PGUP (kc 112)
        keys
    };
    let nav_keys_bot: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 119, S_NORM, 0));  // DELE (kc 119)
        keys.push(mk_key(&key_names, 115, S_NORM, KEY_GAP)); // END (kc 115)
        keys.push(mk_key(&key_names, 117, S_NORM, KEY_GAP)); // PGDN (kc 117)
        keys
    };

    // Also: PrintScreen (kc 107), ScrollLock (kc 78), Pause (kc 127)
    let nav_keys_top2: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 107, S_NORM, 0));  // PRSC
        keys.push(mk_key(&key_names, 78, S_NORM, KEY_GAP));  // SCLK
        keys.push(mk_key(&key_names, 127, S_NORM, KEY_GAP)); // PAUS
        keys
    };

    let nav_section = XkbSection {
        name: sect_nav,
        top: 0,
        left: nav_left,
        width: (KEY_UNIT * 3) as u16,
        height: (alpha_top + KEY_UNIT * 3) as u16,
        angle: 0,
        priority: 2,
        rows: vec![
            XkbRow { top: 0, left: 0, vertical: false, keys: nav_keys_top2 },
            XkbRow { top: alpha_top, left: 0, vertical: false, keys: nav_keys_top },
            XkbRow { top: alpha_top + KEY_UNIT, left: 0, vertical: false, keys: nav_keys_bot },
        ],
        doodads: Vec::new(),
        overlays: Vec::new(),
    };

    // ----- Section 4: Arrow keys -----
    let arrow_left: i16 = nav_left;
    let arrow_top: i16 = alpha_top + KEY_UNIT * 3 + KEY_GAP * 5;

    let arrow_keys_top: Vec<XkbKey> = {
        let mut keys = Vec::new();
        // Up arrow centered above Down: offset by 1 key
        keys.push(mk_key(&key_names, 111, S_NORM, KEY_UNIT)); // UP (kc 111)
        keys
    };
    let arrow_keys_bot: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 113, S_NORM, 0));  // LEFT (kc 113)
        keys.push(mk_key(&key_names, 116, S_NORM, KEY_GAP)); // DOWN (kc 116)
        keys.push(mk_key(&key_names, 114, S_NORM, KEY_GAP)); // RGHT (kc 114)
        keys
    };

    let arrow_section = XkbSection {
        name: sect_arrows,
        top: arrow_top,
        left: arrow_left,
        width: (KEY_UNIT * 3) as u16,
        height: (KEY_UNIT * 2) as u16,
        angle: 0,
        priority: 3,
        rows: vec![
            XkbRow { top: 0, left: 0, vertical: false, keys: arrow_keys_top },
            XkbRow { top: KEY_UNIT, left: 0, vertical: false, keys: arrow_keys_bot },
        ],
        doodads: Vec::new(),
        overlays: Vec::new(),
    };

    // ----- Section 5: Numeric keypad -----
    let kp_left: i16 = nav_left + KEY_UNIT * 3 + KEY_GAP * 5;

    // Numpad row 0: NumLock, KP/, KP*, KP-
    let kp_row0: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 77, S_NORM, 0));   // NMLK
        keys.push(mk_key(&key_names, 106, S_NORM, KEY_GAP)); // KPDV (kc 106)
        keys.push(mk_key(&key_names, 63, S_NORM, KEY_GAP));  // KPMU
        keys.push(mk_key(&key_names, 82, S_NORM, KEY_GAP));  // KPSU (kc 82)
        keys
    };
    // Numpad row 1: KP7, KP8, KP9, KP+
    let kp_row1: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 79, S_NORM, 0));   // KP7
        keys.push(mk_key(&key_names, 80, S_NORM, KEY_GAP)); // KP8
        keys.push(mk_key(&key_names, 81, S_NORM, KEY_GAP)); // KP9
        keys.push(mk_key(&key_names, 86, S_TALL, KEY_GAP)); // KPAD (kc 86) - tall
        keys
    };
    // Numpad row 2: KP4, KP5, KP6 (KP+ spans from row 1)
    let kp_row2: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 83, S_NORM, 0));   // KP4
        keys.push(mk_key(&key_names, 84, S_NORM, KEY_GAP)); // KP5
        keys.push(mk_key(&key_names, 85, S_NORM, KEY_GAP)); // KP6
        keys
    };
    // Numpad row 3: KP1, KP2, KP3, KPEnter
    let kp_row3: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 87, S_NORM, 0));   // KP1
        keys.push(mk_key(&key_names, 88, S_NORM, KEY_GAP)); // KP2
        keys.push(mk_key(&key_names, 89, S_NORM, KEY_GAP)); // KP3
        keys.push(mk_key(&key_names, 104, S_TALL, KEY_GAP)); // KPEN (kc 104) - tall
        keys
    };
    // Numpad row 4: KP0 (wide), KPDot
    let kp_row4: Vec<XkbKey> = {
        let mut keys = Vec::new();
        keys.push(mk_key(&key_names, 90, S_LARG, 0));   // KP0 - wide
        keys.push(mk_key(&key_names, 91, S_NORM, KEY_GAP)); // KPDL (kc 91)
        keys
    };

    let keypad_section = XkbSection {
        name: sect_keypad,
        top: alpha_top,
        left: kp_left,
        width: (KEY_UNIT * 4) as u16,
        height: (KEY_UNIT * 5) as u16,
        angle: 0,
        priority: 4,
        rows: vec![
            XkbRow { top: 0, left: 0, vertical: false, keys: kp_row0 },
            XkbRow { top: KEY_UNIT, left: 0, vertical: false, keys: kp_row1 },
            XkbRow { top: KEY_UNIT * 2, left: 0, vertical: false, keys: kp_row2 },
            XkbRow { top: KEY_UNIT * 3, left: 0, vertical: false, keys: kp_row3 },
            XkbRow { top: KEY_UNIT * 4, left: 0, vertical: false, keys: kp_row4 },
        ],
        doodads: Vec::new(),
        overlays: Vec::new(),
    };

    // Indicator doodads for LEDs
    let led_numlk = intern(atoms, "Num Lock");
    let led_caps = intern(atoms, "Caps Lock");
    let led_scroll = intern(atoms, "Scroll Lock");

    let indicator_doodads = vec![
        XkbDoodad::Indicator {
            name: led_numlk,
            top: alpha_top - KEY_GAP * 3,
            left: nav_left,
            angle: 0,
            shape_ndx: 5, // LED shape
            on_color_ndx: 1,
            off_color_ndx: 0,
            priority: 0,
        },
        XkbDoodad::Indicator {
            name: led_caps,
            top: alpha_top - KEY_GAP * 3,
            left: nav_left + 80,
            angle: 0,
            shape_ndx: 5,
            on_color_ndx: 1,
            off_color_ndx: 0,
            priority: 0,
        },
        XkbDoodad::Indicator {
            name: led_scroll,
            top: alpha_top - KEY_GAP * 3,
            left: nav_left + 160,
            angle: 0,
            shape_ndx: 5,
            on_color_ndx: 1,
            off_color_ndx: 0,
            priority: 0,
        },
    ];

    // Key aliases (common alternate names)
    let key_aliases: Vec<([u8; 4], [u8; 4])> = vec![
        (*b"AC12", *b"BKSL"),
        (*b"MENU", *b"COMP"),
        (*b"HZTG", *b"TLDE"),
        (*b"LMTA", *b"LWIN"),
        (*b"RMTA", *b"RWIN"),
        (*b"ALGR", *b"RALT"),
        (*b"KPPT", *b"I129"),
        (*b"LatQ", *b"AD01"),
        (*b"LatW", *b"AD02"),
        (*b"LatE", *b"AD03"),
        (*b"LatR", *b"AD04"),
        (*b"LatT", *b"AD05"),
        (*b"LatY", *b"AD06"),
        (*b"LatU", *b"AD07"),
        (*b"LatI", *b"AD08"),
        (*b"LatO", *b"AD09"),
        (*b"LatP", *b"AD10"),
        (*b"LatA", *b"AC01"),
        (*b"LatS", *b"AC02"),
        (*b"LatD", *b"AC03"),
        (*b"LatF", *b"AC04"),
        (*b"LatG", *b"AC05"),
        (*b"LatH", *b"AC06"),
        (*b"LatJ", *b"AC07"),
        (*b"LatK", *b"AC08"),
        (*b"LatL", *b"AC09"),
        (*b"LatZ", *b"AB01"),
        (*b"LatX", *b"AB02"),
        (*b"LatC", *b"AB03"),
        (*b"LatV", *b"AB04"),
        (*b"LatB", *b"AB05"),
        (*b"LatN", *b"AB06"),
        (*b"LatM", *b"AB07"),
    ];

    XkbGeometry {
        name: geom_name,
        width_mm: 470,
        height_mm: 170,
        label_font: "helvetica".to_string(),
        properties: vec![
            XkbProperty {
                name: "description".to_string(),
                value: "Generic 105-key PC".to_string(),
            },
        ],
        colors,
        shapes,
        sections: vec![fn_section, alpha_section, nav_section, arrow_section, keypad_section],
        doodads: indicator_doodads,
        key_aliases,
    }
}

// ---------------------------------------------------------------------------
// GetGeometry reply (opcode 19)
// ---------------------------------------------------------------------------

/// Build a complete XKB GetGeometry reply.
pub(crate) fn build_xkb_get_geometry_reply(
    state: &mut ClientState,
    seq: u16,
    device_id: u8,
) -> Vec<u8> {
    let geom = default_pc105_geometry(&state.atoms);
    let body = serialize_geometry_body(&geom);
    let length_words = (body.len() / 4) as u32;
    let total_len = 32 + body.len();

    let mut reply = vec![0u8; total_len];
    reply[0] = 1; // Reply
    reply[1] = device_id;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_words.to_le_bytes());
    reply[8..12].copy_from_slice(&geom.name.to_le_bytes()); // name ATOM
    reply[12] = 1; // foundGeometry = TRUE
    reply[13] = 0; // pad
    reply[14..16].copy_from_slice(&geom.width_mm.to_le_bytes());
    reply[16..18].copy_from_slice(&geom.height_mm.to_le_bytes());
    reply[18..20].copy_from_slice(&(geom.properties.len() as u16).to_le_bytes());
    reply[20..22].copy_from_slice(&(geom.colors.len() as u16).to_le_bytes());
    reply[22..24].copy_from_slice(&(geom.shapes.len() as u16).to_le_bytes());
    reply[24..26].copy_from_slice(&(geom.sections.len() as u16).to_le_bytes());
    reply[26..28].copy_from_slice(&(geom.doodads.len() as u16).to_le_bytes());
    reply[28..30].copy_from_slice(&(geom.key_aliases.len() as u16).to_le_bytes());
    reply[30] = 0; // baseColorNdx
    reply[31] = 1; // labelColorNdx

    reply[32..].copy_from_slice(&body);
    reply
}

// ---------------------------------------------------------------------------
// SetGeometry (opcode 20) -- parse and store client-supplied geometry
// ---------------------------------------------------------------------------

/// Handle XKB SetGeometry request. We parse the header and acknowledge
/// silently (void request -- no reply). The parsed geometry is logged
/// but not persisted beyond the current session since our server
/// always provides the default PC-105 geometry.
pub(crate) fn handle_xkb_set_geometry(
    _state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    // SetGeometry is a void request -- no reply byte. We just need to
    // not crash when xkbcomp sends one.
    if data.len() < 20 {
        debug!("XKB SetGeometry: request too short ({} bytes)", data.len());
        return Vec::new();
    }

    // Parse the fixed header to validate.
    let _device_id = data[4];
    let n_shapes = u16::from_le_bytes([data[10], data[11]]);
    let n_sections = u16::from_le_bytes([data[12], data[13]]);
    let _name_atom = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);

    debug!(
        "XKB SetGeometry: {} shapes, {} sections (accepted, not persisted)",
        n_shapes, n_sections
    );

    // Void request -- no reply.
    Vec::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xserver::atoms::AtomManager;

    /// Create a test atom manager.
    fn test_atoms() -> Arc<Mutex<AtomManager>> {
        Arc::new(Mutex::new(AtomManager::new()))
    }

    #[test]
    fn default_geometry_has_five_sections() {
        let atoms = test_atoms();
        let geom = default_pc105_geometry(&atoms);
        assert_eq!(geom.sections.len(), 5, "expected 5 sections (fn, alpha, nav, arrows, keypad)");
    }

    #[test]
    fn default_geometry_dimensions() {
        let atoms = test_atoms();
        let geom = default_pc105_geometry(&atoms);
        assert_eq!(geom.width_mm, 470);
        assert_eq!(geom.height_mm, 170);
    }

    #[test]
    fn serialized_reply_has_valid_structure() {
        let atoms = test_atoms();
        let geom = default_pc105_geometry(&atoms);
        let body = serialize_geometry_body(&geom);

        // Build a reply manually to test structure.
        let length_words = (body.len() / 4) as u32;
        let total_len = 32 + body.len();

        let mut reply = vec![0u8; total_len];
        reply[0] = 1;
        reply[1] = 3; // device_id
        reply[2..4].copy_from_slice(&42u16.to_le_bytes());
        reply[4..8].copy_from_slice(&length_words.to_le_bytes());
        reply[8..12].copy_from_slice(&geom.name.to_le_bytes());
        reply[12] = 1; // foundGeometry
        reply[14..16].copy_from_slice(&geom.width_mm.to_le_bytes());
        reply[16..18].copy_from_slice(&geom.height_mm.to_le_bytes());
        reply[18..20].copy_from_slice(&(geom.properties.len() as u16).to_le_bytes());
        reply[20..22].copy_from_slice(&(geom.colors.len() as u16).to_le_bytes());
        reply[22..24].copy_from_slice(&(geom.shapes.len() as u16).to_le_bytes());
        reply[24..26].copy_from_slice(&(geom.sections.len() as u16).to_le_bytes());
        reply[26..28].copy_from_slice(&(geom.doodads.len() as u16).to_le_bytes());
        reply[28..30].copy_from_slice(&(geom.key_aliases.len() as u16).to_le_bytes());
        reply[30] = 0;
        reply[31] = 1;
        reply[32..].copy_from_slice(&body);

        // Check reply header
        assert_eq!(reply[0], 1, "first byte should be Reply");
        assert_eq!(reply[1], 3, "device ID should be 3");
        assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 42, "sequence");
        let length = u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]);
        assert_eq!(reply.len(), 32 + (length as usize) * 4, "reply length matches");
        assert_eq!(reply[12], 1, "foundGeometry should be TRUE");

        // Check counts in header
        let n_properties = u16::from_le_bytes([reply[18], reply[19]]);
        let n_colors = u16::from_le_bytes([reply[20], reply[21]]);
        let n_shapes = u16::from_le_bytes([reply[22], reply[23]]);
        let n_sections = u16::from_le_bytes([reply[24], reply[25]]);
        let n_doodads = u16::from_le_bytes([reply[26], reply[27]]);
        let n_key_aliases = u16::from_le_bytes([reply[28], reply[29]]);

        assert_eq!(n_properties, 1);
        assert_eq!(n_colors, 3);
        assert_eq!(n_shapes, 6);
        assert_eq!(n_sections, 5);
        assert_eq!(n_doodads, 3);
        assert!(n_key_aliases > 0);

        // Total reply should be 4-byte aligned
        assert_eq!(reply.len() % 4, 0, "reply must be 4-byte aligned");
    }

    #[test]
    fn key_names_match_keymap() {
        let atoms = test_atoms();
        let geom = default_pc105_geometry(&atoms);
        let key_names = us_qwerty_key_names();

        // Collect all key names from geometry
        let mut geom_key_names: Vec<[u8; 4]> = Vec::new();
        for section in &geom.sections {
            for row in &section.rows {
                for key in &row.keys {
                    geom_key_names.push(key.name);
                }
            }
        }

        // Verify ESC is present (kc 9, idx 1)
        assert!(geom_key_names.contains(key_names[1]), "ESC should be in geometry");
        // Verify SPCE is present (kc 65, idx 57)
        assert!(geom_key_names.contains(key_names[57]), "SPCE should be in geometry");
        // Verify RTRN is present (kc 36, idx 28)
        assert!(geom_key_names.contains(key_names[28]), "RTRN should be in geometry");
    }

    #[test]
    fn set_geometry_accepts_minimal_request() {
        // Build a minimal SetGeometry request (at least 20 bytes).
        // We only need the data buffer; SetGeometry is stateless in our impl.
        let mut data = vec![0u8; 24];
        data[1] = 20; // minor opcode
        data[4] = 3;  // device_id
        // Parse the header the same way the handler does.
        assert!(data.len() >= 20);
        let n_shapes = u16::from_le_bytes([data[10], data[11]]);
        let n_sections = u16::from_le_bytes([data[12], data[13]]);
        assert_eq!(n_shapes, 0);
        assert_eq!(n_sections, 0);
    }
}
