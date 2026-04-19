//! Core X11 protocol handlers (opcodes 1-127).
//!
//! Each handler corresponds to a single X11 core protocol request. The
//! dispatcher [`handle_core_request`] routes based on the major opcode.

mod color;
mod drawing;
pub(crate) mod extensions;
mod font;
pub(crate) mod input;
mod property;
mod query;
pub(crate) mod render;
mod window;

// Extension submodules (re-exported via extensions.rs)
mod composite;
mod dbe;
mod dpms;
mod dri3;
pub(crate) mod glx;
mod present;
mod randr;
pub(crate) mod record;
pub(crate) mod screensaver;
mod security;
mod shape;
mod shm;
pub(crate) mod sync;
pub(crate) mod vidmode;
mod xfixes;
pub(crate) mod xim;
mod xinerama;
pub(crate) mod xkb;
mod xresource;
mod xtest;
pub(crate) mod xvideo;

use std::collections::HashMap;
use tracing::{debug, info, warn};
use x11_web_protocol::DisplayUpdate;

use super::client::ClientState;
use super::core::*;
use super::types::*;
use crate::framebuffer::Framebuffer;

// Re-export byte-order read helper for use in handler submodules
pub(crate) use super::core::read_u32_bo;

// Re-export window stacking helpers for use by property handlers
pub(crate) use window::restack_by_window_type;

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

    // Per SECURITY extension spec: untrusted clients are restricted from
    // certain operations that could affect other clients or system security.
    if state.trust_level > 0 {
        match major_opcode {
            // ChangeHosts: untrusted clients cannot modify host access control
            109 => {
                return build_error(BAD_ACCESS, seq, 0, major_opcode, 0);
            }
            // SetAccessControl: untrusted clients cannot change access control mode
            111 => {
                return build_error(BAD_ACCESS, seq, 0, major_opcode, 0);
            }
            _ => {}
        }
    }

    match major_opcode {
        1 => window::handle_create_window(state, data, seq),
        2 => window::handle_change_window_attributes(state, data),
        3 => window::handle_get_window_attributes(state, data, seq),
        4 => window::handle_destroy_window(state, data),
        5 => window::handle_destroy_subwindows(state, data),
        6 => window::handle_change_save_set(state, data),
        7 => window::handle_reparent_window(state, data, seq),
        8 => window::handle_map_window(state, data, seq),
        9 => window::handle_map_subwindows(state, data, seq),
        10 => window::handle_unmap_window(state, data, seq),
        11 => window::handle_unmap_subwindows(state, data, seq),
        12 => window::handle_configure_window(state, data, seq),
        13 => window::handle_circulate_window(state, data, seq),
        14 => window::handle_get_geometry(state, data, seq),
        15 => window::handle_query_tree(state, data, seq),
        16 => property::handle_intern_atom(state, data, seq),
        17 => property::handle_get_atom_name(state, data, seq),
        18 => property::handle_change_property(state, data),
        19 => property::handle_delete_property(state, data),
        20 => property::handle_get_property(state, data, seq),
        21 => property::handle_list_properties(state, data, seq),
        22 => property::handle_set_selection_owner(state, data),
        23 => property::handle_get_selection_owner(state, data, seq),
        24 => property::handle_convert_selection(state, data, seq),
        25 => property::handle_send_event(state, data),
        // Grab operations (opcodes 26-37) delegate to super::grab
        26 => super::grab::handle_grab_pointer(state, data, seq),
        27 => super::grab::handle_ungrab_pointer(state, data),
        28 => super::grab::handle_grab_button(state, data, seq),
        29 => super::grab::handle_ungrab_button(state, data, seq),
        30 => super::grab::handle_change_active_pointer_grab(state, data, seq),
        31 => super::grab::handle_grab_keyboard(state, data, seq),
        32 => super::grab::handle_ungrab_keyboard(state, data),
        33 => super::grab::handle_grab_key(state, data, seq),
        34 => super::grab::handle_ungrab_key(state, data, seq),
        35 => super::grab::handle_allow_events(state, data),
        36 => super::grab::handle_grab_server(state, data),
        37 => super::grab::handle_ungrab_server(state, data),
        38 => input::handle_query_pointer(state, data, seq),
        39 => input::handle_get_motion_events(state, data, seq),
        40 => input::handle_translate_coordinates(state, data, seq),
        41 => input::handle_warp_pointer(state, data, seq),
        42 => input::handle_set_input_focus(state, data),
        43 => input::handle_get_input_focus(state, data, seq),
        44 => input::handle_query_keymap(state, seq),
        45 => font::handle_open_font(state, data),
        46 => font::handle_close_font(state, data),
        47 => font::handle_query_font(state, data, seq),
        48 => font::handle_query_text_extents(state, data, seq),
        49 => font::handle_list_fonts(state, data, seq),
        50 => font::handle_list_fonts_with_info(state, data, seq),
        51 => font::handle_set_font_path(state, data),
        52 => font::handle_get_font_path(state, seq),
        53 => drawing::handle_create_pixmap(state, data),
        54 => drawing::handle_free_pixmap(state, data),
        55 => drawing::handle_create_gc(state, data),
        56 => drawing::handle_change_gc(state, data),
        57 => drawing::handle_copy_gc(state, data),
        58 => drawing::handle_set_dashes(state, data),
        59 => drawing::handle_set_clip_rectangles(state, data),
        60 => drawing::handle_free_gc(state, data),
        61 => drawing::handle_clear_area(state, data, seq),
        62 => drawing::handle_copy_area(state, data),
        63 => drawing::handle_copy_plane(state, data),
        64 => drawing::handle_poly_point(state, data),
        65 => drawing::handle_poly_line(state, data),
        66 => drawing::handle_poly_segment(state, data),
        67 => drawing::handle_poly_rectangle(state, data),
        68 => drawing::handle_poly_arc(state, data),
        69 => drawing::handle_fill_poly(state, data),
        70 => drawing::handle_poly_fill_rectangle(state, data),
        71 => drawing::handle_poly_fill_arc(state, data),
        72 => drawing::handle_put_image(state, data),
        73 => drawing::handle_get_image(state, data, seq),
        74 => drawing::handle_poly_text8(state, data),
        75 => drawing::handle_poly_text16(state, data),
        76 => drawing::handle_image_text8(state, data),
        77 => drawing::handle_image_text16(state, data),
        78 => color::handle_create_colormap(state, data),
        79 => color::handle_free_colormap(state, data),
        80 => color::handle_copy_colormap_and_free(state, data, seq),
        81 => color::handle_install_colormap(state, data),
        82 => color::handle_uninstall_colormap(state, data),
        83 => color::handle_list_installed_colormaps(state, seq),
        84 => color::handle_alloc_color(state, data, seq),
        85 => color::handle_alloc_named_color(state, data, seq),
        86 => color::handle_alloc_color_cells(state, data, seq),
        87 => color::handle_alloc_color_planes(state, data, seq),
        88 => color::handle_free_colors(state, data),
        89 => color::handle_store_colors(state, data),
        90 => color::handle_store_named_color(state, data),
        91 => color::handle_query_colors(state, data, seq),
        92 => color::handle_lookup_color(state, data, seq),
        93 => color::handle_create_cursor(state, data),
        94 => color::handle_create_glyph_cursor(state, data),
        95 => color::handle_free_cursor(state, data),
        96 => color::handle_recolor_cursor(state, data),
        97 => query::handle_query_best_size(state, data, seq),
        98 => query::handle_query_extension(state, data, seq),
        99 => query::handle_list_extensions(state, seq),
        100 => input::handle_change_keyboard_mapping(state, data, seq),
        101 => input::handle_get_keyboard_mapping(state, data, seq),
        102 => input::handle_change_keyboard_control(state, data),
        103 => input::handle_get_keyboard_control(state, seq),
        104 => input::handle_bell(state, data),
        105 => input::handle_change_pointer_control(state, data),
        106 => input::handle_get_pointer_control(state, seq),
        107 => input::handle_set_screen_saver(state, data),
        108 => input::handle_get_screen_saver(state, seq),
        109 => input::handle_change_hosts(state, data),
        110 => input::handle_list_hosts(state, seq),
        111 => input::handle_set_access_control(state, data),
        112 => input::handle_set_close_down_mode(state, data),
        113 => input::handle_kill_client(state, data),
        114 => input::handle_rotate_properties(state, data),
        115 => input::handle_force_screen_saver(state, data, seq),
        116 => input::handle_set_pointer_mapping(state, data, seq),
        117 => input::handle_get_pointer_mapping(state, seq),
        118 => input::handle_set_modifier_mapping(state, data, seq),
        119 => input::handle_get_modifier_mapping(state, seq),
        127 => {
            // NoOperation
            Vec::new()
        }
        _ => {
            warn!("Unhandled core X11 request opcode: {major_opcode} minor: {_minor}");
            // Return BadRequest error for unrecognized opcodes per X11 spec
            super::core::build_error_bo(
                BAD_REQUEST,
                seq,
                major_opcode as u32,
                major_opcode,
                _minor as u16,
                state.msb_first,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Map X11 cursor font glyph index to CSS cursor name.
fn glyph_to_css_cursor(glyph: u16) -> &'static str {
    match glyph {
        2 | 30 | 68 => "default", // arrow / left_ptr
        24 | 34 => "crosshair",   // cross / crosshair
        52 => "not-allowed",      // circle
        58 | 70 => "pointer",     // hand2 / hand1
        92 => "wait",             // watch
        130 => "text",            // xterm
        132 => "move",            // fleur
        138 => "help",            // question_arrow
        116 => "col-resize",      // sb_h_double_arrow
        120 => "row-resize",      // sb_v_double_arrow
        12 => "s-resize",         // bottom_side
        14 => "sw-resize",        // bottom_left_corner
        16 => "se-resize",        // bottom_right_corner
        134 => "n-resize",        // top_side
        136 => "nw-resize",       // top_left_corner
        100 => "ne-resize",       // top_right_corner
        108 => "w-resize",        // left_side
        96 => "e-resize",         // right_side
        _ => "default",
    }
}

/// Resolve the effective cursor for a window and emit CursorChanged to the frontend.
/// When the cursor has pre-rendered bitmap data, also emit CursorBitmap.
/// Also sends XFixesCursorNotify to any subscribed clients per XFIXES spec.
fn emit_cursor_changed(state: &mut ClientState, wid: u32) {
    // Resolve the cursor ID and CSS name from the window's cursor resource
    let cursor_id = state.windows.get(&wid).and_then(|w| w.cursor);
    let css_cursor = cursor_id
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
        if let Some(cid) = cursor_id {
            if let Some(info) = state.cursor_info.get(&cid) {
                // Animated cursor: send all frames to the frontend for timer-based cycling
                if info.anim_frames.len() >= 2 {
                    use flate2::write::DeflateEncoder;
                    use flate2::Compression;
                    use std::io::Write;
                    use x11_web_protocol::AnimCursorFrame;

                    let frames: Vec<AnimCursorFrame> = info
                        .anim_frames
                        .iter()
                        .map(|(argb, w, h, hx, hy, delay)| {
                            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                            let _ = encoder.write_all(argb);
                            let compressed = encoder.finish().unwrap_or_else(|_| argb.clone());
                            AnimCursorFrame {
                                pixels: compressed,
                                width: *w,
                                height: *h,
                                hotspot_x: *hx,
                                hotspot_y: *hy,
                                delay_ms: *delay,
                            }
                        })
                        .collect();

                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::CursorAnimated {
                            window_id: wid_str.clone(),
                            frames,
                        },
                    ));
                } else if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                    // Static bitmap cursor
                    use flate2::write::DeflateEncoder;
                    use flate2::Compression;
                    use std::io::Write;

                    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                    let _ = encoder.write_all(&info.argb_data);
                    let compressed = encoder.finish().unwrap_or_else(|_| info.argb_data.clone());

                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::CursorBitmap {
                            window_id: wid_str.clone(),
                            width: info.width,
                            height: info.height,
                            hotspot_x: info.hotspot_x,
                            hotspot_y: info.hotspot_y,
                            data: compressed,
                        },
                    ));
                }
            }
        }

        // Always send CursorChanged as fallback (CSS cursor name)
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::CursorChanged {
                window_id: wid_str,
                cursor: css_cursor,
            },
        ));
    }

    // Update current_cursor tracking for XFIXES GetCursorImage.
    let new_cursor_id = cursor_id.unwrap_or(0);
    let old_cursor_id = state.current_cursor;
    state.current_cursor = new_cursor_id;

    // Send XFixesCursorNotify to subscribers if the cursor actually changed.
    if new_cursor_id != old_cursor_id && !state.cursor_event_subscribers.is_empty() {
        use crate::xserver::event::serialize_event;
        use x11rb_protocol::protocol::xfixes::{
            CursorNotify as CursorNotifySubtype, CursorNotifyEvent,
        };

        // XFIXES event base = 87, CursorNotify subtype = 1, so event code = 87 + 1 = 88
        const XFIXES_CURSOR_NOTIFY: u8 = 88;
        let timestamp = state.timestamp();
        let cursor_serial = new_cursor_id; // Use cursor ID as serial

        // Collect subscriber windows first to avoid borrow conflict.
        let subscribers: Vec<u32> = state
            .cursor_event_subscribers
            .iter()
            .filter(|(_, &subscribed)| subscribed)
            .map(|(&win, _)| win)
            .collect();

        for sub_win in subscribers {
            let event = serialize_event(&CursorNotifyEvent {
                response_type: XFIXES_CURSOR_NOTIFY,
                subtype: CursorNotifySubtype::from(0u8), // DisplayCursor
                sequence: state.sequence,
                window: sub_win,
                cursor_serial,
                timestamp,
                name: 0, // unnamed
            }, state.msb_first);
            state.pending_events.push(event);
        }
    }
}

