//! Core X11 protocol handlers (opcodes 1-127).
//!
//! Each handler corresponds to a single X11 core protocol request. The
//! dispatcher [`handle_core_request`] routes based on the major opcode.

pub(crate) mod extensions;

use std::collections::HashMap;
use tracing::{debug, info, warn};
use x11_web_protocol::DisplayUpdate;

use super::client::ClientState;
use super::core::*;
use super::types::*;
use crate::framebuffer::Framebuffer;

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Dispatch a core X11 protocol request (opcodes 1-127) to the appropriate
/// handler function. Returns the response bytes (reply, event, or empty for
/// void requests).
pub(crate) fn handle_core_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;

    match major_opcode {
        1 => handle_create_window(state, data, seq),
        2 => handle_change_window_attributes(state, data),
        3 => handle_get_window_attributes(state, data, seq),
        4 => handle_destroy_window(state, data),
        5 => handle_destroy_subwindows(state, data),
        6 => {
            // ChangeSaveSet - no-op in our implementation
            Vec::new()
        }
        7 => handle_reparent_window(state, data, seq),
        8 => handle_map_window(state, data, seq),
        9 => handle_map_subwindows(state, data, seq),
        10 => handle_unmap_window(state, data, seq),
        11 => handle_unmap_subwindows(state, data, seq),
        12 => handle_configure_window(state, data, seq),
        13 => {
            // CirculateWindow - no-op for now (stacking order)
            Vec::new()
        }
        14 => handle_get_geometry(state, data, seq),
        15 => handle_query_tree(state, data, seq),
        16 => handle_intern_atom(state, data, seq),
        17 => handle_get_atom_name(state, data, seq),
        18 => handle_change_property(state, data),
        19 => handle_delete_property(state, data),
        20 => handle_get_property(state, data, seq),
        21 => handle_list_properties(state, data, seq),
        22 => handle_set_selection_owner(state, data),
        23 => handle_get_selection_owner(state, data, seq),
        24 => handle_convert_selection(state, data, seq),
        25 => handle_send_event(state, data),
        // Grab operations (opcodes 26-37) delegate to super::grab
        26 => super::grab::handle_grab_pointer(state, data, seq),
        27 => super::grab::handle_ungrab_pointer(state, data),
        28 => super::grab::handle_grab_button(state, data),
        29 => super::grab::handle_ungrab_button(state, data),
        30 => super::grab::handle_change_active_pointer_grab(state, data),
        31 => super::grab::handle_grab_keyboard(state, data, seq),
        32 => super::grab::handle_ungrab_keyboard(state, data),
        33 => super::grab::handle_grab_key(state, data),
        34 => super::grab::handle_ungrab_key(state, data),
        35 => super::grab::handle_allow_events(state, data),
        36 => super::grab::handle_grab_server(state, data),
        37 => super::grab::handle_ungrab_server(state, data),
        38 => handle_query_pointer(state, data, seq),
        39 => handle_get_motion_events(state, data, seq),
        40 => handle_translate_coordinates(state, data, seq),
        41 => handle_warp_pointer(state, data, seq),
        42 => handle_set_input_focus(state, data),
        43 => handle_get_input_focus(state, data, seq),
        44 => handle_query_keymap(state, seq),
        45 => handle_open_font(state, data),
        46 => handle_close_font(state, data),
        47 => handle_query_font(state, data, seq),
        48 => handle_query_text_extents(state, data, seq),
        49 => handle_list_fonts(state, data, seq),
        50 => handle_list_fonts_with_info(seq),
        51 => {
            // SetFontPath - no-op
            Vec::new()
        }
        52 => handle_get_font_path(seq),
        53 => handle_create_pixmap(state, data),
        54 => handle_free_pixmap(state, data),
        55 => handle_create_gc(state, data),
        56 => handle_change_gc(state, data),
        57 => handle_copy_gc(state, data),
        58 => handle_set_dashes(state, data),
        59 => handle_set_clip_rectangles(state, data),
        60 => handle_free_gc(state, data),
        61 => handle_clear_area(state, data, seq),
        62 => handle_copy_area(state, data),
        63 => handle_copy_plane(state, data),
        64 => handle_poly_point(state, data),
        65 => handle_poly_line(state, data),
        66 => handle_poly_segment(state, data),
        67 => handle_poly_rectangle(state, data),
        68 => handle_poly_arc(state, data),
        69 => handle_fill_poly(state, data),
        70 => handle_poly_fill_rectangle(state, data),
        71 => handle_poly_fill_arc(state, data),
        72 => handle_put_image(state, data),
        73 => handle_get_image(state, data, seq),
        74 => handle_poly_text8(state, data),
        75 => handle_poly_text16(state, data),
        76 => handle_image_text8(state, data),
        77 => handle_image_text16(state, data),
        78 => handle_create_colormap(state, data),
        79 => {
            // FreeColormap - no-op for TrueColor
            Vec::new()
        }
        80 => handle_copy_colormap_and_free(state, data, seq),
        81 => handle_install_colormap(seq),
        82 => handle_uninstall_colormap(seq),
        83 => handle_list_installed_colormaps(state, seq),
        84 => handle_alloc_color(state, data, seq),
        85 => handle_alloc_named_color(state, data, seq),
        86 => handle_alloc_color_cells(state, data, seq),
        87 => handle_alloc_color_planes(state, data, seq),
        88 => {
            // FreeColors - no-op for TrueColor
            Vec::new()
        }
        89 => {
            // StoreColors - no-op for TrueColor
            Vec::new()
        }
        90 => {
            // StoreNamedColor - no-op for TrueColor
            Vec::new()
        }
        91 => handle_query_colors(state, data, seq),
        92 => handle_lookup_color(state, data, seq),
        93 => handle_create_cursor(state, data),
        94 => handle_create_glyph_cursor(state, data),
        95 => handle_free_cursor(state, data),
        96 => {
            // RecolorCursor - no-op
            Vec::new()
        }
        97 => handle_query_best_size(data, seq),
        98 => handle_query_extension(state, data, seq),
        99 => handle_list_extensions(seq),
        100 => handle_change_keyboard_mapping(state, data, seq),
        101 => handle_get_keyboard_mapping(data, seq),
        102 => handle_change_keyboard_control(state, data),
        103 => handle_get_keyboard_control(state, seq),
        104 => {
            // Bell - no-op
            Vec::new()
        }
        105 => handle_change_pointer_control(state, data),
        106 => handle_get_pointer_control(state, seq),
        107 => handle_set_screen_saver(state, data),
        108 => handle_get_screen_saver(state, seq),
        109 => {
            // ChangeHosts - no-op (access control stub)
            Vec::new()
        }
        110 => handle_list_hosts(seq),
        111 => {
            // SetAccessControl - no-op
            Vec::new()
        }
        112 => handle_set_close_down_mode(state, data),
        113 => handle_kill_client(state, data),
        114 => handle_rotate_properties(state, data),
        115 => {
            // ForceScreenSaver - no-op
            Vec::new()
        }
        116 => handle_set_pointer_mapping(seq),
        117 => handle_get_pointer_mapping(seq),
        118 => handle_set_modifier_mapping(seq),
        119 => handle_get_modifier_mapping(seq),
        127 => {
            // NoOperation
            Vec::new()
        }
        _ => {
            warn!("Unhandled core X11 request opcode: {major_opcode} minor: {_minor}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Map X11 cursor font glyph index to CSS cursor name.
fn glyph_to_css_cursor(glyph: u16) -> &'static str {
    match glyph {
        2 | 30 | 68 => "default",    // arrow / left_ptr
        24 | 34 => "crosshair",      // cross / crosshair
        52 => "not-allowed",          // circle
        58 | 70 => "pointer",         // hand2 / hand1
        92 => "wait",                 // watch
        130 => "text",                // xterm
        132 => "move",                // fleur
        138 => "help",                // question_arrow
        116 => "col-resize",          // sb_h_double_arrow
        120 => "row-resize",          // sb_v_double_arrow
        12 => "s-resize",             // bottom_side
        14 => "sw-resize",            // bottom_left_corner
        16 => "se-resize",            // bottom_right_corner
        134 => "n-resize",            // top_side
        136 => "nw-resize",           // top_left_corner
        100 => "ne-resize",           // top_right_corner
        108 => "w-resize",            // left_side
        96 => "e-resize",             // right_side
        _ => "default",
    }
}

/// Resolve the effective cursor for a window and emit CursorChanged to the frontend.
fn emit_cursor_changed(state: &mut ClientState, wid: u32) {
    // Resolve the CSS cursor name from the window's cursor resource
    let css_cursor = state.windows.get(&wid)
        .and_then(|w| w.cursor)
        .and_then(|cid| state.cursors.get(&cid))
        .cloned()
        .unwrap_or_else(|| "default".to_string());

    // Walk up to the top-level ancestor
    let mut target = wid;
    for _ in 0..10 {
        match state.windows.get(&target) {
            Some(w) if w.parent != state.root_window && w.parent != 0 => {
                target = w.parent;
            }
            _ => break,
        }
    }

    if let Some(wid_str) = state.window_uuid(target) {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::CursorChanged {
                window_id: wid_str,
                cursor: css_cursor,
            },
        ));
    }
}

/// Walk from `start` up through `parent` links collecting the chain of
/// window IDs until we hit the root window or fall off the tree.
fn ancestor_chain(windows: &HashMap<u32, WindowState>, start: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut cur = start;
    for _ in 0..32 {
        chain.push(cur);
        match windows.get(&cur).map(|w| w.parent) {
            Some(p) if p != 0 && p != cur => cur = p,
            _ => break,
        }
    }
    chain
}

/// Check if window `child` is a descendant of window `ancestor`.
fn is_descendant_of(windows: &HashMap<u32, WindowState>, child: u32, ancestor: u32) -> bool {
    let mut current = child;
    for _ in 0..20 {
        let parent = match windows.get(&current) {
            Some(w) => w.parent,
            None => return false,
        };
        if parent == ancestor {
            return true;
        }
        if parent == 0 {
            return false;
        }
        current = parent;
    }
    false
}

/// Parse a named color to RGB. Returns (r16, g16, b16) in 16-bit values.
fn parse_color_name(name: &str) -> (u16, u16, u16) {
    match name.to_lowercase().as_str() {
        "white" => (0xFFFF, 0xFFFF, 0xFFFF),
        "black" => (0, 0, 0),
        "red" => (0xFFFF, 0, 0),
        "green" => (0, 0xFFFF, 0),
        "blue" => (0, 0, 0xFFFF),
        "yellow" => (0xFFFF, 0xFFFF, 0),
        "cyan" => (0, 0xFFFF, 0xFFFF),
        "magenta" => (0xFFFF, 0, 0xFFFF),
        "gray" | "grey" => (0xBEBE, 0xBEBE, 0xBEBE),
        "light gray" | "light grey" | "lightgray" | "lightgrey" => (0xD3D3, 0xD3D3, 0xD3D3),
        "dark gray" | "dark grey" | "darkgray" | "darkgrey" => (0xA9A9, 0xA9A9, 0xA9A9),
        "orange" => (0xFFFF, 0xA5A5, 0),
        "brown" => (0xA5A5, 0x2A2A, 0x2A2A),
        "pink" => (0xFFFF, 0xC0C0, 0xCBCB),
        "purple" => (0x8080, 0, 0x8080),
        "navy" => (0, 0, 0x8080),
        "olive" => (0x8080, 0x8080, 0),
        "teal" => (0, 0x8080, 0x8080),
        "maroon" => (0x8080, 0, 0),
        "silver" => (0xC0C0, 0xC0C0, 0xC0C0),
        "aqua" => (0, 0xFFFF, 0xFFFF),
        "lime" => (0, 0xFFFF, 0),
        "fuchsia" => (0xFFFF, 0, 0xFFFF),
        _ => {
            // Try to parse hex format: #RRGGBB or #RGB
            if name.starts_with('#') && name.len() == 7 {
                let r = u16::from_str_radix(&name[1..3], 16).unwrap_or(0);
                let g = u16::from_str_radix(&name[3..5], 16).unwrap_or(0);
                let b = u16::from_str_radix(&name[5..7], 16).unwrap_or(0);
                (r * 257, g * 257, b * 257)
            } else {
                (0, 0, 0) // default to black for unknown colors
            }
        }
    }
}

