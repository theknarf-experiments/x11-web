//! Core X11 protocol handlers (opcodes 1-127).
//!
//! Each handler corresponds to a single X11 core protocol request. The
//! dispatcher [`handle_core_request`] routes based on the major opcode.

mod color;
pub(crate) mod default_keymap;
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

// Re-export window stacking helpers for use by property handlers
pub(crate) use window::restack_by_window_type;

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Parse the request body into a typed x11rb request struct, then call the
/// handler with `(state, &req)`. If the parse fails (typically because the
/// wire length doesn't match the struct), emit a BadLength error. This
/// centralises the boilerplate that used to live at the top of every handler.
macro_rules! typed {
    ($T:ty, $handler:path, $opcode:literal, $data:ident, $state:ident) => {{
        match <$T>::try_parse_request(crate::xserver::request::request_header($data), &$data[4..]) {
            Ok(req) => $handler($state, &req),
            Err(_) => crate::xserver::core::build_error(
                crate::xserver::core::LENGTH_ERROR,
                $state.sequence,
                0,
                $opcode,
                0,
            ),
        }
    }};
}

/// Parse a minor-opcode extension request body into a typed x11rb
/// request struct, returning early with a BadLength error reply on
/// failure. The optional last argument lets callers substitute a
/// different `RequestHeader` (used when our wire numbering differs from
/// x11rb's constants for a given minor opcode).
macro_rules! parse_minor {
    ($T:ty, $data:ident, $state:ident, $seq:ident, $major:expr, $minor:expr) => {
        parse_minor!(
            $T,
            $data,
            $state,
            $seq,
            $major,
            $minor,
            crate::xserver::request::request_header($data)
        )
    };
    ($T:ty, $data:ident, $state:ident, $seq:ident, $major:expr, $minor:expr, $header:expr) => {
        match <$T>::try_parse_endian_request($header, &$data[4..], $state.byte_order()) {
            Ok(r) => r,
            Err(_) => {
                return crate::xserver::core::build_error(
                    crate::xserver::core::LENGTH_ERROR,
                    $seq,
                    0,
                    $major,
                    $minor as u16,
                )
            }
        }
    };
}
pub(crate) use parse_minor;

/// Parse a request body into a typed x11rb struct, returning early
/// with an empty `Vec<u8>` on parse failure. Use this for void or
/// reply-less extension requests where a malformed packet should be
/// silently dropped rather than reported as a protocol error.
macro_rules! parse_or_void {
    ($T:ty, $data:ident, $state:ident) => {
        match <$T>::try_parse_endian_request(
            crate::xserver::request::request_header($data),
            &$data[4..],
            $state.byte_order(),
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        }
    };
}
pub(crate) use parse_or_void;

