//! Input, keyboard, and pointer handlers (opcodes 38-44, 100-119).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::event::serialize_event;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::xproto::{
    ChangeHostsRequest, ChangeKeyboardControlRequest, ChangeKeyboardMappingRequest,
    ChangePointerControlRequest, ExposeEvent, GetKeyboardMappingRequest, MappingNotifyEvent,
    MotionNotifyEvent, PropertyNotifyEvent, SetInputFocusRequest, SetModifierMappingRequest,
    SetPointerMappingRequest, SetScreenSaverRequest, WarpPointerRequest,
};

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
        x11_web_protocol::DisplayUpdate::Bell {
            percent: effective_percent,
        },
    ));
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 38: QueryPointer
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    state.motion_hint_suppressed = false;
    require_len!(data, 8, seq, 38);
    use x11rb_protocol::protocol::xproto::QueryPointerRequest;
    let req = match QueryPointerRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 38, 0),
    };
    let window = req.window;

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

    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(1) // same_screen
        .set_u32(8, state.root_window)
        .set_u32(12, child)
        .set_i16(16, state.pointer_x)
        .set_i16(18, state.pointer_y)
        .set_i16(20, win_x)
        .set_i16(22, win_y)
        .set_u16(24, mask)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 39: GetMotionEvents
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_motion_events(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    state.motion_hint_suppressed = false;
    require_len!(data, 16, seq, 39);
    use x11rb_protocol::protocol::xproto::GetMotionEventsRequest;
    let req = match GetMotionEventsRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 39, 0),
    };
    let start_time = req.start;
    let stop_time = req.stop;

    // Filter motion history by time range
    let events: Vec<&(u32, i16, i16)> = state
        .motion_history
        .iter()
        .filter(|(ts, _, _)| {
            (start_time == 0 || *ts >= start_time) && (stop_time == 0 || *ts <= stop_time)
        })
        .collect();

    let n_events = events.len() as u32;
    // Each motion event is 8 bytes: timestamp(4) + x(2) + y(2)
    let data_bytes = n_events as usize * 8;
    let data_padded = (data_bytes + 3) & !3;
    let mut reply = ReplyBuf::with_extra(seq, data_padded, state.msb_first)
        .set_u32(8, n_events);

    for (i, (ts, x, y)) in events.iter().enumerate() {
        let off = 32 + i * 8;
        reply = reply.set_u32(off, *ts).set_i16(off + 4, *x).set_i16(off + 6, *y);
    }

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 40: TranslateCoordinates
// ---------------------------------------------------------------------------

pub(crate) fn handle_translate_coordinates(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 16, seq, 40);

    use x11rb_protocol::protocol::xproto::TranslateCoordinatesRequest;
    let req = match TranslateCoordinatesRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 40, 0),
    };
    let src_window = req.src_window;
    let dst_window = req.dst_window;
    let src_x = req.src_x;
    let src_y = req.src_y;

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

    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(1) // same_screen
        .set_u32(8, child)
        .set_i16(12, dst_x)
        .set_i16(14, dst_y)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 41: WarpPointer
// ---------------------------------------------------------------------------