/// Map X11 keycode to (normal_keysym, shifted_keysym).
/// Based on standard US keyboard layout.
fn keycode_to_keysym(keycode: u8) -> (u32, u32) {
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
        10 => (0x31, 0x21), // 1 !
        11 => (0x32, 0x40), // 2 @
        12 => (0x33, 0x23), // 3 #
        13 => (0x34, 0x24), // 4 $
        14 => (0x35, 0x25), // 5 %
        15 => (0x36, 0x5e), // 6 ^
        16 => (0x37, 0x26), // 7 &
        17 => (0x38, 0x2a), // 8 *
        18 => (0x39, 0x28), // 9 (
        19 => (0x30, 0x29), // 0 )
        20 => (0x2d, 0x5f), // - _
        21 => (0x3d, 0x2b), // = +
        22 => (XK_BACKSPACE, XK_BACKSPACE),
        23 => (XK_TAB, XK_TAB),
        24 => (0x71, 0x51), // q Q
        25 => (0x77, 0x57), // w W
        26 => (0x65, 0x45), // e E
        27 => (0x72, 0x52), // r R
        28 => (0x74, 0x54), // t T
        29 => (0x79, 0x59), // y Y
        30 => (0x75, 0x55), // u U
        31 => (0x69, 0x49), // i I
        32 => (0x6f, 0x4f), // o O
        33 => (0x70, 0x50), // p P
        34 => (0x5b, 0x7b), // [ {
        35 => (0x5d, 0x7d), // ] }
        36 => (XK_RETURN, XK_RETURN),
        37 => (XK_CONTROL_L, XK_CONTROL_L),
        38 => (0x61, 0x41), // a A
        39 => (0x73, 0x53), // s S
        40 => (0x64, 0x44), // d D
        41 => (0x66, 0x46), // f F
        42 => (0x67, 0x47), // g G
        43 => (0x68, 0x48), // h H
        44 => (0x6a, 0x4a), // j J
        45 => (0x6b, 0x4b), // k K
        46 => (0x6c, 0x4c), // l L
        47 => (0x3b, 0x3a), // ; :
        48 => (0x27, 0x22), // ' "
        49 => (0x60, 0x7e), // ` ~
        50 => (XK_SHIFT_L, XK_SHIFT_L),
        51 => (0x5c, 0x7c), // \ |
        52 => (0x7a, 0x5a), // z Z
        53 => (0x78, 0x58), // x X
        54 => (0x63, 0x43), // c C
        55 => (0x76, 0x56), // v V
        56 => (0x62, 0x42), // b B
        57 => (0x6e, 0x4e), // n N
        58 => (0x6d, 0x4d), // m M
        59 => (0x2c, 0x3c), // , <
        60 => (0x2e, 0x3e), // . >
        61 => (0x2f, 0x3f), // / ?
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

fn parse_gc_values(gc: &mut GcState, value_mask: u32, data: &[u8]) {
    let mut offset = 0;
    for bit in 0..23 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => gc.function = val as u8,
                    1 => gc.plane_mask = val,
                    2 => gc.foreground = val,
                    3 => gc.background = val,
                    4 => gc.line_width = val as u16,
                    5 => gc.line_style = val as u8,
                    6 => gc.cap_style = val as u8,
                    7 => gc.join_style = val as u8,
                    8 => gc.fill_style = val as u8,
                    9 => gc.fill_rule = val as u8,
                    10 => gc.tile = val,
                    11 => gc.stipple = val,
                    12 => gc.ts_x = val as i16,
                    13 => gc.ts_y = val as i16,
                    14 => gc.font_id = val,
                    15 => gc.subwindow_mode = val as u8,
                    16 => gc.graphics_exposures = val != 0,
                    17 => gc.clip_x = val as i16,
                    18 => gc.clip_y = val as i16,
                    19 => gc.clip_mask = val,
                    20 => gc.dash_offset = val as u16,
                    21 => gc.dashes = val as u8,
                    22 => gc.arc_mode = val as u8,
                    _ => {}
                }
                offset += 4;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Opcode 1: CreateWindow
// ---------------------------------------------------------------------------

fn handle_create_window(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let parent = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let width = u16::from_le_bytes([data[16], data[17]]);
    let height = u16::from_le_bytes([data[18], data[19]]);
    let border_width = u16::from_le_bytes([data[20], data[21]]);
    let class = u16::from_le_bytes([data[22], data[23]]);
    let visual = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let value_mask = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

    let mut background_pixel = 0u32;
    let mut event_mask = 0u32;
    let mut override_redirect = false;
    let mut cursor_id: Option<u32> = None;

    // Parse value list
    let mut offset = 32;
    for bit in 0..15 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => {} // background-pixmap
                    1 => background_pixel = val,
                    2 => {} // border-pixmap
                    3 => {} // border-pixel
                    4 => {} // bit-gravity
                    5 => {} // win-gravity
                    6 => {} // backing-store
                    7 => {} // backing-planes
                    8 => {} // backing-pixel
                    9 => override_redirect = val != 0,
                    10 => {} // save-under
                    11 => event_mask = val,
                    12 => {} // do-not-propagate-mask
                    13 => {} // colormap
                    14 => if val != 0 { cursor_id = Some(val); }
                    _ => {}
                }
                offset += 4;
            }
        }
    }

    let use_visual = if visual == 0 { ROOT_VISUAL } else { visual };

    info!("CreateWindow: id={wid:#x} parent={parent:#x} {x},{y} {width}x{height} depth={} class={class} visual={visual:#x} bg={background_pixel:#x}", data[1]);

    state.windows.insert(
        wid,
        WindowState {
            id: wid,
            parent,
            x,
            y,
            width,
            height,
            border_width,
            visual: use_visual,
            class,
            mapped: false,
            event_mask,
            background_pixel,
            override_redirect,
            redirected: false,
            framebuffer: Framebuffer::new(width as u32, height as u32),
            properties: HashMap::new(),
            owner_client_id: state.client_id.clone(),
            cursor: cursor_id,
        },
    );

    // Set _NET_FRAME_EXTENTS = (0,0,0,0) on new windows -- GTK3 checks this.
    let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties.insert(atom_frame, PropertyValue {
            prop_type: 6, // CARDINAL
            format: 32,
            data: vec![0; 16], // left, right, top, bottom = 0
        });
    }

    let is_top_level = parent == state.root_window && class == 1 && !override_redirect;
    let wid_str = state.get_or_create_window_uuid(wid);
    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowCreated {
            window_id: wid_str,
            x,
            y,
            width,
            height,
            is_top_level,
        },
    ));

    Vec::new() // No reply for CreateWindow
}

// ---------------------------------------------------------------------------
// Opcode 2: ChangeWindowAttributes
// ---------------------------------------------------------------------------

fn handle_change_window_attributes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    let mut cursor_changed = false;
    if let Some(win) = state.windows.get_mut(&wid) {
        let mut offset = 12;
        for bit in 0..15 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        1 => win.background_pixel = val,
                        11 => {
                            win.event_mask = val;
                            // SubstructureRedirectMask = bit 20 = 0x0010_0000
                            const SUBSTRUCTURE_REDIRECT_MASK: u32 = 0x0010_0000;
                            if wid == state.root_window && (val & SUBSTRUCTURE_REDIRECT_MASK) != 0 {
                                info!(
                                    "Client {} registering as window manager (SubstructureRedirectMask on root)",
                                    state.client_id
                                );
                                if let Ok(mut wm) = state.wm_state.lock() {
                                    wm.client_id = Some(state.client_id.clone());
                                    wm.event_tx = Some(state.wm_events_tx.clone());
                                }
                            }
                        }
                        14 => {
                            let new_cursor = if val == 0 { None } else { Some(val) };
                            if win.cursor != new_cursor {
                                win.cursor = new_cursor;
                                cursor_changed = true;
                            }
                        }
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }
    }

    if cursor_changed {
        emit_cursor_changed(state, wid);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 3: GetWindowAttributes
// ---------------------------------------------------------------------------

fn handle_get_window_attributes(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let win = match state.windows.get(&wid) {
        Some(w) => w,
        None => return build_error(3, seq, wid, 3, 0), // BadWindow
    };

    let mut reply = vec![0u8; 44];
    reply[0] = 1; // Reply
    reply[1] = 0; // backing-store: NotUseful
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&3u32.to_le_bytes()); // length = 3 extra u32s
    reply[8..12].copy_from_slice(&win.visual.to_le_bytes());     // visual (4 bytes)
    reply[12..14].copy_from_slice(&win.class.to_le_bytes());     // class (2 bytes)
    reply[14] = 0; // bit_gravity
    reply[15] = 0; // win_gravity
    reply[16..20].copy_from_slice(&0u32.to_le_bytes()); // backing_planes
    reply[20..24].copy_from_slice(&0u32.to_le_bytes()); // backing_pixel
    reply[24] = 0; // save_under = false
    reply[25] = 1; // map_is_installed = true
    reply[26] = if win.mapped { 2 } else { 0 }; // map_state: Viewable or Unmapped
    reply[27] = if win.override_redirect { 1 } else { 0 };
    reply[28..32].copy_from_slice(&ROOT_COLORMAP.to_le_bytes()); // colormap
    reply[32..36].copy_from_slice(&win.event_mask.to_le_bytes()); // all_event_masks
    reply[36..40].copy_from_slice(&0u32.to_le_bytes()); // your_event_mask
    reply[40..42].copy_from_slice(&0u16.to_le_bytes()); // do_not_propagate_mask
    // bytes 42-43: unused padding

    reply
}

// ---------------------------------------------------------------------------
// Opcode 4: DestroyWindow
// ---------------------------------------------------------------------------

fn handle_destroy_window(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.windows.remove(&wid);
    state.gtk_menu_paths.remove(&wid);
    state.menu_tracker.window_index().unregister(wid);
    if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
        state.window_router.unregister_all(&[uuid.clone()]);
        state.menu_tracker.detach(&uuid);
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowDestroyed { window_id: uuid },
        ));
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 5: DestroySubwindows
// ---------------------------------------------------------------------------

fn handle_destroy_subwindows(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let parent = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Collect all direct children first
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent)
        .map(|w| w.id)
        .collect();

    // Recursively collect all descendants (depth-first)
    let mut all_descendants = Vec::new();
    let mut stack = children.clone();
    while let Some(wid) = stack.pop() {
        all_descendants.push(wid);
        let grandchildren: Vec<u32> = state
            .windows
            .values()
            .filter(|w| w.parent == wid)
            .map(|w| w.id)
            .collect();
        stack.extend(grandchildren);
    }

    // Destroy all descendants
    for wid in all_descendants {
        state.windows.remove(&wid);
        state.gtk_menu_paths.remove(&wid);
        state.menu_tracker.window_index().unregister(wid);
        if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
            state.window_router.unregister_all(&[uuid.clone()]);
            state.menu_tracker.detach(&uuid);
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowDestroyed { window_id: uuid },
            ));
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 7: ReparentWindow
// ---------------------------------------------------------------------------

fn handle_reparent_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let new_parent = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);

    // Unmap the window first if mapped
    let was_mapped = state.windows.get(&window).map_or(false, |w| w.mapped);
    if was_mapped {
        if let Some(win) = state.windows.get_mut(&window) {
            win.mapped = false;
        }
    }

    // Update parent and position
    if let Some(win) = state.windows.get_mut(&window) {
        win.parent = new_parent;
        win.x = x;
        win.y = y;
    }

    // Send ReparentNotify event
    let mut event = [0u8; 32];
    event[0] = REPARENT_NOTIFY_EVENT;
    event[2..4].copy_from_slice(&seq.to_le_bytes());
    event[4..8].copy_from_slice(&window.to_le_bytes()); // event window
    event[8..12].copy_from_slice(&window.to_le_bytes()); // window
    event[12..16].copy_from_slice(&new_parent.to_le_bytes()); // parent
    event[16..18].copy_from_slice(&x.to_le_bytes());
    event[18..20].copy_from_slice(&y.to_le_bytes());
    let override_redirect = state.windows.get(&window).map_or(false, |w| w.override_redirect);
    event[20] = if override_redirect { 1 } else { 0 };

    // If the window was mapped before reparenting, re-map it
    if was_mapped {
        if let Some(win) = state.windows.get_mut(&window) {
            win.mapped = true;
        }
    }

    event.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 8: MapWindow
