//! X11 grab infrastructure: pointer grabs, keyboard grabs, passive grabs.
//!
//! The X11 protocol supports two kinds of grabs:
//! - **Active grabs**: GrabPointer/GrabKeyboard take immediate control
//! - **Passive grabs**: GrabButton/GrabKey activate on a matching event
//!
//! When a grab is active, events are redirected to the grabbing client
//! and other clients don't see them.

use tracing::{debug, info};

use super::client::ClientState;
use super::core::build_error;

/// State for all grab operations on a connection.
#[derive(Default)]
pub(crate) struct GrabState {
    /// Active pointer grab, if any.
    pub(crate) pointer_grab: Option<ActivePointerGrab>,
    /// Active keyboard grab, if any.
    pub(crate) keyboard_grab: Option<ActiveKeyboardGrab>,
    /// Passive button grabs: (window, button, modifiers) -> grab info.
    pub(crate) button_grabs: Vec<PassiveButtonGrab>,
    /// Passive key grabs: (window, keycode, modifiers) -> grab info.
    pub(crate) key_grabs: Vec<PassiveKeyGrab>,
    /// Server grab count (GrabServer/UngrabServer).
    pub(crate) server_grab_count: u32,
}

#[derive(Clone)]
pub(crate) struct ActivePointerGrab {
    pub(crate) grab_window: u32,
    pub(crate) event_mask: u32,
    pub(crate) pointer_mode: u8,
    pub(crate) keyboard_mode: u8,
    pub(crate) confine_to: u32,
    pub(crate) cursor: u32,
    pub(crate) owner_events: bool,
}

#[derive(Clone)]
pub(crate) struct ActiveKeyboardGrab {
    pub(crate) grab_window: u32,
    pub(crate) pointer_mode: u8,
    pub(crate) keyboard_mode: u8,
    pub(crate) owner_events: bool,
}

#[derive(Clone)]
pub(crate) struct PassiveButtonGrab {
    pub(crate) grab_window: u32,
    pub(crate) button: u8,        // 0 = AnyButton
    pub(crate) modifiers: u16,    // 0x8000 = AnyModifier
    pub(crate) event_mask: u32,
    pub(crate) pointer_mode: u8,
    pub(crate) keyboard_mode: u8,
    pub(crate) confine_to: u32,
    pub(crate) cursor: u32,
    pub(crate) owner_events: bool,
}

#[derive(Clone)]
pub(crate) struct PassiveKeyGrab {
    pub(crate) grab_window: u32,
    pub(crate) key: u8,           // 0 = AnyKey
    pub(crate) modifiers: u16,    // 0x8000 = AnyModifier
    pub(crate) pointer_mode: u8,
    pub(crate) keyboard_mode: u8,
    pub(crate) owner_events: bool,
}

/// GrabPointer (opcode 26)
pub(crate) fn handle_grab_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        return build_error(16, seq, 0, 26, 0); // BadLength
    }

    let owner_events = data[1] != 0;
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let event_mask = u16::from_le_bytes([data[8], data[9]]) as u32;
    let pointer_mode = data[10];
    let keyboard_mode = data[11];
    let confine_to = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let cursor = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

    // Validate grab_window exists
    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(3, seq, grab_window, 26, 0); // BadWindow
    }

    info!("GrabPointer: window={grab_window:#x} owner_events={owner_events} event_mask={event_mask:#x}");

    state.grabs.pointer_grab = Some(ActivePointerGrab {
        grab_window,
        event_mask,
        pointer_mode,
        keyboard_mode,
        confine_to,
        cursor,
        owner_events,
    });

    // Reply: GrabSuccess
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[1] = 0; // GrabSuccess
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

/// UngrabPointer (opcode 27)
pub(crate) fn handle_ungrab_pointer(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if state.grabs.pointer_grab.is_some() {
        debug!("UngrabPointer: releasing active pointer grab");
        state.grabs.pointer_grab = None;
    }
    Vec::new()
}

/// GrabButton (opcode 28)
pub(crate) fn handle_grab_button(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let owner_events = data[1] != 0;
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let event_mask = u16::from_le_bytes([data[8], data[9]]) as u32;
    let pointer_mode = data[10];
    let keyboard_mode = data[11];
    let confine_to = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let cursor = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let button = data[20];
    let modifiers = u16::from_le_bytes([data[22], data[23]]);

    debug!("GrabButton: window={grab_window:#x} button={button} modifiers={modifiers:#x}");

    // Remove any existing grab with the same (window, button, modifiers)
    state.grabs.button_grabs.retain(|g| {
        !(g.grab_window == grab_window && g.button == button && g.modifiers == modifiers)
    });

    state.grabs.button_grabs.push(PassiveButtonGrab {
        grab_window,
        button,
        modifiers,
        event_mask,
        pointer_mode,
        keyboard_mode,
        confine_to,
        cursor,
        owner_events,
    });

    Vec::new()
}

