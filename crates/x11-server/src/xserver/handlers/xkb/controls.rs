//! XKB state and controls: GetState, LatchLockState, GetControls, SetControls.

use super::super::super::client::ClientState;
use super::{
    XKB_ACCESS_X_FEEDBACK_MASK, XKB_ACCESS_X_KEYS_MASK, XKB_ACCESS_X_TIMEOUT_MASK,
    XKB_ALL_BOOLEAN_CTRLS_MASK, XKB_BOUNCE_KEYS_MASK, XKB_MOUSE_KEYS_MASK, XKB_REPEAT_KEYS_MASK,
    XKB_SLOW_KEYS_MASK,
};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_i16_bo as read_i16, read_u16_bo as read_u16};
use crate::xserver::reply::ReplyBuf;
use tracing::debug;

/// Reply-byte offsets for the XKB GetState reply (32 bytes total).
mod get_state_reply {
    pub(super) const MODS: usize = 8;
    pub(super) const BASE_MODS: usize = 9;
    pub(super) const LATCHED_MODS: usize = 10;
    pub(super) const LOCKED_MODS: usize = 11;
    pub(super) const GROUP: usize = 12;
    pub(super) const LOCKED_GROUP: usize = 13;
    pub(super) const BASE_GROUP: usize = 14; // i16
    pub(super) const LATCHED_GROUP: usize = 16; // i16
    pub(super) const COMPAT_STATE: usize = 18;
    pub(super) const GRAB_MODS: usize = 19;
    pub(super) const COMPAT_GRAB_MODS: usize = 20;
    pub(super) const LOOKUP_MODS: usize = 21;
    pub(super) const COMPAT_LOOKUP_MODS: usize = 22;
    // 23 = pad; 24-25 ptrBtnState; 26-31 pad
}

/// Request-byte offsets for the XKB LatchLockState wire request (16 bytes).
mod latch_lock_state_req {
    pub(super) const DEVICE_SPEC: usize = 4; // u16
    pub(super) const AFFECT_MOD_LOCKS: usize = 6;
    pub(super) const MOD_LOCKS: usize = 7;
    pub(super) const LOCK_GROUP: usize = 8;
    pub(super) const GROUP_LOCK: usize = 9;
    pub(super) const AFFECT_MOD_LATCHES: usize = 10;
    pub(super) const MOD_LATCHES: usize = 11;
    // 12-13 = pad
    pub(super) const LATCH_GROUP: usize = 14;
    pub(super) const GROUP_LATCH: usize = 14; // i16, overlapping name
}

/// Reply-byte offsets for the XKB GetControls reply (32 + 60 = 92 bytes).
mod get_controls_reply {
    pub(super) const MK_DFLT_BTN: usize = 8;
    pub(super) const NUM_GROUPS: usize = 9;
    // 10: groupsWrap, 11-14: internal/ignore mod masks, 15-18: vmods
    pub(super) const REPEAT_DELAY: usize = 20; // u16
    pub(super) const REPEAT_INTERVAL: usize = 22; // u16
    pub(super) const SLOW_KEYS_DELAY: usize = 24; // u16
    pub(super) const DEBOUNCE_DELAY: usize = 26; // u16
    pub(super) const MK_DELAY: usize = 28; // u16
    pub(super) const MK_INTERVAL: usize = 30; // u16
    pub(super) const MK_TIME_TO_MAX: usize = 32; // u16
    pub(super) const MK_MAX_SPEED: usize = 34; // u16
    pub(super) const MK_CURVE: usize = 36; // i16
    pub(super) const AX_OPTIONS: usize = 38; // u16
    pub(super) const AX_TIMEOUT: usize = 40; // u16
    // 42-55: various timeout masks/values (left zero)
    pub(super) const ENABLED_CTRLS: usize = 56; // u32
    pub(super) const PER_KEY_REPEAT: std::ops::Range<usize> = 60..92;
}

