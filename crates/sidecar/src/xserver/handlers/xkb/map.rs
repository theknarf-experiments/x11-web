//! XKB keymap operations: GetMap, SetMap, GetKbdByName, key name/sym tables.

use super::super::super::client::{is_lock_key, ClientState};
use super::{
    MIN_KEY_CODE, MAX_KEY_CODE, N_KEYS, MAX_GROUPS,
    SA_SET_MODS, SA_LOCK_MODS, SA_SET_GROUP, SA_LOCK_GROUP,
    KB_LOCK, MODIFIER_KEYS,
};
use crate::xserver::core::require_len;
use tracing::debug;

/// Build an XKB GetMap reply with full sections: KeyTypes, KeySyms,
/// KeyActions, KeyBehaviors, VirtualMods, ExplicitComponents,
/// ModifierMap, and VirtualModMap.
pub(crate) fn build_xkb_get_map_reply(state: &mut ClientState, seq: u16) -> Vec<u8> {
    // ----- Build the variable-length sections -----
    let mut data = Vec::new();

    // How many groups are active? At least 1 (US-QWERTY). Additional groups
    // come from state.xkb_extra_groups (populated by SetMap or layout config).
    let num_groups = (1 + state.xkb_extra_groups.len() as u8).min(MAX_GROUPS);

    // =====================================================================
    // 1. KeyTypes: 4 standard XKB types
    // =====================================================================
    let n_types = 4u8;

    // type 0 — ONE_LEVEL: numLevels=1, no map entries.
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, // mask, realMods, vmods (16-bit)
        0x01, // numLevels
        0x00, // nMapEntries
        0x00, 0x00, // hasPreserve, pad
    ]);

    // type 1 — TWO_LEVEL: Shift mask, 1 entry mapping Shift -> level 1.
    data.extend_from_slice(&[
        0x01, 0x01, 0x00, 0x00, // mask=Shift, realMods=Shift, vmods=0
        0x02, // numLevels
        0x01, // nMapEntries
        0x00, 0x00, // hasPreserve, pad
    ]);
    // map entry: active=1, mask=Shift, level=1, realMods=Shift
    data.extend_from_slice(&[0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);

    // type 2 — ALPHABETIC: Shift+Lock, 2 entries.
    data.extend_from_slice(&[
        0x03, 0x03, 0x00, 0x00, // mask=Shift|Lock, realMods=Shift|Lock
        0x02, // numLevels
        0x02, // nMapEntries
        0x00, 0x00,
    ]);
    data.extend_from_slice(&[0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x01, 0x02, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00]);

    // type 3 — KEYPAD: NumLock (Mod2 = 0x10), 1 entry.
    data.extend_from_slice(&[
        0x10, 0x10, 0x00, 0x00, // mask=Mod2, realMods=Mod2
        0x02, // numLevels
        0x01, // nMapEntries
        0x00, 0x00,
    ]);
    data.extend_from_slice(&[0x01, 0x10, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00]);

    // =====================================================================
    // 2. KeySyms: one XkbSymMapWireDesc per key, with multi-group support
    // =====================================================================
    let mut total_syms_count: u16 = 0;
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        let (normal, shifted) = super::super::resolve_keysym(kc, &state.custom_keymap);
        let two_level = normal != 0 && shifted != 0 && normal != shifted;
        let width: u8 = if two_level { 2 } else { 1 };

        // Collect keysyms for each group.
        // Group 0: custom keymap (from ChangeKeyboardMapping/SetMap) or built-in US-QWERTY.
        // Groups 1+: from xkb_extra_groups tables (fallback to group 0 if missing).
        let mut group_syms: Vec<Vec<u32>> = Vec::with_capacity(num_groups as usize);

        // Group 0
        if two_level {
            group_syms.push(vec![normal, shifted]);
        } else {
            group_syms.push(vec![normal]);
        }

        // Additional groups
        for gi in 0..num_groups.saturating_sub(1) {
            let gi = gi as usize;
            if gi < state.xkb_extra_groups.len() {
                let extra = &state.xkb_extra_groups[gi];
                if let Some(syms) = extra.get(&kc) {
                    // Pad or truncate to `width` entries
                    let mut gs = syms.clone();
                    gs.resize(width as usize, 0);
                    group_syms.push(gs);
                } else {
                    // Fallback: duplicate group 0
                    group_syms.push(group_syms[0].clone());
                }
            } else {
                group_syms.push(group_syms[0].clone());
            }
        }

        let actual_groups = group_syms.len() as u8;
        let n_syms = (width as u16) * (actual_groups as u16);

        let off = data.len();
        // kt_index: one per group (up to 4), packed into 4 bytes
        let kt_idx = if two_level { 1u8 } else { 0u8 };
        data.extend_from_slice(&[kt_idx, kt_idx, kt_idx, kt_idx]);
        // groupInfo: low 4 bits = nGroups
        data.push(actual_groups);
        data.push(width);
        data.extend_from_slice(&[0u8; 2]); // nSyms placeholder
        state.write_u16(&mut data, off + 6, n_syms);

        // Emit keysyms: group0_level0, group0_level1, group1_level0, ...
        for gs in &group_syms {
            for &sym in gs {
                let sym_off = data.len();
                data.extend_from_slice(&[0u8; 4]);
                state.write_u32(&mut data, sym_off, sym);
            }
        }
        total_syms_count += n_syms;
    }

    // =====================================================================
    // 3. KeyActions: per-key nActs array + action records
    // =====================================================================
    // Build lookup of keycode -> action info.
    // Action data: (keycode, action_type, param) where param is mod_mask for
    // SA_SetMods/SA_LockMods or group_index for SA_SetGroup/SA_LockGroup.
    let mut key_actions: Vec<(u8, u8, u8)> = Vec::new();
    for &(kc, real_mod, _vmod) in MODIFIER_KEYS {
        if is_lock_key(kc) {
            key_actions.push((kc, SA_LOCK_MODS, real_mod));
        } else {
            key_actions.push((kc, SA_SET_MODS, real_mod));
        }
    }
    // If multiple groups are configured, add group-switch actions.
    // By convention: right Alt (108) can toggle groups when multi-layout is active.
    if num_groups > 1 {
        // Check if client has configured explicit group-switch keys via xkb_group_switch_keys;
        // otherwise don't override the default Alt_R binding.
        for &(kc, group_idx) in &state.xkb_group_switch_keys {
            // Remove any existing modifier action for this key
            key_actions.retain(|(k, _, _)| *k != kc);
            key_actions.push((kc, SA_LOCK_GROUP, group_idx));
        }
    }

    // Write per-key nActs array (1 byte per key)
    let mut total_actions: u16 = 0;
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        if key_actions.iter().any(|(k, _, _)| *k == kc) {
            data.push(1); // 1 action for this key
            total_actions += 1;
        } else {
            data.push(0); // no actions
        }
    }
    // Pad to 4-byte boundary
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // Write the action records (8 bytes each, XkbAnyAction)
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        if let Some(&(_, action_type, param)) = key_actions.iter().find(|(k, _, _)| *k == kc) {
            match action_type {
                SA_SET_GROUP | SA_LOCK_GROUP => {
                    // XkbGroupAction layout (8 bytes):
                    //   byte 0: type (SA_SetGroup or SA_LockGroup)
                    //   byte 1: flags (0 = absolute group)
                    //   byte 2: group (signed group index)
                    //   byte 3-7: pad
                    let mut action = [0u8; 8];
                    action[0] = action_type;
                    action[1] = 0; // flags: 0 = absolute
                    action[2] = param; // group index
                    data.extend_from_slice(&action);
                }
                _ => {
                    // XkbModAction layout (8 bytes):
                    //   byte 0: type (SA_SetMods or SA_LockMods)
                    //   byte 1: flags
                    //   byte 2: mask (real modifier mask)
                    //   byte 3: realMods
                    //   byte 4-5: vmods
                    //   byte 6-7: pad
                    let mut action = [0u8; 8];
                    action[0] = action_type;
                    action[1] = 0;
                    action[2] = param; // mod_mask
                    action[3] = param; // realMods
                    // vmods: set if this key has a virtual modifier
                    if let Some(&(_, _, vmod_idx)) = MODIFIER_KEYS.iter().find(|(k, _, _)| *k == kc) {
                        if vmod_idx != 0xFF {
                            let vmod_bits: u16 = 1 << vmod_idx;
                            data.push(action[0]);
                            data.push(action[1]);
                            data.push(action[2]);
                            data.push(action[3]);
                            let vmod_off = data.len();
                            data.extend_from_slice(&[0u8; 2]);
                            state.write_u16(&mut data, vmod_off, vmod_bits);
                            data.extend_from_slice(&[0u8; 2]); // pad
                            continue;
                        }
                    }
                    data.extend_from_slice(&action);
                }
            }
        }
    }

    // =====================================================================
    // 4. KeyBehaviors: XkbBehaviorWireDesc (4 bytes each, sparse)
    // =====================================================================
    // Only emit entries for lock keys (CapsLock, NumLock)
    let mut behavior_entries: Vec<(u8, u8)> = Vec::new(); // (keycode, behavior_type)
    for &kc in &[66u8, 77u8] { // CapsLock, NumLock
        behavior_entries.push((kc, KB_LOCK));
    }
    let n_key_behaviors = behavior_entries.len() as u8;
    let total_key_behaviors = n_key_behaviors;

    for &(kc, behavior) in &behavior_entries {
        // XkbBehaviorWireDesc: keycode(1) + type(1) + data(1) + pad(1)
        data.push(kc);
        data.push(behavior);
        data.push(0); // data
        data.push(0); // pad
    }

    // =====================================================================
    // 5. VirtualMods: per-vmod modifier mapping
    // =====================================================================
    // Virtual modifier mask: bits 0,1,3 = Alt(0), NumLock(1), Super(3)
    let virtual_mods: u16 = (1 << 0) | (1 << 1) | (1 << 3);
    // For each set bit in virtualMods, emit 1 byte (the real modifier mapping)
    // Bit 0 (Alt) → Mod1 = 0x08
    data.push(0x08);
    // Bit 1 (NumLock) → Mod2 = 0x10
    data.push(0x10);
    // Bit 3 (Super) → Mod4 = 0x40
    data.push(0x40);
    // Pad to 4-byte boundary
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // =====================================================================
    // 6. ExplicitComponents: XkbExplicitWireDesc (2 bytes each, sparse)
    // =====================================================================
    // Mark modifier keys with explicit interpretation flags
    // XkbExplicitWireDesc: keycode(1) + explicit(1)
    let mut explicit_entries: Vec<(u8, u8)> = Vec::new();
    for &(kc, _, _) in MODIFIER_KEYS {
        // XkbExplicitInterpretMask (0x10) | XkbExplicitAutoRepeatMask (0x20)
        explicit_entries.push((kc, 0x30));
    }
    let n_key_explicit = explicit_entries.len() as u8;
    let total_key_explicit = n_key_explicit;

    for &(kc, explicit) in &explicit_entries {
        data.push(kc);
        data.push(explicit);
    }
    // Pad to 4-byte boundary
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // =====================================================================
    // 7. ModifierMap: XkbKeyModMapWireDesc (2 bytes each, sparse)
    // =====================================================================
    // Map keycodes to real modifier bits
    let mut modmap_entries: Vec<(u8, u8)> = Vec::new(); // (keycode, mods)
    for &(kc, real_mod, _) in MODIFIER_KEYS {
        // Deduplicate: only add if not already present
        if !modmap_entries.iter().any(|(k, _)| *k == kc) {
            modmap_entries.push((kc, real_mod));
        }
    }
    let n_mod_map_keys = modmap_entries.len() as u8;
    let total_mod_map_keys = n_mod_map_keys;

    for &(kc, mods) in &modmap_entries {
        data.push(kc);
        data.push(mods);
    }
    // Pad to 4-byte boundary
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // =====================================================================
    // 8. VirtualModMap: XkbKeyVModMapWireDesc (4 bytes each, sparse)
    // =====================================================================
    let mut vmodmap_entries: Vec<(u8, u16)> = Vec::new(); // (keycode, vmod_mask)
    for &(kc, _, vmod_idx) in MODIFIER_KEYS {
        if vmod_idx != 0xFF {
            let mask = 1u16 << vmod_idx;
            if !vmodmap_entries.iter().any(|(k, _)| *k == kc) {
                vmodmap_entries.push((kc, mask));
            }
        }
    }
    let n_vmod_map_keys = vmodmap_entries.len() as u8;
    let total_vmod_map_keys = n_vmod_map_keys;

    for &(kc, vmods) in &vmodmap_entries {
        // XkbKeyVModMapWireDesc: keycode(1) + pad(1) + vmods(2)
        data.push(kc);
        data.push(0); // pad
        let off = data.len();
        data.extend_from_slice(&[0u8; 2]);
        state.write_u16(&mut data, off, vmods);
    }

    // Pad to 4-byte boundary
    while data.len() % 4 != 0 {
        data.push(0);
    }

    // ----- Header -----
    let total_len = 40 + data.len();
    let mut reply = vec![0u8; total_len];
    reply[0] = 1; // Reply
    reply[1] = 3; // deviceID (matches Xvfb's default core kbd)
    state.write_u16(&mut reply, 2, seq);
    let length_words = ((8 + data.len()) / 4) as u32;
    state.write_u32(&mut reply, 4, length_words);
    reply[10] = MIN_KEY_CODE;
    reply[11] = MAX_KEY_CODE;
    let present: u16 = 0x00ff;
    state.write_u16(&mut reply, 12, present);
    reply[14] = 0; // firstType
    reply[15] = n_types;
    reply[16] = n_types; // totalTypes
    reply[17] = MIN_KEY_CODE; // firstKeySym
    state.write_u16(&mut reply, 18, total_syms_count);
    reply[20] = N_KEYS as u8; // nKeySyms
    reply[21] = MIN_KEY_CODE; // firstKeyAction
    state.write_u16(&mut reply, 22, total_actions);
    reply[24] = N_KEYS as u8; // nKeyActions (full range)
    reply[25] = MIN_KEY_CODE; // firstKeyBehavior
    reply[26] = n_key_behaviors;
    reply[27] = total_key_behaviors;
    reply[28] = MIN_KEY_CODE; // firstKeyExplicit
    reply[29] = n_key_explicit;
    reply[30] = total_key_explicit;
    reply[31] = MIN_KEY_CODE; // firstModMapKey
    reply[32] = n_mod_map_keys;
    reply[33] = total_mod_map_keys;
    reply[34] = MIN_KEY_CODE; // firstVModMapKey
    reply[35] = n_vmod_map_keys;
    reply[36] = total_vmod_map_keys;
    // 37 = pad2
    state.write_u16(&mut reply, 38, virtual_mods);

    reply[40..].copy_from_slice(&data);
    reply
}

