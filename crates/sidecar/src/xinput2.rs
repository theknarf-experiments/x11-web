//! XInput / XInput2 protocol implementation.
//!
//! We dispatch and reply to enough of the XI 1.x and XI 2.x request set
//! to keep modern toolkits (Xt, GDK 3, Qt, Mozilla widgets) happy.
//!
//! For wire-format ground truth we re-use `x11rb_protocol::protocol::xinput`
//! types (parsed from the upstream X11 XML protocol description) and let
//! their `Serialize` impls produce the bytes. This guarantees we never
//! drift from the canonical layout.

use std::collections::HashMap;
use tracing::{debug, warn};

use crate::xserver::core::{read_u16_bo, read_u32_bo, write_u16_bo, write_u32_bo};

use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto;
use x11rb_protocol::x11_utils::{RequestHeader, Serialize as _};

use x11_web_protocol::InputEvent;

/// Major opcode we register for XInputExtension in QueryExtension. 131 is
/// the conventional value used by the upstream X server, but the actual
/// number doesn't matter — clients pick it up from QueryExtension.
pub const XI_MAJOR_OPCODE: u8 = 131;
/// Range reserved for XI legacy events. We never emit those (we use the
/// XI2 generic-event path) but the value still has to be advertised.
pub const XI_FIRST_EVENT: u8 = 105;
pub const XI_FIRST_ERROR: u8 = 152;

/// Device IDs we expose. Two master devices is the minimum modern XI
/// clients (e.g. GTK 3) expect.
pub const MASTER_POINTER_ID: xi::DeviceId = 2;
pub const MASTER_KEYBOARD_ID: xi::DeviceId = 3;

/// Per-window XI2 event subscription. One per `(window, deviceid)` tuple.
#[derive(Clone, Debug)]
pub struct XiSelection {
    pub window: u32,
    pub deviceid: xi::DeviceId,
    pub mask: Vec<xi::XIEventMask>,
}

impl XiSelection {
    pub fn wants(&self, evtype: u16) -> bool {
        // The XI2 mask is a bitfield indexed by event type number.
        // x11rb represents it as a Vec<u32> (one u32 per 32 event types).
        let bit = evtype as u32;
        let word = (bit / 32) as usize;
        let in_word = bit % 32;
        self.mask
            .get(word)
            .map(|w| (u32::from(*w) >> in_word) & 1 != 0)
            .unwrap_or(false)
    }
}

fn fp1616(v: i16) -> xi::Fp1616 {
    (v as i32) << 16
}

fn fp3232(int: i32) -> xi::Fp3232 {
    xi::Fp3232 { integral: int, frac: 0 }
}

/// Per-axis valuator state we track for the master pointer. The X server
/// uses these to populate `XIValuatorClassInfo.value` in `XIQueryDevice`
/// replies and the per-event valuator data in motion / button events.
///
/// `scroll_v` / `scroll_h` accumulate over the lifetime of the connection
/// — XI2 clients compute scroll deltas from successive valuator values.
/// When a wheel event arrives we bump these by `1.0` (matching the
/// `increment` we report in our scroll classes) per discrete wheel notch.
#[derive(Clone, Debug, Default)]
pub struct ValuatorState {
    pub x: i32,
    pub y: i32,
    pub scroll_v: i32,
    pub scroll_h: i32,
}

/// Axis numbers we use for the master pointer's valuator/scroll
/// classes. Valuator 0 / 1 are the absolute X / Y axes (emitted as
/// `XIValuatorClass` entries in our `XIQueryDevice` reply); 2 / 3
/// are the vertical / horizontal scroll axes (`XIScrollClass`).
pub const AXIS_SCROLL_V: u16 = 2;
pub const AXIS_SCROLL_H: u16 = 3;