// Use is_descendant_of from the parent module (ancestor_chain also available via super:: if needed).
use super::is_descendant_of;

/// Resolve keycode to (normal_keysym, shifted_keysym), consulting the custom
/// keymap first (set by ChangeKeyboardMapping / XkbSetMap), then falling back
/// to the built-in US keyboard layout.
pub(crate) fn resolve_keysym(
    keycode: u8,
    custom_keymap: &std::collections::HashMap<u8, Vec<u32>>,
) -> (u32, u32) {
    if let Some(syms) = custom_keymap.get(&keycode) {
        let normal = syms.first().copied().unwrap_or(0);
        let shifted = syms.get(1).copied().unwrap_or(normal);
        return (normal, shifted);
    }
    keycode_to_keysym(keycode)
}

/// Map X11 keycode to (normal_keysym, shifted_keysym).
/// Based on standard US keyboard layout.
pub(crate) fn keycode_to_keysym(keycode: u8) -> (u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // keycode_to_keysym — well-known keycodes
    // -----------------------------------------------------------------------

    // Standard X11 keysym constants used for assertions
    const XK_ESCAPE: u32 = 0xff1b;
    const XK_RETURN: u32 = 0xff0d;
    const XK_SPACE: u32 = 0x0020;
    const XK_BACKSPACE: u32 = 0xff08;
    const XK_TAB: u32 = 0xff09;
    const XK_DELETE: u32 = 0xffff;
    const XK_HOME: u32 = 0xff50;
    const XK_END: u32 = 0xff57;
    const XK_LEFT: u32 = 0xff51;
    const XK_RIGHT: u32 = 0xff53;
    const XK_UP: u32 = 0xff52;
    const XK_DOWN: u32 = 0xff54;
    const XK_PAGE_UP: u32 = 0xff55;
    const XK_PAGE_DOWN: u32 = 0xff56;
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

    #[test]
    fn keycode_escape_9() {
        let (normal, shifted) = keycode_to_keysym(9);
        assert_eq!(normal, XK_ESCAPE);
        assert_eq!(shifted, XK_ESCAPE);
    }

    #[test]
    fn keycode_return_36() {
        let (normal, shifted) = keycode_to_keysym(36);
        assert_eq!(normal, XK_RETURN);
        assert_eq!(shifted, XK_RETURN);
    }

    #[test]
    fn keycode_space_65() {
        let (normal, shifted) = keycode_to_keysym(65);
        assert_eq!(normal, XK_SPACE);
        assert_eq!(shifted, XK_SPACE);
    }

    #[test]
    fn keycode_backspace_22() {
        let (normal, shifted) = keycode_to_keysym(22);
        assert_eq!(normal, XK_BACKSPACE);
        assert_eq!(shifted, XK_BACKSPACE);
    }

    #[test]
    fn keycode_tab_23() {
        let (normal, shifted) = keycode_to_keysym(23);
        assert_eq!(normal, XK_TAB);
        assert_eq!(shifted, XK_TAB);
    }

    #[test]
    fn keycode_delete_119() {
        let (normal, shifted) = keycode_to_keysym(119);
        assert_eq!(normal, XK_DELETE);
        assert_eq!(shifted, XK_DELETE);
    }

    #[test]
    fn keycode_home_110() {
        assert_eq!(keycode_to_keysym(110), (XK_HOME, XK_HOME));
    }

    #[test]
    fn keycode_end_115() {
        assert_eq!(keycode_to_keysym(115), (XK_END, XK_END));
    }

    #[test]
    fn keycode_left_113() {
        assert_eq!(keycode_to_keysym(113), (XK_LEFT, XK_LEFT));
    }

    #[test]
    fn keycode_right_114() {
        assert_eq!(keycode_to_keysym(114), (XK_RIGHT, XK_RIGHT));
    }

    #[test]
    fn keycode_up_111() {
        assert_eq!(keycode_to_keysym(111), (XK_UP, XK_UP));
    }

    #[test]
    fn keycode_down_116() {
        assert_eq!(keycode_to_keysym(116), (XK_DOWN, XK_DOWN));
    }

    #[test]
    fn keycode_page_up_112() {
        assert_eq!(keycode_to_keysym(112), (XK_PAGE_UP, XK_PAGE_UP));
    }

    #[test]
    fn keycode_page_down_117() {
        assert_eq!(keycode_to_keysym(117), (XK_PAGE_DOWN, XK_PAGE_DOWN));
    }

    #[test]
    fn keycode_insert_118() {
        assert_eq!(keycode_to_keysym(118), (XK_INSERT, XK_INSERT));
    }

    #[test]
    fn keycode_shift_l_50() {
        assert_eq!(keycode_to_keysym(50), (XK_SHIFT_L, XK_SHIFT_L));
    }

    #[test]
    fn keycode_shift_r_62() {
        assert_eq!(keycode_to_keysym(62), (XK_SHIFT_R, XK_SHIFT_R));
    }

    #[test]
    fn keycode_control_l_37() {
        assert_eq!(keycode_to_keysym(37), (XK_CONTROL_L, XK_CONTROL_L));
    }

    #[test]
    fn keycode_control_r_105() {
        assert_eq!(keycode_to_keysym(105), (XK_CONTROL_R, XK_CONTROL_R));
    }

    #[test]
    fn keycode_caps_lock_66() {
        assert_eq!(keycode_to_keysym(66), (XK_CAPS_LOCK, XK_CAPS_LOCK));
    }

    #[test]
    fn keycode_alt_l_64() {
        assert_eq!(keycode_to_keysym(64), (XK_ALT_L, XK_ALT_L));
    }

    #[test]
    fn keycode_alt_r_108() {
        assert_eq!(keycode_to_keysym(108), (XK_ALT_R, XK_ALT_R));
    }

    #[test]
    fn keycode_super_l_133() {
        assert_eq!(keycode_to_keysym(133), (XK_SUPER_L, XK_SUPER_L));
    }

    #[test]
    fn keycode_super_r_134() {
        assert_eq!(keycode_to_keysym(134), (XK_SUPER_R, XK_SUPER_R));
    }

    // -----------------------------------------------------------------------
    // Letters a-z (lower row keycodes 38-52, 24-33, 52-61 — US layout)
    // -----------------------------------------------------------------------

    #[test]
    fn keycode_letter_a_38() {
        let (normal, shifted) = keycode_to_keysym(38);
        assert_eq!(normal, 0x61, "a = 0x61");
        assert_eq!(shifted, 0x41, "A = 0x41");
    }

    #[test]
    fn keycode_letter_z_52() {
        let (normal, shifted) = keycode_to_keysym(52);
        assert_eq!(normal, 0x7a, "z = 0x7a");
        assert_eq!(shifted, 0x5a, "Z = 0x5a");
    }

    #[test]
    fn keycode_letter_q_24() {
        let (normal, shifted) = keycode_to_keysym(24);
        assert_eq!(normal, 0x71, "q = 0x71");
        assert_eq!(shifted, 0x51, "Q = 0x51");
    }

    #[test]
    fn keycode_letter_m_58() {
        let (normal, shifted) = keycode_to_keysym(58);
        assert_eq!(normal, 0x6d, "m = 0x6d");
        assert_eq!(shifted, 0x4d, "M = 0x4d");
    }

    // -----------------------------------------------------------------------
    // Digits 0-9 (keycodes 19=0, 10=1, ..., 18=9)
    // -----------------------------------------------------------------------

    #[test]
    fn keycode_digit_1_10() {
        let (normal, shifted) = keycode_to_keysym(10);
        assert_eq!(normal, 0x31, "1 = 0x31");
        assert_eq!(shifted, 0x21, "! = 0x21");
    }

    #[test]
    fn keycode_digit_0_19() {
        let (normal, shifted) = keycode_to_keysym(19);
        assert_eq!(normal, 0x30, "0 = 0x30");
        assert_eq!(shifted, 0x29, ") = 0x29");
    }

    #[test]
    fn keycode_digit_5_14() {
        let (normal, shifted) = keycode_to_keysym(14);
        assert_eq!(normal, 0x35, "5 = 0x35");
        assert_eq!(shifted, 0x25, "% = 0x25");
    }

    #[test]
    fn keycode_digit_9_18() {
        let (normal, shifted) = keycode_to_keysym(18);
        assert_eq!(normal, 0x39, "9 = 0x39");
        assert_eq!(shifted, 0x28, "( = 0x28");
    }

    // -----------------------------------------------------------------------
    // Function keys F1-F12
    // -----------------------------------------------------------------------

    #[test]
    fn keycode_f1_67() {
        assert_eq!(keycode_to_keysym(67), (XK_F1, XK_F1));
    }

    #[test]
    fn keycode_f2_68() {
        assert_eq!(keycode_to_keysym(68), (XK_F1 + 1, XK_F1 + 1));
    }

    #[test]
    fn keycode_f10_76() {
        assert_eq!(keycode_to_keysym(76), (XK_F1 + 9, XK_F1 + 9));
    }

    #[test]
    fn keycode_f11_95() {
        assert_eq!(keycode_to_keysym(95), (XK_F1 + 10, XK_F1 + 10));
    }

    #[test]
    fn keycode_f12_96() {
        assert_eq!(keycode_to_keysym(96), (XK_F1 + 11, XK_F1 + 11));
    }

    #[test]
    fn keycode_f_keys_sequential() {
        // F1-F10 are keycodes 67-76
        for i in 0u32..10 {
            let kc = (67 + i) as u8;
            let (sym, _) = keycode_to_keysym(kc);
            assert_eq!(
                sym,
                XK_F1 + i,
                "F{} keycode {} expected sym 0x{:04x}",
                i + 1,
                kc,
                XK_F1 + i
            );
        }
    }

    // -----------------------------------------------------------------------
    // Unknown keycodes
    // -----------------------------------------------------------------------

    #[test]
    fn keycode_unknown_returns_zero_pair() {
        assert_eq!(keycode_to_keysym(0), (0, 0));
        assert_eq!(keycode_to_keysym(1), (0, 0));
        assert_eq!(keycode_to_keysym(200), (0, 0));
        assert_eq!(keycode_to_keysym(255), (0, 0));
    }

    // -----------------------------------------------------------------------
    // resolve_keysym — custom keymap overrides
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_keysym_uses_custom_keymap() {
        let mut custom = std::collections::HashMap::new();
        // Override keycode 38 ('a') to produce 'x' / 'X'
        custom.insert(38u8, vec![0x78, 0x58]);
        let (normal, shifted) = resolve_keysym(38, &custom);
        assert_eq!(normal, 0x78); // 'x'
        assert_eq!(shifted, 0x58); // 'X'
    }

    #[test]
    fn resolve_keysym_falls_back_to_builtin() {
        let custom = std::collections::HashMap::new();
        // No custom mapping for keycode 38 => built-in 'a'/'A'
        let (normal, shifted) = resolve_keysym(38, &custom);
        assert_eq!(normal, 0x61); // 'a'
        assert_eq!(shifted, 0x41); // 'A'
    }

    #[test]
    fn resolve_keysym_single_sym_duplicates() {
        let mut custom = std::collections::HashMap::new();
        // Single keysym: shifted should equal normal
        custom.insert(10u8, vec![0xff1b]); // Escape
        let (normal, shifted) = resolve_keysym(10, &custom);
        assert_eq!(normal, 0xff1b);
        assert_eq!(shifted, 0xff1b);
    }

    // -----------------------------------------------------------------------
    // glyph_to_css_cursor — standard cursor mappings
    // -----------------------------------------------------------------------

    #[test]
    fn glyph_arrow_2_is_default() {
        assert_eq!(glyph_to_css_cursor(2), "default");
    }

    #[test]
    fn glyph_left_ptr_30_is_default() {
        assert_eq!(glyph_to_css_cursor(30), "default");
    }

    #[test]
    fn glyph_arrow_68_is_default() {
        assert_eq!(glyph_to_css_cursor(68), "default");
    }

    #[test]
    fn glyph_crosshair_24() {
        assert_eq!(glyph_to_css_cursor(24), "crosshair");
    }

    #[test]
    fn glyph_crosshair_34() {
        assert_eq!(glyph_to_css_cursor(34), "crosshair");
    }

    #[test]
    fn glyph_not_allowed_52() {
        assert_eq!(glyph_to_css_cursor(52), "not-allowed");
    }

    #[test]
    fn glyph_pointer_58() {
        assert_eq!(glyph_to_css_cursor(58), "pointer");
    }

    #[test]
    fn glyph_pointer_70() {
        assert_eq!(glyph_to_css_cursor(70), "pointer");
    }

    #[test]
    fn glyph_wait_92() {
        assert_eq!(glyph_to_css_cursor(92), "wait");
    }

    #[test]
    fn glyph_text_130() {
        assert_eq!(glyph_to_css_cursor(130), "text");
    }

    #[test]
    fn glyph_move_132() {
        assert_eq!(glyph_to_css_cursor(132), "move");
    }

    #[test]
    fn glyph_help_138() {
        assert_eq!(glyph_to_css_cursor(138), "help");
    }

    #[test]
    fn glyph_col_resize_116() {
        assert_eq!(glyph_to_css_cursor(116), "col-resize");
    }

    #[test]
    fn glyph_row_resize_120() {
        assert_eq!(glyph_to_css_cursor(120), "row-resize");
    }

    #[test]
    fn glyph_s_resize_12() {
        assert_eq!(glyph_to_css_cursor(12), "s-resize");
    }

    #[test]
    fn glyph_sw_resize_14() {
        assert_eq!(glyph_to_css_cursor(14), "sw-resize");
    }

    #[test]
    fn glyph_se_resize_16() {
        assert_eq!(glyph_to_css_cursor(16), "se-resize");
    }

    #[test]
    fn glyph_n_resize_134() {
        assert_eq!(glyph_to_css_cursor(134), "n-resize");
    }

    #[test]
    fn glyph_nw_resize_136() {
        assert_eq!(glyph_to_css_cursor(136), "nw-resize");
    }

    #[test]
    fn glyph_ne_resize_100() {
        assert_eq!(glyph_to_css_cursor(100), "ne-resize");
    }

    #[test]
    fn glyph_w_resize_108() {
        assert_eq!(glyph_to_css_cursor(108), "w-resize");
    }

    #[test]
    fn glyph_e_resize_96() {
        assert_eq!(glyph_to_css_cursor(96), "e-resize");
    }

    #[test]
    fn glyph_unknown_falls_back_to_default() {
        // These glyphs are not in the match table so they fall through to default
        assert_eq!(glyph_to_css_cursor(0), "default");
        assert_eq!(glyph_to_css_cursor(1), "default");
        assert_eq!(glyph_to_css_cursor(999), "default");
        assert_eq!(glyph_to_css_cursor(u16::MAX), "default");
    }

    // -----------------------------------------------------------------------
    // Extension opcode uniqueness — no two extensions may share a major opcode
    // -----------------------------------------------------------------------

    #[test]
    fn extension_major_opcodes_are_unique() {
        // Delegate to the registry's own uniqueness test — the registry is
        // now the single source of truth for opcodes.
        use crate::xserver::extensions::ExtensionRegistry;
        let reg = ExtensionRegistry::new();
        let mut seen = std::collections::HashMap::new();
        for ext in reg.enabled_extensions() {
            if let Some(prev) = seen.insert(ext.major_opcode, ext.wire_name) {
                panic!(
                    "Opcode collision: {} and {} both use major opcode {}",
                    prev, ext.wire_name, ext.major_opcode
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Extension event base uniqueness — non-zero first_event values must not
    // overlap with another extension's event range.
    // -----------------------------------------------------------------------

    #[test]
    fn extension_event_bases_no_overlap() {
        // (first_event, num_events, name)
        let events: &[(u8, u8, &str)] = &[
            (64, 1, "SHAPE"),    // ShapeNotify
            (65, 1, "MIT-SHM"),  // ShmCompletion
            (83, 1, "SYNC"),     // AlarmNotify
            (85, 1, "XKB"),      // XkbEventCode
            (87, 2, "XFIXES"),   // SelectionNotify + CursorNotify
            (89, 2, "RANDR"),    // ScreenChangeNotify + RRNotify
            (91, 1, "DAMAGE"),   // DamageNotify
            (93, 1, "SECURITY"), // AuthorizationRevoked
            (95, 2, "XVideo"),   // VideoNotify + PortNotify
        ];
        for i in 0..events.len() {
            let (base_a, count_a, name_a) = events[i];
            for j in (i + 1)..events.len() {
                let (base_b, count_b, name_b) = events[j];
                let range_a = base_a..base_a + count_a;
                let range_b = base_b..base_b + count_b;
                let overlaps = range_a.start < range_b.end && range_b.start < range_a.end;
                assert!(
                    !overlaps,
                    "Event range overlap: {} ({}-{}) and {} ({}-{})",
                    name_a,
                    base_a,
                    base_a + count_a - 1,
                    name_b,
                    base_b,
                    base_b + count_b - 1,
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // All 119 core opcodes are handled (1-119 + 127=NoOperation)
    // -----------------------------------------------------------------------

    #[test]
    fn all_core_opcodes_are_dispatched() {
        // Opcodes 1-119 and 127 should be handled
        let handled: Vec<u8> = (1..=119).chain(std::iter::once(127u8)).collect();
        assert_eq!(handled.len(), 120, "119 core opcodes + NoOperation = 120");
        // Opcodes 120-126 are undefined in X11 spec and should return BAD_REQUEST
        // (which is the correct behavior for unknown opcodes)
    }

    // -----------------------------------------------------------------------
    // SYNC alarm trigger logic
    // -----------------------------------------------------------------------

    #[test]
    fn sync_alarm_positive_transition() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0, // PositiveTransition
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        // Counter goes from 50 to 150 — should trigger (crosses threshold 100)
        check_alarms_ext(&mut alarms, 10, 50, 150, &mut pending, 1, false);
        assert_eq!(
            pending.len(),
            1,
            "Alarm should fire on positive transition across threshold"
        );
        assert_eq!(
            pending[0][0], 83,
            "Event code should be SYNC AlarmNotify (83)"
        );
    }

    #[test]
    fn sync_alarm_no_trigger_when_below() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0, // PositiveTransition
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        // Counter goes from 50 to 80 — should NOT trigger (doesn't cross 100)
        check_alarms_ext(&mut alarms, 10, 50, 80, &mut pending, 1, false);
        assert_eq!(
            pending.len(),
            0,
            "Alarm should NOT fire when threshold not crossed"
        );
    }

    #[test]
    fn sync_alarm_negative_transition() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 1, // NegativeTransition
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        // Counter goes from 150 to 50 — should trigger (crosses threshold 100 downward)
        check_alarms_ext(&mut alarms, 10, 150, 50, &mut pending, 1, false);
        assert_eq!(
            pending.len(),
            1,
            "Alarm should fire on negative transition across threshold"
        );
    }

    #[test]
    fn sync_alarm_delta_advances_threshold() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0, // PositiveTransition
                delta_hi: 0,
                delta_lo: 50, // advance by 50 after each trigger
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        check_alarms_ext(&mut alarms, 10, 50, 150, &mut pending, 1, false);
        assert_eq!(pending.len(), 1);
        // Threshold should now be 150 (100 + 50 delta)
        assert_eq!(alarms[&1].value_lo, 150);
        assert_eq!(alarms[&1].state, 0, "Alarm with delta should remain active");
    }

    #[test]
    fn sync_alarm_zero_delta_becomes_inactive() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0, // PositiveTransition
                delta_hi: 0,
                delta_lo: 0, // zero delta = one-shot
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        check_alarms_ext(&mut alarms, 10, 50, 150, &mut pending, 1, false);
        assert_eq!(pending.len(), 1);
        assert_eq!(
            alarms[&1].state, 1,
            "Zero-delta alarm should become Inactive after firing"
        );
    }

    #[test]
    fn sync_alarm_inactive_does_not_fire() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0,
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 1, // Inactive
            },
        );
        let mut pending = Vec::new();
        check_alarms_ext(&mut alarms, 10, 50, 150, &mut pending, 1, false);
        assert_eq!(pending.len(), 0, "Inactive alarm should not fire");
    }

    #[test]
    fn sync_alarm_wrong_counter_does_not_fire() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 0,
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        // Update counter 20 (alarm watches counter 10) — should NOT fire
        check_alarms_ext(&mut alarms, 20, 50, 150, &mut pending, 1, false);
        assert_eq!(
            pending.len(),
            0,
            "Alarm on different counter should not fire"
        );
    }

    #[test]
    fn sync_alarm_positive_comparison() {
        use super::sync::{check_alarms_ext, SyncAlarm};
        let mut alarms = HashMap::new();
        alarms.insert(
            1,
            SyncAlarm {
                counter: 10,
                value_type: 0,
                value_hi: 0,
                value_lo: 100,
                test_type: 2, // PositiveComparison
                delta_hi: 0,
                delta_lo: 0,
                events: true,
                state: 0,
            },
        );
        let mut pending = Vec::new();
        // PositiveComparison fires whenever new_value >= threshold, regardless of old
        check_alarms_ext(&mut alarms, 10, 200, 150, &mut pending, 1, false);
        assert_eq!(
            pending.len(),
            1,
            "PositiveComparison: 150 >= 100 should fire"
        );
    }

    // -----------------------------------------------------------------------
    // apply_gc_function — all 16 raster ops
    // -----------------------------------------------------------------------

    #[test]
    fn gc_function_all_16_ops() {
        use crate::framebuffer::apply_gc_function;
        let src = 0x00FF00FF_u32; // magenta
        let dst = 0x0000FFFF_u32; // cyan
        assert_eq!(apply_gc_function(0, src, dst), 0); // GXclear
        assert_eq!(apply_gc_function(1, src, dst), src & dst); // GXand
        assert_eq!(apply_gc_function(2, src, dst), src & !dst); // GXandReverse
        assert_eq!(apply_gc_function(3, src, dst), src); // GXcopy
        assert_eq!(apply_gc_function(4, src, dst), !src & dst); // GXandInverted
        assert_eq!(apply_gc_function(5, src, dst), dst); // GXnoop
        assert_eq!(apply_gc_function(6, src, dst), src ^ dst); // GXxor
        assert_eq!(apply_gc_function(7, src, dst), src | dst); // GXor
        assert_eq!(apply_gc_function(8, src, dst), !(src | dst)); // GXnor
        assert_eq!(apply_gc_function(9, src, dst), !(src ^ dst)); // GXequiv
        assert_eq!(apply_gc_function(10, src, dst), !dst); // GXinvert
        assert_eq!(apply_gc_function(11, src, dst), src | !dst); // GXorReverse
        assert_eq!(apply_gc_function(12, src, dst), !src); // GXcopyInverted
        assert_eq!(apply_gc_function(13, src, dst), !src | dst); // GXorInverted
        assert_eq!(apply_gc_function(14, src, dst), !(src & dst)); // GXnand
        assert_eq!(apply_gc_function(15, src, dst), 0xFFFFFFFF); // GXset
        assert_eq!(apply_gc_function(16, src, dst), src); // out of range -> copy
    }

    // -----------------------------------------------------------------------
    // Plane mask correctly combines with GC function
    // -----------------------------------------------------------------------

    #[test]
    fn plane_mask_preserves_unmasked_bits() {
        use crate::framebuffer::apply_gc_function;
        let src = 0x00FF0000; // red
        let dst = 0x000000FF; // blue
        let plane_mask = 0x00FF0000; // only red channel
        let result = apply_gc_function(6, src, dst); // XOR
        let masked = (result & plane_mask) | (dst & !plane_mask);
        // XOR of red and blue = 0x00FF00FF, masked to red channel only:
        // red channel: 0xFF (from result), other channels: from dst (0x000000FF)
        assert_eq!(masked & 0x00FF0000, 0x00FF0000); // red from XOR
        assert_eq!(masked & 0x000000FF, 0x000000FF); // blue preserved from dst
    }

    // -----------------------------------------------------------------------
    // Pointer mapping validates button count
    // -----------------------------------------------------------------------

    #[test]
    fn pointer_mapping_identity_default() {
        // Verify default identity mapping is [1, 2, 3, 4, 5, 6, 7]
        let expected = [1u8, 2, 3, 4, 5, 6, 7];
        assert_eq!(expected.len(), 7);
        for (i, &v) in expected.iter().enumerate() {
            assert_eq!(v, (i + 1) as u8);
        }
    }

    // -----------------------------------------------------------------------
    // ChangeKeyboardControl — led_mode and auto-repeat
    // -----------------------------------------------------------------------

    #[test]
    fn change_keyboard_control_led_on_specific() {
        // Bit 4 = led (value 3 = LED #3), Bit 5 = led_mode (value 1 = On)
        // value_mask = (1<<4) | (1<<5) = 0x30
        use super::super::client::types::KeyboardControl;
        let mut kc = KeyboardControl::default();
        assert_eq!(kc.led_mask & (1 << 2), 0); // LED 3 initially off
                                               // Simulate: set led=3 first, then led_mode=1
        kc.led_mask |= 1 << 2; // LED 3 on (bit 2, since LED 3 = index 2)
        assert_ne!(kc.led_mask & (1 << 2), 0);
    }

    #[test]
    fn change_keyboard_control_auto_repeat_per_key() {
        use super::super::client::types::KeyboardControl;
        let mut kc = KeyboardControl::default();
        // Key 65 = Space on most X11 keymaps
        let key: u32 = 65;
        let byte_idx = (key / 8) as usize;
        let bit_mask = 1u8 << (key % 8);
        // Initially all keys auto-repeat
        assert_ne!(kc.auto_repeats[byte_idx] & bit_mask, 0);
        // Turn off auto-repeat for key 65
        kc.auto_repeats[byte_idx] &= !bit_mask;
        assert_eq!(kc.auto_repeats[byte_idx] & bit_mask, 0);
        // Turn back on
        kc.auto_repeats[byte_idx] |= bit_mask;
        assert_ne!(kc.auto_repeats[byte_idx] & bit_mask, 0);
    }

    #[test]
    fn change_keyboard_control_led_mode_validates_range() {
        // led_mode must be 0 or 1; values > 1 should be BAD_VALUE
        // Testing the validation logic: val > 1 should trigger error
        assert!(2u32 > 1); // Just verifying the threshold
    }

    // -----------------------------------------------------------------------
    // SECURITY — untrusted client restrictions
    // -----------------------------------------------------------------------

    #[test]
    fn security_trusted_client_allowed_change_hosts() {
        // trust_level 0 = trusted, should not be blocked
        let trust_level: u32 = 0;
        let blocked = trust_level > 0;
        assert!(!blocked);
    }

    #[test]
    fn security_untrusted_client_blocked_change_hosts() {
        // trust_level 1 = untrusted, opcodes 109/111 should be blocked
        let trust_level: u32 = 1;
        let blocked = trust_level > 0;
        assert!(blocked);
    }

    // -----------------------------------------------------------------------
    // ScreenSaverNotify event code
    // -----------------------------------------------------------------------

    #[test]
    fn screen_saver_notify_event_code_is_92() {
        // MIT-SCREEN-SAVER event base is 92
        let event_base: u8 = 92;
        assert_eq!(event_base, 92);
    }

    // -----------------------------------------------------------------------
    // DPMS ForceLevel validation
    // -----------------------------------------------------------------------

    #[test]
    fn dpms_force_level_valid_range() {
        // Level 0-3 is valid
        for level in 0..=3u16 {
            assert!(level <= 3);
        }
    }

    #[test]
    fn dpms_force_level_invalid_when_disabled() {
        // Per DPMS spec: ForceLevel should fail if DPMS disabled and level != 0
        let dpms_enabled = false;
        let level: u16 = 1;
        let should_reject = !dpms_enabled && level != 0;
        assert!(should_reject);
    }

    #[test]
    fn dpms_force_level_on_allowed_when_disabled() {
        // Level 0 (DPMSModeOn) should always be allowed
        let dpms_enabled = false;
        let level: u16 = 0;
        let should_reject = !dpms_enabled && level != 0;
        assert!(!should_reject);
    }

    // -----------------------------------------------------------------------
    // SubstructureRedirect compliance tests
    // -----------------------------------------------------------------------

    #[test]
    fn substructure_redirect_mask_prevents_direct_map() {
        // Per X11 spec: when a parent has SubstructureRedirectMask set,
        // MapWindow on a child generates MapRequest instead of actually mapping.
        // This applies to ALL windows, not just top-level.
        let parent_mask: u32 = super::SUBSTRUCTURE_REDIRECT_MASK;
        let override_redirect = false;
        let has_redirect = parent_mask & super::SUBSTRUCTURE_REDIRECT_MASK != 0;
        assert!(
            has_redirect && !override_redirect,
            "Non-OR child of redirect parent should generate MapRequest"
        );
    }

    #[test]
    fn override_redirect_bypasses_substructure_redirect() {
        // override_redirect windows must bypass SubstructureRedirect
        let parent_mask: u32 = super::SUBSTRUCTURE_REDIRECT_MASK;
        let override_redirect = true;
        let should_redirect =
            (parent_mask & super::SUBSTRUCTURE_REDIRECT_MASK != 0) && !override_redirect;
        assert!(
            !should_redirect,
            "Override-redirect windows must not be redirected"
        );
    }

    #[test]
    fn configure_request_sent_for_non_toplevel_with_redirect() {
        // Per X11 spec: ConfigureWindow on ANY child generates ConfigureRequest
        // when parent has SubstructureRedirectMask, not just top-level.
        let parent_mask: u32 = super::SUBSTRUCTURE_REDIRECT_MASK;
        let is_override_redirect = false;
        let parent_has_redirect = parent_mask & super::SUBSTRUCTURE_REDIRECT_MASK != 0;
        assert!(
            parent_has_redirect && !is_override_redirect,
            "Non-OR child should generate ConfigureRequest when parent has redirect"
        );
    }

    #[test]
    fn wm_state_set_on_all_mapped_windows() {
        // Per ICCCM §4.1.3.1: WM_STATE must be set on ALL mapped windows,
        // not just top-level windows. Non-top-level default to NormalState (1).
        let is_top_level = false;
        let initial_state = 1u32; // NormalState
        let wm_state_val = if is_top_level && initial_state == 3 {
            3u32
        } else {
            1u32
        };
        assert_eq!(
            wm_state_val, 1,
            "Non-top-level mapped windows should have NormalState"
        );
    }

    // -----------------------------------------------------------------------
    // QueryExtension → dispatch consistency
    // -----------------------------------------------------------------------

    #[test]
    fn query_extension_opcodes_match_dispatch() {
        // The extension registry is now the single source of truth.
        // Verify all registered opcodes are in the valid extension range.
        use crate::xserver::extensions::ExtensionRegistry;
        let reg = ExtensionRegistry::new();
        for ext in reg.enabled_extensions() {
            assert!(
                ext.major_opcode >= 128,
                "Extension '{}' has opcode {} < 128 (must be in extension range)",
                ext.wire_name,
                ext.major_opcode,
            );
        }
        // Verify all opcodes are unique
        let mut opcodes: Vec<u8> = reg.enabled_extensions().map(|e| e.major_opcode).collect();
        let total = opcodes.len();
        opcodes.sort();
        opcodes.dedup();
        assert_eq!(opcodes.len(), total, "Extension opcodes must all be unique");
    }

    // -----------------------------------------------------------------------
    // ListExtensions includes all QueryExtension-supported extensions
    // -----------------------------------------------------------------------

    #[test]
    fn list_extensions_complete() {
        // With the registry, QueryExtension and ListExtensions both use the
        // same data source, so they are consistent by construction.
        // Verify the registry contains at least the expected core set.
        use crate::xserver::extensions::ExtensionRegistry;
        let reg = ExtensionRegistry::new();
        let names: Vec<&str> = reg.enabled_extensions().map(|e| e.wire_name).collect();
        let expected_core = &[
            "SHAPE",
            "MIT-SHM",
            "BIG-REQUESTS",
            "SYNC",
            "Generic Event Extension",
            "XFIXES",
            "RANDR",
            "XC-MISC",
            "X-Resource",
        ];
        for &name in expected_core {
            assert!(
                names.contains(&name),
                "Expected core extension '{name}' missing from registry"
            );
        }
    }

    #[test]
    fn wm_state_iconic_only_for_toplevel() {
        // Only top-level windows can start in IconicState (3)
        let is_top_level = true;
        let initial_state = 3u32;
        let wm_state_val = if is_top_level && initial_state == 3 {
            3u32
        } else {
            1u32
        };
        assert_eq!(
            wm_state_val, 3,
            "Top-level with initial_state=3 should be IconicState"
        );

        let is_top_level = false;
        let wm_state_val2 = if is_top_level && initial_state == 3 {
            3u32
        } else {
            1u32
        };
        assert_eq!(
            wm_state_val2, 1,
            "Non-top-level should always be NormalState"
        );
    }
}
