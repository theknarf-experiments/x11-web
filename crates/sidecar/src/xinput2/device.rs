use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::x11_utils::Serialize as _;

use super::{
    fp3232, serialize_xi_reply, ValuatorState, AXIS_SCROLL_H, AXIS_SCROLL_V, MASTER_KEYBOARD_ID,
    MASTER_POINTER_ID,
};

/// Minimum keycode we advertise in the Setup reply.
pub(crate) const MIN_KEYCODE: u8 = 8;
/// Maximum keycode we advertise.
pub(crate) const MAX_KEYCODE: u8 = 255;
/// Number of physical + scroll buttons for the virtual pointer.
pub(crate) const N_POINTER_BUTTONS: u16 = 7;

/// Build a proper ListInputDevices reply with the two virtual core devices.
/// Each device carries its full set of XI 1.x input classes (KeyInfo,
/// ButtonInfo, ValuatorInfo) so legacy toolkits can discover device
/// capabilities.
pub(crate) fn build_list_input_devices_reply(
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
        infos: vec![
            pointer_button_info,
            pointer_valuator_info,
            keyboard_key_info,
        ],
        names: vec![
            xproto::Str {
                name: b"Virtual core pointer".to_vec(),
            },
            xproto::Str {
                name: b"Virtual core keyboard".to_vec(),
            },
        ],
    };
    serialize_xi_reply(&reply, msb_first)
}

/// Build an OpenDevice reply for the given device_id. Returns the device's
/// input classes (key, button, valuator) so XI 1.x clients can query
/// device capabilities after opening.
pub(crate) fn build_open_device_reply(
    device_id: u8,
    seq: u16,
    _screen_width: u16,
    _screen_height: u16,
    msb_first: bool,
) -> Vec<u8> {
    let is_keyboard = device_id == MASTER_KEYBOARD_ID as u8;
    let class_info: Vec<xi::InputClassInfo> = if is_keyboard {
        vec![xi::InputClassInfo { class_id: xi::InputClass::KEY, event_type_base: 0 }]
    } else {
        vec![
            xi::InputClassInfo { class_id: xi::InputClass::BUTTON, event_type_base: 0 },
            xi::InputClassInfo { class_id: xi::InputClass::VALUATOR, event_type_base: 0 },
        ]
    };
    let reply = xi::OpenDeviceReply {
        xi_reply_type: 3,
        sequence: seq,
        length: 0,
        class_info,
    };
    crate::xinput2::serialize_xi_reply(&reply, msb_first)
}

/// Build a GetDeviceKeyMapping reply that mirrors the core keyboard mapping.
/// Returns keysyms for the requested keycode range.
pub(crate) fn build_device_key_mapping_reply(
    first_keycode: u8,
    count: u8,
    seq: u16,
    msb_first: bool,
    custom_keymap: &std::collections::HashMap<u8, Vec<u32>>,
) -> Vec<u8> {
    // 4 keysyms per keycode (normal, shift, altgr, shift+altgr)
    let keysyms_per_keycode: u8 = 4;
    let mut keysyms: Vec<u32> = Vec::with_capacity(count as usize * keysyms_per_keycode as usize);
    for i in 0..count as usize {
        let kc = first_keycode.wrapping_add(i as u8);
        if let Some(syms) = custom_keymap.get(&kc) {
            for j in 0..keysyms_per_keycode as usize {
                keysyms.push(syms.get(j).copied().unwrap_or(0));
            }
        } else {
            let (normal, shifted) = keycode_to_keysym_xi(kc);
            keysyms.extend_from_slice(&[normal, shifted, 0, 0]);
        }
    }
    let reply = xi::GetDeviceKeyMappingReply {
        xi_reply_type: 24,
        sequence: seq,
        keysyms_per_keycode,
        keysyms,
    };
    crate::xinput2::serialize_xi_reply(&reply, msb_first)
}

