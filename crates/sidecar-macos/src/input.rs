//! Inject keyboard and mouse events from frontend `InputEvent`s into
//! the target app's process.
//!
//! Click dispatch is AX-only: `AXUIElementCopyElementAtPosition`
//! resolves the screen-space coord to an AX element, then
//! `AXPress` runs through `FocusGuard` so focus snaps back to the
//! previously frontmost app. Pure RPC, no synthetic mouse events.
//!
//! Keyboard dispatch goes through `CGEvent::newKeyboardEvent` +
//! `SLEventPostToPid` (with the SkyLight auth-message envelope
//! attached, required on macOS 14+ for strict synthetic-event
//! filters). AX has no general way to dispatch arrow keys, tab, or
//! modifier combinations, so this path stays.

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{CGEvent, CGEventFlags, CGMouseButton, CGScrollEventUnit};
use tracing::warn;
use x11_web_protocol::InputEvent;

use crate::ax;
use crate::focus_guard;
use crate::router::WindowRoute;
use crate::skylight::probe;

/// Inject a single browser `InputEvent` against the window described
/// by `route`. Logs and skips silently for event kinds we don't
/// support yet (touch, gestures).
///
/// Click dispatch goes through `AXUIElementCopyElementAtPosition`
/// to find an AX-addressable element at the screen point; if found,
/// dispatch `AXPress` wrapped in `FocusGuard` (Layer 2 + Layer 3).
/// Pure RPC, no synthetic mouse events, no focus disturbance.
pub fn inject(route: WindowRoute, event: InputEvent) {
    // Tag the metric by event family so we can split key/click/scroll
    // workloads in OpenObserve. Recorded at attempt time — drops
    // inside `send_key` / `try_ax_click` (e.g. missing AX permission)
    // still count, which pairs naturally with the AX warnings in logs.
    if let Some(m) = crate::telemetry::metrics() {
        let kind = match &event {
            InputEvent::KeyPress { .. } | InputEvent::KeyRelease { .. } => "key",
            InputEvent::ButtonPress { button, .. } | InputEvent::ButtonRelease { button, .. } => {
                if scroll_delta_for_button(*button).is_some() {
                    "scroll"
                } else {
                    "button"
                }
            }
            InputEvent::MotionNotify { .. } => "motion",
            InputEvent::MenuActivate { .. } => "menu",
            _ => "other",
        };
        m.input_events
            .add(1, &[opentelemetry::KeyValue::new("kind", kind)]);
    }
    match event {
        InputEvent::ButtonPress { button, x, y, .. } => {
            // Scroll-wheel buttons (X11 convention: 4/5 vertical,
            // 6/7 horizontal). Frontend emits one Press/Release
            // pair per wheel tick, so each ButtonPress becomes one
            // line-scroll event.
            if let Some((dy, dx)) = scroll_delta_for_button(button) {
                send_scroll(&route, dy, dx);
                return;
            }
            if map_button(button).is_none() {
                return;
            }
            let target = window_local_to_screen(&route, x, y);
            match try_ax_click(&route, target) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::info!(
                        "AX click did not dispatch (no element / wrong pid / unsupported)"
                    );
                }
                Err(e) => {
                    tracing::warn!("AX click error: {e}");
                }
            }
        }
        InputEvent::ButtonRelease { .. } => {
            // No-op — scroll-button release is part of the
            // press/release pair the frontend synthesizes per tick;
            // we already emitted the wheel event on Press.
        }
        InputEvent::MotionNotify { x, y, .. } => {
            let _ = (x, y);
            // No-op for AX-only mode — mouseMoved doesn't help AX
            // dispatch.
        }
        InputEvent::KeyPress { keycode, state } => {
            send_key(&route, keycode, state, true);
        }
        InputEvent::KeyRelease { keycode, state } => {
            send_key(&route, keycode, state, false);
        }
        InputEvent::MenuActivate { action } => {
            // The action's `name` is the AX path we baked in when
            // the menu was first read (`p<pid>/i/j/k`). Re-walk
            // and AXPress the leaf — runs on the calling thread,
            // a single AX RPC takes a couple ms.
            match crate::menu::dispatch_action(&action.name) {
                Ok(()) => tracing::info!("menu activate: {}", action.name),
                Err(e) => tracing::warn!("menu activate {}: {e}", action.name),
            }
        }
        _ => {}
    }
}

