//! XKB state and controls: GetState, LatchLockState, GetControls, SetControls.

use super::super::super::client::ClientState;
use super::{
    XKB_REPEAT_KEYS_MASK, XKB_SLOW_KEYS_MASK, XKB_BOUNCE_KEYS_MASK,
    XKB_MOUSE_KEYS_MASK, XKB_ACCESS_X_KEYS_MASK, XKB_ACCESS_X_TIMEOUT_MASK,
    XKB_ACCESS_X_FEEDBACK_MASK, XKB_ALL_BOOLEAN_CTRLS_MASK,
};
use crate::xserver::core::{read_u16_bo as read_u16, read_u32_bo as read_u32, read_i16_bo as read_i16};
use tracing::debug;

/// Build an XKB GetState reply with real modifier/group state.
pub(crate) fn build_xkb_get_state_reply(state: &ClientState, seq: u16, device_id: u8) -> Vec<u8> {
    let xkb = &state.xkb_state;
    let effective_mods = xkb.effective_mods();
    let effective_group = xkb.effective_group() as u8;

    // GetState reply layout (32 bytes):
    //   0: type = Reply (1)
    //   1: deviceID
    //   2-3: sequence
    //   4-7: length (0 for 32-byte reply)
    //   8: mods (effective modifiers)
    //   9: baseMods
    //  10: latchedMods
    //  11: lockedMods
    //  12: group (effective group)
    //  13: lockedGroup
    //  14-15: baseGroup (INT16)
    //  16-17: latchedGroup (INT16)
    //  18: compatState (same as mods for compat)
    //  19: grabMods (same as effective for now)
    //  20: compatGrabMods
    //  21: lookupMods (same as effective minus internal)
    //  22: compatLookupMods
    //  23: pad
    //  24-25: ptrBtnState (pointer button state)
    //  26-31: pad
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = device_id;
    state.write_u16(&mut reply, 2, seq);
    // length = 0 (fits in 32 bytes)
    reply[8] = effective_mods;
    reply[9] = xkb.base_mods;
    reply[10] = xkb.latched_mods;
    reply[11] = xkb.locked_mods;
    reply[12] = effective_group;
    reply[13] = xkb.locked_group as u8;
    state.write_i16(&mut reply, 14, xkb.base_group);
    state.write_i16(&mut reply, 16, xkb.latched_group);
    reply[18] = effective_mods; // compatState
    reply[19] = effective_mods; // grabMods
    reply[20] = effective_mods; // compatGrabMods
    reply[21] = effective_mods; // lookupMods
    reply[22] = effective_mods; // compatLookupMods
    // 23 = pad
    // 24-25: ptrBtnState = 0 (no pointer button state tracked here)
    reply.to_vec()
}

/// Handle XKB LatchLockState request.
pub(crate) fn handle_xkb_latch_lock_state(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            136, data[1] as u16, state.msb_first,
        );
    }

    let msb = state.msb_first;
    let _device_spec = read_u16(data, 4, msb);
    let affect_mod_locks = data[6];
    let mod_locks = data[7];
    let lock_group = data[8] != 0;
    let group_lock = data[9] as i16;
    let affect_mod_latches = data[10];
    let mod_latches = data[11];
    // Bytes 12-13: pad
    let latch_group = if data.len() >= 15 { data[14] != 0 } else { false };
    let group_latch = if data.len() >= 16 { read_i16(data, 14, msb) } else { 0 };

    let xkb = &mut state.xkb_state;

    // Apply modifier locks
    if affect_mod_locks != 0 {
        xkb.locked_mods = (xkb.locked_mods & !affect_mod_locks) | (mod_locks & affect_mod_locks);
    }

    // Apply modifier latches
    if affect_mod_latches != 0 {
        xkb.latched_mods = (xkb.latched_mods & !affect_mod_latches) | (mod_latches & affect_mod_latches);
    }

    // Apply group lock
    if lock_group {
        xkb.locked_group = group_lock;
    }

    // Apply group latch
    if latch_group {
        xkb.latched_group = group_latch;
    }

    debug!(
        "LatchLockState: locked_mods=0x{:02x} latched_mods=0x{:02x} locked_group={} latched_group={}",
        xkb.locked_mods, xkb.latched_mods, xkb.locked_group, xkb.latched_group
    );

    Vec::new()
}

