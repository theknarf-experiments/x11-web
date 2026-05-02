//! Frontend-driven window resize via AX.
//!
//! When the user drags a `WindowFrame`'s resize handle, the
//! frontend sends `BackendToSidecar::ResizeWindow`. We look up the
//! AX window matching the route's screen position and write its
//! `AXSize` attribute. macOS resizes the window for real; the next
//! enumerator tick picks up the new bounds and emits
//! `WindowConfigured` back to the frontend, so the canvas size is
//! still server-authoritative — there's no client-side optimistic
//! resize.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_application_services::{AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGPoint, CGSize};

use crate::ax::application_root;
use crate::router::WindowRoute;
use crate::windows::WindowBounds;

/// Resize the macOS window described by `route` to `width × height`
/// (in screen points). Logs and returns silently on failure — the
/// frontend's drag has already moved the resize handle, and not
/// having any way to notify it that the resize was rejected is
/// acceptable for v1.
pub fn inject_resize(route: WindowRoute, width: u16, height: u16) {
    let Some(window) = find_ax_window(route.pid, route.bounds) else {
        tracing::warn!(
            "resize: no AX window matching pid={} bounds=({:.0},{:.0},{:.0}x{:.0})",
            route.pid,
            route.bounds.x,
            route.bounds.y,
            route.bounds.width,
            route.bounds.height
        );
        return;
    };
    let size = CGSize {
        width: width as f64,
        height: height as f64,
    };
    // SAFETY: pointer is to a stack value alive for the duration
    // of the AXValueCreate call; AXValueCreate copies the bytes.
    let ax_size = unsafe {
        AXValue::new(
            AXValueType::CGSize,
            NonNull::new_unchecked(&size as *const CGSize as *mut c_void),
        )
    };
    let Some(ax_size) = ax_size else {
        tracing::warn!("resize: AXValue::new returned None");
        return;
    };

    let cfkey = CFString::from_str("AXSize");
    // AXValue is a CFType — same trick `set_attribute_bool` uses
    // for CFBoolean. The reference's lifetime is tied to ax_size
    // so the value lives until set_attribute_value returns.
    let cfval_ref: &CFType = unsafe { &*(&*ax_size as *const AXValue as *const CFType) };
    let result = unsafe { window.set_attribute_value(&cfkey, cfval_ref) };
    if result.0 != 0 {
        tracing::warn!("resize: AXSize setter failed code={}", result.0);
    }
}

/// Probe whether the macOS window described by `route` is
/// drag-resizable. We treat "resizable" as `AXSize` being settable —
/// fixed-size apps (e.g., Calculator's Basic mode) report
/// `settable = false`, while normal documents (TextEdit, Safari)
/// report `true`. Returns `true` on probe failure (no AX granted,
/// window vanished mid-tick) so we never falsely lock down a UI we
/// can't introspect — better to leave the handle visible and have
/// the resize attempt no-op than to silently strip controls.
pub fn is_resizable(pid: i32, bounds: WindowBounds) -> bool {
    let Some(window) = find_ax_window(pid, bounds) else {
        return true;
    };
    let cfkey = CFString::from_str("AXSize");
    let mut settable: u8 = 0;
    let result = unsafe {
        window.is_attribute_settable(
            &cfkey,
            NonNull::new_unchecked(&mut settable as *mut u8),
        )
    };
    if result.0 != 0 {
        return true;
    }
    settable != 0
}

/// Walk the app's `AXWindows` and return the one whose `AXPosition`
/// matches `bounds.{x,y}`. Returns `None` for apps that haven't
/// granted AX, or if none of their windows is at the expected
/// position (e.g., the user moved the macOS window between the
/// enumerator's last tick and now).
fn find_ax_window(pid: i32, bounds: WindowBounds) -> Option<CFRetained<AXUIElement>> {
    let app = application_root(pid);
    let arr = attribute_array(&app, "AXWindows")?;
    let n = arr.count();
    for i in 0..n {
        let ptr = unsafe { arr.value_at_index(i) };
        if ptr.is_null() {
            continue;
        }
        let win: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
        let Some(pos) = read_cgpoint(win, "AXPosition") else {
            continue;
        };
        if (pos.x - bounds.x).abs() < 2.0 && (pos.y - bounds.y).abs() < 2.0 {
            return Some(unsafe {
                CFRetained::retain(NonNull::new_unchecked(win as *const _ as *mut AXUIElement))
            });
        }
    }
    None
}

fn attribute_array(element: &AXUIElement, attribute: &str) -> Option<CFRetained<CFArray>> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    cf.downcast::<CFArray>().ok()
}

/// Read a `kAXValueTypeCGPoint`-encoded attribute (notably
/// `AXPosition`) into a plain `CGPoint`. Returns `None` for
/// missing / wrong-typed attributes.
fn read_cgpoint(element: &AXUIElement, attribute: &str) -> Option<CGPoint> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    let ax_value = cf.downcast::<AXValue>().ok()?;
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let ok = unsafe {
        ax_value.value(
            AXValueType::CGPoint,
            NonNull::new_unchecked(&mut point as *mut CGPoint as *mut c_void),
        )
    };
    if !ok {
        return None;
    }
    Some(point)
}