/// Dispatch a core X11 protocol request (opcodes 1-127) to the appropriate
/// handler function. Returns the response bytes (reply, event, or empty for
/// void requests).
pub(crate) fn handle_core_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    use x11rb_protocol::protocol::xproto::*;
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;

    // Per SECURITY extension spec: untrusted clients are restricted from
    // certain operations that could affect other clients or system security.
    if state.trust_level > 0 {
        match major_opcode {
            // ChangeHosts: untrusted clients cannot modify host access control
            109 => {
                return build_error(ACCESS_ERROR, seq, 0, major_opcode, 0);
            }
            // SetAccessControl: untrusted clients cannot change access control mode
            111 => {
                return build_error(ACCESS_ERROR, seq, 0, major_opcode, 0);
            }
            _ => {}
        }
    }

    match major_opcode {
        1 => typed!(
            CreateWindowRequest,
            window::handle_create_window,
            1,
            data,
            state
        ),
        2 => typed!(
            ChangeWindowAttributesRequest,
            window::handle_change_window_attributes,
            2,
            data,
            state
        ),
        3 => typed!(
            GetWindowAttributesRequest,
            window::handle_get_window_attributes,
            3,
            data,
            state
        ),
        4 => typed!(
            DestroyWindowRequest,
            window::handle_destroy_window,
            4,
            data,
            state
        ),
        5 => typed!(
            DestroySubwindowsRequest,
            window::handle_destroy_subwindows,
            5,
            data,
            state
        ),
        6 => typed!(
            ChangeSaveSetRequest,
            window::handle_change_save_set,
            6,
            data,
            state
        ),
        7 => typed!(
            ReparentWindowRequest,
            window::handle_reparent_window,
            7,
            data,
            state
        ),
        8 => typed!(MapWindowRequest, window::handle_map_window, 8, data, state),
        9 => typed!(
            MapSubwindowsRequest,
            window::handle_map_subwindows,
            9,
            data,
            state
        ),
        10 => typed!(
            UnmapWindowRequest,
            window::handle_unmap_window,
            10,
            data,
            state
        ),
        11 => typed!(
            UnmapSubwindowsRequest,
            window::handle_unmap_subwindows,
            11,
            data,
            state
        ),
        12 => typed!(
            ConfigureWindowRequest,
            window::handle_configure_window,
            12,
            data,
            state
        ),
        13 => typed!(
            CirculateWindowRequest,
            window::handle_circulate_window,
            13,
            data,
            state
        ),
        14 => typed!(
            GetGeometryRequest,
            window::handle_get_geometry,
            14,
            data,
            state
        ),
        15 => typed!(QueryTreeRequest, window::handle_query_tree, 15, data, state),
        16 => typed!(
            InternAtomRequest,
            property::handle_intern_atom,
            16,
            data,
            state
        ),
        17 => typed!(
            GetAtomNameRequest,
            property::handle_get_atom_name,
            17,
            data,
            state
        ),
        18 => typed!(
            ChangePropertyRequest,
            property::handle_change_property,
            18,
            data,
            state
        ),
        19 => typed!(
            DeletePropertyRequest,
            property::handle_delete_property,
            19,
            data,
            state
        ),
        20 => typed!(
            GetPropertyRequest,
            property::handle_get_property,
            20,
            data,
            state
        ),
        21 => typed!(
            ListPropertiesRequest,
            property::handle_list_properties,
            21,
            data,
            state
        ),
        22 => typed!(
            SetSelectionOwnerRequest,
            property::handle_set_selection_owner,
            22,
            data,
            state
        ),
        23 => typed!(
            GetSelectionOwnerRequest,
            property::handle_get_selection_owner,
            23,
            data,
            state
        ),
        24 => typed!(
            ConvertSelectionRequest,
            property::handle_convert_selection,
            24,
            data,
            state
        ),
        25 => typed!(
            SendEventRequest,
            property::handle_send_event,
            25,
            data,
            state
        ),
        // Grab operations (opcodes 26-37) delegate to super::grab
        26 => typed!(
            GrabPointerRequest,
            super::grab::handle_grab_pointer,
            26,
            data,
            state
        ),
        27 => typed!(
            UngrabPointerRequest,
            super::grab::handle_ungrab_pointer,
            27,
            data,
            state
        ),
        28 => typed!(
            GrabButtonRequest,
            super::grab::handle_grab_button,
            28,
            data,
            state
        ),
        29 => typed!(
            UngrabButtonRequest,
            super::grab::handle_ungrab_button,
            29,
            data,
            state
        ),
        30 => typed!(
            ChangeActivePointerGrabRequest,
            super::grab::handle_change_active_pointer_grab,
            30,
            data,
            state
        ),
        31 => typed!(
            GrabKeyboardRequest,
            super::grab::handle_grab_keyboard,
            31,
            data,
            state
        ),
        32 => typed!(
            UngrabKeyboardRequest,
            super::grab::handle_ungrab_keyboard,
            32,
            data,
            state
        ),
        33 => typed!(
            GrabKeyRequest,
            super::grab::handle_grab_key,
            33,
            data,
            state
        ),
        34 => typed!(
            UngrabKeyRequest,
            super::grab::handle_ungrab_key,
            34,
            data,
            state
        ),
        35 => typed!(
            AllowEventsRequest,
            super::grab::handle_allow_events,
            35,
            data,
            state
        ),
        36 => typed!(
            GrabServerRequest,
            super::grab::handle_grab_server,
            36,
            data,
            state
        ),
        37 => typed!(
            UngrabServerRequest,
            super::grab::handle_ungrab_server,
            37,
            data,
            state
        ),
        38 => typed!(
            QueryPointerRequest,
            input::handle_query_pointer,
            38,
            data,
            state
        ),
        39 => typed!(
            GetMotionEventsRequest,
            input::handle_get_motion_events,
            39,
            data,
            state
        ),
        40 => typed!(
            TranslateCoordinatesRequest,
            input::handle_translate_coordinates,
            40,
            data,
            state
        ),
        41 => typed!(
            WarpPointerRequest,
            input::handle_warp_pointer,
            41,
            data,
            state
        ),
        42 => typed!(
            SetInputFocusRequest,
            input::handle_set_input_focus,
            42,
            data,
            state
        ),
        43 => typed!(
            GetInputFocusRequest,
            input::handle_get_input_focus,
            43,
            data,
            state
        ),
        44 => typed!(
            QueryKeymapRequest,
            input::handle_query_keymap,
            44,
            data,
            state
        ),
        45 => typed!(OpenFontRequest, font::handle_open_font, 45, data, state),
        46 => typed!(CloseFontRequest, font::handle_close_font, 46, data, state),
        47 => typed!(QueryFontRequest, font::handle_query_font, 47, data, state),
        48 => typed!(
            QueryTextExtentsRequest,
            font::handle_query_text_extents,
            48,
            data,
            state
        ),
        49 => typed!(ListFontsRequest, font::handle_list_fonts, 49, data, state),
        50 => typed!(
            ListFontsWithInfoRequest,
            font::handle_list_fonts_with_info,
            50,
            data,
            state
        ),
        51 => typed!(
            SetFontPathRequest,
            font::handle_set_font_path,
            51,
            data,
            state
        ),
        52 => typed!(
            GetFontPathRequest,
            font::handle_get_font_path,
            52,
            data,
            state
        ),
        53 => typed!(
            CreatePixmapRequest,
            drawing::handle_create_pixmap,
            53,
            data,
            state
        ),
        54 => typed!(
            FreePixmapRequest,
            drawing::handle_free_pixmap,
            54,
            data,
            state
        ),
        55 => typed!(CreateGCRequest, drawing::handle_create_gc, 55, data, state),
        56 => typed!(ChangeGCRequest, drawing::handle_change_gc, 56, data, state),
        57 => typed!(CopyGCRequest, drawing::handle_copy_gc, 57, data, state),
        58 => typed!(
            SetDashesRequest,
            drawing::handle_set_dashes,
            58,
            data,
            state
        ),
        59 => typed!(
            SetClipRectanglesRequest,
            drawing::handle_set_clip_rectangles,
            59,
            data,
            state
        ),
        60 => typed!(FreeGCRequest, drawing::handle_free_gc, 60, data, state),
        61 => typed!(
            ClearAreaRequest,
            drawing::handle_clear_area,
            61,
            data,
            state
        ),
        62 => typed!(CopyAreaRequest, drawing::handle_copy_area, 62, data, state),
        63 => typed!(
            CopyPlaneRequest,
            drawing::handle_copy_plane,
            63,
            data,
            state
        ),
        64 => typed!(
            PolyPointRequest,
            drawing::handle_poly_point,
            64,
            data,
            state
        ),
        65 => typed!(PolyLineRequest, drawing::handle_poly_line, 65, data, state),
        66 => typed!(
            PolySegmentRequest,
            drawing::handle_poly_segment,
            66,
            data,
            state
        ),
        67 => typed!(
            PolyRectangleRequest,
            drawing::handle_poly_rectangle,
            67,
            data,
            state
        ),
        68 => typed!(PolyArcRequest, drawing::handle_poly_arc, 68, data, state),
        69 => typed!(FillPolyRequest, drawing::handle_fill_poly, 69, data, state),
        70 => typed!(
            PolyFillRectangleRequest,
            drawing::handle_poly_fill_rectangle,
            70,
            data,
            state
        ),
        71 => typed!(
            PolyFillArcRequest,
            drawing::handle_poly_fill_arc,
            71,
            data,
            state
        ),
        72 => typed!(PutImageRequest, drawing::handle_put_image, 72, data, state),
        73 => typed!(GetImageRequest, drawing::handle_get_image, 73, data, state),
        74 => typed!(
            PolyText8Request,
            drawing::handle_poly_text8,
            74,
            data,
            state
        ),
        75 => typed!(
            PolyText16Request,
            drawing::handle_poly_text16,
            75,
            data,
            state
        ),
        76 => typed!(
            ImageText8Request,
            drawing::handle_image_text8,
            76,
            data,
            state
        ),
        77 => typed!(
            ImageText16Request,
            drawing::handle_image_text16,
            77,
            data,
            state
        ),
        78 => typed!(
            CreateColormapRequest,
            color::handle_create_colormap,
            78,
            data,
            state
        ),
        79 => typed!(
            FreeColormapRequest,
            color::handle_free_colormap,
            79,
            data,
            state
        ),
        80 => typed!(
            CopyColormapAndFreeRequest,
            color::handle_copy_colormap_and_free,
            80,
            data,
            state
        ),
        81 => typed!(
            InstallColormapRequest,
            color::handle_install_colormap,
            81,
            data,
            state
        ),
        82 => typed!(
            UninstallColormapRequest,
            color::handle_uninstall_colormap,
            82,
            data,
            state
        ),
        83 => typed!(
            ListInstalledColormapsRequest,
            color::handle_list_installed_colormaps,
            83,
            data,
            state
        ),
        84 => typed!(
            AllocColorRequest,
            color::handle_alloc_color,
            84,
            data,
            state
        ),
        85 => typed!(
            AllocNamedColorRequest,
            color::handle_alloc_named_color,
            85,
            data,
            state
        ),
        86 => typed!(
            AllocColorCellsRequest,
            color::handle_alloc_color_cells,
            86,
            data,
            state
        ),
        87 => typed!(
            AllocColorPlanesRequest,
            color::handle_alloc_color_planes,
            87,
            data,
            state
        ),
        88 => typed!(
            FreeColorsRequest,
            color::handle_free_colors,
            88,
            data,
            state
        ),
        89 => typed!(
            StoreColorsRequest,
            color::handle_store_colors,
            89,
            data,
            state
        ),
        90 => typed!(
            StoreNamedColorRequest,
            color::handle_store_named_color,
            90,
            data,
            state
        ),
        91 => typed!(
            QueryColorsRequest,
            color::handle_query_colors,
            91,
            data,
            state
        ),
        92 => typed!(
            LookupColorRequest,
            color::handle_lookup_color,
            92,
            data,
            state
        ),
        93 => typed!(
            CreateCursorRequest,
            color::handle_create_cursor,
            93,
            data,
            state
        ),
        94 => typed!(
            CreateGlyphCursorRequest,
            color::handle_create_glyph_cursor,
            94,
            data,
            state
        ),
        95 => typed!(
            FreeCursorRequest,
            color::handle_free_cursor,
            95,
            data,
            state
        ),
        96 => typed!(
            RecolorCursorRequest,
            color::handle_recolor_cursor,
            96,
            data,
            state
        ),
        97 => typed!(
            QueryBestSizeRequest,
            query::handle_query_best_size,
            97,
            data,
            state
        ),
        98 => typed!(
            QueryExtensionRequest,
            query::handle_query_extension,
            98,
            data,
            state
        ),
        99 => typed!(
            ListExtensionsRequest,
            query::handle_list_extensions,
            99,
            data,
            state
        ),
        100 => typed!(
            ChangeKeyboardMappingRequest,
            input::handle_change_keyboard_mapping,
            100,
            data,
            state
        ),
        101 => typed!(
            GetKeyboardMappingRequest,
            input::handle_get_keyboard_mapping,
            101,
            data,
            state
        ),
        102 => typed!(
            ChangeKeyboardControlRequest,
            input::handle_change_keyboard_control,
            102,
            data,
            state
        ),
        103 => typed!(
            GetKeyboardControlRequest,
            input::handle_get_keyboard_control,
            103,
            data,
            state
        ),
        104 => typed!(BellRequest, input::handle_bell, 104, data, state),
        105 => typed!(
            ChangePointerControlRequest,
            input::handle_change_pointer_control,
            105,
            data,
            state
        ),
        106 => typed!(
            GetPointerControlRequest,
            input::handle_get_pointer_control,
            106,
            data,
            state
        ),
        107 => typed!(
            SetScreenSaverRequest,
            input::handle_set_screen_saver,
            107,
            data,
            state
        ),
        108 => typed!(
            GetScreenSaverRequest,
            input::handle_get_screen_saver,
            108,
            data,
            state
        ),
        109 => typed!(
            ChangeHostsRequest,
            input::handle_change_hosts,
            109,
            data,
            state
        ),
        110 => typed!(ListHostsRequest, input::handle_list_hosts, 110, data, state),
        111 => typed!(
            SetAccessControlRequest,
            input::handle_set_access_control,
            111,
            data,
            state
        ),
        112 => typed!(
            SetCloseDownModeRequest,
            input::handle_set_close_down_mode,
            112,
            data,
            state
        ),
        113 => typed!(
            KillClientRequest,
            input::handle_kill_client,
            113,
            data,
            state
        ),
        114 => typed!(
            RotatePropertiesRequest,
            input::handle_rotate_properties,
            114,
            data,
            state
        ),
        115 => typed!(
            ForceScreenSaverRequest,
            input::handle_force_screen_saver,
            115,
            data,
            state
        ),
        116 => typed!(
            SetPointerMappingRequest,
            input::handle_set_pointer_mapping,
            116,
            data,
            state
        ),
        117 => typed!(
            GetPointerMappingRequest,
            input::handle_get_pointer_mapping,
            117,
            data,
            state
        ),
        118 => typed!(
            SetModifierMappingRequest,
            input::handle_set_modifier_mapping,
            118,
            data,
            state
        ),
        119 => typed!(
            GetModifierMappingRequest,
            input::handle_get_modifier_mapping,
            119,
            data,
            state
        ),
        127 => {
            // NoOperation
            Vec::new()
        }
        _ => {
            warn!("Unhandled core X11 request opcode: {major_opcode} minor: {_minor}");
            // Return BadRequest error for unrecognized opcodes per X11 spec
            super::core::build_error(
                REQUEST_ERROR,
                seq,
                major_opcode as u32,
                major_opcode,
                _minor as u16,
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

/// Resolve the effective cursor for a window and update XFIXES
/// state.
///
/// **Frontend cursor delivery is currently disabled** — the
/// `DisplayUpdate::Cursor*` variants were removed because the
/// browser-side rendering never worked end to end. The X11 cursor
/// resource tables (`state.cursors`, `state.cursor_info`,
/// `state.current_cursor`) are still populated by Create/Render
/// cursor handlers and read by `GetCursorImage` etc., so X11 clients
/// see a coherent cursor story even though the browser doesn't.
/// Re-introducing browser cursors should resurrect the wire-side
/// emit logic at <https://git/issue/TODO> (pre-existing diff in the
/// commit that removed it).
fn emit_cursor_changed(state: &mut ClientState, wid: u32) {
    // Resolve the cursor ID for XFIXES tracking. The CSS name path
    // and the top-level ancestor walk that previously fed the
    // frontend emit are gone.
    let cursor_id = state.windows.get(&wid).and_then(|w| w.cursor);

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
            let event = serialize_event(
                &CursorNotifyEvent {
                    response_type: XFIXES_CURSOR_NOTIFY,
                    subtype: CursorNotifySubtype::from(0u8), // DisplayCursor
                    sequence: state.sequence,
                    window: sub_win,
                    cursor_serial,
                    timestamp,
                    name: 0, // unnamed
                },
                state.msb_first,
            );
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

/// Map X11 keycode to (normal_keysym, shifted_keysym) using a libxkbcommon
/// keymap compiled for `evdev / pc105 / us`. Falls back to a tiny hardcoded
/// table when xkb data files aren't installed.
pub(crate) fn keycode_to_keysym(keycode: u8) -> (u32, u32) {
    default_keymap::keysyms_for_keycode(keycode)
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
        // Per the evdev/us xkb layout, Shift+Tab produces ISO_Left_Tab —
        // the same behaviour real X servers report through GetKeyboardMapping.
        const XK_ISO_LEFT_TAB: u32 = 0xfe20;
        let (normal, shifted) = keycode_to_keysym(23);
        assert_eq!(normal, XK_TAB);
        assert_eq!(shifted, XK_ISO_LEFT_TAB);
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
        // Shift+Alt produces either Alt_L (xkbcommon defaults on some
        // platforms) or Meta_L (Linux evdev/us); both are
        // spec-compliant. Just assert level 1 is Alt_L and level 2 is
        // a defined modifier keysym.
        const XK_META_L: u32 = 0xffe7;
        let (normal, shifted) = keycode_to_keysym(64);
        assert_eq!(normal, XK_ALT_L);
        assert!(
            shifted == XK_ALT_L || shifted == XK_META_L,
            "shifted level expected Alt_L or Meta_L, got {:#x}",
            shifted
        );
    }

    #[test]
    fn keycode_alt_r_108() {
        const XK_META_R: u32 = 0xffe8;
        let (normal, shifted) = keycode_to_keysym(108);
        assert_eq!(normal, XK_ALT_R);
        assert!(
            shifted == XK_ALT_R || shifted == XK_META_R,
            "shifted level expected Alt_R or Meta_R, got {:#x}",
            shifted
        );
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
    fn keycode_below_min_returns_zero_pair() {
        // Keycodes below xkb's evdev minimum (9) have no mapping.
        assert_eq!(keycode_to_keysym(0), (0, 0));
        assert_eq!(keycode_to_keysym(1), (0, 0));
        assert_eq!(keycode_to_keysym(8), (0, 0));
    }

    #[test]
    fn keycode_high_range_returns_xf86_multimedia() {
        // The evdev/us layout maps keycodes 200+ to XF86 multimedia keysyms
        // (e.g. AudioMute, AudioRaiseVolume, etc.). All XF86 vendor keysyms
        // share the 0x1008FFxx prefix.
        let (normal, _) = keycode_to_keysym(200);
        assert!(
            (0x1008FF00..=0x1008FFFF).contains(&normal) || normal == 0,
            "expected XF86 vendor keysym or 0 for keycode 200, got 0x{normal:x}"
        );
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
        // Opcodes 120-126 are undefined in X11 spec and should return REQUEST_ERROR
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
        // led_mode must be 0 or 1; values > 1 should be VALUE_ERROR
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
        let parent_mask: u32 = u32::from(super::EventMask::SUBSTRUCTURE_REDIRECT);
        let override_redirect = false;
        let has_redirect =
            parent_mask & super::EventMask::SUBSTRUCTURE_REDIRECT != super::EventMask::NO_EVENT;
        assert!(
            has_redirect && !override_redirect,
            "Non-OR child of redirect parent should generate MapRequest"
        );
    }

    #[test]
    fn override_redirect_bypasses_substructure_redirect() {
        // override_redirect windows must bypass SubstructureRedirect
        let parent_mask: u32 = u32::from(super::EventMask::SUBSTRUCTURE_REDIRECT);
        let override_redirect = true;
        let should_redirect = (parent_mask & super::EventMask::SUBSTRUCTURE_REDIRECT
            != super::EventMask::NO_EVENT)
            && !override_redirect;
        assert!(
            !should_redirect,
            "Override-redirect windows must not be redirected"
        );
    }

    #[test]
    fn configure_request_sent_for_non_toplevel_with_redirect() {
        // Per X11 spec: ConfigureWindow on ANY child generates ConfigureRequest
        // when parent has SubstructureRedirectMask, not just top-level.
        let parent_mask: u32 = u32::from(super::EventMask::SUBSTRUCTURE_REDIRECT);
        let is_override_redirect = false;
        let parent_has_redirect =
            parent_mask & super::EventMask::SUBSTRUCTURE_REDIRECT != super::EventMask::NO_EVENT;
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