/// Handle XKB SetMap request: allow clients to change key type assignments
/// and symbol mappings.
pub(crate) fn handle_xkb_set_map(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 36, seq, 136, data[1] as u16, state.msb_first);

    let present = state.read_u16(data, 8);
    let _flags = state.read_u16(data, 10);
    let _min_key_code = data[12];
    let _max_key_code = data[13];
    let first_type = data[14];
    let n_types = data[15];
    let first_key_sym = data[16];
    let _n_key_syms = data[17];
    // totalActs at 18-19
    let first_key_act = data[20];
    let n_key_acts = data[21];
    // firstKeyBehavior, nKeyBehaviors at 22-23
    let first_key_behavior = data[22];
    let n_key_behaviors = data[23];
    // firstKeyExplicit, nKeyExplicit at 24-25
    // firstModMapKey, nModMapKeys at 26-27
    // firstVModMapKey, nVModMapKeys at 28-29
    // virtualMods at 30-31

    let mut offset = 36;

    // Parse KeyTypes if present (bit 0)
    if present & 0x01 != 0 && n_types > 0 {
        use super::super::super::client::{XkbKeyType, XkbKTMapEntry, XkbModsWire};
        for type_idx in first_type..first_type.wrapping_add(n_types) {
            if offset + 8 > data.len() { break; }
            let mods_mask = data[offset];
            let mods_mods = data[offset + 1];
            // vmods at offset+2..4
            let num_levels = data[offset + 4];
            let n_map_entries = data[offset + 5] as usize;
            let has_preserve = data[offset + 6] != 0;
            offset += 8; // XkbKeyTypeWireDesc

            let mut map = Vec::with_capacity(n_map_entries);
            for _ in 0..n_map_entries {
                if offset + 8 > data.len() { break; }
                let active = data[offset] != 0;
                let entry_mask = data[offset + 1];
                let level = data[offset + 2];
                let entry_mods = data[offset + 3];
                let entry_vmods = state.read_u16(data, offset + 4);
                map.push(XkbKTMapEntry {
                    active,
                    mods_mask: entry_mask,
                    level,
                    mods_mods: entry_mods,
                    mods_vmods: entry_vmods,
                });
                offset += 8;
            }

            let mut preserve = Vec::new();
            if has_preserve {
                for _ in 0..n_map_entries {
                    if offset + 4 > data.len() { break; }
                    preserve.push(XkbModsWire {
                        mask: data[offset],
                        real_mods: data[offset + 1],
                        vmods: state.read_u16(data, offset + 2),
                    });
                    offset += 4;
                }
            }

            state.xkb_key_types.insert(type_idx, XkbKeyType {
                mods_mask,
                mods_mods,
                num_levels,
                map,
                preserve,
            });
        }
        debug!("SetMap: stored {n_types} key types starting at {first_type}");
    }

    // Parse KeySyms if present (bit 1)
    if present & 0x02 != 0 {
        let mut kc = first_key_sym;
        while offset + 8 <= data.len() {
            let _group_info = data[offset + 4];
            let _width = data[offset + 5];
            let n_syms = state.read_u16(data, offset + 6) as usize;
            offset += 8;

            if n_syms > 0 && offset + n_syms * 4 <= data.len() {
                let mut syms = Vec::with_capacity(n_syms);
                for i in 0..n_syms {
                    syms.push(state.read_u32(data, offset + i * 4));
                }
                state.custom_keymap.insert(kc, syms);
                offset += n_syms * 4;
            }

            kc = kc.wrapping_add(1);
            if kc == 0 { break; }
        }
        debug!("SetMap: updated keysym mappings starting at keycode {first_key_sym}");
    }

    // Parse KeyActions if present (bit 2)
    if present & 0x04 != 0 && n_key_acts > 0 {
        use super::super::super::client::XkbAction;
        // Read per-key nActs array
        let n_acts_start = offset;
        let mut per_key_counts = Vec::with_capacity(n_key_acts as usize);
        for i in 0..n_key_acts as usize {
            if n_acts_start + i < data.len() {
                per_key_counts.push(data[n_acts_start + i]);
            } else {
                per_key_counts.push(0);
            }
        }
        offset += n_key_acts as usize;
        // Pad to 4-byte boundary
        offset = (offset + 3) & !3;
        // Parse action records (8 bytes each)
        for (i, &count) in per_key_counts.iter().enumerate() {
            let kc = first_key_act.wrapping_add(i as u8);
            let mut actions = Vec::with_capacity(count as usize);
            for _ in 0..count {
                if offset + 8 > data.len() { break; }
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&data[offset..offset + 8]);
                actions.push(XkbAction { raw });
                offset += 8;
            }
            if !actions.is_empty() {
                state.xkb_key_actions.insert(kc, actions);
            }
        }
        debug!("SetMap: stored {} key actions starting at {first_key_act}", per_key_counts.iter().map(|c| *c as usize).sum::<usize>());
    }

    // Parse KeyBehaviors if present (bit 3)
    if present & 0x08 != 0 && n_key_behaviors > 0 {
        use super::super::super::client::XkbKeyBehavior;
        for i in 0..n_key_behaviors as usize {
            if offset + 4 > data.len() { break; }
            let kc = first_key_behavior.wrapping_add(i as u8);
            let behavior_type = data[offset];
            let behavior_data = data[offset + 1];
            // bytes 2-3 are padding
            if behavior_type != 0 {
                state.xkb_key_behaviors.insert(kc, XkbKeyBehavior {
                    behavior_type,
                    data: behavior_data,
                });
            }
            offset += 4;
        }
        debug!("SetMap: stored {n_key_behaviors} key behaviors starting at {first_key_behavior}");
    }

    // Parse VirtualMods if present (bit 4)
    if present & 0x10 != 0 {
        let vmods_mask = state.read_u16(data, 30);
        let n_vmods = vmods_mask.count_ones() as usize;
        for i in 0..n_vmods {
            if offset + 1 > data.len() { break; }
            // Find which vmod index this corresponds to
            let mut vmod_idx = 0u8;
            let mut count = 0;
            for bit in 0..16u16 {
                if vmods_mask & (1 << bit) != 0 {
                    if count == i {
                        vmod_idx = bit as u8;
                        break;
                    }
                    count += 1;
                }
            }
            state.xkb_vmod_bindings[vmod_idx as usize] = data[offset];
            offset += 1;
        }
        // Pad to 4-byte boundary
        offset = (offset + 3) & !3;
        debug!("SetMap: stored {n_vmods} virtual modifier bindings");
    }

    // Parse Explicit if present (bit 5)
    if present & 0x20 != 0 {
        let first_key_explicit = data[24];
        let n_key_explicit = data[25] as usize;
        for i in 0..n_key_explicit {
            if offset + 2 > data.len() { break; }
            let kc = first_key_explicit.wrapping_add(i as u8);
            let explicit_flags = data[offset];
            if explicit_flags != 0 {
                state.xkb_explicit.insert(kc, explicit_flags);
            }
            offset += 2; // keycode (implicit) + explicit byte, padded to 2
        }
        // Pad to 4-byte boundary
        offset = (offset + 3) & !3;
        debug!("SetMap: stored {n_key_explicit} explicit flags");
    }

    // Parse ModMap if present (bit 6)
    if present & 0x40 != 0 {
        let first_mod_map_key = data[26];
        let n_mod_map_keys = data[27] as usize;
        for i in 0..n_mod_map_keys {
            if offset + 2 > data.len() { break; }
            let kc = first_mod_map_key.wrapping_add(i as u8);
            let mods = data[offset];
            if mods != 0 {
                state.xkb_modmap.insert(kc, mods);
            }
            offset += 2;
        }
        // Pad to 4-byte boundary
        offset = (offset + 3) & !3;
        debug!("SetMap: stored {n_mod_map_keys} modifier map entries");
    }

    // Parse VModMap if present (bit 7)
    if present & 0x80 != 0 {
        let first_vmod_map_key = data[28];
        let n_vmod_map_keys = data[29] as usize;
        for i in 0..n_vmod_map_keys {
            if offset + 4 > data.len() { break; }
            let kc = first_vmod_map_key.wrapping_add(i as u8);
            let vmods = state.read_u16(data, offset + 2);
            if vmods != 0 {
                state.xkb_vmodmap.insert(kc, vmods);
            }
            offset += 4;
        }
        debug!("SetMap: stored {n_vmod_map_keys} virtual modifier map entries");
    }

    let _ = offset;
    debug!("SetMap: present=0x{present:04x} fully processed");

    // Send XkbMapNotify if client subscribed.
    super::maybe_send_xkb_map_notify(state, present);

    Vec::new()
}

