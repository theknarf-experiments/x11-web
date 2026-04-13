use std::collections::HashMap;

use x11rb_protocol::protocol::xinput as xi;

use super::{MASTER_KEYBOARD_ID, MASTER_POINTER_ID, Xi2ActiveGrab, Xi2PassiveGrab};

use crate::xinput2::{PendingSynthetic, ValuatorState, XiSelection};

/// Maximum number of frozen XI2 events before oldest are dropped.
#[allow(dead_code)]
const MAX_XI2_FROZEN_EVENTS: usize = 4096;

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

#[allow(dead_code)]
impl XiState {
    /// Check if a passive grab should activate for the given event.
    /// Returns the matching passive grab if found.
    pub fn check_passive_grab(
        &self,
        deviceid: xi::DeviceId,
        detail: u32,
        grab_type: u8,
        modifiers: u32,
        window_chain: &[u32],
    ) -> Option<&Xi2PassiveGrab> {
        // Walk the window hierarchy looking for passive grabs (LIFO order).
        for window in window_chain {
            // Search in reverse (LIFO) for matching passive grabs.
            for grab in self.passive_grabs.iter().rev() {
                if grab.grab_window != *window {
                    continue;
                }
                if grab.grab_type != grab_type {
                    continue;
                }
                // Device match: 0 = AllDevices, 1 = AllMaster, or exact match.
                if grab.deviceid != 0
                    && grab.deviceid != 1
                    && grab.deviceid != deviceid
                {
                    continue;
                }
                // Detail match: 0 = AnyKey/AnyButton.
                if grab.detail != 0 && grab.detail != detail {
                    continue;
                }
                // Modifier match: 0x8000 = AnyModifier.
                if grab.modifiers != 0x8000 && grab.modifiers != modifiers {
                    continue;
                }
                return Some(grab);
            }
        }
        None
    }

    /// Activate a passive grab (convert to active).
    pub fn activate_passive_grab(&mut self, grab: &Xi2PassiveGrab) {
        let active = Xi2ActiveGrab {
            deviceid: grab.deviceid,
            grab_window: grab.grab_window,
            event_mask: grab.event_mask.clone(),
            owner_events: grab.owner_events,
            paired_device_mode: grab.paired_device_mode,
            grab_mode: grab.grab_mode,
        };
        // Freeze if synchronous mode.
        if grab.grab_mode == 0 {
            if grab.deviceid == MASTER_POINTER_ID || grab.deviceid == 0 || grab.deviceid == 1 {
                self.pointer_frozen = true;
            }
            if grab.deviceid == MASTER_KEYBOARD_ID || grab.deviceid == 0 || grab.deviceid == 1 {
                self.keyboard_frozen = true;
            }
        }
        self.active_grabs.insert(active.deviceid, active);
    }

    /// Queue an event during a synchronous grab freeze.
    pub fn freeze_pointer_event(&mut self, event: Vec<u8>) {
        if self.frozen_pointer_events.len() >= MAX_XI2_FROZEN_EVENTS {
            self.frozen_pointer_events.remove(0);
        }
        self.frozen_pointer_events.push(event);
    }

    /// Queue a keyboard event during a synchronous grab freeze.
    pub fn freeze_keyboard_event(&mut self, event: Vec<u8>) {
        if self.frozen_keyboard_events.len() >= MAX_XI2_FROZEN_EVENTS {
            self.frozen_keyboard_events.remove(0);
        }
        self.frozen_keyboard_events.push(event);
    }

    /// Thaw pointer events and return frozen events for delivery.
    pub fn thaw_pointer(&mut self) -> Vec<Vec<u8>> {
        self.pointer_frozen = false;
        std::mem::take(&mut self.frozen_pointer_events)
    }

    /// Thaw keyboard events and return frozen events for delivery.
    pub fn thaw_keyboard(&mut self) -> Vec<Vec<u8>> {
        self.keyboard_frozen = false;
        std::mem::take(&mut self.frozen_keyboard_events)
    }
}