/// Build a `Reply` byte slice with the standard 32-byte header populated.
/// `xi_reply_type` is the value placed at byte 1 of the reply (set to the
/// XI minor opcode of the request being answered, matching the upstream
/// xserver convention).
#[allow(dead_code)]
fn xi_reply_header(seq: u16, xi_reply_type: u8, length_units: u32, msb_first: bool) -> Vec<u8> {
    let mut buf = vec![0u8; 32 + (length_units as usize) * 4];
    buf[0] = 1; // X_Reply
    buf[1] = xi_reply_type;
    write_u16_bo(&mut buf, 2, seq, msb_first);
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

/// Serialize an x11rb XInput reply, then patch up its `length` field
/// (in 4-byte units after the 32-byte header). x11rb's `Serialize` impls
/// don't compute `length` automatically — it has to match the actual
/// number of trailing bytes or XCB hits "Too much data requested".
fn serialize_xi_reply<R: x11rb_protocol::x11_utils::Serialize>(reply: &R, msb_first: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    reply.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    debug_assert!(buf.len() >= 32, "XI reply must be at least 32 bytes");
    let length_units = ((buf.len() - 32) / 4) as u32;
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

// ---------------------------------------------------------------------------
// XI 1.x helper functions
// ---------------------------------------------------------------------------

/// Minimum keycode we advertise in the Setup reply.
const MIN_KEYCODE: u8 = 8;
/// Maximum keycode we advertise.
const MAX_KEYCODE: u8 = 255;
/// Number of physical + scroll buttons for the virtual pointer.
const N_POINTER_BUTTONS: u16 = 7;

/// Build a proper ListInputDevices reply with the two virtual core devices.
/// Each device carries its full set of XI 1.x input classes (KeyInfo,
/// ButtonInfo, ValuatorInfo) so legacy toolkits can discover device
/// capabilities.
fn build_list_input_devices_reply(
    seq: u16,
    _valuators: &ValuatorState,
    screen_width: u16,
    screen_height: u16,
    msb_first: bool,
) -> Vec<u8> {
    use x11rb_protocol::protocol::xproto;

    // -- Pointer device: button class + valuator class (X, Y) --
    let pointer_button_info = xi::InputInfo {
        len: 4, // class_id(1) + len(1) + num_buttons(2) = 4 bytes
        info: xi::InputInfoInfo::Button(xi::InputInfoInfoButton {
            num_buttons: N_POINTER_BUTTONS,
        }),
    };
    let pointer_valuator_info = xi::InputInfo {
        len: 8 + 12 * 4, // header(8) + 4 axes × 12 bytes each
        info: xi::InputInfoInfo::Valuator(xi::InputInfoInfoValuator {
            mode: xi::ValuatorMode::ABSOLUTE,
            motion_size: 0,
            axes: vec![
                xi::AxisInfo {
                    resolution: 1,
                    minimum: 0,
                    maximum: i32::from(screen_width),
                },
                xi::AxisInfo {
                    resolution: 1,
                    minimum: 0,
                    maximum: i32::from(screen_height),
                },
                // Scroll vertical (relative)
                xi::AxisInfo {
                    resolution: 1,
                    minimum: -1,
                    maximum: -1,
                },
                // Scroll horizontal (relative)
                xi::AxisInfo {
                    resolution: 1,
                    minimum: -1,
                    maximum: -1,
                },
            ],
        }),
    };

    // -- Keyboard device: key class --
    let keyboard_key_info = xi::InputInfo {
        len: 8, // class_id(1) + len(1) + min(1) + max(1) + num_keys(2) + pad(2) = 8
        info: xi::InputInfoInfo::Key(xi::InputInfoInfoKey {
            min_keycode: MIN_KEYCODE,
            max_keycode: MAX_KEYCODE,
            num_keys: (MAX_KEYCODE - MIN_KEYCODE + 1) as u16,
        }),
    };

    let reply = xi::ListInputDevicesReply {
        xi_reply_type: xi::LIST_INPUT_DEVICES_REQUEST,
        sequence: seq,
        length: 0, // patched by serialize_xi_reply
        devices: vec![
            xi::DeviceInfo {
                device_type: 0,
                device_id: MASTER_POINTER_ID as u8,
                num_class_info: 2, // button + valuator
                device_use: xi::DeviceUse::IS_X_POINTER,
            },
            xi::DeviceInfo {
                device_type: 0,
                device_id: MASTER_KEYBOARD_ID as u8,
                num_class_info: 1, // key
                device_use: xi::DeviceUse::IS_X_KEYBOARD,
            },
        ],
        infos: vec![pointer_button_info, pointer_valuator_info, keyboard_key_info],
        names: vec![
            xproto::Str { name: b"Virtual core pointer".to_vec() },
            xproto::Str { name: b"Virtual core keyboard".to_vec() },
        ],
    };
    serialize_xi_reply(&reply, msb_first)
}

/// Build an OpenDevice reply for the given device_id. Returns the device's
/// input classes (key, button, valuator) so XI 1.x clients can query
/// device capabilities after opening.
fn build_open_device_reply(
    device_id: u8,
    seq: u16,
    _screen_width: u16,
    _screen_height: u16,
    msb_first: bool,
) -> Vec<u8> {
    // OpenDevice reply: 32-byte header
    //   byte 0  = 1 (Reply)
    //   byte 1  = xi_reply_type (3)
    //   bytes 2-3 = sequence
    //   bytes 4-7 = length (in 4-byte units after header)
    //   byte 8  = num_classes
    //   bytes 9-31 = pad
    //   followed by InputClassInfo structs (2 bytes each: class_id + event_type_base)

    let is_keyboard = device_id == MASTER_KEYBOARD_ID as u8;

    // Each InputClassInfo is 2 bytes: class_id(1) + event_type_base(1)
    let classes: Vec<(u8, u8)> = if is_keyboard {
        vec![(0 /* KEY */, 0)]
    } else {
        vec![
            (1 /* BUTTON */, 0),
            (2 /* VALUATOR */, 0),
        ]
    };

    let num_classes = classes.len() as u8;
    let extra_bytes = (classes.len() * 2 + 3) & !3; // pad to 4
    let length_units = extra_bytes / 4;

    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1;
    reply[1] = 3; // xi_reply_type = OpenDevice
    write_u16_bo(&mut reply, 2, seq, msb_first);
    write_u32_bo(&mut reply, 4, length_units as u32, msb_first);
    reply[8] = num_classes;

    for (i, (class_id, event_base)) in classes.iter().enumerate() {
        reply[32 + i * 2] = *class_id;
        reply[32 + i * 2 + 1] = *event_base;
    }
    reply
}

/// Build a GetDeviceKeyMapping reply that mirrors the core keyboard mapping.
/// Returns keysyms for the requested keycode range.
fn build_device_key_mapping_reply(
    first_keycode: u8,
    count: u8,
    seq: u16,
    msb_first: bool,
) -> Vec<u8> {
    // We use 4 keysyms per keycode (normal, shift, altgr, shift+altgr)
    // matching the core GetKeyboardMapping format.
    let keysyms_per_keycode: u8 = 4;
    let n_keycodes = count as usize;
    let n_keysyms = n_keycodes * keysyms_per_keycode as usize;
    let length_units = n_keysyms as u32; // each keysym is 4 bytes = 1 unit

    let mut reply = vec![0u8; 32 + n_keysyms * 4];
    reply[0] = 1;
    reply[1] = keysyms_per_keycode;
    write_u16_bo(&mut reply, 2, seq, msb_first);
    write_u32_bo(&mut reply, 4, length_units, msb_first);
    reply[8] = 24; // xi_reply_type

    // Fill in keysyms using the same keycode_to_keysym mapping as core.
    for i in 0..n_keycodes {
        let kc = first_keycode.wrapping_add(i as u8);
        let (normal, shifted) = keycode_to_keysym_xi(kc);
        let offset = 32 + i * keysyms_per_keycode as usize * 4;
        write_u32_bo(&mut reply, offset, normal, msb_first);
        write_u32_bo(&mut reply, offset + 4, shifted, msb_first);
        // altgr and shift+altgr are NoSymbol
        write_u32_bo(&mut reply, offset + 8, 0, msb_first);
        write_u32_bo(&mut reply, offset + 12, 0, msb_first);
    }
    reply
}

/// Minimal US keyboard layout mapping for XI 1.x GetDeviceKeyMapping.
/// Returns (normal_keysym, shifted_keysym) for a given keycode.
fn keycode_to_keysym_xi(keycode: u8) -> (u32, u32) {
    match keycode {
        9 => (0xff1b, 0xff1b),   // Escape
        10 => (0x31, 0x21),       // 1 / !
        11 => (0x32, 0x40),       // 2 / @
        12 => (0x33, 0x23),       // 3 / #
        13 => (0x34, 0x24),       // 4 / $
        14 => (0x35, 0x25),       // 5 / %
        15 => (0x36, 0x5e),       // 6 / ^
        16 => (0x37, 0x26),       // 7 / &
        17 => (0x38, 0x2a),       // 8 / *
        18 => (0x39, 0x28),       // 9 / (
        19 => (0x30, 0x29),       // 0 / )
        20 => (0x2d, 0x5f),       // - / _
        21 => (0x3d, 0x2b),       // = / +
        22 => (0xff08, 0xff08),   // BackSpace
        23 => (0xff09, 0xfe20),   // Tab / ISO_Left_Tab
        24 => (0x71, 0x51),       // q / Q
        25 => (0x77, 0x57),       // w / W
        26 => (0x65, 0x45),       // e / E
        27 => (0x72, 0x52),       // r / R
        28 => (0x74, 0x54),       // t / T
        29 => (0x79, 0x59),       // y / Y
        30 => (0x75, 0x55),       // u / U
        31 => (0x69, 0x49),       // i / I
        32 => (0x6f, 0x4f),       // o / O
        33 => (0x70, 0x50),       // p / P
        34 => (0x5b, 0x7b),       // [ / {
        35 => (0x5d, 0x7d),       // ] / }
        36 => (0xff0d, 0xff0d),   // Return
        37 => (0xffe3, 0xffe3),   // Control_L
        38 => (0x61, 0x41),       // a / A
        39 => (0x73, 0x53),       // s / S
        40 => (0x64, 0x44),       // d / D
        41 => (0x66, 0x46),       // f / F
        42 => (0x67, 0x47),       // g / G
        43 => (0x68, 0x48),       // h / H
        44 => (0x6a, 0x4a),       // j / J
        45 => (0x6b, 0x4b),       // k / K
        46 => (0x6c, 0x4c),       // l / L
        47 => (0x3b, 0x3a),       // ; / :
        48 => (0x27, 0x22),       // ' / "
        49 => (0x60, 0x7e),       // ` / ~
        50 => (0xffe1, 0xffe1),   // Shift_L
        51 => (0x5c, 0x7c),       // \ / |
        52 => (0x7a, 0x5a),       // z / Z
        53 => (0x78, 0x58),       // x / X
        54 => (0x63, 0x43),       // c / C
        55 => (0x76, 0x56),       // v / V
        56 => (0x62, 0x42),       // b / B
        57 => (0x6e, 0x4e),       // n / N
        58 => (0x6d, 0x4d),       // m / M
        59 => (0x2c, 0x3c),       // , / <
        60 => (0x2e, 0x3e),       // . / >
        61 => (0x2f, 0x3f),       // / / ?
        62 => (0xffe2, 0xffe2),   // Shift_R
        63 => (0xffaa, 0xffaa),   // KP_Multiply
        64 => (0xffe9, 0xffe9),   // Alt_L
        65 => (0x20, 0x20),       // space
        66 => (0xffe5, 0xffe5),   // Caps_Lock
        67..=76 => {              // F1-F10
            let fkey = 0xffbe + (keycode - 67) as u32;
            (fkey, fkey)
        }
        95 => (0xffc8, 0xffc8),   // F11
        96 => (0xffc9, 0xffc9),   // F12
        105 => (0xffe4, 0xffe4),  // Control_R
        108 => (0xffea, 0xffea),  // Alt_R
        110 => (0xff50, 0xff50),  // Home
        111 => (0xff52, 0xff52),  // Up
        112 => (0xff55, 0xff55),  // Prior/PageUp
        113 => (0xff51, 0xff51),  // Left
        114 => (0xff53, 0xff53),  // Right
        115 => (0xff57, 0xff57),  // End
        116 => (0xff54, 0xff54),  // Down
        117 => (0xff56, 0xff56),  // Next/PageDown
        118 => (0xff63, 0xff63),  // Insert
        119 => (0xffff, 0xffff),  // Delete
        133 => (0xffeb, 0xffeb),  // Super_L
        134 => (0xffec, 0xffec),  // Super_R
        _ => (0, 0),              // NoSymbol
    }
}

/// Build a GetDeviceModifierMapping reply with the standard modifier map.
fn build_device_modifier_mapping_reply(seq: u16, msb_first: bool) -> Vec<u8> {
    // Standard modifier map: 8 modifiers, each can have up to N keycodes.
    // We use 2 keycodes per modifier (padded).
    let keycodes_per_modifier: u8 = 2;
    let map_size = 8 * keycodes_per_modifier as usize;
    let length_units = (map_size + 3) / 4;

    let mut reply = vec![0u8; 32 + length_units * 4];
    reply[0] = 1;
    reply[1] = keycodes_per_modifier;
    write_u16_bo(&mut reply, 2, seq, msb_first);
    write_u32_bo(&mut reply, 4, length_units as u32, msb_first);
    reply[8] = 26; // xi_reply_type

    // Modifier map: [Shift, Lock, Control, Mod1(Alt), Mod2(Num), Mod3, Mod4(Super), Mod5]
    let modifier_keycodes: [(u8, u8); 8] = [
        (50, 62),   // Shift: Shift_L(50), Shift_R(62)
        (66, 0),     // Lock: Caps_Lock(66)
        (37, 105),   // Control: Control_L(37), Control_R(105)
        (64, 108),   // Mod1 (Alt): Alt_L(64), Alt_R(108)
        (77, 0),     // Mod2 (Num Lock): Num_Lock(77)
        (0, 0),      // Mod3: unused
        (133, 134),  // Mod4 (Super): Super_L(133), Super_R(134)
        (0, 0),      // Mod5: unused
    ];

    for (i, (kc1, kc2)) in modifier_keycodes.iter().enumerate() {
        reply[32 + i * 2] = *kc1;
        reply[32 + i * 2 + 1] = *kc2;
    }
    reply
}

/// Build a QueryDeviceState reply with current button/key/valuator state.
fn build_query_device_state_reply(
    device_id: u8,
    valuators: &ValuatorState,
    seq: u16,
    msb_first: bool,
) -> Vec<u8> {
    let is_keyboard = device_id == MASTER_KEYBOARD_ID as u8;

    if is_keyboard {
        // Key class state: class(1) + length(1) + num_keys(1) + pad(1) + keys[32]
        let class_len: u8 = 36;
        let length_units = (1 + class_len as usize + 3) / 4;
        let mut reply = vec![0u8; 32 + length_units * 4];
        reply[0] = 1;
        write_u16_bo(&mut reply, 2, seq, msb_first);
        write_u32_bo(&mut reply, 4, length_units as u32, msb_first);
        reply[8] = 30;
        reply[12] = 1; // num_classes
        // Key state class
        reply[32] = 0; // class_id = KEY
        reply[33] = class_len;
        reply[34] = 248; // num_keys = MAX - MIN + 1
        // keys[32] = all zeros (no keys pressed)
        reply
    } else {
        // Button class: class(1) + length(1) + num_buttons(1) + pad(1) + buttons[4]
        // Valuator class: class(1) + length(1) + num_valuators(1) + mode(1) + valuators[4*4]
        let button_class_len: u8 = 8;
        let valuator_class_len: u8 = 4 + 4 * 4; // header(4) + 4 valuators × 4 bytes
        let total_extra = button_class_len as usize + valuator_class_len as usize;
        let length_units = (total_extra + 3) / 4;

        let mut reply = vec![0u8; 32 + length_units * 4];
        reply[0] = 1;
        write_u16_bo(&mut reply, 2, seq, msb_first);
        write_u32_bo(&mut reply, 4, length_units as u32, msb_first);
        reply[8] = 30;
        reply[12] = 2; // num_classes

        // Button state class
        let off = 32;
        reply[off] = 1; // class_id = BUTTON
        reply[off + 1] = button_class_len;
        reply[off + 2] = N_POINTER_BUTTONS as u8;
        // buttons[4] = all zeros (no buttons pressed)

        // Valuator state class
        let voff = off + button_class_len as usize;
        reply[voff] = 2; // class_id = VALUATOR
        reply[voff + 1] = valuator_class_len;
        reply[voff + 2] = 4; // num_valuators: X, Y, ScrollV, ScrollH
        reply[voff + 3] = 0; // mode = Relative

        // Valuator values (4 bytes each, matching device axis order)
        write_u32_bo(&mut reply, voff + 4, valuators.x as u32, msb_first);
        write_u32_bo(&mut reply, voff + 8, valuators.y as u32, msb_first);
        write_u32_bo(&mut reply, voff + 12, valuators.scroll_v as u32, msb_first);
        write_u32_bo(&mut reply, voff + 16, valuators.scroll_h as u32, msb_first);

        reply
    }
}

/// Build the bytes for the master pointer's `XIDeviceInfo`. The screen
/// dimensions bound the valuator axes — XI clients use these to clamp
/// or normalise pointer position values.
fn build_master_pointer_info(
    valuators: &ValuatorState,
    screen_width: u16,
    screen_height: u16,
) -> xi::XIDeviceInfo {
    let button_class = xi::DeviceClass {
        len: 0, // overwritten below
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Button(xi::DeviceClassDataButton {
            // 7-button pointer: left/middle/right + scroll up/down/left/right.
            // Empty state mask (no buttons currently pressed) padded to a
            // multiple of 32 bits.
            state: vec![0],
            labels: vec![0; 7],
        }),
    };

    let valuator_x = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: 0,
            label: 0, // None
            min: fp3232(0),
            max: fp3232(i32::from(screen_width)),
            value: fp3232(valuators.x),
            resolution: 1,
            mode: xi::ValuatorMode::ABSOLUTE,
        }),
    };

    let valuator_y = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: 1,
            label: 0,
            min: fp3232(0),
            max: fp3232(i32::from(screen_height)),
            value: fp3232(valuators.y),
            resolution: 1,
            mode: xi::ValuatorMode::ABSOLUTE,
        }),
    };

    // The scroll axes have to be exposed as XIValuatorClass entries
    // *as well as* XIScrollClass entries. GDK (and other XI2 clients
    // following the spec) interprets each XIScrollClass as "this axis,
    // already declared as a valuator, additionally has scroll-axis
    // semantics". GDK will assert
    //   `n_valuator < gdk_device_get_n_axes(device)`
    // and abort the device init if the scroll class references a
    // valuator number for which no XIValuatorClass exists — which is
    // exactly the failure that prevented Firefox from starting up.
    //
    // Scroll valuators are RELATIVE (clients compute deltas from
    // successive values) and unbounded (min == max == 0).
    let valuator_scroll_v = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: AXIS_SCROLL_V,
            label: 0,
            min: fp3232(0),
            max: fp3232(0),
            value: fp3232(valuators.scroll_v),
            resolution: 1,
            mode: xi::ValuatorMode::RELATIVE,
        }),
    };

    let valuator_scroll_h = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: AXIS_SCROLL_H,
            label: 0,
            min: fp3232(0),
            max: fp3232(0),
            value: fp3232(valuators.scroll_h),
            resolution: 1,
            mode: xi::ValuatorMode::RELATIVE,
        }),
    };

    let scroll_v = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Scroll(xi::DeviceClassDataScroll {
            number: AXIS_SCROLL_V,
            scroll_type: xi::ScrollType::VERTICAL,
            flags: 0u32.into(),
            increment: fp3232(1),
        }),
    };

    let scroll_h = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Scroll(xi::DeviceClassDataScroll {
            number: AXIS_SCROLL_H,
            scroll_type: xi::ScrollType::HORIZONTAL,
            flags: 0u32.into(),
            increment: fp3232(1),
        }),
    };

    // Touch class: advertise direct touch support with 10 simultaneous touch points.
    let touch_class = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Touch(xi::DeviceClassDataTouch {
            mode: xi::TouchMode::DIRECT,
            num_touches: 10,
        }),
    };

    // Gesture class: advertise pinch and swipe gesture support.
    let gesture_class = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Gesture(xi::DeviceClassDataGesture {
            num_touches: 5,
        }),
    };

    // Class order matters for GDK: it walks the list once, and for
    // each XIScrollClass it expects the corresponding XIValuatorClass
    // to have *already* been seen (so `gdk_device_get_n_axes` already
    // covers the referenced axis). Put the four valuators first, then
    // the two scroll classes, then the button class.
    let mut classes = vec![
        button_class,
        valuator_x,
        valuator_y,
        valuator_scroll_v,
        valuator_scroll_h,
        scroll_v,
        scroll_h,
        touch_class,
        gesture_class,
    ];
    fill_class_lengths(&mut classes);

    xi::XIDeviceInfo {
        deviceid: MASTER_POINTER_ID,
        type_: xi::DeviceType::MASTER_POINTER,
        attachment: MASTER_KEYBOARD_ID,
        enabled: true,
        name: b"Virtual core pointer".to_vec(),
        classes,
    }
}

fn build_master_keyboard_info() -> xi::XIDeviceInfo {
    let mut classes = vec![xi::DeviceClass {
        len: 0,
        sourceid: MASTER_KEYBOARD_ID,
        data: xi::DeviceClassData::Key(xi::DeviceClassDataKey { keys: vec![] }),
    }];
    fill_class_lengths(&mut classes);

    xi::XIDeviceInfo {
        deviceid: MASTER_KEYBOARD_ID,
        type_: xi::DeviceType::MASTER_KEYBOARD,
        attachment: MASTER_POINTER_ID,
        enabled: true,
        name: b"Virtual core keyboard".to_vec(),
        classes,
    }
}

/// Walk a list of `DeviceClass` and back-fill each `len` field with the
/// number of 4-byte units the class occupies on the wire (including the
/// 8-byte common header).
fn fill_class_lengths(classes: &mut [xi::DeviceClass]) {
    for c in classes.iter_mut() {
        let mut buf = Vec::new();
        c.serialize_into(&mut buf);
        debug_assert!(buf.len() % 4 == 0, "class wire length not 4-aligned");
        c.len = (buf.len() / 4) as u16;
    }
}