/// Minimal US keyboard layout mapping for XI 1.x GetDeviceKeyMapping.
/// Returns (normal_keysym, shifted_keysym) for a given keycode.
pub(crate) fn keycode_to_keysym_xi(keycode: u8) -> (u32, u32) {
    match keycode {
        9 => (0xff1b, 0xff1b),  // Escape
        10 => (0x31, 0x21),     // 1 / !
        11 => (0x32, 0x40),     // 2 / @
        12 => (0x33, 0x23),     // 3 / #
        13 => (0x34, 0x24),     // 4 / $
        14 => (0x35, 0x25),     // 5 / %
        15 => (0x36, 0x5e),     // 6 / ^
        16 => (0x37, 0x26),     // 7 / &
        17 => (0x38, 0x2a),     // 8 / *
        18 => (0x39, 0x28),     // 9 / (
        19 => (0x30, 0x29),     // 0 / )
        20 => (0x2d, 0x5f),     // - / _
        21 => (0x3d, 0x2b),     // = / +
        22 => (0xff08, 0xff08), // BackSpace
        23 => (0xff09, 0xfe20), // Tab / ISO_Left_Tab
        24 => (0x71, 0x51),     // q / Q
        25 => (0x77, 0x57),     // w / W
        26 => (0x65, 0x45),     // e / E
        27 => (0x72, 0x52),     // r / R
        28 => (0x74, 0x54),     // t / T
        29 => (0x79, 0x59),     // y / Y
        30 => (0x75, 0x55),     // u / U
        31 => (0x69, 0x49),     // i / I
        32 => (0x6f, 0x4f),     // o / O
        33 => (0x70, 0x50),     // p / P
        34 => (0x5b, 0x7b),     // [ / {
        35 => (0x5d, 0x7d),     // ] / }
        36 => (0xff0d, 0xff0d), // Return
        37 => (0xffe3, 0xffe3), // Control_L
        38 => (0x61, 0x41),     // a / A
        39 => (0x73, 0x53),     // s / S
        40 => (0x64, 0x44),     // d / D
        41 => (0x66, 0x46),     // f / F
        42 => (0x67, 0x47),     // g / G
        43 => (0x68, 0x48),     // h / H
        44 => (0x6a, 0x4a),     // j / J
        45 => (0x6b, 0x4b),     // k / K
        46 => (0x6c, 0x4c),     // l / L
        47 => (0x3b, 0x3a),     // ; / :
        48 => (0x27, 0x22),     // ' / "
        49 => (0x60, 0x7e),     // ` / ~
        50 => (0xffe1, 0xffe1), // Shift_L
        51 => (0x5c, 0x7c),     // \ / |
        52 => (0x7a, 0x5a),     // z / Z
        53 => (0x78, 0x58),     // x / X
        54 => (0x63, 0x43),     // c / C
        55 => (0x76, 0x56),     // v / V
        56 => (0x62, 0x42),     // b / B
        57 => (0x6e, 0x4e),     // n / N
        58 => (0x6d, 0x4d),     // m / M
        59 => (0x2c, 0x3c),     // , / <
        60 => (0x2e, 0x3e),     // . / >
        61 => (0x2f, 0x3f),     // / / ?
        62 => (0xffe2, 0xffe2), // Shift_R
        63 => (0xffaa, 0xffaa), // KP_Multiply
        64 => (0xffe9, 0xffe9), // Alt_L
        65 => (0x20, 0x20),     // space
        66 => (0xffe5, 0xffe5), // Caps_Lock
        67..=76 => {
            // F1-F10
            let fkey = 0xffbe + (keycode - 67) as u32;
            (fkey, fkey)
        }
        95 => (0xffc8, 0xffc8),  // F11
        96 => (0xffc9, 0xffc9),  // F12
        105 => (0xffe4, 0xffe4), // Control_R
        108 => (0xffea, 0xffea), // Alt_R
        110 => (0xff50, 0xff50), // Home
        111 => (0xff52, 0xff52), // Up
        112 => (0xff55, 0xff55), // Prior/PageUp
        113 => (0xff51, 0xff51), // Left
        114 => (0xff53, 0xff53), // Right
        115 => (0xff57, 0xff57), // End
        116 => (0xff54, 0xff54), // Down
        117 => (0xff56, 0xff56), // Next/PageDown
        118 => (0xff63, 0xff63), // Insert
        119 => (0xffff, 0xffff), // Delete
        133 => (0xffeb, 0xffeb), // Super_L
        134 => (0xffec, 0xffec), // Super_R
        _ => (0, 0),             // NoSymbol
    }
}

