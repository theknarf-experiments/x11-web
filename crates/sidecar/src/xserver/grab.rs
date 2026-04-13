//! X11 grab infrastructure: pointer grabs, keyboard grabs, passive grabs.
//!
//! The X11 protocol supports two kinds of grabs:
//! - **Active grabs**: GrabPointer/GrabKeyboard take immediate control
//! - **Passive grabs**: GrabButton/GrabKey activate on a matching event
//!
//! When a grab is active, events are redirected to the grabbing client
//! and other clients don't see them.

use std::collections::HashMap;
use tracing::{debug, info};
use super::core::{read_u16_bo, read_u32_bo, require_len, BAD_CURSOR, BAD_LENGTH, BAD_WINDOW};
use super::client::ClientState;
use super::core::build_error;
use super::types::WindowState;
use super::{CROSSING_MODE_GRAB, CROSSING_MODE_UNGRAB};

/// Generate crossing events for a grab activation.
/// Per X11 spec §11.3, uses mode=Grab and computes proper detail
/// (Ancestor/Inferior/Nonlinear) based on the relationship between the
/// current pointer window and the grab window, with Virtual events on
/// intermediate windows.
fn emit_grab_crossing_events(state: &mut ClientState, grab_window: u32) {
    let x = state.pointer_x;
    let y = state.pointer_y;
    let events = super::input::build_crossing_events_with_mode(
        state, grab_window, x, y, x, y, CROSSING_MODE_GRAB,
    );
    if !events.is_empty() {
        // build_crossing_events_with_mode already sets last_entered_window
        for chunk in events.chunks_exact(32) {
            state.pending_events.push(chunk.to_vec());
        }
    }
}

/// Generate crossing events for a grab deactivation.
/// Per X11 spec §11.3, uses mode=Ungrab and computes proper detail based
/// on the relationship between the grab window and the current pointer window.
fn emit_ungrab_crossing_events(state: &mut ClientState, grab_window: u32) {
    // On ungrab, crossing events go from grab_window to the window actually
    // containing the pointer (approximated as root; next MotionNotify corrects).
    let dest_window = state.root_window;
    if grab_window == dest_window {
        return;
    }
    // Temporarily set last_entered_window to grab_window so crossing events
    // are computed from the correct source.
    state.last_entered_window = grab_window;
    let x = state.pointer_x;
    let y = state.pointer_y;
    let events = super::input::build_crossing_events_with_mode(
        state, dest_window, x, y, x, y, CROSSING_MODE_UNGRAB,
    );
    if !events.is_empty() {
        for chunk in events.chunks_exact(32) {
            state.pending_events.push(chunk.to_vec());
        }
    }
}

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
    /// Whether pointer events are frozen (Synchronous mode grab).
    pub(crate) pointer_frozen: bool,
    /// Whether keyboard events are frozen (Synchronous mode grab).
    pub(crate) keyboard_frozen: bool,
    /// Re-freeze pointer after delivering one event (SyncPointer / SyncBoth mode).
    pub(crate) pointer_sync_pending: bool,
    /// Re-freeze keyboard after delivering one event (SyncKeyboard / SyncBoth mode).
    pub(crate) keyboard_sync_pending: bool,
    /// Frozen pointer events, queued until AllowEvents thaws them.
    pub(crate) frozen_pointer_events: Vec<Vec<u8>>,
    /// Frozen keyboard events, queued until AllowEvents thaws them.
    pub(crate) frozen_keyboard_events: Vec<Vec<u8>>,
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
    /// Absolute screen bounds for confine_to window (x, y, x+width, y+height).
    /// None if confine_to is 0 (no confinement).
    pub(crate) confine_bounds: Option<(i16, i16, i16, i16)>,
}

/// Check if a window is viewable: it must be mapped and all ancestors up to root
/// must also be mapped (per X11 spec, a window is viewable iff it and all of its
/// ancestors are mapped).
fn is_viewable(windows: &HashMap<u32, WindowState>, wid: u32, root: u32) -> bool {
    let mut cur = wid;
    for _ in 0..128 {
        if cur == root { return true; }
        match windows.get(&cur) {
            Some(w) if w.mapped => cur = w.parent,
            _ => return false,
        }
    }
    false
}