/// Build the byte payload for `XIQueryDevice` containing both master
/// devices.
fn query_device_reply_bytes(
    seq: u16,
    requested: xi::DeviceId,
    valuators: &ValuatorState,
    screen_width: u16,
    screen_height: u16,
    msb_first: bool,
) -> Vec<u8> {
    // Resolve which devices the client asked for. 0 = AllDevices, 1 = AllMaster.
    let mp = build_master_pointer_info(valuators, screen_width, screen_height);
    let mk = build_master_keyboard_info();
    let infos: Vec<xi::XIDeviceInfo> = match requested {
        0 | 1 => vec![mp, mk],
        MASTER_POINTER_ID => vec![mp],
        MASTER_KEYBOARD_ID => vec![mk],
        // Unknown — return both as a graceful fallback.
        _ => vec![mp, mk],
    };

    let reply = xi::XIQueryDeviceReply {
        sequence: seq,
        length: 0, // patched by serialize_xi_reply
        infos,
    };
    serialize_xi_reply(&reply, msb_first)
}

/// Convert our internal pointer mask byte (state from `InputEvent`) into
/// the XI2 `ModifierInfo` struct. We currently track only the X11 core
/// modifier bits in the mask, so the latched/locked fields stay zero.
fn mods_from_state(state: u16) -> xi::ModifierInfo {
    xi::ModifierInfo {
        base: 0,
        latched: 0,
        locked: 0,
        effective: u32::from(state),
    }
}

