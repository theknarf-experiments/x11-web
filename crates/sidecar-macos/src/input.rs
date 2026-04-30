//! Synthesize mouse events from frontend `InputEvent`s and post them
//! to the target app's process.
//!
//! Direct port of cua-driver's `clickViaAuthSignedPost` recipe in
//! `MouseInput.swift`. The recipe — verbatim:
//!
//!   1. `FocusWithoutRaise.activateWithoutRaise(pid, cg_id)` —
//!      flips AppKit-active to the target without restacking. Sleep
//!      50 ms for WindowServer to commit the focus change.
//!   2. Stamped `mouseMoved` at the target screen point.
//!   3. Sleep 15 ms.
//!   4. Stamped `leftMouseDown` at off-screen `(-1, -1)` — primer
//!      that satisfies Chromium's user-activation gate without
//!      hitting any DOM element.
//!   5. Sleep 1 ms.
//!   6. Stamped `leftMouseUp` at `(-1, -1)`.
//!   7. Sleep 100 ms.
//!   8. Stamped `leftMouseDown` at the target.
//!   9. Sleep 1 ms.
//!  10. Stamped `leftMouseUp` at the target.
//!
//! Each event carries cua's full stamp: `mouseEventClickState=1`,
//! `mouseEventButtonNumber`, `mouseEventSubtype=3`,
//! `mouseEventWindowUnderMousePointer{,ThatCanHandleThisEvent}`,
//! `CGEventSetWindowLocation` (window-local point), and SkyLight
//! field 40 (target pid). Posted via `SLEventPostToPid` *without*
//! the auth message envelope — per cua's note, attaching the auth
//! envelope forks SkyLight onto a direct-mach delivery path that
//! bypasses `IOHIDPostEvent`, which Chromium's window-event handler
//! is the one subscribing to.
//!
//! Our protocol delivers `ButtonPress` and `ButtonRelease` as
//! separate frontend events (the user is press-and-hold-capable in
//! the canvas), so we run the primer prologue + target-down on
//! `ButtonPress` and the target-up on `ButtonRelease`. Drag works
//! because press-hold-release semantics are preserved; the only
//! thing not matching cua's tight 1 ms target down→up timing is the
//! span between user press and release, which AppKit / Chromium
//! tolerate.
//!
//! Right- and middle-click skip the primer prologue (cua's recipe is
//! left-click-only) but keep `FocusWithoutRaise`, the field stamps,
//! and dual posting.

use std::os::raw::c_void;
use std::time::Duration;

use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSEvent, NSEventType, NSRunningApplication, NSWorkspace,
};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{CGEvent, CGEventField, CGEventType, CGMouseButton};
use tracing::warn;
use x11_web_protocol::InputEvent;

use crate::ax;
use crate::focus::activate_without_raise;
use crate::focus_guard;
use crate::router::WindowRoute;
use crate::skylight::probe;

/// Off-screen primer target. Negative on both axes so no on-screen
/// window — including the menubar strip — claims the coord; Chromium
/// discards the click but the user-activation gate still ticks.
const PRIMER_POINT: CGPoint = CGPoint { x: -1.0, y: -1.0 };

