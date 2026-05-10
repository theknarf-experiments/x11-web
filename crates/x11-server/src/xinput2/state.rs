use std::collections::HashMap;

use x11rb_protocol::protocol::xinput as xi;

use super::{Xi2ActiveGrab, Xi2PassiveGrab, MASTER_POINTER_ID};

use crate::xinput2::{PendingSynthetic, ValuatorState, XiSelection};

/// Per-client XI state stored on `ClientState`.
pub struct XiState {
    pub valuators: ValuatorState,
    pub selections: Vec<XiSelection>,
    /// Synthetic events that should be emitted at the next pending-event
    /// flush, using the *current* sequence number rather than a stale one.
    pub pending: PendingSynthetic,
    /// The client pointer device ID. Defaults to `MASTER_POINTER_ID` (2).
    /// Set by `XISetClientPointer`.
    pub client_pointer: u16,
    /// Per-device properties, keyed by `(device_id, property_atom)`.
    /// Written by `XIChangeProperty`, removed by `XIDeleteProperty`.
    pub device_properties: HashMap<(u16, u32), Vec<u8>>,
    /// Active XI2 device grabs (one per device). Records the grab
    /// parameters so dispatch can route events to `grab_window` with
    /// the grab's mask.
    pub active_grabs: HashMap<xi::DeviceId, Xi2ActiveGrab>,
    /// Passive XI2 device grabs.
    pub passive_grabs: Vec<Xi2PassiveGrab>,
    /// Whether the pointer device events are frozen (synchronous grab mode).
    pub pointer_frozen: bool,
    /// Whether the keyboard device events are frozen (synchronous grab mode).
    pub keyboard_frozen: bool,
    /// Frozen pointer events queue.
    pub frozen_pointer_events: Vec<Vec<u8>>,
    /// Frozen keyboard events queue.
    pub frozen_keyboard_events: Vec<Vec<u8>>,
    /// XI 1.x per-window "don't propagate" event class lists.
    /// Lazily initialized on first ChangeDeviceDontPropagateList call.
    pub xi1_dont_propagate: Option<HashMap<u32, Vec<u32>>>,
}

impl Default for XiState {
    fn default() -> Self {
        Self {
            valuators: ValuatorState::default(),
            selections: Vec::new(),
            pending: PendingSynthetic::default(),
            client_pointer: MASTER_POINTER_ID,
            device_properties: HashMap::new(),
            active_grabs: HashMap::new(),
            passive_grabs: Vec::new(),
            pointer_frozen: false,
            keyboard_frozen: false,
            frozen_pointer_events: Vec::new(),
            frozen_keyboard_events: Vec::new(),
            xi1_dont_propagate: None,
        }
    }
}

impl XiState {
    /// Look up a passive grab matching the supplied event details.
    ///
    /// Returns the matching grab when its `(grab_type, deviceid,
    /// detail, modifiers)` tuple is compatible with the event AND the
    /// `grab_window` is in the propagation `chain` (the event window
    /// or one of its ancestors). Wildcard values: deviceid `0`/`1`
    /// match any master pair member; detail `0` matches any button or
    /// keycode; modifiers `0x8000` (core AnyModifier) and `0x80000000`
    /// (XI AnyModifier) match any modifier state.
    pub fn check_passive_grab(
        &self,
        deviceid: xi::DeviceId,
        detail: u32,
        grab_type: u8,
        modifiers: u16,
        chain: &[u32],
    ) -> Option<&Xi2PassiveGrab> {
        const XI_ANY_MOD: u32 = 1 << 31;
        const CORE_ANY_MOD: u32 = 1 << 15;
        self.passive_grabs.iter().find(|g| {
            g.grab_type == grab_type
                && (g.deviceid == 0 || g.deviceid == 1 || g.deviceid == deviceid)
                && (g.detail == 0 || g.detail == detail)
                && (g.modifiers == XI_ANY_MOD
                    || g.modifiers == CORE_ANY_MOD
                    || (g.modifiers as u16) == modifiers)
                && chain.iter().any(|w| *w == g.grab_window)
        })
    }
}