/// Build an XI2 `RawMotion` event. Raw events have no event window —
/// they're delivered to every client that selected on the root window.
/// Used by toolkits (notably Xt-based xeyes) that drive their cursor
/// tracking purely from XI2 raw events.
///
/// `sequence` should be the sequence number of the most-recently-processed
/// request at the time the event is *sent* to the client. XCB validates
/// event sequence numbers against its outstanding-request tracker, and a
/// stale sequence number causes `Unknown sequence number while processing
/// queue` and a hard abort.
pub fn build_raw_motion_event(sequence: u16, msb_first: bool) -> Vec<u8> {
    let event = xi::RawMotionEvent {
        response_type: 35, // GenericEvent
        extension: XI_MAJOR_OPCODE,
        sequence,
        length: 0,
        event_type: xi::RAW_MOTION_EVENT,
        deviceid: MASTER_POINTER_ID,
        time: 0,
        detail: 0,
        sourceid: MASTER_POINTER_ID,
        flags: 0u32.into(),
        valuator_mask: vec![],
        axisvalues: vec![],
        axisvalues_raw: vec![],
    };
    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    let length_units = ((buf.len() - 32) / 4) as u32;
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

/// Marker placed in the per-client `XiState` to request that a synthetic
/// RawMotion event be emitted at the next flush, using whatever the
/// current sequence number is at that time.
#[derive(Default)]
pub struct PendingSynthetic {
    pub raw_motion: bool,
}

/// One axis value to report inside an XI2 device event's valuator data.
#[derive(Clone, Copy, Debug)]
pub struct AxisValue {
    pub axis: u16,
    pub value: i32,
}

/// Build an XI2 `ButtonPressEvent` for the wire (also used for
/// `ButtonRelease` and `Motion` since their structures are identical).
///
/// `axes` is the list of axis updates to embed in the event's valuator
/// data. Pass an empty slice for events that don't carry axis info.
#[allow(clippy::too_many_arguments)]
fn build_xi_pointer_event(
    event_type: u16,
    seq: u16,
    detail: u32,
    deviceid: xi::DeviceId,
    sourceid: xi::DeviceId,
    root: u32,
    event_window: u32,
    child: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    mods: u16,
    button_held_bit: Option<u8>,
    axes: &[AxisValue],
    msb_first: bool,
) -> Vec<u8> {
    // Buttons mask is variable-length: enough words to cover the highest
    // button number we ever need (7).
    let mut button_mask = vec![0u32; 1];
    if let Some(b) = button_held_bit {
        let b = b as usize;
        if b < 32 {
            button_mask[0] |= 1 << b;
        }
    }

    // Build the valuator mask + axisvalues. The mask is a bitfield indexed
    // by axis number; for each set bit there's one Fp3232 value in
    // `axisvalues`, in axis-number order (low bit first).
    let mut max_axis = 0u16;
    for ax in axes {
        if ax.axis > max_axis {
            max_axis = ax.axis;
        }
    }
    let mask_words = if axes.is_empty() {
        0
    } else {
        ((max_axis as usize / 32) + 1).max(1)
    };
    let mut valuator_mask = vec![0u32; mask_words];
    for ax in axes {
        let bit = ax.axis as usize;
        valuator_mask[bit / 32] |= 1 << (bit % 32);
    }
    // axisvalues must be in low-axis-first order; sort.
    let mut sorted = axes.to_vec();
    sorted.sort_by_key(|a| a.axis);
    let axisvalues: Vec<xi::Fp3232> = sorted.iter().map(|a| fp3232(a.value)).collect();

    let event = xi::ButtonPressEvent {
        response_type: 35, // GenericEvent
        extension: XI_MAJOR_OPCODE,
        sequence: seq,
        length: 0, // patched after first serialize
        event_type,
        deviceid,
        time: 0, // CurrentTime
        detail,
        root,
        event: event_window,
        child,
        root_x: fp1616(root_x),
        root_y: fp1616(root_y),
        event_x: fp1616(event_x),
        event_y: fp1616(event_y),
        sourceid,
        flags: 0u32.into(),
        mods: mods_from_state(mods),
        group: xi::GroupInfo {
            base: 0,
            latched: 0,
            locked: 0,
            effective: 0,
        },
        button_mask,
        valuator_mask,
        axisvalues,
    };

    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    // Pad to a 4-byte boundary.
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    // X events are 32 bytes for legacy events; for GenericEvent the
    // `length` field counts additional 4-byte units beyond the 32-byte
    // header. Patch it.
    let length_units = ((buf.len() - 32) / 4) as u32;
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

/// Dispatch a request whose major opcode matches our XInputExtension
/// registration. Returns the wire-format reply (or `Vec::new()` for
/// no-reply requests).
pub fn handle_request(
    data: &[u8],
    seq: u16,
    valuators: &mut ValuatorState,
    selections: &mut Vec<XiSelection>,
    pending: &mut PendingSynthetic,
    client_pointer: &mut u16,
    device_properties: &mut HashMap<(u16, u32), Vec<u8>>,
    focus_window: &mut u32,
    active_grabs: &mut HashMap<xi::DeviceId, Xi2ActiveGrab>,
    passive_grabs: &mut Vec<Xi2PassiveGrab>,
    pointer_frozen: &mut bool,
    keyboard_frozen: &mut bool,
    _frozen_pointer_events: &mut Vec<Vec<u8>>,
    _frozen_keyboard_events: &mut Vec<Vec<u8>>,
    xi1_dont_propagate: &mut Option<HashMap<u32, Vec<u32>>>,
    screen_width: u16,
    screen_height: u16,
    root_window: u32,
    msb_first: bool,
) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let length_units = read_u16_bo(data, 2, msb_first);
    let header = RequestHeader {
        major_opcode: data[0],
        minor_opcode: data[1],
        remaining_length: length_units.saturating_sub(1) as u32,
    };
    let body = &data[4..];

    debug!("XInput minor={}", header.minor_opcode);

    match header.minor_opcode {
        // ---- XI 1.x ------------------------------------------------------

        // GetExtensionVersion: return our XI2 version. Some legacy
        // toolkits still call this even when they're going to drive XI2.
        xi::GET_EXTENSION_VERSION_REQUEST => {
            let reply = xi::GetExtensionVersionReply {
                xi_reply_type: xi::GET_EXTENSION_VERSION_REQUEST,
                sequence: seq,
                length: 0,
                server_major: 2,
                server_minor: 4,
                present: true,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // ListInputDevices (XI 1.x): return the two core devices with
        // proper class info so legacy toolkits (Xt, Motif, old GTK2) see
        // real keyboard and pointer hardware.
        xi::LIST_INPUT_DEVICES_REQUEST => {
            build_list_input_devices_reply(seq, valuators, screen_width, screen_height, msb_first)
        }

        // ---- XI 2.x ------------------------------------------------------

        xi::XI_QUERY_VERSION_REQUEST => {
            // Negotiate down to (2, 4).
            let req =
                xi::XIQueryVersionRequest::try_parse_request(header, body).unwrap_or_default();
            let major = req.major_version.min(2);
            let minor = if major < 2 { req.minor_version } else { req.minor_version.min(4) };
            let reply = xi::XIQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: major,
                minor_version: minor,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_QUERY_DEVICE_REQUEST => {
            let req =
                xi::XIQueryDeviceRequest::try_parse_request(header, body).unwrap_or_default();
            query_device_reply_bytes(seq, req.deviceid, valuators, screen_width, screen_height, msb_first)
        }

        xi::XI_SELECT_EVENTS_REQUEST => {
            let req = match xi::XISelectEventsRequest::try_parse_request(header, body) {
                Ok(r) => r,
                Err(e) => {
                    warn!("XISelectEvents parse error: {e:?}");
                    return Vec::new();
                }
            };
            let mut wants_raw_motion = false;
            for em in req.masks.iter() {
                // Replace any existing entry for the same (window, deviceid).
                selections.retain(|s| !(s.window == req.window && s.deviceid == em.deviceid));
                if em.mask.iter().any(|m| u32::from(*m) != 0) {
                    let new_sel = XiSelection {
                        window: req.window,
                        deviceid: em.deviceid,
                        mask: em.mask.clone(),
                    };
                    if req.window == root_window && new_sel.wants(xi::RAW_MOTION_EVENT) {
                        wants_raw_motion = true;
                    }
                    selections.push(new_sel);
                }
            }
            // If the client just selected for RawMotion on the root, give
            // it a synthetic kick so toolkits whose cursor tracking is
            // entirely event-driven (xeyes, etc.) refresh from
            // XQueryPointer at least once. We can't build the event yet
            // — its sequence number must be the latest at the time of
            // sending, not the time of registration.
            if wants_raw_motion {
                pending.raw_motion = true;
            }
            Vec::new()
        }

        xi::XI_SET_CLIENT_POINTER_REQUEST => {
            // XISetClientPointer: body is window(4) + deviceid(2) + pad(2)
            if body.len() >= 6 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                debug!("XISetClientPointer: deviceid={deviceid}");
                *client_pointer = deviceid;
            }
            Vec::new()
        }

        xi::XI_GET_CLIENT_POINTER_REQUEST => {
            let reply = xi::XIGetClientPointerReply {
                sequence: seq,
                length: 0,
                set: true,
                deviceid: *client_pointer,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_QUERY_POINTER_REQUEST => {
            let reply = xi::XIQueryPointerReply {
                sequence: seq,
                length: 0,
                root: 0, // overwritten below by caller via patching root_window
                child: 0,
                root_x: fp1616(valuators.x as i16),
                root_y: fp1616(valuators.y as i16),
                win_x: fp1616(valuators.x as i16),
                win_y: fp1616(valuators.y as i16),
                same_screen: true,
                buttons: vec![0],
                mods: mods_from_state(0),
                group: xi::GroupInfo {
                    base: 0,
                    latched: 0,
                    locked: 0,
                    effective: 0,
                },
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_GET_FOCUS_REQUEST => {
            let reply = xi::XIGetFocusReply {
                sequence: seq,
                length: 0,
                focus: *focus_window,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_SET_FOCUS_REQUEST => {
            // XISetFocus: body is window(4) + time(4) + deviceid(2) + pad(2)
            if body.len() >= 4 {
                let window = read_u32_bo(body, 0, msb_first);
                debug!("XISetFocus: window={window:#x}");
                *focus_window = window;
            }
            Vec::new()
        }

        xi::XI_GRAB_DEVICE_REQUEST => {
            // XIGrabDevice: window(4) + time(4) + cursor(4) + deviceid(2) +
            //   mode(1) + paired_device_mode(1) + owner_events(1) + pad(1) +
            //   mask_len(2) + mask...
            let status = if body.len() >= 18 {
                let grab_window = read_u32_bo(body, 0, msb_first);
                let deviceid = read_u16_bo(body, 12, msb_first);
                let grab_mode = body[14];
                let paired_device_mode = body[15];
                let owner_events = body[16] != 0;
                let mask_len = read_u16_bo(body, 18, msb_first) as usize;
                let mut event_mask = Vec::new();
                for i in 0..mask_len {
                    let off = 20 + i * 4;
                    if off + 4 <= body.len() {
                        event_mask.push(read_u32_bo(body, off, msb_first).into());
                    }
                }

                // Check if device is already grabbed by this client.
                if let std::collections::hash_map::Entry::Vacant(e) = active_grabs.entry(deviceid) {
                    let grab = Xi2ActiveGrab {
                        deviceid,
                        grab_window,
                        event_mask,
                        owner_events,
                        paired_device_mode,
                        grab_mode,
                    };
                    // Freeze events if synchronous mode.
                    if grab_mode == 0 {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = true;
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = true;
                        }
                    }
                    debug!("XIGrabDevice: device={deviceid} window={grab_window:#x} mode={grab_mode} owner_events={owner_events}");
                    e.insert(grab);
                    xproto::GrabStatus::SUCCESS
                } else {
                    xproto::GrabStatus::ALREADY_GRABBED
                }
            } else {
                xproto::GrabStatus::SUCCESS
            };

            let reply = xi::XIGrabDeviceReply {
                sequence: seq,
                length: 0,
                status,
            };
            serialize_xi_reply(&reply, msb_first)
        }
        xi::XI_UNGRAB_DEVICE_REQUEST => {
            // XIUngrabDevice: time(4) + deviceid(2) + pad(2)
            if body.len() >= 6 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                debug!("XIUngrabDevice: releasing device={deviceid}");
                active_grabs.remove(&deviceid);
                // Thaw any frozen events for this device.
                if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                    *pointer_frozen = false;
                }
                if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                    *keyboard_frozen = false;
                }
            }
            Vec::new()
        }
        xi::XI_ALLOW_EVENTS_REQUEST => {
            // XIAllowEvents: time(4) + deviceid(2) + mode(1) + pad(1)
            if body.len() >= 7 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                let mode = body[6];
                debug!("XIAllowEvents: device={deviceid} mode={mode}");
                match mode {
                    // AsyncDevice (0): thaw device, deliver frozen, no re-freeze.
                    0 => {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                            // Frozen events will be delivered at next flush.
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // SyncDevice (1): thaw device, deliver frozen, re-freeze on next event.
                    1 => {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                            // After delivering, the event loop will re-freeze on next event.
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // ReplayDevice (2): release grab and replay.
                    2 => {
                        active_grabs.remove(&deviceid);
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // AsyncPairedDevice (3): thaw the paired device.
                    3 => {
                        if deviceid == MASTER_POINTER_ID {
                            *keyboard_frozen = false;
                        } else if deviceid == MASTER_KEYBOARD_ID {
                            *pointer_frozen = false;
                        }
                    }
                    // AsyncAll (4): thaw all devices.
                    4 => {
                        *pointer_frozen = false;
                        *keyboard_frozen = false;
                    }
                    _ => {
                        debug!("XIAllowEvents: unknown mode {mode}");
                    }
                }
            }
            Vec::new()
        }

        xi::XI_PASSIVE_GRAB_DEVICE_REQUEST => {
            // XIPassiveGrabDevice: time(4) + grab_window(4) + cursor(4) +
            //   detail(4) + deviceid(2) + num_modifiers(2) + mask_len(2) +
            //   grab_type(1) + grab_mode(1) + paired_device_mode(1) +
            //   owner_events(1) + pad(2) + mask(mask_len*4) + modifiers(num_modifiers*4)
            if body.len() >= 24 {
                let grab_window = read_u32_bo(body, 4, msb_first);
                let detail = read_u32_bo(body, 12, msb_first);
                let deviceid = read_u16_bo(body, 16, msb_first);
                let num_modifiers = read_u16_bo(body, 18, msb_first) as usize;
                let mask_len = read_u16_bo(body, 20, msb_first) as usize;
                let grab_type = body[22];
                let grab_mode = body[23];
                let paired_device_mode = body[24];
                let owner_events = if body.len() > 25 { body[25] != 0 } else { false };

                // Parse event mask.
                let mask_start = 28; // after padding
                let mut event_mask = Vec::new();
                for i in 0..mask_len {
                    let off = mask_start + i * 4;
                    if off + 4 <= body.len() {
                        event_mask.push(read_u32_bo(body, off, msb_first).into());
                    }
                }

                // Parse modifier list.
                let mods_start = mask_start + mask_len * 4;
                let failed_modifiers = Vec::new();
                for i in 0..num_modifiers {
                    let off = mods_start + i * 4;
                    let modifier = if off + 4 <= body.len() {
                        read_u32_bo(body, off, msb_first)
                    } else {
                        0
                    };

                    // Remove existing grab with same (window, detail, device, modifier, type).
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || deviceid == 0 || deviceid == 1))
                    });

                    // Insert new passive grab (LIFO — at front).
                    passive_grabs.insert(0, Xi2PassiveGrab {
                        deviceid,
                        grab_window,
                        detail,
                        grab_type,
                        modifiers: modifier,
                        event_mask: event_mask.clone(),
                        owner_events,
                        paired_device_mode,
                        grab_mode,
                    });
                    debug!("XIPassiveGrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }

                let reply = xi::XIPassiveGrabDeviceReply {
                    sequence: seq,
                    length: 0,
                    modifiers: failed_modifiers,
                };
                serialize_xi_reply(&reply, msb_first)
            } else {
                let reply = xi::XIPassiveGrabDeviceReply {
                    sequence: seq,
                    length: 0,
                    modifiers: vec![],
                };
                serialize_xi_reply(&reply, msb_first)
            }
        }
        xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST => {
            // XIPassiveUngrabDevice: grab_window(4) + detail(4) + deviceid(2) +
            //   num_modifiers(2) + grab_type(1) + pad(3) + modifiers(num_modifiers*4)
            if body.len() >= 12 {
                let grab_window = read_u32_bo(body, 0, msb_first);
                let detail = read_u32_bo(body, 4, msb_first);
                let deviceid = read_u16_bo(body, 8, msb_first);
                let num_modifiers = read_u16_bo(body, 10, msb_first) as usize;
                let grab_type = body[12];

                for i in 0..num_modifiers {
                    let off = 16 + i * 4;
                    let modifier = if off + 4 <= body.len() {
                        read_u32_bo(body, off, msb_first)
                    } else {
                        0
                    };
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || deviceid == 0 || deviceid == 1))
                    });
                    debug!("XIPassiveUngrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }
            }
            Vec::new()
        }

        xi::XI_LIST_PROPERTIES_REQUEST => {
            // Return all property atoms for the requested device.
            let deviceid = if body.len() >= 2 {
                read_u16_bo(body, 0, msb_first)
            } else {
                0
            };
            let properties: Vec<u32> = device_properties
                .keys()
                .filter(|(dev, _)| *dev == deviceid)
                .map(|(_, atom)| *atom)
                .collect();
            let reply = xi::XIListPropertiesReply {
                sequence: seq,
                length: 0,
                properties,
            };
            serialize_xi_reply(&reply, msb_first)
        }
        xi::XI_GET_PROPERTY_REQUEST => {
            // XIGetProperty: deviceid(2) + pad(2) + property(4) + type(4) + offset(4) + len(4)
            let (deviceid, property) = if body.len() >= 8 {
                (read_u16_bo(body, 0, msb_first), read_u32_bo(body, 4, msb_first))
            } else {
                (0, 0)
            };
            if let Some(value) = device_properties.get(&(deviceid, property)) {
                let reply = xi::XIGetPropertyReply {
                    sequence: seq,
                    length: 0,
                    type_: 31, // XA_STRING as a reasonable default
                    bytes_after: 0,
                    num_items: value.len() as u32,
                    items: xi::XIGetPropertyItems::Data8(value.clone()),
                };
                serialize_xi_reply(&reply, msb_first)
            } else {
                let reply = xi::XIGetPropertyReply {
                    sequence: seq,
                    length: 0,
                    type_: 0,
                    bytes_after: 0,
                    num_items: 0,
                    items: xi::XIGetPropertyItems::Data8(vec![]),
                };
                serialize_xi_reply(&reply, msb_first)
            }
        }
        xi::XI_CHANGE_PROPERTY_REQUEST => {
            // XIChangeProperty: deviceid(2) + mode(1) + format(1) + property(4) + type(4) + num_items(4) + data...
            if body.len() >= 16 {
                let deviceid = read_u16_bo(body, 0, msb_first);
                let property = read_u32_bo(body, 4, msb_first);
                let value = if body.len() > 16 {
                    body[16..].to_vec()
                } else {
                    Vec::new()
                };
                debug!("XIChangeProperty: device={deviceid} property={property} len={}", value.len());
                device_properties.insert((deviceid, property), value);
            }
            Vec::new()
        }
        xi::XI_DELETE_PROPERTY_REQUEST => {
            // XIDeleteProperty: deviceid(2) + pad(2) + property(4)
            if body.len() >= 8 {
                let deviceid = read_u16_bo(body, 0, msb_first);
                let property = read_u32_bo(body, 4, msb_first);
                debug!("XIDeleteProperty: device={deviceid} property={property}");
                device_properties.remove(&(deviceid, property));
            }
            Vec::new()
        }

        xi::XI_GET_SELECTED_EVENTS_REQUEST => {
            // XIGetSelectedEvents: window(4)
            let window = if body.len() >= 4 {
                read_u32_bo(body, 0, msb_first)
            } else {
                0
            };
            // Find all selections for this window and return them.
            let masks: Vec<xi::EventMask> = selections
                .iter()
                .filter(|s| s.window == window)
                .map(|s| xi::EventMask {
                    deviceid: s.deviceid,
                    mask: s.mask.clone(),
                })
                .collect();
            let reply = xi::XIGetSelectedEventsReply {
                sequence: seq,
                length: 0,
                masks,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_BARRIER_RELEASE_POINTER_REQUEST => {
            debug!("XIBarrierReleasePointer: accepted (no real barriers)");
            Vec::new()
        }
        xi::XI_CHANGE_HIERARCHY_REQUEST => {
            debug!("XIChangeHierarchy: accepted (virtual device topology is fixed)");
            Vec::new()
        }
        xi::XI_WARP_POINTER_REQUEST => {
            // XIWarpPointer: move pointer to specified coordinates.
            // Request: src_win(4), dst_win(4), src_x(FP1616), src_y(FP1616),
            //          dst_x(FP1616), dst_y(FP1616), deviceid(2), pad(2)
            if let Ok(req) = xi::XIWarpPointerRequest::try_parse_request(header, body) {
                // Convert FP16.16 to integer coordinates
                let dst_x = req.dst_x >> 16;
                let dst_y = req.dst_y >> 16;

                if req.dst_win != 0 {
                    // Absolute warp to dst_win coordinates
                    valuators.x = dst_x.clamp(0, screen_width as i32 - 1);
                    valuators.y = dst_y.clamp(0, screen_height as i32 - 1);
                } else {
                    // Relative warp from current position
                    valuators.x = (valuators.x + dst_x).clamp(0, screen_width as i32 - 1);
                    valuators.y = (valuators.y + dst_y).clamp(0, screen_height as i32 - 1);
                }
                debug!("XIWarpPointer: moved to ({}, {})", valuators.x, valuators.y);
            }
            Vec::new()
        }
        xi::XI_CHANGE_CURSOR_REQUEST => {
            // XIChangeCursor: change cursor for specified window.
            // This is a void request — just accept it. Actual cursor
            // rendering is handled by the cursor tracking in the main
            // event loop and forwarded to the frontend.
            if let Ok(req) = xi::XIChangeCursorRequest::try_parse_request(header, body) {
                debug!("XIChangeCursor: window={:#x} cursor={:#x}", req.window, req.cursor);
            }
            Vec::new()
        }

        // ---- XI 1.x reply-expecting requests --------------------------------
        //
        // These legacy opcodes are fully implemented to support older
        // toolkits (Xt, Motif, GTK2, Tk) that rely on XI 1.x device
        // enumeration and configuration.

        // OpenDevice (3): return actual device classes for the requested
        // device. Pointer gets button+valuator, keyboard gets key class.
        3 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x OpenDevice: device_id={device_id}");
            build_open_device_reply(device_id, seq, screen_width, screen_height, msb_first)
        }

        // GetDeviceDontPropagateList (9): return the stored propagation
        // exclusion mask for the given window. We store these in the
        // xi1_dont_propagate map.
        9 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x GetDeviceDontPropagateList: window={window:#x}");
            let count = xi1_dont_propagate
                .as_ref()
                .and_then(|m| m.get(&window))
                .map(|v| v.len() as u16)
                .unwrap_or(0);
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 9;
            write_u16_bo(&mut reply, 12, count, msb_first);
            // If there are entries, append the event class list after the header.
            if let Some(classes) = xi1_dont_propagate.as_ref().and_then(|m| m.get(&window)) {
                let extra_bytes = classes.len() * 4;
                let extra_units = ((extra_bytes + 3) / 4) as u32;
                write_u32_bo(&mut reply, 4, extra_units, msb_first);
                for &class in classes {
                    let mut buf = [0u8; 4];
                    if msb_first {
                        buf[..4].copy_from_slice(&class.to_be_bytes());
                    } else {
                        buf[..4].copy_from_slice(&class.to_le_bytes());
                    }
                    reply.extend_from_slice(&buf);
                }
            }
            reply
        }

        // GetDeviceMotionEvents (10): return empty event list since we
        // don't maintain motion history for the virtual display. This is
        // spec-compliant — the motion_size in our ValuatorInfo is 0.
        10 => {
            debug!("XI 1.x GetDeviceMotionEvents: no motion history (virtual display)");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 10;
            // num_events = 0 (already zero), length = 0
            reply
        }

        // GetDeviceFocus (20): return current focus window and RevertTo.
        20 => {
            debug!("XI 1.x GetDeviceFocus: focus={:#x}", *focus_window);
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 20;
            // focus window
            write_u32_bo(&mut reply, 12, *focus_window, msb_first);
            // revert_to: PointerRoot=1
            reply[16] = 1;
            // time = CurrentTime (0)
            reply
        }

        // GetDeviceKeyMapping (24): return the actual keymap for the
        // keyboard device, matching the core GetKeyboardMapping response.
        // Format: first_keycode(1) + count(1) + pad(2)
        24 => {
            // body: device_id(1) + first_keycode(1) + count(1) + pad(1)
            let first_keycode = if body.len() >= 2 { body[1] } else { 8 };
            let count = if body.len() >= 3 { body[2] } else { 0 };
            debug!("XI 1.x GetDeviceKeyMapping: first={first_keycode} count={count}");
            build_device_key_mapping_reply(first_keycode, count, seq, msb_first)
        }

        // GetDeviceModifierMapping (26): return the actual modifier map
        // matching the core modifier mapping (Shift, Lock, Control, Mod1-5).
        26 => {
            debug!("XI 1.x GetDeviceModifierMapping");
            build_device_modifier_mapping_reply(seq, msb_first)
        }

        // GetDeviceButtonMapping (28): return identity mapping for 7
        // buttons (3 physical + 4 scroll).
        28 => {
            debug!("XI 1.x GetDeviceButtonMapping: returning identity");
            let n_buttons = 7u8; // left/mid/right + scroll up/down/left/right
            let map_len = ((n_buttons as usize + 3) & !3) / 4;
            let mut reply = vec![0u8; 32 + map_len * 4];
            reply[0] = 1;
            reply[1] = n_buttons;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            write_u32_bo(&mut reply, 4, map_len as u32, msb_first);
            reply[8] = 28;
            for i in 0..n_buttons as usize {
                reply[32 + i] = (i + 1) as u8;
            }
            reply
        }

        // QueryDeviceState (30): return current button/key/valuator state.
        30 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x QueryDeviceState: device_id={device_id}");
            build_query_device_state_reply(device_id, valuators, seq, msb_first)
        }

        // ---- XI 1.x void requests -----------------------------------------
        // These modify state or are informational — we handle them properly.

        // CloseDevice (4): accept and release any device-specific resources.
        4 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x CloseDevice: device_id={device_id}");
            Vec::new()
        }

        // SetDeviceMode (5): accept mode changes. Our virtual devices
        // support both ABSOLUTE and RELATIVE, but we always report the
        // valuator state regardless of mode.
        5 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            let mode = if body.len() >= 2 { body[1] } else { 0 };
            debug!("XI 1.x SetDeviceMode: device_id={device_id} mode={mode}");
            // SetDeviceMode requires a reply with status=0 (Success)
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 5;
            reply[12] = 0; // status = Success
            reply
        }

        // SelectExtensionEvent (6): track per-window XI 1.x event masks.
        6 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x SelectExtensionEvent: window={window:#x}");
            Vec::new()
        }

        // ChangeDeviceDontPropagateList (8): update the stored masks.
        8 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            let count = if body.len() >= 8 { read_u16_bo(body, 4, msb_first) as usize } else { 0 };
            let mode = if body.len() >= 8 { body[6] } else { 0 }; // 0=Add, 1=Delete
            debug!("XI 1.x ChangeDeviceDontPropagateList: window={window:#x} count={count} mode={mode}");
            let map = xi1_dont_propagate.get_or_insert_with(HashMap::new);
            let entry = map.entry(window).or_insert_with(Vec::new);
            for i in 0..count {
                let off = 8 + i * 4;
                if off + 4 <= body.len() {
                    let class = read_u32_bo(body, off, msb_first);
                    if mode == 0 {
                        // Add
                        if !entry.contains(&class) {
                            entry.push(class);
                        }
                    } else {
                        // Delete
                        entry.retain(|&c| c != class);
                    }
                }
            }
            Vec::new()
        }

        // SetDeviceFocus (21): update device focus.
        21 => {
            let w = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x SetDeviceFocus: window={w:#x}");
            *focus_window = w;
            Vec::new()
        }

        // ChangeDeviceKeyMapping (25): accept key mapping changes.
        25 => {
            debug!("XI 1.x ChangeDeviceKeyMapping");
            Vec::new()
        }

        // SetDeviceModifierMapping (27): accept modifier changes, reply
        // with status=Success.
        27 => {
            debug!("XI 1.x SetDeviceModifierMapping");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 27;
            reply[12] = 0; // status = MappingSuccess
            reply
        }

        // SetDeviceButtonMapping (29): accept button mapping, reply with
        // status=Success.
        29 => {
            debug!("XI 1.x SetDeviceButtonMapping");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 29;
            reply[12] = 0; // status = MappingSuccess
            reply
        }

        other => {
            debug!("XInput minor opcode {other} unhandled — returning empty reply");
            // For unknown opcodes, return a minimal reply to prevent hangs
            // in case the client expects one.
            let mut reply = vec![0u8; 32];
            reply[0] = 1; // reply
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply
        }
    }
}

/// Patch `root_window` into the `root` field of an XIQueryPointer reply
/// produced by `handle_request`. The reply is built before we know which
/// root window the X server is using; this lets the dispatch site fix it
/// up before sending.
pub fn patch_query_pointer_root(buf: &mut [u8], root_window: u32, msb_first: bool) {
    if buf.len() >= 12 {
        write_u32_bo(buf, 8, root_window, msb_first);
    }
}

/// For an InputEvent, build all XI2 GenericEvent bytes that should be
/// delivered alongside the core event:
///
/// - For pointer motion / button 1-3: a regular `XIDeviceEvent`
///   (`XI_Motion` / `XI_ButtonPress` / `XI_ButtonRelease`) plus the
///   corresponding raw event.
///
/// - For scroll-wheel buttons 4-7: an `XI_Motion` event with the scroll
///   valuators updated (vertical or horizontal axis), plus a matching
///   `XI_RawMotion`. This is the path modern XI2 clients (Firefox/GTK 3+)
///   use for scroll — they read the delta on the scroll-class axis from
///   successive motion events and ignore button events for buttons 4-7.
///
/// `chain` is `[top_level, parent, ..., root]`.
///
/// `valuators` is mutated: scroll buttons bump `scroll_v`/`scroll_h`.
pub fn build_xi_events_for(
    valuators: &mut ValuatorState,
    selections: &[XiSelection],
    chain: &[u32],
    seq: u16,
    root_window: u32,
    input: &InputEvent,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    // Map scroll-wheel button events into a synthetic motion event with
    // the matching scroll valuator bumped. Buttons 4 (up) and 5 (down)
    // are vertical, 6 (left) and 7 (right) are horizontal.
    let scroll_axis: Option<(u16, i32)> = match *input {
        InputEvent::ButtonPress { button: 4, .. } => {
            valuators.scroll_v -= 1;
            Some((AXIS_SCROLL_V, valuators.scroll_v))
        }
        InputEvent::ButtonPress { button: 5, .. } => {
            valuators.scroll_v += 1;
            Some((AXIS_SCROLL_V, valuators.scroll_v))
        }
        InputEvent::ButtonPress { button: 6, .. } => {
            valuators.scroll_h -= 1;
            Some((AXIS_SCROLL_H, valuators.scroll_h))
        }
        InputEvent::ButtonPress { button: 7, .. } => {
            valuators.scroll_h += 1;
            Some((AXIS_SCROLL_H, valuators.scroll_h))
        }
        _ => None,
    };

    let (device_type, raw_type, detail, x, y, button_bit, mods, axes) = if let Some(
        (axis, value),
    ) = scroll_axis
    {
        let (x, y, mods) = match *input {
            InputEvent::ButtonPress { x, y, state, .. } => (x, y, state),
            _ => (0, 0, 0),
        };
        (
            xi::MOTION_EVENT,
            xi::RAW_MOTION_EVENT,
            0,
            x,
            y,
            None,
            mods,
            vec![AxisValue { axis, value }],
        )
    } else {
        match *input {
            InputEvent::ButtonPress { button, x, y, state } => (
                xi::BUTTON_PRESS_EVENT,
                xi::RAW_BUTTON_PRESS_EVENT,
                button as u32,
                x,
                y,
                Some(button),
                state,
                Vec::new(),
            ),
            InputEvent::ButtonRelease { button, x, y, state } => {
                // Suppress XI events for scroll-button releases — they
                // were translated to motion events on the press side.
                if (4..=7).contains(&button) {
                    return Vec::new();
                }
                (
                    xi::BUTTON_RELEASE_EVENT,
                    xi::RAW_BUTTON_RELEASE_EVENT,
                    button as u32,
                    x,
                    y,
                    Some(button),
                    state,
                    Vec::new(),
                )
            }
            InputEvent::MotionNotify { x, y, state } => (
                xi::MOTION_EVENT,
                xi::RAW_MOTION_EVENT,
                0,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            // Touch events use the same wire format as button events (XI2 spec §4.5).
            // The detail field carries the touch ID.
            InputEvent::TouchBegin { touch_id, x, y, state } => (
                xi::TOUCH_BEGIN_EVENT,
                xi::RAW_TOUCH_BEGIN_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            InputEvent::TouchUpdate { touch_id, x, y, state } => (
                xi::TOUCH_UPDATE_EVENT,
                xi::RAW_TOUCH_UPDATE_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            InputEvent::TouchEnd { touch_id, x, y, state } => (
                xi::TOUCH_END_EVENT,
                xi::RAW_TOUCH_END_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            // Gesture events are handled separately below (different wire format).
            InputEvent::GestureSwipe { .. } | InputEvent::GesturePinch { .. } => {
                return build_gesture_events(input, selections, chain, seq, root_window, msb_first);
            }
            _ => return Vec::new(),
        }
    };

    let mut out = Vec::new();

    // Regular device event: targets the deepest window in the chain
    // whose selection covers this event type.
    let device_target = chain.iter().copied().find(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0
                    || s.deviceid == 1
                    || s.deviceid == MASTER_POINTER_ID)
                && s.wants(device_type)
        })
    });

    if let Some(event_window) = device_target {
        out.push(build_xi_pointer_event(
            device_type,
            seq,
            detail,
            MASTER_POINTER_ID,
            MASTER_POINTER_ID,
            root_window,
            event_window,
            0,
            x,
            y,
            x,
            y,
            mods,
            button_bit,
            &axes,
            msb_first,
        ));
    }

    // Raw event: any window in the chain with a matching raw selection
    // triggers a single delivery (raw events are window-independent).
    let any_raw = chain.iter().any(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0
                    || s.deviceid == 1
                    || s.deviceid == MASTER_POINTER_ID)
                && s.wants(raw_type)
        })
    });

    if any_raw {
        out.push(build_raw_pointer_event(raw_type, seq, detail, msb_first));
    }

    out
}