pub(crate) fn handle_warp_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 24, seq, 41);

    let req = match WarpPointerRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 41, 0),
    };
    let src_window = req.src_window;
    let dst_window = req.dst_window;
    let src_x = req.src_x;
    let src_y = req.src_y;
    let src_width = req.src_width;
    let src_height = req.src_height;
    let dst_x = req.dst_x;
    let dst_y = req.dst_y;

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
                if cur == state.root_window || cur == 0 {
                    break;
                }
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
        let check_w = if src_width == 0 {
            sw_width
        } else {
            src_width as i32
        };
        let check_h = if src_height == 0 {
            sw_height
        } else {
            src_height as i32
        };

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
            &state.barriers,
            old_px,
            old_py,
            state.pointer_x,
            state.pointer_y,
        );
        state.pointer_x = bx;
        state.pointer_y = by;
    }

    // Per X11 spec: WarpPointer must generate EnterNotify/LeaveNotify crossing
    // events if the pointer moves between different windows.
    let new_x = state.pointer_x;
    let new_y = state.pointer_y;

    // Find the deepest mapped window under the new pointer position.
    // Use find_deepest_window (geometry-only) rather than find_event_subwindow
    // because crossing events must be generated based on which window the
    // pointer is IN, regardless of whether that window selects for crossing
    // events.  The emit_crossing helper filters by event mask later.
    let new_window = {
        let (w, _, _) = super::super::input::find_deepest_window(
            &state.windows,
            state.root_window,
            new_x,
            new_y,
        );
        w
    };

    // Generate crossing events for the pointer move
    let crossing =
        super::super::input::build_crossing_events(state, new_window, new_x, new_y, new_x, new_y);
    for chunk in crossing.chunks_exact(32) {
        state.pending_events.push(chunk.to_vec());
    }

    // Send MotionNotify event to let the client know the pointer moved
    let event = serialize_event(&MotionNotifyEvent {
        response_type: MOTION_NOTIFY_EVENT,
        detail: 0u8.into(), // Normal
        sequence: seq,
        time: state.timestamp(),
        root: state.root_window,
        event: new_window,
        child: 0,
        root_x: state.pointer_x,
        root_y: state.pointer_y,
        event_x: state.pointer_x,
        event_y: state.pointer_y,
        state: 0u16.into(),
        same_screen: true,
    }, state.msb_first);
    state.pending_events.push(event);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 42: SetInputFocus
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_input_focus(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 42);
    let req = match SetInputFocusRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 42, 0),
    };
    // revert_to (0=None, 1=PointerRoot, 2=Parent)
    let revert_to = u8::from(req.revert_to);
    if revert_to > 2 {
        return build_error(VALUE_ERROR, state.sequence, revert_to as u32, 42, 0);
    }
    let focus = req.focus;
    // Per X11 spec: focus can be 0 (None), 1 (PointerRoot), or a valid window ID.
    // If it's a specific window, validate it exists.
    if focus > 1 && !state.windows.contains_key(&focus) {
        return build_error(WINDOW_ERROR, state.sequence, focus, 42, 0);
    }
    state.focus_revert_to = revert_to;
    state.set_focus_window(focus);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 43: GetInputFocus
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_input_focus(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(state.focus_revert_to)
        .set_u32(8, state.focus_window)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 44: QueryKeymap
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_keymap(state: &ClientState, seq: u16) -> Vec<u8> {
    // Return actual pressed keys state
    ReplyBuf::with_extra(seq, 8, state.msb_first)
        .set_bytes(32, &state.pressed_keys[0..8])
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 100: ChangeKeyboardMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_keyboard_mapping(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 8, seq, 100);

    let req = match ChangeKeyboardMappingRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 100, 0),
    };
    let keycode_count = req.keycode_count as usize;
    let first_keycode = req.first_keycode;
    let keysyms_per_keycode = req.keysyms_per_keycode as usize;

    if keysyms_per_keycode == 0 {
        return build_error(VALUE_ERROR, seq, 0, 100, 0);
    }

    // Store the new keycode->keysym mappings from the parsed keysyms list
    for i in 0..keycode_count {
        let keycode = first_keycode.wrapping_add(i as u8);
        let start = i * keysyms_per_keycode;
        let end = start + keysyms_per_keycode;
        let syms: Vec<u32> = req.keysyms[start..end].to_vec();
        state.custom_keymap.insert(keycode, syms);
    }

    debug!(
        "ChangeKeyboardMapping: first={first_keycode} count={keycode_count} syms_per_kc={keysyms_per_keycode} — stored {} mappings",
        keycode_count
    );

    // MappingNotify must be sent to ALL clients per X11 spec (section 12.7).
    // The requesting client gets it via pending_events, all others via broadcast.
    let event = serialize_event(&MappingNotifyEvent {
        response_type: MAPPING_NOTIFY_EVENT,
        sequence: seq,
        request: 1u8.into(), // Keyboard
        first_keycode: first_keycode,
        count: keycode_count as u8,
    }, state.msb_first);
    state.pending_events.push(event.clone());
    state
        .event_broadcaster
        .broadcast_global(&event, &state.client_id);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 101: GetKeyboardMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_keyboard_mapping(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 101);
    let req = match GetKeyboardMappingRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 101, 0),
    };
    let first_keycode = req.first_keycode;
    let count = req.count;

    // Determine keysyms_per_keycode: use max from custom mappings or default 4
    let max_custom = state
        .custom_keymap
        .values()
        .map(|v| v.len())
        .max()
        .unwrap_or(0);
    let keysyms_per_keycode = max_custom.max(4) as u8;
    let total_syms = count as u32 * keysyms_per_keycode as u32;
    let extra_bytes = total_syms as usize * 4;

    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_data_byte(keysyms_per_keycode);

    for i in 0..count as usize {
        let keycode = first_keycode.wrapping_add(i as u8);
        let offset = 32 + i * keysyms_per_keycode as usize * 4;

        if let Some(custom_syms) = state.custom_keymap.get(&keycode) {
            // Use the custom mapping set by ChangeKeyboardMapping
            for (j, &sym) in custom_syms.iter().enumerate() {
                if j < keysyms_per_keycode as usize {
                    reply = reply.set_u32(offset + j * 4, sym);
                }
            }
        } else {
            // Fall back to built-in US layout
            let (normal, shifted) = keycode_to_keysym(keycode);
            reply = reply.set_u32(offset, normal).set_u32(offset + 4, shifted);
            // Remaining slots (mode switch, mode+shift) left as 0 (NoSymbol)
        }
    }

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 102: ChangeKeyboardControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_keyboard_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 102);

    let req = match ChangeKeyboardControlRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 102, 0),
    };
    let vl = &*req.value_list;

    // Per X11 spec, led (bit 4) and led_mode (bit 5) work together,
    // and key (bit 6) and auto_repeat_mode (bit 7) work together.

    if let Some(val) = vl.key_click_percent {
        state.keyboard_control.key_click_percent = (val as u32).min(100) as u8;
    }
    if let Some(val) = vl.bell_percent {
        state.keyboard_control.bell_percent = (val as u32).min(100) as u8;
    }
    if let Some(val) = vl.bell_pitch {
        state.keyboard_control.bell_pitch = val as u16;
    }
    if let Some(val) = vl.bell_duration {
        state.keyboard_control.bell_duration = val as u16;
    }

    if let Some(led_mode) = vl.led_mode {
        let val = u32::from(led_mode);
        if val > 1 {
            return build_error(VALUE_ERROR, state.sequence, val, 102, 0);
        }
        if let Some(led) = vl.led {
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

    if let Some(auto_repeat_mode) = vl.auto_repeat_mode {
        let val = u32::from(auto_repeat_mode);
        if val > 2 {
            return build_error(VALUE_ERROR, state.sequence, val, 102, 0);
        }
        if let Some(key) = vl.key {
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

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 103: GetKeyboardControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_keyboard_control(state: &ClientState, seq: u16) -> Vec<u8> {
    let kc = &state.keyboard_control;
    ReplyBuf::with_extra(seq, 20, state.msb_first)
        .set_data_byte(kc.global_auto_repeat)
        .set_u32(8, kc.led_mask)
        .set_u8(12, kc.key_click_percent)
        .set_u8(13, kc.bell_percent)
        .set_u16(14, kc.bell_pitch)
        .set_u16(16, kc.bell_duration)
        .set_bytes(20, &kc.auto_repeats)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 105: ChangePointerControl
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_pointer_control(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 105);

    let req = match ChangePointerControlRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 105, 0),
    };
    let accel_num = req.acceleration_numerator;
    let accel_den = req.acceleration_denominator;
    let threshold = req.threshold;
    let do_accel = req.do_acceleration;
    let do_threshold = req.do_threshold;

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
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, pc.acceleration_numerator)
        .set_u16(10, pc.acceleration_denominator)
        .set_u16(12, pc.threshold)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 107: SetScreenSaver
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_screen_saver(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 10, state.sequence, 107);

    let req = match SetScreenSaverRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 107, 0),
    };
    let timeout = req.timeout;
    let interval = req.interval;
    let prefer_blanking = u8::from(req.prefer_blanking);
    let allow_exposures = u8::from(req.allow_exposures);

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
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, ss.timeout)
        .set_u16(10, ss.interval)
        .set_u8(12, ss.prefer_blanking)
        .set_u8(13, ss.allow_exposures)
        .build()
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
                let expose_targets: Vec<(u32, u16, u16, u32)> = state
                    .windows
                    .values()
                    .filter(|w| w.mapped && w.id != state.root_window)
                    .map(|w| (w.id, w.width, w.height, w.event_mask))
                    .collect();
                for (wid, w, h, mask) in &expose_targets {
                    let expose = serialize_event(&ExposeEvent {
                        response_type: EXPOSE_EVENT,
                        sequence: seq,
                        window: *wid,
                        x: 0,
                        y: 0,
                        width: *w,
                        height: *h,
                        count: 0,
                    }, bo);
                    if *mask & EventMask::EXPOSURE != EventMask::NO_EVENT {
                        state.pending_events.push(expose.clone());
                    }
                    state.broadcast_event(*wid, u32::from(EventMask::EXPOSURE), &expose);
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
            build_error(VALUE_ERROR, seq, mode as u32, 115, 0)
        }
    }
}

