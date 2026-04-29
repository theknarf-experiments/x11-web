//! Bridge to SkyLight.framework's private per-pid event-post path.
//!
//! Direct port of cua-driver's `SkyLightEventPost.swift`. The technique:
//!
//!   1. `dlopen` SkyLight (a private framework — no public headers, not
//!      in the SDK).
//!   2. `dlsym` the SPI symbols by name.
//!   3. Call them through `unsafe extern "C"` function pointers.
//!
//! Two reasons we go through SkyLight rather than the public
//! `CGEventPostToPid`:
//!
//!   - **CGSTickleActivityMonitor**: `SLEventPostToPid` calls into it;
//!     `CGEventPostToPid` does not. Without the tickle, Chromium-family
//!     apps drop synthetic keys on the floor (their omnibox keyboard
//!     pipeline checks "is this live input?").
//!
//!   - **Auth-signed events**: macOS 14+ WindowServer gates synthetic
//!     keys against Chromium-like targets on an attached
//!     `SLSEventAuthenticationMessage`. The message is built per-event
//!     via the ObjC factory selector `messageWithEventRecord:pid:version:`
//!     and attached with `SLEventSetAuthenticationMessage`.
//!
//! v0.1 here just resolves the symbols and exposes a `probe()` that
//! reports which subset is available. Real `post_to_pid` / auth-message
//! plumbing lands when we wire keyboard input (v0.3).

use std::os::raw::{c_int, c_void};
use std::sync::OnceLock;

const SKYLIGHT_PATH: &[u8] =
    b"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight\0";

/// `void SLEventPostToPid(pid_t, CGEventRef)`
pub type PostToPidFn = unsafe extern "C" fn(pid: i32, event: *mut c_void);

/// `void SLEventSetAuthenticationMessage(CGEventRef, id)`
pub type SetAuthMessageFn = unsafe extern "C" fn(event: *mut c_void, msg: *mut c_void);

/// `void SLEventSetIntegerValueField(CGEventRef, uint32_t, int64_t)` —
/// SkyLight's raw-field SPI; reaches private fields like f51/f91/f92
/// that the public `CGEventSetIntegerValueField` rejects.
pub type SetIntFieldFn =
    unsafe extern "C" fn(event: *mut c_void, field: u32, value: i64);

/// `CGSConnectionID CGSMainConnectionID(void)` — main SkyLight
/// connection handle for the calling process. Source for the session-id
/// stamp consumed by private mouse-event fields.
pub type ConnectionIdFn = unsafe extern "C" fn() -> u32;

/// `void CGEventSetWindowLocation(CGEventRef, CGPoint)` — stamps a
/// window-local point alongside the screen-space location set via
/// public `CGEventSetLocation`. Lets WindowServer hit-test in
/// window-local space when the target is backgrounded.
pub type SetWindowLocationFn = unsafe extern "C" fn(event: *mut c_void, x: f64, y: f64);

/// `OSStatus SLPSPostEventRecordTo(ProcessSerialNumber*, uint8_t*)` —
/// posts a 248-byte synthetic event record into a process's Carbon
/// queue. Used by the focus-without-raise recipe.
pub type PostEventRecordToFn =
    unsafe extern "C" fn(psn: *const c_void, bytes: *const u8) -> i32;

/// Snapshot of which SPI subset resolved at startup. Cached for the
/// lifetime of the process — symbol availability does not change at
/// runtime.
pub struct SkyLight {
    pub post_to_pid: bool,
    pub auth_message: bool,
    pub window_location: bool,
    pub focus_without_raise: bool,
    /// Resolved function pointers for the auth-signed post path. `None`
    /// when any leg is missing — caller should fall back to public
    /// `CGEventPostToPid`.
    pub fns: Option<SkyLightFns>,
}

#[derive(Clone, Copy)]
pub struct SkyLightFns {
    pub post_to_pid: PostToPidFn,
    pub set_auth_message: SetAuthMessageFn,
    pub set_window_location: Option<SetWindowLocationFn>,
    pub set_int_field: Option<SetIntFieldFn>,
    pub main_connection_id: Option<ConnectionIdFn>,
}

static RESOLVED: OnceLock<SkyLight> = OnceLock::new();

/// Resolve the SkyLight SPI surface and cache the result. Idempotent.
pub fn probe() -> &'static SkyLight {
    RESOLVED.get_or_init(resolve)
}

fn resolve() -> SkyLight {
    // dlopen the framework so its ObjC classes register and dlsym sees
    // its symbols. Without this, lookups fail on processes that don't
    // transitively link SkyLight.
    let handle = unsafe { libc::dlopen(SKYLIGHT_PATH.as_ptr() as *const _, libc::RTLD_LAZY) };
    if handle.is_null() {
        return SkyLight {
            post_to_pid: false,
            auth_message: false,
            window_location: false,
            focus_without_raise: false,
            fns: None,
        };
    }

    let post_to_pid = sym::<PostToPidFn>(b"SLEventPostToPid\0");
    let set_auth = sym::<SetAuthMessageFn>(b"SLEventSetAuthenticationMessage\0");
    let set_window = sym::<SetWindowLocationFn>(b"CGEventSetWindowLocation\0");
    let set_int_field = sym::<SetIntFieldFn>(b"SLEventSetIntegerValueField\0");
    let main_connection = sym::<ConnectionIdFn>(b"CGSMainConnectionID\0");

    let post_record = sym::<PostEventRecordToFn>(b"SLPSPostEventRecordTo\0");
    let get_front = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"_SLPSGetFrontProcess\0".as_ptr() as *const _,
        )
    };
    let get_psn = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"GetProcessForPID\0".as_ptr() as *const _,
        )
    };
    let focus_without_raise =
        post_record.is_some() && !get_front.is_null() && !get_psn.is_null();

    let fns = match (post_to_pid, set_auth) {
        (Some(post), Some(auth)) => Some(SkyLightFns {
            post_to_pid: post,
            set_auth_message: auth,
            set_window_location: set_window,
            set_int_field,
            main_connection_id: main_connection,
        }),
        _ => None,
    };

    SkyLight {
        post_to_pid: post_to_pid.is_some(),
        auth_message: set_auth.is_some(),
        window_location: set_window.is_some(),
        focus_without_raise,
        fns,
    }
}

fn sym<T: Copy>(name: &[u8]) -> Option<T> {
    debug_assert_eq!(*name.last().unwrap(), 0, "symbol name must be NUL-terminated");
    let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr() as *const _) };
    if p.is_null() {
        return None;
    }
    // SAFETY: we only call this with `T = unsafe extern "C" fn(...)`,
    // which is pointer-sized. dlsym returned non-null.
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

// Silence unused-warnings on c_int — kept exported because callers in
// later commits will need the type to declare PSN buffers etc.
#[allow(dead_code)]
const _: c_int = 0;