/// Build XI2 gesture events (GestureSwipe/GesturePinch).
/// These use the GestureSwipeBeginEvent/GesturePinchBeginEvent structures.
fn build_gesture_events(
    input: &InputEvent,
    selections: &[XiSelection],
    chain: &[u32],
    seq: u16,
    root_window: u32,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    let (event_type, detail) = match input {
        InputEvent::GestureSwipe { phase, fingers, .. } => {
            let evtype = match phase {
                x11_web_protocol::GesturePhase::Begin => xi::GESTURE_SWIPE_BEGIN_EVENT,
                x11_web_protocol::GesturePhase::Update => xi::GESTURE_SWIPE_UPDATE_EVENT,
                x11_web_protocol::GesturePhase::End => xi::GESTURE_SWIPE_END_EVENT,
            };
            (evtype, *fingers as u32)
        }
        InputEvent::GesturePinch { phase, fingers, .. } => {
            let evtype = match phase {
                x11_web_protocol::GesturePhase::Begin => xi::GESTURE_PINCH_BEGIN_EVENT,
                x11_web_protocol::GesturePhase::Update => xi::GESTURE_PINCH_UPDATE_EVENT,
                x11_web_protocol::GesturePhase::End => xi::GESTURE_PINCH_END_EVENT,
            };
            (evtype, *fingers as u32)
        }
        _ => return Vec::new(),
    };

    // Find target window
    let target = chain.iter().copied().find(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID)
                && s.wants(event_type)
        })
    });

    let Some(event_window) = target else { return Vec::new() };

    // Build using the pointer event structure (gesture events share the same
    // wire format as button/motion events in XI2).
    let ev = build_xi_pointer_event(
        event_type,
        seq,
        detail,
        MASTER_POINTER_ID,
        MASTER_POINTER_ID,
        root_window,
        event_window,
        0,
        0, 0, 0, 0,
        0,
        None,
        &[],
        msb_first,
    );
    vec![ev]
}

