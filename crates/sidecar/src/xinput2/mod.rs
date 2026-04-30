//! XInput / XInput2 protocol implementation.
//!
//! We dispatch and reply to enough of the XI 1.x and XI 2.x request set
//! to keep modern toolkits (Xt, GDK 3, Qt, Mozilla widgets) happy.
//!
//! For wire-format ground truth we re-use `x11rb_protocol::protocol::xinput`
//! types (parsed from the upstream X11 XML protocol description) and let
//! their `Serialize` impls produce the bytes. This guarantees we never
//! drift from the canonical layout.

use x11rb_protocol::protocol::xinput as xi;

use crate::xserver::core::write_u32_bo;

mod device;
mod events;
mod handler;
mod state;
#[cfg(test)]
mod tests;

// Re-export pub(crate) items for internal use across the crate.
// These are used by submodules (handler, tests) via `use super::*`.

// Re-export public items used by other modules in the crate.
pub use events::{build_raw_motion_event, build_xi_events_for, patch_query_pointer_root};

pub use handler::handle_request;

pub use state::XiState;

/// Major opcode we register for XInputExtension in QueryExtension. 131 is
/// the conventional value used by the upstream X server, but the actual
/// number doesn't matter — clients pick it up from QueryExtension.
pub const XI_MAJOR_OPCODE: u8 = 131;

/// Device IDs we expose. Two master devices is the minimum modern XI
/// clients (e.g. GTK 3) expect.
pub const MASTER_POINTER_ID: xi::DeviceId = 2;
pub const MASTER_KEYBOARD_ID: xi::DeviceId = 3;

/// Per-window XI2 event subscription. One per `(window, deviceid)` tuple.
#[derive(Clone, Debug)]
pub struct XiSelection {
    pub window: u32,
    pub deviceid: xi::DeviceId,
    pub mask: Vec<xi::XIEventMask>,
}

impl XiSelection {
    pub fn wants(&self, evtype: u16) -> bool {
        // The XI2 mask is a bitfield indexed by event type number.
        // x11rb represents it as a Vec<u32> (one u32 per 32 event types).
        let bit = evtype as u32;
        let word = (bit / 32) as usize;
        let in_word = bit % 32;
        self.mask
            .get(word)
            .map(|w| (u32::from(*w) >> in_word) & 1 != 0)
            .unwrap_or(false)
    }
}

fn fp1616(v: i16) -> xi::Fp1616 {
    (v as i32) << 16
}

fn fp3232(int: i32) -> xi::Fp3232 {
    xi::Fp3232 {
        integral: int,
        frac: 0,
    }
}

/// Per-axis valuator state we track for the master pointer. The X server
/// uses these to populate `XIValuatorClassInfo.value` in `XIQueryDevice`
/// replies and the per-event valuator data in motion / button events.
///
/// `scroll_v` / `scroll_h` accumulate over the lifetime of the connection
/// — XI2 clients compute scroll deltas from successive valuator values.
/// When a wheel event arrives we bump these by `1.0` (matching the
/// `increment` we report in our scroll classes) per discrete wheel notch.
#[derive(Clone, Debug, Default)]
pub struct ValuatorState {
    pub x: i32,
    pub y: i32,
    pub scroll_v: i32,
    pub scroll_h: i32,
}

/// Axis numbers we use for the master pointer's valuator/scroll
/// classes. Valuator 0 / 1 are the absolute X / Y axes (emitted as
/// `XIValuatorClass` entries in our `XIQueryDevice` reply); 2 / 3
/// are the vertical / horizontal scroll axes (`XIScrollClass`).
pub const AXIS_SCROLL_V: u16 = 2;
pub const AXIS_SCROLL_H: u16 = 3;

/// Serialize any x11rb XInput value with a 32-byte header, then patch
/// the `length` field (4-byte units after the header). x11rb's
/// `Serialize` impls don't compute `length` automatically — it has to
/// match the actual trailing-bytes count or XCB rejects the message
/// with "Too much data requested".
///
/// Used for both XI replies and XI GenericEvent (XGE) events.
pub(crate) fn serialize_xi_reply<R: x11rb_protocol::x11_utils::Serialize>(
    reply: &R,
    msb_first: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();
    reply.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    debug_assert!(buf.len() >= 32, "XI reply/event must be at least 32 bytes");
    let length_units = ((buf.len() - 32) / 4) as u32;
    write_u32_bo(&mut buf, 4, length_units, msb_first);
    buf
}

/// Marker placed in the per-client `XiState` to request that a synthetic
/// RawMotion event be emitted at the next flush, using whatever the
/// current sequence number is at that time.
#[derive(Default)]
pub struct PendingSynthetic {
    pub raw_motion: bool,
}

/// One axis value to report inside an XI2 device event's valuator data.
#[derive(Clone, Copy, Debug)]
pub struct AxisValue {
    pub axis: u16,
    pub value: i32,
}

/// Active XI2 device grab (from XIGrabDevice).
#[derive(Clone, Debug)]
pub struct Xi2ActiveGrab {
    /// The device that was grabbed.
    pub deviceid: xi::DeviceId,
    /// The window the grab is associated with.
    pub grab_window: u32,
    /// Event mask for events delivered during the grab.
    pub event_mask: Vec<xi::XIEventMask>,
    /// Whether owner_events is set.
    pub owner_events: bool,
    /// Grab mode for the paired device (0=Sync, 1=Async).
    pub paired_device_mode: u8,
    /// Grab mode for this device (0=Sync, 1=Async).
    pub grab_mode: u8,
}

/// Passive XI2 device grab (from XIPassiveGrabDevice).
#[derive(Clone, Debug)]
pub struct Xi2PassiveGrab {
    /// The device the passive grab is for.
    pub deviceid: xi::DeviceId,
    /// The window the grab is associated with.
    pub grab_window: u32,
    /// The detail (button, keycode, or touch) that triggers the grab.
    pub detail: u32,
    /// Grab type: 1=Button, 2=Keycode, 3=Enter, 4=FocusIn, 5=TouchBegin.
    pub grab_type: u8,
    /// Modifier combination that triggers the grab.
    pub modifiers: u32,
    /// Event mask to deliver during the grab.
    pub event_mask: Vec<xi::XIEventMask>,
    /// Whether owner_events is set.
    pub owner_events: bool,
    /// Grab mode for the paired device.
    pub paired_device_mode: u8,
    /// Grab mode for this device.
    pub grab_mode: u8,
}
