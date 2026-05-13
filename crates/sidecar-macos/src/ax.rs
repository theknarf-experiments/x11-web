//! Thin wrappers over `AXUIElement` from `ApplicationServices`.
//!
//! Mirrors the surface cua-driver's `AXInput.swift` exposes — the
//! handful of functions we need to drive the AX click path:
//!
//!   - `system_wide()` — returns the system-wide AX root used for
//!     screen-coordinate hit-testing.
//!   - `element_at_point(...)` — `AXUIElementCopyElementAtPosition`.
//!     Yields whichever AX node lives at the given screen point
//!     (top-left origin, points). System-wide root traverses across
//!     processes / windows, so this is the standard way to address
//!     a click target by coord.
//!   - `enclosing_window(...)` — walks up `kAXWindowAttribute` until
//!     it finds the window node that contains the element. Needed by
//!     `FocusGuard` Layer 2 (synthetic focus).
//!   - `perform_action(...)` — `AXUIElementPerformAction`. Dispatches
//!     `AXPress` / `AXShowMenu` / etc. against an element; pure RPC,
//!     no synthesized mouse events involved, no focus disturbance
//!     when wrapped in `FocusGuard`.
//!   - `set_attribute_bool(...)` / `attribute_value(...)` — generic
//!     property accessors used by `FocusGuard` Layer 2 to write
//!     `AXFocused` / `AXMain` and read them back to restore state.

use std::ptr::NonNull;

use objc2_application_services::AXUIElement;
use objc2_core_foundation::{
    kCFBooleanFalse, kCFBooleanTrue, CFArray, CFRetained, CFString, CFType,
};

#[derive(Debug)]
pub enum AxError {
    NotAuthorized,
    NoElementAt,
    AttributeMissing(&'static str),
    AttributeFailed { attribute: &'static str, code: i32 },
    ActionFailed { action: &'static str, code: i32 },
}

impl std::fmt::Display for AxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "Accessibility permission not granted"),
            Self::NoElementAt => write!(f, "no AX element at point"),
            Self::AttributeMissing(a) => write!(f, "attribute {a} not present on element"),
            Self::AttributeFailed { attribute, code } => {
                write!(f, "AX setAttribute {attribute} failed with code {code}")
            }
            Self::ActionFailed { action, code } => {
                write!(f, "AX action {action} failed with code {code}")
            }
        }
    }
}

impl std::error::Error for AxError {}

/// `AXUIElementCreateSystemWide()` — wrapper. Used as the root for
/// screen-coordinate hit-tests.
pub fn system_wide() -> CFRetained<AXUIElement> {
    unsafe { AXUIElement::new_system_wide() }
}

/// `AXUIElementCreateApplication(pid)` — wrapper. Returns the AX
/// root for a specific process. Used by `FocusGuard` for Layer 1
/// (`AXManualAccessibility`/`AXEnhancedUserInterface` enablement).
pub fn application_root(pid: i32) -> CFRetained<AXUIElement> {
    unsafe { AXUIElement::new_application(pid) }
}

/// `AXUIElementCopyElementAtPosition(systemWide, x, y, &element)` —
/// returns the AX node at the given screen point. Coordinates are
/// in points, top-left origin (matches what we already compute via
/// `WindowRoute.bounds + window-local`).
pub fn element_at_point(
    root: &AXUIElement,
    x: f32,
    y: f32,
) -> Result<CFRetained<AXUIElement>, AxError> {
    let mut out: *const AXUIElement = std::ptr::null();
    let result = unsafe { root.copy_element_at_position(x, y, NonNull::new_unchecked(&mut out)) };
    if result.0 != 0 || out.is_null() {
        return Err(AxError::NoElementAt);
    }
    // SAFETY: AX returns a +1 retained AXUIElement on success; we
    // wrap and own that retain via `from_raw`.
    Ok(unsafe { CFRetained::from_raw(NonNull::new_unchecked(out as *mut AXUIElement)) })
}

/// Walk up `kAXWindowAttribute` until we find the enclosing window,
/// or hit a node without an `AXWindow` attribute. Mirrors cua's
/// `enclosingWindow(of:)`.
pub fn enclosing_window(element: &AXUIElement) -> Option<CFRetained<AXUIElement>> {
    attribute_element(element, "AXWindow")
}

