// Derived from waylandcraft — https://github.com/EVV1E/waylandcraft
// Upstream file:   native/src/utils.rs
// Upstream commit: 233d1431e6acbad1d0c47dfba44d971ce0cebfe8
// GPLv3 — see crates/wayland-server/NOTICE
//
// Changed from upstream: the `to_fixed` / `to_fixed2` wl_fixed helpers
// were dropped — they exist upstream only to marshal doubles across
// the JNI boundary, and wayland-server's generated bindings already
// take `f64` for every fixed-point argument we send.

use std::time::{SystemTime, UNIX_EPOCH};

use smithay::utils::SERIAL_COUNTER;

/// Next protocol serial. Wayland serials are a single monotonic
/// sequence shared by every interface, which is why this is a process
/// global in smithay rather than per-object state.
#[allow(dead_code)] // STAGE: Input — first caller is the seat.
pub(crate) fn new_serial() -> u32 {
    SERIAL_COUNTER.next_serial().into()
}

/// Milliseconds-since-epoch truncated to `u32`, the timestamp every
/// Wayland input and `wl_callback.done` event carries.
///
/// The truncation is not a bug: the protocol defines these timestamps
/// as `uint` milliseconds with undefined base, and clients are
/// required to handle wraparound. Using the wall clock (rather than a
/// monotonic zero) matches upstream and keeps frame callback
/// timestamps comparable with what a real compositor sends.
pub(crate) fn get_time() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}