/// Build an XKB GetState reply with real modifier/group state.
pub(crate) fn build_xkb_get_state_reply(state: &ClientState, seq: u16, device_id: u8) -> Vec<u8> {
    let xkb = &state.xkb_state;
    let effective_mods = xkb.effective_mods();
    let effective_group = xkb.effective_group() as u8;

    // GetState reply layout per `get_state_reply` module above. Bytes 23,
    // 24-25 (ptrBtnState), and 26-31 are left zero.
    use get_state_reply as r;
    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(device_id)
        .set_u8(r::MODS, effective_mods)
        .set_u8(r::BASE_MODS, xkb.base_mods)
        .set_u8(r::LATCHED_MODS, xkb.latched_mods)
        .set_u8(r::LOCKED_MODS, xkb.locked_mods)
        .set_u8(r::GROUP, effective_group)
        .set_u8(r::LOCKED_GROUP, xkb.locked_group as u8)
        .set_i16(r::BASE_GROUP, xkb.base_group)
        .set_i16(r::LATCHED_GROUP, xkb.latched_group)
        .set_u8(r::COMPAT_STATE, effective_mods)
        .set_u8(r::GRAB_MODS, effective_mods)
        .set_u8(r::COMPAT_GRAB_MODS, effective_mods)
        .set_u8(r::LOOKUP_MODS, effective_mods)
        .set_u8(r::COMPAT_LOOKUP_MODS, effective_mods)
        .build()
}

/// Handle XKB LatchLockState request.
pub(crate) fn handle_xkb_latch_lock_state(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 16, seq, 136, data[1] as u16, state.msb_first);

    let before = super::XkbStateSnapshot::capture(state);

    use latch_lock_state_req as r;
    let msb = state.msb_first;
    let _device_spec = read_u16(data, r::DEVICE_SPEC, msb);
    let affect_mod_locks = data[r::AFFECT_MOD_LOCKS];
    let mod_locks = data[r::MOD_LOCKS];
    let lock_group = data[r::LOCK_GROUP] != 0;
    let group_lock = data[r::GROUP_LOCK] as i16;
    let affect_mod_latches = data[r::AFFECT_MOD_LATCHES];
    let mod_latches = data[r::MOD_LATCHES];
    let latch_group = if data.len() >= 15 {
        data[r::LATCH_GROUP] != 0
    } else {
        false
    };
    let group_latch = if data.len() >= 16 {
        read_i16(data, r::GROUP_LATCH, msb)
    } else {
        0
    };

    let xkb = &mut state.xkb_state;

    // Apply modifier locks
    if affect_mod_locks != 0 {
        xkb.locked_mods = (xkb.locked_mods & !affect_mod_locks) | (mod_locks & affect_mod_locks);
    }

    // Apply modifier latches
    if affect_mod_latches != 0 {
        xkb.latched_mods =
            (xkb.latched_mods & !affect_mod_latches) | (mod_latches & affect_mod_latches);
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
        state.xkb_state.locked_mods, state.xkb_state.latched_mods,
        state.xkb_state.locked_group, state.xkb_state.latched_group
    );

    // Send XkbStateNotify if state changed and client subscribed.
    super::maybe_send_xkb_state_notify(state, &before, 0, 0);

    Vec::new()
}

/// Build an XKB GetControls reply with real control state.
pub(crate) fn build_xkb_get_controls_reply(
    state: &ClientState,
    seq: u16,
    device_id: u8,
) -> Vec<u8> {
    let ctrls = &state.xkb_state.controls;

    // GetControls reply: 92 bytes total (32 header + 60 body). Bytes 10-19,
    // 42-55 are left zero (groupsWrap, internal/ignore mod masks + vmods,
    // various timeout masks).
    use get_controls_reply as r;
    let mut reply = ReplyBuf::with_extra(seq, 60, state.msb_first)
        .set_data_byte(device_id)
        .set_u8(r::MK_DFLT_BTN, ctrls.mk_dflt_btn)
        .set_u8(r::NUM_GROUPS, ctrls.num_groups.max(1));

    reply = reply
        .set_u16(r::REPEAT_DELAY, ctrls.repeat_delay)
        .set_u16(r::REPEAT_INTERVAL, ctrls.repeat_interval)
        .set_u16(r::SLOW_KEYS_DELAY, ctrls.slow_keys_delay)
        .set_u16(r::DEBOUNCE_DELAY, ctrls.debounce_delay)
        .set_u16(r::MK_DELAY, ctrls.mk_delay)
        .set_u16(r::MK_INTERVAL, ctrls.mk_interval)
        .set_u16(r::MK_TIME_TO_MAX, ctrls.mk_time_to_max)
        .set_u16(r::MK_MAX_SPEED, ctrls.mk_max_speed)
        .set_i16(r::MK_CURVE, ctrls.mk_curve)
        .set_u16(r::AX_OPTIONS, ctrls.ax_options as u16)
        .set_u16(r::AX_TIMEOUT, ctrls.ax_timeout)
        .set_u32(r::ENABLED_CTRLS, ctrls.enabled_ctrls);

    reply.buf_mut()[r::PER_KEY_REPEAT].copy_from_slice(&ctrls.per_key_repeat);

    reply.build()
}

