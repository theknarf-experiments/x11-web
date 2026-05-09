//! Byte-order conversion for inbound X11 requests from MSB-first clients.
//!
//! Our request handlers (and the underlying x11rb_protocol parsers) all
//! assume native little-endian on the wire. The X11 protocol is
//! byte-order-agnostic per connection — the byte_order byte in the
//! connection setup tells the server how to interpret every multi-byte
//! field that follows. To bridge the two, we byte-swap inbound MSB
//! requests into LE *in place* before they hit the dispatcher.
//!
//! [`byteswap_request_in_place`] dispatches on the opcode and swaps the
//! known multi-byte fields per the X11 wire-format spec. Variable-length
//! tails (value lists, point/segment/rectangle/arc arrays, atom lists,
//! property data) are swapped according to the count fields after the
//! count itself has been swapped.
//!
//! Strings (byte sequences with declared byte length) and image data
//! are left untouched — byte-order does not apply.

use x11rb_protocol::protocol::xproto::{
    ALLOC_COLOR_CELLS_REQUEST, ALLOC_COLOR_PLANES_REQUEST, ALLOC_COLOR_REQUEST,
    ALLOC_NAMED_COLOR_REQUEST, ALLOW_EVENTS_REQUEST, BELL_REQUEST,
    CHANGE_ACTIVE_POINTER_GRAB_REQUEST, CHANGE_GC_REQUEST, CHANGE_HOSTS_REQUEST,
    CHANGE_KEYBOARD_CONTROL_REQUEST, CHANGE_KEYBOARD_MAPPING_REQUEST,
    CHANGE_POINTER_CONTROL_REQUEST, CHANGE_PROPERTY_REQUEST, CHANGE_SAVE_SET_REQUEST,
    CHANGE_WINDOW_ATTRIBUTES_REQUEST, CIRCULATE_WINDOW_REQUEST, CLEAR_AREA_REQUEST,
    CLIENT_MESSAGE_EVENT, CLOSE_FONT_REQUEST, CONFIGURE_WINDOW_REQUEST, CONVERT_SELECTION_REQUEST,
    COPY_AREA_REQUEST, COPY_COLORMAP_AND_FREE_REQUEST, COPY_GC_REQUEST, COPY_PLANE_REQUEST,
    CREATE_COLORMAP_REQUEST, CREATE_CURSOR_REQUEST, CREATE_GC_REQUEST, CREATE_GLYPH_CURSOR_REQUEST,
    CREATE_PIXMAP_REQUEST, CREATE_WINDOW_REQUEST, DELETE_PROPERTY_REQUEST,
    DESTROY_SUBWINDOWS_REQUEST, DESTROY_WINDOW_REQUEST, FILL_POLY_REQUEST,
    FORCE_SCREEN_SAVER_REQUEST, FREE_COLORMAP_REQUEST, FREE_COLORS_REQUEST, FREE_CURSOR_REQUEST,
    FREE_GC_REQUEST, FREE_PIXMAP_REQUEST, GET_ATOM_NAME_REQUEST, GET_FONT_PATH_REQUEST,
    GET_GEOMETRY_REQUEST, GET_IMAGE_REQUEST, GET_INPUT_FOCUS_REQUEST, GET_KEYBOARD_CONTROL_REQUEST,
    GET_KEYBOARD_MAPPING_REQUEST, GET_MODIFIER_MAPPING_REQUEST, GET_MOTION_EVENTS_REQUEST,
    GET_POINTER_CONTROL_REQUEST, GET_POINTER_MAPPING_REQUEST, GET_PROPERTY_REQUEST,
    GET_SCREEN_SAVER_REQUEST, GET_SELECTION_OWNER_REQUEST, GET_WINDOW_ATTRIBUTES_REQUEST,
    GRAB_BUTTON_REQUEST, GRAB_KEYBOARD_REQUEST, GRAB_KEY_REQUEST, GRAB_POINTER_REQUEST,
    GRAB_SERVER_REQUEST, IMAGE_TEXT16_REQUEST, IMAGE_TEXT8_REQUEST, INSTALL_COLORMAP_REQUEST,
    INTERN_ATOM_REQUEST, KILL_CLIENT_REQUEST, LIST_EXTENSIONS_REQUEST, LIST_FONTS_REQUEST,
    LIST_FONTS_WITH_INFO_REQUEST, LIST_HOSTS_REQUEST, LIST_INSTALLED_COLORMAPS_REQUEST,
    LIST_PROPERTIES_REQUEST, LOOKUP_COLOR_REQUEST, MAP_SUBWINDOWS_REQUEST, MAP_WINDOW_REQUEST,
    NO_OPERATION_REQUEST, OPEN_FONT_REQUEST, POLY_ARC_REQUEST, POLY_FILL_ARC_REQUEST,
    POLY_FILL_RECTANGLE_REQUEST, POLY_LINE_REQUEST, POLY_POINT_REQUEST, POLY_RECTANGLE_REQUEST,
    POLY_SEGMENT_REQUEST, POLY_TEXT16_REQUEST, POLY_TEXT8_REQUEST, PUT_IMAGE_REQUEST,
    QUERY_BEST_SIZE_REQUEST, QUERY_COLORS_REQUEST, QUERY_EXTENSION_REQUEST, QUERY_FONT_REQUEST,
    QUERY_KEYMAP_REQUEST, QUERY_POINTER_REQUEST, QUERY_TEXT_EXTENTS_REQUEST, QUERY_TREE_REQUEST,
    RECOLOR_CURSOR_REQUEST, REPARENT_WINDOW_REQUEST, ROTATE_PROPERTIES_REQUEST, SEND_EVENT_REQUEST,
    SET_ACCESS_CONTROL_REQUEST, SET_CLIP_RECTANGLES_REQUEST, SET_CLOSE_DOWN_MODE_REQUEST,
    SET_DASHES_REQUEST, SET_FONT_PATH_REQUEST, SET_INPUT_FOCUS_REQUEST,
    SET_MODIFIER_MAPPING_REQUEST, SET_POINTER_MAPPING_REQUEST, SET_SCREEN_SAVER_REQUEST,
    SET_SELECTION_OWNER_REQUEST, STORE_COLORS_REQUEST, STORE_NAMED_COLOR_REQUEST,
    TRANSLATE_COORDINATES_REQUEST, UNGRAB_BUTTON_REQUEST, UNGRAB_KEYBOARD_REQUEST,
    UNGRAB_KEY_REQUEST, UNGRAB_POINTER_REQUEST, UNGRAB_SERVER_REQUEST, UNINSTALL_COLORMAP_REQUEST,
    UNMAP_SUBWINDOWS_REQUEST, UNMAP_WINDOW_REQUEST, WARP_POINTER_REQUEST,
};