/// Synthesize a keyboard event and post it to the target's pid.
/// Mirrors cua-driver's `KeyboardInput.sendKey(...)`:
///
///   1. Build a `CGEvent` keyboard event with the macOS virtual
///      key code and `keyDown` flag.
///   2. Stamp `CGEventFlags` from the X11 modifier state mask so
///      Cmd-, Shift-, Ctrl-, Alt- combos arrive as such.
///   3. Post via `SLEventPostToPid` (no auth message in this first
///      cut — we rely on the fallback `CGEvent.postToPid` if the
///      SkyLight SPI wasn't resolvable). Auth-message envelope for
///      Chromium-family acceptance can land in a follow-up.
///
/// Cua's note: keyboard goes through `CGEvent` rather than AX
/// because AX has no general way to dispatch arrow keys, tab, or
/// modifier combinations. This is the same path every Mac app uses
/// for programmatic keyboard input — focus-steal concerns don't
/// apply because the events route directly to the target pid.
fn send_key(route: &WindowRoute, x11_keycode: u32, x11_state: u16, down: bool) {
    let Some(vk) = x11_keycode_to_mac_vk(x11_keycode) else {
        warn!("send_key: no macOS virtual key for X11 keycode {x11_keycode}");
        return;
    };
    let flags = x11_state_to_cg_flags(x11_state);

    let Some(cg_event) = CGEvent::new_keyboard_event(None, vk, down) else {
        warn!("CGEventCreateKeyboardEvent returned null for vk={vk:#04x}");
        return;
    };
    CGEvent::set_flags(Some(&cg_event), flags);

    // Post via SkyLight first (Chromium needs CGSTickleActivityMonitor),
    // fall back to public CGEvent.postToPid.
    let raw_event_ptr: *mut std::os::raw::c_void =
        (&*cg_event as *const CGEvent) as *mut std::os::raw::c_void;
    let sky = probe();

    // Attach the auth-message envelope before posting. Required by
    // strict synthetic-event filters on macOS 14+ (Chromium, kitty,
    // and probably others) — without it the event reaches the
    // target's mach port but is filtered out before reaching the
    // app's keyboard pipeline. Cua's `SkyLightEventPost.postToPid`
    // attaches by default for keyboard (`attachAuthMessage: true`)
    // and only skips it on the mouse path.
    let _ = crate::skylight::attach_auth_message(raw_event_ptr, route.pid);

    let posted_via_skylight = if let Some(post) = sky.fns.as_ref().map(|f| f.post_to_pid) {
        unsafe { post(route.pid, raw_event_ptr) };
        true
    } else {
        false
    };
    if !posted_via_skylight {
        CGEvent::post_to_pid(route.pid, Some(&cg_event));
    }
}

