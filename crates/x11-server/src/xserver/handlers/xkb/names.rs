//! XKB names operations: GetNames, SetNames.

use super::super::super::client::ClientState;
use super::{MAX_KEY_CODE, MIN_KEY_CODE, N_KEYS};
use crate::xserver::core::require_len;
use crate::xserver::core::{read_u16_bo as read_u16, read_u32_bo as read_u32};
use crate::xserver::reply::ReplyBuf;
use tracing::debug;

/// Build an XKB GetNames reply.
///
/// The request `which` bitmask (from the wire) controls which name
/// sections are returned. Bit assignments are fixed by the XKB
/// protocol (see `xkbproto.h` `XkbXxxNameMask`):
///
///   bit  mask    section
///    0   0x0001  KeycodesName       8   0x0100  IndicatorNames
///    1   0x0002  GeometryName       9   0x0200  GroupNames
///    2   0x0004  SymbolsName       10   0x0400  VirtualModNames
///    3   0x0008  PhysSymbolsName   11   0x0800  KeyNames
///    4   0x0010  TypesName         12   0x1000  KeyAliases
///    5   0x0020  CompatName        13   0x2000  RGNames
///    6   0x0040  KeyTypeNames
///    7   0x0080  KTLevelNames
///
/// The data section appears in this exact bit order; reordering or
/// renumbering breaks Xlib (`xkbcomp` reports "Could not load names"
/// followed by `BadName` on `XkbGetGeometry` because the geometry
/// atom slot is parsed from the wrong wire offset).
pub(crate) fn build_xkb_get_names_reply(
    state: &mut ClientState,
    seq: u16,
    device_id: u8,
    req_which: u32,
) -> Vec<u8> {
    const KEY_NAME_LEN: usize = 4;
    const N_TYPES: u8 = 4;
    const LEVELS_PER_TYPE: [u8; 4] = [1, 2, 2, 2];

    // Virtual mods mask must match GetMap: bits 0, 1, 3.
    let virtual_mods: u16 = (1 << 0) | (1 << 1) | (1 << 3);
    let _n_vmods = virtual_mods.count_ones() as u8; // 3

    // Intern all name atoms we may need up front.
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
    let vmod_name_strs = ["Alt", "NumLock", "Super"]; // bits 0, 1, 3

    // Intern type/level/vmod names.
    let (type_atoms, level_atoms, vmod_atoms) = {
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
        let mut la: Vec<Vec<u32>> = Vec::new();
        for (ti, names) in level_name_strs.iter().enumerate() {
            let mut v = Vec::new();
            for (li, s) in names.iter().enumerate() {
                let a = state
                    .xkb_kt_level_names
                    .get(ti)
                    .and_then(|lv| lv.get(li))
                    .copied()
                    .unwrap_or_else(|| atoms.intern(s, false));
                v.push(a);
            }
            la.push(v);
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
        (ta, la, va)
    };

    let num_groups = (1 + state.xkb_extra_groups.len() as u8).min(4);
    let group_name_strs_default = ["English (US)", "Group 2", "Group 3", "Group 4"];
    let group_atoms = {
        let mut atoms = state.atoms.lock().unwrap();
        (0..num_groups as usize)
            .map(|i| {
                state
                    .xkb_group_names
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| atoms.intern(group_name_strs_default[i], false))
            })
            .collect::<Vec<_>>()
    };
    let group_names_present: u8 = (1u8 << num_groups) - 1;

    let indicator_name_strs = ["Caps Lock", "Num Lock", "Scroll Lock", "Group 2"];
    let indicator_atoms = {
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
            .collect::<Vec<u32>>()
    };
    let indicators_present: u32 = 0x0F; // 4 indicators (bits 0-3)

    // Track which sections we actually emit.
    let mut which: u32 = 0;
    let mut data = Vec::new();

    // Helper: append a 4-byte atom.
    macro_rules! emit_atom {
        ($atom:expr) => {{
            let off = data.len();
            data.extend_from_slice(&[0u8; 4]);
            state.write_u32(&mut data, off, $atom);
        }};
    }

    // Bit 0: KeycodesName (1 ATOM)
    if req_which & (1 << 0) != 0 {
        which |= 1 << 0;
        emit_atom!(keycodes_atom);
    }

    // Bit 1: GeometryName (1 ATOM)
    if req_which & (1 << 1) != 0 {
        which |= 1 << 1;
        emit_atom!(geometry_atom);
    }

    // Bit 2: SymbolsName (1 ATOM)
    if req_which & (1 << 2) != 0 {
        which |= 1 << 2;
        emit_atom!(symbols_atom);
    }

    // Bit 3: PhysSymbolsName (1 ATOM)
    if req_which & (1 << 3) != 0 {
        which |= 1 << 3;
        emit_atom!(phys_symbols_atom);
    }

    // Bit 4: TypesName (1 ATOM)
    if req_which & (1 << 4) != 0 {
        which |= 1 << 4;
        emit_atom!(types_atom);
    }

    // Bit 5: CompatName (1 ATOM)
    if req_which & (1 << 5) != 0 {
        which |= 1 << 5;
        emit_atom!(compat_atom);
    }

    // Bit 6: KeyTypeNames (nTypes ATOMs)
    if req_which & (1 << 6) != 0 {
        which |= 1 << 6;
        for &atom in &type_atoms {
            emit_atom!(atom);
        }
    }

    // Bit 7: KTLevelNames (nLevelsPerType bytes, padded, then atoms)
    let total_levels: u8 = LEVELS_PER_TYPE.iter().sum();
    if req_which & (1 << 7) != 0 {
        which |= 1 << 7;
        for &n in LEVELS_PER_TYPE.iter() {
            data.push(n);
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        for type_levels in &level_atoms {
            for &atom in type_levels {
                emit_atom!(atom);
            }
        }
    }

    // Bit 8: IndicatorNames (one ATOM per set bit in indicators)
    if req_which & (1 << 8) != 0 {
        which |= 1 << 8;
        for &atom in &indicator_atoms {
            emit_atom!(atom);
        }
    }

    // Bit 9: GroupNames (one ATOM per set bit in groupNames)
    if req_which & (1 << 9) != 0 {
        which |= 1 << 9;
        for &atom in &group_atoms {
            emit_atom!(atom);
        }
    }

    // Bit 10: VirtualModNames (one ATOM per set bit in virtualMods)
    if req_which & (1 << 10) != 0 {
        which |= 1 << 10;
        for &atom in &vmod_atoms {
            emit_atom!(atom);
        }
    }

    // Bit 11: KeyNames (N_KEYS * 4 bytes)
    if req_which & (1 << 11) != 0 {
        which |= 1 << 11;
        let key_names = super::map::us_qwerty_key_names();
        for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
            let idx = (kc - MIN_KEY_CODE) as usize;
            if let Some(custom) = state.xkb_key_names.get(&kc) {
                data.extend_from_slice(custom);
            } else {
                data.extend_from_slice(key_names[idx]);
            }
        }
        debug_assert_eq!(N_KEYS * KEY_NAME_LEN, 992);
        while data.len() % 4 != 0 {
            data.push(0);
        }
    }

    // Bit 12: KeyAliases (pairs of 4-byte names)
    let n_key_aliases: u16 = if req_which & (1 << 12) != 0 {
        which |= 1 << 12;
        let aliases = &state.xkb_key_aliases;
        for (alias, real) in aliases {
            data.extend_from_slice(alias);
            data.extend_from_slice(real);
        }
        aliases.len() as u16
    } else {
        0
    };

    let mut reply = ReplyBuf::with_extra(seq, data.len(), state.msb_first)
        .set_data_byte(device_id)
        .set_u32(8, which)
        .set_u16(16, virtual_mods)
        .set_u32(20, indicators_present)
        .set_u16(26, u16::from(total_levels)); // nKTLevels
    {
        let buf = reply.buf_mut();
        buf[12] = MIN_KEY_CODE;
        buf[13] = MAX_KEY_CODE;
        buf[14] = N_TYPES;
        buf[15] = group_names_present;
        buf[18] = MIN_KEY_CODE; // firstKey
        buf[19] = N_KEYS as u8; // nKeys
        buf[24] = 0; // nRadioGroups
        buf[25] = n_key_aliases as u8;
        // 28-31: pad
        buf[32..32 + data.len()].copy_from_slice(&data);
    }
    reply.build()
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
        offset = crate::xserver::core::align_to_4(offset );
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
        // virtualMods bitmask is at bytes 30-31 of the request
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