/// Inject a single browser `InputEvent` against the window described
/// by `route`. Logs and skips silently for event kinds we don't
/// support yet (scroll, key, touch, gestures).
///
/// Click dispatch tries cua's two-path strategy:
///
///   1. **AX click** — `AXUIElementCopyElementAtPosition` finds an
///      AX-addressable element at the screen point; if found, dispatch
///      `AXPress` wrapped in `FocusGuard` (Layer 2 + Layer 3). Pure
///      RPC, no synthetic mouse events, no focus disturbance.
///   2. **Pixel fallback** — when no AX element resolves at the point
///      (canvas surfaces, OpenGL viewports, web content rendered
///      outside the AX tree), fall back to the SkyLight primer-click
///      recipe. This path *does* visibly raise the target and steal
///      focus; cua accepts the same trade-off (`MouseInput.swift`
///      file-level docs note pixel clicks are best-effort and direct
///      callers needing reliability to the AX path).
pub fn inject(route: WindowRoute, event: InputEvent) {
    match event {
        InputEvent::ButtonPress { button, x, y, .. } => {
            let Some(cg_button) = map_button(button) else {
                return;
            };
            // AX-only mode: drop pixel fallback so we can isolate
            // FocusGuard / AXPress behaviour without the SkyLight
            // primer recipe stepping on it. Re-enable the fallback
            // (`run_full_click`) once AX is dialled in.
            let _ = cg_button;
            let target = window_local_to_screen(&route, x, y);
            match try_ax_click(&route, target) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::info!("AX click did not dispatch (no element / wrong pid / unsupported)");
                }
                Err(e) => {
                    tracing::warn!("AX click error: {e}");
                }
            }
        }
        InputEvent::ButtonRelease { .. } => {
            // No-op for AX-only mode.
        }
        InputEvent::MotionNotify { x, y, .. } => {
            let _ = (x, y);
            // No-op for AX-only mode — mouseMoved doesn't help AX
            // dispatch.
        }
        _ => {}
    }
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
                "AX click: no element at ({:.0},{:.0}) in pid {} — falling back to pixel",
                target.x, target.y, route.pid
            );
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let elem_pid = ax::pid_of(&element);
    tracing::info!(
        "AX click: dispatching AXPress to element in pid {} at ({:.0},{:.0})",
        elem_pid.unwrap_or(-1), target.x, target.y
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
            // a static label hit by a coord). Treat as "no AX click
            // was possible" so caller falls back to pixel.
            tracing::info!("AX click: AXPress unsupported — falling back to pixel");
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Full click recipe — cua's `clickViaAuthSignedPost` ported in
/// full. Prologue + primer + target down + 1 ms + target up, all
/// emitted in one atomic call. Mirrors cua's
/// `post(move) … post(pair.down); usleep(1_000); post(pair.up)`.
///
/// Also wraps cua's "layer-3 reactive preventer"
/// (`SystemFocusStealPreventer`): captures whichever app is
/// frontmost before the click and re-activates it ~50 ms after.
/// Without this restore, the click promotes the target to AppKit-
/// active and raises its window — leaving the user's previous app
/// in a half-focused state. cua's tests acknowledge a brief flash
/// during this window; what matters is that focus ends up back on
/// the original.
fn run_full_click(
    route: &WindowRoute,
    cg_button: CGMouseButton,
    target: CGPoint,
    window_local: (i16, i16),
) {
    // Capture the previously frontmost app so we can restore focus
    // after the click. Skip the capture if the target is already
    // frontmost (no point re-activating ourselves).
    let restore_to = capture_restore_target(route.pid);

    // Prologue: focus the target without raising. cua sleeps 50 ms
    // here to let WindowServer commit the focus change before the
    // event stream starts.
    if activate_without_raise(route.pid, route.cg_id) {
        std::thread::sleep(Duration::from_millis(50));
    }

    // Right/middle clicks skip the primer entirely — the primer
    // recipe is left-click-only in cua's code (see `click(...)`'s
    // dispatch logic guarding `clickViaAuthSignedPost` on
    // `button == .left`).
    let do_primer = matches!(cg_button, CGMouseButton::Left);

    // mouseMoved at target. Sets the cursor-state Chrome and AppKit
    // expect to see before the click sequence begins.
    post_stamped(route, CGEventType::MouseMoved, None, target, window_local);

    if do_primer {
        // 15 ms — "one frame+ after mouseMoved for cursor state."
        std::thread::sleep(Duration::from_millis(15));

        // Primer down/up at (-1, -1). Stamped with the SAME pid /
        // window field stamps as the target click; the primer is
        // not a routed-elsewhere click, it's a no-op coord chosen
        // to avoid hitting a DOM element while still making
        // Chromium's user-activation counter tick forward.
        post_stamped(
            route,
            CGEventType::LeftMouseDown,
            Some(CGMouseButton::Left),
            PRIMER_POINT,
            (-1, -1),
        );
        std::thread::sleep(Duration::from_millis(1));
        post_stamped(
            route,
            CGEventType::LeftMouseUp,
            Some(CGMouseButton::Left),
            PRIMER_POINT,
            (-1, -1),
        );

        // 100 ms — "≥1 frame so Chromium sees primer + target as
        // separate gestures, not a run-on."
        std::thread::sleep(Duration::from_millis(100));
    }

    // Target down + 1 ms + target up — cua's atomic click. The 1 ms
    // is "below system double-click threshold, clear of coalescing"
    // and short enough that AppKit's mouse-down handlers don't
    // promote the target window to key+raise during the hold.
    post_stamped(route, down_type(cg_button), Some(cg_button), target, window_local);
    std::thread::sleep(Duration::from_millis(1));
    post_stamped(route, up_type(cg_button), Some(cg_button), target, window_local);

    // Layer 3 — restore focus to whichever app was frontmost before
    // we started. cua's `SystemFocusStealPreventer` does this via an
    // NSWorkspace.didActivateApplicationNotification observer that
    // schedules `restoreTo.activate(options: [])` ~50 ms after the
    // target self-activates. We do the simpler unconditional
    // version: 50 ms sleep, then `activate()`. Same end state.
    if let Some(app) = restore_to {
        let restore_pid = unsafe { app.processIdentifier() };
        let restore_name = unsafe { app.localizedName() }
            .map(|s| s.to_string())
            .unwrap_or_default();
        std::thread::sleep(Duration::from_millis(50));
        // `ActivateAllWindows` brings every window of the app
        // forward, not just main+key. Without it, Arc's main
        // window gets restacked but our captured app's window
        // pile may still sit behind Calculator's freshly-raised
        // one. The `[]` empty options cua uses gets you the
        // default "main + key only" behaviour, which apparently
        // isn't enough on macOS 15+ for the user-visible result.
        let ok = unsafe {
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows)
        };
        let now_front_pid = unsafe {
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .map(|a| a.processIdentifier())
                .unwrap_or(-1)
        };
        tracing::info!(
            "Focus restore: target_pid={} restore_to_pid={} ({:?}) activate_ok={} frontmost_after={}",
            route.pid, restore_pid, restore_name, ok, now_front_pid,
        );
    } else {
        tracing::info!("Focus restore skipped: no prior frontmost or target was already frontmost");
    }
}

