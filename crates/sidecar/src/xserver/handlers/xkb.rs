//! XKB (X Keyboard Extension) handler — module root.
//!
//! Submodules split by concern:
//!   map        — GetMap, SetMap, GetKbdByName, key-name/sym tables
//!   controls   — GetState, LatchLockState, GetControls, SetControls
//!   names      — GetNames, SetNames
//!   compat     — GetCompatMap, SetCompatMap, compat compilation
//!   indicators — GetIndicatorState, GetIndicatorMap, SetIndicatorMap, GetNamedIndicator
//!   device     — ListComponents, GetDeviceInfo, SetDeviceInfo
use super::parse_minor;
use crate::xserver::reply::ReplyBuf;

mod compat;
mod controls;
mod device;
mod geometry;
mod indicators;
mod map;
mod names;

use super::super::client::{ClientState, XkbIndicatorMap, XkbNamedIndicator, XkbSymInterpretation};
use tracing::debug;

/// Build the default compat SI table for initializing new ClientState instances.
pub(crate) fn default_compat_si() -> Vec<XkbSymInterpretation> {
    compat::default_si_table()
}

// ---------------------------------------------------------------------------
// XKB constants — shared across all submodules via `super::` paths
// ---------------------------------------------------------------------------

const MIN_KEY_CODE: u8 = 8;
const MAX_KEY_CODE: u8 = 255;
const N_KEYS: usize = (MAX_KEY_CODE - MIN_KEY_CODE + 1) as usize; // 248

// XKB action types
#[allow(dead_code)]
const SA_NO_ACTION: u8 = 0;
const SA_SET_MODS: u8 = 1;
const SA_LOCK_MODS: u8 = 3;
const SA_SET_GROUP: u8 = 4;
#[allow(dead_code)]
const SA_LATCH_GROUP: u8 = 5;
const SA_LOCK_GROUP: u8 = 6;

// XKB behavior types
#[allow(dead_code)]
const KB_DEFAULT: u8 = 0;
const KB_LOCK: u8 = 1;

/// Number of keyboard groups (layouts) we support.
/// XKB allows up to 4 groups. We always advertise all 4 slots but
/// populate groups 2-4 only when the client has configured them via
/// SetMap or the server has loaded additional layouts.
const MAX_GROUPS: u8 = 4;

// XKB control bits
const XKB_REPEAT_KEYS_MASK: u32 = 1 << 0;
const XKB_SLOW_KEYS_MASK: u32 = 1 << 1;
const XKB_BOUNCE_KEYS_MASK: u32 = 1 << 2;
#[allow(dead_code)]
const XKB_STICKY_KEYS_MASK: u32 = 1 << 3;
const XKB_MOUSE_KEYS_MASK: u32 = 1 << 4;
#[allow(dead_code)]
const XKB_MOUSE_KEYS_ACCEL_MASK: u32 = 1 << 5;
const XKB_ACCESS_X_KEYS_MASK: u32 = 1 << 6;
const XKB_ACCESS_X_TIMEOUT_MASK: u32 = 1 << 7;
const XKB_ACCESS_X_FEEDBACK_MASK: u32 = 1 << 8;
#[allow(dead_code)]
const XKB_AUDIBLE_BELL_MASK: u32 = 1 << 9;
const XKB_ALL_BOOLEAN_CTRLS_MASK: u32 = (1 << 10) - 1;

// Modifier key keycodes (evdev)
const MODIFIER_KEYS: &[(u8, u8, u8)] = &[
    // (keycode, real_mod_bit, virtual_mod_index)
    // Shift_L, Shift_R → Shift (bit 0)
    (50, 0x01, 0xFF),
    (62, 0x01, 0xFF),
    // Caps_Lock → Lock (bit 1)
    (66, 0x02, 0xFF),
    // Control_L, Control_R → Control (bit 2)
    (37, 0x04, 0xFF),
    (105, 0x04, 0xFF),
    // Alt_L, Alt_R → Mod1 (bit 3), vmod 0 (Alt)
    (64, 0x08, 0),
    (108, 0x08, 0),
    // Num_Lock → Mod2 (bit 4), vmod 1 (NumLock)
    (77, 0x10, 1),
    // Super_L, Super_R → Mod4 (bit 6), vmod 3 (Super)
    (133, 0x40, 3),
    (134, 0x40, 3),
];