/// Handle XKB SetControls request via the typed `xkb::SetControlsRequest`.
pub(crate) fn handle_xkb_set_controls(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use super::super::parse_minor;
    use x11rb_protocol::protocol::xkb::SetControlsRequest;

    let req = parse_minor!(SetControlsRequest, data, state, seq, 136, data[1] as u16);
    let change_ctrls = u32::from(req.change_controls);
    let enabled_ctrls_before = state.xkb_state.controls.enabled_ctrls;
    let ctrls = &mut state.xkb_state.controls;

    if change_ctrls & XKB_MOUSE_KEYS_MASK != 0 {
        ctrls.mk_dflt_btn = req.mouse_keys_dflt_btn;
    }

    if (1..=4).contains(&req.groups_wrap) {
        ctrls.num_groups = req.groups_wrap;
    }

    if change_ctrls & XKB_REPEAT_KEYS_MASK != 0 {
        if req.repeat_delay > 0 {
            ctrls.repeat_delay = req.repeat_delay;
        }
        if req.repeat_interval > 0 {
            ctrls.repeat_interval = req.repeat_interval;
        }
    }

    if change_ctrls & XKB_SLOW_KEYS_MASK != 0 {
        ctrls.slow_keys_delay = req.slow_keys_delay;
    }
    if change_ctrls & XKB_BOUNCE_KEYS_MASK != 0 {
        ctrls.debounce_delay = req.debounce_delay;
    }

    if change_ctrls & XKB_MOUSE_KEYS_MASK != 0 {
        ctrls.mk_delay = req.mouse_keys_delay;
        ctrls.mk_interval = req.mouse_keys_interval;
        ctrls.mk_time_to_max = req.mouse_keys_time_to_max;
        ctrls.mk_max_speed = req.mouse_keys_max_speed;
        ctrls.mk_curve = req.mouse_keys_curve;
    }

    if change_ctrls
        & (XKB_ACCESS_X_KEYS_MASK | XKB_ACCESS_X_TIMEOUT_MASK | XKB_ACCESS_X_FEEDBACK_MASK)
        != 0
    {
        ctrls.ax_timeout = req.access_x_timeout;
        ctrls.ax_options = u32::from(u16::from(req.access_x_options));
    }

    let enable_ctrls = u32::from(req.enabled_controls);
    let disable_ctrls = u32::from(req.affect_enabled_controls) & !enable_ctrls;
    ctrls.enabled_ctrls = (ctrls.enabled_ctrls | enable_ctrls) & !disable_ctrls;
    ctrls.enabled_ctrls &= XKB_ALL_BOOLEAN_CTRLS_MASK;

    if change_ctrls & XKB_REPEAT_KEYS_MASK != 0 {
        ctrls
            .per_key_repeat
            .copy_from_slice(&req.per_key_repeat[..]);
    }

    debug!(
        "SetControls: change=0x{change_ctrls:08x} enabled=0x{:08x} repeat_delay={} repeat_interval={}",
        ctrls.enabled_ctrls, ctrls.repeat_delay, ctrls.repeat_interval
    );

    super::maybe_send_xkb_controls_notify(state, change_ctrls, enabled_ctrls_before);

    Vec::new()
}
