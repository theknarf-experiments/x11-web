//! Window enumeration via `CGWindowListCopyWindowInfo`.
//!
//! Mirrors cua-driver's `WindowEnumerator.swift`. No TCC required —
//! reads the WindowServer's window list. Returns one `WindowInfo` per
//! window; callers filter by `layer == 0` to drop menubar/dock chrome
//! and by `bounds.width/height > 1` to drop the degenerate placeholders
//! the WindowServer keeps for a few system services.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    self, kCGNullWindowID, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowName,
    kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID, CGWindowID, CGWindowListOption,
};

/// A single entry from CGWindowList. Values match the dictionary keys
/// from `kCGWindow*`; field types are pre-converted from CFNumber into
/// the smallest sensible Rust type.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: CGWindowID,
    pub pid: i32,
    pub owner: String,
    pub name: String,
    pub bounds: WindowBounds,
    pub layer: i32,
    pub on_screen: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// All windows on screen, excluding desktop elements (Finder's desktop,
/// menubar). Returns an empty Vec if the SPI fails — typically because
/// the process isn't permitted to read the window list (which on macOS
/// 14+ shouldn't happen for `optionOnScreenOnly` without TCC, but we
/// defend against it anyway).
pub fn visible_windows() -> Vec<WindowInfo> {
    enumerate(window::kCGWindowListOptionOnScreenOnly | window::kCGWindowListExcludeDesktopElements)
}

fn enumerate(options: CGWindowListOption) -> Vec<WindowInfo> {
    let array = match window::copy_window_info(options, kCGNullWindowID) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(array.len() as usize);
    for entry in array.iter() {
        // Each element of the CFArrayRef returned by CGWindowList is a
        // CFDictionary<CFString, CFType>. The crate's CFArray::iter()
        // yields opaque CFType pointers, so we re-wrap manually.
        let dict_ref = unsafe {
            CFDictionary::<CFString, CFType>::wrap_under_get_rule(
                *entry as core_foundation::dictionary::CFDictionaryRef,
            )
        };
        if let Some(info) = parse(&dict_ref) {
            out.push(info);
        }
    }
    out
}

fn parse(dict: &CFDictionary<CFString, CFType>) -> Option<WindowInfo> {
    let id = number(dict, unsafe { kCGWindowNumber })?.to_i64()? as CGWindowID;
    let pid = number(dict, unsafe { kCGWindowOwnerPID })?.to_i64()? as i32;

    let bounds_key = unsafe { CFString::wrap_under_get_rule(kCGWindowBounds) };
    let bounds_dict = dict
        .find(&bounds_key)
        .and_then(|v| v.clone().downcast::<CFDictionary>())?;
    let bounds = parse_bounds(&bounds_dict)?;

    let owner = string(dict, unsafe { kCGWindowOwnerName }).unwrap_or_default();
    let name = string(dict, unsafe { kCGWindowName }).unwrap_or_default();
    let layer = number(dict, unsafe { kCGWindowLayer })
        .and_then(|n| n.to_i64())
        .unwrap_or(0) as i32;
    let on_screen = boolean(dict, unsafe { kCGWindowIsOnscreen }).unwrap_or(false);

    Some(WindowInfo {
        id,
        pid,
        owner,
        name,
        bounds,
        layer,
        on_screen,
    })
}

fn parse_bounds(dict: &CFDictionary) -> Option<WindowBounds> {
    let x = bounds_field(dict, "X")?;
    let y = bounds_field(dict, "Y")?;
    let w = bounds_field(dict, "Width")?;
    let h = bounds_field(dict, "Height")?;
    Some(WindowBounds {
        x,
        y,
        width: w,
        height: h,
    })
}

fn bounds_field(dict: &CFDictionary, key: &str) -> Option<f64> {
    use core_foundation::base::CFTypeRef;
    let key = CFString::new(key);
    let raw = dict
        .find(key.as_concrete_TypeRef() as *const _ as *const std::ffi::c_void)
        .map(|p| *p)?;
    let cf = unsafe { CFType::wrap_under_get_rule(raw as CFTypeRef) };
    cf.downcast::<CFNumber>().and_then(|n| n.to_f64())
}

fn number(dict: &CFDictionary<CFString, CFType>, key: core_foundation::string::CFStringRef) -> Option<CFNumber> {
    let key_owned = unsafe { CFString::wrap_under_get_rule(key) };
    dict.find(&key_owned).and_then(|v| v.clone().downcast::<CFNumber>())
}

fn string(dict: &CFDictionary<CFString, CFType>, key: core_foundation::string::CFStringRef) -> Option<String> {
    let key_owned = unsafe { CFString::wrap_under_get_rule(key) };
    dict.find(&key_owned)
        .and_then(|v| v.clone().downcast::<CFString>())
        .map(|s| s.to_string())
}

fn boolean(dict: &CFDictionary<CFString, CFType>, key: core_foundation::string::CFStringRef) -> Option<bool> {
    let key_owned = unsafe { CFString::wrap_under_get_rule(key) };
    dict.find(&key_owned)
        .and_then(|v| v.clone().downcast::<CFBoolean>())
        .map(|b| b == CFBoolean::true_value())
}

// Suppress unused warning: CFArray import is needed for trait resolution
// on .len() / .iter() above.
#[allow(dead_code)]
const _: fn() = || {
    let _ = std::mem::size_of::<CFArray<CFType>>();
};