// ---------------------------------------------------------------------------
// SelectEvents (minor opcode 1)
// ---------------------------------------------------------------------------

/// Handle XKB SelectEvents: parse the affectWhich/selectAll/clear fields from
/// the request and update this client's `xkb_event_mask` accordingly.
///
/// Wire layout (bytes, 0-indexed from start of request data):
///   0     major opcode (XKB)
///   1     minor opcode (1 = SelectEvents)
///   2-3   request length in 4-byte units
///   4-5   deviceSpec (u16)
///   6-7   affectWhich (u16) — which event bits this request touches
///   8-9   clear (u16)       — bits to clear unconditionally
///  10-11  selectAll (u16)   — bits to set unconditionally
///  12-13  affectMap (u16)   — (for Map details, ignored here)
///  14-15  map (u16)
///
/// New mask formula (per XKB spec §7.3):
///   new_mask = (old_mask & !affectWhich) | (selectAll & affectWhich & !clear)
fn handle_xkb_select_events(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        debug!("XKB SelectEvents: request too short ({} bytes)", data.len());
        return Vec::new();
    }
    let affect_which = state.read_u16(data, 6) as u32;
    let clear = state.read_u16(data, 8) as u32;
    let select_all = state.read_u16(data, 10) as u32;

    let old_mask = state.xkb_event_mask;
    // Bits touched by this request: first clear them, then apply selectAll
    let new_mask = (old_mask & !affect_which) | (select_all & affect_which & !clear);
    state.xkb_event_mask = new_mask;

    debug!(
        "XKB SelectEvents: affectWhich={affect_which:#06x} clear={clear:#06x} \
         selectAll={select_all:#06x} old={old_mask:#010x} new={new_mask:#010x}"
    );
    Vec::new() // void request
}

// ---------------------------------------------------------------------------
// SetCompatMap (minor opcode 11) — delegated to compat module
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SetNamedIndicator (minor opcode 16)
// ---------------------------------------------------------------------------