/// Build an XKB GetControls reply with real control state.
pub(crate) fn build_xkb_get_controls_reply(state: &ClientState, seq: u16, device_id: u8) -> Vec<u8> {
    let ctrls = &state.xkb_state.controls;

    // GetControls reply: 92 bytes total (32 header + 60 body)
    let mut reply = vec![0u8; 92];
    reply[0] = 1;
    reply[1] = device_id;
    state.write_u16(&mut reply, 2, seq);
    // length = (92 - 32) / 4 = 15
    state.write_u32(&mut reply, 4, 15);

    // Byte 8: mouseKeysDfltBtn
    reply[8] = ctrls.mk_dflt_btn;
    // Byte 9: numGroups (must be >= 1)
    reply[9] = ctrls.num_groups.max(1);
    // Byte 10: groupsWrap = 0 (Wrap)
    // Byte 11: internalMods mask
    // Byte 12: ignoreLockMods mask
    // Byte 13: internalMods realMods
    // Byte 14: ignoreLockMods realMods
    // Bytes 15-16: internalMods vmods (CARD16)
    // Bytes 17-18: ignoreLockMods vmods (CARD16)

    // Byte 20-21: repeatDelay
    state.write_u16(&mut reply, 20, ctrls.repeat_delay);
    // Byte 22-23: repeatInterval
    state.write_u16(&mut reply, 22, ctrls.repeat_interval);

    // Bytes 24-25: slowKeysDelay
    state.write_u16(&mut reply, 24, ctrls.slow_keys_delay);
    // Bytes 26-27: debounceDelay
    state.write_u16(&mut reply, 26, ctrls.debounce_delay);

    // Bytes 28-29: mouseKeysDelay
    state.write_u16(&mut reply, 28, ctrls.mk_delay);
    // Bytes 30-31: mouseKeysInterval
    state.write_u16(&mut reply, 30, ctrls.mk_interval);
    // Bytes 32-33: mouseKeysTimeToMax
    state.write_u16(&mut reply, 32, ctrls.mk_time_to_max);
    // Bytes 34-35: mouseKeysMaxSpeed
    state.write_u16(&mut reply, 34, ctrls.mk_max_speed);
    // Bytes 36-37: mouseKeysCurve (INT16)
    state.write_i16(&mut reply, 36, ctrls.mk_curve);

    // Bytes 38-39: accessXOption (CARD16)
    state.write_u16(&mut reply, 38, ctrls.ax_options as u16);

    // Bytes 40-41: accessXTimeout
    state.write_u16(&mut reply, 40, ctrls.ax_timeout);

    // Bytes 42-43: accessXTimeoutOptionsMask
    // Bytes 44-45: accessXTimeoutOptionsValues
    // Bytes 46-47: pad

    // Bytes 48-51: accessXTimeoutMask (CARD32)
    // Bytes 52-55: accessXTimeoutValues (CARD32)

    // Bytes 56-59: enabledControls (CARD32)
    state.write_u32(&mut reply, 56, ctrls.enabled_ctrls);

    // Bytes 60-91: perKeyRepeat (32 bytes)
    reply[60..92].copy_from_slice(&ctrls.per_key_repeat);

    reply
}

/// Handle XKB SetControls request.
pub(crate) fn handle_xkb_set_controls(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 92 {
        debug!("SetControls: request too short ({} bytes)", data.len());
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            136, data[1] as u16, state.msb_first,
        );
    }

    // Read all values upfront to avoid borrow conflicts
    let msb = state.msb_first;
    let change_ctrls = read_u32(data, 8, msb);
    let repeat_delay = read_u16(data, 24, msb);
    let repeat_interval = read_u16(data, 26, msb);
    let slow_keys_delay = read_u16(data, 28, msb);
    let debounce_delay = read_u16(data, 30, msb);
    let mk_delay = read_u16(data, 32, msb);
    let mk_interval = read_u16(data, 34, msb);
    let mk_time_to_max = read_u16(data, 36, msb);
    let mk_max_speed = read_u16(data, 38, msb);
    let mk_curve = read_i16(data, 40, msb);
    let ax_timeout = read_u16(data, 44, msb);
    let ax_options = read_u32(data, 48, msb);
    let enable_ctrls = read_u32(data, 56, msb);
    let disable_ctrls = read_u32(data, 60, msb);

    let ctrls = &mut state.xkb_state.controls;

    // Byte 12: mouseKeysDfltBtn
    if change_ctrls & XKB_MOUSE_KEYS_MASK != 0 {
        ctrls.mk_dflt_btn = data[12];
    }

    // Byte 13: numGroups
    let new_groups = data[13];
    if new_groups >= 1 && new_groups <= 4 {
        ctrls.num_groups = new_groups;
    }

    // RepeatKeys controls
    if change_ctrls & XKB_REPEAT_KEYS_MASK != 0 {
        if repeat_delay > 0 { ctrls.repeat_delay = repeat_delay; }
        if repeat_interval > 0 { ctrls.repeat_interval = repeat_interval; }
    }

    // SlowKeys
    if change_ctrls & XKB_SLOW_KEYS_MASK != 0 {
        ctrls.slow_keys_delay = slow_keys_delay;
    }

    // BounceKeys
    if change_ctrls & XKB_BOUNCE_KEYS_MASK != 0 {
        ctrls.debounce_delay = debounce_delay;
    }

    // MouseKeys
    if change_ctrls & XKB_MOUSE_KEYS_MASK != 0 {
        ctrls.mk_delay = mk_delay;
        ctrls.mk_interval = mk_interval;
        ctrls.mk_time_to_max = mk_time_to_max;
        ctrls.mk_max_speed = mk_max_speed;
        ctrls.mk_curve = mk_curve;
    }

    // AccessX settings
    if change_ctrls & (XKB_ACCESS_X_KEYS_MASK | XKB_ACCESS_X_TIMEOUT_MASK | XKB_ACCESS_X_FEEDBACK_MASK) != 0 {
        ctrls.ax_timeout = ax_timeout;
        ctrls.ax_options = ax_options & 0xFFFF;
    }

    // Enabled controls bitmask
    ctrls.enabled_ctrls = (ctrls.enabled_ctrls | enable_ctrls) & !disable_ctrls;
    ctrls.enabled_ctrls &= XKB_ALL_BOOLEAN_CTRLS_MASK;

    // Per-key repeat bitmap (32 bytes at offset 64)
    if change_ctrls & XKB_REPEAT_KEYS_MASK != 0 && data.len() >= 96 {
        ctrls.per_key_repeat.copy_from_slice(&data[64..96]);
    }

    debug!(
        "SetControls: change=0x{change_ctrls:08x} enabled=0x{:08x} repeat_delay={} repeat_interval={}",
        ctrls.enabled_ctrls, ctrls.repeat_delay, ctrls.repeat_interval
    );

    Vec::new()
}
