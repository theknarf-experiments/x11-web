//! XKB indicator operations: GetIndicatorState, GetIndicatorMap, SetIndicatorMap, GetNamedIndicator.

use super::super::super::client::ClientState;
use crate::xserver::core::read_u32_bo as read_u32;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xkb::{
    GetIndicatorMapReply, GetIndicatorStateReply, GetNamedIndicatorReply, IMFlag, IMGroupsWhich,
    IMModsWhich, IndicatorMap as WireIndicatorMap, SetOfGroup, SetOfGroups, VMod,
};

/// Real-modifier mask bits as `u8` aliases of `x11rb::xproto::ModMask`.
const MOD_LOCK: u8 = 1 << 1; // Caps Lock
const MOD_M2: u8 = 1 << 4; // Num Lock
const MOD_M3: u8 = 1 << 5; // Scroll Lock

/// XkbIM_UseLocked flag for the IndicatorMap whichMods field.
const WHICH_MODS_USE_LOCKED: u8 = 0x04;

/// XkbIM_UseEffective flag for the IndicatorMap whichGroups field.
const WHICH_GROUPS_USE_EFFECTIVE: u8 = 0x80;

/// Group bitmask for "lit when not in group 0" — covers groups 2, 3, and 4.
const GROUPS_NON_BASE: u8 = 0x0E;

/// XkbIndicatorMapWireDesc offset of the ctrls u32 — used when parsing
/// `SetIndicatorMap` requests since the inline ctrls field is at offset 8
/// within each 12-byte entry.
const SET_INDICATOR_CTRLS_OFFSET: usize = 8;
const INDICATOR_MAP_WIRE_SIZE: usize = 12;

/// Handle GetIndicatorState (minor opcode 12).
pub(crate) fn handle_get_indicator_state(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let mut ind_state: u32 = 0;
    let eff_mods = state.xkb_state.effective_mods();
    if eff_mods & MOD_LOCK != 0 {
        ind_state |= 1 << 0;
    }
    if eff_mods & MOD_M2 != 0 {
        ind_state |= 1 << 1;
    }
    if eff_mods & MOD_M3 != 0 {
        ind_state |= 1 << 2;
    }
    if state.xkb_state.effective_group() > 0 {
        ind_state |= 1 << 3;
    }
    state.xkb_indicators = ind_state;

    serialize_reply(
        &GetIndicatorStateReply {
            device_id: device_id_byte,
            sequence: seq,
            length: 0,
            state: ind_state,
        },
        state.byte_order(),
    )
}

fn empty_indicator_map() -> WireIndicatorMap {
    WireIndicatorMap {
        flags: IMFlag::from(0u8),
        which_groups: IMGroupsWhich::from(0u8),
        groups: SetOfGroup::from(0u8),
        which_mods: IMModsWhich::from(0u8),
        mods: x11rb_protocol::protocol::xproto::ModMask::from(0u16),
        real_mods: x11rb_protocol::protocol::xproto::ModMask::from(0u16),
        vmods: VMod::from(0u16),
        ctrls: x11rb_protocol::protocol::xkb::BoolCtrl::from(0u32),
    }
}

/// Handle GetIndicatorMap (minor opcode 13).
pub(crate) fn handle_get_indicator_map(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let which: u32 = if data.len() >= 12 {
        state.read_u32(data, 8)
    } else {
        0x0F
    };
    let mut maps: Vec<WireIndicatorMap> = Vec::new();
    for bit in 0..32u32 {
        if which & (1 << bit) == 0 {
            continue;
        }
        let mut m = empty_indicator_map();
        match bit {
            0 => {
                // Caps Lock: driven by Lock modifier
                m.which_mods = IMModsWhich::from(WHICH_MODS_USE_LOCKED);
                m.mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_LOCK as u16);
                m.real_mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_LOCK as u16);
            }
            1 => {
                m.which_mods = IMModsWhich::from(WHICH_MODS_USE_LOCKED);
                m.mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_M2 as u16);
                m.real_mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_M2 as u16);
            }
            2 => {
                m.which_mods = IMModsWhich::from(WHICH_MODS_USE_LOCKED);
                m.mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_M3 as u16);
                m.real_mods = x11rb_protocol::protocol::xproto::ModMask::from(MOD_M3 as u16);
            }
            3 => {
                m.which_groups = IMGroupsWhich::from(WHICH_GROUPS_USE_EFFECTIVE);
                m.groups = SetOfGroup::from(GROUPS_NON_BASE);
            }
            _ => {}
        }
        maps.push(m);
    }
    let n_indicators = maps.len() as u8;

    serialize_var_reply(
        &GetIndicatorMapReply {
            device_id: device_id_byte,
            sequence: seq,
            length: 0,
            which,
            real_indicators: which,
            n_indicators,
            maps,
        },
        state.byte_order(),
    )
}