/// Handle XKB SetNamedIndicator: parse indicator name atom and settings and
/// store them in `state.xkb_named_indicators`.
///
/// Wire layout (bytes):
///   0-1   major/minor opcode
///   2-3   request length
///   4-5   deviceSpec (u16)
///   6-7   ledClass (u16)
///   8-9   ledID (u16)
///  10-11  pad
///  12-15  indicator (ATOM) — name atom for this indicator
///  16     setState (BOOL)  — whether to change indicator on/off state
///  17     on (BOOL)        — new state if setState
///  18     setMap (BOOL)    — whether to change the indicator map
///  19     createMap (BOOL) — create if not present
///  20     pad
///  21     map.flags (CARD8)
///  22     map.whichGroups (CARD8)
///  23     map.groups (CARD8)
///  24     map.whichMods (CARD8)
///  25     map.mods (CARD8)
///  26     map.realMods (CARD8)
///  27-28  map.vmods (CARD16)
///  29-32  map.ctrls (CARD32)
fn handle_xkb_set_named_indicator(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    use x11rb_protocol::protocol::xkb::SetNamedIndicatorRequest;
    let Ok(req) = SetNamedIndicatorRequest::try_parse_request(
        crate::xserver::request::request_header(data),
        &data[4..],
    ) else {
        debug!("XKB SetNamedIndicator: parse failed ({}B)", data.len());
        return Vec::new();
    };

    let indicator_atom = req.indicator;
    let set_state = req.set_state;
    let on = req.on;
    let set_map = req.set_map;

    // Parse the indicator map if setMap is true; otherwise preserve the
    // existing map for this indicator (or default).
    let map = if set_map {
        XkbIndicatorMap {
            which_groups: u8::from(req.map_which_groups),
            groups: u8::from(req.map_groups),
            which_mods: u8::from(req.map_which_mods),
            mods: u16::from(req.map_real_mods) as u8,
            ctrls: u32::from(req.map_ctrls),
        }
    } else {
        state
            .xkb_named_indicators
            .get(&indicator_atom)
            .map(|ni| ni.map.clone())
            .unwrap_or_default()
    };

    // Determine the indicator index: try the existing entry first, then
    // allocate a new slot (capped at 31 so the 32-bit state bitmask is safe).
    let next_index = state.xkb_named_indicators.len().min(31) as u8;
    let index = state
        .xkb_named_indicators
        .get(&indicator_atom)
        .map(|ni| ni.index)
        .unwrap_or(next_index);

    let entry = XkbNamedIndicator {
        index,
        change_state: set_state,
        led_state: on,
        affect_which: u8::from(req.map_flags),
        change_which: u8::from(set_map),
        map,
    };

    debug!(
        "XKB SetNamedIndicator: atom={indicator_atom} index={index} \
         setState={set_state} on={on} setMap={set_map}"
    );

    state.xkb_named_indicators.insert(indicator_atom, entry);

    if set_state {
        if on {
            state.xkb_indicators |= 1 << index;
        } else {
            state.xkb_indicators &= !(1u32 << index);
        }
    }

    Vec::new() // void request
}

// ---------------------------------------------------------------------------
// Generic Event Extension handler
// ---------------------------------------------------------------------------