// ---------------------------------------------------------------------------

fn handle_map_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    info!("MapWindow called: wid={wid:#x} exists={}", state.windows.contains_key(&wid));

    let mut events = Vec::new();

    if !state.windows.contains_key(&wid) {
        warn!("MapWindow: id={wid:#x} NOT FOUND in client {}", state.client_id);
        return events;
    }

    // Check if this is a top-level window (parent == root) and a WM is active.
    // If so, redirect as a MapRequest event to the WM instead of mapping directly.
    // override_redirect windows bypass the WM redirect.
    let is_top_level = state.windows.get(&wid).map_or(false, |w| w.parent == state.root_window);
    let is_override_redirect = state.windows.get(&wid).map_or(false, |w| w.override_redirect);

    if is_top_level && !is_override_redirect {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                // Only redirect if the WM is a *different* client
                wm.client_id.as_ref().map_or(false, |id| id != &state.client_id)
            } else {
                false
            }
        };

        if should_redirect {
            info!(
                "MapWindow: redirecting wid={wid:#x} as MapRequest to WM"
            );
            // Build MapRequest event (code 20)
            let mut map_request = [0u8; 32];
            map_request[0] = MAP_REQUEST_EVENT;
            // map_request[1] = 0; // unused
            // sequence number will be the WM's -- but we use 0 since the server
            // inserts events asynchronously.
            map_request[4..8].copy_from_slice(&state.root_window.to_le_bytes()); // parent
            map_request[8..12].copy_from_slice(&wid.to_le_bytes()); // window

            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(map_request.to_vec());
                }
            }
            // Don't map the window -- the WM will do it.
            return events;
        }
    }

    let Some(wid_str) = state.window_uuid(wid) else {
        warn!("MapWindow: no UUID for {wid:#x}, skipping");
        return events;
    };
    let wm_state_atom = state.intern_atom("WM_STATE", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        info!("MapWindow: id={wid:#x} {}x{} mapped={}", win.width, win.height, win.mapped);
        let is_top_level = win.parent == state.root_window && win.class == 1 && !win.override_redirect;
        win.mapped = true;

        // Auto-fill the framebuffer with the window's background pixel.
        let w = win.width;
        let h = win.height;
        let bg = win.background_pixel;
        win.framebuffer.fill_rect(0, 0, w, h, bg);

        // Set WM_STATE = NormalState for top-level windows (apps check this)
        if is_top_level {
            let mut wm_state_data = vec![0u8; 8];
            wm_state_data[0..4].copy_from_slice(&1u32.to_le_bytes()); // NormalState
            win.properties.insert(wm_state_atom, PropertyValue {
                prop_type: wm_state_atom,
                format: 32,
                data: wm_state_data,
            });
        }

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowMapped { window_id: wid_str.clone(), is_top_level },
        ));

        // Send MapNotify event
        let mut map_event = [0u8; 32];
        map_event[0] = MAP_NOTIFY_EVENT;
        map_event[2..4].copy_from_slice(&seq.to_le_bytes());
        map_event[4..8].copy_from_slice(&wid.to_le_bytes()); // event window
        map_event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
        map_event[12] = if win.override_redirect { 1 } else { 0 };
        events.extend_from_slice(&map_event);

        // Send Expose event
        let width = win.width;
        let height = win.height;
        let mut expose_event = [0u8; 32];
        expose_event[0] = EXPOSE_EVENT;
        expose_event[2..4].copy_from_slice(&seq.to_le_bytes());
        expose_event[4..8].copy_from_slice(&wid.to_le_bytes());
        // x=0, y=0 already zero
        expose_event[12..14].copy_from_slice(&width.to_le_bytes());
        expose_event[14..16].copy_from_slice(&height.to_le_bytes());
        // count = 0
        events.extend_from_slice(&expose_event);

        // Also send Expose to all mapped descendant windows.
        let descendants: Vec<(u32, u16, u16)> = state
            .windows
            .values()
            .filter(|w| w.mapped && w.id != wid && is_descendant_of(&state.windows, w.id, wid))
            .map(|w| (w.id, w.width, w.height))
            .collect();

        if !descendants.is_empty() {
        }

        for (desc_id, dw, dh) in descendants {
            let mut exp = [0u8; 32];
            exp[0] = EXPOSE_EVENT;
            exp[2..4].copy_from_slice(&seq.to_le_bytes());
            exp[4..8].copy_from_slice(&desc_id.to_le_bytes());
            exp[12..14].copy_from_slice(&dw.to_le_bytes());
            exp[14..16].copy_from_slice(&dh.to_le_bytes());
            events.extend_from_slice(&exp);
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 9: MapSubwindows
// ---------------------------------------------------------------------------

fn handle_map_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let parent = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Collect child window IDs first to avoid borrow issues
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && !w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        // Construct a fake MapWindow request for each child
        let mut fake_data = [0u8; 8];
        fake_data[0] = 8; // MapWindow opcode
        fake_data[2..4].copy_from_slice(&2u16.to_le_bytes()); // length = 2
        fake_data[4..8].copy_from_slice(&child_id.to_le_bytes());
        let events = handle_map_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}

// ---------------------------------------------------------------------------
// Opcode 10: UnmapWindow
// ---------------------------------------------------------------------------

fn handle_unmap_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut events = Vec::new();

    if let Some(win) = state.windows.get_mut(&wid) {
        win.mapped = false;
        if let Some(uuid) = state.window_uuid(wid) {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowUnmapped { window_id: uuid },
            ));
        }

        let mut event = [0u8; 32];
        event[0] = UNMAP_NOTIFY_EVENT;
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&wid.to_le_bytes());
        event[8..12].copy_from_slice(&wid.to_le_bytes());
        events.extend_from_slice(&event);
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 11: UnmapSubwindows
// ---------------------------------------------------------------------------

fn handle_unmap_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let parent = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Collect all mapped children
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        let mut fake_data = [0u8; 8];
        fake_data[0] = 10; // UnmapWindow opcode
        fake_data[2..4].copy_from_slice(&2u16.to_le_bytes());
        fake_data[4..8].copy_from_slice(&child_id.to_le_bytes());
        let events = handle_unmap_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}

// ---------------------------------------------------------------------------
// Opcode 12: ConfigureWindow
// ---------------------------------------------------------------------------

fn handle_configure_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u16::from_le_bytes([data[8], data[9]]);

    // Check if this is a top-level window that should be redirected to the WM.
    let is_top_level = state.windows.get(&wid).map_or(false, |w| w.parent == state.root_window);
    let is_override_redirect = state.windows.get(&wid).map_or(false, |w| w.override_redirect);

    if is_top_level && !is_override_redirect {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                wm.client_id.as_ref().map_or(false, |id| id != &state.client_id)
            } else {
                false
            }
        };

        if should_redirect {
            info!("ConfigureWindow: redirecting wid={wid:#x} as ConfigureRequest to WM");

            // Parse the values from the request to populate the ConfigureRequest event.
            let mut x: i16 = 0;
            let mut y: i16 = 0;
            let mut width: u16 = 0;
            let mut height: u16 = 0;
            let mut border_width: u16 = 0;
            let mut sibling: u32 = 0;
            let mut stack_mode: u8 = 0;

            // Pre-fill with current values from the window
            if let Some(win) = state.windows.get(&wid) {
                x = win.x;
                y = win.y;
                width = win.width;
                height = win.height;
                border_width = win.border_width;
            }

            let mut offset = 12;
            for bit in 0..7u16 {
                if value_mask & (1 << bit) != 0 {
                    if offset + 4 <= data.len() {
                        let val = u32::from_le_bytes([
                            data[offset], data[offset + 1],
                            data[offset + 2], data[offset + 3],
                        ]);
                        match bit {
                            0 => x = val as i16,
                            1 => y = val as i16,
                            2 => width = val as u16,
                            3 => height = val as u16,
                            4 => border_width = val as u16,
                            5 => sibling = val,
                            6 => stack_mode = val as u8,
                            _ => {}
                        }
                        offset += 4;
                    }
                }
            }

            // Build ConfigureRequest event (code 23)
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_REQUEST_EVENT;
            event[1] = stack_mode; // detail = stack-mode
            // sequence = 0 (asynchronous server event)
            event[4..8].copy_from_slice(&state.root_window.to_le_bytes()); // parent
            event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
            event[12..16].copy_from_slice(&sibling.to_le_bytes()); // sibling
            event[16..18].copy_from_slice(&x.to_le_bytes());
            event[18..20].copy_from_slice(&y.to_le_bytes());
            event[20..22].copy_from_slice(&width.to_le_bytes());
            event[22..24].copy_from_slice(&height.to_le_bytes());
            event[24..26].copy_from_slice(&border_width.to_le_bytes());
            event[26..28].copy_from_slice(&value_mask.to_le_bytes());

            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(event.to_vec());
                }
            }
            return Vec::new();
        }
    }

    let mut offset = 12;
    let mut changed = false;
    let wid_str = state.window_uuid(wid);

    if let Some(win) = state.windows.get_mut(&wid) {
        for bit in 0..7 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        0 => {
                            win.x = val as i16;
                            changed = true;
                        }
                        1 => {
                            win.y = val as i16;
                            changed = true;
                        }
                        2 => {
                            win.width = val as u16;
                            changed = true;
                        }
                        3 => {
                            win.height = val as u16;
                            changed = true;
                        }
                        4 => {
                            win.border_width = val as u16;
                        }
                        5 => {} // sibling
                        6 => {} // stack-mode
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }

        if changed {
            // Resize the framebuffer if the window dimensions changed
            let new_w = win.width as u32;
            let new_h = win.height as u32;
            if new_w != win.framebuffer.width() || new_h != win.framebuffer.height() {
                win.framebuffer = Framebuffer::new(new_w, new_h);
            }

            if let Some(ref uuid) = wid_str {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowConfigured {
                    window_id: uuid.clone(),
                    x: win.x,
                    y: win.y,
                    width: win.width,
                    height: win.height,
                },
            ));
            }

            // Send ConfigureNotify
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes()); // event
            event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
                                                              // above_sibling = 0
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&win.width.to_le_bytes());
            event[22..24].copy_from_slice(&win.height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            event[26] = if win.override_redirect { 1 } else { 0 };
            return event.to_vec();
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 14: GetGeometry
// ---------------------------------------------------------------------------

fn handle_get_geometry(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Check windows first, then pixmaps
    if let Some(win) = state.windows.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = 24; // depth
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[12..14].copy_from_slice(&win.x.to_le_bytes());
        reply[14..16].copy_from_slice(&win.y.to_le_bytes());
        reply[16..18].copy_from_slice(&win.width.to_le_bytes());
        reply[18..20].copy_from_slice(&win.height.to_le_bytes());
        reply[20..22].copy_from_slice(&win.border_width.to_le_bytes());
        return reply.to_vec();
    }

    if let Some(pixmap) = state.pixmaps.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = 24; // depth
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[16..18].copy_from_slice(&pixmap.width.to_le_bytes());
        reply[18..20].copy_from_slice(&pixmap.height.to_le_bytes());
        return reply.to_vec();
    }

    // Drawable not found - return BadDrawable error (error code 9)
    build_error(9, seq, drawable, 14, 0)
}

// ---------------------------------------------------------------------------
// Opcode 15: QueryTree
// ---------------------------------------------------------------------------