/// Map an X11 keycode (as produced by the frontend's
/// `browserKeyToX11Keycode` — keyed on the browser's `event.code`,
/// not `event.key`) to a macOS Carbon virtual-key value
/// (`kVK_*`). Returns `None` for keycodes we don't translate; the
/// caller logs and drops those rather than guessing.
fn x11_keycode_to_mac_vk(keycode: u32) -> Option<u16> {
    Some(match keycode {
        // First row.
        9 => 0x35,  // Escape -> kVK_Escape
        10 => 0x12, // 1
        11 => 0x13, // 2
        12 => 0x14, // 3
        13 => 0x15, // 4
        14 => 0x17, // 5
        15 => 0x16, // 6
        16 => 0x1A, // 7
        17 => 0x1C, // 8
        18 => 0x19, // 9
        19 => 0x1D, // 0
        20 => 0x1B, // -  (Minus)
        21 => 0x18, // =  (Equal)
        22 => 0x33, // Backspace -> kVK_Delete
        // QWERTY row.
        23 => 0x30, // Tab
        24 => 0x0C, // q
        25 => 0x0D, // w
        26 => 0x0E, // e
        27 => 0x0F, // r
        28 => 0x11, // t
        29 => 0x10, // y
        30 => 0x20, // u
        31 => 0x22, // i
        32 => 0x1F, // o
        33 => 0x23, // p
        34 => 0x21, // [
        35 => 0x1E, // ]
        36 => 0x24, // Return
        // Modifiers and ASDF row.
        37 => 0x3B, // Control_L
        38 => 0x00, // a
        39 => 0x01, // s
        40 => 0x02, // d
        41 => 0x03, // f
        42 => 0x05, // g
        43 => 0x04, // h
        44 => 0x26, // j
        45 => 0x28, // k
        46 => 0x25, // l
        47 => 0x29, // ;
        48 => 0x27, // '
        49 => 0x32, // `  (Backquote)
        50 => 0x38, // Shift_L
        51 => 0x2A, // \
        // ZXCV row.
        52 => 0x06, // z
        53 => 0x07, // x
        54 => 0x08, // c
        55 => 0x09, // v
        56 => 0x0B, // b
        57 => 0x2D, // n
        58 => 0x2E, // m
        59 => 0x2B, // ,
        60 => 0x2F, // .
        61 => 0x2C, // /
        62 => 0x3C, // Shift_R
        // Bottom row.
        63 => 0x43, // Numpad *
        64 => 0x3A, // Alt_L (Option)
        65 => 0x31, // Space
        66 => 0x39, // CapsLock
        // F-keys.
        67 => 0x7A, // F1
        68 => 0x78, // F2
        69 => 0x63, // F3
        70 => 0x76, // F4
        71 => 0x60, // F5
        72 => 0x61, // F6
        73 => 0x62, // F7
        74 => 0x64, // F8
        75 => 0x65, // F9
        76 => 0x6D, // F10
        95 => 0x67, // F11
        96 => 0x6F, // F12
        // Arrows + nav.
        105 => 0x3E, // Control_R
        108 => 0x3D, // Alt_R (Option_R)
        110 => 0x73, // Home
        111 => 0x7E, // ArrowUp
        112 => 0x74, // PageUp
        113 => 0x7B, // ArrowLeft
        114 => 0x7C, // ArrowRight
        115 => 0x77, // End
        116 => 0x7D, // ArrowDown
        117 => 0x79, // PageDown
        119 => 0x75, // Delete (Forward)
        // Meta / Cmd.
        133 => 0x37, // Meta_L (Cmd)
        134 => 0x36, // Meta_R (Cmd_R)
        _ => return None,
    })
}

/// X11 modifier-state mask (bits 0..7) → `CGEventFlags`. Mirrors
/// the modifier vocabulary cua's `KeyboardInput.modifierMask(for:)`
/// uses, but takes its input from our protocol's already-encoded
/// state field rather than from named strings.
fn x11_state_to_cg_flags(state: u16) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    // Bit 0 = Shift, 1 = Lock, 2 = Control, 3 = Mod1 (Alt),
    // 6 = Mod4 (Super/Cmd) — standard X11 modifier mapping.
    if state & 0x01 != 0 {
        flags |= CGEventFlags::MaskShift;
    }
    if state & 0x02 != 0 {
        flags |= CGEventFlags::MaskAlphaShift;
    }
    if state & 0x04 != 0 {
        flags |= CGEventFlags::MaskControl;
    }
    if state & 0x08 != 0 {
        flags |= CGEventFlags::MaskAlternate;
    }
    if state & 0x40 != 0 {
        flags |= CGEventFlags::MaskCommand;
    }
    flags
}