/// `AXUIElementCopyAttributeValue(element, attribute, &value)`,
/// returning the value as a `CFRetained<AXUIElement>` if the value
/// is itself an AX element. `None` for missing or non-AX-element
/// attributes.
pub fn attribute_element(
    element: &AXUIElement,
    attribute: &str,
) -> Option<CFRetained<AXUIElement>> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    // Re-interpret as AXUIElement by taking ownership of the raw
    // pointer. The CFType returned is the AX element itself when the
    // attribute is an AX node.
    Some(unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut AXUIElement)) })
}

/// Read a String-typed attribute (e.g. `AXRole`, `AXSubrole`).
/// `None` when the attribute is missing or isn't a `CFString`. Used
/// by the click path to walk parent chains looking for an `AXRow`
/// ancestor when `AXPress` returns `kAXErrorActionUnsupported`.
pub fn attribute_string(element: &AXUIElement, attribute: &str) -> Option<String> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    cf.downcast::<CFString>().ok().map(|s| s.to_string())
}

/// Read a Boolean attribute. `None` when the attribute is missing
/// or not a CFBoolean.
pub fn attribute_bool(element: &AXUIElement, attribute: &str) -> Option<bool> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    cf.downcast::<objc2_core_foundation::CFBoolean>()
        .ok()
        .map(|b| b.value())
}

/// `AXUIElementSetAttributeValue` for a Boolean-valued attribute.
/// Used by `FocusGuard` Layer 2 to write `AXFocused` / `AXMain` /
/// `AXManualAccessibility` / `AXEnhancedUserInterface`.
pub fn set_attribute_bool(
    element: &AXUIElement,
    attribute: &'static str,
    value: bool,
) -> Result<(), AxError> {
    let cfkey = CFString::from_str(attribute);
    let cfbool = unsafe {
        if value {
            kCFBooleanTrue.expect("kCFBooleanTrue static missing")
        } else {
            kCFBooleanFalse.expect("kCFBooleanFalse static missing")
        }
    };
    // The set_attribute_value signature wants `&CFType`; CFBoolean
    // is a CFType so a cast through the as-CFType ref reaches the
    // right type.
    let cfval_ref: &CFType = unsafe { &*(cfbool as *const _ as *const CFType) };
    let result = unsafe { element.set_attribute_value(&cfkey, cfval_ref) };
    if result.0 != 0 {
        return Err(AxError::AttributeFailed {
            attribute,
            code: result.0,
        });
    }
    Ok(())
}

/// `AXUIElementPerformAction(element, action)`. The AX way to
/// dispatch a click / show-menu / etc. without going through any
/// CGEvent stream.
pub fn perform_action(element: &AXUIElement, action: &'static str) -> Result<(), AxError> {
    let cfkey = CFString::from_str(action);
    let result = unsafe { element.perform_action(&cfkey) };
    if result.0 != 0 {
        return Err(AxError::ActionFailed {
            action,
            code: result.0,
        });
    }
    Ok(())
}

/// Names of actions the element advertises. Empty when the element
/// has no actions or the call fails. Used to verify a target
/// element actually supports the action we're about to dispatch.
pub fn advertised_action_names(element: &AXUIElement) -> Vec<String> {
    let mut raw: *const CFArray = std::ptr::null();
    let result = unsafe { element.copy_action_names(NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return Vec::new();
    }
    let _arr = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFArray)) };
    // The CFArray<Opaque>::iter() in objc2-core-foundation 0.3 is
    // awkward to use here — the action-list is a v1 nice-to-have,
    // not on the click hot-path. Return empty for now; we'll fill
    // this in via low-level CFArrayGetCount/GetValueAtIndex when a
    // caller actually needs it.
    Vec::new()
}

/// Convenience: process ID that owns the AX element. Useful for
/// FocusGuard's "is target already frontmost?" check and for
/// applying enablement attributes against the right pid.
pub fn pid_of(element: &AXUIElement) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    extern "C-unwind" {
        fn AXUIElementGetPid(element: &AXUIElement, pid: *mut libc::pid_t) -> i32;
    }
    let result = unsafe { AXUIElementGetPid(element, &mut pid) };
    if result != 0 {
        return None;
    }
    Some(pid)
}

/// Verify Accessibility TCC is granted. Wraps `AXIsProcessTrusted`.
pub fn is_authorized() -> bool {
    unsafe { objc2_application_services::AXIsProcessTrusted() }
}