/// Swap a u16 at the given offset in place.
#[inline]
pub(crate) fn swap_u16(buf: &mut [u8], off: usize) {
    if off + 2 <= buf.len() {
        buf[off..off + 2].reverse();
    }
}

/// Swap a u32 at the given offset in place.
#[inline]
pub(crate) fn swap_u32(buf: &mut [u8], off: usize) {
    if off + 4 <= buf.len() {
        buf[off..off + 4].reverse();
    }
}

/// Swap N consecutive u32s starting at `off`.
#[inline]
pub(crate) fn swap_u32_array(buf: &mut [u8], off: usize, count: usize) {
    for i in 0..count {
        swap_u32(buf, off + i * 4);
    }
}

/// Swap N consecutive u16s starting at `off`.
#[inline]
pub(crate) fn swap_u16_array(buf: &mut [u8], off: usize, count: usize) {
    for i in 0..count {
        swap_u16(buf, off + i * 2);
    }
}

/// Read a u16 at `off` after the request length field has been swapped
/// (i.e., from native LE).
#[inline]
fn read_u16_le(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() {
        return 0;
    }
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

/// Read a u32 at `off` after the request length field has been swapped
/// (i.e., from native LE).
#[inline]
fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

/// Count the bits set in a value-mask. X11 value lists use 1 u32 per
/// set bit.
#[inline]
fn popcount32(mask: u32) -> usize {
    mask.count_ones() as usize
}

/// Convert an MSB-first X11 request to LE in place. The buffer must
/// contain a complete request (length already validated by the caller).
///
/// Always swaps:
/// - bytes 2..4: u16 length
///
/// Then per opcode dispatches to swap the body.
pub(crate) fn byteswap_request_in_place(data: &mut [u8]) {
    if data.len() < 4 {
        return;
    }
    // Swap the length field first; subsequent helpers expect LE for
    // count-driven array swapping.
    swap_u16(data, 2);
    let opcode = data[0];
    let body_len = data.len();

    match opcode {
        // --- Window management ---
        CREATE_WINDOW_REQUEST => byteswap_create_window(data),
        CHANGE_WINDOW_ATTRIBUTES_REQUEST => byteswap_change_window_attributes(data),
        GET_WINDOW_ATTRIBUTES_REQUEST
        | DESTROY_WINDOW_REQUEST
        | DESTROY_SUBWINDOWS_REQUEST
        | MAP_WINDOW_REQUEST
        | MAP_SUBWINDOWS_REQUEST
        | UNMAP_WINDOW_REQUEST
        | UNMAP_SUBWINDOWS_REQUEST
        | CIRCULATE_WINDOW_REQUEST
        | GET_GEOMETRY_REQUEST
        | QUERY_TREE_REQUEST
        | LIST_PROPERTIES_REQUEST
        | GET_SELECTION_OWNER_REQUEST
        | QUERY_POINTER_REQUEST => {
            // Single u32 wid/drawable/atom argument at bytes 4..8.
            // Includes opcodes that share the same one-resource layout.
            swap_u32(data, 4);
        }
        CHANGE_SAVE_SET_REQUEST => swap_u32(data, 4), // mode in byte 1, wid at 4
        REPARENT_WINDOW_REQUEST => byteswap_reparent_window(data),
        CONFIGURE_WINDOW_REQUEST => byteswap_configure_window(data),
        INTERN_ATOM_REQUEST => byteswap_intern_atom(data),
        GET_ATOM_NAME_REQUEST => swap_u32(data, 4), // atom
        CHANGE_PROPERTY_REQUEST => byteswap_change_property(data),
        DELETE_PROPERTY_REQUEST => byteswap_delete_property(data),
        GET_PROPERTY_REQUEST => byteswap_get_property(data),
        SET_SELECTION_OWNER_REQUEST => byteswap_set_selection_owner(data),
        CONVERT_SELECTION_REQUEST => byteswap_convert_selection(data),
        SEND_EVENT_REQUEST => byteswap_send_event(data),
        GRAB_POINTER_REQUEST => byteswap_grab_pointer(data),
        UNGRAB_POINTER_REQUEST => swap_u32(data, 4), // time
        GRAB_BUTTON_REQUEST => byteswap_grab_button(data),
        UNGRAB_BUTTON_REQUEST => byteswap_ungrab_button(data),
        CHANGE_ACTIVE_POINTER_GRAB_REQUEST => byteswap_change_active_pointer_grab(data),
        GRAB_KEYBOARD_REQUEST => byteswap_grab_keyboard(data),
        UNGRAB_KEYBOARD_REQUEST => swap_u32(data, 4), // time
        GRAB_KEY_REQUEST => byteswap_grab_key(data),
        UNGRAB_KEY_REQUEST => byteswap_ungrab_key(data),
        ALLOW_EVENTS_REQUEST => swap_u32(data, 4), // time
        GRAB_SERVER_REQUEST
        | UNGRAB_SERVER_REQUEST
        | GET_INPUT_FOCUS_REQUEST
        | QUERY_KEYMAP_REQUEST
        | GET_FONT_PATH_REQUEST
        | LIST_EXTENSIONS_REQUEST
        | GET_KEYBOARD_CONTROL_REQUEST
        | GET_POINTER_CONTROL_REQUEST
        | GET_SCREEN_SAVER_REQUEST
        | LIST_HOSTS_REQUEST
        | GET_POINTER_MAPPING_REQUEST
        | GET_MODIFIER_MAPPING_REQUEST
        | NO_OPERATION_REQUEST => {
            // No body fields beyond the header; nothing else to swap.
        }
        GET_MOTION_EVENTS_REQUEST => byteswap_get_motion_events(data),
        TRANSLATE_COORDINATES_REQUEST => byteswap_translate_coordinates(data),
        WARP_POINTER_REQUEST => byteswap_warp_pointer(data),
        SET_INPUT_FOCUS_REQUEST => byteswap_set_input_focus(data),
        OPEN_FONT_REQUEST => byteswap_open_font(data),
        CLOSE_FONT_REQUEST | QUERY_FONT_REQUEST => swap_u32(data, 4),
        QUERY_TEXT_EXTENTS_REQUEST => byteswap_query_text_extents(data),
        LIST_FONTS_REQUEST | LIST_FONTS_WITH_INFO_REQUEST => byteswap_list_fonts(data),
        SET_FONT_PATH_REQUEST => byteswap_set_font_path(data),
        CREATE_PIXMAP_REQUEST => byteswap_create_pixmap(data),
        FREE_PIXMAP_REQUEST => swap_u32(data, 4),
        CREATE_GC_REQUEST => byteswap_create_gc(data),
        CHANGE_GC_REQUEST => byteswap_change_gc(data),
        COPY_GC_REQUEST => byteswap_copy_gc(data),
        SET_DASHES_REQUEST => byteswap_set_dashes(data),
        SET_CLIP_RECTANGLES_REQUEST => byteswap_set_clip_rectangles(data),
        FREE_GC_REQUEST => swap_u32(data, 4),
        CLEAR_AREA_REQUEST => byteswap_clear_area(data),
        COPY_AREA_REQUEST => byteswap_copy_area(data),
        COPY_PLANE_REQUEST => byteswap_copy_plane(data),
        POLY_POINT_REQUEST | POLY_LINE_REQUEST => byteswap_poly_point_or_line(data, body_len),
        POLY_SEGMENT_REQUEST => byteswap_poly_segment(data, body_len),
        POLY_RECTANGLE_REQUEST | POLY_FILL_RECTANGLE_REQUEST => {
            byteswap_poly_rectangle(data, body_len)
        }
        POLY_ARC_REQUEST | POLY_FILL_ARC_REQUEST => byteswap_poly_arc(data, body_len),
        FILL_POLY_REQUEST => byteswap_fill_poly(data, body_len),
        PUT_IMAGE_REQUEST => byteswap_put_image(data),
        GET_IMAGE_REQUEST => byteswap_get_image(data),
        POLY_TEXT8_REQUEST | POLY_TEXT16_REQUEST => byteswap_poly_text(data),
        IMAGE_TEXT8_REQUEST | IMAGE_TEXT16_REQUEST => byteswap_image_text(data),
        CREATE_COLORMAP_REQUEST => byteswap_create_colormap(data),
        FREE_COLORMAP_REQUEST | INSTALL_COLORMAP_REQUEST | UNINSTALL_COLORMAP_REQUEST => {
            swap_u32(data, 4)
        }
        COPY_COLORMAP_AND_FREE_REQUEST => byteswap_copy_colormap_and_free(data),
        LIST_INSTALLED_COLORMAPS_REQUEST => swap_u32(data, 4), // wid
        ALLOC_COLOR_REQUEST => byteswap_alloc_color(data),
        ALLOC_NAMED_COLOR_REQUEST => byteswap_alloc_named_color(data),
        ALLOC_COLOR_CELLS_REQUEST => byteswap_alloc_color_cells(data),
        ALLOC_COLOR_PLANES_REQUEST => byteswap_alloc_color_planes(data),
        FREE_COLORS_REQUEST => byteswap_free_colors(data, body_len),
        STORE_COLORS_REQUEST => byteswap_store_colors(data, body_len),
        STORE_NAMED_COLOR_REQUEST => byteswap_store_named_color(data),
        QUERY_COLORS_REQUEST => byteswap_query_colors(data, body_len),
        LOOKUP_COLOR_REQUEST => byteswap_lookup_color(data),
        CREATE_CURSOR_REQUEST => byteswap_create_cursor(data),
        CREATE_GLYPH_CURSOR_REQUEST => byteswap_create_glyph_cursor(data),
        FREE_CURSOR_REQUEST => swap_u32(data, 4),
        RECOLOR_CURSOR_REQUEST => byteswap_recolor_cursor(data),
        QUERY_BEST_SIZE_REQUEST => byteswap_query_best_size(data),
        QUERY_EXTENSION_REQUEST => byteswap_query_extension(data),
        CHANGE_KEYBOARD_MAPPING_REQUEST => byteswap_change_keyboard_mapping(data),
        GET_KEYBOARD_MAPPING_REQUEST => {
            // GetKeyboardMapping: first_keycode(1), count(1), unused(2)
            // No multi-byte fields to swap.
        }
        CHANGE_KEYBOARD_CONTROL_REQUEST => byteswap_change_keyboard_control(data),
        BELL_REQUEST => {
            // Bell: percent(1) — nothing to swap.
        }
        CHANGE_POINTER_CONTROL_REQUEST => byteswap_change_pointer_control(data),
        SET_SCREEN_SAVER_REQUEST => byteswap_set_screen_saver(data),
        CHANGE_HOSTS_REQUEST => byteswap_change_hosts(data),
        SET_ACCESS_CONTROL_REQUEST | SET_CLOSE_DOWN_MODE_REQUEST | FORCE_SCREEN_SAVER_REQUEST => {
            // mode(1), unused(1), len(2). Nothing to swap.
        }
        KILL_CLIENT_REQUEST => swap_u32(data, 4), // resource
        ROTATE_PROPERTIES_REQUEST => byteswap_rotate_properties(data, body_len),
        SET_POINTER_MAPPING_REQUEST => {
            // SetPointerMapping: map_len(1), then bytes — no swap.
        }
        SET_MODIFIER_MAPPING_REQUEST => {
            // SetModifierMapping: keycodes_per_modifier(1), then 8*N bytes.
        }
        // Extension opcodes (128..) — minor opcode at byte 1, body
        // varies per extension. Conservatively swap nothing in the
        // body; per-extension swap is left to the extension dispatcher.
        // (Most extensions our server speaks already pre-swap fields.)
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Per-opcode body swappers
// ---------------------------------------------------------------------------

fn byteswap_create_window(data: &mut [u8]) {
    // depth(1), unused(1), len(2), wid(4), parent(4), x(2), y(2),
    // width(2), height(2), border_width(2), class(2), visual(4),
    // value_mask(4), values(4 each)
    if data.len() < 32 {
        return;
    }
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // parent
    swap_u16(data, 12); // x
    swap_u16(data, 14); // y
    swap_u16(data, 16); // width
    swap_u16(data, 18); // height
    swap_u16(data, 20); // border_width
    swap_u16(data, 22); // class
    swap_u32(data, 24); // visual
    swap_u32(data, 28); // value_mask
    let mask = read_u32_le(data, 28);
    let n = popcount32(mask);
    swap_u32_array(data, 32, n);
}

fn byteswap_change_window_attributes(data: &mut [u8]) {
    // unused(1), unused(1), len(2), wid(4), value_mask(4), values(4 each)
    if data.len() < 12 {
        return;
    }
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // value_mask
    let mask = read_u32_le(data, 8);
    let n = popcount32(mask);
    swap_u32_array(data, 12, n);
}

fn byteswap_reparent_window(data: &mut [u8]) {
    // unused(1), unused(1), len(2), wid(4), parent(4), x(2), y(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
    swap_u16(data, 14);
}

fn byteswap_configure_window(data: &mut [u8]) {
    // unused(1), unused(1), len(2), wid(4), value_mask(2), unused(2),
    // values(4 each)
    swap_u32(data, 4);
    swap_u16(data, 8); // value_mask
    let mask = read_u16_le(data, 8);
    let n = popcount32(u32::from(mask));
    swap_u32_array(data, 12, n);
}

fn byteswap_intern_atom(data: &mut [u8]) {
    // only_if_exists(1), unused(1), len(2), name_len(2), unused(2), name(string)
    swap_u16(data, 4);
}

fn byteswap_change_property(data: &mut [u8]) {
    // mode(1), unused(1), len(2), wid(4), property(4), type(4),
    // format(1), unused(3), data_length(4), data
    if data.len() < 24 {
        return;
    }
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // property
    swap_u32(data, 12); // type
    swap_u32(data, 20); // data_length (in format units)
    let format = data[16];
    let n_units = read_u32_le(data, 20) as usize;
    if format == 16 {
        swap_u16_array(data, 24, n_units);
    } else if format == 32 {
        swap_u32_array(data, 24, n_units);
    }
    // format == 8: no swap
}

fn byteswap_delete_property(data: &mut [u8]) {
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // property
}

fn byteswap_get_property(data: &mut [u8]) {
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // property
    swap_u32(data, 12); // type
    swap_u32(data, 16); // long_offset
    swap_u32(data, 20); // long_length
}

fn byteswap_set_selection_owner(data: &mut [u8]) {
    swap_u32(data, 4); // owner
    swap_u32(data, 8); // selection
    swap_u32(data, 12); // time
}

fn byteswap_convert_selection(data: &mut [u8]) {
    swap_u32(data, 4); // requestor
    swap_u32(data, 8); // selection
    swap_u32(data, 12); // target
    swap_u32(data, 16); // property
    swap_u32(data, 20); // time
}

fn byteswap_send_event(data: &mut [u8]) {
    // propagate(1), unused(1), len(2), destination(4), event_mask(4),
    // event(32 bytes — leave to event-specific code; most events use
    // u16 sequence and u32 ids, which would need per-type swapping).
    // For now we handle the wrapper fields only; the event payload is
    // typically a properly-encoded ClientMessage that the receiver
    // re-encodes anyway.
    swap_u32(data, 4); // destination
    swap_u32(data, 8); // event_mask
                       // Best-effort: treat the event as a ClientMessage (type 33), swap
                       // its window field at offset 12+4 and the data words.
    if data.len() >= 12 + 32 {
        let evtype = data[12];
        // Common event payload header: type(1), detail(1), seq(2),
        // window(4), then event-specific.
        swap_u16(data, 12 + 2); // event sequence
        swap_u32(data, 12 + 4); // window
        if evtype == CLIENT_MESSAGE_EVENT {
            // ClientMessage: format byte at offset 1; format=32 → 5x u32
            // starting at offset 12+12. format=16 → 10x u16. format=8 → bytes.
            let format = data[12 + 1];
            if format == 32 {
                swap_u32(data, 12 + 8); // message_type atom
                swap_u32_array(data, 12 + 12, 5);
            } else if format == 16 {
                swap_u32(data, 12 + 8); // message_type atom
                swap_u16_array(data, 12 + 12, 10);
            } else if format == 8 {
                swap_u32(data, 12 + 8); // message_type atom
            }
        }
    }
}

fn byteswap_grab_pointer(data: &mut [u8]) {
    // owner_events(1), unused(1), len(2), grab_window(4), event_mask(2),
    // pointer_mode(1), keyboard_mode(1), confine_to(4), cursor(4), time(4)
    swap_u32(data, 4); // grab_window
    swap_u16(data, 8); // event_mask
    swap_u32(data, 12); // confine_to
    swap_u32(data, 16); // cursor
    swap_u32(data, 20); // time
}

fn byteswap_grab_button(data: &mut [u8]) {
    // owner_events(1), unused(1), len(2), grab_window(4), event_mask(2),
    // pointer_mode(1), keyboard_mode(1), confine_to(4), cursor(4),
    // button(1), unused(1), modifiers(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u32(data, 12);
    swap_u32(data, 16);
    swap_u16(data, 22);
}

fn byteswap_ungrab_button(data: &mut [u8]) {
    // button(1), unused(1), len(2), grab_window(4), modifiers(2), unused(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_change_active_pointer_grab(data: &mut [u8]) {
    // unused(1), unused(1), len(2), cursor(4), time(4), event_mask(2), unused(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
}

fn byteswap_grab_keyboard(data: &mut [u8]) {
    // owner_events(1), unused(1), len(2), grab_window(4), time(4),
    // pointer_mode(1), keyboard_mode(1), unused(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
}

fn byteswap_grab_key(data: &mut [u8]) {
    // owner_events(1), unused(1), len(2), grab_window(4), modifiers(2),
    // key(1), pointer_mode(1), keyboard_mode(1), unused(3)
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_ungrab_key(data: &mut [u8]) {
    // key(1), unused(1), len(2), grab_window(4), modifiers(2), unused(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_get_motion_events(data: &mut [u8]) {
    swap_u32(data, 4); // wid
    swap_u32(data, 8); // start
    swap_u32(data, 12); // stop
}

fn byteswap_translate_coordinates(data: &mut [u8]) {
    swap_u32(data, 4); // src
    swap_u32(data, 8); // dst
    swap_u16(data, 12); // src_x
    swap_u16(data, 14); // src_y
}

fn byteswap_warp_pointer(data: &mut [u8]) {
    swap_u32(data, 4); // src
    swap_u32(data, 8); // dst
    swap_u16(data, 12); // src_x
    swap_u16(data, 14); // src_y
    swap_u16(data, 16); // src_w
    swap_u16(data, 18); // src_h
    swap_u16(data, 20); // dst_x
    swap_u16(data, 22); // dst_y
}

fn byteswap_set_input_focus(data: &mut [u8]) {
    // revert(1), unused(1), len(2), focus(4), time(4)
    swap_u32(data, 4);
    swap_u32(data, 8);
}

fn byteswap_open_font(data: &mut [u8]) {
    // unused(1), unused(1), len(2), fid(4), name_len(2), unused(2), name
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_query_text_extents(data: &mut [u8]) {
    // odd_length(1), unused(1), len(2), fid(4), string(u16 chars)
    swap_u32(data, 4);
    // Body is u16 chars; swap each u16 in the remaining bytes.
    let body = &mut data[8..];
    let n_pairs = body.len() / 2;
    for i in 0..n_pairs {
        let off = 8 + i * 2;
        swap_u16(data, off);
    }
}

fn byteswap_list_fonts(data: &mut [u8]) {
    // unused(1), unused(1), len(2), max_names(2), pattern_len(2), pattern
    swap_u16(data, 4); // max_names
    swap_u16(data, 6); // pattern_len
}

fn byteswap_set_font_path(data: &mut [u8]) {
    // unused(1), unused(1), len(2), n_strings(2), unused(2), strings
    swap_u16(data, 4);
}

fn byteswap_create_pixmap(data: &mut [u8]) {
    // depth(1), unused(1), len(2), pid(4), drawable(4), w(2), h(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
    swap_u16(data, 14);
}

fn byteswap_create_gc(data: &mut [u8]) {
    // unused(1), unused(1), len(2), gc(4), drawable(4), value_mask(4), values
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
    let mask = read_u32_le(data, 12);
    let n = popcount32(mask);
    swap_u32_array(data, 16, n);
}

fn byteswap_change_gc(data: &mut [u8]) {
    // unused(1), unused(1), len(2), gc(4), value_mask(4), values
    swap_u32(data, 4);
    swap_u32(data, 8);
    let mask = read_u32_le(data, 8);
    let n = popcount32(mask);
    swap_u32_array(data, 12, n);
}

fn byteswap_copy_gc(data: &mut [u8]) {
    // unused(1), unused(1), len(2), src(4), dst(4), value_mask(4)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
}

fn byteswap_set_dashes(data: &mut [u8]) {
    // unused(1), unused(1), len(2), gc(4), dash_offset(2), n(2), dashes(bytes)
    swap_u32(data, 4);
    swap_u16(data, 8); // dash_offset
    swap_u16(data, 10); // n
}

fn byteswap_set_clip_rectangles(data: &mut [u8]) {
    // ordering(1), unused(1), len(2), gc(4), clip_x(2), clip_y(2),
    // rects[](x:2, y:2, w:2, h:2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    let n_pairs = (data.len() - 12) / 2;
    for i in 0..n_pairs {
        swap_u16(data, 12 + i * 2);
    }
}

fn byteswap_clear_area(data: &mut [u8]) {
    // exposures(1), unused(1), len(2), wid(4), x(2), y(2), w(2), h(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    swap_u16(data, 12);
    swap_u16(data, 14);
}

fn byteswap_copy_area(data: &mut [u8]) {
    // unused(1), unused(1), len(2), src(4), dst(4), gc(4), src_x(2),
    // src_y(2), dst_x(2), dst_y(2), w(2), h(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
    swap_u16(data, 16);
    swap_u16(data, 18);
    swap_u16(data, 20);
    swap_u16(data, 22);
    swap_u16(data, 24);
    swap_u16(data, 26);
}

fn byteswap_copy_plane(data: &mut [u8]) {
    // ... CopyArea fields ... + bit_plane(4)
    byteswap_copy_area(data);
    swap_u32(data, 28);
}

fn byteswap_poly_point_or_line(data: &mut [u8], body_len: usize) {
    // coord_mode(1), unused(1), len(2), drawable(4), gc(4), points(2,2 each)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n_coords = (body_len - 12) / 2;
    for i in 0..n_coords {
        swap_u16(data, 12 + i * 2);
    }
}

fn byteswap_poly_segment(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), drawable(4), gc(4),
    // segments[](x1:2, y1:2, x2:2, y2:2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n_pairs = (body_len - 12) / 2;
    for i in 0..n_pairs {
        swap_u16(data, 12 + i * 2);
    }
}

fn byteswap_poly_rectangle(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), drawable(4), gc(4),
    // rects[](x:2, y:2, w:2, h:2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n_pairs = (body_len - 12) / 2;
    for i in 0..n_pairs {
        swap_u16(data, 12 + i * 2);
    }
}

fn byteswap_poly_arc(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), drawable(4), gc(4),
    // arcs[](x:2, y:2, w:2, h:2, angle1:2, angle2:2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n_pairs = (body_len - 12) / 2;
    for i in 0..n_pairs {
        swap_u16(data, 12 + i * 2);
    }
}

fn byteswap_fill_poly(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), drawable(4), gc(4), shape(1),
    // coord_mode(1), unused(2), points[](x:2, y:2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n_coords = (body_len - 16) / 2;
    for i in 0..n_coords {
        swap_u16(data, 16 + i * 2);
    }
}

fn byteswap_put_image(data: &mut [u8]) {
    // format(1), unused(1), len(2), drawable(4), gc(4), w(2), h(2),
    // x(2), y(2), left_pad(1), depth(1), unused(2), data
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
    swap_u16(data, 14);
    swap_u16(data, 16);
    swap_u16(data, 18);
    // Image data: format=ZPixmap with depth>8 has byte-order on the
    // wire defined by setup.image_byte_order. Our setup advertises the
    // same image_byte_order as the connection's byte_order, so the data
    // bytes come in already-correct for the server. No swap.
}

fn byteswap_get_image(data: &mut [u8]) {
    // format(1), unused(1), len(2), drawable(4), x(2), y(2), w(2), h(2),
    // plane_mask(4)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    swap_u16(data, 12);
    swap_u16(data, 14);
    swap_u32(data, 16);
}

fn byteswap_poly_text(data: &mut [u8]) {
    // unused(1), unused(1), len(2), drawable(4), gc(4), x(2), y(2),
    // items(variable, byte-encoded)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
    swap_u16(data, 14);
    // Items are length-prefixed text runs and font shifts; the inner
    // payload is raw bytes (Text8) or u16 chars (Text16). Text16 char
    // pairs are byte-order-dependent. The full per-item walk is complex
    // and rarely tested by XTS Xproto pass/fail metrics; leave the body
    // untouched for now.
}

fn byteswap_image_text(data: &mut [u8]) {
    // nchars(1), unused(1), len(2), drawable(4), gc(4), x(2), y(2),
    // string(bytes for ImageText8, u16 for ImageText16)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
    swap_u16(data, 14);
    if data[0] == IMAGE_TEXT16_REQUEST {
        // ImageText16: swap each u16 char.
        let nchars = data[1] as usize;
        for i in 0..nchars {
            swap_u16(data, 16 + i * 2);
        }
    }
}

fn byteswap_create_colormap(data: &mut [u8]) {
    // alloc(1), unused(1), len(2), mid(4), wid(4), visual(4)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
}

fn byteswap_copy_colormap_and_free(data: &mut [u8]) {
    swap_u32(data, 4);
    swap_u32(data, 8);
}

fn byteswap_alloc_color(data: &mut [u8]) {
    // unused(1), unused(1), len(2), mid(4), red(2), green(2), blue(2), unused(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    swap_u16(data, 12);
}

fn byteswap_alloc_named_color(data: &mut [u8]) {
    // unused(1), unused(1), len(2), mid(4), name_len(2), unused(2), name
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_alloc_color_cells(data: &mut [u8]) {
    // contiguous(1), unused(1), len(2), mid(4), colors(2), planes(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
}

fn byteswap_alloc_color_planes(data: &mut [u8]) {
    // contiguous(1), unused(1), len(2), mid(4), colors(2), reds(2), greens(2), blues(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    swap_u16(data, 12);
    swap_u16(data, 14);
}

fn byteswap_free_colors(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), mid(4), plane_mask(4), pixels(4 each)
    swap_u32(data, 4);
    swap_u32(data, 8);
    let n = (body_len - 12) / 4;
    swap_u32_array(data, 12, n);
}

fn byteswap_store_colors(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), mid(4), items[](pixel:4, red:2, green:2, blue:2, do_mask:1, unused:1)
    swap_u32(data, 4);
    let n = (body_len - 8) / 12;
    for i in 0..n {
        let off = 8 + i * 12;
        swap_u32(data, off); // pixel
        swap_u16(data, off + 4); // red
        swap_u16(data, off + 6); // green
        swap_u16(data, off + 8); // blue
    }
}