/// UngrabButton (opcode 29)
pub(crate) fn handle_ungrab_button(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let button = data[1];
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let modifiers = u16::from_le_bytes([data[8], data[9]]);

    debug!("UngrabButton: window={grab_window:#x} button={button} modifiers={modifiers:#x}");

    if button == 0 && modifiers == 0x8000 {
        // AnyButton + AnyModifier: remove all button grabs on this window
        state.grabs.button_grabs.retain(|g| g.grab_window != grab_window);
    } else {
        state.grabs.button_grabs.retain(|g| {
            !(g.grab_window == grab_window && g.button == button && g.modifiers == modifiers)
        });
    }

    Vec::new()
}

/// ChangeActivePointerGrab (opcode 30)
pub(crate) fn handle_change_active_pointer_grab(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    if let Some(ref mut grab) = state.grabs.pointer_grab {
        let cursor = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let event_mask = u16::from_le_bytes([data[12], data[13]]) as u32;
        grab.cursor = cursor;
        grab.event_mask = event_mask;
        debug!("ChangeActivePointerGrab: cursor={cursor:#x} event_mask={event_mask:#x}");
    }

    Vec::new()
}

/// GrabKeyboard (opcode 31)
pub(crate) fn handle_grab_keyboard(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return build_error(16, seq, 0, 31, 0);
    }

    let owner_events = data[1] != 0;
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let pointer_mode = data[12];
    let keyboard_mode = data[13];

    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(3, seq, grab_window, 31, 0);
    }

    info!("GrabKeyboard: window={grab_window:#x} owner_events={owner_events}");

    state.grabs.keyboard_grab = Some(ActiveKeyboardGrab {
        grab_window,
        pointer_mode,
        keyboard_mode,
        owner_events,
    });

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // GrabSuccess
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

/// UngrabKeyboard (opcode 32)
pub(crate) fn handle_ungrab_keyboard(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if state.grabs.keyboard_grab.is_some() {
        debug!("UngrabKeyboard: releasing active keyboard grab");
        state.grabs.keyboard_grab = None;
    }
    Vec::new()
}

/// GrabKey (opcode 33)
pub(crate) fn handle_grab_key(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let owner_events = data[1] != 0;
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let modifiers = u16::from_le_bytes([data[8], data[9]]);
    let key = data[10];
    let pointer_mode = data[11];
    let keyboard_mode = data[12];

    debug!("GrabKey: window={grab_window:#x} key={key} modifiers={modifiers:#x}");

    // Remove any existing grab with the same (window, key, modifiers)
    state.grabs.key_grabs.retain(|g| {
        !(g.grab_window == grab_window && g.key == key && g.modifiers == modifiers)
    });

    state.grabs.key_grabs.push(PassiveKeyGrab {
        grab_window,
        key,
        modifiers,
        pointer_mode,
        keyboard_mode,
        owner_events,
    });

    Vec::new()
}

/// UngrabKey (opcode 34)
pub(crate) fn handle_ungrab_key(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let key = data[1];
    let grab_window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let modifiers = u16::from_le_bytes([data[8], data[9]]);

    debug!("UngrabKey: window={grab_window:#x} key={key} modifiers={modifiers:#x}");

    if key == 0 && modifiers == 0x8000 {
        state.grabs.key_grabs.retain(|g| g.grab_window != grab_window);
    } else {
        state.grabs.key_grabs.retain(|g| {
            !(g.grab_window == grab_window && g.key == key && g.modifiers == modifiers)
        });
    }

    Vec::new()
}

/// AllowEvents (opcode 35)
pub(crate) fn handle_allow_events(_state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let mode = data[1];
        debug!("AllowEvents: mode={mode}");
    }
    // In our implementation, events are never frozen, so this is a no-op.
    Vec::new()
}

/// GrabServer (opcode 36)
pub(crate) fn handle_grab_server(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    state.grabs.server_grab_count += 1;
    debug!("GrabServer: count={}", state.grabs.server_grab_count);
    Vec::new()
}

/// UngrabServer (opcode 37)
pub(crate) fn handle_ungrab_server(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if state.grabs.server_grab_count > 0 {
        state.grabs.server_grab_count -= 1;
    }
    debug!("UngrabServer: count={}", state.grabs.server_grab_count);
    Vec::new()
}