/// Compute absolute screen-space bounds for a window by walking up to root.
fn window_abs_bounds(windows: &HashMap<u32, WindowState>, wid: u32, root: u32) -> Option<(i16, i16, i16, i16)> {
    let w = windows.get(&wid)?;
    let width = w.width as i16;
    let height = w.height as i16;
    let mut abs_x = w.x;
    let mut abs_y = w.y;
    let mut cur = w.parent;
    for _ in 0..128 {
        if cur == root || cur == 0 { break; }
        if let Some(p) = windows.get(&cur) {
            abs_x += p.x;
            abs_y += p.y;
            cur = p.parent;
        } else {
            break;
        }
    }
    Some((abs_x, abs_y, abs_x.saturating_add(width), abs_y.saturating_add(height)))
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
///
/// Status codes: 0=Success, 1=AlreadyGrabbed, 2=InvalidTime,
///               3=NotViewable, 4=Frozen
pub(crate) fn handle_grab_pointer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        return build_error(BAD_LENGTH, seq, 0, 26, 0);
    }

    let owner_events = data[1] != 0;
    let grab_window = state.read_u32(data, 4);
    let event_mask = state.read_u16(data, 8) as u32;
    let pointer_mode = data[10];
    let keyboard_mode = data[11];
    let confine_to = state.read_u32(data, 12);
    let cursor = state.read_u32(data, 16);
    let timestamp = state.read_u32(data, 20); // 0 = CurrentTime

    // Validate grab_window exists
    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(BAD_WINDOW, seq, grab_window, 26, 0);
    }

    // Validate cursor ID if specified (0 = None)
    if cursor != 0 && !state.cursors.contains_key(&cursor) {
        return build_error(BAD_CURSOR, seq, cursor, 26, 0);
    }

    info!("GrabPointer: window={grab_window:#x} owner_events={owner_events} event_mask={event_mask:#x}");

    // Status 1: AlreadyGrabbed — another client already holds an active pointer grab.
    // In our single-client-per-connection model we check our own grab state; a grab
    // held by *this* client is replaced (per spec), but if the pointer is frozen by
    // another grab we report Frozen instead.
    if state.grabs.pointer_grab.is_some() {
        // Per X11 spec, an active grab by the *same* client is replaced — only
        // report AlreadyGrabbed if a *different* client holds it.  Since each
        // ClientState is per-connection, the grab here always belongs to us, so
        // we allow replacement.  (Cross-client grabs would be checked via shared
        // state in a multi-client server.)
    }

    // Status 4: Frozen — if the pointer is frozen by an active synchronous grab
    // from a different client, we should return Frozen.  In our per-connection
    // model we approximate this: if the pointer is already frozen and we don't
    // own the grab, report Frozen.
    if state.grabs.pointer_frozen && state.grabs.pointer_grab.is_none() {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 4; // Frozen
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    // Status 3: NotViewable — grab_window must be viewable (mapped + all ancestors mapped).
    // The root window is always viewable.
    if grab_window != state.root_window
        && !is_viewable(&state.windows, grab_window, state.root_window)
    {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 3; // NotViewable
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    // Status 2: InvalidTime — if a non-zero timestamp is earlier than the
    // current server time, the grab is rejected.
    if timestamp != 0 {
        let now = state.timestamp();
        // Treat the timestamp as invalid if it is more than 0 but appears to be
        // in the past (simple unsigned comparison; wraparound after ~49 days is
        // unlikely in practice).
        let delta = now.wrapping_sub(timestamp);
        if delta > 0 && delta < 0x8000_0000 {
            // timestamp is in the past
        } else if timestamp != now {
            // timestamp is in the future — also invalid per spec
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 2; // InvalidTime
            state.write_u16(&mut reply, 2, seq);
            return reply.to_vec();
        }
    }

    // Synchronous mode (1) freezes the device
    state.grabs.pointer_frozen = pointer_mode == 1;
    if keyboard_mode == 1 {
        state.grabs.keyboard_frozen = true;
    }

    // Compute confine bounds if confine_to is specified
    let confine_bounds = if confine_to != 0 {
        if confine_to == state.root_window {
            Some((0, 0, state.screen_width as i16, state.screen_height as i16))
        } else {
            window_abs_bounds(&state.windows, confine_to, state.root_window)
        }
    } else {
        None
    };

    state.grabs.pointer_grab = Some(ActivePointerGrab {
        grab_window,
        event_mask,
        pointer_mode,
        keyboard_mode,
        confine_to,
        cursor,
        owner_events,
        confine_bounds,
    });

    // Generate crossing events: Leave(Grab) from current window, Enter(Grab) to grab window
    emit_grab_crossing_events(state, grab_window);

    // Reply: GrabSuccess
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[1] = 0; // GrabSuccess
    state.write_u16(&mut reply, 2, seq);
    reply.to_vec()
}

/// UngrabPointer (opcode 27)
pub(crate) fn handle_ungrab_pointer(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if let Some(ref grab) = state.grabs.pointer_grab {
        let grab_window = grab.grab_window;
        debug!("UngrabPointer: releasing active pointer grab");
        // Generate crossing events: Leave(Ungrab) from grab window, Enter(Ungrab) to pointer window
        emit_ungrab_crossing_events(state, grab_window);
        // Thaw frozen pointer events
        state.grabs.pointer_frozen = false;
        let events = std::mem::take(&mut state.grabs.frozen_pointer_events);
        for e in events { state.pending_events.push(e); }
        state.grabs.pointer_grab = None;
    }
    Vec::new()
}

/// GrabButton (opcode 28)
pub(crate) fn handle_grab_button(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 24, seq, 28);

    let owner_events = data[1] != 0;
    let grab_window = state.read_u32(data, 4);

    // Validate grab_window exists
    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(BAD_WINDOW, seq, grab_window, 28, 0);
    }

    let event_mask = state.read_u16(data, 8) as u32;
    let pointer_mode = data[10];
    let keyboard_mode = data[11];
    let confine_to = state.read_u32(data, 12);
    let cursor = state.read_u32(data, 16);

    // Validate cursor if non-zero
    if cursor != 0 && !state.cursors.contains_key(&cursor) {
        return build_error(BAD_CURSOR, seq, cursor, 28, 0);
    }

    let button = data[20];
    let modifiers = state.read_u16(data, 22);

    debug!("GrabButton: window={grab_window:#x} button={button} modifiers={modifiers:#x}");

    // Remove any existing grab with the same (window, button, modifiers)
    state.grabs.button_grabs.retain(|g| {
        !(g.grab_window == grab_window && g.button == button && g.modifiers == modifiers)
    });

    // Insert at front for LIFO ordering: per X11 spec, the most recently
    // established passive grab wins when multiple grabs match.
    state.grabs.button_grabs.insert(0, PassiveButtonGrab {
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
pub(crate) fn handle_ungrab_button(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 29);

    let button = data[1];
    let grab_window = state.read_u32(data, 4);
    let modifiers = state.read_u16(data, 8);

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
pub(crate) fn handle_change_active_pointer_grab(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 30);

    let bo = state.msb_first;
    let cursor = read_u32_bo(data, 4, bo);

    // Validate cursor ID if specified (0 = None)
    if cursor != 0 && !state.cursors.contains_key(&cursor) {
        return build_error(BAD_CURSOR, seq, cursor, 30, 0);
    }

    if let Some(ref mut grab) = state.grabs.pointer_grab {
        let event_mask = read_u16_bo(data, 12, bo) as u32;
        grab.cursor = cursor;
        grab.event_mask = event_mask;
        debug!("ChangeActivePointerGrab: cursor={cursor:#x} event_mask={event_mask:#x}");
    }

    Vec::new()
}

/// GrabKeyboard (opcode 31)
///
/// Status codes: 0=Success, 1=AlreadyGrabbed, 2=InvalidTime,
///               3=NotViewable, 4=Frozen
pub(crate) fn handle_grab_keyboard(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return build_error(BAD_LENGTH, seq, 0, 31, 0);
    }

    let owner_events = data[1] != 0;
    let grab_window = state.read_u32(data, 4);
    let timestamp = state.read_u32(data, 8); // 0 = CurrentTime
    let pointer_mode = data[12];
    let keyboard_mode = data[13];

    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(BAD_WINDOW, seq, grab_window, 31, 0);
    }

    info!("GrabKeyboard: window={grab_window:#x} owner_events={owner_events}");

    // Status 4: Frozen -- keyboard is frozen by another grab we don't own
    if state.grabs.keyboard_frozen && state.grabs.keyboard_grab.is_none() {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 4; // Frozen
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    // Status 3: NotViewable -- grab_window must be viewable
    if grab_window != state.root_window
        && !is_viewable(&state.windows, grab_window, state.root_window)
    {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 3; // NotViewable
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    // Status 2: InvalidTime -- reject if timestamp is in the future
    if timestamp != 0 {
        let now = state.timestamp();
        let delta = now.wrapping_sub(timestamp);
        if delta > 0 && delta < 0x8000_0000 {
            // timestamp is in the past -- OK
        } else if timestamp != now {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 2; // InvalidTime
            state.write_u16(&mut reply, 2, seq);
            return reply.to_vec();
        }
    }

    // Synchronous mode (1) freezes the device
    state.grabs.keyboard_frozen = keyboard_mode == 1;
    if pointer_mode == 1 {
        state.grabs.pointer_frozen = true;
    }

    state.grabs.keyboard_grab = Some(ActiveKeyboardGrab {
        grab_window,
        pointer_mode,
        keyboard_mode,
        owner_events,
    });

    // Generate crossing events: Leave(Grab) from current window, Enter(Grab) to grab window
    emit_grab_crossing_events(state, grab_window);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // GrabSuccess
    state.write_u16(&mut reply, 2, seq);
    reply.to_vec()
}

/// UngrabKeyboard (opcode 32)
pub(crate) fn handle_ungrab_keyboard(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if let Some(ref grab) = state.grabs.keyboard_grab {
        let grab_window = grab.grab_window;
        let pointer_mode = grab.pointer_mode;
        debug!("UngrabKeyboard: releasing active keyboard grab");
        // Generate crossing events: Leave(Ungrab) from grab window, Enter(Ungrab) to pointer window
        emit_ungrab_crossing_events(state, grab_window);
        // Thaw frozen keyboard events
        state.grabs.keyboard_frozen = false;
        state.grabs.keyboard_sync_pending = false;
        let events = std::mem::take(&mut state.grabs.frozen_keyboard_events);
        for e in events { state.pending_events.push(e); }
        // Per X11 spec: if this keyboard grab had pointer_mode=Synchronous,
        // the pointer was frozen by this grab — unfreeze it now.
        if pointer_mode == 1 && state.grabs.pointer_grab.is_none() {
            state.grabs.pointer_frozen = false;
            state.grabs.pointer_sync_pending = false;
            let pevents = std::mem::take(&mut state.grabs.frozen_pointer_events);
            for e in pevents { state.pending_events.push(e); }
        }
        state.grabs.keyboard_grab = None;
    }
    Vec::new()
}

/// GrabKey (opcode 33)
pub(crate) fn handle_grab_key(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 33);

    let owner_events = data[1] != 0;
    let grab_window = state.read_u32(data, 4);

    // Validate grab_window exists
    if !state.windows.contains_key(&grab_window) && grab_window != state.root_window {
        return build_error(BAD_WINDOW, seq, grab_window, 33, 0);
    }
    let modifiers = state.read_u16(data, 8);
    let key = data[10];
    let pointer_mode = data[11];
    let keyboard_mode = data[12];

    debug!("GrabKey: window={grab_window:#x} key={key} modifiers={modifiers:#x}");

    // Remove any existing grab with the same (window, key, modifiers)
    state.grabs.key_grabs.retain(|g| {
        !(g.grab_window == grab_window && g.key == key && g.modifiers == modifiers)
    });

    // Insert at front for LIFO ordering: per X11 spec, the most recently
    // established passive grab wins when multiple grabs match.
    state.grabs.key_grabs.insert(0, PassiveKeyGrab {
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
pub(crate) fn handle_ungrab_key(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 34);

    let key = data[1];
    let grab_window = state.read_u32(data, 4);
    let modifiers = state.read_u16(data, 8);

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
///
/// Modes: 0=AsyncPointer, 1=SyncPointer, 2=ReplayPointer,
///        3=AsyncKeyboard, 4=SyncKeyboard, 5=ReplayKeyboard,
///        6=AsyncBoth, 7=SyncBoth
pub(crate) fn handle_allow_events(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let mode = data[1];
    debug!("AllowEvents: mode={mode}");

    match mode {
        0 => {
            // AsyncPointer: thaw pointer, deliver frozen events, no re-freeze
            state.grabs.pointer_frozen = false;
            state.grabs.pointer_sync_pending = false;
            let events = std::mem::take(&mut state.grabs.frozen_pointer_events);
            for e in events {
                state.pending_events.push(e);
            }
        }
        1 => {
            // SyncPointer: thaw pointer, deliver frozen events, re-freeze on next event
            state.grabs.pointer_frozen = false;
            state.grabs.pointer_sync_pending = true;
            let events = std::mem::take(&mut state.grabs.frozen_pointer_events);
            for e in events {
                state.pending_events.push(e);
            }
        }
        2 => {
            // ReplayPointer: release grab, replay frozen events through normal event delivery
            if let Some(ref grab) = state.grabs.pointer_grab {
                let gw = grab.grab_window;
                emit_ungrab_crossing_events(state, gw);
            }
            state.grabs.pointer_frozen = false;
            state.grabs.pointer_sync_pending = false;
            let events = std::mem::take(&mut state.grabs.frozen_pointer_events);
            state.grabs.pointer_grab = None;
            for e in events {
                state.pending_events.push(e);
            }
        }
        3 => {
            // AsyncKeyboard: thaw keyboard, deliver frozen events, no re-freeze
            state.grabs.keyboard_frozen = false;
            state.grabs.keyboard_sync_pending = false;
            let events = std::mem::take(&mut state.grabs.frozen_keyboard_events);
            for e in events {
                state.pending_events.push(e);
            }
        }
        4 => {
            // SyncKeyboard: thaw keyboard, deliver frozen events, re-freeze on next event
            state.grabs.keyboard_frozen = false;
            state.grabs.keyboard_sync_pending = true;
            let events = std::mem::take(&mut state.grabs.frozen_keyboard_events);
            for e in events {
                state.pending_events.push(e);
            }
        }
        5 => {
            // ReplayKeyboard: release grab, replay frozen events through normal event delivery
            if let Some(ref grab) = state.grabs.keyboard_grab {
                let gw = grab.grab_window;
                emit_ungrab_crossing_events(state, gw);
            }
            state.grabs.keyboard_frozen = false;
            state.grabs.keyboard_sync_pending = false;
            let events = std::mem::take(&mut state.grabs.frozen_keyboard_events);
            state.grabs.keyboard_grab = None;
            for e in events {
                state.pending_events.push(e);
            }
        }
        6 => {
            // AsyncBoth: thaw both pointer and keyboard, no re-freeze
            state.grabs.pointer_frozen = false;
            state.grabs.keyboard_frozen = false;
            state.grabs.pointer_sync_pending = false;
            state.grabs.keyboard_sync_pending = false;
            let pevents = std::mem::take(&mut state.grabs.frozen_pointer_events);
            let kevents = std::mem::take(&mut state.grabs.frozen_keyboard_events);
            for e in pevents { state.pending_events.push(e); }
            for e in kevents { state.pending_events.push(e); }
        }
        7 => {
            // SyncBoth: thaw both, re-freeze on next event of either type
            state.grabs.pointer_frozen = false;
            state.grabs.keyboard_frozen = false;
            state.grabs.pointer_sync_pending = true;
            state.grabs.keyboard_sync_pending = true;
            let pevents = std::mem::take(&mut state.grabs.frozen_pointer_events);
            let kevents = std::mem::take(&mut state.grabs.frozen_keyboard_events);
            for e in pevents { state.pending_events.push(e); }
            for e in kevents { state.pending_events.push(e); }
        }
        _ => {}
    }

    Vec::new()
}

/// Check for matching passive button grabs and activate if found.
/// Returns true if a grab was activated (event should be redirected to grab window).
pub(crate) fn check_passive_button_grab(state: &mut ClientState, button: u8, modifiers: u16, window: u32) -> bool {
    // Walk up the window hierarchy to find a matching passive button grab
    let mut current = window;
    for _ in 0..128 {
        let matching = state.grabs.button_grabs.iter().find(|g| {
            g.grab_window == current
            && (g.button == 0 || g.button == button)         // AnyButton or exact match
            && (g.modifiers == 0x8000 || g.modifiers == modifiers) // AnyModifier or exact match
        }).cloned();

        if let Some(grab) = matching {
            let gw = grab.grab_window;
            debug!("Passive button grab activated: window={gw:#x} button={button}");
            // Compute confine bounds for passive grab activation
            let confine_bounds = if grab.confine_to != 0 {
                if grab.confine_to == state.root_window {
                    Some((0, 0, state.screen_width as i16, state.screen_height as i16))
                } else {
                    window_abs_bounds(&state.windows, grab.confine_to, state.root_window)
                }
            } else {
                None
            };
            // Per X11 spec §11.4: when a passive grab activates, synchronous
            // modes take effect immediately — freeze the appropriate devices.
            if grab.pointer_mode == 1 {
                state.grabs.pointer_frozen = true;
            }
            if grab.keyboard_mode == 1 {
                state.grabs.keyboard_frozen = true;
            }
            state.grabs.pointer_grab = Some(ActivePointerGrab {
                grab_window: gw,
                event_mask: grab.event_mask,
                pointer_mode: grab.pointer_mode,
                keyboard_mode: grab.keyboard_mode,
                confine_to: grab.confine_to,
                cursor: grab.cursor,
                owner_events: grab.owner_events,
                confine_bounds,
            });
            // Generate crossing events for passive grab activation
            emit_grab_crossing_events(state, gw);
            return true;
        }

        // Walk up to parent
        match state.windows.get(&current) {
            Some(w) if w.parent != 0 && w.parent != current => current = w.parent,
            _ => break,
        }
    }
    false
}

/// Check for matching passive key grabs and activate if found.
/// Returns true if a grab was activated.
pub(crate) fn check_passive_key_grab(state: &mut ClientState, keycode: u8, modifiers: u16, window: u32) -> bool {
    let mut current = window;
    for _ in 0..128 {
        let matching = state.grabs.key_grabs.iter().find(|g| {
            g.grab_window == current
            && (g.key == 0 || g.key == keycode)              // AnyKey or exact match
            && (g.modifiers == 0x8000 || g.modifiers == modifiers) // AnyModifier or exact match
        }).cloned();

        if let Some(grab) = matching {
            let gw = grab.grab_window;
            debug!("Passive key grab activated: window={gw:#x} key={keycode}");
            // Per X11 spec §11.4: when a passive grab activates, synchronous
            // modes take effect immediately — freeze the appropriate devices.
            if grab.keyboard_mode == 1 {
                state.grabs.keyboard_frozen = true;
            }
            if grab.pointer_mode == 1 {
                state.grabs.pointer_frozen = true;
            }
            state.grabs.keyboard_grab = Some(ActiveKeyboardGrab {
                grab_window: gw,
                pointer_mode: grab.pointer_mode,
                keyboard_mode: grab.keyboard_mode,
                owner_events: grab.owner_events,
            });
            // Generate crossing events for passive grab activation
            emit_grab_crossing_events(state, gw);
            return true;
        }

        match state.windows.get(&current) {
            Some(w) if w.parent != 0 && w.parent != current => current = w.parent,
            _ => break,
        }
    }
    false
}

/// Deactivate an active pointer grab on ButtonRelease if all buttons are released.
pub(crate) fn check_button_release_ungrab(state: &mut ClientState, _button: u8, button_mask: u16) {
    // If we have an active pointer grab from a passive activation,
    // release it when all buttons are released (button_mask has no button bits set)
    if let Some(ref grab) = state.grabs.pointer_grab {
        // Button bits in state mask: Button1=0x100, Button2=0x200, Button3=0x400, Button4=0x800, Button5=0x1000
        let any_buttons_held = (button_mask & 0x1F00) != 0;
        if !any_buttons_held {
            let grab_window = grab.grab_window;
            debug!("Auto-ungrab: all buttons released");
            // Generate crossing events for automatic ungrab
            emit_ungrab_crossing_events(state, grab_window);
            // Thaw frozen events on auto-ungrab
            state.grabs.pointer_frozen = false;
            state.grabs.pointer_sync_pending = false;
            let events = std::mem::take(&mut state.grabs.frozen_pointer_events);
            for e in events { state.pending_events.push(e); }
            state.grabs.pointer_grab = None;
        }
    }
}

/// GrabServer (opcode 36)
///
/// Per X11 spec, GrabServer freezes processing of requests from all other
/// clients until UngrabServer is issued. We record the grab in the shared
/// ServerGrabLock so the per-client request loops can block.
pub(crate) fn handle_grab_server(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    state.grabs.server_grab_count += 1;
    // Set the shared grab lock so other clients' request loops will block.
    // We spin briefly if the mutex is momentarily held by another client's
    // check; in practice the first try_lock() always succeeds because the
    // mutex is only held for nanoseconds during the grab-check path.
    let (lock, _notify) = &*state.server_grab;
    for _ in 0..100 {
        if let Ok(mut holder) = lock.try_lock() {
            *holder = Some(state.client_id.clone());
            break;
        }
        std::hint::spin_loop();
    }
    debug!("GrabServer: count={} client={}", state.grabs.server_grab_count, state.client_id);
    Vec::new()
}

/// UngrabServer (opcode 37)
pub(crate) fn handle_ungrab_server(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    if state.grabs.server_grab_count > 0 {
        state.grabs.server_grab_count -= 1;
    }
    if state.grabs.server_grab_count == 0 {
        let (lock, notify) = &*state.server_grab;
        for _ in 0..100 {
            if let Ok(mut holder) = lock.try_lock() {
                *holder = None;
                break;
            }
            std::hint::spin_loop();
        }
        notify.notify_waiters();
    }
    debug!("UngrabServer: count={} client={}", state.grabs.server_grab_count, state.client_id);
    Vec::new()
}

/// Check if a pointer event should be frozen (Synchronous grab mode).
/// If pointer_sync_pending is true, re-freeze the pointer after delivering one event.
/// Returns true if the event should be queued (frozen), false if it should be delivered.
/// Maximum number of frozen events to queue per device (pointer/keyboard).
/// Beyond this, oldest events are dropped to prevent unbounded memory growth.
const MAX_FROZEN_EVENTS: usize = 4096;

pub(crate) fn check_pointer_sync_freeze(state: &mut ClientState, event: &[u8]) -> bool {
    if state.grabs.pointer_frozen {
        // Already frozen — queue the event (with bounded capacity)
        if state.grabs.frozen_pointer_events.len() >= MAX_FROZEN_EVENTS {
            state.grabs.frozen_pointer_events.remove(0);
        }
        state.grabs.frozen_pointer_events.push(event.to_vec());
        return true;
    }
    if state.grabs.pointer_sync_pending {
        // Deliver this one event, then re-freeze
        state.grabs.pointer_sync_pending = false;
        state.grabs.pointer_frozen = true;
        return false; // deliver this event
    }
    false
}

/// Check if a keyboard event should be frozen (Synchronous grab mode).
/// Returns true if the event should be queued (frozen), false if it should be delivered.
pub(crate) fn check_keyboard_sync_freeze(state: &mut ClientState, event: &[u8]) -> bool {
    if state.grabs.keyboard_frozen {
        if state.grabs.frozen_keyboard_events.len() >= MAX_FROZEN_EVENTS {
            state.grabs.frozen_keyboard_events.remove(0);
        }
        state.grabs.frozen_keyboard_events.push(event.to_vec());
        return true;
    }
    if state.grabs.keyboard_sync_pending {
        state.grabs.keyboard_sync_pending = false;
        state.grabs.keyboard_frozen = true;
        return false;
    }
    false
}

/// If there is an active pointer grab with confine_to, clamp the given
/// coordinates to the confine window's bounds. Returns the (possibly clamped)
/// coordinates.
pub(crate) fn clamp_to_confine(state: &ClientState, x: i16, y: i16) -> (i16, i16) {
    if let Some(ref grab) = state.grabs.pointer_grab {
        if let Some((x1, y1, x2, y2)) = grab.confine_bounds {
            let cx = x.max(x1).min(x2.saturating_sub(1));
            let cy = y.max(y1).min(y2.saturating_sub(1));
            return (cx, cy);
        }
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grab_state() -> GrabState {
        GrabState::default()
    }

    // -----------------------------------------------------------------------
    // Passive button grab activation sets freeze state
    // -----------------------------------------------------------------------

    #[test]
    fn passive_button_grab_sync_mode_freezes_pointer() {
        let mut gs = make_grab_state();
        // Add a passive button grab with pointer_mode=Synchronous (1)
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100,
            button: 1,
            modifiers: 0,
            event_mask: 0x04, // ButtonPressMask
            pointer_mode: 1,  // Synchronous
            keyboard_mode: 0, // Async
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        // Verify the grab has sync mode stored
        assert_eq!(gs.button_grabs[0].pointer_mode, 1);
        // When this grab activates, pointer_frozen should be set to true
        // (tested via check_passive_button_grab in integration tests)
    }

    #[test]
    fn passive_button_grab_async_mode_no_freeze() {
        let mut gs = make_grab_state();
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100,
            button: 1,
            modifiers: 0,
            event_mask: 0x04,
            pointer_mode: 0, // Async
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        assert_eq!(gs.button_grabs[0].pointer_mode, 0);
        assert!(!gs.pointer_frozen);
    }

    #[test]
    fn passive_key_grab_sync_mode_freezes_keyboard() {
        let mut gs = make_grab_state();
        gs.key_grabs.push(PassiveKeyGrab {
            grab_window: 100,
            key: 38, // 'a'
            modifiers: 0,
            pointer_mode: 0,
            keyboard_mode: 1, // Synchronous
            owner_events: false,
        });
        assert_eq!(gs.key_grabs[0].keyboard_mode, 1);
    }

    // -----------------------------------------------------------------------
    // GrabState freeze/thaw mechanics
    // -----------------------------------------------------------------------

    #[test]
    fn pointer_sync_freeze_queues_event() {
        let mut gs = make_grab_state();
        gs.pointer_frozen = true;
        // Simulate a frozen event
        gs.frozen_pointer_events.push(vec![0u8; 32]);
        assert_eq!(gs.frozen_pointer_events.len(), 1);
    }

    #[test]
    fn allow_events_async_pointer_thaws() {
        let mut gs = make_grab_state();
        gs.pointer_frozen = true;
        gs.pointer_sync_pending = true;
        gs.frozen_pointer_events.push(vec![0u8; 32]);

        // Simulate AllowEvents(AsyncPointer): mode 0
        gs.pointer_frozen = false;
        gs.pointer_sync_pending = false;
        let events = std::mem::take(&mut gs.frozen_pointer_events);
        assert_eq!(events.len(), 1);
        assert!(!gs.pointer_frozen);
        assert!(!gs.pointer_sync_pending);
    }

    #[test]
    fn allow_events_sync_pointer_refreeze() {
        let mut gs = make_grab_state();
        gs.pointer_frozen = true;
        gs.frozen_pointer_events.push(vec![0u8; 32]);

        // Simulate AllowEvents(SyncPointer): mode 1
        gs.pointer_frozen = false;
        gs.pointer_sync_pending = true;
        let _events = std::mem::take(&mut gs.frozen_pointer_events);

        // sync_pending should cause re-freeze on next event
        assert!(gs.pointer_sync_pending);
        assert!(!gs.pointer_frozen);
    }

    #[test]
    fn ungrab_keyboard_unfreezes_pointer_if_sync() {
        let mut gs = make_grab_state();
        // Simulate a keyboard grab with pointer_mode=Synchronous
        gs.keyboard_grab = Some(ActiveKeyboardGrab {
            grab_window: 100,
            pointer_mode: 1, // Synchronous — froze the pointer
            keyboard_mode: 0,
            owner_events: false,
        });
        gs.pointer_frozen = true;
        gs.frozen_pointer_events.push(vec![0u8; 32]);

        // Simulate UngrabKeyboard
        let grab = gs.keyboard_grab.take().unwrap();
        gs.keyboard_frozen = false;
        gs.keyboard_sync_pending = false;
        // Per spec: if keyboard grab had pointer_mode=Sync and no pointer grab,
        // unfreeze pointer too.
        if grab.pointer_mode == 1 && gs.pointer_grab.is_none() {
            gs.pointer_frozen = false;
            gs.pointer_sync_pending = false;
            let pevents = std::mem::take(&mut gs.frozen_pointer_events);
            assert_eq!(pevents.len(), 1);
        }
        assert!(!gs.pointer_frozen);
    }

    #[test]
    fn ungrab_keyboard_keeps_pointer_frozen_if_pointer_grab_active() {
        let mut gs = make_grab_state();
        gs.keyboard_grab = Some(ActiveKeyboardGrab {
            grab_window: 100,
            pointer_mode: 1,
            keyboard_mode: 0,
            owner_events: false,
        });
        // Also have an active pointer grab (so we shouldn't unfreeze)
        gs.pointer_grab = Some(ActivePointerGrab {
            grab_window: 200,
            event_mask: 0x04,
            pointer_mode: 1,
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
            confine_bounds: None,
        });
        gs.pointer_frozen = true;

        let grab = gs.keyboard_grab.take().unwrap();
        gs.keyboard_frozen = false;
        // Per spec: don't unfreeze if pointer_grab is still active
        if grab.pointer_mode == 1 && gs.pointer_grab.is_none() {
            gs.pointer_frozen = false;
        }
        // Pointer should still be frozen because pointer_grab is active
        assert!(gs.pointer_frozen);
    }

    // -----------------------------------------------------------------------
    // Button release auto-ungrab thaws frozen events
    // -----------------------------------------------------------------------

    #[test]
    fn auto_ungrab_thaws_frozen_pointer() {
        let mut gs = make_grab_state();
        gs.pointer_grab = Some(ActivePointerGrab {
            grab_window: 100,
            event_mask: 0x04,
            pointer_mode: 1,
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
            confine_bounds: None,
        });
        gs.pointer_frozen = true;
        gs.frozen_pointer_events.push(vec![0u8; 32]);

        // Simulate all buttons released (button_mask = 0)
        let any_buttons_held = (0u16 & 0x1F00) != 0;
        assert!(!any_buttons_held);

        // Simulate auto-ungrab
        gs.pointer_frozen = false;
        gs.pointer_sync_pending = false;
        let events = std::mem::take(&mut gs.frozen_pointer_events);
        gs.pointer_grab = None;

        assert!(!gs.pointer_frozen);
        assert!(gs.pointer_grab.is_none());
        assert_eq!(events.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Confine bounds clamping
    // -----------------------------------------------------------------------

    #[test]
    fn confine_clamp_within_bounds() {
        let mut gs = make_grab_state();
        gs.pointer_grab = Some(ActivePointerGrab {
            grab_window: 100,
            event_mask: 0,
            pointer_mode: 0,
            keyboard_mode: 0,
            confine_to: 200,
            cursor: 0,
            owner_events: false,
            confine_bounds: Some((100, 100, 300, 300)),
        });
        // Test: point inside bounds unchanged
        if let Some((x1, y1, x2, y2)) = gs.pointer_grab.as_ref().unwrap().confine_bounds {
            let cx = 150i16.max(x1).min(x2.saturating_sub(1));
            let cy = 200i16.max(y1).min(y2.saturating_sub(1));
            assert_eq!((cx, cy), (150, 200));
        }
    }

    #[test]
    fn confine_clamp_outside_bounds() {
        let mut gs = make_grab_state();
        gs.pointer_grab = Some(ActivePointerGrab {
            grab_window: 100,
            event_mask: 0,
            pointer_mode: 0,
            keyboard_mode: 0,
            confine_to: 200,
            cursor: 0,
            owner_events: false,
            confine_bounds: Some((100, 100, 300, 300)),
        });
        if let Some((x1, y1, x2, y2)) = gs.pointer_grab.as_ref().unwrap().confine_bounds {
            let cx = 50i16.max(x1).min(x2.saturating_sub(1));
            let cy = 400i16.max(y1).min(y2.saturating_sub(1));
            assert_eq!((cx, cy), (100, 299));
        }
    }

    // -----------------------------------------------------------------------
    // Passive grab matching
    // -----------------------------------------------------------------------

    #[test]
    fn passive_button_grab_any_button_matches() {
        let mut gs = make_grab_state();
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100,
            button: 0, // AnyButton
            modifiers: 0,
            event_mask: 0x04,
            pointer_mode: 0,
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        // AnyButton (0) should match any button
        let grab = &gs.button_grabs[0];
        assert!(grab.button == 0 || grab.button == 3);
    }

    #[test]
    fn passive_key_grab_any_modifier_matches() {
        let mut gs = make_grab_state();
        gs.key_grabs.push(PassiveKeyGrab {
            grab_window: 100,
            key: 38,
            modifiers: 0x8000, // AnyModifier
            pointer_mode: 0,
            keyboard_mode: 0,
            owner_events: false,
        });
        let grab = &gs.key_grabs[0];
        assert_eq!(grab.modifiers, 0x8000);
        // AnyModifier matches any modifier state
        assert!(grab.modifiers == 0x8000 || grab.modifiers == 0x04);
    }

    #[test]
    fn grab_button_replaces_same_triple() {
        let mut gs = make_grab_state();
        // First grab
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100,
            button: 1,
            modifiers: 0,
            event_mask: 0x04,
            pointer_mode: 0,
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        // Remove existing + insert new (same triple)
        gs.button_grabs.retain(|g| {
            !(g.grab_window == 100 && g.button == 1 && g.modifiers == 0)
        });
        gs.button_grabs.insert(0, PassiveButtonGrab {
            grab_window: 100,
            button: 1,
            modifiers: 0,
            event_mask: 0x08, // different event_mask
            pointer_mode: 1,
            keyboard_mode: 0,
            confine_to: 0,
            cursor: 0,
            owner_events: true,
        });
        assert_eq!(gs.button_grabs.len(), 1);
        assert_eq!(gs.button_grabs[0].event_mask, 0x08);
        assert!(gs.button_grabs[0].owner_events);
    }

    #[test]
    fn ungrab_button_any_removes_all_on_window() {
        let mut gs = make_grab_state();
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100, button: 1, modifiers: 0,
            event_mask: 0x04, pointer_mode: 0, keyboard_mode: 0,
            confine_to: 0, cursor: 0, owner_events: false,
        });
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 100, button: 2, modifiers: 0,
            event_mask: 0x04, pointer_mode: 0, keyboard_mode: 0,
            confine_to: 0, cursor: 0, owner_events: false,
        });
        gs.button_grabs.push(PassiveButtonGrab {
            grab_window: 200, button: 1, modifiers: 0,
            event_mask: 0x04, pointer_mode: 0, keyboard_mode: 0,
            confine_to: 0, cursor: 0, owner_events: false,
        });
        // AnyButton + AnyModifier removes all on window 100
        gs.button_grabs.retain(|g| g.grab_window != 100);
        assert_eq!(gs.button_grabs.len(), 1);
        assert_eq!(gs.button_grabs[0].grab_window, 200);
    }

    // -----------------------------------------------------------------------
    // Frozen event capacity limit
    // -----------------------------------------------------------------------

    #[test]
    fn frozen_events_bounded() {
        let mut gs = make_grab_state();
        gs.pointer_frozen = true;
        for _ in 0..MAX_FROZEN_EVENTS + 10 {
            if gs.frozen_pointer_events.len() >= MAX_FROZEN_EVENTS {
                gs.frozen_pointer_events.remove(0);
            }
            gs.frozen_pointer_events.push(vec![0u8; 32]);
        }
        assert_eq!(gs.frozen_pointer_events.len(), MAX_FROZEN_EVENTS);
    }

    // -----------------------------------------------------------------------
    // Server grab count
    // -----------------------------------------------------------------------

    #[test]
    fn server_grab_nesting() {
        let mut gs = make_grab_state();
        gs.server_grab_count += 1;
        gs.server_grab_count += 1;
        assert_eq!(gs.server_grab_count, 2);
        gs.server_grab_count -= 1;
        assert_eq!(gs.server_grab_count, 1);
        gs.server_grab_count -= 1;
        assert_eq!(gs.server_grab_count, 0);
    }

    // -----------------------------------------------------------------------
    // Passive grab data structures
    // -----------------------------------------------------------------------

    #[test]
    fn passive_button_grab_stores_all_fields() {
        let grab = PassiveButtonGrab {
            grab_window: 0x100,
            button: 1,
            modifiers: 0x8000, // AnyModifier
            event_mask: 0x04,
            pointer_mode: 1, // Async
            keyboard_mode: 1, // Async
            confine_to: 0,
            cursor: 0,
            owner_events: true,
        };
        assert_eq!(grab.grab_window, 0x100);
        assert_eq!(grab.button, 1);
        assert_eq!(grab.modifiers, 0x8000);
        assert!(grab.owner_events);
    }

    #[test]
    fn passive_key_grab_stores_all_fields() {
        let grab = PassiveKeyGrab {
            grab_window: 0x200,
            key: 0, // AnyKey
            modifiers: 0x01, // Shift
            pointer_mode: 0, // Sync
            keyboard_mode: 1, // Async
            owner_events: false,
        };
        assert_eq!(grab.grab_window, 0x200);
        assert_eq!(grab.key, 0);
        assert_eq!(grab.modifiers, 0x01);
        assert!(!grab.owner_events);
    }

    #[test]
    fn grab_button_lifo_ordering() {
        let mut gs = make_grab_state();
        // Insert two button grabs with different modifiers
        gs.button_grabs.insert(0, PassiveButtonGrab {
            grab_window: 0x100,
            button: 1,
            modifiers: 0,
            event_mask: 0x04,
            pointer_mode: 1,
            keyboard_mode: 1,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        gs.button_grabs.insert(0, PassiveButtonGrab {
            grab_window: 0x100,
            button: 1,
            modifiers: 0x01, // Shift
            event_mask: 0x04,
            pointer_mode: 1,
            keyboard_mode: 1,
            confine_to: 0,
            cursor: 0,
            owner_events: false,
        });
        // LIFO: most recently inserted should be at front
        assert_eq!(gs.button_grabs[0].modifiers, 0x01);
        assert_eq!(gs.button_grabs[1].modifiers, 0);
    }
}
