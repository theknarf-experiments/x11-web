//! XKB names operations: GetNames, SetNames.

use super::super::super::client::ClientState;
use super::{MAX_KEY_CODE, MIN_KEY_CODE, N_KEYS};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_u16_bo as read_u16, read_u32_bo as read_u32};
use crate::xserver::reply::serialize_var_reply;
use tracing::debug;
use x11rb_protocol::protocol::xkb::{
    GetNamesReply, GetNamesValueList, GetNamesValueListKTLevelNames, KeyAlias, KeyName, SetOfGroup,
    VMod,
};

/// Build an XKB GetNames reply.
///
/// The request `which` bitmask (from the wire) controls which name
/// sections are returned. The XKB protocol specifies fixed bit
/// assignments (`xkbproto.h` `XkbXxxNameMask`); the codegen-emitted
/// `GetNamesValueList` Serialize impl emits the sections in the same
/// bit order via switch_expr() over the populated `Option<…>` fields.
pub(crate) fn build_xkb_get_names_reply(
    state: &mut ClientState,
    seq: u16,
    device_id: u8,
    req_which: u32,
) -> Vec<u8> {
    const N_TYPES: u8 = 4;
    const LEVELS_PER_TYPE: [u8; 4] = [1, 2, 2, 2];

    // Virtual mods mask must match GetMap: bits 0, 1, 3.
    let virtual_mods: u16 = (1 << 0) | (1 << 1) | (1 << 3);

    let (keycodes_atom, geometry_atom, symbols_atom, types_atom, compat_atom) = {
        let mut atoms = state.atoms.lock().unwrap();
        let kc = state
            .xkb_names_atoms
            .get(&0)
            .copied()
            .unwrap_or_else(|| atoms.intern("evdev", false));
        let geo = state
            .xkb_names_atoms
            .get(&1)
            .copied()
            .unwrap_or_else(|| atoms.intern("pc(pc105)", false));
        let sym = state
            .xkb_names_atoms
            .get(&2)
            .copied()
            .unwrap_or_else(|| atoms.intern("pc+us", false));
        let types = state
            .xkb_names_atoms
            .get(&3)
            .copied()
            .unwrap_or_else(|| atoms.intern("complete", false));
        let compat = state
            .xkb_names_atoms
            .get(&4)
            .copied()
            .unwrap_or_else(|| atoms.intern("complete", false));
        (kc, geo, sym, types, compat)
    };
    // PhysSymbolsName mirrors SymbolsName; the protocol slot must be
    // a non-zero atom for Xlib's XkbGetNames parser to succeed.
    let phys_symbols_atom = symbols_atom;

    let type_name_strs = ["ONE_LEVEL", "TWO_LEVEL", "ALPHABETIC", "KEYPAD"];
    let level_name_strs = [
        &["Any"][..],
        &["Base", "Shift"],
        &["Base", "Caps"],
        &["Base", "Number"],
    ];
    let vmod_name_strs = ["Alt", "NumLock", "Super"];

    let (type_atoms, level_atoms_flat, vmod_atoms) = {
        let mut atoms = state.atoms.lock().unwrap();
        let ta: Vec<u32> = type_name_strs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                state
                    .xkb_type_names
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| atoms.intern(s, false))
            })
            .collect();
        let mut flat: Vec<u32> = Vec::new();
        for (ti, names) in level_name_strs.iter().enumerate() {
            for (li, s) in names.iter().enumerate() {
                let a = state
                    .xkb_kt_level_names
                    .get(ti)
                    .and_then(|lv| lv.get(li))
                    .copied()
                    .unwrap_or_else(|| atoms.intern(s, false));
                flat.push(a);
            }
        }
        let va: Vec<u32> = vmod_name_strs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                state
                    .xkb_vmod_names
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| atoms.intern(s, false))
            })
            .collect();
        (ta, flat, va)
    };

    let num_groups = (1 + state.xkb_extra_groups.len() as u8).min(4);
    let group_name_strs_default = ["English (US)", "Group 2", "Group 3", "Group 4"];
    let group_atoms: Vec<u32> = {
        let mut atoms = state.atoms.lock().unwrap();
        (0..num_groups as usize)
            .map(|i| {
                state
                    .xkb_group_names
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| atoms.intern(group_name_strs_default[i], false))
            })
            .collect()
    };
    let group_names_present: u8 = (1u8 << num_groups) - 1;

    let indicator_name_strs = ["Caps Lock", "Num Lock", "Scroll Lock", "Group 2"];
    let indicator_atoms: Vec<u32> = {
        let mut atoms = state.atoms.lock().unwrap();
        indicator_name_strs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                state
                    .xkb_indicator_name_atoms
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| atoms.intern(s, false))
            })
            .collect()
    };
    let indicators_present: u32 = 0x0F;
    let total_levels: u8 = LEVELS_PER_TYPE.iter().sum();

    // Build the typed value list — each Option<…> presence corresponds to
    // one bit of `which`. The Serialize impl recomputes `which` from
    // `switch_expr()` and emits sections in fixed bit order.
    let key_names_vec: Vec<KeyName> = {
        let default_names = super::map::us_qwerty_key_names();
        (MIN_KEY_CODE..=MAX_KEY_CODE)
            .map(|kc| {
                let idx = (kc - MIN_KEY_CODE) as usize;
                let bytes = state
                    .xkb_key_names
                    .get(&kc)
                    .copied()
                    .unwrap_or_else(|| {
                        let mut arr = [0u8; 4];
                        arr.copy_from_slice(default_names[idx]);
                        arr
                    });
                KeyName { name: bytes }
            })
            .collect()
    };

    let value_list = GetNamesValueList {
        keycodes_name: (req_which & (1 << 0) != 0).then_some(keycodes_atom),
        geometry_name: (req_which & (1 << 1) != 0).then_some(geometry_atom),
        symbols_name: (req_which & (1 << 2) != 0).then_some(symbols_atom),
        phys_symbols_name: (req_which & (1 << 3) != 0).then_some(phys_symbols_atom),
        types_name: (req_which & (1 << 4) != 0).then_some(types_atom),
        compat_name: (req_which & (1 << 5) != 0).then_some(compat_atom),
        type_names: (req_which & (1 << 6) != 0).then(|| type_atoms.clone()),
        kt_level_names: (req_which & (1 << 7) != 0).then(|| GetNamesValueListKTLevelNames {
            n_levels_per_type: LEVELS_PER_TYPE.to_vec(),
            kt_level_names: level_atoms_flat,
        }),
        indicator_names: (req_which & (1 << 8) != 0).then(|| indicator_atoms.clone()),
        virtual_mod_names: (req_which & (1 << 10) != 0).then(|| vmod_atoms.clone()),
        groups: (req_which & (1 << 9) != 0).then(|| group_atoms.clone()),
        key_names: (req_which & (1 << 11) != 0).then_some(key_names_vec),
        key_aliases: (req_which & (1 << 12) != 0).then(|| {
            state
                .xkb_key_aliases
                .iter()
                .map(|(alias, real)| KeyAlias {
                    real: *real,
                    alias: *alias,
                })
                .collect()
        }),
        radio_group_names: None,
    };

    let n_key_aliases = value_list.key_aliases.as_ref().map_or(0, |v| v.len()) as u8;

    serialize_var_reply(
        &GetNamesReply {
            device_id,
            sequence: seq,
            length: 0,
            min_key_code: MIN_KEY_CODE,
            max_key_code: MAX_KEY_CODE,
            n_types: N_TYPES,
            group_names: SetOfGroup::from(group_names_present),
            virtual_mods: VMod::from(virtual_mods),
            first_key: MIN_KEY_CODE,
            n_keys: N_KEYS as u8,
            indicators: indicators_present,
            n_radio_groups: 0,
            n_key_aliases,
            n_kt_levels: total_levels as u16,
            value_list,
        },
        state.byte_order(),
    )
}

