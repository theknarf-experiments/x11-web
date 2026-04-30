//! XKB keymap operations: GetMap, SetMap, GetKbdByName, key name/sym tables.

use super::super::super::client::{is_lock_key, ClientState};
use super::super::parse_minor;
use super::{
    KB_LOCK, MAX_GROUPS, MAX_KEY_CODE, MIN_KEY_CODE, MODIFIER_KEYS, N_KEYS, SA_LOCK_GROUP,
    SA_LOCK_MODS, SA_SET_GROUP, SA_SET_MODS,
};
use crate::xserver::reply::ReplyBuf;
use tracing::debug;
use x11rb_protocol::x11_utils::Serialize;

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
                    if let Some(&(_, _, vmod_idx)) = MODIFIER_KEYS.iter().find(|(k, _, _)| *k == kc)
                    {
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
    for &kc in &[66u8, 77u8] {
        // CapsLock, NumLock
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
    let total_extra = 8 + data.len();
    let mut reply = ReplyBuf::with_extra(seq, total_extra, state.msb_first).set_data_byte(3); // deviceID (matches Xvfb's default core kbd)
    reply.buf_mut()[10] = MIN_KEY_CODE;
    reply.buf_mut()[11] = MAX_KEY_CODE;
    let present: u16 = 0x00ff;
    reply = reply.set_u16(12, present);
    reply.buf_mut()[14] = 0; // firstType
    reply.buf_mut()[15] = n_types;
    reply.buf_mut()[16] = n_types; // totalTypes
    reply.buf_mut()[17] = MIN_KEY_CODE; // firstKeySym
    reply = reply.set_u16(18, total_syms_count);
    reply.buf_mut()[20] = N_KEYS as u8; // nKeySyms
    reply.buf_mut()[21] = MIN_KEY_CODE; // firstKeyAction
    reply = reply.set_u16(22, total_actions);
    reply.buf_mut()[24] = N_KEYS as u8; // nKeyActions (full range)
    reply.buf_mut()[25] = MIN_KEY_CODE; // firstKeyBehavior
    reply.buf_mut()[26] = n_key_behaviors;
    reply.buf_mut()[27] = total_key_behaviors;
    reply.buf_mut()[28] = MIN_KEY_CODE; // firstKeyExplicit
    reply.buf_mut()[29] = n_key_explicit;
    reply.buf_mut()[30] = total_key_explicit;
    reply.buf_mut()[31] = MIN_KEY_CODE; // firstModMapKey
    reply.buf_mut()[32] = n_mod_map_keys;
    reply.buf_mut()[33] = total_mod_map_keys;
    reply.buf_mut()[34] = MIN_KEY_CODE; // firstVModMapKey
    reply.buf_mut()[35] = n_vmod_map_keys;
    reply.buf_mut()[36] = total_vmod_map_keys;
    // 37 = pad2
    reply = reply.set_u16(38, virtual_mods);

    reply.buf_mut()[40..].copy_from_slice(&data);
    reply.build()
}

/// Handle XKB SetMap request: allow clients to change key type assignments,
/// symbol mappings, key actions, behaviors, vmods, explicit flags, modmap and
/// vmod-map. Parses via the typed `xkb::SetMapRequest`.
pub(crate) fn handle_xkb_set_map(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use super::super::super::client::{
        XkbAction, XkbKTMapEntry, XkbKeyBehavior, XkbKeyType, XkbModsWire,
    };
    use x11rb_protocol::protocol::xkb::{MapPart, SetMapRequest};

    let req = parse_minor!(SetMapRequest, data, state, seq, 136, data[1] as u16);
    let aux = req.values.into_owned();
    let mut present: u16 = 0;

    if let Some(types) = aux.types {
        present |= u16::from(MapPart::KEY_TYPES);
        let count = types.len();
        for (i, t) in types.into_iter().enumerate() {
            let type_idx = req.first_type.wrapping_add(i as u8);
            let map: Vec<XkbKTMapEntry> = t
                .entries
                .iter()
                .map(|e| XkbKTMapEntry {
                    active: true,
                    mods_mask: u16::from(e.real_mods) as u8,
                    level: e.level,
                    mods_mods: u16::from(e.real_mods) as u8,
                    mods_vmods: u16::from(e.virtual_mods),
                })
                .collect();
            let preserve: Vec<XkbModsWire> = t
                .preserve_entries
                .iter()
                .map(|e| XkbModsWire {
                    mask: u16::from(e.real_mods) as u8,
                    real_mods: u16::from(e.real_mods) as u8,
                    vmods: u16::from(e.virtual_mods),
                })
                .collect();
            state.xkb_key_types.insert(
                type_idx,
                XkbKeyType {
                    mods_mask: u16::from(t.mask) as u8,
                    mods_mods: u16::from(t.real_mods) as u8,
                    num_levels: t.num_levels,
                    map,
                    preserve,
                },
            );
        }
        debug!(
            "SetMap: stored {count} key types starting at {}",
            req.first_type
        );
    }

    if let Some(syms) = aux.syms {
        present |= u16::from(MapPart::KEY_SYMS);
        let mut kc = req.first_key_sym;
        for sm in syms {
            state.custom_keymap.insert(kc, sm.syms);
            kc = kc.wrapping_add(1);
            if kc == 0 {
                break;
            }
        }
        debug!(
            "SetMap: updated keysym mappings starting at keycode {}",
            req.first_key_sym
        );
    }

    if let Some(key_actions) = aux.key_actions {
        present |= u16::from(MapPart::KEY_ACTIONS);
        let total: usize = key_actions.actions_count.iter().map(|c| *c as usize).sum();
        let mut act_iter = key_actions.actions.into_iter();
        for (i, &count) in key_actions.actions_count.iter().enumerate() {
            let kc = req.first_key_action.wrapping_add(i as u8);
            let actions: Vec<XkbAction> = (0..count)
                .filter_map(|_| act_iter.next().map(|a| XkbAction { raw: a.serialize() }))
                .collect();
            if !actions.is_empty() {
                state.xkb_key_actions.insert(kc, actions);
            }
        }
        debug!(
            "SetMap: stored {total} key actions starting at {}",
            req.first_key_action
        );
    }

    if let Some(behaviors) = aux.behaviors {
        present |= u16::from(MapPart::KEY_BEHAVIORS);
        let count = behaviors.len();
        for b in behaviors {
            let common = b.behavior.as_common();
            if common.type_ != 0 {
                state.xkb_key_behaviors.insert(
                    b.keycode,
                    XkbKeyBehavior {
                        behavior_type: common.type_,
                        data: common.data,
                    },
                );
            }
        }
        debug!("SetMap: stored {count} key behaviors");
    }

    if let Some(vmods) = aux.vmods {
        present |= u16::from(MapPart::VIRTUAL_MODS);
        let mask = u16::from(req.virtual_mods);
        let mut iter = vmods.into_iter();
        for bit in 0..16u8 {
            if mask & (1u16 << bit) != 0 {
                if let Some(b) = iter.next() {
                    state.xkb_vmod_bindings[bit as usize] = b;
                }
            }
        }
        debug!("SetMap: stored virtual modifier bindings (mask={mask:#06x})");
    }

    if let Some(explicit) = aux.explicit {
        present |= u16::from(MapPart::EXPLICIT_COMPONENTS);
        let count = explicit.len();
        for e in explicit {
            let flags = u8::from(e.explicit);
            if flags != 0 {
                state.xkb_explicit.insert(e.keycode, flags);
            }
        }
        debug!("SetMap: stored {count} explicit flags");
    }

    if let Some(modmap) = aux.modmap {
        present |= u16::from(MapPart::MODIFIER_MAP);
        let count = modmap.len();
        for m in modmap {
            let mods = u16::from(m.mods) as u8;
            if mods != 0 {
                state.xkb_modmap.insert(m.keycode, mods);
            }
        }
        debug!("SetMap: stored {count} modifier map entries");
    }

    if let Some(vmodmap) = aux.vmodmap {
        present |= u16::from(MapPart::VIRTUAL_MOD_MAP);
        let count = vmodmap.len();
        for v in vmodmap {
            let bits = u16::from(v.vmods);
            if bits != 0 {
                state.xkb_vmodmap.insert(v.keycode, bits);
            }
        }
        debug!("SetMap: stored {count} virtual modifier map entries");
    }

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

    let need: u16 = if data.len() >= 8 {
        state.read_u16(data, 6)
    } else {
        0
    };
    let want: u16 = if data.len() >= 10 {
        state.read_u16(data, 8)
    } else {
        0
    };
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
    let mut reply = ReplyBuf::with_extra(seq, total_body_len, state.msb_first)
        .set_data_byte(device_id)
        .set_u8(8, MIN_KEY_CODE)
        .set_u8(9, MAX_KEY_CODE)
        .set_u8(12, 1) // loaded = true
        .set_u8(13, 0) // newKeyboard = false
        .set_u16(14, reported) // found
        .set_u16(16, reported); // reported

    if total_body_len > 0 {
        reply.buf_mut()[32..].copy_from_slice(&body);
    }

    debug!(
        "XKB GetKbdByName: need=0x{need:04x} want=0x{want:04x} reported=0x{reported:04x} body={}B",
        total_body_len
    );
    reply.build()
}

/// 4-character XKB key names for keycodes 8..=255, cached from libxkbcommon's
/// evdev/pc105/us keymap. The first call compiles the keymap and fills the
/// static cache; later calls reuse it. References are stable for the
/// lifetime of the process so callers can build `[&'static [u8; 4]; 248]`.
fn cached_xkb_key_names() -> &'static [[u8; 4]; 248] {
    static CACHE: std::sync::OnceLock<[[u8; 4]; 248]> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let mut arr = [*b"K   "; 248];
        for kc in 8u32..=255 {
            arr[(kc - 8) as usize] = crate::xserver::handlers::default_keymap::key_name(kc as u8);
        }
        arr
    })
}

/// 4-character XKB key names for keycodes 8..255.
pub(crate) fn us_qwerty_key_names() -> [&'static [u8; 4]; 248] {
    let cache = cached_xkb_key_names();
    let mut out: [&'static [u8; 4]; 248] = [b"K   "; 248];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = &cache[i];
    }
    out
}