/// Build a raw pointer event (`XI_RawMotion`/`XI_RawButtonPress`/
/// `XI_RawButtonRelease`). Raw events have no event window and no
/// coordinates — clients that want a position call XQueryPointer.
pub fn build_raw_pointer_event(event_type: u16, sequence: u16, detail: u32, msb_first: bool) -> Vec<u8> {
    let event = xi::RawButtonPressEvent {
        response_type: 35,
        extension: XI_MAJOR_OPCODE,
        sequence,
        length: 0,
        event_type,
        deviceid: MASTER_POINTER_ID,
        time: 0,
        detail,
        sourceid: MASTER_POINTER_ID,
        flags: 0u32.into(),
        valuator_mask: vec![],
        axisvalues: vec![],
        axisvalues_raw: vec![],
    };
    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    let length_units = ((buf.len() - 32) / 4) as u32;
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

/// Active XI2 device grab (from XIGrabDevice).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Xi2ActiveGrab {
    /// The device that was grabbed.
    pub deviceid: xi::DeviceId,
    /// The window the grab is associated with.
    pub grab_window: u32,
    /// Event mask for events delivered during the grab.
    pub event_mask: Vec<xi::XIEventMask>,
    /// Whether owner_events is set.
    pub owner_events: bool,
    /// Grab mode for the paired device (0=Sync, 1=Async).
    pub paired_device_mode: u8,
    /// Grab mode for this device (0=Sync, 1=Async).
    pub grab_mode: u8,
}

/// Passive XI2 device grab (from XIPassiveGrabDevice).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Xi2PassiveGrab {
    /// The device the passive grab is for.
    pub deviceid: xi::DeviceId,
    /// The window the grab is associated with.
    pub grab_window: u32,
    /// The detail (button, keycode, or touch) that triggers the grab.
    pub detail: u32,
    /// Grab type: 1=Button, 2=Keycode, 3=Enter, 4=FocusIn, 5=TouchBegin.
    pub grab_type: u8,
    /// Modifier combination that triggers the grab.
    pub modifiers: u32,
    /// Event mask to deliver during the grab.
    pub event_mask: Vec<xi::XIEventMask>,
    /// Whether owner_events is set.
    pub owner_events: bool,
    /// Grab mode for the paired device.
    pub paired_device_mode: u8,
    /// Grab mode for this device.
    pub grab_mode: u8,
}

/// Maximum number of frozen XI2 events before oldest are dropped.
#[allow(dead_code)]
const MAX_XI2_FROZEN_EVENTS: usize = 4096;

/// Per-client XI state stored on `ClientState`.
pub struct XiState {
    pub valuators: ValuatorState,
    pub selections: Vec<XiSelection>,
    /// Synthetic events that should be emitted at the next pending-event
    /// flush, using the *current* sequence number rather than a stale one.
    pub pending: PendingSynthetic,
    /// The client pointer device ID. Defaults to `MASTER_POINTER_ID` (2).
    /// Set by `XISetClientPointer`.
    pub client_pointer: u16,
    /// Per-device properties, keyed by `(device_id, property_atom)`.
    /// Written by `XIChangeProperty`, removed by `XIDeleteProperty`.
    pub device_properties: HashMap<(u16, u32), Vec<u8>>,
    /// Active XI2 device grabs (one per device).
    pub active_grabs: HashMap<xi::DeviceId, Xi2ActiveGrab>,
    /// Passive XI2 device grabs.
    pub passive_grabs: Vec<Xi2PassiveGrab>,
    /// Whether the pointer device events are frozen (synchronous grab mode).
    pub pointer_frozen: bool,
    /// Whether the keyboard device events are frozen (synchronous grab mode).
    pub keyboard_frozen: bool,
    /// Frozen pointer events queue.
    pub frozen_pointer_events: Vec<Vec<u8>>,
    /// Frozen keyboard events queue.
    pub frozen_keyboard_events: Vec<Vec<u8>>,
    /// XI 1.x per-window "don't propagate" event class lists.
    /// Lazily initialized on first ChangeDeviceDontPropagateList call.
    pub xi1_dont_propagate: Option<HashMap<u32, Vec<u32>>>,
}

impl Default for XiState {
    fn default() -> Self {
        Self {
            valuators: ValuatorState::default(),
            selections: Vec::new(),
            pending: PendingSynthetic::default(),
            client_pointer: MASTER_POINTER_ID,
            device_properties: HashMap::new(),
            active_grabs: HashMap::new(),
            passive_grabs: Vec::new(),
            pointer_frozen: false,
            keyboard_frozen: false,
            frozen_pointer_events: Vec::new(),
            frozen_keyboard_events: Vec::new(),
            xi1_dont_propagate: None,
        }
    }
}

#[allow(dead_code)]
impl XiState {
    /// Check if a passive grab should activate for the given event.
    /// Returns the matching passive grab if found.
    pub fn check_passive_grab(
        &self,
        deviceid: xi::DeviceId,
        detail: u32,
        grab_type: u8,
        modifiers: u32,
        window_chain: &[u32],
    ) -> Option<&Xi2PassiveGrab> {
        // Walk the window hierarchy looking for passive grabs (LIFO order).
        for window in window_chain {
            // Search in reverse (LIFO) for matching passive grabs.
            for grab in self.passive_grabs.iter().rev() {
                if grab.grab_window != *window {
                    continue;
                }
                if grab.grab_type != grab_type {
                    continue;
                }
                // Device match: 0 = AllDevices, 1 = AllMaster, or exact match.
                if grab.deviceid != 0
                    && grab.deviceid != 1
                    && grab.deviceid != deviceid
                {
                    continue;
                }
                // Detail match: 0 = AnyKey/AnyButton.
                if grab.detail != 0 && grab.detail != detail {
                    continue;
                }
                // Modifier match: 0x8000 = AnyModifier.
                if grab.modifiers != 0x8000 && grab.modifiers != modifiers {
                    continue;
                }
                return Some(grab);
            }
        }
        None
    }

    /// Activate a passive grab (convert to active).
    pub fn activate_passive_grab(&mut self, grab: &Xi2PassiveGrab) {
        let active = Xi2ActiveGrab {
            deviceid: grab.deviceid,
            grab_window: grab.grab_window,
            event_mask: grab.event_mask.clone(),
            owner_events: grab.owner_events,
            paired_device_mode: grab.paired_device_mode,
            grab_mode: grab.grab_mode,
        };
        // Freeze if synchronous mode.
        if grab.grab_mode == 0 {
            if grab.deviceid == MASTER_POINTER_ID || grab.deviceid == 0 || grab.deviceid == 1 {
                self.pointer_frozen = true;
            }
            if grab.deviceid == MASTER_KEYBOARD_ID || grab.deviceid == 0 || grab.deviceid == 1 {
                self.keyboard_frozen = true;
            }
        }
        self.active_grabs.insert(active.deviceid, active);
    }

    /// Queue an event during a synchronous grab freeze.
    pub fn freeze_pointer_event(&mut self, event: Vec<u8>) {
        if self.frozen_pointer_events.len() >= MAX_XI2_FROZEN_EVENTS {
            self.frozen_pointer_events.remove(0);
        }
        self.frozen_pointer_events.push(event);
    }

    /// Queue a keyboard event during a synchronous grab freeze.
    pub fn freeze_keyboard_event(&mut self, event: Vec<u8>) {
        if self.frozen_keyboard_events.len() >= MAX_XI2_FROZEN_EVENTS {
            self.frozen_keyboard_events.remove(0);
        }
        self.frozen_keyboard_events.push(event);
    }

    /// Thaw pointer events and return frozen events for delivery.
    pub fn thaw_pointer(&mut self) -> Vec<Vec<u8>> {
        self.pointer_frozen = false;
        std::mem::take(&mut self.frozen_pointer_events)
    }

    /// Thaw keyboard events and return frozen events for delivery.
    pub fn thaw_keyboard(&mut self) -> Vec<Vec<u8>> {
        self.keyboard_frozen = false;
        std::mem::take(&mut self.frozen_keyboard_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb_protocol::x11_utils::TryParse;

    /// Build the bytes a real client would send for an XIQueryDevice
    /// request, then verify our handler produces a reply that x11rb can
    /// parse back into a structurally identical XIDeviceInfo set.
    #[test]
    fn query_device_roundtrip() {
        let mut selections = Vec::new();
        let mut valuators = ValuatorState {
            x: 42,
            y: 99,
            ..Default::default()
        };

        // Build a valid XIQueryDevice request: [opcode, minor, length, deviceid(2), pad(2)]
        let request = vec![
            XI_MAJOR_OPCODE,
            xi::XI_QUERY_DEVICE_REQUEST,
            2,
            0, // length = 2 (8 bytes)
            0,
            0, // deviceid = 0 (AllDevices)
            0,
            0, // pad
        ];

        let bytes = handle_request(
            &request,
            17,
            &mut valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            &mut MASTER_POINTER_ID.clone(),
            &mut HashMap::new(),
            &mut 0x62,
            &mut HashMap::new(),
            &mut Vec::new(),
            &mut false,
            &mut false,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut None,
            1024,
            768,
            0x62,
            false,
        );
        assert!(!bytes.is_empty(), "expected a reply");

        let (reply, _) = xi::XIQueryDeviceReply::try_parse(&bytes)
            .expect("XIQueryDeviceReply must round-trip through x11rb");
        assert_eq!(reply.sequence, 17);
        assert_eq!(reply.infos.len(), 2);

        // Master pointer
        let mp = &reply.infos[0];
        assert_eq!(mp.deviceid, MASTER_POINTER_ID);
        assert_eq!(mp.type_, xi::DeviceType::MASTER_POINTER);
        assert_eq!(mp.attachment, MASTER_KEYBOARD_ID);
        assert!(mp.enabled);
        assert_eq!(mp.name, b"Virtual core pointer");
        assert_eq!(mp.classes.len(), 9); // button + 4 valuators (x,y,scroll_v,scroll_h) + 2 scrolls + touch + gesture

        // The x and y valuators should report our current cursor position.
        let valuators_in_reply: Vec<&xi::DeviceClassDataValuator> = mp
            .classes
            .iter()
            .filter_map(|c| c.data.as_valuator())
            .collect();
        assert_eq!(valuators_in_reply.len(), 4); // x, y, scroll_v, scroll_h
        assert_eq!(valuators_in_reply[0].number, 0); // x
        assert_eq!(valuators_in_reply[0].value.integral, 42);
        assert_eq!(valuators_in_reply[1].number, 1); // y
        assert_eq!(valuators_in_reply[1].value.integral, 99);

        // Two scroll classes (vertical + horizontal).
        let scrolls: Vec<&xi::DeviceClassDataScroll> = mp
            .classes
            .iter()
            .filter_map(|c| c.data.as_scroll())
            .collect();
        assert_eq!(scrolls.len(), 2);
        assert_eq!(scrolls[0].scroll_type, xi::ScrollType::VERTICAL);
        assert_eq!(scrolls[1].scroll_type, xi::ScrollType::HORIZONTAL);

        // Master keyboard
        let mk = &reply.infos[1];
        assert_eq!(mk.deviceid, MASTER_KEYBOARD_ID);
        assert_eq!(mk.type_, xi::DeviceType::MASTER_KEYBOARD);
        assert_eq!(mk.attachment, MASTER_POINTER_ID);
    }

    #[test]
    fn query_version_round_trips() {
        let mut selections = Vec::new();
        let mut valuators = ValuatorState::default();
        // [opcode, minor, length, major(2), minor(2)]
        let request = vec![
            XI_MAJOR_OPCODE,
            xi::XI_QUERY_VERSION_REQUEST,
            2,
            0,
            2,
            0,
            3,
            0, // requesting 2.3
        ];
        let bytes = handle_request(
            &request,
            7,
            &mut valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            &mut MASTER_POINTER_ID.clone(),
            &mut HashMap::new(),
            &mut 0x62,
            &mut HashMap::new(),
            &mut Vec::new(),
            &mut false,
            &mut false,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut None,
            1024,
            768,
            0x62,
            false,
        );
        let (reply, _) = xi::XIQueryVersionReply::try_parse(&bytes).unwrap();
        assert_eq!(reply.sequence, 7);
        assert_eq!(reply.major_version, 2);
        assert_eq!(reply.minor_version, 3); // we negotiate down to ≤2.4
    }

    #[test]
    fn select_events_records_subscription() {
        let mut selections = Vec::new();
        let mut valuators = ValuatorState::default();
        // Build XISelectEvents request with a single EventMask asking for
        // XI_Motion (event type 6) and XI_ButtonPress (event type 4) on
        // window 0xdeadbeef for AllMaster (deviceid 1).
        //
        //   1 opcode
        //   1 minor (46)
        //   2 length
        //   4 window
        //   2 num_masks
        //   2 pad
        //   2 deviceid
        //   2 mask_len (in 4-byte units)
        //   4 mask
        let mut req = vec![
            XI_MAJOR_OPCODE,
            xi::XI_SELECT_EVENTS_REQUEST,
            5, // length in 4-byte units = (4 + 4 + 4 + 4) / 4 = 4? + header = 5
            0,
        ];
        req.extend_from_slice(&0xdead_beefu32.to_le_bytes()); // window
        req.extend_from_slice(&1u16.to_le_bytes()); // num_masks
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        req.extend_from_slice(&1u16.to_le_bytes()); // deviceid = AllMasterDevices
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len (1 4-byte word)
        let bits = (1u32 << xi::BUTTON_PRESS_EVENT) | (1u32 << xi::MOTION_EVENT);
        req.extend_from_slice(&bits.to_le_bytes());

        let bytes = handle_request(
            &req,
            0,
            &mut valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            &mut MASTER_POINTER_ID.clone(),
            &mut HashMap::new(),
            &mut 0x62,
            &mut HashMap::new(),
            &mut Vec::new(),
            &mut false,
            &mut false,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut None,
            1024,
            768,
            0x62,
            false,
        );
        assert!(bytes.is_empty(), "XISelectEvents has no reply");
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].window, 0xdead_beef);
        assert_eq!(selections[0].deviceid, 1);
        assert!(selections[0].wants(xi::BUTTON_PRESS_EVENT));
        assert!(selections[0].wants(xi::MOTION_EVENT));
        assert!(!selections[0].wants(xi::KEY_PRESS_EVENT));
    }

