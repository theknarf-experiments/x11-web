//! XKB indicator operations: GetIndicatorState, GetIndicatorMap, SetIndicatorMap, GetNamedIndicator.

use super::super::super::client::ClientState;
use crate::xserver::core::{read_u32_bo as read_u32};

/// Handle GetIndicatorState (minor opcode 12).
pub(crate) fn handle_get_indicator_state(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let mut ind_state: u32 = 0;
    let eff_mods = state.xkb_state.effective_mods();
    // Indicator 0: Caps Lock (Lock modifier = bit 1)
    if eff_mods & 0x02 != 0 { ind_state |= 1 << 0; }
    // Indicator 1: Num Lock (Mod2 = bit 4)
    if eff_mods & 0x10 != 0 { ind_state |= 1 << 1; }
    // Indicator 2: Scroll Lock (Mod3 = bit 5) - not common but supported
    if eff_mods & 0x20 != 0 { ind_state |= 1 << 2; }
    // Indicator 3: Group (lit when group > 0)
    if state.xkb_state.effective_group() > 0 { ind_state |= 1 << 3; }
    state.xkb_indicators = ind_state;

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = device_id_byte;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, ind_state);
    reply.to_vec()
}

/// Handle GetIndicatorMap (minor opcode 13).
pub(crate) fn handle_get_indicator_map(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    // which = bitmask from request (bytes 8-11), return all requested
    let which: u32 = if data.len() >= 12 { state.read_u32(data, 8) } else { 0x0F };
    let n_indicators = which.count_ones() as usize;
    let map_size = 12; // XkbIndicatorMapWireDesc = 12 bytes
    let body_len = n_indicators * map_size;
    let mut reply = vec![0u8; 32 + body_len];
    reply[0] = 1;
    reply[1] = device_id_byte;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (body_len / 4) as u32);
    state.write_u32(&mut reply, 8, which); // which indicators
    // Indicator 12-byte maps: flags(1), whichGroups(1), groups(1), whichMods(1),
    //                         mods(1), realMods(1), vmods(2), ctrls(4)
    let mut off = 32;
    for bit in 0..32u32 {
        if which & (1 << bit) == 0 { continue; }
        if off + map_size > reply.len() { break; }
        match bit {
            0 => {
                // Caps Lock: driven by Lock modifier (0x02)
                reply[off] = 0; // flags
                reply[off + 1] = 0; // whichGroups
                reply[off + 2] = 0; // groups
                reply[off + 3] = 0x04; // whichMods: UseLocked
                reply[off + 4] = 0x02; // mods: Lock
                reply[off + 5] = 0x02; // realMods: Lock
            }
            1 => {
                // Num Lock: driven by Mod2 (0x10)
                reply[off + 3] = 0x04; // whichMods: UseLocked
                reply[off + 4] = 0x10; // mods: Mod2
                reply[off + 5] = 0x10; // realMods: Mod2
            }
            2 => {
                // Scroll Lock: driven by Mod3 (0x20)
                reply[off + 3] = 0x04;
                reply[off + 4] = 0x20;
                reply[off + 5] = 0x20;
            }
            3 => {
                // Group indicator: driven by effective group != 0
                reply[off + 1] = 0x80; // whichGroups: UseEffective
                reply[off + 2] = 0x0E; // groups: 1|2|3 (lit when not group 0)
            }
            _ => {}
        }
        off += map_size;
    }
    reply
}

/// Handle SetIndicatorMap (minor opcode 14).
pub(crate) fn handle_set_indicator_map(
    state: &mut ClientState,
    data: &[u8],
) -> Vec<u8> {
    if data.len() >= 12 {
        let msb = state.msb_first;
        let which = read_u32(data, 8, msb);
        let mut off = 16;
        for bit in 0..32u32 {
            if which & (1 << bit) == 0 { continue; }
            if off + 12 > data.len() { break; }
            while state.xkb_indicator_maps.len() <= bit as usize {
                state.xkb_indicator_maps.push(super::super::super::client::XkbIndicatorMap::default());
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
    let indicator_atom = if data.len() >= 12 { state.read_u32(data, 8) } else { 0 };
    let mut reply = vec![0u8; 32];
    reply[0] = 1;
    reply[1] = device_id_byte;
    state.write_u16(&mut reply, 2, seq);
    // Return the indicator atom and current state
    state.write_u32(&mut reply, 8, indicator_atom);
    // found = true if it's one of our known indicators
    reply[12] = 1; // found
    // on = check current indicator state
    let eff_mods = state.xkb_state.effective_mods();
    let on = if indicator_atom == 0 {
        false
    } else {
        // Check by looking up the atom name for well-known indicators
        let name = state.atoms.lock().unwrap().get_name(indicator_atom)
            .map(|s| s.to_string());
        match name.as_deref() {
            Some("Caps Lock") => eff_mods & 0x02 != 0,
            Some("Num Lock") => eff_mods & 0x10 != 0,
            Some("Scroll Lock") => eff_mods & 0x20 != 0,
            Some("Group 2") => state.xkb_state.effective_group() >= 1,
            _ => false,
        }
    };
    reply[13] = on as u8;
    reply.to_vec()
}