pub(crate) fn handle_ge_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Generic Event Extension minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.0
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // major version
                .set_u16(10, 0) // minor version
                .build()
        }
        _ => {
            debug!("Unhandled GE minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                135,
                minor as u16,
                state.msb_first,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Main XKB dispatcher
// ---------------------------------------------------------------------------

pub(crate) fn handle_xkb_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XKB minor opcode: {minor}");

    let device_id_byte = if data.len() >= 6 { data[4] } else { 0 };

    match minor {
        0 => {
            // UseExtension: reply with supported=true, version 1.0
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(1) // supported = true
                .set_u16(8, 1) // server major version
                .set_u16(10, 0) // server minor version
                .build()
        }
        1 => handle_xkb_select_events(state, data), // SelectEvents
        3 => {
            // Bell: ring the bell with percent from request.
            // XKB Bell request layout: 4-5=deviceSpec, 6-7=bellClass,
            //   8-9=bellID, 10=percent, 11=forceSound, 12=soundOnly,
            //   13=pad, 14-17=name(ATOM), 18-21=window
            let percent = if data.len() > 10 {
                data[10]
            } else {
                state.keyboard_control.bell_percent
            };
            let _force = if data.len() > 11 {
                data[11] != 0
            } else {
                false
            };
            debug!("XKB Bell: percent={percent}");
            // No wire reply for Bell (it is void). We log it so the
            // frontend or test harness can observe bell events.
            Vec::new()
        }
        4 => controls::build_xkb_get_state_reply(state, seq, device_id_byte),
        5 => controls::handle_xkb_latch_lock_state(state, data, seq),
        6 => controls::build_xkb_get_controls_reply(state, seq, device_id_byte),
        7 => controls::handle_xkb_set_controls(state, data, seq),
        8 => map::build_xkb_get_map_reply(state, seq),
        9 => map::handle_xkb_set_map(state, data, seq),
        10 => compat::build_xkb_get_compat_map_reply(state, seq, device_id_byte),
        11 => compat::handle_xkb_set_compat_map(state, data), // SetCompatMap
        12 => indicators::handle_get_indicator_state(state, seq, device_id_byte),
        13 => indicators::handle_get_indicator_map(state, data, seq, device_id_byte),
        14 => indicators::handle_set_indicator_map(state, data),
        15 => indicators::handle_get_named_indicator(state, data, seq, device_id_byte),
        16 => handle_xkb_set_named_indicator(state, data), // SetNamedIndicator
        17 => {
            // GetNames: which-name bitmask selects which strings to return.
            use x11rb_protocol::protocol::xkb::GetNamesRequest;
            let req_which = GetNamesRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            )
            .map(|r| u32::from(r.which))
            .unwrap_or(0x0FFF);
            names::build_xkb_get_names_reply(state, seq, device_id_byte, req_which)
        }
        18 => names::handle_xkb_set_names(state, data, seq), // SetNames
        19 => geometry::build_xkb_get_geometry_reply(state, seq, device_id_byte),
        20 => geometry::handle_xkb_set_geometry(state, data, seq),
        21 => {
            // PerClientFlags: reply with `value & change` and the supported mask.
            use x11rb_protocol::protocol::xkb::PerClientFlagsRequest;
            let req = parse_minor!(PerClientFlagsRequest, data, state, seq, 135, 21);
            let change = u32::from(req.change);
            let value = u32::from(req.value);
            let result = value & change;
            debug!("PerClientFlags reply");
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(device_id_byte)
                .set_u32(8, 0x1F) // supported flags (all per-client flags)
                .set_u32(12, result)
                .set_u32(16, u32::from(req.auto_ctrls))
                .set_u32(20, u32::from(req.auto_ctrls_values))
                .build()
        }
        22 => device::handle_list_components(state, seq, device_id_byte),
        23 => map::handle_xkb_get_kbd_by_name(state, data, seq, device_id_byte),
        24 => device::handle_get_device_info(state, seq, device_id_byte),
        25 => device::handle_set_device_info(state, data),
        101 => {
            // SetDebuggingFlags: reply with all-zero flags/ctrls.
            // Wire: 4-7=msg_length, 8-11=affect_flags, 12-15=flags,
            //       16-19=affect_ctrls, 20-23=ctrls, then message.
            // 8-11: currentFlags = 0
            // 12-15: supportedFlags = 0
            // 16-19: currentCtrls = 0
            // 20-23: supportedCtrls = 0
            // All zeros already.
            debug!("SetDebuggingFlags: returning zeros");
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(device_id_byte)
                .build()
        }
        _ => {
            debug!("Unhandled XKB minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                136,
                minor as u16,
                state.msb_first,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// XKB event constants
// ---------------------------------------------------------------------------

/// XKB extension event code (first_event assigned by QueryExtension).
pub(crate) const XKB_EVENT_BASE: u8 = 85;

// XKB event type codes (xkbType field in the event)
const XKB_STATE_NOTIFY: u8 = 0;
const XKB_MAP_NOTIFY: u8 = 1;
const XKB_CONTROLS_NOTIFY: u8 = 3;

// XKB SelectEvents mask bits (from spec Table 18.1)
const XKB_STATE_NOTIFY_MASK: u32 = 1 << 0;
const XKB_MAP_NOTIFY_MASK: u32 = 1 << 1;
const XKB_CONTROLS_NOTIFY_MASK: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// XKB event builders
// ---------------------------------------------------------------------------

/// Snapshot of XKB modifier/group state for change detection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct XkbStateSnapshot {
    pub(crate) base_mods: u8,
    pub(crate) latched_mods: u8,
    pub(crate) locked_mods: u8,
    pub(crate) base_group: i16,
    pub(crate) latched_group: i16,
    pub(crate) locked_group: i16,
}

impl XkbStateSnapshot {
    /// Capture the current XKB state.
    pub(crate) fn capture(state: &ClientState) -> Self {
        let xkb = &state.xkb_state;
        Self {
            base_mods: xkb.base_mods,
            latched_mods: xkb.latched_mods,
            locked_mods: xkb.locked_mods,
            base_group: xkb.base_group,
            latched_group: xkb.latched_group,
            locked_group: xkb.locked_group,
        }
    }
}

/// Build and enqueue an XkbStateNotify event if the client subscribed and
/// the XKB state actually changed from `before`.
///
/// The `keycode` is the triggering key (0 if triggered by LatchLockState).
/// The `event_type` is the X11 event type that triggered the change
/// (2=KeyPress, 3=KeyRelease, 0=programmatic).
pub(crate) fn maybe_send_xkb_state_notify(
    state: &mut ClientState,
    before: &XkbStateSnapshot,
    keycode: u8,
    event_type: u8,
) {
    // Check if this client subscribed to XkbStateNotify.
    if state.xkb_event_mask & XKB_STATE_NOTIFY_MASK == 0 {
        return;
    }

    let after = XkbStateSnapshot::capture(state);
    if *before == after {
        return; // No change
    }

    let xkb = &state.xkb_state;
    let effective_mods = xkb.effective_mods();
    let effective_group = xkb.effective_group() as u8;

    // Build the changed bitmask: which components changed.
    let mut changed: u16 = 0;
    if before.base_mods != after.base_mods {
        changed |= 1 << 0;
    } // ModifierState
    if before.latched_mods != after.latched_mods {
        changed |= 1 << 1;
    } // ModifierBase
    if before.locked_mods != after.locked_mods {
        changed |= 1 << 2;
    } // ModifierLatch
    if before.base_group != after.base_group {
        changed |= 1 << 3;
    }
    if before.latched_group != after.latched_group {
        changed |= 1 << 4;
    }
    if before.locked_group != after.locked_group {
        changed |= 1 << 5;
    }

    // XkbStateNotify event layout (32 bytes):
    //   0: event code (XKB_EVENT_BASE)
    //   1: xkbType (0 = StateNotify)
    //   2-3: sequence number
    //   4-7: time (CARD32)
    //   8: deviceID
    //   9: mods (effective)
    //  10: baseMods
    //  11: latchedMods
    //  12: lockedMods
    //  13: group (effective)
    //  14: baseGroup (INT16, low byte)
    //  15: baseGroup (INT16, high byte)
    //  16-17: latchedGroup (INT16)
    //  18: compatState
    //  19: grabMods
    //  20: compatGrabMods
    //  21: lookupMods
    //  22: compatLookupMods
    //  23: ptrBtnState (high byte)
    //  24-25: changed (CARD16)
    //  26: keycode
    //  27: eventType (what triggered: KeyPress=2, KeyRelease=3)
    //  28-29: requestMajor (CARD8) / requestMinor (CARD8)
    //  30-31: pad
    let mut event = [0u8; 32];
    event[0] = XKB_EVENT_BASE;
    event[1] = XKB_STATE_NOTIFY;
    state.write_u16(&mut event, 2, state.sequence);
    state.write_u32(&mut event, 4, state.timestamp());
    event[8] = 0; // deviceID = core keyboard
    event[9] = effective_mods;
    event[10] = after.base_mods;
    event[11] = after.latched_mods;
    event[12] = after.locked_mods;
    event[13] = effective_group;
    state.write_i16(&mut event, 14, after.base_group);
    state.write_i16(&mut event, 16, after.latched_group);
    event[18] = effective_mods; // compatState
    event[19] = effective_mods; // grabMods
    event[20] = effective_mods; // compatGrabMods
    event[21] = effective_mods; // lookupMods
    event[22] = effective_mods; // compatLookupMods
                                // 23: ptrBtnState high byte = 0
    state.write_u16(&mut event, 24, changed);
    event[26] = keycode;
    event[27] = event_type;

    state.pending_events.push(event.to_vec());
}

/// Build and enqueue an XkbMapNotify event if the client subscribed.
///
/// Called after SetMap to notify interested clients of keymap changes.
/// The `changed` bitmask indicates which components were modified.
pub(crate) fn maybe_send_xkb_map_notify(state: &mut ClientState, changed: u16) {
    if state.xkb_event_mask & XKB_MAP_NOTIFY_MASK == 0 {
        return;
    }

    // XkbMapNotify event layout (32 bytes):
    //   0: event code (XKB_EVENT_BASE)
    //   1: xkbType (1 = MapNotify)
    //   2-3: sequence number
    //   4-7: time
    //   8: deviceID
    //   9: ptrBtnActions
    //  10-11: changed (CARD16)
    //  12: minKeyCode
    //  13: maxKeyCode
    //  14: firstType
    //  15: nTypes
    //  16: firstKeySym
    //  17: nKeySyms
    //  18: firstKeyAct
    //  19: nKeyActs
    //  20: firstKeyBehavior
    //  21: nKeyBehaviors
    //  22: firstKeyExplicit
    //  23: nKeyExplicit
    //  24: firstModMapKey
    //  25: nModMapKeys
    //  26: firstVModMapKey
    //  27: nVModMapKeys
    //  28-29: virtualMods (CARD16)
    //  30-31: pad
    let mut event = [0u8; 32];
    event[0] = XKB_EVENT_BASE;
    event[1] = XKB_MAP_NOTIFY;
    state.write_u16(&mut event, 2, state.sequence);
    state.write_u32(&mut event, 4, state.timestamp());
    event[8] = 0; // deviceID
    state.write_u16(&mut event, 10, changed);
    event[12] = MIN_KEY_CODE;
    event[13] = MAX_KEY_CODE;
    // For simplicity, report the full range for all components.
    event[14] = 0; // firstType
    event[15] = 4; // nTypes (typical)
    event[16] = MIN_KEY_CODE; // firstKeySym
    event[17] = MAX_KEY_CODE - MIN_KEY_CODE + 1; // nKeySyms
    event[18] = MIN_KEY_CODE; // firstKeyAct
    event[19] = MAX_KEY_CODE - MIN_KEY_CODE + 1; // nKeyActs

    state.pending_events.push(event.to_vec());
}

/// Build and enqueue an XkbControlsNotify event if the client subscribed.
///
/// Called after SetControls with the bitmask of changed control fields.
pub(crate) fn maybe_send_xkb_controls_notify(
    state: &mut ClientState,
    changed_ctrls: u32,
    enabled_ctrls_before: u32,
) {
    if state.xkb_event_mask & XKB_CONTROLS_NOTIFY_MASK == 0 {
        return;
    }

    let enabled_changes = state.xkb_state.controls.enabled_ctrls ^ enabled_ctrls_before;

    // XkbControlsNotify event layout (32 bytes):
    //   0: event code (XKB_EVENT_BASE)
    //   1: xkbType (3 = ControlsNotify)
    //   2-3: sequence number
    //   4-7: time
    //   8: deviceID
    //   9: numGroups
    //  10-11: pad
    //  12-15: changedControls (CARD32)
    //  16-19: enabledControls (CARD32)
    //  20-23: enabledControlChanges (CARD32)
    //  24: keycode
    //  25: eventType
    //  26-27: requestMajor/requestMinor
    //  28-31: pad
    let mut event = [0u8; 32];
    event[0] = XKB_EVENT_BASE;
    event[1] = XKB_CONTROLS_NOTIFY;
    state.write_u16(&mut event, 2, state.sequence);
    state.write_u32(&mut event, 4, state.timestamp());
    event[8] = 0; // deviceID
    event[9] = state.xkb_state.controls.num_groups;
    state.write_u32(&mut event, 12, changed_ctrls);
    state.write_u32(&mut event, 16, state.xkb_state.controls.enabled_ctrls);
    state.write_u32(&mut event, 20, enabled_changes);

    state.pending_events.push(event.to_vec());
}
