use x11rb_protocol::protocol::xproto::{
    BackingStore, Depth, EventMask, Format, ImageOrder, Screen, Setup,
    Visualtype, VisualClass,
};
use x11rb_protocol::x11_utils::Serialize;
use super::core::*;

/// Byte-swap all multi-byte fields in an x11rb-serialized Setup reply from
/// little-endian to big-endian (MSB-first).  This walks the wire-format
/// structure defined in the X11 protocol spec section 8 ("Connection Setup").
pub(crate) fn byteswap_setup_reply(buf: &mut [u8]) {
    if buf.len() < 32 {
        return;
    }

    // Helper: swap a u16 in-place at `off`
    fn swap16(b: &mut [u8], off: usize) {
        if off + 2 <= b.len() {
            b.swap(off, off + 1);
        }
    }
    // Helper: swap a u32 in-place at `off`
    fn swap32(b: &mut [u8], off: usize) {
        if off + 4 <= b.len() {
            b.swap(off, off + 3);
            b.swap(off + 1, off + 2);
        }
    }

    // --- Fixed-size header (bytes 0..39) ---
    // [0] status (u8) — no swap
    // [1] unused
    swap16(buf, 2);   // protocol-major-version
    swap16(buf, 4);   // protocol-minor-version
    swap16(buf, 6);   // additional-data length (in 4-byte units)
    swap32(buf, 8);   // release-number
    swap32(buf, 12);  // resource-id-base
    swap32(buf, 16);  // resource-id-mask
    swap32(buf, 20);  // motion-buffer-size
    // Read vendor length and format count BEFORE swapping them (they're still LE)
    let vendor_len = u16::from_le_bytes([buf[24], buf[25]]) as usize;
    let num_formats = buf[29] as usize;
    let num_screens = buf[28] as usize;
    swap16(buf, 24);  // length-of-vendor
    swap16(buf, 26);  // maximum-request-length
    // [28] number-of-screens (u8) — no swap
    // [29] number-of-formats (u8) — no swap
    // [30] image-byte-order (u8) — no swap
    // [31] bitmap-format-bit-order (u8) — no swap
    // [32] bitmap-format-scanline-unit (u8) — no swap
    // [33] bitmap-format-scanline-pad (u8) — no swap
    // [34] min-keycode (u8) — no swap
    // [35] max-keycode (u8) — no swap
    // [36..39] unused (4 bytes)

    // --- Vendor string (padded to 4 bytes) ---
    let mut off = 40;
    let vendor_padded = (vendor_len + 3) & !3;
    off += vendor_padded; // skip vendor bytes (raw, no swap needed)

    // --- Pixmap formats (8 bytes each) ---
    // Format: depth(1) bpp(1) scanline_pad(1) pad(5) — all single-byte, no swap
    off += num_formats * 8;

    // --- Screens ---
    for _ in 0..num_screens {
        if off + 40 > buf.len() {
            return;
        }
        swap32(buf, off);      // root window
        swap32(buf, off + 4);  // default-colormap
        swap32(buf, off + 8);  // white-pixel
        swap32(buf, off + 12); // black-pixel
        swap32(buf, off + 16); // current-input-masks
        swap16(buf, off + 20); // width-in-pixels
        swap16(buf, off + 22); // height-in-pixels
        swap16(buf, off + 24); // width-in-millimeters
        swap16(buf, off + 26); // height-in-millimeters
        swap16(buf, off + 28); // min-installed-maps
        swap16(buf, off + 30); // max-installed-maps
        swap32(buf, off + 32); // root-visual
        // [off+36] backing-stores (u8)
        // [off+37] save-unders (u8)
        // [off+38] root-depth (u8)
        let num_depths = buf[off + 39] as usize;
        off += 40;

        // --- Depths ---
        for _ in 0..num_depths {
            if off + 8 > buf.len() {
                return;
            }
            // [off] depth (u8)
            // [off+1] unused
            let num_visuals = u16::from_le_bytes([buf[off + 2], buf[off + 3]]) as usize;
            swap16(buf, off + 2); // number-of-visuals
            // [off+4..7] unused (4 bytes)
            off += 8;

            // --- Visuals (24 bytes each) ---
            for _ in 0..num_visuals {
                if off + 24 > buf.len() {
                    return;
                }
                swap32(buf, off);      // visual-id
                // [off+4] class (u8)
                // [off+5] bits-per-rgb-value (u8)
                swap16(buf, off + 6);  // colormap-entries
                swap32(buf, off + 8);  // red-mask
                swap32(buf, off + 12); // green-mask
                swap32(buf, off + 16); // blue-mask
                // [off+20..23] unused (4 bytes)
                off += 24;
            }
        }
    }
}