/// Handle XKB SetNames request (minor opcode 18).
///
/// Per the XKB spec, SetNames allows clients to assign symbolic atom names
/// to keycodes, types, indicators, groups, virtual modifiers, key aliases,
/// and the overall keyboard description.
pub(crate) fn handle_xkb_set_names(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 136, data[1] as u16, state.msb_first);

    let msb = state.msb_first;
    let which = read_u32(data, 8, msb);
    // Byte 12: firstType, 13: nTypes, 14: firstKTLevelName, 15: nKTLevelNames
    let first_type = if data.len() > 12 { data[12] } else { 0 };
    let n_types = if data.len() > 13 { data[13] } else { 0 };
    let _first_kt = if data.len() > 14 { data[14] } else { 0 };
    let n_kt_levels = if data.len() > 15 { data[15] } else { 0 };
    // 16-19: indicators (CARD32)
    let indicators = if data.len() >= 20 {
        read_u32(data, 16, msb)
    } else {
        0
    };
    // 20: groupNames (CARD8), 21: nRadioGroups, 22: firstKey, 23: nKeys
    let group_names_mask = if data.len() > 20 { data[20] } else { 0 };
    let _n_radio = if data.len() > 21 { data[21] } else { 0 };
    let first_key = if data.len() > 22 {
        data[22]
    } else {
        MIN_KEY_CODE
    };
    let n_keys = if data.len() > 23 { data[23] } else { 0 };
    // 24-25: nKeyAliases (CARD16)
    let n_key_aliases = if data.len() >= 26 {
        read_u16(data, 24, msb)
    } else {
        0
    };
    // 28-29: totalKTLevelNames (CARD16)

    let mut offset = 32;

    // Bit 0: Keycodes name atom
    if which & (1 << 0) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(0, atom);
        offset += 4;
    }
    // Bit 1: Geometry name atom
    if which & (1 << 1) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(1, atom);
        offset += 4;
    }
    // Bit 2: Symbols name atom
    if which & (1 << 2) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(2, atom);
        offset += 4;
    }
    // Bit 3: PhysSymbolsName (1 atom)
    if which & (1 << 3) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(3, atom);
        offset += 4;
    }
    // Bit 4: TypesName (1 atom)
    if which & (1 << 4) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(4, atom);
        offset += 4;
    }
    // Bit 5: CompatName (1 atom)
    if which & (1 << 5) != 0 && offset + 4 <= data.len() {
        let atom = read_u32(data, offset, msb);
        state.xkb_names_atoms.insert(5, atom);
        offset += 4;
    }
    // Bit 6: KeyTypeNames
    if which & (1 << 6) != 0 {
        state.xkb_type_names.clear();
        for _ in 0..n_types {
            if offset + 4 <= data.len() {
                let atom = read_u32(data, offset, msb);
                state.xkb_type_names.push(atom);
                offset += 4;
            }
        }
    }
    // Bit 7: KTLevelNames
    if which & (1 << 7) != 0 {
        state.xkb_kt_level_names.clear();
        let mut levels_per_type = Vec::new();
        for i in 0..n_kt_levels as usize {
            if offset + i < data.len() {
                levels_per_type.push(data[offset + i]);
            }
        }
        offset += n_kt_levels as usize;
        offset = crate::xserver::core::align_to_4(offset);
        for &n_levels in &levels_per_type {
            let mut level_atoms = Vec::new();
            for _ in 0..n_levels {
                if offset + 4 <= data.len() {
                    let atom = read_u32(data, offset, msb);
                    level_atoms.push(atom);
                    offset += 4;
                }
            }
            state.xkb_kt_level_names.push(level_atoms);
        }
    }
    // Bit 8: IndicatorNames (one atom per set bit in `indicators`)
    if which & (1 << 8) != 0 {
        let n_indicators = indicators.count_ones() as usize;
        state.xkb_indicator_name_atoms.clear();
        for _ in 0..n_indicators {
            if offset + 4 <= data.len() {
                let atom = read_u32(data, offset, msb);
                state.xkb_indicator_name_atoms.push(atom);
                offset += 4;
            }
        }
    }
    // Bit 9: GroupNames (one atom per set bit in groupNames mask)
    if which & (1 << 9) != 0 {
        let n_groups = group_names_mask.count_ones() as usize;
        state.xkb_group_names.clear();
        for _ in 0..n_groups {
            if offset + 4 <= data.len() {
                let atom = read_u32(data, offset, msb);
                state.xkb_group_names.push(atom);
                offset += 4;
            }
        }
    }
    // Bit 10: VirtualModNames (one atom per set bit in virtualMods)
    if which & (1 << 10) != 0 {
        let vmods: u16 = if data.len() >= 32 {
            read_u16(data, 30, msb)
        } else {
            0
        };
        let n_vmods = vmods.count_ones() as usize;
        state.xkb_vmod_names.clear();
        for _ in 0..n_vmods {
            if offset + 4 <= data.len() {
                let atom = read_u32(data, offset, msb);
                state.xkb_vmod_names.push(atom);
                offset += 4;
            }
        }
    }
    // Bit 11: KeyNames (4-byte names per key)
    if which & (1 << 11) != 0 {
        for i in 0..n_keys as usize {
            if offset + 4 <= data.len() {
                let kc = first_key.wrapping_add(i as u8);
                let mut name = [0u8; 4];
                name.copy_from_slice(&data[offset..offset + 4]);
                state.xkb_key_names.insert(kc, name);
                offset += 4;
            }
        }
    }
    // Bit 12: KeyAliases (pairs of 4-byte names)
    if which & (1 << 12) != 0 {
        state.xkb_key_aliases.clear();
        for _ in 0..n_key_aliases {
            if offset + 8 <= data.len() {
                let mut alias = [0u8; 4];
                let mut real = [0u8; 4];
                alias.copy_from_slice(&data[offset..offset + 4]);
                real.copy_from_slice(&data[offset + 4..offset + 8]);
                state.xkb_key_aliases.push((alias, real));
                offset += 8;
            }
        }
    }

    let _ = first_type;
    debug!("XKB SetNames: which=0x{which:08x} processed {offset} bytes");
    Vec::new()
}
