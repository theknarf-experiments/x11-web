//! XTEST extension handler (opcode 150).

use tracing::{debug, warn};
use super::parse_minor;

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// XTEST (opcode 150)
pub(crate) fn handle_xtest_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let bo = state.msb_first;
    let xtest_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error_bo(code, seq, bad_value, 150, minor as u16, bo)
    };
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
            use x11rb_protocol::protocol::xtest::CompareCursorRequest;
            let req = parse_minor!(CompareCursorRequest, data, state, seq, 150, minor as u16);
            let window = req.window;
            let cursor_id = req.cursor;

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
                return xtest_err(crate::xserver::core::ACCESS_ERROR, 0);
            }
            require_len!(data, 24, seq, 150, minor as u16, state.msb_first);
            {
                // FakeInput uses a complex wire format; x11rb's FakeInputRequest
                // parses the fields we need. However the event_type/detail are
                // embedded at specific offsets that the typed struct exposes.
                use x11rb_protocol::protocol::xtest::FakeInputRequest;
                let req = parse_minor!(FakeInputRequest, data, state, seq, 150, minor as u16);
                let event_type = req.type_;
                let detail = req.detail;
                let root_x = req.root_x;
                let root_y = req.root_y;

                debug!("XTEST FakeInput: type={event_type} detail={detail} rootX={root_x} rootY={root_y}");

                // Builder for the KeyButtonPointer-class wire layout shared by
                // KeyPress, KeyRelease, ButtonPress, ButtonRelease, MotionNotify.
                let build_kbp_event = |state: &super::super::client::ClientState,
                                       response_type: u8,
                                       detail: u8|
                 -> Vec<u8> {
                    use x11rb_protocol::protocol::xproto::KeyPressEvent;
                    let ev = KeyPressEvent {
                        response_type,
                        detail,
                        sequence: seq,
                        time: state.timestamp(),
                        root: state.root_window,
                        event: state.focus_window,
                        child: state.focus_window,
                        root_x: state.pointer_x,
                        root_y: state.pointer_y,
                        event_x: state.pointer_x,
                        event_y: state.pointer_y,
                        state: 0u16.into(),
                        same_screen: true,
                    };
                    crate::xserver::event::serialize_event(&ev, state.msb_first)
                };

                match event_type {
                    2 | 3 => {
                        // KeyPress (2) / KeyRelease (3)
                        let keycode = detail;

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
                        super::xkb::maybe_send_xkb_state_notify(state, &xkb_before, keycode, event_type);

                        let event = build_kbp_event(state, event_type, keycode);
                        state.pending_events.push(event);
                    }
                    4 | 5 => {
                        // ButtonPress (4) / ButtonRelease (5)
                        let event = build_kbp_event(state, event_type, detail);
                        state.pending_events.push(event);
                    }
                    6 => {
                        // MotionNotify
                        let old_px = state.pointer_x;
                        let old_py = state.pointer_y;
                        if detail == 0 {
                            state.pointer_x = state.pointer_x.saturating_add(root_x);
                            state.pointer_y = state.pointer_y.saturating_add(root_y);
                        } else {
                            state.pointer_x = root_x;
                            state.pointer_y = root_y;
                        }
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
                        let event = build_kbp_event(state, 6, 0);
                        state.pending_events.push(event);
                    }
                    _ => {
                        warn!("XTEST FakeInput: unknown event type {event_type}");
                        return xtest_err(crate::xserver::core::VALUE_ERROR, event_type as u32);
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
            xtest_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