/// Handle XKB GetKbdByName (minor opcode 23).
///
/// This compound request returns a keyboard description that includes the
/// map (types, syms, actions, behaviors) and names data.  The reply contains
/// embedded sub-replies for GetMap and GetNames whose `reported` bitmask
/// tells the client which sections are present.
pub(crate) fn handle_xkb_get_kbd_by_name(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    device_id: u8,
) -> Vec<u8> {
    // GetKbdByName request layout:
    //   4: deviceSpec (CARD16)
    //   6: need (CARD16) — which components the client wants
    //   8: want (CARD16) — which components the client would like
    //   10: load (BOOL)
    //   We produce GetMap (bit 0) and GetNames (bit 9) sections.

    let need: u16 = if data.len() >= 8 { state.read_u16(data, 6) } else { 0 };
    let want: u16 = if data.len() >= 10 { state.read_u16(data, 8) } else { 0 };
    let components = need | want;

    // Build the GetMap sub-reply
    let map_reply = build_xkb_get_map_reply(state, seq);
    // Build the GetNames sub-reply
    let names_reply = super::names::build_xkb_get_names_reply(state, seq, device_id, 0x0FFF);

    // Determine which sections we're including
    let mut reported: u16 = 0;
    let mut body = Vec::new();

    // Types/Map section (bit 0 = GBN_TypesNames)
    if components & 0x01 != 0 || components & 0x02 != 0 || components & 0x04 != 0 {
        reported |= 0x01; // GBN_Types
        // Embed the GetMap reply body (skip the 32-byte header)
        body.extend_from_slice(&map_reply);
    }

    // Names section (bit 9 = GBN_OtherNames)
    if components & 0x200 != 0 || components & 0x01 != 0 {
        reported |= 0x200;
        body.extend_from_slice(&names_reply);
    }

    // If no specific sections requested, return both
    if reported == 0 {
        reported = 0x01 | 0x200;
        body.extend_from_slice(&map_reply);
        body.extend_from_slice(&names_reply);
    }

    // Build the outer reply header
    // The GetKbdByName reply format:
    //   0: type = Reply (1)
    //   1: deviceID
    //   2-3: sequence
    //   4-7: length
    //   8-9: minKeyCode
    //  10-11: maxKeyCode
    //  12: loaded (BOOL)
    //  13: newKeyboard (BOOL)
    //  14-15: found (CARD16) — which sections are present
    //  16-17: reported (CARD16) — which sub-replies follow
    //  18-31: pad
    let total_body_len = body.len();
    let length_words = (total_body_len / 4) as u32;
    let total = 32 + total_body_len;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    reply[1] = device_id;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_words);
    reply[8] = MIN_KEY_CODE;
    reply[9] = MAX_KEY_CODE;
    reply[12] = 1; // loaded = true
    reply[13] = 0; // newKeyboard = false
    state.write_u16(&mut reply, 14, reported); // found
    state.write_u16(&mut reply, 16, reported); // reported

    if total_body_len > 0 {
        reply[32..].copy_from_slice(&body);
    }

    debug!("XKB GetKbdByName: need=0x{need:04x} want=0x{want:04x} reported=0x{reported:04x} body={}B", total_body_len);
    reply
}

