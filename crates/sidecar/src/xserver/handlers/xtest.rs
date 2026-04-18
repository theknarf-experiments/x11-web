//! XTEST extension handler (opcode 150).

use tracing::{debug, warn};

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// XTEST (opcode 150)
pub(crate) fn handle_xtest_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // GetVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(2) // major_version in data byte
                .set_u16(8, 2) // minor_version
                .build()
        }
        1 => {
            // CompareCursor
            require_len!(data, 12, seq, 150, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            let cursor_id = state.read_u32(data, 8);

            // Compare the cursor currently set on the window against cursor_id.
            // cursor_id=0 means "current cursor" (always same).
            // cursor_id=1 means "None" cursor.
            let win_cursor = state
                .windows
                .get(&window)
                .and_then(|w| w.cursor)
                .unwrap_or(0);
            let same = if cursor_id == 0 {
                true // Comparing against current cursor always matches
            } else {
                win_cursor == cursor_id
            };

            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(if same { 1 } else { 0 })
                .build()
        }
        2 => {
            // FakeInput
            // SECURITY: untrusted clients are denied FakeInput (BadAccess)
            if state.trust_level > 0 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_ACCESS,
                    seq,
                    0,
                    150,
                    minor as u16,
                    state.msb_first,
                );
            }
            require_len!(data, 24, seq, 150, minor as u16, state.msb_first);
            {
                let event_type = data[4];
                let detail = data[5];
                let root_x = state.read_i16(data, 20);
                let root_y = state.read_i16(data, 22);

                debug!("XTEST FakeInput: type={event_type} detail={detail} rootX={root_x} rootY={root_y}");

                match event_type {
                    2 | 3 => {
                        // KeyPress (2) / KeyRelease (3)
                        let keycode = detail;

                        // Update pressed_keys bitmap + XKB modifier state
                        let xkb_before = super::xkb::XkbStateSnapshot::capture(state);
                        let byte_idx = (keycode / 8) as usize;
                        let bit_mask = 1u8 << (keycode % 8);
                        if byte_idx < state.pressed_keys.len() {
                            if event_type == 2 {
                                state.pressed_keys[byte_idx] |= bit_mask;
                                state.xkb_state.key_press(keycode);
                            } else {
                                state.pressed_keys[byte_idx] &= !bit_mask;
                                state.xkb_state.key_release(keycode);
                            }
                        }
                        super::xkb::maybe_send_xkb_state_notify(
                            state,
                            &xkb_before,
                            keycode,
                            event_type,
                        );

                        let mut event = [0u8; 32];
                        event[0] = event_type;
                        event[1] = keycode;
                        state.write_u16(&mut event, 2, seq);
                        state.write_u32(&mut event, 4, state.timestamp());
                        state.write_u32(&mut event, 8, state.root_window);
                        state.write_u32(&mut event, 12, state.focus_window);
                        state.write_u32(&mut event, 16, state.focus_window);
                        state.write_i16(&mut event, 20, state.pointer_x);
                        state.write_i16(&mut event, 22, state.pointer_y);
                        state.write_i16(&mut event, 24, state.pointer_x);
                        state.write_i16(&mut event, 26, state.pointer_y);
                        state.write_u16(&mut event, 28, 0);
                        event[30] = 1; // same_screen = true

                        state.pending_events.push(event.to_vec());
                    }
                    4 | 5 => {
                        // ButtonPress (4) / ButtonRelease (5)
                        let button = detail;

                        let mut event = [0u8; 32];
                        event[0] = event_type;
                        event[1] = button;
                        state.write_u16(&mut event, 2, seq);
                        state.write_u32(&mut event, 4, state.timestamp());
                        state.write_u32(&mut event, 8, state.root_window);
                        state.write_u32(&mut event, 12, state.focus_window);
                        state.write_u32(&mut event, 16, state.focus_window);
                        state.write_i16(&mut event, 20, state.pointer_x);
                        state.write_i16(&mut event, 22, state.pointer_y);
                        state.write_i16(&mut event, 24, state.pointer_x);
                        state.write_i16(&mut event, 26, state.pointer_y);
                        state.write_u16(&mut event, 28, 0);
                        event[30] = 1; // same_screen = true

                        state.pending_events.push(event.to_vec());
                    }
                    6 => {
                        // MotionNotify
                        let old_px = state.pointer_x;
                        let old_py = state.pointer_y;
                        if detail == 0 {
                            // Relative motion
                            state.pointer_x = state.pointer_x.saturating_add(root_x);
                            state.pointer_y = state.pointer_y.saturating_add(root_y);
                        } else {
                            // Absolute motion
                            state.pointer_x = root_x;
                            state.pointer_y = root_y;
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

                        let mut event = [0u8; 32];
                        event[0] = 6;
                        event[1] = 0; // detail for motion
                        state.write_u16(&mut event, 2, seq);
                        state.write_u32(&mut event, 4, state.timestamp());
                        state.write_u32(&mut event, 8, state.root_window);
                        state.write_u32(&mut event, 12, state.focus_window);
                        state.write_u32(&mut event, 16, state.focus_window);
                        state.write_i16(&mut event, 20, state.pointer_x);
                        state.write_i16(&mut event, 22, state.pointer_y);
                        state.write_i16(&mut event, 24, state.pointer_x);
                        state.write_i16(&mut event, 26, state.pointer_y);
                        state.write_u16(&mut event, 28, 0);
                        event[30] = 1; // same_screen = true

                        state.pending_events.push(event.to_vec());
                    }
                    _ => {
                        warn!("XTEST FakeInput: unknown event type {event_type}");
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::BAD_VALUE,
                            seq,
                            event_type as u32,
                            150,
                            minor as u16,
                            state.msb_first,
                        );
                    }
                }
            }
            Vec::new()
        }
        3 => {
            // GrabControl
            // Impervious mode: when enabled, XTEST events bypass active grabs.
            // This allows accessibility tools and test harnesses to inject
            // events even when another client holds a grab.
            require_len!(data, 8, seq, 150, minor as u16, state.msb_first);
            let impervious = data[4] != 0;
            state.xtest_grab_impervious = impervious;
            debug!("XTEST GrabControl: impervious={impervious}");
            Vec::new()
        }
        _ => {
            debug!("XTEST: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST,
                seq,
                minor as u32,
                150,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
