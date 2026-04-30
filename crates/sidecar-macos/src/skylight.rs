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

const SKYLIGHT_PATH: &[u8] = b"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight\0";

/// `void SLEventPostToPid(pid_t, CGEventRef)`
pub type PostToPidFn = unsafe extern "C" fn(pid: i32, event: *mut c_void);

/// `void SLEventSetAuthenticationMessage(CGEventRef, id)`
pub type SetAuthMessageFn = unsafe extern "C" fn(event: *mut c_void, msg: *mut c_void);

/// `void SLEventSetIntegerValueField(CGEventRef, uint32_t, int64_t)` —
/// SkyLight's raw-field SPI; reaches private fields like f51/f91/f92
/// that the public `CGEventSetIntegerValueField` rejects.
pub type SetIntFieldFn = unsafe extern "C" fn(event: *mut c_void, field: u32, value: i64);

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
pub type PostEventRecordToFn = unsafe extern "C" fn(psn: *const c_void, bytes: *const u8) -> i32;

/// `objc_msgSend` typed for
/// `+[SLSEventAuthenticationMessage messageWithEventRecord:pid:version:]`:
/// `(Class, Selector, SLSEventRecord*, int32_t, uint32_t) -> id`.
/// Same shape cua uses (`SkyLightEventPost.swift` line 68-70).
pub type AuthFactoryMsgSendFn = unsafe extern "C" fn(
    cls: *mut c_void,
    sel: *mut c_void,
    record: *mut c_void,
    pid: i32,
    version: u32,
) -> *mut c_void;

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
    /// `objc_msgSend` cast for the auth-message factory + the ObjC
    /// class object + selector for `+messageWithEventRecord:pid:
    /// version:`. Available when the SkyLight private framework
    /// loaded and the class is registered in the ObjC runtime —
    /// which is the normal case on macOS 14+. Used by the keyboard
    /// path to give Chromium / kitty / other strict targets an
    /// auth-signed event they'll honour.
    pub auth_factory_msg_send: Option<AuthFactoryMsgSendFn>,
    pub auth_message_class: Option<*mut c_void>,
    pub auth_factory_selector: Option<*mut c_void>,
}

// SAFETY: the cached function pointers + class object + selector are
// all immutable for the lifetime of the process. Worst-case sharing
// across threads is reading the same address.
unsafe impl Send for SkyLightFns {}
unsafe impl Sync for SkyLightFns {}

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
    let focus_without_raise = post_record.is_some() && !get_front.is_null() && !get_psn.is_null();

    // Auth-message factory: `objc_msgSend` cast to call
    // `+[SLSEventAuthenticationMessage messageWithEventRecord:pid:
    // version:]`, plus the class object and selector. Present on
    // macOS 14+ when SkyLight has registered its ObjC classes.
    let auth_factory_msg_send = sym::<AuthFactoryMsgSendFn>(b"objc_msgSend\0");
    let (auth_message_class, auth_factory_selector) = unsafe {
        let cls = ns_class_from_string("SLSEventAuthenticationMessage");
        let sel = sel_register("messageWithEventRecord:pid:version:");
        (cls, sel)
    };

    let fns = match (post_to_pid, set_auth) {
        (Some(post), Some(auth)) => Some(SkyLightFns {
            post_to_pid: post,
            set_auth_message: auth,
            set_window_location: set_window,
            set_int_field,
            main_connection_id: main_connection,
            auth_factory_msg_send,
            auth_message_class,
            auth_factory_selector,
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

/// Look up an ObjC class by name via `NSClassFromString`. Returns
/// `None` when the class isn't registered (e.g. the SkyLight
/// private framework didn't load this binding's classes).
unsafe fn ns_class_from_string(name: &str) -> Option<*mut c_void> {
    extern "C" {
        // Provided by Foundation / objc4. We dlopen Foundation
        // implicitly via `objc2-foundation`; the symbol is part of
        // the running process either way.
        fn NSClassFromString(name: *mut c_void) -> *mut c_void;
    }
    use objc2_foundation::NSString;
    let ns = NSString::from_str(name);
    // NSClassFromString takes an NSString*. Re-cast.
    let ptr = NSClassFromString(objc2::rc::Retained::as_ptr(&ns) as *mut c_void);
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

/// Register a selector by name. Same as `NSSelectorFromString` /
/// `sel_registerName`.
unsafe fn sel_register(name: &str) -> Option<*mut c_void> {
    extern "C" {
        fn sel_registerName(name: *const u8) -> *mut c_void;
    }
    let cstr = std::ffi::CString::new(name).ok()?;
    let sel = sel_registerName(cstr.as_ptr() as *const u8);
    if sel.is_null() {
        None
    } else {
        Some(sel)
    }
}

/// Attach an `SLSEventAuthenticationMessage` to `event` (matching
/// cua's `SkyLightEventPost.postToPid(... attachAuthMessage: true)`).
/// Required for keyboard events on macOS 14+ to land on Chromium-
/// family targets (and apparently kitty as well — strict event
/// filters latch onto the auth envelope before honouring synthetic
/// keystrokes).
///
/// Returns `true` when every leg resolved and we successfully built
/// + attached the message. Returns `false` on any failure; the
/// caller should still post the event without the envelope, since
/// AppKit-only targets don't need it and we'd rather degrade than
/// drop the event entirely.
pub fn attach_auth_message(event_ptr: *mut c_void, pid: i32) -> bool {
    let Some(fns) = probe().fns.as_ref() else {
        return false;
    };
    let (Some(msg_send), Some(cls), Some(sel)) = (
        fns.auth_factory_msg_send,
        fns.auth_message_class,
        fns.auth_factory_selector,
    ) else {
        return false;
    };
    // Extract the embedded `SLSEventRecord *` from the CGEvent.
    // cua's `extractEventRecord(from:)` probes offsets 24, 32, 16
    // for resilience across OS revisions; we mirror exactly.
    let Some(record) = (unsafe { extract_event_record(event_ptr) }) else {
        return false;
    };
    let msg = unsafe { msg_send(cls, sel, record, pid, 0) };
    if msg.is_null() {
        return false;
    }
    unsafe { (fns.set_auth_message)(event_ptr, msg) };
    true
}

/// Read the `SLSEventRecord *` slot embedded in a `__CGEvent`
/// struct. cua's note (`SkyLightEventPost.swift` line 366-374):
/// "The layout of `__CGEvent` exposed by SkyLight's ObjC type
/// encodings is `{CFRuntimeBase, uint32_t, SLSEventRecord *}` —
/// on 64-bit that puts the record pointer at offset 24
/// (CFRuntimeBase=16 + uint32=4 + 4 bytes padding). We probe a
/// few adjacent offsets for resilience across OS revisions."
unsafe fn extract_event_record(event: *mut c_void) -> Option<*mut c_void> {
    if event.is_null() {
        return None;
    }
    for &offset in &[24usize, 32, 16] {
        let slot = event.add(offset) as *const *mut c_void;
        let p = *slot;
        if !p.is_null() {
            return Some(p);
        }
    }
    None
}

fn sym<T: Copy>(name: &[u8]) -> Option<T> {
    debug_assert_eq!(
        *name.last().unwrap(),
        0,
        "symbol name must be NUL-terminated"
    );
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
const _: c_int = 0;
