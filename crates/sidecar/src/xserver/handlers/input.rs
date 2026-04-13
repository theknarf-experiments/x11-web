//! Input, keyboard, and pointer handlers (opcodes 38-44, 100-119).

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 104: Bell
// ---------------------------------------------------------------------------

pub(crate) fn handle_bell(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let percent = if data.len() >= 2 { data[1] } else { 0 };
    let effective_percent = if percent == 0 {
        state.keyboard_control.bell_percent
    } else if percent > 0 && percent <= 100 {
        percent
    } else {
        // negative percent means reduce from base
        let p = percent as i8;
        let base = state.keyboard_control.bell_percent as i16;
        (base + (base * p as i16) / 100).clamp(0, 100) as u8
    };
    let _ = state.update_tx.send((
        state.client_id.clone(),
        x11_web_protocol::DisplayUpdate::Bell { percent: effective_percent },
    ));
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 38: QueryPointer
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    state.motion_hint_suppressed = false;
    require_len!(data, 8, seq, 38);
    // Read the window parameter from the request (offset 4, u32)
    let window = state.read_u32(data, 4);

    // Calculate window-relative coordinates by walking up from window to root
    let mut win_origin_x = 0i32;
    let mut win_origin_y = 0i32;
    {
        let mut cur = window;
        for _ in 0..128 {
            if cur == state.root_window || cur == 0 {
                break;
            }
            if let Some(w) = state.windows.get(&cur) {
                win_origin_x += w.x as i32;
                win_origin_y += w.y as i32;
                cur = w.parent;
            } else {
                break;
            }
        }
    }
    let win_x = (state.pointer_x as i32 - win_origin_x) as i16;
    let win_y = (state.pointer_y as i32 - win_origin_y) as i16;

    // Find the direct child of `window` that contains the pointer, or 0
    let child = state
        .windows
        .values()
        .find(|w| {
            w.parent == window
                && win_x >= w.x
                && win_x < w.x.saturating_add(w.width as i16)
                && win_y >= w.y
                && win_y < w.y.saturating_add(w.height as i16)
        })
        .map(|w| w.id)
        .unwrap_or(0);

    // Build modifier/button mask: low byte = keyboard modifiers, bits 8-12 = buttons 1-5
    let mask = state.xkb_state.effective_mods() as u16 | state.pointer_button_mask;

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // same_screen
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, state.root_window); // root
    state.write_u32(&mut reply, 12, child);             // child
    state.write_i16(&mut reply, 16, state.pointer_x);   // root_x
    state.write_i16(&mut reply, 18, state.pointer_y);   // root_y
    state.write_i16(&mut reply, 20, win_x);              // win_x
    state.write_i16(&mut reply, 22, win_y);              // win_y
    state.write_u16(&mut reply, 24, mask);               // mask
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 39: GetMotionEvents
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_motion_events(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    state.motion_hint_suppressed = false;
    require_len!(data, 16, seq, 39);
    // Parse time range from request
    let start_time = state.read_u32(data, 8);
    let stop_time = state.read_u32(data, 12);

    // Filter motion history by time range
    let events: Vec<&(u32, i16, i16)> = state.motion_history.iter()
        .filter(|(ts, _, _)| {
            (start_time == 0 || *ts >= start_time) && (stop_time == 0 || *ts <= stop_time)
        })
        .collect();

    let n_events = events.len() as u32;
    // Each motion event is 8 bytes: timestamp(4) + x(2) + y(2)
    let data_bytes = n_events as usize * 8;
    let data_padded = (data_bytes + 3) & !3;
    let mut reply = vec![0u8; 32 + data_padded];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (data_padded / 4) as u32);
    state.write_u32(&mut reply, 8, n_events);

    for (i, (ts, x, y)) in events.iter().enumerate() {
        let off = 32 + i * 8;
        state.write_u32(&mut reply, off, *ts);
        state.write_i16(&mut reply, off + 4, *x);
        state.write_i16(&mut reply, off + 6, *y);
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 40: TranslateCoordinates
// ---------------------------------------------------------------------------

pub(crate) fn handle_translate_coordinates(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 40);

    let src_window = state.read_u32(data, 4);
    let dst_window = state.read_u32(data, 8);
    let src_x = state.read_i16(data, 12);
    let src_y = state.read_i16(data, 14);

    // Convert src_x, src_y from src_window coordinate space to root, then to dst_window.
    let mut sx = src_x as i32;
    let mut sy = src_y as i32;
    {
        let mut cur = src_window;
        for _ in 0..128 {
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
    // Now (sx, sy) is in root coordinates.
    let mut dx = 0i32;
    let mut dy = 0i32;
    {
        let mut cur = dst_window;
        for _ in 0..128 {
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
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, child);
    state.write_i16(&mut reply, 12, dst_x);
    state.write_i16(&mut reply, 14, dst_y);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 41: WarpPointer
// ---------------------------------------------------------------------------

pub(crate) fn handle_warp_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 24, seq, 41);

    let src_window = state.read_u32(data, 4);
    let dst_window = state.read_u32(data, 8);
    let src_x = state.read_i16(data, 12);
    let src_y = state.read_i16(data, 14);
    let src_width = state.read_u16(data, 16);
    let src_height = state.read_u16(data, 18);
    let dst_x = state.read_i16(data, 20);
    let dst_y = state.read_i16(data, 22);

    // Per X11 spec §11.6.2: if src_window is not 0, the warp is conditional.
    // The pointer must currently be inside src_window within the specified
    // rectangle (src_x, src_y, src_width, src_height).  If src_width or
    // src_height is 0, the full window extent is used.
    if src_window != 0 {
        // Compute absolute position of src_window.
        let mut sw_abs_x: i32 = 0;
        let mut sw_abs_y: i32 = 0;
        let mut sw_width: i32 = state.screen_width as i32;
        let mut sw_height: i32 = state.screen_height as i32;
        if src_window != state.root_window {
            let mut cur = src_window;
            if let Some(w) = state.windows.get(&cur) {
                sw_width = w.width as i32;
                sw_height = w.height as i32;
                sw_abs_x = w.x as i32;
                sw_abs_y = w.y as i32;
                cur = w.parent;
            }
            for _ in 0..128 {
                if cur == state.root_window || cur == 0 { break; }
                if let Some(w) = state.windows.get(&cur) {
                    sw_abs_x += w.x as i32;
                    sw_abs_y += w.y as i32;
                    cur = w.parent;
                } else {
                    break;
                }
            }
        }
        // Compute the check rectangle in root coordinates.
        let check_x = sw_abs_x + src_x as i32;
        let check_y = sw_abs_y + src_y as i32;
        let check_w = if src_width == 0 { sw_width } else { src_width as i32 };
        let check_h = if src_height == 0 { sw_height } else { src_height as i32 };

        let px = state.pointer_x as i32;
        let py = state.pointer_y as i32;
        if px < check_x || py < check_y || px >= check_x + check_w || py >= check_y + check_h {
            // Pointer is outside the source rectangle — skip the warp.
            return Vec::new();
        }
    }

    let old_px = state.pointer_x;
    let old_py = state.pointer_y;

    if dst_window == 0 {
        // Relative warp: offset from current position
        state.pointer_x = state.pointer_x.saturating_add(dst_x);
        state.pointer_y = state.pointer_y.saturating_add(dst_y);
    } else {
        // Absolute warp: position relative to dst_window, converted to root coords
        let mut abs_x = dst_x as i32;
        let mut abs_y = dst_y as i32;
        let mut cur = dst_window;
        for _ in 0..128 {
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
        state.pointer_x = abs_x.clamp(0, state.screen_width as i32 - 1) as i16;
        state.pointer_y = abs_y.clamp(0, state.screen_height as i32 - 1) as i16;
    }

    // Enforce XFIXES pointer barriers
    if !state.barriers.is_empty() {
        let (bx, by) = super::super::input::enforce_barriers(
            &state.barriers, old_px, old_py, state.pointer_x, state.pointer_y,
        );
        state.pointer_x = bx;
        state.pointer_y = by;
    }

    // Send MotionNotify event to let the client know the pointer moved
    let mut event = [0u8; 32];
    event[0] = MOTION_NOTIFY_EVENT;
    event[1] = 0; // detail = Normal
    state.write_u16(&mut event, 2, seq);
    state.write_u32(&mut event, 4, state.timestamp()); // timestamp
    state.write_u32(&mut event, 8, state.root_window); // root
    // event window = focus_window
    state.write_u32(&mut event, 12, state.focus_window);
    state.write_i16(&mut event, 20, state.pointer_x); // root_x
    state.write_i16(&mut event, 22, state.pointer_y); // root_y
    state.write_i16(&mut event, 24, state.pointer_x); // event_x
    state.write_i16(&mut event, 26, state.pointer_y); // event_y
    event[30] = 1; // same_screen = true
    state.pending_events.push(event.to_vec());

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 42: SetInputFocus
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_input_focus(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 42);
    // data[1] = revert_to (0=None, 1=PointerRoot, 2=Parent)
    let revert_to = data[1];
    if revert_to > 2 {
        return build_error(BAD_VALUE, state.sequence, revert_to as u32, 42, 0);
    }
    let focus = state.read_u32(data, 4);
    // Per X11 spec: focus can be 0 (None), 1 (PointerRoot), or a valid window ID.
    // If it's a specific window, validate it exists.
    if focus > 1 && !state.windows.contains_key(&focus) {
        return build_error(BAD_WINDOW, state.sequence, focus, 42, 0);
    }
    state.focus_revert_to = revert_to;
    state.set_focus_window(focus);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 43: GetInputFocus
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_input_focus(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = state.focus_revert_to;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, state.focus_window);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 44: QueryKeymap
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_keymap(state: &ClientState, seq: u16) -> Vec<u8> {
    // Return actual pressed keys state
    let mut reply = [0u8; 40]; // 32 + 8 bytes of keymap
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, 2u32); // length = 2 (8 extra bytes)
    // Copy the pressed_keys bitmap into the reply
    reply[32..40].copy_from_slice(&state.pressed_keys[0..8]);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 100: ChangeKeyboardMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_keyboard_mapping(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 100);

    let keycode_count = data[1] as usize;
    let first_keycode = data[4];
    let keysyms_per_keycode = data[5] as usize;

    if keysyms_per_keycode == 0 {
        return build_error(BAD_VALUE, seq, 0, 100, 0);
    }

    // Parse and store the new keycode->keysym mappings
    let total_syms = keycode_count * keysyms_per_keycode;
    if data.len() < 8 + total_syms * 4 {
        debug!("ChangeKeyboardMapping: request too short ({} < {})", data.len(), 8 + total_syms * 4);
        return build_error(BAD_LENGTH, seq, 0, 100, 0);
    }

    for i in 0..keycode_count {
        let keycode = first_keycode.wrapping_add(i as u8);
        let mut syms = Vec::with_capacity(keysyms_per_keycode);
        for j in 0..keysyms_per_keycode {
            let off = 8 + (i * keysyms_per_keycode + j) * 4;
            let sym = state.read_u32(data, off);
            syms.push(sym);
        }
        state.custom_keymap.insert(keycode, syms);
    }

    debug!(
        "ChangeKeyboardMapping: first={first_keycode} count={keycode_count} syms_per_kc={keysyms_per_keycode} — stored {} mappings",
        keycode_count
    );

    // MappingNotify must be sent to ALL clients per X11 spec (section 12.7).
    // The requesting client gets it via pending_events, all others via broadcast.
    let mut event = [0u8; 32];
    event[0] = MAPPING_NOTIFY_EVENT;
    state.write_u16(&mut event, 2, seq);
    event[4] = 1; // request = Keyboard
    event[5] = first_keycode;
    event[6] = keycode_count as u8;
    state.pending_events.push(event.to_vec());
    state.event_broadcaster.broadcast_global(&event, &state.client_id);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 101: GetKeyboardMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_keyboard_mapping(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 101);
    let first_keycode = data[4];
    let count = data[5];

    // Determine keysyms_per_keycode: use max from custom mappings or default 4
    let max_custom = state.custom_keymap.values().map(|v| v.len()).max().unwrap_or(0);
    let keysyms_per_keycode = max_custom.max(4) as u8;
    let total_syms = count as u32 * keysyms_per_keycode as u32;
    let reply_len = 32 + total_syms as usize * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    reply[1] = keysyms_per_keycode;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, total_syms);

    for i in 0..count as usize {
        let keycode = first_keycode.wrapping_add(i as u8);
        let offset = 32 + i * keysyms_per_keycode as usize * 4;

        if let Some(custom_syms) = state.custom_keymap.get(&keycode) {
            // Use the custom mapping set by ChangeKeyboardMapping
            for (j, &sym) in custom_syms.iter().enumerate() {
                if j < keysyms_per_keycode as usize {
                    state.write_u32(&mut reply, offset + j * 4, sym);
                }
            }
        } else {
            // Fall back to built-in US layout
            let (normal, shifted) = keycode_to_keysym(keycode);
            state.write_u32(&mut reply, offset, normal);
            state.write_u32(&mut reply, offset + 4, shifted);
            // Remaining slots (mode switch, mode+shift) left as 0 (NoSymbol)
        }
    }

    reply
}

// ---------------------------------------------------------------------------
// Opcode 102: ChangeKeyboardControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_keyboard_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 102);

    let value_mask = state.read_u32(data, 4);
    let mut offset = 8;

    // Per X11 spec, led (bit 4) and led_mode (bit 5) work together,
    // and key (bit 6) and auto_repeat_mode (bit 7) work together.
    // We must track intermediate values as we parse sequentially.
    let mut led_value: Option<u32> = None;
    let mut key_value: Option<u32> = None;

    for bit in 0..8u32 {
        if value_mask & (1 << bit) != 0
            && offset + 4 <= data.len() {
                let val = state.read_u32(data, offset);
                match bit {
                    0 => state.keyboard_control.key_click_percent = val.min(100) as u8,
                    1 => state.keyboard_control.bell_percent = val.min(100) as u8,
                    2 => state.keyboard_control.bell_pitch = val as u16,
                    3 => state.keyboard_control.bell_duration = val as u16,
                    4 => {
                        // led: identifies which LED (1-32) to modify with led_mode
                        led_value = Some(val);
                        // If led_mode is not also set, this just stores the value
                        // for potential use by led_mode if it appears later
                    }
                    5 => {
                        // led_mode: 0=Off, 1=On for the LED specified by led (bit 4)
                        if val > 1 {
                            return build_error(BAD_VALUE, state.sequence, val, 102, 0);
                        }
                        if let Some(led) = led_value {
                            if led >= 1 && led <= 32 {
                                let bit_pos = led - 1;
                                if val == 1 {
                                    state.keyboard_control.led_mask |= 1 << bit_pos;
                                } else {
                                    state.keyboard_control.led_mask &= !(1 << bit_pos);
                                }
                            }
                        } else {
                            // Per spec: if led is not specified, led_mode applies to all LEDs
                            if val == 1 {
                                state.keyboard_control.led_mask = 0xFFFFFFFF;
                            } else {
                                state.keyboard_control.led_mask = 0;
                            }
                        }
                    }
                    6 => {
                        // key: identifies which keycode's auto-repeat to modify
                        key_value = Some(val);
                    }
                    7 => {
                        // auto_repeat_mode: 0=Off, 1=On, 2=Default
                        if val > 2 {
                            return build_error(BAD_VALUE, state.sequence, val, 102, 0);
                        }
                        if let Some(key) = key_value {
                            // Per spec: key must be a valid keycode (8-255)
                            if key >= 8 && key <= 255 {
                                let byte_idx = (key / 8) as usize;
                                let bit_mask = 1u8 << (key % 8);
                                match val {
                                    0 => state.keyboard_control.auto_repeats[byte_idx] &= !bit_mask,
                                    1 => state.keyboard_control.auto_repeats[byte_idx] |= bit_mask,
                                    2 => state.keyboard_control.auto_repeats[byte_idx] |= bit_mask, // Default = On
                                    _ => {}
                                }
                            }
                        } else {
                            // Per spec: if key is not specified, this sets global auto_repeat
                            state.keyboard_control.global_auto_repeat = val.min(1) as u8;
                        }
                    }
                    _ => {}
                }
                offset += 4;
            }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 103: GetKeyboardControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_keyboard_control(state: &ClientState, seq: u16) -> Vec<u8> {
    let kc = &state.keyboard_control;
    let mut reply = [0u8; 52]; // 32 + 20 extra
    reply[0] = 1;
    reply[1] = kc.global_auto_repeat;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, 5u32); // length = 5 (20 extra bytes)
    state.write_u32(&mut reply, 8, kc.led_mask);
    reply[12] = kc.key_click_percent;
    reply[13] = kc.bell_percent;
    state.write_u16(&mut reply, 14, kc.bell_pitch);
    state.write_u16(&mut reply, 16, kc.bell_duration);
    // auto_repeats: 32 bytes at offset 20
    reply[20..52].copy_from_slice(&kc.auto_repeats);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 105: ChangePointerControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_pointer_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 105);

    let accel_num = state.read_i16(data, 4);
    let accel_den = state.read_i16(data, 6);
    let threshold = state.read_i16(data, 8);
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

pub(crate) fn handle_get_pointer_control(state: &ClientState, seq: u16) -> Vec<u8> {
    let pc = &state.pointer_control;
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u16(&mut reply, 8, pc.acceleration_numerator);
    state.write_u16(&mut reply, 10, pc.acceleration_denominator);
    state.write_u16(&mut reply, 12, pc.threshold);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 107: SetScreenSaver
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_screen_saver(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 10, state.sequence, 107);

    let timeout = state.read_i16(data, 4);
    let interval = state.read_i16(data, 6);
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

pub(crate) fn handle_get_screen_saver(state: &ClientState, seq: u16) -> Vec<u8> {
    let ss = &state.screen_saver;
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u16(&mut reply, 8, ss.timeout);
    state.write_u16(&mut reply, 10, ss.interval);
    reply[12] = ss.prefer_blanking;
    reply[13] = ss.allow_exposures;
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 115: ForceScreenSaver
// ---------------------------------------------------------------------------

pub(crate) fn handle_force_screen_saver(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // data[1] contains the mode field (per X11 spec, it's in the second byte
    // of the request header, not the request body).
    let mode = data[1];

    match mode {
        0 => {
            // Reset: reset the screen saver timer as if user activity occurred.
            // If the screen saver is currently active, deactivate it.
            state.screen_saver.last_reset_ms = state.timestamp();
            let was_active = state.screen_saver.active;
            state.screen_saver.active = false;

            if was_active {
                debug!("ForceScreenSaver Reset: deactivated screen saver");

                // Per X11 spec, when the screen saver is reset (deactivated),
                // Expose events must be generated for all mapped windows that
                // select ExposureMask, since the screen content needs repainting.
                let bo = state.msb_first;
                let expose_targets: Vec<(u32, u16, u16, u32)> = state.windows.values()
                    .filter(|w| w.mapped && w.id != state.root_window)
                    .map(|w| (w.id, w.width, w.height, w.event_mask))
                    .collect();
                for (wid, w, h, mask) in &expose_targets {
                    let mut expose = [0u8; 32];
                    expose[0] = EXPOSE_EVENT;
                    write_u16_bo(&mut expose, 2, seq, bo);
                    write_u32_bo(&mut expose, 4, *wid, bo);
                    // x=0, y=0 already zero
                    write_u16_bo(&mut expose, 12, *w, bo);
                    write_u16_bo(&mut expose, 14, *h, bo);
                    write_u16_bo(&mut expose, 16, 0, bo); // count = 0
                    if mask & EXPOSURE_MASK != 0 {
                        state.pending_events.push(expose.to_vec());
                    }
                    state.broadcast_event(*wid, EXPOSURE_MASK, &expose);
                }

                // Send ScreenSaverNotify (state=Off) to interested clients.
                build_screen_saver_notify(state, 0 /* Off */)
            } else {
                debug!("ForceScreenSaver Reset: timer reset");
                Vec::new()
            }
        }
        1 => {
            // Activate: activate the screen saver immediately.
            state.screen_saver.active = true;
            debug!("ForceScreenSaver Activate: screen saver activated");

            // Send ScreenSaverNotify (state=On) to interested clients.
            build_screen_saver_notify(state, 1 /* On */)
        }
        _ => {
            // Invalid mode value
            build_error(BAD_VALUE, seq, mode as u32, 115, 0)
        }
    }
}

/// Build a MIT-SCREEN-SAVER ScreenSaverNotify event if the client has
/// selected for it (event_mask != 0). The `saver_state` parameter is
/// 0 = Off, 1 = On, 2 = Cycle.
fn build_screen_saver_notify(state: &ClientState, saver_state: u8) -> Vec<u8> {
    if state.screen_saver_event_mask == 0 {
        return Vec::new();
    }

    // ScreenSaverNotify is event code 0 from the MIT-SCREEN-SAVER extension.
    // The event is 32 bytes:
    //   byte 0:   event code (extension base event + 0)
    //   byte 1:   saver state (0=Off, 1=On, 2=Cycle)
    //   bytes 2-3: sequence number
    //   bytes 4-7: timestamp
    //   bytes 8-11: root window
    //   bytes 12-15: saver window (or 0)
    //   byte 16: kind (0=Blanked, 1=Internal, 2=External)
    //   byte 17: forced (1 = forced via ForceScreenSaver)
    //   bytes 18-31: pad
    let mut event = [0u8; 32];
    // MIT-SCREEN-SAVER ScreenSaverNotify event code = extension base (92) + 0
    event[0] = 92;
    event[1] = saver_state;
    state.write_u16(&mut event, 2, state.sequence);
    state.write_u32(&mut event, 4, state.timestamp());
    state.write_u32(&mut event, 8, state.root_window);
    state.write_u32(&mut event, 12, state.screen_saver_window);
    event[16] = 0; // kind = Blanked
    event[17] = 1; // forced = true (ForceScreenSaver)
    event.to_vec()
}

/// Build a ScreenSaverNotify (Off) event for automatic deactivation on input.
/// Returns empty vec if client doesn't subscribe to screen saver events.
pub(crate) fn build_screen_saver_off_event(state: &ClientState) -> Vec<u8> {
    build_screen_saver_notify(state, 0 /* Off */)
}

/// Build a ScreenSaverNotify (On) event for automatic activation on timeout.
pub(crate) fn build_screen_saver_on_event(state: &ClientState) -> Vec<u8> {
    build_screen_saver_notify(state, 1 /* On */)
}

// ---------------------------------------------------------------------------
// Opcode 110: ListHosts
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_hosts(state: &ClientState, seq: u16) -> Vec<u8> {
    // Build host list from access_hosts
    let mut host_entries = Vec::new();
    for host in &state.access_hosts {
        // Each host entry: family(1) + pad(1) + address_length(2) + address(padded)
        let addr_len = host.address.len();
        let padded = (addr_len + 3) & !3;
        let mut entry = vec![0u8; 4 + padded];
        entry[0] = host.family;
        entry[2] = (addr_len & 0xFF) as u8;
        entry[3] = ((addr_len >> 8) & 0xFF) as u8;
        entry[4..4 + addr_len].copy_from_slice(&host.address);
        host_entries.extend_from_slice(&entry);
    }

    let extra_words = (host_entries.len() / 4) as u32;
    let mut reply = vec![0u8; 32 + host_entries.len()];
    reply[0] = 1;
    reply[1] = if state.access_control_enabled { 1 } else { 0 };
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, extra_words);
    state.write_u16(&mut reply, 8, state.access_hosts.len() as u16);
    reply[32..].copy_from_slice(&host_entries);
    reply
}

// ---------------------------------------------------------------------------
// Opcode 109: ChangeHosts
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_hosts(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    use super::super::client::AccessHost;

    require_len!(data, 8, state.sequence, 109);
    let mode = data[1]; // 0 = Insert, 1 = Delete
    let family = data[4];
    let addr_len = state.read_u16(data, 6) as usize;
    require_len!(data, 8 + addr_len, state.sequence, 109);
    let address = data[8..8 + addr_len].to_vec();

    // Validate mode: per X11 spec, only 0 (Insert) and 1 (Delete) are valid
    if mode > 1 {
        return build_error(BAD_VALUE, state.sequence, mode as u32, 109, 0);
    }

    // Validate address family and address length per X11 spec:
    //   0 = Internet (IPv4, 4 bytes)
    //   1 = DECnet (Phase IV address, 2 bytes)
    //   2 = Chaos (Chaosnet address)
    //   5 = ServerInterpreted (variable length, null-separated type+value)
    //   6 = Internet6 (IPv6, 16 bytes)
    //   254 = Local (no address data expected)
    match family {
        0 => { // Internet (IPv4)
            if addr_len != 4 {
                return build_error(BAD_VALUE, state.sequence, addr_len as u32, 109, 0);
            }
        }
        6 => { // Internet6 (IPv6)
            if addr_len != 16 {
                return build_error(BAD_VALUE, state.sequence, addr_len as u32, 109, 0);
            }
        }
        1 => { // DECnet
            if addr_len != 2 {
                return build_error(BAD_VALUE, state.sequence, addr_len as u32, 109, 0);
            }
        }
        5 => { // ServerInterpreted - variable length, must have at least type+NUL+value
            if addr_len < 1 {
                return build_error(BAD_VALUE, state.sequence, addr_len as u32, 109, 0);
            }
        }
        254 => { /* Local - accept any length */ }
        2 => { /* Chaos - accept any length (protocol-defined, uncommon) */ }
        _ => {
            // Unknown address family
            return build_error(BAD_VALUE, state.sequence, family as u32, 109, 0);
        }
    }

    match mode {
        0 => { // Insert
            // Don't add duplicates
            if !state.access_hosts.iter().any(|h| h.family == family && h.address == address) {
                state.access_hosts.push(AccessHost { family, address: address.clone() });
            }
        }
        1 => { // Delete
            state.access_hosts.retain(|h| !(h.family == family && h.address == address));
        }
        _ => unreachable!(), // Validated above
    }

    // Sync to shared server-wide access control for TCP enforcement
    if let Ok(mut acl) = state.shared_access_control.lock() {
        acl.hosts = state.access_hosts.clone();
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 111: SetAccessControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_access_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 4, state.sequence, 111);
    state.access_control_enabled = data[1] != 0;
    debug!("SetAccessControl: enabled={}", state.access_control_enabled);

    // Sync to shared server-wide access control for TCP enforcement
    if let Ok(mut acl) = state.shared_access_control.lock() {
        acl.enabled = state.access_control_enabled;
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 112: SetCloseDownMode
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_close_down_mode(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 4, state.sequence, 112);
    let mode = data[1];
    // Per X11 spec: mode must be 0 (Destroy), 1 (RetainPermanent), or 2 (RetainTemporary).
    if mode > 2 {
        return build_error(BAD_VALUE, state.sequence, mode as u32, 112, 0);
    }
    state.close_down_mode = mode;
    debug!("SetCloseDownMode: mode={mode}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 113: KillClient
// ---------------------------------------------------------------------------

pub(crate) fn handle_kill_client(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 113);

    let resource = state.read_u32(data, 4);

    if resource == 0 {
        // AllTemporary: destroy all windows retained from clients that
        // disconnected with close_down_mode = RetainTemporary.
        debug!("KillClient: AllTemporary");
        let to_destroy: Vec<u32> = state
            .windows
            .values()
            .filter(|w| w.retained_temporary)
            .map(|w| w.id)
            .collect();
        for wid in to_destroy {
            state.windows.remove(&wid);
            if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
                state.window_router.unregister_all(std::slice::from_ref(&uuid));
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowDestroyed { window_id: uuid },
                ));
            }
        }
        // Also clean up retained_temporary_windows list
        state.retained_temporary_windows.clear();
    } else {
        debug!("KillClient: resource={resource:#x}");

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
                    state.window_router.unregister_all(std::slice::from_ref(&uuid));
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

pub(crate) fn handle_rotate_properties(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 114);

    let window = state.read_u32(data, 4);
    if !state.windows.contains_key(&window) {
        return build_error(BAD_WINDOW, state.sequence, window, 114, 0);
    }

    let n_atoms = state.read_u16(data, 8) as usize;
    let delta = state.read_i16(data, 10);

    if n_atoms == 0 || delta == 0 {
        return Vec::new();
    }

    // Validate that the atom list fits within the request data
    let required_len = 12 + n_atoms * 4;
    require_len!(data, required_len, state.sequence, 114);

    // Read the atom list
    let mut atoms = Vec::with_capacity(n_atoms);
    for i in 0..n_atoms {
        let off = 12 + i * 4;
        atoms.push(state.read_u32(data, off));
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

    // Per X11 spec, RotateProperties generates a PropertyNotify event for
    // each property in the list, with state=NewValue (0).
    let seq = state.sequence;
    let timestamp = state.timestamp();
    let win_mask = state.windows.get(&window)
        .map(|w| w.event_mask)
        .unwrap_or(0);
    for &atom in &atoms {
        let mut event = [0u8; 32];
        event[0] = PROPERTY_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, seq);
        state.write_u32(&mut event, 4, window);
        state.write_u32(&mut event, 8, atom);
        state.write_u32(&mut event, 12, timestamp);
        event[16] = 0; // NewValue
        if win_mask & PROPERTY_CHANGE_MASK != 0 {
            state.pending_events.push(event.to_vec());
        }
        state.broadcast_event(window, PROPERTY_CHANGE_MASK, &event);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 116: SetPointerMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_pointer_mapping(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 4, seq, 116);
    let n_buttons = data[1] as usize;
    // Parse the new mapping from the request data (support up to 7 buttons)
    let max_buttons = state.pointer_mapping.len();
    if data.len() >= 4 + n_buttons && n_buttons <= max_buttons {
        state.pointer_mapping[..n_buttons].copy_from_slice(&data[4..4 + n_buttons]);
        debug!("SetPointerMapping: {:?}", &state.pointer_mapping[..n_buttons]);

        // MappingNotify (request=Pointer) must be sent to ALL clients per X11 spec.
        let mut event = [0u8; 32];
        event[0] = MAPPING_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, seq);
        event[4] = 2; // request = Pointer
        state.pending_events.push(event.to_vec());
        state.event_broadcaster.broadcast_global(&event, &state.client_id);
    }

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // MappingSuccess
    state.write_u16(&mut reply, 2, seq);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 117: GetPointerMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_pointer_mapping(state: &ClientState, seq: u16) -> Vec<u8> {
    let map = &state.pointer_mapping;
    let n = map.len() as u8;
    let padded_len = (n as usize + 3) & !3;
    let reply_extra_units = (padded_len / 4) as u32;
    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1; // Reply
    reply[1] = n;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, reply_extra_units);
    reply[32..32 + n as usize].copy_from_slice(map);
    reply
}

// ---------------------------------------------------------------------------
// Opcode 118: SetModifierMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_modifier_mapping(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 4, seq, 118);
    let keycodes_per_modifier = data[1] as usize;
    let total_keycodes = 8 * keycodes_per_modifier;

    if data.len() >= 4 + total_keycodes && keycodes_per_modifier > 0 {
        state.modifier_map.clear();
        for mod_idx in 0..8 {
            let start = 4 + mod_idx * keycodes_per_modifier;
            let end = start + keycodes_per_modifier;
            let keycodes: Vec<u8> = data[start..end].iter().copied().filter(|&k| k != 0).collect();
            state.modifier_map.push(keycodes);
        }
        debug!("SetModifierMapping: {} keycodes/modifier", keycodes_per_modifier);

        // MappingNotify must be sent to ALL clients per X11 spec.
        let mut event = [0u8; 32];
        event[0] = MAPPING_NOTIFY_EVENT;
        state.write_u16(&mut event, 2, state.sequence);
        event[4] = 0; // request = Modifier
        state.pending_events.push(event.to_vec());
        state.event_broadcaster.broadcast_global(&event, &state.client_id);
    }

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // MappingSuccess
    state.write_u16(&mut reply, 2, seq);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 119: GetModifierMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_modifier_mapping(state: &ClientState, seq: u16) -> Vec<u8> {
    // Find the max keycodes per modifier to determine padding
    let max_keycodes = state.modifier_map.iter().map(|v| v.len()).max().unwrap_or(2).max(2);
    let keycodes_per_modifier = max_keycodes as u8;

    let data_len = 8 * keycodes_per_modifier as usize;
    let reply_len = 32 + data_len;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[1] = keycodes_per_modifier;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (data_len / 4) as u32);

    for (i, keycodes) in state.modifier_map.iter().enumerate() {
        let off = 32 + i * keycodes_per_modifier as usize;
        for (j, &kc) in keycodes.iter().enumerate() {
            if j < keycodes_per_modifier as usize {
                reply[off + j] = kc;
            }
        }
    }
    reply
}