/// Build the XSETTINGS binary property data (LSB-first byte order).
///
/// See <https://specifications.freedesktop.org/xsettings-spec/0.5/> for format.
pub(crate) fn build_xsettings_data() -> Vec<u8> {
    let mut buf = Vec::with_capacity(1024);

    // Header
    buf.push(0); // byte-order: LSB
    buf.extend_from_slice(&[0u8; 3]); // padding
    buf.extend_from_slice(&0u32.to_le_bytes()); // serial
    // n_settings placeholder — we'll patch it after adding all settings
    let n_settings_offset = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());

    let mut count: u32 = 0;

    // Helper closures captured by reference
    let write_integer = |buf: &mut Vec<u8>, name: &str, value: u32| {
        buf.push(0); // type = XSettingsTypeInteger
        buf.push(0); // padding
        let name_bytes = name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        // pad name to 4-byte boundary
        let pad = (4 - (name_bytes.len() % 4)) % 4;
        for _ in 0..pad { buf.push(0); }
        buf.extend_from_slice(&0u32.to_le_bytes()); // last_change_serial
        buf.extend_from_slice(&value.to_le_bytes());
    };

    let write_string = |buf: &mut Vec<u8>, name: &str, value: &str| {
        buf.push(1); // type = XSettingsTypeString
        buf.push(0); // padding
        let name_bytes = name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        let pad = (4 - (name_bytes.len() % 4)) % 4;
        for _ in 0..pad { buf.push(0); }
        buf.extend_from_slice(&0u32.to_le_bytes()); // last_change_serial
        let val_bytes = value.as_bytes();
        buf.extend_from_slice(&(val_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(val_bytes);
        let vpad = (4 - (val_bytes.len() % 4)) % 4;
        for _ in 0..vpad { buf.push(0); }
    };

    // Xft settings
    write_integer(&mut buf, "Xft/DPI", 98304); count += 1;
    write_integer(&mut buf, "Xft/Antialias", 1); count += 1;
    write_integer(&mut buf, "Xft/Hinting", 1); count += 1;
    write_string(&mut buf, "Xft/HintStyle", "hintslight"); count += 1;
    write_string(&mut buf, "Xft/RGBA", "rgb"); count += 1;

    // Net settings
    write_string(&mut buf, "Net/ThemeName", "Adwaita"); count += 1;
    write_string(&mut buf, "Net/IconThemeName", "Adwaita"); count += 1;
    write_integer(&mut buf, "Net/CursorBlink", 1); count += 1;
    write_integer(&mut buf, "Net/CursorBlinkTime", 1200); count += 1;
    write_integer(&mut buf, "Net/EnableEventSounds", 0); count += 1;
    write_integer(&mut buf, "Net/EnableInputFeedbackSounds", 0); count += 1;

    // Gtk settings
    write_string(&mut buf, "Gtk/CursorThemeName", "default"); count += 1;
    write_integer(&mut buf, "Gtk/CursorThemeSize", 24); count += 1;
    write_string(&mut buf, "Gtk/FontName", "Sans 10"); count += 1;
    write_integer(&mut buf, "Gtk/EnableAnimations", 1); count += 1;
    write_integer(&mut buf, "Gtk/DialogsUseHeader", 1); count += 1;
    write_string(&mut buf, "Gtk/DecorationLayout", "menu:minimize,maximize,close"); count += 1;
    write_integer(&mut buf, "Gtk/ShellShowsMenubar", 0); count += 1;
    write_integer(&mut buf, "Gtk/ShellShowsAppMenu", 0); count += 1;

    // Patch n_settings
    buf[n_settings_offset..n_settings_offset + 4].copy_from_slice(&count.to_le_bytes());

    buf
}

pub(crate) fn build_setup(conn_index: u32) -> Setup {
    // Root visual: 24-bit TrueColor (the primary visual)
    let visual = Visualtype {
        visual_id: ROOT_VISUAL,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    // Additional 24-bit DirectColor visual for apps that prefer it
    let visual_dc24 = Visualtype {
        visual_id: 0x22,
        class: VisualClass::DIRECT_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth24 = Depth {
        depth: 24,
        visuals: vec![visual, visual_dc24],
    };

    // 32-bit TrueColor with alpha (for compositing, ARGB windows)
    let visual_argb = Visualtype {
        visual_id: 0x40,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth32 = Depth {
        depth: 32,
        visuals: vec![visual_argb],
    };

    // 8-bit PseudoColor visual (for legacy apps like xv, some games)
    // We emulate PseudoColor by mapping 256-entry colormap to TrueColor internally.
    let visual_8bit = Visualtype {
        visual_id: 0x23,
        class: VisualClass::PSEUDO_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
    };

    // 8-bit GrayScale visual (writable grayscale, for apps that want to modify gray levels)
    let visual_gray8 = Visualtype {
        visual_id: 0x26,
        class: VisualClass::GRAY_SCALE,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
    };

    // 8-bit StaticColor visual (read-only indexed color, 3-3-2 RGB)
    let visual_static_color = Visualtype {
        visual_id: 0x27,
        class: VisualClass::STATIC_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0xE0,
        green_mask: 0x1C,
        blue_mask: 0x03,
    };

    let depth8 = Depth {
        depth: 8,
        visuals: vec![visual_8bit, visual_gray8, visual_static_color],
    };

    // 16-bit TrueColor visual (for some embedded/legacy apps)
    let visual_16bit = Visualtype {
        visual_id: 0x24,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 5,
        colormap_entries: 32,
        red_mask: 0xF800,
        green_mask: 0x07E0,
        blue_mask: 0x001F,
    };

    let depth16 = Depth {
        depth: 16,
        visuals: vec![visual_16bit],
    };

    // Depth 1: for bitmaps/stipple patterns
    let depth1 = Depth {
        depth: 1,
        visuals: vec![], // No visual for depth 1 (bitmap only)
    };

    // 4-bit StaticGray visual (for xbiff, some ancient apps)
    let visual_4bit = Visualtype {
        visual_id: 0x25,
        class: VisualClass::STATIC_GRAY,
        bits_per_rgb_value: 4,
        colormap_entries: 16,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
    };

    let depth4 = Depth {
        depth: 4,
        visuals: vec![visual_4bit],
    };

    let screen = Screen {
        root: ROOT_WINDOW,
        default_colormap: ROOT_COLORMAP,
        white_pixel: 0x00FFFFFF,
        black_pixel: 0x00000000,
        current_input_masks: EventMask::from(0u32),
        width_in_pixels: SCREEN_WIDTH,
        height_in_pixels: SCREEN_HEIGHT,
        width_in_millimeters: 270,
        height_in_millimeters: 203,
        min_installed_maps: 1,
        max_installed_maps: 1,
        root_visual: ROOT_VISUAL,
        backing_stores: BackingStore::ALWAYS,
        save_unders: true,
        root_depth: 24,
        allowed_depths: vec![depth1, depth4, depth8, depth16, depth24, depth32],
    };

    let format24 = Format { depth: 24, bits_per_pixel: 32, scanline_pad: 32 };
    let format32 = Format { depth: 32, bits_per_pixel: 32, scanline_pad: 32 };
    let format1 = Format { depth: 1, bits_per_pixel: 1, scanline_pad: 32 };
    let format4 = Format { depth: 4, bits_per_pixel: 8, scanline_pad: 32 };
    let format8 = Format { depth: 8, bits_per_pixel: 8, scanline_pad: 32 };
    let format16 = Format { depth: 16, bits_per_pixel: 16, scanline_pad: 32 };

    let mut setup = Setup {
        status: 1,
        protocol_major_version: 11,
        protocol_minor_version: 0,
        length: 0,
        release_number: 0,
        resource_id_base: (conn_index + 1) << 22,
        resource_id_mask: 0x003FFFFF,
        motion_buffer_size: 256,
        maximum_request_length: 65535,
        image_byte_order: ImageOrder::LSB_FIRST,
        bitmap_format_bit_order: ImageOrder::LSB_FIRST,
        bitmap_format_scanline_unit: 32,
        bitmap_format_scanline_pad: 32,
        min_keycode: 8,
        max_keycode: 255,
        vendor: b"x11-web".to_vec(),
        pixmap_formats: vec![format1, format4, format8, format16, format24, format32],
        roots: vec![screen],
    };

    let mut bytes = Vec::new();
    setup.serialize_into(&mut bytes);
    setup.length = ((bytes.len() - 8) / 4) as u16;

    setup
}
