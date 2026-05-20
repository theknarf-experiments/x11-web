//! XKB state and controls: GetState, LatchLockState, GetControls, SetControls.

use super::super::super::client::ClientState;
use super::{
    XKB_ACCESS_X_FEEDBACK_MASK, XKB_ACCESS_X_KEYS_MASK, XKB_ACCESS_X_TIMEOUT_MASK,
    XKB_ALL_BOOLEAN_CTRLS_MASK, XKB_BOUNCE_KEYS_MASK, XKB_MOUSE_KEYS_MASK, XKB_REPEAT_KEYS_MASK,
    XKB_SLOW_KEYS_MASK,
};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_i16_bo as read_i16, read_u16_bo as read_u16};
use crate::xserver::reply::serialize_reply;
use tracing::debug;
use x11rb_protocol::protocol::xkb::{
    AXOption, BoolCtrl, GetControlsReply, GetStateReply, Group, VMod,
};
use x11rb_protocol::protocol::xproto::{KeyButMask, ModMask};

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

/// Build an XKB GetState reply with real modifier/group state.
pub(crate) fn build_xkb_get_state_reply(state: &ClientState, seq: u16, device_id: u8) -> Vec<u8> {
    let xkb = &state.xkb;
    let effective_mods = xkb.effective_mods();
    let effective_group = xkb.effective_group() as u8;

    serialize_reply(
        &GetStateReply {
            device_id,
            sequence: seq,
            length: 0,
            mods: ModMask::from(effective_mods as u16),
            base_mods: ModMask::from(xkb.base_mods as u16),
            latched_mods: ModMask::from(xkb.latched_mods as u16),
            locked_mods: ModMask::from(xkb.locked_mods as u16),
            group: Group::from(effective_group),
            locked_group: Group::from(xkb.locked_group as u8),
            base_group: xkb.base_group,
            latched_group: xkb.latched_group,
            compat_state: ModMask::from(effective_mods as u16),
            grab_mods: ModMask::from(effective_mods as u16),
            compat_grab_mods: ModMask::from(effective_mods as u16),
            lookup_mods: ModMask::from(effective_mods as u16),
            compat_lookup_mods: ModMask::from(effective_mods as u16),
            ptr_btn_state: KeyButMask::from(0u16),
        },
        state.byte_order(),
    )
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

    let xkb = &mut state.xkb;

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
        state.xkb.locked_mods, state.xkb.latched_mods,
        state.xkb.locked_group, state.xkb.latched_group
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
    let ctrls = &state.xkb.controls;
    let zero_mod = ModMask::from(0u16);

    serialize_reply(
        &GetControlsReply {
            device_id,
            sequence: seq,
            length: 0,
            mouse_keys_dflt_btn: ctrls.mk_dflt_btn,
            num_groups: ctrls.num_groups.max(1),
            groups_wrap: 0,
            internal_mods_mask: zero_mod,
            ignore_lock_mods_mask: zero_mod,
            internal_mods_real_mods: zero_mod,
            ignore_lock_mods_real_mods: zero_mod,
            internal_mods_vmods: VMod::from(0u16),
            ignore_lock_mods_vmods: VMod::from(0u16),
            repeat_delay: ctrls.repeat_delay,
            repeat_interval: ctrls.repeat_interval,
            slow_keys_delay: ctrls.slow_keys_delay,
            debounce_delay: ctrls.debounce_delay,
            mouse_keys_delay: ctrls.mk_delay,
            mouse_keys_interval: ctrls.mk_interval,
            mouse_keys_time_to_max: ctrls.mk_time_to_max,
            mouse_keys_max_speed: ctrls.mk_max_speed,
            mouse_keys_curve: ctrls.mk_curve,
            access_x_option: AXOption::from(ctrls.ax_options as u16),
            access_x_timeout: ctrls.ax_timeout,
            access_x_timeout_options_mask: AXOption::from(0u16),
            access_x_timeout_options_values: AXOption::from(0u16),
            access_x_timeout_mask: BoolCtrl::from(0u32),
            access_x_timeout_values: BoolCtrl::from(0u32),
            enabled_controls: BoolCtrl::from(ctrls.enabled_ctrls),
            per_key_repeat: ctrls.per_key_repeat,
        },
        state.byte_order(),
    )
}

/// Handle XKB SetControls request via the typed `xkb::SetControlsRequest`.
pub(crate) fn handle_xkb_set_controls(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use super::super::parse_minor;
    use x11rb_protocol::protocol::xkb::SetControlsRequest;

    let req = parse_minor!(SetControlsRequest, data, state, seq, 136, data[1] as u16);
    let change_ctrls = u32::from(req.change_controls);
    let enabled_ctrls_before = state.xkb.controls.enabled_ctrls;
    let ctrls = &mut state.xkb.controls;

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
