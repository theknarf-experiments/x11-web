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
    /// Active XI2 device grabs (one per device).
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

impl XiState {}