/// Capture `NSWorkspace.shared.frontmostApplication` for later
/// restoration, but only if it isn't the click target itself
/// (re-activating the same app we just clicked is a no-op that
/// would also flash the target's window briefly).
fn capture_restore_target(target_pid: i32) -> Option<Retained<NSRunningApplication>> {
    let workspace = unsafe { NSWorkspace::sharedWorkspace() };
    let front = unsafe { workspace.frontmostApplication() }?;
    if unsafe { front.processIdentifier() } == target_pid {
        return None;
    }
    Some(front)
}

/// Build, stamp, and post a single mouse event. Mirrors cua's
/// `stamp(...)` + `post(...)` pair.
///
/// Event construction goes through the **NSEvent bridge** —
/// `+[NSEvent mouseEventWithType:...].CGEvent` — rather than raw
/// `CGEventCreateMouseEvent`. cua's note (line 26-30 of
/// MouseInput.swift): "raw-CGEvent-built events are silently
/// filtered at the renderer IPC boundary. Switching to the
/// NSEvent-bridged path fixed Chromium web-content hit-tests on
/// backgrounded targets." Empirically also required for AppKit
/// targets to honour per-pid posted events.
fn post_stamped(
    route: &WindowRoute,
    event_type: CGEventType,
    button: Option<CGMouseButton>,
    screen_point: CGPoint,
    window_local: (i16, i16),
) {
    // Map our CGEventType → NSEventType for the bridge constructor.
    // Same type space, different naming.
    let ns_type = match event_type {
        CGEventType::LeftMouseDown => NSEventType::LeftMouseDown,
        CGEventType::LeftMouseUp => NSEventType::LeftMouseUp,
        CGEventType::RightMouseDown => NSEventType::RightMouseDown,
        CGEventType::RightMouseUp => NSEventType::RightMouseUp,
        CGEventType::OtherMouseDown => NSEventType::OtherMouseDown,
        CGEventType::OtherMouseUp => NSEventType::OtherMouseUp,
        CGEventType::MouseMoved => NSEventType::MouseMoved,
        _ => return,
    };

    // cua's `clickViaAuthSignedPost.makeEvent` passes
    // `location: .zero` and re-stamps the real screen point via
    // `event.location = screenPt` after the bridge returns. We
    // match exactly.
    let zero_loc = objc2_foundation::NSPoint { x: 0.0, y: 0.0 };

    let click_count: isize = if matches!(
        event_type,
        CGEventType::LeftMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::RightMouseDown
            | CGEventType::RightMouseUp
            | CGEventType::OtherMouseDown
            | CGEventType::OtherMouseUp
    ) {
        1
    } else {
        0
    };

    // windowNumber: cua's `buildCGEvent` doc (line 656-661):
    // "pass the actual CGWindowID when targeting a specific
    // backgrounded window. When 0, NSApp.sendEvent falls back to a
    // screen-location hit-test that skips non-key windows, so
    // backgrounded AppKit targets never receive the event. The
    // actual window ID routes via NSApplication.window(with:)
    // directly, bypassing the key-window restriction." Critical for
    // our use case — we're always targeting a backgrounded window.
    let window_number: isize = route.cg_id as isize;

    let ns_event = unsafe {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            ns_type,
            zero_loc,
            objc2_app_kit::NSEventModifierFlags::empty(),
            0.0,
            window_number,
            None,
            0,
            click_count,
            1.0,
        )
    };
    let Some(ns_event) = ns_event else {
        warn!("NSEvent.mouseEventWithType returned nil for {event_type:?}");
        return;
    };

    let Some(cg_event) = (unsafe { ns_event.CGEvent() }) else {
        warn!("NSEvent.cgEvent bridge returned nil for {event_type:?}");
        return;
    };

    // The bridge sets event.location to the original Quartz top-
    // left point (cua's note) but cua re-stamps it explicitly. Mirror.
    CGEvent::set_location(Some(&cg_event), screen_point);

    // Stamp the public CGEventFields cua sets on every mouse event.
    let is_button_event = matches!(
        event_type,
        CGEventType::LeftMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::RightMouseDown
            | CGEventType::RightMouseUp
            | CGEventType::OtherMouseDown
            | CGEventType::OtherMouseUp
    );
    if is_button_event {
        let btn_num = match button.unwrap_or(CGMouseButton::Left) {
            CGMouseButton::Left => 0i64,
            CGMouseButton::Right => 1,
            CGMouseButton::Center => 2,
            _ => 0,
        };
        // mouseEventClickState=1: cua's note — Chrome's gate only
        // treats clickState 1 as a real single click; 0 or 2+ have
        // different semantics that don't land. Same for AppKit's
        // hit-tracker.
        CGEvent::set_integer_value_field(
            Some(&cg_event),
            CGEventField::MouseEventClickState,
            1,
        );
        CGEvent::set_integer_value_field(
            Some(&cg_event),
            CGEventField::MouseEventButtonNumber,
            btn_num,
        );
        // NSEventSubtypeMouseEvent — what NSEvent.mouseEvent stamps
        // when called for a regular click. AppKit's renderer checks
        // this; CGEventCreateMouseEvent leaves it at 0.
        CGEvent::set_integer_value_field(
            Some(&cg_event),
            CGEventField::MouseEventSubtype,
            3,
        );
    }

    // mouseEventWindowUnderMousePointer pair — the CGWindowID the
    // event is "under". cua stamps both whenever they have a
    // resolved windowID; we always have one here (the route lookup
    // succeeded by definition).
    let cg_id = route.cg_id as i64;
    if cg_id != 0 {
        CGEvent::set_integer_value_field(
            Some(&cg_event),
            CGEventField::MouseEventWindowUnderMousePointer,
            cg_id,
        );
        CGEvent::set_integer_value_field(
            Some(&cg_event),
            CGEventField::MouseEventWindowUnderMousePointerThatCanHandleThisEvent,
            cg_id,
        );
    }

    // SkyLight private SPIs that cua's stamp() helper applies.
    // `cg_event` is a `Retained<CGEvent>` from the NSEvent bridge;
    // get its raw pointer via deref. SkyLight's `CGEventRef` and the
    // ObjC `__CGEvent *` are the same opaque pointer at the ABI
    // level (cua note in SkyLightEventPost.swift line 32-34).
    let raw_event_ptr: *mut c_void = (&*cg_event as *const CGEvent) as *mut c_void;
    let sky = probe();
    if let Some(set_window_loc) = sky.fns.as_ref().and_then(|f| f.set_window_location) {
        unsafe {
            set_window_loc(
                raw_event_ptr,
                window_local.0 as f64,
                window_local.1 as f64,
            );
        }
    }
    if let Some(set_int) = sky.fns.as_ref().and_then(|f| f.set_int_field) {
        // Field 40 — cua's note: "Chromium's synthetic-event filter
        // latches onto this — missing it = click dropped." AppKit
        // appears to use it too based on cua's testing.
        unsafe {
            set_int(raw_event_ptr, 40, route.pid as i64);
        }
    }

    // Stamp the event timestamp to current uptime nanoseconds. cua's
    // `post()` helper does this immediately before each post:
    // `event.timestamp = clock_gettime_nsec_np(CLOCK_UPTIME_RAW)`.
    // Without this, AppKit / Chromium see timestamp=0 and may
    // discard the event as stale.
    let ts = uptime_nanoseconds();
    CGEvent::set_timestamp(Some(&cg_event), ts);

    // Post via SkyLight's per-pid path (no auth message — that
    // forks onto direct-mach which Chromium's window-event handler
    // doesn't subscribe to). Falls back to the public
    // `CGEvent.postToPid` if the SkyLight SPI isn't resolvable; in
    // practice it always is.
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