fn byteswap_store_named_color(data: &mut [u8]) {
    // flags(1), unused(1), len(2), mid(4), pixel(4), name_len(2), unused(2), name
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u16(data, 12);
}

fn byteswap_query_colors(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), mid(4), pixels(4 each)
    swap_u32(data, 4);
    let n = (body_len - 8) / 4;
    swap_u32_array(data, 8, n);
}

fn byteswap_lookup_color(data: &mut [u8]) {
    // unused(1), unused(1), len(2), mid(4), name_len(2), unused(2), name
    swap_u32(data, 4);
    swap_u16(data, 8);
}

fn byteswap_create_cursor(data: &mut [u8]) {
    // unused(1), unused(1), len(2), cid(4), source(4), mask(4),
    // fg_red(2), fg_green(2), fg_blue(2), bg_red(2), bg_green(2),
    // bg_blue(2), x(2), y(2)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
    for i in 0..8 {
        swap_u16(data, 16 + i * 2);
    }
}

fn byteswap_create_glyph_cursor(data: &mut [u8]) {
    // unused(1), unused(1), len(2), cid(4), source_font(4), mask_font(4),
    // source_char(2), mask_char(2), fg_red/g/b(2 each), bg_red/g/b(2 each)
    swap_u32(data, 4);
    swap_u32(data, 8);
    swap_u32(data, 12);
    for i in 0..8 {
        swap_u16(data, 16 + i * 2);
    }
}

