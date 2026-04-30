//! Two-layer focus guard for AX-dispatched actions on backgrounded
//! apps. Direct port of cua-driver's `FocusGuard` + the underlying
//! `SyntheticAppFocusEnforcer` and `SystemFocusStealPreventer`,
//! minus Layer 1 (`AXEnablementAssertion` — only matters for
//! Chromium/Electron, can be added later).
//!
//! What each layer does:
//!
//!   - **Layer 2 — synthetic focus** (`SyntheticAppFocusEnforcer`):
//!     write `AXFocused = true` and `AXMain = true` on the target's
//!     enclosing window, plus `AXFocused = true` on the element
//!     itself, just before the AX action. AppKit's responder chain
//!     reads these attributes when deciding "is this click a real
//!     user click on a focused element?" — without the synthetic
//!     write, AppKit treats the action as untrusted and the target
//!     app's `applicationDidBecomeActive` reflex flips the target
//!     to the system-wide frontmost app.
//!
//!   - **Layer 3 — system focus-steal preventer**
//!     (`SystemFocusStealPreventer`): capture
//!     `NSWorkspace.shared.frontmostApplication` before the action.
//!     If the target self-activates anyway despite Layer 2, restore
//!     the captured app. cua observes
//!     `NSWorkspace.didActivateApplicationNotification` and schedules
//!     `restoreTo.activate()` ~50 ms after the notification fires;
//!     since our process doesn't run an NSRunLoop, we use a fixed
//!     delay after the action.

use std::time::Duration;

use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFRetained;

use crate::ax;

/// Snapshot of the AX boolean attributes Layer 2 may have rewritten.
/// `with_focus_suppressed` returns one of these to its caller;
/// `restore` uses it to put the originals back. Attributes that
/// couldn't be read on entry stay untouched on exit (writing a fake
/// `false` would be worse than leaving the synthesized `true`).
pub struct FocusState {
    window: Option<CFRetained<AXUIElement>>,
    element: Option<CFRetained<AXUIElement>>,
    prior_window_focused: Option<bool>,
    prior_window_main: Option<bool>,
    prior_element_focused: Option<bool>,
}

/// Apply Layer 2 + Layer 3 around an AX action.
///
/// `window` and `element` are both optional in cua's API for the
/// app-root case where the action targets the whole app rather than
/// a specific element. We mirror that even though our coord-driven
/// path always has both.
///
/// Returns the `FocusState` snapshot Layer 2 captured plus the
/// `NSRunningApplication` Layer 3 saved for restoration. Caller
/// performs the AX action between this and `release(...)`.
pub fn arm(
    target_pid: i32,
    window: Option<CFRetained<AXUIElement>>,
    element: Option<CFRetained<AXUIElement>>,
) -> FocusGuardHandle {
    // Layer 3 — capture frontmost. Skip when target is already
    // frontmost (cua's `isTargetFrontmost` check on FocusGuard.swift
    // line 90).
    let restore_to = capture_restore_target(target_pid);

    // Layer 2 — synthetic focus snapshot + write.
    let prior_window_focused = window
        .as_ref()
        .and_then(|w| ax::attribute_bool(w, "AXFocused"));
    let prior_window_main = window
        .as_ref()
        .and_then(|w| ax::attribute_bool(w, "AXMain"));
    let prior_element_focused = element
        .as_ref()
        .and_then(|e| ax::attribute_bool(e, "AXFocused"));

    if let Some(w) = window.as_ref() {
        let _ = ax::set_attribute_bool(w, "AXFocused", true);
        let _ = ax::set_attribute_bool(w, "AXMain", true);
    }
    if let Some(e) = element.as_ref() {
        let _ = ax::set_attribute_bool(e, "AXFocused", true);
    }

    FocusGuardHandle {
        focus_state: FocusState {
            window,
            element,
            prior_window_focused,
            prior_window_main,
            prior_element_focused,
        },
        restore_to,
    }
}

pub struct FocusGuardHandle {
    focus_state: FocusState,
    restore_to: Option<Retained<NSRunningApplication>>,
}

/// Reverse the modifications `arm(...)` made:
///   - Restore Layer 2's prior attribute values (only those we
///     could read on entry).
///   - Sleep `restore_delay`, then re-activate Layer 3's saved
///     frontmost. cua uses ~50 ms; we expose it as a parameter so
///     callers on slower targets can tune.
pub fn release(handle: FocusGuardHandle, restore_delay: Duration) {
    // Layer 2 — restore.
    let s = handle.focus_state;
    if let Some(w) = s.window.as_ref() {
        if let Some(prior) = s.prior_window_focused {
            let _ = ax::set_attribute_bool(w, "AXFocused", prior);
        }
        if let Some(prior) = s.prior_window_main {
            let _ = ax::set_attribute_bool(w, "AXMain", prior);
        }
    }
    if let (Some(e), Some(prior)) = (s.element.as_ref(), s.prior_element_focused) {
        let _ = ax::set_attribute_bool(e, "AXFocused", prior);
    }

    // Layer 3 — restore previous frontmost. The delay lets the
    // target's activation settle (if it happened despite Layer 2)
    // before we steal focus back; without the wait, our re-activate
    // call can race ahead and the target ends up frontmost.
    if let Some(app) = handle.restore_to {
        std::thread::sleep(restore_delay);
        unsafe {
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
    }
}

/// Capture `NSWorkspace.shared.frontmostApplication` to restore
/// after the action. Returns `None` when the target is already
/// frontmost (no point suppressing self → self).
fn capture_restore_target(target_pid: i32) -> Option<Retained<NSRunningApplication>> {
    let workspace = unsafe { NSWorkspace::sharedWorkspace() };
    let front = unsafe { workspace.frontmostApplication() }?;
    if unsafe { front.processIdentifier() } == target_pid {
        return None;
    }
    Some(front)
}