/// Handle SetIndicatorMap (minor opcode 14).
pub(crate) fn handle_set_indicator_map(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 12 {
        let msb = state.msb_first;
        let which = read_u32(data, 8, msb);
        let mut off = 16;
        for bit in 0..32u32 {
            if which & (1 << bit) == 0 {
                continue;
            }
            if off + INDICATOR_MAP_WIRE_SIZE > data.len() {
                break;
            }
            while state.xkb_indicator_maps.len() <= bit as usize {
                state
                    .xkb_indicator_maps
                    .push(super::super::super::client::XkbIndicatorMap::default());
            }
            let ctrls_val = read_u32(data, off + SET_INDICATOR_CTRLS_OFFSET, msb);
            let map = &mut state.xkb_indicator_maps[bit as usize];
            // Inline parsing matches XkbIndicatorMapWireDesc layout:
            //   flags(0) whichGroups(1) groups(2) whichMods(3) mods(4) realMods(5)
            //   vmods(6,7) ctrls(8,9,10,11)
            map.which_groups = data[off + 1];
            map.groups = data[off + 2];
            map.which_mods = data[off + 3];
            map.mods = data[off + 4];
            map.ctrls = ctrls_val;
            off += INDICATOR_MAP_WIRE_SIZE;
        }
    }
    Vec::new()
}

/// Handle GetNamedIndicator (minor opcode 15).
pub(crate) fn handle_get_named_indicator(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let indicator_atom = if data.len() >= 12 {
        state.read_u32(data, 8)
    } else {
        0
    };

    let eff_mods = state.xkb_state.effective_mods();
    let on = if indicator_atom == 0 {
        false
    } else {
        let name = state
            .atoms
            .lock()
            .unwrap()
            .get_name(indicator_atom)
            .map(|s| s.to_string());
        match name.as_deref() {
            Some("Caps Lock") => eff_mods & MOD_LOCK != 0,
            Some("Num Lock") => eff_mods & MOD_M2 != 0,
            Some("Scroll Lock") => eff_mods & MOD_M3 != 0,
            Some("Group 2") => state.xkb_state.effective_group() >= 1,
            _ => false,
        }
    };

    serialize_reply(
        &GetNamedIndicatorReply {
            device_id: device_id_byte,
            sequence: seq,
            length: 0,
            indicator: indicator_atom,
            found: true,
            on,
            real_indicator: false,
            ndx: 0,
            map_flags: IMFlag::from(0u8),
            map_which_groups: IMGroupsWhich::from(0u8),
            map_groups: SetOfGroups::from(0u8),
            map_which_mods: IMModsWhich::from(0u8),
            map_mods: x11rb_protocol::protocol::xproto::ModMask::from(0u16),
            map_real_mods: x11rb_protocol::protocol::xproto::ModMask::from(0u16),
            map_vmod: VMod::from(0u16),
            map_ctrls: x11rb_protocol::protocol::xkb::BoolCtrl::from(0u32),
            supported: true,
        },
        state.byte_order(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb_protocol::protocol::xproto::ModMask;

    #[test]
    fn mod_constants_match_x11rb() {
        assert_eq!(MOD_LOCK, u16::from(ModMask::LOCK) as u8);
        assert_eq!(MOD_M2, u16::from(ModMask::M2) as u8);
        assert_eq!(MOD_M3, u16::from(ModMask::M3) as u8);
    }
}