/// Build a GetDeviceModifierMapping reply with the standard modifier map.
pub(crate) fn build_device_modifier_mapping_reply(seq: u16, msb_first: bool) -> Vec<u8> {
    // Modifier map: [Shift, Lock, Control, Mod1(Alt), Mod2(Num), Mod3, Mod4(Super), Mod5]
    // 8 modifiers × 2 keycodes per modifier (the xi_reply_type byte is set
    // to keycodes-per-modifier; serialize_xi_reply patches the length).
    let keymaps: Vec<u8> = vec![
        50, 62,   // Shift: Shift_L(50), Shift_R(62)
        66, 0,    // Lock: Caps_Lock(66)
        37, 105,  // Control: Control_L(37), Control_R(105)
        64, 108,  // Mod1 (Alt): Alt_L(64), Alt_R(108)
        77, 0,    // Mod2 (Num Lock): Num_Lock(77)
        0, 0,     // Mod3: unused
        133, 134, // Mod4 (Super): Super_L(133), Super_R(134)
        0, 0,     // Mod5: unused
    ];
    let reply = xi::GetDeviceModifierMappingReply {
        xi_reply_type: 26,
        sequence: seq,
        length: 0,
        keymaps,
    };
    crate::xinput2::serialize_xi_reply(&reply, msb_first)
}

/// Build a QueryDeviceState reply with current button/key/valuator state.
pub(crate) fn build_query_device_state_reply(
    device_id: u8,
    valuators: &ValuatorState,
    seq: u16,
    msb_first: bool,
) -> Vec<u8> {
    let is_keyboard = device_id == MASTER_KEYBOARD_ID as u8;
    let classes: Vec<xi::InputState> = if is_keyboard {
        vec![xi::InputState {
            len: 36, // class_id + len + InputStateDataKey(34) = 36
            data: xi::InputStateData::Key(xi::InputStateDataKey {
                num_keys: 248,
                keys: [0u8; 32],
            }),
        }]
    } else {
        vec![
            xi::InputState {
                len: 8,
                data: xi::InputStateData::Button(xi::InputStateDataButton {
                    num_buttons: N_POINTER_BUTTONS as u8,
                    // Only the first 4 bytes (32 bits) ever go on the wire
                    // for our 7-button pointer; the rest is padding.
                    buttons: [0u8; 32],
                }),
            },
            xi::InputState {
                len: 4 + 4 * 4, // header + 4 valuators × 4 bytes
                data: xi::InputStateData::Valuator(xi::InputStateDataValuator {
                    // bitmask: 0 = Relative mode, in proximity
                    mode: 0u8.into(),
                    valuators: vec![
                        valuators.x,
                        valuators.y,
                        valuators.scroll_v,
                        valuators.scroll_h,
                    ],
                }),
            },
        ]
    };
    let reply = xi::QueryDeviceStateReply {
        xi_reply_type: 30,
        sequence: seq,
        length: 0,
        classes,
    };
    crate::xinput2::serialize_xi_reply(&reply, msb_first)
}

/// Build the bytes for the master pointer's `XIDeviceInfo`. The screen
/// dimensions bound the valuator axes — XI clients use these to clamp
/// or normalise pointer position values.
pub(crate) fn build_master_pointer_info(
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
        data: xi::DeviceClassData::Gesture(xi::DeviceClassDataGesture { num_touches: 5 }),
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

pub(crate) fn build_master_keyboard_info() -> xi::XIDeviceInfo {
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
pub(crate) fn fill_class_lengths(classes: &mut [xi::DeviceClass]) {
    for c in classes.iter_mut() {
        let mut buf = Vec::new();
        c.serialize_into(&mut buf);
        debug_assert!(buf.len() % 4 == 0, "class wire length not 4-aligned");
        c.len = (buf.len() / 4) as u16;
    }
}

/// Build the byte payload for `XIQueryDevice` containing both master
/// devices.
pub(crate) fn query_device_reply_bytes(
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
pub(crate) fn mods_from_state(state: u16) -> xi::ModifierInfo {
    xi::ModifierInfo {
        base: 0,
        latched: 0,
        locked: 0,
        effective: u32::from(state),
    }
}