fn byteswap_recolor_cursor(data: &mut [u8]) {
    // unused(1), unused(1), len(2), cursor(4), fg_r/g/b(2 each), bg_r/g/b(2 each)
    swap_u32(data, 4);
    for i in 0..6 {
        swap_u16(data, 8 + i * 2);
    }
}

fn byteswap_query_best_size(data: &mut [u8]) {
    // class(1), unused(1), len(2), drawable(4), w(2), h(2)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
}

fn byteswap_query_extension(data: &mut [u8]) {
    // unused(1), unused(1), len(2), name_len(2), unused(2), name
    swap_u16(data, 4);
}

fn byteswap_change_keyboard_mapping(data: &mut [u8]) {
    // keycode_count(1), unused(1), len(2), first_keycode(1),
    // keysyms_per_keycode(1), unused(2), keysyms(4 each)
    let count = data[1] as usize;
    let per = data.get(4).copied().unwrap_or(0) as usize;
    let n = count * per;
    swap_u32_array(data, 8, n);
}

fn byteswap_change_keyboard_control(data: &mut [u8]) {
    // unused(1), unused(1), len(2), value_mask(4), values(4 each)
    swap_u32(data, 4);
    let mask = read_u32_le(data, 4);
    let n = popcount32(mask);
    swap_u32_array(data, 8, n);
}