/// 4-character XKB key names for keycodes 8..255.
pub(crate) fn us_qwerty_key_names() -> [&'static [u8; 4]; 248] {
    let mut names: [&[u8; 4]; 248] = [b"K   "; 248];
    // Standard evdev/xkb keycode-to-keyname mapping for a pc105 keyboard.
    // Names follow the XKB <XXXX> convention (without angle brackets).
    let real: &[(u8, &[u8; 4])] = &[
        // Row: Escape
        (9,  b"ESC "),
        // Row: Number row (AE = Alphanumeric E row)
        (10, b"AE01"), (11, b"AE02"), (12, b"AE03"), (13, b"AE04"),
        (14, b"AE05"), (15, b"AE06"), (16, b"AE07"), (17, b"AE08"),
        (18, b"AE09"), (19, b"AE10"), (20, b"AE11"), (21, b"AE12"),
        (22, b"BKSP"),
        // Row: Tab row (AD = Alphanumeric D row)
        (23, b"TAB "),
        (24, b"AD01"), (25, b"AD02"), (26, b"AD03"), (27, b"AD04"),
        (28, b"AD05"), (29, b"AD06"), (30, b"AD07"), (31, b"AD08"),
        (32, b"AD09"), (33, b"AD10"), (34, b"AD11"), (35, b"AD12"),
        (36, b"RTRN"),
        // Left Control
        (37, b"LCTL"),
        // Row: Home row (AC = Alphanumeric C row)
        (38, b"AC01"), (39, b"AC02"), (40, b"AC03"), (41, b"AC04"),
        (42, b"AC05"), (43, b"AC06"), (44, b"AC07"), (45, b"AC08"),
        (46, b"AC09"), (47, b"AC10"), (48, b"AC11"),
        // Tilde/grave
        (49, b"TLDE"),
        // Left Shift
        (50, b"LFSH"),
        // Backslash (between left shift and Z on ISO keyboards)
        (51, b"BKSL"),
        // Row: Bottom row (AB = Alphanumeric B row)
        (52, b"AB01"), (53, b"AB02"), (54, b"AB03"), (55, b"AB04"),
        (56, b"AB05"), (57, b"AB06"), (58, b"AB07"), (59, b"AB08"),
        (60, b"AB09"), (61, b"AB10"),
        // Right Shift
        (62, b"RTSH"),
        // Keypad multiply
        (63, b"KPMU"),
        // Left Alt
        (64, b"LALT"),
        // Space
        (65, b"SPCE"),
        // Caps Lock
        (66, b"CAPS"),
        // Function keys F1-F10
        (67, b"FK01"), (68, b"FK02"), (69, b"FK03"), (70, b"FK04"),
        (71, b"FK05"), (72, b"FK06"), (73, b"FK07"), (74, b"FK08"),
        (75, b"FK09"), (76, b"FK10"),
        // Num Lock, Scroll Lock
        (77, b"NMLK"), (78, b"SCLK"),
        // Keypad 7-9, minus, 4-6, plus, 1-3, 0, decimal
        (79, b"KP7 "), (80, b"KP8 "), (81, b"KP9 "), (82, b"KPSU"),
        (83, b"KP4 "), (84, b"KP5 "), (85, b"KP6 "), (86, b"KPAD"),
        (87, b"KP1 "), (88, b"KP2 "), (89, b"KP3 "),
        (90, b"KP0 "), (91, b"KPDL"),
        // ISO extra key (between left shift and Z)
        (94, b"LSGT"),
        // Function keys F11-F12
        (95, b"FK11"), (96, b"FK12"),
        // Katakana, Hiragana, Henkan, etc. (Japanese keyboard)
        (97, b"AB11"), (98, b"KATA"), (99, b"HIRA"),
        (100, b"HENK"), (101, b"HKTG"), (102, b"MUHE"),
        // Keypad Enter, Right Control
        (104, b"KPEN"), (105, b"RCTL"),
        // Keypad divide, Print Screen
        (106, b"KPDV"), (107, b"PRSC"),
        // Right Alt (AltGr)
        (108, b"RALT"),
        // Line Feed
        (109, b"LNFD"),
        // Home, Up, PgUp, Left, Right, End, Down, PgDn, Insert, Delete
        (110, b"HOME"), (111, b"UP  "), (112, b"PGUP"),
        (113, b"LEFT"), (114, b"RGHT"),
        (115, b"END "), (116, b"DOWN"), (117, b"PGDN"),
        (118, b"INS "), (119, b"DELE"),
        // Macro key
        (120, b"I120"),
        // Audio: Mute, VolumeDown, VolumeUp
        (121, b"MUTE"), (122, b"VOL-"), (123, b"VOL+"),
        // Power
        (124, b"POWR"),
        // Keypad equals
        (125, b"KPEQ"),
        // Keypad plus-minus
        (126, b"I126"),
        // Pause/Break
        (127, b"PAUS"),
        // Launch A (XF86Launch1)
        (128, b"I128"),
        // Keypad decimal (comma on some layouts)
        (129, b"I129"),
        // Hangul, Hanja (Korean keyboard)
        (130, b"HNGL"), (131, b"HJCV"),
        // Yen, Left Super (Windows/Meta), Right Super
        (132, b"AE13"),
        (133, b"LWIN"), (134, b"RWIN"),
        // Compose / Menu
        (135, b"COMP"),
        // Stop, Again, Props, Undo, Front, Copy, Open, Paste, Find, Cut
        (136, b"STOP"), (137, b"AGAI"), (138, b"PROP"), (139, b"UNDO"),
        (140, b"FRNT"), (141, b"COPY"), (142, b"OPEN"), (143, b"PAST"),
        (144, b"FIND"), (145, b"CUT "),
        // Help
        (146, b"HELP"),
        // XF86 multimedia keys
        (147, b"I147"), (148, b"I148"),
        // Function keys F13-F24
        (191, b"FK13"), (192, b"FK14"), (193, b"FK15"), (194, b"FK16"),
        (195, b"FK17"), (196, b"FK18"), (197, b"FK19"), (198, b"FK20"),
        (199, b"FK21"), (200, b"FK22"), (201, b"FK23"), (202, b"FK24"),
        // Keypad Enter (alias)
        (203, b"I203"),
        // Navigation / media keys
        (208, b"I208"), (209, b"I209"), (210, b"I210"),
        (211, b"I211"), (212, b"I212"), (213, b"I213"),
        // Other standard keys
        (214, b"I214"), (215, b"I215"), (216, b"I216"),
        (217, b"I217"), (218, b"I218"), (219, b"I219"),
        (220, b"I220"), (221, b"I221"), (222, b"I222"),
        (223, b"I223"), (224, b"I224"), (225, b"I225"),
        (226, b"I226"), (227, b"I227"), (228, b"I228"),
        (229, b"I229"), (230, b"I230"), (231, b"I231"),
        (232, b"I232"), (233, b"I233"), (234, b"I234"),
        (235, b"I235"), (236, b"I236"), (237, b"I237"),
        (238, b"I238"), (239, b"I239"), (240, b"I240"),
        (241, b"I241"), (242, b"I242"), (243, b"I243"),
        (244, b"I244"), (245, b"I245"), (246, b"I246"),
        (247, b"I247"), (248, b"I248"), (249, b"I249"),
        (250, b"I250"), (251, b"I251"), (252, b"I252"),
        (253, b"I253"), (254, b"I254"), (255, b"I255"),
    ];
    for &(kc, name) in real {
        if kc >= 8 {
            names[(kc - 8) as usize] = name;
        }
    }
    // For any remaining unmapped keycodes, generate "I<NNN>" placeholder names
    // following the XKB convention for unnamed internet/multimedia keys.
    static PLACEHOLDERS: [[u8; 4]; 256] = {
        let mut out = [[b' '; 4]; 256];
        let mut i = 0;
        while i < 256 {
            out[i][0] = b'I';
            out[i][1] = b'0' + ((i / 100) % 10) as u8;
            out[i][2] = b'0' + ((i / 10) % 10) as u8;
            out[i][3] = b'0' + (i % 10) as u8;
            i += 1;
        }
        out
    };
    for kc in 8u8..=255 {
        let idx = (kc - 8) as usize;
        if names[idx] == b"K   " {
            names[idx] = &PLACEHOLDERS[kc as usize];
        }
    }
    names
}