/// `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` — Darwin SPI returning
/// nanoseconds since boot, monotonic, unaffected by clock changes.
/// Same routine cua's `post(...)` helper uses to stamp event
/// timestamps. CGEvent's timestamp field is treated as ns on modern
/// macOS.
fn uptime_nanoseconds() -> u64 {
    extern "C" {
        fn clock_gettime_nsec_np(clk_id: u32) -> u64;
    }
    // CLOCK_UPTIME_RAW = 8 on Darwin (sys/_clock_id.h).
    const CLOCK_UPTIME_RAW: u32 = 8;
    unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
}

fn window_local_to_screen(route: &WindowRoute, x: i16, y: i16) -> CGPoint {
    CGPoint {
        x: route.bounds.x + x as f64,
        y: route.bounds.y + y as f64,
    }
}

/// Map an X11-style button index to a `CGMouseButton`. Returns
/// `None` for indices we don't translate (scroll buttons 4/5/6/7).
fn map_button(x11_button: u8) -> Option<CGMouseButton> {
    match x11_button {
        1 => Some(CGMouseButton::Left),
        2 => Some(CGMouseButton::Center),
        3 => Some(CGMouseButton::Right),
        _ => None,
    }
}

fn down_type(b: CGMouseButton) -> CGEventType {
    match b {
        CGMouseButton::Left => CGEventType::LeftMouseDown,
        CGMouseButton::Right => CGEventType::RightMouseDown,
        _ => CGEventType::OtherMouseDown,
    }
}

fn up_type(b: CGMouseButton) -> CGEventType {
    match b {
        CGMouseButton::Left => CGEventType::LeftMouseUp,
        CGMouseButton::Right => CGEventType::RightMouseUp,
        _ => CGEventType::OtherMouseUp,
    }
}