/// Build a MIT-SCREEN-SAVER ScreenSaverNotify event if the client has
/// selected for it (event_mask != 0). The `saver_state` parameter is
/// 0 = Off, 1 = On, 2 = Cycle.
fn build_screen_saver_notify(state: &ClientState, saver_state: u8) -> Vec<u8> {
    use x11rb_protocol::protocol::screensaver::{
        Kind as SsKind, NotifyEvent as ScreenSaverNotifyEvent, State as SsState,
    };

    if state.screen_saver_event_mask == 0 {
        return Vec::new();
    }

    serialize_event(&ScreenSaverNotifyEvent {
        response_type: 92,
        state: SsState::from(saver_state),
        sequence: state.sequence,
        time: state.timestamp(),
        root: state.root_window,
        window: state.screen_saver_window,
        kind: SsKind::from(0u8), // Blanked
        forced: true,
    }, state.msb_first)
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

    let extra_padded = (host_entries.len() + 3) & !3;
    ReplyBuf::with_extra(seq, extra_padded, state.msb_first)
        .set_data_byte(if state.access_control_enabled { 1 } else { 0 })
        .set_u16(8, state.access_hosts.len() as u16)
        .set_bytes(32, &host_entries)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 109: ChangeHosts
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_hosts(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    use super::super::client::AccessHost;

    require_len!(data, 8, state.sequence, 109);
    let req = match ChangeHostsRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 109, 0),
    };
    let mode = u8::from(req.mode);
    let family = u8::from(req.family);
    let addr_len = req.address.len();
    let address = req.address.into_owned();

    // Validate mode: per X11 spec, only 0 (Insert) and 1 (Delete) are valid
    if mode > 1 {
        return build_error(VALUE_ERROR, state.sequence, mode as u32, 109, 0);
    }

    // Validate address family and address length per X11 spec:
    //   0 = Internet (IPv4, 4 bytes)
    //   1 = DECnet (Phase IV address, 2 bytes)
    //   2 = Chaos (Chaosnet address)
    //   5 = ServerInterpreted (variable length, null-separated type+value)
    //   6 = Internet6 (IPv6, 16 bytes)
    //   254 = Local (no address data expected)
    match family {
        0 => {
            // Internet (IPv4)
            if addr_len != 4 {
                return build_error(VALUE_ERROR, state.sequence, addr_len as u32, 109, 0);
            }
        }
        6 => {
            // Internet6 (IPv6)
            if addr_len != 16 {
                return build_error(VALUE_ERROR, state.sequence, addr_len as u32, 109, 0);
            }
        }
        1 => {
            // DECnet
            if addr_len != 2 {
                return build_error(VALUE_ERROR, state.sequence, addr_len as u32, 109, 0);
            }
        }
        5 => {
            // ServerInterpreted - variable length, must have at least type+NUL+value
            if addr_len < 1 {
                return build_error(VALUE_ERROR, state.sequence, addr_len as u32, 109, 0);
            }
        }
        254 => { /* Local - accept any length */ }
        2 => { /* Chaos - accept any length (protocol-defined, uncommon) */ }
        _ => {
            // Unknown address family
            return build_error(VALUE_ERROR, state.sequence, family as u32, 109, 0);
        }
    }

    match mode {
        0 => {
            // Insert
            // Don't add duplicates
            if !state
                .access_hosts
                .iter()
                .any(|h| h.family == family && h.address == address)
            {
                state.access_hosts.push(AccessHost {
                    family,
                    address: address.clone(),
                });
            }
        }
        1 => {
            // Delete
            state
                .access_hosts
                .retain(|h| !(h.family == family && h.address == address));
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
        return build_error(VALUE_ERROR, state.sequence, mode as u32, 112, 0);
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

    use x11rb_protocol::protocol::xproto::KillClientRequest;
    let req = match KillClientRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 113, 0),
    };
    let resource = req.resource;

    if resource == 0 {
        // AllTemporary: destroy all windows retained from clients that
        // disconnected with close_down_mode = RetainTemporary.
        // Per X11 spec, this destroys ALL resources from such clients.
        debug!("KillClient: AllTemporary");

        // Collect retained_temporary windows from both local and shared state.
        let mut to_destroy: Vec<u32> = state
            .windows
            .values()
            .filter(|w| w.retained_temporary)
            .map(|w| w.id)
            .collect();

        // Also check shared_windows for retained windows from other clients.
        if let Ok(shared) = state.shared_windows.lock() {
            for (wid, win) in shared.iter() {
                if win.retained_temporary && !to_destroy.contains(wid) {
                    to_destroy.push(*wid);
                }
            }
        }

        for wid in &to_destroy {
            state.windows.remove(wid);
            if let Some(uuid) = state.x11_to_uuid.remove(wid) {
                state
                    .window_router
                    .unregister_all(std::slice::from_ref(&uuid));
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowDestroyed { window_id: uuid },
                ));
            }
        }

        // Remove from shared_windows as well.
        if let Ok(mut shared) = state.shared_windows.lock() {
            for wid in &to_destroy {
                shared.remove(wid);
            }
        }

        // Also clean up retained_temporary_windows list
        state.retained_temporary_windows.clear();
    } else {
        debug!("KillClient: resource={resource:#x}");

        let owner = state
            .windows
            .get(&resource)
            .map(|w| w.owner_client_id.clone());
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
                    state
                        .window_router
                        .unregister_all(std::slice::from_ref(&uuid));
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

    use x11rb_protocol::protocol::xproto::RotatePropertiesRequest;
    let req = match RotatePropertiesRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 114, 0),
    };
    let window = req.window;
    if !state.windows.contains_key(&window) {
        return build_error(WINDOW_ERROR, state.sequence, window, 114, 0);
    }

    let n_atoms = req.atoms.len();
    let delta = req.delta;

    if n_atoms == 0 || delta == 0 {
        return Vec::new();
    }

    let atoms: Vec<u32> = req.atoms.into_owned();

    // Per X11 spec, duplicate atoms in the list generate BadMatch.
    {
        let mut seen = std::collections::HashSet::with_capacity(atoms.len());
        for &atom in &atoms {
            if !seen.insert(atom) {
                return build_error(MATCH_ERROR, state.sequence, atom, 114, 0);
            }
        }
    }

    if atoms.len() < 2 {
        // Single atom: valid but a no-op per spec.
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
    let win_mask = state
        .windows
        .get(&window)
        .map(|w| w.event_mask)
        .unwrap_or(0);
    for &atom in &atoms {
        let event = serialize_event(&PropertyNotifyEvent {
            response_type: PROPERTY_NOTIFY_EVENT,
            sequence: seq,
            window,
            atom,
            time: timestamp,
            state: 0u8.into(), // NewValue
        }, state.msb_first);
        if win_mask & EventMask::PROPERTY_CHANGE != EventMask::NO_EVENT {
            state.pending_events.push(event.clone());
        }
        state.broadcast_event(window, u32::from(EventMask::PROPERTY_CHANGE), &event);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 116: SetPointerMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_pointer_mapping(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 4, seq, 116);
    let req = match SetPointerMappingRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 116, 0),
    };
    let n_buttons = req.map.len();
    // Parse the new mapping from the request data (support up to 7 buttons)
    let max_buttons = state.pointer_mapping.len();
    if n_buttons <= max_buttons {
        state.pointer_mapping[..n_buttons].copy_from_slice(&req.map);
        debug!(
            "SetPointerMapping: {:?}",
            &state.pointer_mapping[..n_buttons]
        );

        // MappingNotify (request=Pointer) must be sent to ALL clients per X11 spec.
        let event = serialize_event(&MappingNotifyEvent {
            response_type: MAPPING_NOTIFY_EVENT,
            sequence: seq,
            request: 2u8.into(), // Pointer
            first_keycode: 0,
            count: 0,
        }, state.msb_first);
        state.pending_events.push(event.clone());
        state
            .event_broadcaster
            .broadcast_global(&event, &state.client_id);
    }

    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(0) // MappingSuccess
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 117: GetPointerMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_pointer_mapping(state: &ClientState, seq: u16) -> Vec<u8> {
    let map = &state.pointer_mapping;
    let n = map.len() as u8;
    let padded_len = (n as usize + 3) & !3;
    ReplyBuf::with_extra(seq, padded_len, state.msb_first)
        .set_data_byte(n)
        .set_bytes(32, map)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 118: SetModifierMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_modifier_mapping(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 4, seq, 118);
    let req = match SetModifierMappingRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 118, 0),
    };
    let keycodes_per_modifier = req.keycodes.len() / 8;

    if keycodes_per_modifier > 0 {
        state.modifier_map.clear();
        for mod_idx in 0..8 {
            let start = mod_idx * keycodes_per_modifier;
            let end = start + keycodes_per_modifier;
            let keycodes: Vec<u8> = req.keycodes[start..end]
                .iter()
                .copied()
                .filter(|&k| k != 0)
                .collect();
            state.modifier_map.push(keycodes);
        }
        debug!(
            "SetModifierMapping: {} keycodes/modifier",
            keycodes_per_modifier
        );

        // MappingNotify must be sent to ALL clients per X11 spec.
        let event = serialize_event(&MappingNotifyEvent {
            response_type: MAPPING_NOTIFY_EVENT,
            sequence: state.sequence,
            request: 0u8.into(), // Modifier
            first_keycode: 0,
            count: 0,
        }, state.msb_first);
        state.pending_events.push(event.clone());
        state
            .event_broadcaster
            .broadcast_global(&event, &state.client_id);
    }

    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(0) // MappingSuccess
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 119: GetModifierMapping
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_modifier_mapping(state: &ClientState, seq: u16) -> Vec<u8> {
    // Find the max keycodes per modifier to determine padding
    let max_keycodes = state
        .modifier_map
        .iter()
        .map(|v| v.len())
        .max()
        .unwrap_or(2)
        .max(2);
    let keycodes_per_modifier = max_keycodes as u8;

    let data_len = 8 * keycodes_per_modifier as usize;
    let mut reply = ReplyBuf::with_extra(seq, data_len, state.msb_first)
        .set_data_byte(keycodes_per_modifier);

    for (i, keycodes) in state.modifier_map.iter().enumerate() {
        let off = 32 + i * keycodes_per_modifier as usize;
        for (j, &kc) in keycodes.iter().enumerate() {
            if j < keycodes_per_modifier as usize {
                reply = reply.set_u8(off + j, kc);
            }
        }
    }
    reply.build()
}
