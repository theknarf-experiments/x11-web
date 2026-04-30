//! Activate a target app without raising its windows or warping
//! WindowServer's z-order.
//!
//! Direct port of cua-driver's `FocusWithoutRaise.swift`, itself
//! ported from yabai's `window_manager_focus_window_without_raise`.
//! Recipe:
//!
//!   1. `_SLPSGetFrontProcess(&prevPSN)` — grab whichever app is
//!      currently frontmost.
//!   2. `GetProcessForPID(targetPid, &targetPSN)` — resolve target.
//!   3. Build a 248-byte synthetic Carbon event record.
//!   4. `SLPSPostEventRecordTo(prevPSN, defocus-record)` — tell
//!      WindowServer the previous front lost focus.
//!   5. `SLPSPostEventRecordTo(targetPSN, focus-record)` with the
//!      target window id stamped at offset 0x3C-0x3F.
//!
//! After this dance, `NSRunningApplication.isActive` flips true on
//! the target, AX events fire, and `CGEvent.postToPid` /
//! `SLEventPostToPid` start being honoured by AppKit's responder
//! chain. The window does NOT get raised and the user's Space
//! doesn't follow — exactly what we need for remote control without
//! disturbing the local user's foreground app.
//!
//! Buffer layout (verified against macOS 15/26):
//!
//!   bytes[0x04]      = 0xF8           — opcode high
//!   bytes[0x08]      = 0x0D           — opcode low
//!   bytes[0x3C..0x3F] = CGWindowID    — little-endian
//!   bytes[0x8A]      = 0x01 (focus) | 0x02 (defocus)
//!   all other bytes  = 0
//!
//! `SLPSSetFrontProcessWithOptions` is deliberately omitted — yabai
//! calls it next, but cua's empirical testing showed the focus event
//! alone is enough for AppKit and Chromium to accept subsequent
//! synthetic clicks; the SetFront call adds a visible raise + Space
//! follow.

use core_graphics::window::CGWindowID;

use crate::skylight::probe;

/// Activate `target_pid`'s window `target_wid` without raising any
/// windows. Returns `false` when the SkyLight SPIs aren't resolvable
/// or one of the event posts failed.
pub fn activate_without_raise(target_pid: i32, target_wid: CGWindowID) -> bool {
    let sky = probe();
    if !sky.focus_without_raise {
        return false;
    }

    // PSN buffers: 8 bytes each (high u32, low u32). cua fills these
    // via Apple's APIs, not by hand.
    let mut prev_psn = [0u8; 8];
    let mut target_psn = [0u8; 8];

    // SAFETY: dlsym'd SPIs that take 8 raw bytes. We allocate exactly
    // 8 bytes above; the SPIs write the PSN into them.
    let prev_ok = unsafe { resolve_front_psn(&mut prev_psn) };
    if !prev_ok {
        tracing::warn!("FocusWithoutRaise: _SLPSGetFrontProcess failed");
        return false;
    }
    let target_ok = unsafe { resolve_pid_psn(target_pid, &mut target_psn) };
    if !target_ok {
        tracing::warn!("FocusWithoutRaise: GetProcessForPID({target_pid}) failed");
        return false;
    }
    tracing::info!(
        "FocusWithoutRaise: prev_psn={:02x?} target_psn={:02x?} target_wid={}",
        prev_psn, target_psn, target_wid
    );

    let mut buf = [0u8; 0xF8];
    buf[0x04] = 0xF8;
    buf[0x08] = 0x0D;
    let wid = target_wid;
    buf[0x3C] = (wid & 0xFF) as u8;
    buf[0x3D] = ((wid >> 8) & 0xFF) as u8;
    buf[0x3E] = ((wid >> 16) & 0xFF) as u8;
    buf[0x3F] = ((wid >> 24) & 0xFF) as u8;

    // Defocus previous front.
    buf[0x8A] = 0x02;
    // SAFETY: post_event_record_to dlsym'd, takes raw pointers we
    // own; psn is 8 bytes; bytes is 0xF8 bytes.
    let defocus_ok = unsafe {
        post_record(&prev_psn, &buf)
    };
    tracing::info!("FocusWithoutRaise: defocus prev -> {defocus_ok}");

    // Focus target.
    buf[0x8A] = 0x01;
    let focus_ok = unsafe {
        post_record(&target_psn, &buf)
    };
    tracing::info!("FocusWithoutRaise: focus target -> {focus_ok}");

    defocus_ok && focus_ok
}

/// `_SLPSGetFrontProcess(&psn)` thin wrapper.
unsafe fn resolve_front_psn(psn: &mut [u8; 8]) -> bool {
    let sym = libc::dlsym(libc::RTLD_DEFAULT, b"_SLPSGetFrontProcess\0".as_ptr() as *const _);
    if sym.is_null() {
        return false;
    }
    let f: extern "C" fn(*mut std::ffi::c_void) -> i32 = std::mem::transmute(sym);
    f(psn.as_mut_ptr() as *mut _) == 0
}

/// `GetProcessForPID(pid, &psn)` thin wrapper.
unsafe fn resolve_pid_psn(pid: i32, psn: &mut [u8; 8]) -> bool {
    let sym = libc::dlsym(libc::RTLD_DEFAULT, b"GetProcessForPID\0".as_ptr() as *const _);
    if sym.is_null() {
        return false;
    }
    let f: extern "C" fn(i32, *mut std::ffi::c_void) -> i32 = std::mem::transmute(sym);
    f(pid, psn.as_mut_ptr() as *mut _) == 0
}

/// `SLPSPostEventRecordTo(psn, bytes)` thin wrapper.
unsafe fn post_record(psn: &[u8; 8], bytes: &[u8; 0xF8]) -> bool {
    let sym = libc::dlsym(libc::RTLD_DEFAULT, b"SLPSPostEventRecordTo\0".as_ptr() as *const _);
    if sym.is_null() {
        return false;
    }
    let f: extern "C" fn(*const std::ffi::c_void, *const u8) -> i32 = std::mem::transmute(sym);
    f(psn.as_ptr() as *const _, bytes.as_ptr()) == 0
}