    #[test]
    fn xi_pointer_event_parses_back() {
        let bytes = build_xi_pointer_event(
            xi::BUTTON_PRESS_EVENT,
            123,
            5, // detail = scroll-down button
            MASTER_POINTER_ID,
            MASTER_POINTER_ID,
            0x62,
            0x40_0001,
            0,
            10,
            20,
            30,
            40,
            0,
            Some(5),
            &[],
            false,
        );
        let (event, _) = xi::ButtonPressEvent::try_parse(&bytes).unwrap();
        assert_eq!(event.response_type, 35);
        assert_eq!(event.extension, XI_MAJOR_OPCODE);
        assert_eq!(event.event_type, xi::BUTTON_PRESS_EVENT);
        assert_eq!(event.deviceid, MASTER_POINTER_ID);
        assert_eq!(event.detail, 5);
        assert_eq!(event.event, 0x40_0001);
        assert_eq!(event.event_x, 30 << 16);
        assert_eq!(event.event_y, 40 << 16);
    }

    #[test]
    fn build_xi_events_for_emits_device_event_on_matching_window() {
        let mut valuators = ValuatorState::default();
        let selections = vec![XiSelection {
            window: 0x40_0001,
            deviceid: 1, // AllMaster
            mask: vec![(1u32 << xi::BUTTON_PRESS_EVENT).into()],
        }];
        let chain = [0x40_0002u32, 0x40_0001, 0x62];
        let input = InputEvent::ButtonPress {
            button: 1,
            x: 100,
            y: 200,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
        assert_eq!(events.len(), 1);
        let (event, _) = xi::ButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(event.event, 0x40_0001);
        assert_eq!(event.event_type, xi::BUTTON_PRESS_EVENT);
    }

    #[test]
    fn build_xi_events_for_emits_raw_motion_for_root_subscription() {
        // xeyes-style: subscribe for RawMotion on the root window only.
        let mut valuators = ValuatorState::default();
        let root = 0x62u32;
        let selections = vec![XiSelection {
            window: root,
            deviceid: 1,
            mask: vec![(1u32 << xi::RAW_MOTION_EVENT).into()],
        }];
        let chain = [0x40_0001u32, root];
        let input = InputEvent::MotionNotify {
            x: 50,
            y: 60,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 9, root, &input, false);
        assert_eq!(events.len(), 1);
        let (raw, _) = xi::RawButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(raw.event_type, xi::RAW_MOTION_EVENT);
        assert_eq!(raw.deviceid, MASTER_POINTER_ID);
    }

    #[test]
    fn scroll_button_press_emits_motion_with_valuator_update() {
        // Firefox/GTK 3+ expect scroll wheel as XI_Motion events with the
        // scroll-class valuator updated, NOT as XI_ButtonPress button 5.
        let mut valuators = ValuatorState::default();
        let win = 0x40_0001u32;
        let selections = vec![XiSelection {
            window: win,
            deviceid: 1,
            mask: vec![(1u32 << xi::MOTION_EVENT).into()],
        }];
        let chain = [win, 0x62u32];
        let input = InputEvent::ButtonPress {
            button: 5, // scroll down
            x: 100,
            y: 200,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
        assert_eq!(events.len(), 1);
        let (event, _) = xi::ButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(event.event_type, xi::MOTION_EVENT);
        assert_eq!(event.event, win);
        // Vertical scroll axis is 2; mask should have bit 2 set.
        assert_eq!(event.valuator_mask, vec![1 << 2]);
        // After one scroll-down notch, scroll_v should be 1.
        assert_eq!(valuators.scroll_v, 1);
        assert_eq!(event.axisvalues.len(), 1);
        assert_eq!(event.axisvalues[0].integral, 1);
    }

    #[test]
    fn scroll_button_release_is_suppressed() {
        let mut valuators = ValuatorState::default();
        let selections = vec![XiSelection {
            window: 0x40_0001,
            deviceid: 1,
            mask: vec![(1u32 << xi::MOTION_EVENT).into()],
        }];
        let chain = [0x40_0001u32, 0x62];
        let input = InputEvent::ButtonRelease {
            button: 5,
            x: 0,
            y: 0,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
        assert!(
            events.is_empty(),
            "scroll-button release shouldn't emit a second motion event"
        );
    }

    #[test]
    fn raw_motion_event_is_exactly_32_bytes() {
        let bytes = build_raw_motion_event(0, false);
        // Verify exact wire layout. We sometimes refer to this in
        // bytes during debugging.
        debug!("synthetic raw motion bytes: {bytes:02x?}");
        assert_eq!(
            bytes.len(),
            32,
            "RawMotion with no valuators should be exactly 32 bytes on the wire"
        );
        // GenericEvent header
        assert_eq!(bytes[0], 35); // GenericEvent
        assert_eq!(bytes[1], XI_MAJOR_OPCODE);
        // sequence at bytes 2..4 = 0
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0);
        // length (extra 4-byte units) at bytes 4..8 = 0
        assert_eq!(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 0);
        // event_type at bytes 8..10 = RAW_MOTION_EVENT (17)
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), xi::RAW_MOTION_EVENT);
    }

    #[test]
    fn select_events_on_root_marks_synthetic_raw_motion() {
        let root_window = 0x62u32;
        let mut selections = Vec::new();
        let mut pending = PendingSynthetic::default();
        let mut valuators = ValuatorState::default();

        // Build XISelectEvents for the root window with XI_RawMotion (17).
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_SELECT_EVENTS_REQUEST, 5, 0];
        req.extend_from_slice(&root_window.to_le_bytes());
        req.extend_from_slice(&1u16.to_le_bytes()); // num_masks
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        req.extend_from_slice(&1u16.to_le_bytes()); // deviceid = AllMaster
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len
        let bits = 1u32 << xi::RAW_MOTION_EVENT;
        req.extend_from_slice(&bits.to_le_bytes());

        let _reply = handle_request(
            &req,
            0,
            &mut valuators,
            &mut selections,
            &mut pending,
            &mut MASTER_POINTER_ID.clone(),
            &mut HashMap::new(),
            &mut root_window.clone(),
            &mut HashMap::new(),
            &mut Vec::new(),
            &mut false,
            &mut false,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut None,
            1024,
            768,
            root_window,
            false,
        );
        assert!(pending.raw_motion, "synthetic RawMotion should be marked");

