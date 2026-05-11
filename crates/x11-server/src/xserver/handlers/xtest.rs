//! XTEST extension handler (opcode 150).

use super::parse_minor;
use tracing::{debug, warn};
use x11rb_protocol::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    MOTION_NOTIFY_EVENT,
};
use x11rb_protocol::protocol::xtest::{
    COMPARE_CURSOR_REQUEST, FAKE_INPUT_REQUEST, GET_VERSION_REQUEST, GRAB_CONTROL_REQUEST,
};

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// XTEST (opcode 150)
pub(crate) fn handle_xtest_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let xtest_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 150, minor as u16)
    };
    match minor {
        GET_VERSION_REQUEST => {
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(2) // major_version in data byte
                .set_u16(8, 2) // minor_version
                .build()
        }
        COMPARE_CURSOR_REQUEST => {
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
        FAKE_INPUT_REQUEST => {
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
                // `event_window` is the target window (focus for keys, the
                // pointer-hit subwindow for buttons/motion); `event_x`,
                // `event_y` are window-local coordinates.
                let build_kbp_event = |state: &super::super::client::ClientState,
                                       response_type: u8,
                                       detail: u8,
                                       event_window: u32,
                                       event_x: i16,
                                       event_y: i16|
                 -> Vec<u8> {
                    use x11rb_protocol::protocol::xproto::KeyPressEvent;
                    let ev = KeyPressEvent {
                        response_type,
                        detail,
                        sequence: seq,
                        time: state.timestamp(),
                        root: state.root_window,
                        event: event_window,
                        child: 0,
                        root_x: state.pointer_x,
                        root_y: state.pointer_y,
                        event_x,
                        event_y,
                        state: 0u16.into(),
                        same_screen: true,
                    };
                    crate::xserver::event::serialize_event(&ev, state.msb_first)
                };

                match event_type {
                    KEY_PRESS_EVENT | KEY_RELEASE_EVENT => {
                        let keycode = detail;

                        let xkb_before = super::xkb::XkbStateSnapshot::capture(state);
                        use crate::xserver::types::keycode_bitset;
                        if event_type == KEY_PRESS_EVENT {
                            keycode_bitset::set(&mut state.pressed_keys, keycode);
                            state.xkb_state.key_press(keycode);
                        } else {
                            keycode_bitset::clear(&mut state.pressed_keys, keycode);
                            state.xkb_state.key_release(keycode);
                        }
                        super::xkb::maybe_send_xkb_state_notify(
                            state,
                            &xkb_before,
                            keycode,
                            event_type,
                        );

                        // Keys go to the focus window (or its first ancestor
                        // that selected the mask, but we deliver to the focus
                        // window itself and let the receiver propagate).
                        let event = build_kbp_event(
                            state,
                            event_type,
                            keycode,
                            state.focus_window,
                            state.pointer_x,
                            state.pointer_y,
                        );
                        let mask = if event_type == KEY_PRESS_EVENT {
                            crate::xserver::core::EventMask::KEY_PRESS
                        } else {
                            crate::xserver::core::EventMask::KEY_RELEASE
                        };
                        state.deliver_event(state.focus_window, mask, &event);
                    }
                    BUTTON_PRESS_EVENT | BUTTON_RELEASE_EVENT => {
                        // Buttons go to the deepest mapped window under the
                        // current pointer position, not the focus window.
                        // Walk the SHARED window registry — the local
                        // per-connection `state.windows` only contains
                        // windows this XTEST client created, so target
                        // windows owned by other clients (GTK app, Firefox
                        // chrome, etc.) wouldn't be reachable through it.
                        let mask_bit = u32::from(if event_type == BUTTON_PRESS_EVENT {
                            crate::xserver::core::EventMask::BUTTON_PRESS
                        } else {
                            crate::xserver::core::EventMask::BUTTON_RELEASE
                        });
                        let (event_window, ex, ey) = find_subwindow_in_shared(
                            state,
                            state.pointer_x,
                            state.pointer_y,
                            mask_bit,
                        );
                        let event =
                            build_kbp_event(state, event_type, detail, event_window, ex, ey);
                        // Prefer routing the event directly to the owning
                        // connection via EventRouter — broadcast filters by
                        // subscription mask which can lag behind the
                        // window's actual selection. EventRouter only knows
                        // top-level UUIDs; fall back to the broadcaster for
                        // sub-window targets and for any other client that
                        // also selected on the window.
                        let routed = state.event_router.send_event(event_window, event.clone());
                        if !routed {
                            // Walk up to find the nearest top-level that's
                            // registered with the router.
                            if let Ok(shared) = state.shared_windows.lock() {
                                let mut walker = event_window;
                                for _ in 0..super::super::window_tree::MAX_TREE_DEPTH {
                                    if state
                                        .event_router
                                        .send_event(walker, event.clone())
                                    {
                                        break;
                                    }
                                    match shared.get(&walker) {
                                        Some(w) if w.parent != 0 && w.parent != walker => {
                                            walker = w.parent;
                                        }
                                        _ => break,
                                    }
                                }
                            }
                        }
                        let mask = if event_type == BUTTON_PRESS_EVENT {
                            crate::xserver::core::EventMask::BUTTON_PRESS
                        } else {
                            crate::xserver::core::EventMask::BUTTON_RELEASE
                        };
                        state.broadcast_event(event_window, mask, &event);
                    }
                    MOTION_NOTIFY_EVENT => {
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
                        // Motion goes to the window under the new pointer
                        // position; consult the shared registry so we
                        // reach windows owned by other clients.
                        let (event_window, ex, ey) = find_subwindow_in_shared(
                            state,
                            state.pointer_x,
                            state.pointer_y,
                            u32::from(crate::xserver::core::EventMask::POINTER_MOTION),
                        );

                        // Emit Enter/Leave crossing events first if the
                        // pointer moved to a different window — toolkits
                        // (GTK3 etc.) update their hovered-widget state
                        // on these and won't react to a subsequent click
                        // without them. Without this gating, xdotool
                        // mousemove-then-click silently no-ops.
                        // build_crossing_events returns a concatenated
                        // byte buffer of crossing events; push it to
                        // pending so it reaches the clients before the
                        // MotionNotify.
                        let crossings = super::super::input::build_crossing_events(
                            state,
                            event_window,
                            state.pointer_x,
                            state.pointer_y,
                            ex,
                            ey,
                        );
                        if !crossings.is_empty() {
                            state.pending_events.push(crossings);
                        }

                        let event = build_kbp_event(
                            state,
                            MOTION_NOTIFY_EVENT,
                            0,
                            event_window,
                            ex,
                            ey,
                        );
                        // Route to the owning connection so toolkits update
                        // their pointer-tracking state before the click
                        // arrives.
                        let routed = state.event_router.send_event(event_window, event.clone());
                        if !routed {
                            if let Ok(shared) = state.shared_windows.lock() {
                                let mut walker = event_window;
                                for _ in 0..super::super::window_tree::MAX_TREE_DEPTH {
                                    if state.event_router.send_event(walker, event.clone()) {
                                        break;
                                    }
                                    match shared.get(&walker) {
                                        Some(w) if w.parent != 0 && w.parent != walker => {
                                            walker = w.parent;
                                        }
                                        _ => break,
                                    }
                                }
                            }
                        }
                        state.broadcast_event(
                            event_window,
                            crate::xserver::core::EventMask::POINTER_MOTION,
                            &event,
                        );
                    }
                    _ => {
                        warn!("XTEST FakeInput: unknown event type {event_type}");
                        return xtest_err(crate::xserver::core::VALUE_ERROR, event_type as u32);
                    }
                }
            }
            Vec::new()
        }
        GRAB_CONTROL_REQUEST => {
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

/// Walk the SHARED window registry to find the deepest mapped window
/// containing the given root-relative point that selects for the event
/// (or its first ancestor that does). XTest clients usually don't own
/// the windows their fake events should target — their per-connection
/// `state.windows` only sees this client's own windows, which won't
/// include the GTK app or Firefox chrome that the cursor is actually
/// over.
///
/// The shared registry has all windows but its per-window
/// `children_order` is not synced from per-client state, so we can't
/// use the standard `find_event_subwindow` on it. Instead we descend
/// by scanning every window with a matching `parent` — slower but
/// correct.
fn find_subwindow_in_shared(
    state: &super::super::client::ClientState,
    root_x: i16,
    root_y: i16,
    required_mask: u32,
) -> (u32, i16, i16) {
    use std::collections::HashMap;

    let Ok(shared) = state.shared_windows.lock() else {
        return (state.root_window, root_x, root_y);
    };

    // Step 1: descend from root. At each level scan all children of
    // the current window, pick the one (with highest XID, a stable but
    // arbitrary tie-break) that covers the local point.
    let mut current = state.root_window;
    let mut local_x = root_x;
    let mut local_y = root_y;
    for _ in 0..super::super::window_tree::MAX_TREE_DEPTH {
        let mut hit: Option<(u32, i16, i16)> = None;
        for (&wid, w) in shared.iter() {
            if w.parent != current || !w.mapped {
                continue;
            }
            let cx = w.x;
            let cy = w.y;
            let cw = w.width as i16;
            let ch = w.height as i16;
            if local_x >= cx && local_x < cx + cw && local_y >= cy && local_y < cy + ch {
                let candidate = (wid, local_x - cx, local_y - cy);
                hit = Some(match hit {
                    Some(prev) if prev.0 > wid => prev,
                    _ => candidate,
                });
            }
        }
        match hit {
            Some((wid, lx, ly)) => {
                current = wid;
                local_x = lx;
                local_y = ly;
            }
            None => break,
        }
    }

    // Step 2: walk up from the deepest hit window looking for one that
    // selects for the required mask. Translate local coords back as
    // we go.
    let _ = HashMap::<u32, ()>::new(); // silence unused import if any
    let mut accum_x = local_x;
    let mut accum_y = local_y;
    let mut walker = current;
    let mut found: Option<(u32, i16, i16)> = None;
    for _ in 0..super::super::window_tree::MAX_TREE_DEPTH {
        if let Some(w) = shared.get(&walker) {
            if u32::from(w.event_mask) & required_mask != 0 {
                found = Some((walker, accum_x, accum_y));
                break;
            }
            if u32::from(w.do_not_propagate_mask) & required_mask != 0 {
                break;
            }
            let parent = w.parent;
            if parent == 0 || parent == walker {
                break;
            }
            accum_x += w.x;
            accum_y += w.y;
            walker = parent;
        } else {
            break;
        }
    }
    found.unwrap_or((current, local_x, local_y))
}
