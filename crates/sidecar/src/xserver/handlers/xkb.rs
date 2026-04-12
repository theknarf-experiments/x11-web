//! XKB (X Keyboard Extension) handler — module root.
//!
//! Submodules split by concern:
//!   map        — GetMap, SetMap, GetKbdByName, key-name/sym tables
//!   controls   — GetState, LatchLockState, GetControls, SetControls
//!   names      — GetNames, SetNames
//!   compat     — GetCompatMap, SetCompatMap, compat compilation
//!   indicators — GetIndicatorState, GetIndicatorMap, SetIndicatorMap, GetNamedIndicator
//!   device     — ListComponents, GetDeviceInfo, SetDeviceInfo

mod map;
mod controls;
mod names;
mod compat;
mod indicators;
mod device;
mod geometry;

use super::super::client::{ClientState, XkbNamedIndicator, XkbIndicatorMap, XkbSymInterpretation};
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
    let clear        = state.read_u16(data, 8) as u32;
    let select_all   = state.read_u16(data, 10) as u32;

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
    if data.len() < 20 {
        debug!("XKB SetNamedIndicator: request too short ({} bytes)", data.len());
        return Vec::new();
    }

    let indicator_atom = state.read_u32(data, 12);
    let set_state      = data.get(16).copied().unwrap_or(0) != 0;
    let on             = data.get(17).copied().unwrap_or(0) != 0;
    let set_map        = data.get(18).copied().unwrap_or(0) != 0;

    // Parse the indicator map if setMap is true and we have enough bytes.
    let map = if set_map && data.len() >= 33 {
        let ctrls = state.read_u32(data, 29);
        XkbIndicatorMap {
            which_groups: data[22],
            groups:       data[23],
            which_mods:   data[24],
            mods:         data[25],
            ctrls,
        }
    } else {
        // Preserve the existing map for this indicator, or default.
        state.xkb_named_indicators
            .get(&indicator_atom)
            .map(|ni| ni.map.clone())
            .unwrap_or_default()
    };

    // Determine the indicator index: try the existing entry first, then
    // allocate a new slot (capped at 31 so the 32-bit state bitmask is safe).
    let next_index = state.xkb_named_indicators.len().min(31) as u8;
    let index = state.xkb_named_indicators
        .get(&indicator_atom)
        .map(|ni| ni.index)
        .unwrap_or(next_index);

    let entry = XkbNamedIndicator {
        index,
        change_state: set_state,
        led_state: on,
        affect_which: data.get(21).copied().unwrap_or(0),
        change_which: data.get(18).copied().unwrap_or(0),
        map,
    };

    debug!(
        "XKB SetNamedIndicator: atom={indicator_atom} index={index} \
         setState={set_state} on={on} setMap={set_map}"
    );

    state.xkb_named_indicators.insert(indicator_atom, entry);

    // If the caller requested an explicit LED state change, apply it.
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
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1); // major version
            state.write_u16(&mut reply, 10, 0); // minor version
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled GE minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                135, minor as u16, state.msb_first,
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
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // supported = true
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1); // server major version
            state.write_u16(&mut reply, 10, 0); // server minor version
            reply.to_vec()
        }
        1 => handle_xkb_select_events(state, data), // SelectEvents
        3 => {
            // Bell: ring the bell with percent from request.
            // XKB Bell request layout: 4-5=deviceSpec, 6-7=bellClass,
            //   8-9=bellID, 10=percent, 11=forceSound, 12=soundOnly,
            //   13=pad, 14-17=name(ATOM), 18-21=window
            let percent = if data.len() > 10 { data[10] } else { state.keyboard_control.bell_percent };
            let _force = if data.len() > 11 { data[11] != 0 } else { false };
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
            // GetNames: request bytes 8-11 contain the `which` bitmask.
            let req_which: u32 = if data.len() >= 12 { state.read_u32(data, 8) } else { 0x0FFF };
            names::build_xkb_get_names_reply(state, seq, device_id_byte, req_which)
        }
        18 => names::handle_xkb_set_names(state, data, seq), // SetNames
        19 => geometry::build_xkb_get_geometry_reply(state, seq, device_id_byte),
        20 => geometry::handle_xkb_set_geometry(state, data, seq),
        21 => {
            // PerClientFlags: parse change/value/ctrls and reply with supported flags.
            // Wire: 4-5=device_spec, 8-11=change, 12-15=value,
            //       16-19=ctrls_to_change, 20-23=auto_ctrls, 24-27=auto_ctrls_values
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            state.write_u16(&mut reply, 2, seq);
            if data.len() >= 28 {
                let change = state.read_u32(data, 8);
                let value = state.read_u32(data, 12);
                let _ctrls_to_change = state.read_u32(data, 16);
                let auto_ctrls = state.read_u32(data, 20);
                let auto_ctrls_values = state.read_u32(data, 24);
                let result = value & change;
                let supported: u32 = 0x1F; // all per-client flags supported
                state.write_u32(&mut reply, 8, supported);    // supported
                state.write_u32(&mut reply, 12, result);      // value
                state.write_u32(&mut reply, 16, auto_ctrls);  // autoCtrls
                state.write_u32(&mut reply, 20, auto_ctrls_values); // autoCtrlsValues
            } else if data.len() >= 16 {
                let change = state.read_u32(data, 8);
                let value = state.read_u32(data, 12);
                let result = value & change;
                state.write_u32(&mut reply, 8, 0x1F); // supported
                state.write_u32(&mut reply, 12, result);
            }
            debug!("PerClientFlags reply");
            reply.to_vec()
        }
        22 => device::handle_list_components(state, seq, device_id_byte),
        23 => map::handle_xkb_get_kbd_by_name(state, data, seq, device_id_byte),
        24 => device::handle_get_device_info(state, seq, device_id_byte),
        25 => device::handle_set_device_info(state, data),
        101 => {
            // SetDebuggingFlags: reply with all-zero flags/ctrls.
            // Wire: 4-7=msg_length, 8-11=affect_flags, 12-15=flags,
            //       16-19=affect_ctrls, 20-23=ctrls, then message.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = device_id_byte;
            state.write_u16(&mut reply, 2, seq);
            // 8-11: currentFlags = 0
            // 12-15: supportedFlags = 0
            // 16-19: currentCtrls = 0
            // 20-23: supportedCtrls = 0
            // All zeros already.
            debug!("SetDebuggingFlags: returning zeros");
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled XKB minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                136, minor as u16, state.msb_first,
            )
        }
    }
}
