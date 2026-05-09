//! XKB indicator operations: GetIndicatorState, GetIndicatorMap, SetIndicatorMap, GetNamedIndicator.

use super::super::super::client::ClientState;
use crate::xserver::core::read_u32_bo as read_u32;
use crate::xserver::reply::ReplyBuf;

/// Real-modifier mask bits as `u8` aliases of `x11rb::xproto::ModMask`.
/// Used by the XKB indicator state / map handlers, which need raw bytes
/// rather than the `ModMask` wrapper type. Verified by a test below.
const MOD_LOCK: u8 = 1 << 1; // Caps Lock
const MOD_M2: u8 = 1 << 4; // Num Lock
const MOD_M3: u8 = 1 << 5; // Scroll Lock

/// XkbIM_UseLocked flag for the IndicatorMap whichMods field.
const WHICH_MODS_USE_LOCKED: u8 = 0x04;

/// Handle GetIndicatorState (minor opcode 12).
pub(crate) fn handle_get_indicator_state(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let mut ind_state: u32 = 0;
    let eff_mods = state.xkb_state.effective_mods();
    // Indicator 0: Caps Lock
    if eff_mods & MOD_LOCK != 0 {
        ind_state |= 1 << 0;
    }
    // Indicator 1: Num Lock
    if eff_mods & MOD_M2 != 0 {
        ind_state |= 1 << 1;
    }
    // Indicator 2: Scroll Lock — not common but supported
    if eff_mods & MOD_M3 != 0 {
        ind_state |= 1 << 2;
    }
    // Indicator 3: Group (lit when group > 0)
    if state.xkb_state.effective_group() > 0 {
        ind_state |= 1 << 3;
    }
    state.xkb_indicators = ind_state;

    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(device_id_byte)
        .set_u32(8, ind_state)
        .build()
}

/// Handle GetIndicatorMap (minor opcode 13).
pub(crate) fn handle_get_indicator_map(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    // which = bitmask from request (bytes 8-11), return all requested
    let which: u32 = if data.len() >= 12 {
        state.read_u32(data, 8)
    } else {
        0x0F
    };
    let n_indicators = which.count_ones() as usize;
    let map_size = 12; // XkbIndicatorMapWireDesc = 12 bytes
    let body_len = n_indicators * map_size;
    let mut reply = ReplyBuf::with_extra(seq, body_len, state.msb_first)
        .set_data_byte(device_id_byte)
        .set_u32(8, which); // which indicators
                            // Indicator 12-byte maps: flags(1), whichGroups(1), groups(1), whichMods(1),
                            //                         mods(1), realMods(1), vmods(2), ctrls(4)
    let mut off = 32;
    for bit in 0..32u32 {
        if which & (1 << bit) == 0 {
            continue;
        }
        if off + map_size > reply.buf_mut().len() {
            break;
        }
        match bit {
            0 => {
                // Caps Lock: driven by Lock modifier
                reply.buf_mut()[off] = 0; // flags
                reply.buf_mut()[off + 1] = 0; // whichGroups
                reply.buf_mut()[off + 2] = 0; // groups
                reply.buf_mut()[off + 3] = WHICH_MODS_USE_LOCKED;
                reply.buf_mut()[off + 4] = MOD_LOCK; // mods
                reply.buf_mut()[off + 5] = MOD_LOCK; // realMods
            }
            1 => {
                // Num Lock: driven by Mod2
                reply.buf_mut()[off + 3] = WHICH_MODS_USE_LOCKED;
                reply.buf_mut()[off + 4] = MOD_M2;
                reply.buf_mut()[off + 5] = MOD_M2;
            }
            2 => {
                // Scroll Lock: driven by Mod3
                reply.buf_mut()[off + 3] = WHICH_MODS_USE_LOCKED;
                reply.buf_mut()[off + 4] = MOD_M3;
                reply.buf_mut()[off + 5] = MOD_M3;
            }
            3 => {
                // Group indicator: driven by effective group != 0
                reply.buf_mut()[off + 1] = 0x80; // whichGroups: UseEffective
                reply.buf_mut()[off + 2] = 0x0E; // groups: 1|2|3 (lit when not group 0)
            }
            _ => {}
        }
        off += map_size;
    }
    reply.build()
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
            if off + 12 > data.len() {
                break;
            }
            while state.xkb_indicator_maps.len() <= bit as usize {
                state
                    .xkb_indicator_maps
                    .push(super::super::super::client::XkbIndicatorMap::default());
            }
            let ctrls_val = read_u32(data, off + 8, msb);
            let map = &mut state.xkb_indicator_maps[bit as usize];
            map.which_groups = data[off + 1];
            map.groups = data[off + 2];
            map.which_mods = data[off + 3];
            map.mods = data[off + 4];
            map.ctrls = ctrls_val;
            off += 12;
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
    let mut reply = ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(device_id_byte)
        .set_u32(8, indicator_atom)
        .set_u8(12, 1); // found
                        // on = check current indicator state
    let eff_mods = state.xkb_state.effective_mods();
    let on = if indicator_atom == 0 {
        false
    } else {
        // Check by looking up the atom name for well-known indicators
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
    reply = reply.set_u8(13, on as u8);
    reply.build()
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