/// Try to resolve an AX element at the click point and dispatch
/// `AXPress` against it. Returns `Ok(true)` on a successful AX
/// click, `Ok(false)` when no element resolves (caller should fall
/// back to pixel), `Err` on AX failures we want surfaced.
fn try_ax_click(route: &WindowRoute, target: CGPoint) -> Result<bool, ax::AxError> {
    if !ax::is_authorized() {
        warn!("AX click skipped: Accessibility permission not granted");
        return Ok(false);
    }
    // Hit-test within the **target app's AX tree**, not the
    // system-wide root. cua's setup hit-tests system-wide because
    // the target is at the visual top in their use case (no local
    // user). For us, the user's browser is on top — system-wide
    // hit-test would return a browser element. Using the
    // application root traverses only the target pid's tree, so we
    // get the right element regardless of visual z-order.
    let app_root = ax::application_root(route.pid);
    let element = match ax::element_at_point(&app_root, target.x as f32, target.y as f32) {
        Ok(e) => e,
        Err(ax::AxError::NoElementAt) => {
            tracing::info!(
                "AX click: no element at ({:.0},{:.0}) in pid {}",
                target.x,
                target.y,
                route.pid
            );
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let elem_pid = ax::pid_of(&element);
    tracing::info!(
        "AX click: dispatching AXPress to element in pid {} at ({:.0},{:.0})",
        elem_pid.unwrap_or(-1),
        target.x,
        target.y
    );

    let window = ax::enclosing_window(&element);
    let handle = focus_guard::arm(route.pid, window, Some(element.clone()));

    let result = ax::perform_action(&element, "AXPress");
    tracing::info!("AX click: AXPress result = {result:?}");

    // Always release; cua's preventer matches a 50 ms delay to give
    // the activation a chance to fire before our restore.
    focus_guard::release(handle, std::time::Duration::from_millis(50));

    match result {
        Ok(()) => Ok(true),
        Err(ax::AxError::ActionFailed { .. }) => {
            // Action failed — element doesn't support AXPress (e.g.
            // a static label hit by a coord). Logged and dropped.
            tracing::info!("AX click: AXPress unsupported");
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn window_local_to_screen(route: &WindowRoute, x: i16, y: i16) -> CGPoint {
    CGPoint {
        x: route.bounds.x + x as f64,
        y: route.bounds.y + y as f64,
    }
}

/// X11 scroll-button → (vertical_delta, horizontal_delta) pair in
/// "line" units. Frontend's wheel handler emits a Press/Release
/// pair every `THRESHOLD` deltaY of the user's wheel, mapping
/// vertical scroll to buttons 4/5 and horizontal to 6/7. Each tick
/// becomes one line-scroll event on our side.
fn scroll_delta_for_button(x11_button: u8) -> Option<(i32, i32)> {
    match x11_button {
        4 => Some((1, 0)),  // wheel up — positive y for natural scroll
        5 => Some((-1, 0)), // wheel down
        6 => Some((0, -1)), // horizontal left
        7 => Some((0, 1)),  // horizontal right
        _ => None,
    }
}

/// Synthesize a `CGEventCreateScrollWheelEvent2` line-tick and post
/// it to `route.pid` through SkyLight (with auth-message envelope,
/// same path as keyboard).
///
/// Compatibility (per cua's `ScrollTool.swift` documentation):
///   - **AppKit-native** (TextEdit, Finder, Cocoa scroll views):
///     scrolls cleanly.
///   - **Chromium** (Arc, Chrome): silently dropped. SkyLight has
///     no Scroll-specific auth subclass, the factory falls back to
///     the parent class, and Chromium rejects parent-authed wheel
///     events. cua works around this with synthesized PageDown /
///     ArrowDown keystrokes — we may add that later as a fallback.
fn send_scroll(route: &WindowRoute, dy: i32, dx: i32) {
    let Some(cg_event) =
        CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 2, dy, dx, 0)
    else {
        warn!("CGEventCreateScrollWheelEvent2 returned null (dy={dy} dx={dx})");
        return;
    };

    let raw_event_ptr: *mut std::os::raw::c_void =
        (&*cg_event as *const CGEvent) as *mut std::os::raw::c_void;
    // Skip the auth message — same reason the mouse path skips it
    // (cua's `SkyLightEventPost.swift` lines 240-250). The auth
    // envelope forks SkyLight onto a direct-mach delivery path
    // that bypasses `IOHIDPostEvent`, and AppKit's NSScrollView is
    // wired to the IOHIDPostEvent pipeline. Plain unsigned post
    // hits the pipeline scroll handlers actually listen on.

    let sky = probe();
    let posted_via_skylight = if let Some(post) = sky.fns.as_ref().map(|f| f.post_to_pid) {
        unsafe { post(route.pid, raw_event_ptr) };
        true
    } else {
        false
    };
    if !posted_via_skylight {
        CGEvent::post_to_pid(route.pid, Some(&cg_event));
    }
}

/// Map an X11-style button index to a `CGMouseButton`. Returns
/// `None` for indices we don't translate (scroll buttons 4/5/6/7
/// are intercepted at the dispatch site).
fn map_button(x11_button: u8) -> Option<CGMouseButton> {
    match x11_button {
        1 => Some(CGMouseButton::Left),
        2 => Some(CGMouseButton::Center),
        3 => Some(CGMouseButton::Right),
        _ => None,
    }
}