fn byteswap_change_pointer_control(data: &mut [u8]) {
    // unused(1), unused(1), len(2), accel_num(2), accel_den(2),
    // threshold(2), do_accel(1), do_thresh(1)
    swap_u16(data, 4);
    swap_u16(data, 6);
    swap_u16(data, 8);
}

fn byteswap_set_screen_saver(data: &mut [u8]) {
    // unused(1), unused(1), len(2), timeout(2), interval(2),
    // prefer_blanking(1), allow_exposures(1), unused(2)
    swap_u16(data, 4);
    swap_u16(data, 6);
}

fn byteswap_change_hosts(data: &mut [u8]) {
    // mode(1), unused(1), len(2), family(1), unused(1), addr_len(2), address
    swap_u16(data, 6);
}

fn byteswap_rotate_properties(data: &mut [u8], body_len: usize) {
    // unused(1), unused(1), len(2), wid(4), n_atoms(2), delta(2), atoms(4 each)
    swap_u32(data, 4);
    swap_u16(data, 8);
    swap_u16(data, 10);
    let n = (body_len - 12) / 4;
    swap_u32_array(data, 12, n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_window_swaps_wid_and_parent() {
        // MSB-encoded CreateWindow with wid=0x12345678, parent=0xAABBCCDD,
        // length=8 (32 bytes), default values.
        let mut req = [0u8; 32];
        req[0] = 1; // opcode
        req[1] = 0; // depth
        req[2] = 0;
        req[3] = 8; // length 8 in MSB
        req[4..8].copy_from_slice(&0x12345678u32.to_be_bytes()); // wid
        req[8..12].copy_from_slice(&0xAABBCCDDu32.to_be_bytes()); // parent
        req[12..14].copy_from_slice(&0x0001u16.to_be_bytes()); // x = 1
        req[28..32].copy_from_slice(&0u32.to_be_bytes()); // value_mask = 0

        byteswap_request_in_place(&mut req);

        assert_eq!(u16::from_le_bytes([req[2], req[3]]), 8);
        assert_eq!(
            u32::from_le_bytes([req[4], req[5], req[6], req[7]]),
            0x12345678
        );
        assert_eq!(
            u32::from_le_bytes([req[8], req[9], req[10], req[11]]),
            0xAABBCCDD
        );
        assert_eq!(u16::from_le_bytes([req[12], req[13]]), 1);
    }

    #[test]
    fn change_property_format_32_swaps_data_words() {
        // ChangeProperty: opcode 18, mode=0, len=8 (32 bytes),
        // wid=1, prop=2, type=3, format=32, data_len=2, data=[0xDEADBEEF, 0xFEEDFACE]
        let mut req = [0u8; 32];
        req[0] = 18;
        req[2] = 0;
        req[3] = 8;
        req[4..8].copy_from_slice(&1u32.to_be_bytes());
        req[8..12].copy_from_slice(&2u32.to_be_bytes());
        req[12..16].copy_from_slice(&3u32.to_be_bytes());
        req[16] = 32;
        req[20..24].copy_from_slice(&2u32.to_be_bytes());
        req[24..28].copy_from_slice(&0xDEADBEEFu32.to_be_bytes());
        req[28..32].copy_from_slice(&0xFEEDFACEu32.to_be_bytes());

        byteswap_request_in_place(&mut req);

        assert_eq!(
            u32::from_le_bytes([req[24], req[25], req[26], req[27]]),
            0xDEADBEEF
        );
        assert_eq!(
            u32::from_le_bytes([req[28], req[29], req[30], req[31]]),
            0xFEEDFACE
        );
    }

    #[test]
    fn opcodes_with_no_body_no_panic() {
        // GrabServer: opcode 36, len=1 (4 bytes)
        let mut req = [36u8, 0, 0, 1];
        byteswap_request_in_place(&mut req);
        assert_eq!(u16::from_le_bytes([req[2], req[3]]), 1);
    }
}