/// Standard US-QWERTY keysyms keyed by physical X11 keycode (8..255).
#[allow(dead_code)]
pub(crate) fn us_qwerty_keysyms() -> [u32; 248] {
    let mut syms = [0u32; 248];
    let mappings: &[(u8, u32)] = &[
        (9, 0xff1b),  // Escape
        (10, b'1' as u32),
        (11, b'2' as u32),
        (12, b'3' as u32),
        (13, b'4' as u32),
        (14, b'5' as u32),
        (15, b'6' as u32),
        (16, b'7' as u32),
        (17, b'8' as u32),
        (18, b'9' as u32),
        (19, b'0' as u32),
        (20, b'-' as u32),
        (21, b'=' as u32),
        (22, 0xff08), // BackSpace
        (23, 0xff09), // Tab
        (24, b'q' as u32),
        (25, b'w' as u32),
        (26, b'e' as u32),
        (27, b'r' as u32),
        (28, b't' as u32),
        (29, b'y' as u32),
        (30, b'u' as u32),
        (31, b'i' as u32),
        (32, b'o' as u32),
        (33, b'p' as u32),
        (34, b'[' as u32),
        (35, b']' as u32),
        (36, 0xff0d), // Return
        (37, 0xffe3), // Control_L
        (38, b'a' as u32),
        (39, b's' as u32),
        (40, b'd' as u32),
        (41, b'f' as u32),
        (42, b'g' as u32),
        (43, b'h' as u32),
        (44, b'j' as u32),
        (45, b'k' as u32),
        (46, b'l' as u32),
        (47, b';' as u32),
        (48, b'\'' as u32),
        (49, b'`' as u32),
        (50, 0xffe1), // Shift_L
        (51, b'\\' as u32),
        (52, b'z' as u32),
        (53, b'x' as u32),
        (54, b'c' as u32),
        (55, b'v' as u32),
        (56, b'b' as u32),
        (57, b'n' as u32),
        (58, b'm' as u32),
        (59, b',' as u32),
        (60, b'.' as u32),
        (61, b'/' as u32),
        (62, 0xffe2), // Shift_R
        (63, b'*' as u32),
        (64, 0xffe9), // Alt_L
        (65, b' ' as u32), // Space
        (66, 0xffe5), // Caps_Lock
        (77, 0xff7f), // Num_Lock
        (105, 0xffe4), // Control_R
        (108, 0xffea), // Alt_R
        (133, 0xffeb), // Super_L
        (134, 0xffec), // Super_R
    ];
    for &(kc, sym) in mappings {
        if kc >= 8 {
            syms[(kc - 8) as usize] = sym;
        }
    }
    syms
}