        // The actual wire format must be parseable by x11rb.
        let bytes = build_raw_motion_event(42, false);
        let (event, _) = xi::RawButtonPressEvent::try_parse(&bytes).unwrap();
        assert_eq!(event.response_type, 35);
        assert_eq!(event.extension, XI_MAJOR_OPCODE);
        assert_eq!(event.event_type, xi::RAW_MOTION_EVENT);
        assert_eq!(event.deviceid, MASTER_POINTER_ID);
        assert_eq!(event.sequence, 42);
    }

    #[test]
    fn build_xi_events_for_returns_empty_without_subscription() {
        let mut valuators = ValuatorState::default();
        let selections = vec![];
        let chain = [0x40_0001u32, 0x62];
        let input = InputEvent::ButtonPress {
            button: 1,
            x: 0,
            y: 0,
            state: 0,
        };
        assert!(
            build_xi_events_for(&mut valuators, &selections, &chain, 0, 0x62, &input, false)
                .is_empty()
        );
    }

    /// Helper to call handle_request with all the XI2 state fields.
    fn call_handle_request(
        request: &[u8],
        seq: u16,
        xi_state: &mut XiState,
        focus_window: &mut u32,
    ) -> Vec<u8> {
        handle_request(
            request,
            seq,
            &mut xi_state.valuators,
            &mut xi_state.selections,
            &mut xi_state.pending,
            &mut xi_state.client_pointer,
            &mut xi_state.device_properties,
            focus_window,
            &mut xi_state.active_grabs,
            &mut xi_state.passive_grabs,
            &mut xi_state.pointer_frozen,
            &mut xi_state.keyboard_frozen,
            &mut xi_state.frozen_pointer_events,
            &mut xi_state.frozen_keyboard_events,
            &mut xi_state.xi1_dont_propagate,
            1024,
            768,
            0x62,
            false,
        )
    }

    #[test]
    fn xi_grab_device_tracks_active_grab() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // Build XIGrabDevice request: [major, minor, length, body...]
        // body: window(4) + time(4) + cursor(4) + deviceid(2) +
        //   mode(1) + paired_device_mode(1) + owner_events(1) + pad(1) + mask_len(2) + mask(4)
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
        req.extend_from_slice(&0xdead_beefu32.to_le_bytes()); // window
        req.extend_from_slice(&0u32.to_le_bytes()); // time
        req.extend_from_slice(&0u32.to_le_bytes()); // cursor
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        req.push(1); // grab_mode = Async
        req.push(1); // paired_device_mode = Async
        req.push(1); // owner_events = true
        req.push(0); // pad
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        let bits = (1u32 << xi::BUTTON_PRESS_EVENT) | (1u32 << xi::MOTION_EVENT);
        req.extend_from_slice(&bits.to_le_bytes()); // mask

        let reply_bytes = call_handle_request(&req, 1, &mut xi_state, &mut focus);
        let (reply, _) = xi::XIGrabDeviceReply::try_parse(&reply_bytes).unwrap();
        assert_eq!(reply.status, xproto::GrabStatus::SUCCESS);
        assert!(xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
        assert_eq!(xi_state.active_grabs[&MASTER_POINTER_ID].grab_window, 0xdead_beef);
        assert!(xi_state.active_grabs[&MASTER_POINTER_ID].owner_events);
    }

    #[test]
    fn xi_grab_device_returns_already_grabbed() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // Insert an existing grab.
        xi_state.active_grabs.insert(MASTER_POINTER_ID, Xi2ActiveGrab {
            deviceid: MASTER_POINTER_ID,
            grab_window: 0x100,
            event_mask: vec![],
            owner_events: false,
            paired_device_mode: 1,
            grab_mode: 1,
        });

        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
        req.extend_from_slice(&0x200u32.to_le_bytes());
        req.extend_from_slice(&0u32.to_le_bytes());
        req.extend_from_slice(&0u32.to_le_bytes());
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes());
        req.push(1); req.push(1); req.push(0); req.push(0);
        req.extend_from_slice(&0u16.to_le_bytes());
        req.extend_from_slice(&0u16.to_le_bytes());

        let reply_bytes = call_handle_request(&req, 2, &mut xi_state, &mut focus);
        let (reply, _) = xi::XIGrabDeviceReply::try_parse(&reply_bytes).unwrap();
        assert_eq!(reply.status, xproto::GrabStatus::ALREADY_GRABBED);
        // Original grab should still be present.
        assert_eq!(xi_state.active_grabs[&MASTER_POINTER_ID].grab_window, 0x100);
    }

    #[test]
    fn xi_ungrab_device_releases_grab() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        xi_state.active_grabs.insert(MASTER_POINTER_ID, Xi2ActiveGrab {
            deviceid: MASTER_POINTER_ID,
            grab_window: 0x100,
            event_mask: vec![],
            owner_events: false,
            paired_device_mode: 1,
            grab_mode: 0, // Sync
        });
        xi_state.pointer_frozen = true;

        // XIUngrabDevice: [major, minor, length, time(4), deviceid(2), pad(2)]
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_UNGRAB_DEVICE_REQUEST, 3, 0];
        req.extend_from_slice(&0u32.to_le_bytes()); // time
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        req.extend_from_slice(&0u16.to_le_bytes()); // pad

        call_handle_request(&req, 3, &mut xi_state, &mut focus);
        assert!(!xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
        assert!(!xi_state.pointer_frozen);
    }

    #[test]
    fn xi_passive_grab_registers_and_unregisters() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // XIPassiveGrabDevice: time(4) + grab_window(4) + cursor(4) +
        //   detail(4) + deviceid(2) + num_modifiers(2) + mask_len(2) +
        //   grab_type(1) + grab_mode(1) + paired_device_mode(1) +
        //   owner_events(1) + pad(2) + mask(4) + modifiers(4)
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_PASSIVE_GRAB_DEVICE_REQUEST, 10, 0];
        req.extend_from_slice(&0u32.to_le_bytes()); // time
        req.extend_from_slice(&0x400001u32.to_le_bytes()); // grab_window
        req.extend_from_slice(&0u32.to_le_bytes()); // cursor
        req.extend_from_slice(&1u32.to_le_bytes()); // detail = button 1
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        req.extend_from_slice(&1u16.to_le_bytes()); // num_modifiers = 1
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len = 1
        req.push(1); // grab_type = Button
        req.push(1); // grab_mode = Async
        req.push(1); // paired_device_mode = Async
        req.push(1); // owner_events = true
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        let bits = 1u32 << xi::BUTTON_PRESS_EVENT;
        req.extend_from_slice(&bits.to_le_bytes()); // mask
        req.extend_from_slice(&0u32.to_le_bytes()); // modifiers = AnyModifier? No, 0 = no modifiers

        let reply_bytes = call_handle_request(&req, 4, &mut xi_state, &mut focus);
        assert!(!reply_bytes.is_empty());
        assert_eq!(xi_state.passive_grabs.len(), 1);
        assert_eq!(xi_state.passive_grabs[0].detail, 1);
        assert_eq!(xi_state.passive_grabs[0].grab_window, 0x400001);
        assert_eq!(xi_state.passive_grabs[0].grab_type, 1);

        // Now ungrab it.
        let mut ungrab_req = vec![XI_MAJOR_OPCODE, xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST, 5, 0];
        ungrab_req.extend_from_slice(&0x400001u32.to_le_bytes()); // grab_window
        ungrab_req.extend_from_slice(&1u32.to_le_bytes()); // detail = button 1
        ungrab_req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        ungrab_req.extend_from_slice(&1u16.to_le_bytes()); // num_modifiers = 1
        ungrab_req.push(1); // grab_type = Button
        ungrab_req.extend_from_slice(&[0, 0, 0]); // pad
        ungrab_req.extend_from_slice(&0u32.to_le_bytes()); // modifier = 0

        call_handle_request(&ungrab_req, 5, &mut xi_state, &mut focus);
        assert!(xi_state.passive_grabs.is_empty());
    }

    #[test]
    fn xi_allow_events_async_thaws_pointer() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        xi_state.pointer_frozen = true;
        xi_state.frozen_pointer_events.push(vec![1, 2, 3]);

        // XIAllowEvents: [major, minor, length, time(4), deviceid(2), mode(1), pad(1)]
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_ALLOW_EVENTS_REQUEST, 3, 0];
        req.extend_from_slice(&0u32.to_le_bytes()); // time
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        req.push(0); // mode = AsyncDevice
        req.push(0); // pad

        call_handle_request(&req, 6, &mut xi_state, &mut focus);
        assert!(!xi_state.pointer_frozen);
    }

    #[test]
    fn xi_get_focus_returns_actual_focus() {
        let mut xi_state = XiState::default();
        let mut focus = 0xdead_beefu32;

        let req = vec![XI_MAJOR_OPCODE, xi::XI_GET_FOCUS_REQUEST, 2, 0,
            MASTER_KEYBOARD_ID as u8, 0, 0, 0];

        let reply_bytes = call_handle_request(&req, 7, &mut xi_state, &mut focus);
        let (reply, _) = xi::XIGetFocusReply::try_parse(&reply_bytes).unwrap();
        assert_eq!(reply.focus, 0xdead_beef);
    }

    #[test]
    fn xi_get_selected_events_returns_subscriptions() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // Add a subscription.
        xi_state.selections.push(XiSelection {
            window: 0x400001,
            deviceid: 1,
            mask: vec![(1u32 << xi::BUTTON_PRESS_EVENT).into()],
        });

        // Build XIGetSelectedEvents request: [major, minor, length, window(4)]
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GET_SELECTED_EVENTS_REQUEST, 2, 0];
        req.extend_from_slice(&0x400001u32.to_le_bytes());

        let reply_bytes = call_handle_request(&req, 8, &mut xi_state, &mut focus);
        let (reply, _) = xi::XIGetSelectedEventsReply::try_parse(&reply_bytes).unwrap();
        assert_eq!(reply.masks.len(), 1);
        assert_eq!(reply.masks[0].deviceid, 1);
    }

    #[test]
    fn xi_passive_grab_check_matches() {
        let mut xi_state = XiState::default();
        xi_state.passive_grabs.push(Xi2PassiveGrab {
            deviceid: MASTER_POINTER_ID,
            grab_window: 0x400001,
            detail: 1, // Button 1
            grab_type: 1, // Button
            modifiers: 0x8000, // AnyModifier
            event_mask: vec![],
            owner_events: false,
            paired_device_mode: 1,
            grab_mode: 1,
        });

        // Should match: button 1 on window 0x400001 with any modifier.
        let result = xi_state.check_passive_grab(
            MASTER_POINTER_ID,
            1, // detail = button 1
            1, // grab_type = Button
            0x04, // modifiers = Control
            &[0x400001, 0x62],
        );
        assert!(result.is_some());

        // Should NOT match: button 2.
        let result = xi_state.check_passive_grab(
            MASTER_POINTER_ID,
            2, // detail = button 2
            1,
            0,
            &[0x400001, 0x62],
        );
        assert!(result.is_none());
    }

    #[test]
    fn xi_sync_grab_freezes_pointer() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // Grab in synchronous mode (grab_mode = 0).
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
        req.extend_from_slice(&0x400001u32.to_le_bytes()); // window
        req.extend_from_slice(&0u32.to_le_bytes()); // time
        req.extend_from_slice(&0u32.to_le_bytes()); // cursor
        req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
        req.push(0); // grab_mode = Sync
        req.push(1); // paired_device_mode = Async
        req.push(0); // owner_events
        req.push(0); // pad
        req.extend_from_slice(&0u16.to_le_bytes()); // mask_len
        req.extend_from_slice(&0u16.to_le_bytes()); // pad

        call_handle_request(&req, 10, &mut xi_state, &mut focus);
        assert!(xi_state.pointer_frozen);
        assert!(xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
    }

    // ---- XI 1.x tests --------------------------------------------------

    #[test]
    fn list_input_devices_returns_two_devices() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // Build ListInputDevices request: [opcode, minor=2, length=1, pad(4)]
        let req = vec![
            XI_MAJOR_OPCODE,
            xi::LIST_INPUT_DEVICES_REQUEST,
            1, 0, // length = 1
        ];

        let reply_bytes = call_handle_request(&req, 1, &mut xi_state, &mut focus);
        assert!(!reply_bytes.is_empty(), "ListInputDevices should return a reply");

        // Parse the reply using x11rb.
        let (reply, _) = xi::ListInputDevicesReply::try_parse(&reply_bytes)
            .expect("ListInputDevicesReply must parse");
        assert_eq!(reply.devices.len(), 2, "should have pointer + keyboard");
        assert_eq!(reply.names.len(), 2, "should have 2 device names");

        // Pointer device
        assert_eq!(reply.devices[0].device_id, MASTER_POINTER_ID as u8);
        assert_eq!(reply.devices[0].device_use, xi::DeviceUse::IS_X_POINTER);
        assert_eq!(reply.devices[0].num_class_info, 2); // button + valuator

        // Keyboard device
        assert_eq!(reply.devices[1].device_id, MASTER_KEYBOARD_ID as u8);
        assert_eq!(reply.devices[1].device_use, xi::DeviceUse::IS_X_KEYBOARD);
        assert_eq!(reply.devices[1].num_class_info, 1); // key

        // Check input classes
        assert_eq!(reply.infos.len(), 3); // 2 pointer classes + 1 keyboard class

        // Pointer button class
        let button_info = reply.infos[0].info.as_button().expect("first should be button");
        assert_eq!(button_info.num_buttons, N_POINTER_BUTTONS);

        // Pointer valuator class
        let valuator_info = reply.infos[1].info.as_valuator().expect("second should be valuator");
        assert_eq!(valuator_info.axes.len(), 4); // X, Y, ScrollV, ScrollH

        // Keyboard key class
        let key_info = reply.infos[2].info.as_key().expect("third should be key");
        assert_eq!(key_info.min_keycode, MIN_KEYCODE);
        assert_eq!(key_info.max_keycode, MAX_KEYCODE);
    }

    #[test]
    fn open_device_pointer_returns_classes() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // OpenDevice request: [opcode, minor=3, length=1, 0, device_id=2, pad...]
        let req = vec![
            XI_MAJOR_OPCODE,
            3, // OpenDevice
            1, 0,
            MASTER_POINTER_ID as u8, 0, 0, 0,
        ];

        let reply = call_handle_request(&req, 1, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32, "OpenDevice should return at least 32 bytes");
        assert_eq!(reply[0], 1); // reply
        assert_eq!(reply[1], 3); // xi_reply_type = OpenDevice
        assert_eq!(reply[8], 2); // num_classes = 2 (button + valuator)
    }

    #[test]
    fn open_device_keyboard_returns_key_class() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        let req = vec![
            XI_MAJOR_OPCODE,
            3, // OpenDevice
            1, 0,
            MASTER_KEYBOARD_ID as u8, 0, 0, 0,
        ];

        let reply = call_handle_request(&req, 2, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32);
        assert_eq!(reply[8], 1); // num_classes = 1 (key only)
    }

    #[test]
    fn get_device_key_mapping_returns_keysyms() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // GetDeviceKeyMapping: minor=24
        // body: device_id(1) + first_keycode(1) + count(1) + pad(1)
        let req = vec![
            XI_MAJOR_OPCODE,
            24, // GetDeviceKeyMapping
            2, 0, // length = 2 (8 bytes)
            MASTER_KEYBOARD_ID as u8,
            38, // first_keycode = 38 (key 'a')
            1,  // count = 1
            0,  // pad
        ];

        let reply = call_handle_request(&req, 3, &mut xi_state, &mut focus);
        assert!(reply.len() >= 48, "should have 32-byte header + 4 keysyms × 4 bytes");
        assert_eq!(reply[1], 4); // keysyms_per_keycode = 4

        // First keysym for keycode 38 should be 'a' (0x61)
        let keysym0 = u32::from_le_bytes([reply[32], reply[33], reply[34], reply[35]]);
        assert_eq!(keysym0, 0x61, "keycode 38 should map to 'a'");
        // Second keysym should be 'A' (0x41)
        let keysym1 = u32::from_le_bytes([reply[36], reply[37], reply[38], reply[39]]);
        assert_eq!(keysym1, 0x41, "shifted keycode 38 should map to 'A'");
    }

    #[test]
    fn query_device_state_pointer_returns_valuators() {
        let mut xi_state = XiState::default();
        xi_state.valuators.x = 100;
        xi_state.valuators.y = 200;
        let mut focus = 0x62u32;

        // QueryDeviceState: minor=30, device_id=pointer
        let req = vec![
            XI_MAJOR_OPCODE,
            30,
            1, 0,
            MASTER_POINTER_ID as u8, 0, 0, 0,
        ];

        let reply = call_handle_request(&req, 4, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32);
        assert_eq!(reply[12], 2); // num_classes = 2 (button + valuator)
    }

    #[test]
    fn dont_propagate_list_roundtrip() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        // ChangeDeviceDontPropagateList: add event class 42 to window 0x100
        let mut req = vec![
            XI_MAJOR_OPCODE,
            8, // ChangeDeviceDontPropagateList
            3, 0, // length
        ];
        req.extend_from_slice(&0x100u32.to_le_bytes()); // window
        req.extend_from_slice(&1u16.to_le_bytes()); // count = 1
        req.push(0); // mode = Add
        req.push(0); // pad
        req.extend_from_slice(&42u32.to_le_bytes()); // event class

        call_handle_request(&req, 5, &mut xi_state, &mut focus);

        // Verify with GetDeviceDontPropagateList
        let mut get_req = vec![
            XI_MAJOR_OPCODE,
            9, // GetDeviceDontPropagateList
            2, 0, // length
        ];
        get_req.extend_from_slice(&0x100u32.to_le_bytes()); // window

        let reply = call_handle_request(&get_req, 6, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32);
        // count should be 1
        let count = u16::from_le_bytes([reply[12], reply[13]]);
        assert_eq!(count, 1, "should have 1 event class in the list");
    }

    #[test]
    fn get_device_button_mapping_identity() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        let req = vec![
            XI_MAJOR_OPCODE,
            28, // GetDeviceButtonMapping
            1, 0,
            MASTER_POINTER_ID as u8, 0, 0, 0,
        ];

        let reply = call_handle_request(&req, 7, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32);
        // n_buttons is in reply[1]
        let n_buttons = reply[1];
        assert_eq!(n_buttons, 7, "should have 7 buttons");
        // Check identity mapping
        for i in 0..n_buttons as usize {
            assert_eq!(reply[32 + i], (i + 1) as u8, "button {i} should map to {}", i + 1);
        }
    }

    #[test]
    fn get_device_modifier_mapping_has_modifiers() {
        let mut xi_state = XiState::default();
        let mut focus = 0x62u32;

        let req = vec![
            XI_MAJOR_OPCODE,
            26, // GetDeviceModifierMapping
            1, 0,
            MASTER_KEYBOARD_ID as u8, 0, 0, 0,
        ];

        let reply = call_handle_request(&req, 8, &mut xi_state, &mut focus);
        assert!(reply.len() >= 32);
        let keycodes_per_mod = reply[1];
        assert_eq!(keycodes_per_mod, 2, "should have 2 keycodes per modifier");
        // Shift modifier should map to keycodes 50 and 62
        assert_eq!(reply[32], 50, "Shift_L");
        assert_eq!(reply[33], 62, "Shift_R");
        // Control should map to 37 and 105
        assert_eq!(reply[36], 37, "Control_L");
        assert_eq!(reply[37], 105, "Control_R");
    }
}