fn handle_query_tree(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    if !state.windows.contains_key(&wid) {
        return build_error(3, seq, wid, 15, 0); // BadWindow
    }

    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == wid)
        .map(|w| w.id)
        .collect();

    let n_children = children.len() as u16;
    let reply_len = 32 + children.len() * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(children.len() as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    reply[12..16].copy_from_slice(&parent.to_le_bytes());
    reply[16..18].copy_from_slice(&n_children.to_le_bytes());

    for (i, &child) in children.iter().enumerate() {
        let off = 32 + i * 4;
        reply[off..off + 4].copy_from_slice(&child.to_le_bytes());
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 16: InternAtom
// ---------------------------------------------------------------------------

fn handle_intern_atom(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let only_if_exists = data[1] != 0;
    let name_len = u16::from_le_bytes([data[4], data[5]]) as usize;

    let name = if 8 + name_len <= data.len() {
        String::from_utf8_lossy(&data[8..8 + name_len]).to_string()
    } else {
        String::new()
    };

    let atom = state.intern_atom(&name, only_if_exists);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&atom.to_le_bytes());

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 17: GetAtomName
// ---------------------------------------------------------------------------

fn handle_get_atom_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let atom = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // BadAtom (error code 5) for unknown atoms
    let Some(name) = state.get_atom_name(atom) else {
        return build_error(5, seq, atom, 17, 0);
    };
    let name_bytes = name.as_bytes();
    let padded_len = (name_bytes.len() + 3) & !3;

    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded_len / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    reply[32..32 + name_bytes.len()].copy_from_slice(name_bytes);

    reply
}

// ---------------------------------------------------------------------------
// Opcode 18: ChangeProperty
// ---------------------------------------------------------------------------

fn handle_change_property(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let _mode = data[1]; // 0=Replace, 1=Prepend, 2=Append
    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let property_atom = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let prop_type = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let format = data[16];
    let data_len = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    // Calculate actual byte length based on format
    let byte_len = match format {
        8 => data_len,
        16 => data_len * 2,
        32 => data_len * 4,
        _ => data_len,
    };

    // Store the property value
    if data.len() >= 24 + byte_len {
        let prop_data = data[24..24 + byte_len].to_vec();
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(property_atom, PropertyValue {
                prop_type,
                format,
                data: prop_data,
            });
        }
    }

    // Check if this is WM_NAME (atom 39) or _NET_WM_NAME
    let is_wm_name = property_atom == 39
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "_NET_WM_NAME" || n == "WM_NAME")
            .unwrap_or(false);

    if is_wm_name && format == 8 && data.len() >= 24 + byte_len {
        let title = String::from_utf8_lossy(&data[24..24 + byte_len]).to_string();
        if !title.is_empty() {
            if let Some(uuid) = state.window_uuid(window) {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::TitleChanged {
                    window_id: uuid,
                    title,
                },
            ));
            }
        }
    }

    // Detect GTK application menu export.
    if format == 8 && data.len() >= 24 + byte_len {
        let atom_name = state.get_atom_name(property_atom);
        if let Some(name) = atom_name {
            let is_gtk_menu_atom = matches!(
                name.as_str(),
                "_GTK_UNIQUE_BUS_NAME"
                    | "_GTK_MENUBAR_OBJECT_PATH"
                    | "_GTK_APP_MENU_OBJECT_PATH"
                    | "_GTK_APPLICATION_OBJECT_PATH"
                    | "_GTK_WINDOW_OBJECT_PATH"
            );
            if is_gtk_menu_atom {
                let value = String::from_utf8_lossy(&data[24..24 + byte_len])
                    .trim_end_matches('\0')
                    .to_string();
                let entry = state
                    .gtk_menu_paths
                    .entry(window)
                    .or_default();
                match name.as_str() {
                    "_GTK_UNIQUE_BUS_NAME" => entry.bus_name = value,
                    "_GTK_MENUBAR_OBJECT_PATH" => entry.menubar_path = Some(value),
                    "_GTK_APP_MENU_OBJECT_PATH" => entry.app_menu_path = Some(value),
                    "_GTK_APPLICATION_OBJECT_PATH" => {
                        entry.app_actions_path = Some(value)
                    }
                    "_GTK_WINDOW_OBJECT_PATH" => {
                        entry.win_actions_path = Some(value)
                    }
                    _ => {}
                }
                if let Some(paths) = state.gtk_menu_paths.get(&window) {
                    if paths.has_menu() {
                        if let Some(uuid) = state.window_uuid(window) {
                            state.menu_tracker.attach_gtk(
                                uuid,
                                state.client_id.clone(),
                                paths.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 19: DeleteProperty
// ---------------------------------------------------------------------------

fn handle_delete_property(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 12 {
        let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let property = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.remove(&property);
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 20: GetProperty
// ---------------------------------------------------------------------------

fn handle_get_property(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        return reply.to_vec();
    }

    let delete = data[1] != 0;
    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let property_atom = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _req_type = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let long_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let long_length = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    let prop = state.windows.get(&window).and_then(|w| w.properties.get(&property_atom)).cloned();

    if let Some(prop_val) = prop {
        let byte_offset = long_offset * 4;
        let max_bytes = long_length * 4;
        let total_bytes = prop_val.data.len();
        let available = if byte_offset >= total_bytes { 0 } else { total_bytes - byte_offset };
        let return_bytes = available.min(max_bytes);
        let bytes_after = if available > return_bytes { available - return_bytes } else { 0 };

        let return_data = if byte_offset < total_bytes {
            &prop_val.data[byte_offset..byte_offset + return_bytes]
        } else {
            &[]
        };

        // value_length is in units of format size
        let value_length = match prop_val.format {
            8 => return_data.len() as u32,
            16 => (return_data.len() / 2) as u32,
            32 => (return_data.len() / 4) as u32,
            _ => return_data.len() as u32,
        };

        let padded_len = (return_data.len() + 3) & !3;
        let extra_words = padded_len / 4;
        let total_reply = 32 + padded_len;

        let mut reply = vec![0u8; total_reply];
        reply[0] = 1; // Reply
        reply[1] = prop_val.format;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes()); // length
        reply[8..12].copy_from_slice(&prop_val.prop_type.to_le_bytes()); // type
        reply[12..16].copy_from_slice(&(bytes_after as u32).to_le_bytes()); // bytes_after
        reply[16..20].copy_from_slice(&value_length.to_le_bytes()); // value_length
        reply[32..32 + return_data.len()].copy_from_slice(return_data);

        // Delete property if requested and we returned all of it
        if delete && bytes_after == 0 {
            if let Some(win) = state.windows.get_mut(&window) {
                win.properties.remove(&property_atom);
            }
        }

        reply
    } else {
        // Property not found
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        // type = 0 (None), format = 0, bytes_after = 0, value_length = 0
        reply.to_vec()
    }
}

// ---------------------------------------------------------------------------
// Opcode 21: ListProperties
// ---------------------------------------------------------------------------

fn handle_list_properties(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        return reply.to_vec();
    }
    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let atoms: Vec<u32> = state
        .windows
        .get(&window)
        .map(|w| w.properties.keys().copied().collect())
        .unwrap_or_default();

    let n = atoms.len();
    let extra_bytes = n * 4;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(n as u32).to_le_bytes()); // length in 4-byte units
    reply[8..10].copy_from_slice(&(n as u16).to_le_bytes()); // num_atoms
    for (i, atom) in atoms.iter().enumerate() {
        reply[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&atom.to_le_bytes());
    }
    reply
}

// ---------------------------------------------------------------------------
// Opcode 22: SetSelectionOwner
// ---------------------------------------------------------------------------

fn handle_set_selection_owner(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 12 {
        let owner = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let selection = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if owner == 0 {
            state.selections.remove(&selection);
        } else {
            state.selections.insert(selection, owner);
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 23: GetSelectionOwner
// ---------------------------------------------------------------------------

fn handle_get_selection_owner(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    if data.len() >= 8 {
        let selection = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let owner = state.selections.get(&selection).copied().unwrap_or(0);
        reply[8..12].copy_from_slice(&owner.to_le_bytes());
    }
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 24: ConvertSelection
// ---------------------------------------------------------------------------

fn handle_convert_selection(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() >= 24 {
        let requestor = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let selection = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let target = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        // Send SelectionNotify event back with property=None to indicate no data
        let mut event = [0u8; 32];
        event[0] = 31; // SelectionNotify
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&0u32.to_le_bytes()); // timestamp
        event[8..12].copy_from_slice(&requestor.to_le_bytes()); // requestor
        event[12..16].copy_from_slice(&selection.to_le_bytes()); // selection
        event[16..20].copy_from_slice(&target.to_le_bytes()); // target
        event[20..24].copy_from_slice(&0u32.to_le_bytes()); // property = None
        return event.to_vec();
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 25: SendEvent
// ---------------------------------------------------------------------------

fn handle_send_event(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 44 {
        return Vec::new();
    }

    let _propagate = data[1] != 0;
    let destination = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _event_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    // The event data is 32 bytes starting at offset 12
    let mut event = data[12..44].to_vec();
    // Mark as synthetic (bit 7 of the event code)
    event[0] |= 0x80;

    // Resolve destination:
    // 0 = PointerWindow (use window under pointer, which we approximate as focus)
    // 1 = InputFocus (use focus window)
    let target = match destination {
        0 | 1 => state.focus_window,
        w => w,
    };

    // Deliver to the target window by queueing as a pending event
    if state.windows.contains_key(&target) || target == state.root_window {
        state.pending_events.push(event);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 38: QueryPointer
// ---------------------------------------------------------------------------

fn handle_query_pointer(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // same_screen
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
                                                                    // child = 0
    reply[16..18].copy_from_slice(&state.pointer_x.to_le_bytes()); // root_x
    reply[18..20].copy_from_slice(&state.pointer_y.to_le_bytes()); // root_y
    reply[20..22].copy_from_slice(&state.pointer_x.to_le_bytes()); // win_x
    reply[22..24].copy_from_slice(&state.pointer_y.to_le_bytes()); // win_y
                                                                   // mask = 0
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 39: GetMotionEvents
// ---------------------------------------------------------------------------

fn handle_get_motion_events(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return empty motion buffer (we don't store motion history)
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0, n_events = 0 (already zero)
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 40: TranslateCoordinates
// ---------------------------------------------------------------------------

fn handle_translate_coordinates(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 1; // same_screen
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        return reply.to_vec();
    }

    let src_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst_window = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let src_x = i16::from_le_bytes([data[12], data[13]]);
    let src_y = i16::from_le_bytes([data[14], data[15]]);

    // Convert src_x, src_y from src_window coordinate space to root, then to dst_window.
    // Walk up from src to root accumulating offsets.
    let mut sx = src_x as i32;
    let mut sy = src_y as i32;
    {
        let mut cur = src_window;
        for _ in 0..32 {
            if cur == state.root_window || cur == 0 {
                break;
            }
            if let Some(w) = state.windows.get(&cur) {
                sx += w.x as i32;
                sy += w.y as i32;
                cur = w.parent;
            } else {
                break;
            }
        }
    }
    // Now (sx, sy) is in root coordinates. Walk up from dst to root to find dst offset.
    let mut dx = 0i32;
    let mut dy = 0i32;
    {
        let mut cur = dst_window;
        for _ in 0..32 {
            if cur == state.root_window || cur == 0 {
                break;
            }
            if let Some(w) = state.windows.get(&cur) {
                dx += w.x as i32;
                dy += w.y as i32;
                cur = w.parent;
            } else {
                break;
            }
        }
    }
    let dst_x = (sx - dx) as i16;
    let dst_y = (sy - dy) as i16;

    // Find child of dst_window that contains the point
    let child = state
        .windows
        .values()
        .find(|w| {
            w.parent == dst_window
                && dst_x >= w.x
                && dst_x < w.x + w.width as i16
                && dst_y >= w.y
                && dst_y < w.y + w.height as i16
        })
        .map(|w| w.id)
        .unwrap_or(0);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // same_screen
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&child.to_le_bytes());
    reply[12..14].copy_from_slice(&dst_x.to_le_bytes());
    reply[14..16].copy_from_slice(&dst_y.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 41: WarpPointer
// ---------------------------------------------------------------------------

fn handle_warp_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let _src_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst_window = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _src_x = i16::from_le_bytes([data[12], data[13]]);
    let _src_y = i16::from_le_bytes([data[14], data[15]]);
    let _src_width = u16::from_le_bytes([data[16], data[17]]);
    let _src_height = u16::from_le_bytes([data[18], data[19]]);
    let dst_x = i16::from_le_bytes([data[20], data[21]]);
    let dst_y = i16::from_le_bytes([data[22], data[23]]);

    if dst_window == 0 {
        // Relative warp: offset from current position
        state.pointer_x = state.pointer_x.saturating_add(dst_x);
        state.pointer_y = state.pointer_y.saturating_add(dst_y);
    } else {
        // Absolute warp: position relative to dst_window, converted to root coords
        let mut abs_x = dst_x as i32;
        let mut abs_y = dst_y as i32;
        let mut cur = dst_window;
        for _ in 0..32 {
            if cur == state.root_window || cur == 0 {
                break;
            }
            if let Some(w) = state.windows.get(&cur) {
                abs_x += w.x as i32;
                abs_y += w.y as i32;
                cur = w.parent;
            } else {
                break;
            }
        }
        state.pointer_x = abs_x.clamp(0, SCREEN_WIDTH as i32 - 1) as i16;
        state.pointer_y = abs_y.clamp(0, SCREEN_HEIGHT as i32 - 1) as i16;
    }

    // Send MotionNotify event to let the client know the pointer moved
    let mut event = [0u8; 32];
    event[0] = MOTION_NOTIFY_EVENT;
    event[1] = 0; // detail = Normal
    event[2..4].copy_from_slice(&seq.to_le_bytes());
    // timestamp = 0
    event[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
    // event window = focus_window
    event[12..16].copy_from_slice(&state.focus_window.to_le_bytes());
    event[20..22].copy_from_slice(&state.pointer_x.to_le_bytes()); // root_x
    event[22..24].copy_from_slice(&state.pointer_y.to_le_bytes()); // root_y
    event[24..26].copy_from_slice(&state.pointer_x.to_le_bytes()); // event_x
    event[26..28].copy_from_slice(&state.pointer_y.to_le_bytes()); // event_y
    event[30] = 1; // same_screen = true
    state.pending_events.push(event.to_vec());

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 42: SetInputFocus
// ---------------------------------------------------------------------------

fn handle_set_input_focus(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let focus = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        state.set_focus_window(focus);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 43: GetInputFocus
// ---------------------------------------------------------------------------

fn handle_get_input_focus(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // revert_to = Parent
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.focus_window.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 44: QueryKeymap
// ---------------------------------------------------------------------------

fn handle_query_keymap(state: &ClientState, seq: u16) -> Vec<u8> {
    // Return actual pressed keys state
    let mut reply = [0u8; 40]; // 32 + 8 bytes of keymap
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&2u32.to_le_bytes()); // length = 2 (8 extra bytes)
    // Copy the pressed_keys bitmap into the reply
    reply[32..40].copy_from_slice(&state.pressed_keys[0..8]);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 45: OpenFont
// ---------------------------------------------------------------------------

fn handle_open_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let fid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        String::from_utf8_lossy(&data[12..12 + name_len]).to_string()
    } else {
        "fixed".to_string()
    };
    debug!("OpenFont: fid={fid:#x} name={name}");
    state.font_manager.open_font(fid, &name);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 46: CloseFont
// ---------------------------------------------------------------------------

fn handle_close_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let fid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.font_manager.close_font(fid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 47: QueryFont
// ---------------------------------------------------------------------------

fn handle_query_font(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let fontable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // fontable can be a font ID or a GC ID (containing a font)
    let font = state
        .font_manager
        .get_font(fontable)
        .or_else(|| {
            let gc = state.gcs.get(&fontable)?;
            state.font_manager.get_font(gc.font_id)
        })
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => {
            // No font available -- return minimal stub
            let mut reply = vec![0u8; 60];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&7u32.to_le_bytes());
            reply[40..42].copy_from_slice(&32u16.to_le_bytes());
            reply[42..44].copy_from_slice(&126u16.to_le_bytes());
            reply[44..46].copy_from_slice(&32u16.to_le_bytes());
            reply[48] = 0;
            reply[52..54].copy_from_slice(&10i16.to_le_bytes());
            reply[54..56].copy_from_slice(&3i16.to_le_bytes());
            return reply;
        }
    };

    let n_char_infos = (font.max_char - font.min_char + 1) as u32;
    let char_infos_bytes = n_char_infos as usize * 12;

    let reply_len = 60 + char_infos_bytes;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    let extra_words = ((reply_len - 32) / 4) as u32;
    reply[4..8].copy_from_slice(&extra_words.to_le_bytes());

    // min_bounds at offset 8 (12 bytes)
    {
        let ci = &font.min_bounds;
        reply[8..10].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
        reply[10..12].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
        reply[12..14].copy_from_slice(&ci.character_width.to_le_bytes());
        reply[14..16].copy_from_slice(&ci.ascent.to_le_bytes());
        reply[16..18].copy_from_slice(&ci.descent.to_le_bytes());
        reply[18..20].copy_from_slice(&ci.attributes.to_le_bytes());
    }
    // pad at 20..24

    // max_bounds at offset 24 (12 bytes)
    {
        let ci = &font.max_bounds;
        reply[24..26].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
        reply[26..28].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
        reply[28..30].copy_from_slice(&ci.character_width.to_le_bytes());
        reply[30..32].copy_from_slice(&ci.ascent.to_le_bytes());
        reply[32..34].copy_from_slice(&ci.descent.to_le_bytes());
        reply[34..36].copy_from_slice(&ci.attributes.to_le_bytes());
    }
    // pad at 36..40

    reply[40..42].copy_from_slice(&font.min_char.to_le_bytes());
    reply[42..44].copy_from_slice(&font.max_char.to_le_bytes());
    reply[44..46].copy_from_slice(&font.default_char.to_le_bytes());
    reply[46..48].copy_from_slice(&0u16.to_le_bytes()); // n_properties = 0
    reply[48] = 0; // draw_direction = LeftToRight
    reply[49] = 0; // min_byte1
    reply[50] = 0; // max_byte1
    reply[51] = if font.char_infos.len() == n_char_infos as usize {
        1
    } else {
        0
    }; // all_chars_exist
    reply[52..54].copy_from_slice(&font.font_ascent.to_le_bytes());
    reply[54..56].copy_from_slice(&font.font_descent.to_le_bytes());
    reply[56..60].copy_from_slice(&n_char_infos.to_le_bytes());

    // Char infos at offset 60
    let mut off = 60;
    for ci in &font.char_infos {
        if off + 12 <= reply.len() {
            reply[off..off + 2].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
            reply[off + 2..off + 4].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
            reply[off + 4..off + 6].copy_from_slice(&ci.character_width.to_le_bytes());
            reply[off + 6..off + 8].copy_from_slice(&ci.ascent.to_le_bytes());
            reply[off + 8..off + 10].copy_from_slice(&ci.descent.to_le_bytes());
            reply[off + 10..off + 12].copy_from_slice(&ci.attributes.to_le_bytes());
            off += 12;
        }
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 48: QueryTextExtents
// ---------------------------------------------------------------------------

fn handle_query_text_extents(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        return reply.to_vec();
    }

    let fontable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Try to get actual font metrics
    let font = state
        .font_manager
        .get_font(fontable)
        .or_else(|| {
            let gc = state.gcs.get(&fontable)?;
            state.font_manager.get_font(gc.font_id)
        })
        .or_else(|| state.font_manager.get_default_font());

    let (ascent, descent, overall_width) = if let Some(font) = &font {
        // Calculate width from the text in the request
        // Text starts at offset 8, each char is 2 bytes (CHAR2B format)
        let odd_length = data[1] != 0;
        let text_bytes = data.len() - 8;
        let char_count = if odd_length {
            (text_bytes - 2) / 2
        } else {
            text_bytes / 2
        };
        let mut width: i32 = 0;
        for i in 0..char_count {
            let _byte1 = data[8 + i * 2];
            let byte2 = data[8 + i * 2 + 1];
            width += font.char_info(byte2 as u16).character_width as i32;
        }
        (font.font_ascent, font.font_descent, width as i16)
    } else {
        (12i16, 4i16, 0i16)
    };

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&ascent.to_le_bytes()); // font_ascent
    reply[10..12].copy_from_slice(&descent.to_le_bytes()); // font_descent
    reply[12..14].copy_from_slice(&ascent.to_le_bytes()); // overall_ascent
    reply[14..16].copy_from_slice(&descent.to_le_bytes()); // overall_descent
    reply[16..20].copy_from_slice(&(overall_width as i32).to_le_bytes()); // overall_width
    // overall_left = 0, overall_right = overall_width
    reply[24..28].copy_from_slice(&(overall_width as i32).to_le_bytes()); // overall_right
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 49: ListFonts
// ---------------------------------------------------------------------------

fn handle_list_fonts(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return a single font: "fixed"
    let font_name = b"fixed";
    let str_len = 1 + font_name.len(); // length byte + name
    let padded = (str_len + 3) & !3;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // number of names
    reply[32] = font_name.len() as u8;
    reply[33..33 + font_name.len()].copy_from_slice(font_name);

    reply
}

// ---------------------------------------------------------------------------
// Opcode 50: ListFontsWithInfo
// ---------------------------------------------------------------------------

fn handle_list_fonts_with_info(seq: u16) -> Vec<u8> {
    // Terminate with empty reply (name_length=0)
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[1] = 0; // last-reply indicator (name_length = 0)
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&7u32.to_le_bytes());
    let mut full_reply = reply.to_vec();
    full_reply.resize(32 + 28, 0); // 28 bytes of padding for the min/max bounds
    full_reply
}

// ---------------------------------------------------------------------------
// Opcode 52: GetFontPath
// ---------------------------------------------------------------------------

fn handle_get_font_path(seq: u16) -> Vec<u8> {
    // Empty list
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 53: CreatePixmap
// ---------------------------------------------------------------------------

fn handle_create_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let depth = data[1];
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    info!("CreatePixmap: pid={pid:#x} {}x{} depth={depth}", width, height);

    state.pixmaps.insert(
        pid,
        PixmapState {
            width,
            height,
            depth,
            framebuffer: Framebuffer::new(width as u32, height as u32),
            alias_window: None,
            shm_backing: None,
        },
    );

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 54: FreePixmap
// ---------------------------------------------------------------------------

fn handle_free_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.pixmaps.remove(&pid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 55: CreateGC
// ---------------------------------------------------------------------------

fn handle_create_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let mut gc = GcState::default();
    parse_gc_values(&mut gc, value_mask, &data[16..]);
    state.gcs.insert(gc_id, gc);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 56: ChangeGC
// ---------------------------------------------------------------------------

fn handle_change_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        parse_gc_values(gc, value_mask, &data[12..]);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 57: CopyGC
// ---------------------------------------------------------------------------

fn handle_copy_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let src_gc = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst_gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let value_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let src = match state.gcs.get(&src_gc) {
        Some(g) => g.clone(),
        None => return Vec::new(),
    };

    if let Some(dst) = state.gcs.get_mut(&dst_gc) {
        if value_mask & (1 << 0) != 0 { dst.function = src.function; }
        if value_mask & (1 << 1) != 0 { dst.plane_mask = src.plane_mask; }
        if value_mask & (1 << 2) != 0 { dst.foreground = src.foreground; }
        if value_mask & (1 << 3) != 0 { dst.background = src.background; }
        if value_mask & (1 << 4) != 0 { dst.line_width = src.line_width; }
        if value_mask & (1 << 5) != 0 { dst.line_style = src.line_style; }
        if value_mask & (1 << 6) != 0 { dst.cap_style = src.cap_style; }
        if value_mask & (1 << 7) != 0 { dst.join_style = src.join_style; }
        if value_mask & (1 << 8) != 0 { dst.fill_style = src.fill_style; }
        if value_mask & (1 << 9) != 0 { dst.fill_rule = src.fill_rule; }
        if value_mask & (1 << 10) != 0 { dst.tile = src.tile; }
        if value_mask & (1 << 11) != 0 { dst.stipple = src.stipple; }
        if value_mask & (1 << 12) != 0 { dst.ts_x = src.ts_x; }
        if value_mask & (1 << 13) != 0 { dst.ts_y = src.ts_y; }
        if value_mask & (1 << 14) != 0 { dst.font_id = src.font_id; }
        if value_mask & (1 << 15) != 0 { dst.subwindow_mode = src.subwindow_mode; }
        if value_mask & (1 << 16) != 0 { dst.graphics_exposures = src.graphics_exposures; }
        if value_mask & (1 << 17) != 0 { dst.clip_x = src.clip_x; }
        if value_mask & (1 << 18) != 0 { dst.clip_y = src.clip_y; }
        if value_mask & (1 << 19) != 0 { dst.clip_mask = src.clip_mask; }
        if value_mask & (1 << 20) != 0 { dst.dash_offset = src.dash_offset; }
        if value_mask & (1 << 21) != 0 { dst.dashes = src.dashes; }
        if value_mask & (1 << 22) != 0 { dst.arc_mode = src.arc_mode; }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 58: SetDashes
// ---------------------------------------------------------------------------

fn handle_set_dashes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dash_offset = u16::from_le_bytes([data[8], data[9]]);
    let n_dashes = u16::from_le_bytes([data[10], data[11]]) as usize;

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        gc.dash_offset = dash_offset;
        if 12 + n_dashes <= data.len() {
            gc.dash_list = data[12..12 + n_dashes].to_vec();
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 59: SetClipRectangles
// ---------------------------------------------------------------------------

fn handle_set_clip_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let _ordering = data[1];
    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let clip_x = i16::from_le_bytes([data[8], data[9]]);
    let clip_y = i16::from_le_bytes([data[10], data[11]]);

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        gc.clip_x = clip_x;
        gc.clip_y = clip_y;
        gc.clip_rects.clear();

        let mut offset = 12;
        while offset + 8 <= data.len() {
            let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
            let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let w = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
            let h = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
            gc.clip_rects.push((x, y, w, h));
            offset += 8;
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 60: FreeGC
// ---------------------------------------------------------------------------

fn handle_free_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.gcs.remove(&gc_id);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 61: ClearArea
// ---------------------------------------------------------------------------

fn handle_clear_area(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let x = i16::from_le_bytes([data[8], data[9]]);
    let y = i16::from_le_bytes([data[10], data[11]]);
    let mut width = u16::from_le_bytes([data[12], data[13]]);
    let mut height = u16::from_le_bytes([data[14], data[15]]);

    let bg = state.windows.get(&wid).map(|w| {
        if width == 0 {
            width = w.width;
        }
        if height == 0 {
            height = w.height;
        }
        w.background_pixel
    });

    let bg_pixel = bg.unwrap_or(0);
    if let Some(fb) = state.get_framebuffer_mut(wid) {
        fb.fill_rect(x, y, width, height, bg_pixel);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 62: CopyArea
// ---------------------------------------------------------------------------

fn handle_copy_area(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 28 {
        return Vec::new();
    }

    let src = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let src_x = i16::from_le_bytes([data[16], data[17]]);
    let src_y = i16::from_le_bytes([data[18], data[19]]);
    let dst_x = i16::from_le_bytes([data[20], data[21]]);
    let dst_y = i16::from_le_bytes([data[22], data[23]]);
    let width = u16::from_le_bytes([data[24], data[25]]);
    let height = u16::from_le_bytes([data[26], data[27]]);

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Sync SHM-backed pixmap data before reading from src
    state.sync_shm_pixmap(src);

    // Check if source is a 1-bit depth pixmap (used for clip masks)
    let src_depth = state.pixmaps.get(&src).map(|p| p.depth).unwrap_or(24);

    if src == dst {
        if let Some(fb) = state.get_framebuffer_mut(src) {
            fb.copy_area_self(src_x, src_y, dst_x, dst_y, width, height);
        }
    } else {
        let pixels = state
            .get_framebuffer_mut(src)
            .map(|fb| fb.extract_pixels(src_x, src_y, width, height));
        if let Some(pixels) = pixels {
            if src_depth <= 1 && gc.function != 3 {
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    let fb_w = fb.width() as i32;
                    let fb_h = fb.height() as i32;
                    let src_stride = width as usize * 4;
                    for row in 0..height as usize {
                        let dy = dst_y as i32 + row as i32;
                        if dy < 0 || dy >= fb_h { continue; }
                        for col in 0..width as usize {
                            let dx = dst_x as i32 + col as i32;
                            if dx < 0 || dx >= fb_w { continue; }
                            let src_off = row * src_stride + col * 4;
                            if src_off + 3 >= pixels.len() { continue; }
                            let src_pixel = pixels[src_off] as u32
                                | (pixels[src_off + 1] as u32) << 8
                                | (pixels[src_off + 2] as u32) << 16;
                            let color = if src_pixel != 0 {
                                gc.foreground
                            } else {
                                gc.background
                            };
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else if gc.function != 3 {
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    let src_stride = width as usize * 4;
                    for row in 0..height as usize {
                        let dy = dst_y as i32 + row as i32;
                        for col in 0..width as usize {
                            let dx = dst_x as i32 + col as i32;
                            let src_off = row * src_stride + col * 4;
                            if src_off + 3 >= pixels.len() { continue; }
                            let color = (pixels[src_off + 2] as u32) << 16
                                | (pixels[src_off + 1] as u32) << 8
                                | pixels[src_off] as u32;
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else {
                // GXcopy -- fast path
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    fb.put_image(dst_x, dst_y, width, height, &pixels);
                }
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 63: CopyPlane
// ---------------------------------------------------------------------------

fn handle_copy_plane(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }

    let src = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let src_x = i16::from_le_bytes([data[16], data[17]]);
    let src_y = i16::from_le_bytes([data[18], data[19]]);
    let dst_x = i16::from_le_bytes([data[20], data[21]]);
    let dst_y = i16::from_le_bytes([data[22], data[23]]);
    let width = u16::from_le_bytes([data[24], data[25]]);
    let height = u16::from_le_bytes([data[26], data[27]]);
    let bit_plane = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Extract source pixels
    state.sync_shm_pixmap(src);
    let pixels = state
        .get_framebuffer_mut(src)
        .map(|fb| fb.extract_pixels(src_x, src_y, width, height));

    if let Some(pixels) = pixels {
        if let Some(fb) = state.get_framebuffer_mut(dst) {
            let src_stride = width as usize * 4;
            for row in 0..height as usize {
                for col in 0..width as usize {
                    let src_off = row * src_stride + col * 4;
                    if src_off + 3 >= pixels.len() { continue; }
                    let src_pixel = pixels[src_off] as u32
                        | (pixels[src_off + 1] as u32) << 8
                        | (pixels[src_off + 2] as u32) << 16
                        | (pixels[src_off + 3] as u32) << 24;
                    let color = if (src_pixel & bit_plane) != 0 {
                        gc.foreground
                    } else {
                        gc.background
                    };
                    let dx = dst_x as i32 + col as i32;
                    let dy = dst_y as i32 + row as i32;
                    fb.draw_point_with_func(dx, dy, color, gc.function);
                }
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 64: PolyPoint
// ---------------------------------------------------------------------------

fn handle_poly_point(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let coord_mode = data[1];
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points = Vec::new();
    let mut last_x: i16 = 0;
    let mut last_y: i16 = 0;
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let mut x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let mut y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 {
            x += last_x;
            y += last_y;
        }
        last_x = x;
        last_y = y;
        points.push((x, y));
        offset += 4;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y) in points {
            fb.draw_point(x as i32, y as i32, gc.foreground);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 65: PolyLine
// ---------------------------------------------------------------------------

fn handle_poly_line(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let coord_mode = data[1];
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points: Vec<(i16, i16)> = Vec::new();
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for w in points.windows(2) {
            fb.draw_line(
                w[0].0 as i32, w[0].1 as i32,
                w[1].0 as i32, w[1].1 as i32,
                gc.foreground, gc.line_width,
            );
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 66: PolySegment
// ---------------------------------------------------------------------------

fn handle_poly_segment(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut segments = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x1 = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y1 = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let x2 = i16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let y2 = i16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        segments.push((x1, y1, x2, y2));
        offset += 8;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x1, y1, x2, y2) in segments {
            fb.draw_line(x1 as i32, y1 as i32, x2 as i32, y2 as i32, gc.foreground, gc.line_width);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 67: PolyRectangle
// ---------------------------------------------------------------------------

fn handle_poly_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        rects.push((x, y, width, height));
        offset += 8;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height) in rects {
            let x2 = x as i32 + width as i32;
            let y2 = y as i32 + height as i32;
            fb.draw_line(x as i32, y as i32, x2, y as i32, gc.foreground, gc.line_width);
            fb.draw_line(x2, y as i32, x2, y2, gc.foreground, gc.line_width);
            fb.draw_line(x2, y2, x as i32, y2, gc.foreground, gc.line_width);
            fb.draw_line(x as i32, y2, x as i32, y as i32, gc.foreground, gc.line_width);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 68: PolyArc
// ---------------------------------------------------------------------------

fn handle_poly_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height, angle1, angle2) in arcs {
            fb.draw_arc(x, y, width, height, angle1, angle2, false, gc.foreground);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 69: FillPoly
// ---------------------------------------------------------------------------

fn handle_fill_poly(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();
    let coord_mode = data[13]; // 0 = Origin, 1 = Previous

    let mut points = Vec::new();
    let mut offset = 16;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py): (i16, i16) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    if points.len() >= 3 {
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            fb.fill_polygon(&points, gc.foreground);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 70: PolyFillRectangle
// ---------------------------------------------------------------------------

fn handle_poly_fill_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        rects.push((x, y, width, height));
        offset += 8;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    info!("PolyFillRect: draw={drawable:#x} fg={fg:#x} gc={gc_id:#x} rects={} fn={}", rects.len(), gc.function);
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, width, height) in &rects {
            if gc.function == 10 {
                fb.invert_rect(x, y, width, height);
            } else {
                fb.fill_rect(x, y, width, height, fg);
            }
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height) in &rects {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 71: PolyFillArc
// ---------------------------------------------------------------------------

fn handle_poly_fill_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    info!("PolyFillArc: gc={gc_id:#x} func={} fg_raw={:#x} fg_mapped={fg:#x} draw={drawable:#x}", gc.function, gc.foreground);
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height, angle1, angle2) in &arcs {
            fb.draw_arc(*x, *y, *width, *height, *angle1, *angle2, true, fg);
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height, _, _) in &arcs {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 72: PutImage
// ---------------------------------------------------------------------------

fn handle_put_image(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let format = data[1]; // 0=Bitmap, 1=XYPixmap, 2=ZPixmap
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);
    let dst_x = i16::from_le_bytes([data[16], data[17]]);
    let dst_y = i16::from_le_bytes([data[18], data[19]]);
    let _left_pad = data[20];
    let depth = data[21];

    let pixel_data = &data[24..];

    debug!("PutImage: fmt={format} depth={depth} drawable={drawable:#x} {width}x{height} at ({dst_x},{dst_y}) data={}", pixel_data.len());

    if format == 2 && depth >= 24 {
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            fb.put_image(dst_x, dst_y, width, height, pixel_data);
        }
    } else if format == 2 && depth == 1 {
        // 1-bit depth ZPixmap: used for cursor bitmaps, skip
    } else {
        debug!(
            "PutImage: unsupported format={format} depth={depth} {}x{} data_len={}",
            width,
            height,
            pixel_data.len()
        );
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 73: GetImage
// ---------------------------------------------------------------------------

fn handle_get_image(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }

    let _format = data[1]; // 1=XYPixmap, 2=ZPixmap
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let x = i16::from_le_bytes([data[8], data[9]]);
    let y = i16::from_le_bytes([data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    // Sync SHM pixmaps before reading
    state.sync_shm_pixmap(drawable);

    let depth: u8 = state
        .pixmaps
        .get(&drawable)
        .map(|p| p.depth)
        .unwrap_or(24);

    // Read actual pixel data from the drawable's framebuffer
    let pixels = if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.extract_pixels(x, y, width, height)
    } else {
        vec![0u8; width as usize * height as usize * 4]
    };

    let row_bytes = width as usize * 4;
    let padded_row = (row_bytes + 3) & !3;
    let data_len = padded_row * height as usize;
    let length_field = (data_len / 4) as u32;

    let mut reply = vec![0u8; 32 + data_len];
    reply[0] = 1; // Reply
    reply[1] = depth;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..12].copy_from_slice(&ROOT_VISUAL.to_le_bytes());

    // Copy pixel data into reply (row by row with padding)
    for row in 0..height as usize {
        let src_off = row * row_bytes;
        let dst_off = 32 + row * padded_row;
        let copy_len = row_bytes.min(pixels.len() - src_off);
        if src_off + copy_len <= pixels.len() && dst_off + copy_len <= reply.len() {
            reply[dst_off..dst_off + copy_len].copy_from_slice(&pixels[src_off..src_off + copy_len]);
        }
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 74: PolyText8
// ---------------------------------------------------------------------------

fn handle_poly_text8(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let mut cursor_x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    // Collect text items first to avoid borrow issues
    let mut items: Vec<(i16, i16, u16, u16, Vec<u8>)> = Vec::new();
    let mut offset = 16;
    let end = data.len();

    while offset < end {
        let item_len = data[offset] as usize;

        if item_len == 255 {
            offset += 5;
            continue;
        }
        if item_len == 0 {
            break;
        }
        if offset + 2 + item_len > end {
            break;
        }

        let delta = data[offset + 1] as i8;
        cursor_x += delta as i16;

        let text = &data[offset + 2..offset + 2 + item_len];
        let (img_w, img_h, pixels) = font.render_text_transparent(text, gc.foreground);

        if img_w > 0 && img_h > 0 {
            items.push((cursor_x, y - font.font_ascent, img_w, img_h, pixels));
        }

        let mut text_advance: i32 = 0;
        for &ch in text {
            text_advance += font.char_info(ch as u16).character_width as i32;
        }
        cursor_x += text_advance as i16;
        offset += 2 + item_len;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, w, h, pixels) in items {
            // Use Over compositing to preserve background under transparent pixels
            fb.put_image_over(x, y, w, h, &pixels);
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 75: PolyText16
// ---------------------------------------------------------------------------

fn handle_poly_text16(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // Delegate to 8-bit text rendering for now.
    // PolyText16 has the same structure but with 2-byte characters.
    // We treat the low byte as the character index.
    handle_poly_text8(state, data)
}

// ---------------------------------------------------------------------------
// Opcode 76: ImageText8
// ---------------------------------------------------------------------------

fn handle_image_text8(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let str_len = data[1] as usize;
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let text = if 16 + str_len <= data.len() {
        &data[16..16 + str_len]
    } else {
        return Vec::new();
    };

    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    let (img_w, img_h, pixels) = font.render_text(text, gc.foreground, gc.background);
    if img_w == 0 || img_h == 0 {
        return Vec::new();
    }

    let render_y = y - font.font_ascent;
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.put_image(x, render_y, img_w, img_h, &pixels);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 77: ImageText16
// ---------------------------------------------------------------------------

fn handle_image_text16(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // Delegate to 8-bit ImageText for now.
    // ImageText16 uses 2-byte chars; we extract the low byte.
    if data.len() < 16 {
        return Vec::new();
    }
    let str_len = data[1] as usize;
    let text_start = 16;
    let text_end = text_start + str_len * 2;
    if text_end > data.len() {
        return Vec::new();
    }

    // Build equivalent 8-bit request: take low byte of each 2-byte char
    let mut fake_data = Vec::with_capacity(16 + str_len);
    fake_data.extend_from_slice(&data[0..16]);
    for i in 0..str_len {
        let offset = text_start + i * 2;
        // CHAR2B: byte1 (high), byte2 (low) -- we take byte2
        fake_data.push(data[offset + 1]);
    }
    fake_data[1] = str_len as u8;
    handle_image_text8(state, &fake_data)
}

// ---------------------------------------------------------------------------
// Opcode 78: CreateColormap
// ---------------------------------------------------------------------------

fn handle_create_colormap(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    // For TrueColor visual, colormaps are effectively no-ops.
    // We just acknowledge the request. The colormap ID is not tracked
    // because TrueColor doesn't need it.
    let _ = state;
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 80: CopyColormapAndFree
// ---------------------------------------------------------------------------

fn handle_copy_colormap_and_free(_state: &mut ClientState, _data: &[u8], _seq: u16) -> Vec<u8> {
    // No-op for TrueColor
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 81: InstallColormap
// ---------------------------------------------------------------------------

fn handle_install_colormap(_seq: u16) -> Vec<u8> {
    // No-op for TrueColor
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 82: UninstallColormap
// ---------------------------------------------------------------------------

fn handle_uninstall_colormap(_seq: u16) -> Vec<u8> {
    // No-op for TrueColor
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 83: ListInstalledColormaps
// ---------------------------------------------------------------------------

fn handle_list_installed_colormaps(state: &ClientState, seq: u16) -> Vec<u8> {
    // Return just the default colormap
    let mut reply = vec![0u8; 36];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&1u32.to_le_bytes()); // length = 1 (4 extra bytes)
    reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // n_cmaps = 1
    reply[32..36].copy_from_slice(&ROOT_COLORMAP.to_le_bytes());
    let _ = state;
    reply
}

// ---------------------------------------------------------------------------
// Opcode 84: AllocColor
// ---------------------------------------------------------------------------

fn handle_alloc_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let red = u16::from_le_bytes([data[8], data[9]]);
    let green = u16::from_le_bytes([data[10], data[11]]);
    let blue = u16::from_le_bytes([data[12], data[13]]);

    let r8 = (red >> 8) as u32;
    let g8 = (green >> 8) as u32;
    let b8 = (blue >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&red.to_le_bytes());
    reply[10..12].copy_from_slice(&green.to_le_bytes());
    reply[12..14].copy_from_slice(&blue.to_le_bytes());
    reply[16..20].copy_from_slice(&pixel.to_le_bytes());

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 85: AllocNamedColor
// ---------------------------------------------------------------------------

fn handle_alloc_named_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        std::str::from_utf8(&data[12..12 + name_len]).unwrap_or("")
    } else {
        ""
    };

    let (r16, g16, b16) = parse_color_name(name);
    let r8 = (r16 >> 8) as u32;
    let g8 = (g16 >> 8) as u32;
    let b8 = (b16 >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    info!("AllocNamedColor: name={name:?} -> pixel={pixel:#x}");

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&pixel.to_le_bytes());
    reply[12..14].copy_from_slice(&r16.to_le_bytes()); // exact red
    reply[14..16].copy_from_slice(&g16.to_le_bytes()); // exact green
    reply[16..18].copy_from_slice(&b16.to_le_bytes()); // exact blue
    reply[18..20].copy_from_slice(&r16.to_le_bytes()); // visual red
    reply[20..22].copy_from_slice(&g16.to_le_bytes()); // visual green
    reply[22..24].copy_from_slice(&b16.to_le_bytes()); // visual blue

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 86: AllocColorCells
// ---------------------------------------------------------------------------

fn handle_alloc_color_cells(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // TrueColor visuals don't support writable colormaps
    build_error(BAD_ALLOC, seq, 0, 86, 0)
}

// ---------------------------------------------------------------------------
// Opcode 87: AllocColorPlanes
// ---------------------------------------------------------------------------

fn handle_alloc_color_planes(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // TrueColor visuals don't support writable colormaps
    build_error(BAD_ALLOC, seq, 0, 87, 0)
}

// ---------------------------------------------------------------------------
// Opcode 91: QueryColors
// ---------------------------------------------------------------------------

fn handle_query_colors(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }

    let n_pixels = (data.len() - 8) / 4;
    let mut colors = Vec::with_capacity(n_pixels);

    for i in 0..n_pixels {
        let offset = 8 + i * 4;
        let pixel = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);

        // Decompose TrueColor pixel back to 16-bit RGB
        let r = ((pixel >> 16) & 0xFF) as u16;
        let g = ((pixel >> 8) & 0xFF) as u16;
        let b = (pixel & 0xFF) as u16;

        colors.push((r << 8 | r, g << 8 | g, b << 8 | b));
    }

    let data_len = n_pixels * 8; // Each RGB is 8 bytes (r2, g2, b2, pad2)
    let padded = (data_len + 3) & !3;
    let length_field = (padded / 4) as u32;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..10].copy_from_slice(&(n_pixels as u16).to_le_bytes());

    for (i, &(r, g, b)) in colors.iter().enumerate() {
        let off = 32 + i * 8;
        reply[off..off + 2].copy_from_slice(&r.to_le_bytes());
        reply[off + 2..off + 4].copy_from_slice(&g.to_le_bytes());
        reply[off + 4..off + 6].copy_from_slice(&b.to_le_bytes());
        // pad at off+6..off+8
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 92: LookupColor
// ---------------------------------------------------------------------------

fn handle_lookup_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        std::str::from_utf8(&data[12..12 + name_len]).unwrap_or("")
    } else {
        ""
    };

    let (r16, g16, b16) = parse_color_name(name);

    // Reply: exact and visual colors
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&r16.to_le_bytes());
    reply[10..12].copy_from_slice(&g16.to_le_bytes());
    reply[12..14].copy_from_slice(&b16.to_le_bytes());
    reply[14..16].copy_from_slice(&r16.to_le_bytes());
    reply[16..18].copy_from_slice(&g16.to_le_bytes());
    reply[18..20].copy_from_slice(&b16.to_le_bytes());

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 93: CreateCursor
// ---------------------------------------------------------------------------

fn handle_create_cursor(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }

    let cid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _source_pixmap = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _mask_pixmap = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    // fore_red(2), fore_green(2), fore_blue(2), back_red(2), back_green(2), back_blue(2), x(2), y(2)

    // We can't easily convert arbitrary pixmap data to a CSS cursor,
    // so store as "default" for now. A future improvement could encode
    // the pixmap as a data URI.
    info!("CreateCursor: id={cid:#x} (bitmap cursor, using default)");
    state.cursors.insert(cid, "default".to_string());

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 94: CreateGlyphCursor
// ---------------------------------------------------------------------------

fn handle_create_glyph_cursor(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 32 { return Vec::new(); }
    let cid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let source_char = u16::from_le_bytes([data[16], data[17]]);
    let css_name = glyph_to_css_cursor(source_char).to_string();
    info!("CreateGlyphCursor: id={cid:#x} glyph={source_char} -> \"{css_name}\"");
    state.cursors.insert(cid, css_name);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 95: FreeCursor
// ---------------------------------------------------------------------------

fn handle_free_cursor(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let cid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        state.cursors.remove(&cid);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 97: QueryBestSize
// ---------------------------------------------------------------------------

fn handle_query_best_size(data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    if data.len() >= 12 {
        reply[8..10].copy_from_slice(&data[8..10]); // width
        reply[10..12].copy_from_slice(&data[10..12]); // height
    }
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 98: QueryExtension
// ---------------------------------------------------------------------------

fn handle_query_extension(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Parse extension name from the request
    let name_len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let name = if data.len() >= 8 + name_len {
        std::str::from_utf8(&data[8..8 + name_len]).unwrap_or("")
    } else {
        ""
    };

    debug!("QueryExtension: \"{}\"", name);

    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());

    match name {
        "RENDER" => {
            reply[8] = 1; // present = true
            reply[9] = 139; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "MIT-SHM" => {
            reply[8] = 1;
            reply[9] = 130;
            reply[10] = 65; // ShmCompletion
            reply[11] = 128;
        }
        "BIG-REQUESTS" => {
            reply[8] = 1;
            reply[9] = 133;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XFIXES" => {
            reply[8] = 1;
            reply[9] = 138;
            reply[10] = 87;
            reply[11] = 0;
        }
        "SHAPE" => {
            reply[8] = 1;
            reply[9] = 128;
            reply[10] = 64;
            reply[11] = 0;
        }
        "SYNC" => {
            reply[8] = 1;
            reply[9] = 134;
            reply[10] = 100;
            reply[11] = 0;
        }
        "Generic Event Extension" => {
            reply[8] = 1;
            reply[9] = 135;
            reply[10] = 0;
            reply[11] = 0;
        }
        "Composite" => {
            reply[8] = 1;
            reply[9] = 142;
        }
        "DAMAGE" => {
            reply[8] = 1;
            reply[9] = 143;
            reply[10] = 91;
            reply[11] = 152;
        }
        "RANDR" => {
            reply[8] = 1;
            reply[9] = 140;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XKEYBOARD" => {
            reply[8] = 1;
            reply[9] = 136;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XC-MISC" => {
            reply[8] = 1;
            reply[9] = 141;
            reply[10] = 0;
            reply[11] = 0;
        }
        "Present" => {
            reply[8] = 1;
            reply[9] = 148;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XInputExtension" => {
            reply[8] = 1;
            reply[9] = crate::xinput2::XI_MAJOR_OPCODE;
            reply[10] = crate::xinput2::XI_FIRST_EVENT;
            reply[11] = crate::xinput2::XI_FIRST_ERROR;
        }
        "XINERAMA" => {
            // Not present -- already zero
        }
        _ => {
            // present = false (byte 8 = 0) -- already zero
        }
    }

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 99: ListExtensions
// ---------------------------------------------------------------------------

fn handle_list_extensions(seq: u16) -> Vec<u8> {
    let extensions: &[&str] = &["BIG-REQUESTS", "MIT-SHM", "RENDER", "XFIXES", "SHAPE", "SYNC", "Generic Event Extension", "XC-MISC", "Composite", "DAMAGE", "Present", "RANDR", "XInputExtension", "XKEYBOARD"];

    let mut names_data = Vec::new();
    for ext in extensions {
        names_data.push(ext.len() as u8);
        names_data.extend_from_slice(ext.as_bytes());
    }
    while names_data.len() % 4 != 0 {
        names_data.push(0);
    }

    let extra_len = names_data.len();
    let mut reply = vec![0u8; 32 + extra_len];
    reply[0] = 1; // Reply
    reply[1] = extensions.len() as u8;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra_len / 4) as u32).to_le_bytes());
    reply[32..].copy_from_slice(&names_data);

    reply
}

// ---------------------------------------------------------------------------
// Opcode 100: ChangeKeyboardMapping
// ---------------------------------------------------------------------------

fn handle_change_keyboard_mapping(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Store the mapping (we accept but don't actually remap our built-in table).
    // Send MappingNotify event so clients refresh their keymap cache.
    let mut event = [0u8; 32];
    event[0] = MAPPING_NOTIFY_EVENT;
    event[2..4].copy_from_slice(&seq.to_le_bytes());
    event[4] = 1; // request = Keyboard
    // first_keycode and count would come from the request
    if _data.len() >= 6 {
        event[5] = _data[4]; // first_keycode
        event[6] = _data[5]; // count (keycode_count)
    }
    state.pending_events.push(event.to_vec());

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 101: GetKeyboardMapping
// ---------------------------------------------------------------------------

fn handle_get_keyboard_mapping(data: &[u8], seq: u16) -> Vec<u8> {
    let first_keycode = if data.len() >= 5 { data[4] } else { 8 };
    let count = if data.len() >= 6 { data[5] } else { 248 };

    let keysyms_per_keycode: u8 = 4;
    let total_syms = count as u32 * keysyms_per_keycode as u32;
    let reply_len = 32 + total_syms as usize * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    reply[1] = keysyms_per_keycode;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&total_syms.to_le_bytes());

    for i in 0..count as usize {
        let keycode = first_keycode as usize + i;
        let offset = 32 + i * keysyms_per_keycode as usize * 4;

        let (normal, shifted) = keycode_to_keysym(keycode as u8);

        reply[offset..offset + 4].copy_from_slice(&normal.to_le_bytes());
        reply[offset + 4..offset + 8].copy_from_slice(&shifted.to_le_bytes());
        // Mode switch and mode+shift left as 0 (NoSymbol)
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 102: ChangeKeyboardControl
// ---------------------------------------------------------------------------

fn handle_change_keyboard_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }

    let value_mask = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let mut offset = 8;

    for bit in 0..8u32 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset], data[offset + 1],
                    data[offset + 2], data[offset + 3],
                ]);
                match bit {
                    0 => state.keyboard_control.key_click_percent = val as u8,
                    1 => state.keyboard_control.bell_percent = val as u8,
                    2 => state.keyboard_control.bell_pitch = val as u16,
                    3 => state.keyboard_control.bell_duration = val as u16,
                    4 => state.keyboard_control.led_mask = val,
                    5 => { /* led_mode - no-op */ }
                    6 => { /* key - auto-repeat key */ }
                    7 => state.keyboard_control.global_auto_repeat = val as u8,
                    _ => {}
                }
                offset += 4;
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 103: GetKeyboardControl
// ---------------------------------------------------------------------------

fn handle_get_keyboard_control(state: &ClientState, seq: u16) -> Vec<u8> {
    let kc = &state.keyboard_control;
    let mut reply = [0u8; 52]; // 32 + 20 extra
    reply[0] = 1;
    reply[1] = kc.global_auto_repeat;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&5u32.to_le_bytes()); // length = 5 (20 extra bytes)
    reply[8..12].copy_from_slice(&kc.led_mask.to_le_bytes());
    reply[12] = kc.key_click_percent;
    reply[13] = kc.bell_percent;
    reply[14..16].copy_from_slice(&kc.bell_pitch.to_le_bytes());
    reply[16..18].copy_from_slice(&kc.bell_duration.to_le_bytes());
    // auto_repeats: 32 bytes at offset 20
    reply[20..52].copy_from_slice(&kc.auto_repeats);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 105: ChangePointerControl
// ---------------------------------------------------------------------------

fn handle_change_pointer_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let accel_num = i16::from_le_bytes([data[4], data[5]]);
    let accel_den = i16::from_le_bytes([data[6], data[7]]);
    let threshold = i16::from_le_bytes([data[8], data[9]]);
    let do_accel = data[10] != 0;
    let do_threshold = data[11] != 0;

    if do_accel {
        if accel_num > 0 {
            state.pointer_control.acceleration_numerator = accel_num as u16;
        }
        if accel_den > 0 {
            state.pointer_control.acceleration_denominator = accel_den as u16;
        }
    }
    if do_threshold && threshold >= 0 {
        state.pointer_control.threshold = threshold as u16;
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 106: GetPointerControl
// ---------------------------------------------------------------------------

fn handle_get_pointer_control(state: &ClientState, seq: u16) -> Vec<u8> {
    let pc = &state.pointer_control;
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&pc.acceleration_numerator.to_le_bytes());
    reply[10..12].copy_from_slice(&pc.acceleration_denominator.to_le_bytes());
    reply[12..14].copy_from_slice(&pc.threshold.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 107: SetScreenSaver
// ---------------------------------------------------------------------------

fn handle_set_screen_saver(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 10 {
        return Vec::new();
    }

    let timeout = i16::from_le_bytes([data[4], data[5]]);
    let interval = i16::from_le_bytes([data[6], data[7]]);
    let prefer_blanking = data[8];
    let allow_exposures = data[9];

    if timeout >= 0 {
        state.screen_saver.timeout = timeout as u16;
    }
    if interval >= 0 {
        state.screen_saver.interval = interval as u16;
    }
    if prefer_blanking <= 2 {
        state.screen_saver.prefer_blanking = prefer_blanking;
    }
    if allow_exposures <= 2 {
        state.screen_saver.allow_exposures = allow_exposures;
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 108: GetScreenSaver
// ---------------------------------------------------------------------------

fn handle_get_screen_saver(state: &ClientState, seq: u16) -> Vec<u8> {
    let ss = &state.screen_saver;
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&ss.timeout.to_le_bytes());
    reply[10..12].copy_from_slice(&ss.interval.to_le_bytes());
    reply[12] = ss.prefer_blanking;
    reply[13] = ss.allow_exposures;
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 110: ListHosts
// ---------------------------------------------------------------------------

fn handle_list_hosts(seq: u16) -> Vec<u8> {
    // Return empty host list with access control disabled
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // mode = Disabled
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0, n_hosts = 0 (already zero)
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 112: SetCloseDownMode
// ---------------------------------------------------------------------------

fn handle_set_close_down_mode(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 2 {
        state.close_down_mode = data[1];
        debug!("SetCloseDownMode: mode={}", data[1]);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 113: KillClient
// ---------------------------------------------------------------------------

fn handle_kill_client(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }

    let resource = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    if resource == 0 {
        // AllTemporary: destroy all resources from clients with
        // close-down mode RetainTemporary. We approximate this as a no-op
        // since we don't track other clients' resources.
        debug!("KillClient: AllTemporary (no-op)");
    } else {
        // Destroy all resources belonging to the client that created `resource`.
        // In our single-connection-per-client model, we look up which windows
        // own this resource and destroy them.
        debug!("KillClient: resource={resource:#x}");

        // Find windows owned by the client that created this resource
        let owner = state.windows.get(&resource).map(|w| w.owner_client_id.clone());
        if let Some(owner_id) = owner {
            let to_destroy: Vec<u32> = state
                .windows
                .values()
                .filter(|w| w.owner_client_id == owner_id)
                .map(|w| w.id)
                .collect();
            for wid in to_destroy {
                state.windows.remove(&wid);
                if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
                    state.window_router.unregister_all(&[uuid.clone()]);
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowDestroyed { window_id: uuid },
                    ));
                }
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 114: RotateProperties
// ---------------------------------------------------------------------------

fn handle_rotate_properties(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let n_atoms = u16::from_le_bytes([data[8], data[9]]) as usize;
    let delta = i16::from_le_bytes([data[10], data[11]]);

    if n_atoms == 0 || delta == 0 {
        return Vec::new();
    }

    // Read the atom list
    let mut atoms = Vec::with_capacity(n_atoms);
    for i in 0..n_atoms {
        let off = 12 + i * 4;
        if off + 4 <= data.len() {
            atoms.push(u32::from_le_bytes([
                data[off], data[off + 1], data[off + 2], data[off + 3],
            ]));
        }
    }

    if atoms.len() < 2 {
        return Vec::new();
    }

    // Extract property values for these atoms
    let values: Vec<Option<PropertyValue>> = atoms
        .iter()
        .map(|a| {
            state
                .windows
                .get(&window)
                .and_then(|w| w.properties.get(a))
                .cloned()
        })
        .collect();

    // Rotate: delta > 0 means properties rotate toward higher indices
    let n = values.len() as i16;
    let effective_delta = ((delta % n) + n) % n;

    if let Some(win) = state.windows.get_mut(&window) {
        for (i, atom) in atoms.iter().enumerate() {
            let src_idx = ((i as i16 - effective_delta + n) % n) as usize;
            if let Some(Some(val)) = values.get(src_idx) {
                win.properties.insert(*atom, val.clone());
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 116: SetPointerMapping
// ---------------------------------------------------------------------------

fn handle_set_pointer_mapping(seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // MappingSuccess
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 117: GetPointerMapping
// ---------------------------------------------------------------------------

fn handle_get_pointer_mapping(seq: u16) -> Vec<u8> {
    let map: [u8; 7] = [1, 2, 3, 4, 5, 6, 7];
    let n = map.len() as u8;
    let padded_len = (n as usize + 3) & !3;
    let reply_extra_units = (padded_len / 4) as u32;
    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1; // Reply
    reply[1] = n;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&reply_extra_units.to_le_bytes());
    reply[32..32 + n as usize].copy_from_slice(&map);
    reply
}

// ---------------------------------------------------------------------------
// Opcode 118: SetModifierMapping
// ---------------------------------------------------------------------------

fn handle_set_modifier_mapping(seq: u16) -> Vec<u8> {
    // Return MappingSuccess
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // MappingSuccess
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 119: GetModifierMapping
// ---------------------------------------------------------------------------

fn handle_get_modifier_mapping(seq: u16) -> Vec<u8> {
    const KEYCODES_PER_MODIFIER: u8 = 2;
    const MODIFIER_MAP: [[u8; KEYCODES_PER_MODIFIER as usize]; 8] = [
        [50, 62],   // Shift: Shift_L, Shift_R
        [66, 0],    // Lock:  Caps_Lock
        [37, 105],  // Control: Control_L, Control_R
        [64, 108],  // Mod1 (Alt): Alt_L, Alt_R
        [77, 0],    // Mod2 (NumLock): Num_Lock
        [0, 0],     // Mod3 (unused)
        [133, 134], // Mod4 (Super): Super_L, Super_R
        [0, 0],     // Mod5 (AltGr / Mode_switch -- unused)
    ];
    let data_len = 8 * KEYCODES_PER_MODIFIER as u32;
    let reply_len = 32 + data_len as usize;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[1] = KEYCODES_PER_MODIFIER;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((data_len / 4).to_le_bytes()));
    for (i, row) in MODIFIER_MAP.iter().enumerate() {
        let off = 32 + i * KEYCODES_PER_MODIFIER as usize;
        reply[off..off + KEYCODES_PER_MODIFIER as usize]
            .copy_from_slice(row);
    }
    reply
}
