//! XKB keymap operations: GetMap, SetMap, GetKbdByName, key name/sym tables.

use super::super::super::client::{is_lock_key, ClientState};
use super::super::parse_minor;
use super::{
    KB_LOCK, MAX_GROUPS, MAX_KEY_CODE, MIN_KEY_CODE, MODIFIER_KEYS, N_KEYS, SA_LOCK_GROUP,
    SA_LOCK_MODS, SA_SET_GROUP, SA_SET_MODS,
};
use crate::xserver::reply::{serialize_var_reply, ReplyBuf};
use tracing::debug;
use x11rb_protocol::protocol::xkb::{
    Action, Behavior, CommonBehavior, Explicit, GetMapMap, GetMapMapKeyActions, GetMapReply,
    KTMapEntry, KeyModMap, KeySymMap, KeyType, KeyVModMap, SAType, SASetGroup, SASetMods, SA,
    SetBehavior, SetExplicit, VMod, VModsHigh, VModsLow,
};
use x11rb_protocol::protocol::xproto::ModMask;
use x11rb_protocol::x11_utils::Serialize;

/// Build an XKB GetMap reply with full sections: KeyTypes, KeySyms,
/// KeyActions, KeyBehaviors, VirtualMods, ExplicitComponents,
/// ModifierMap, and VirtualModMap.
pub(crate) fn build_xkb_get_map_reply(state: &mut ClientState, seq: u16) -> Vec<u8> {
    // How many groups are active? At least 1 (US-QWERTY). Additional groups
    // come from state.xkb.extra_groups (populated by SetMap or layout config).
    let num_groups = (1 + state.xkb.extra_groups.len() as u8).min(MAX_GROUPS);

    // =====================================================================
    // 1. KeyTypes: 4 standard XKB types
    // =====================================================================
    let n_types = 4u8;
    let one_level = KeyType {
        mods_mask: ModMask::from(0u16),
        mods_mods: ModMask::from(0u16),
        mods_vmods: VMod::from(0u16),
        num_levels: 1,
        has_preserve: false,
        map: Vec::new(),
        preserve: Vec::new(),
    };
    let two_level = KeyType {
        mods_mask: ModMask::from(0x01u16),
        mods_mods: ModMask::from(0x01u16),
        mods_vmods: VMod::from(0u16),
        num_levels: 2,
        has_preserve: false,
        map: vec![KTMapEntry {
            active: true,
            mods_mask: ModMask::from(0x01u16),
            level: 1,
            mods_mods: ModMask::from(0x01u16),
            mods_vmods: VMod::from(0u16),
        }],
        preserve: Vec::new(),
    };
    let alphabetic = KeyType {
        mods_mask: ModMask::from(0x03u16),
        mods_mods: ModMask::from(0x03u16),
        mods_vmods: VMod::from(0u16),
        num_levels: 2,
        has_preserve: false,
        map: vec![
            KTMapEntry {
                active: true,
                mods_mask: ModMask::from(0x01u16),
                level: 1,
                mods_mods: ModMask::from(0x01u16),
                mods_vmods: VMod::from(0u16),
            },
            KTMapEntry {
                active: true,
                mods_mask: ModMask::from(0x02u16),
                level: 1,
                mods_mods: ModMask::from(0x02u16),
                mods_vmods: VMod::from(0u16),
            },
        ],
        preserve: Vec::new(),
    };
    let keypad = KeyType {
        mods_mask: ModMask::from(0x10u16),
        mods_mods: ModMask::from(0x10u16),
        mods_vmods: VMod::from(0u16),
        num_levels: 2,
        has_preserve: false,
        map: vec![KTMapEntry {
            active: true,
            mods_mask: ModMask::from(0x10u16),
            level: 1,
            mods_mods: ModMask::from(0x10u16),
            mods_vmods: VMod::from(0u16),
        }],
        preserve: Vec::new(),
    };
    let types_rtrn: Vec<KeyType> = vec![one_level, two_level, alphabetic, keypad];

    // =====================================================================
    // 2. KeySyms: one XkbSymMapWireDesc per key, with multi-group support
    // =====================================================================
    let mut total_syms_count: u16 = 0;
    let mut syms_rtrn: Vec<KeySymMap> = Vec::with_capacity(N_KEYS);
    let custom_keymap_snapshot = state.custom_keymap.lock().unwrap().clone();
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        let (normal, shifted) = super::super::resolve_keysym(kc, &custom_keymap_snapshot);
        let two_level_key = normal != 0 && shifted != 0 && normal != shifted;
        let width: u8 = if two_level_key { 2 } else { 1 };

        // Group 0: custom keymap or US-QWERTY. Groups 1+: xkb_extra_groups.
        let mut group_syms: Vec<Vec<u32>> = Vec::with_capacity(num_groups as usize);
        if two_level_key {
            group_syms.push(vec![normal, shifted]);
        } else {
            group_syms.push(vec![normal]);
        }
        for gi in 0..num_groups.saturating_sub(1) {
            let gi = gi as usize;
            if gi < state.xkb.extra_groups.len() {
                let extra = &state.xkb.extra_groups[gi];
                if let Some(syms) = extra.get(&kc) {
                    let mut gs = syms.clone();
                    gs.resize(width as usize, 0);
                    group_syms.push(gs);
                } else {
                    group_syms.push(group_syms[0].clone());
                }
            } else {
                group_syms.push(group_syms[0].clone());
            }
        }

        let actual_groups = group_syms.len() as u8;
        let n_syms = (width as u16) * (actual_groups as u16);
        let kt_idx = if two_level_key { 1u8 } else { 0u8 };

        let flat_syms: Vec<u32> = group_syms.into_iter().flatten().collect();
        syms_rtrn.push(KeySymMap {
            kt_index: [kt_idx, kt_idx, kt_idx, kt_idx],
            group_info: actual_groups,
            width,
            syms: flat_syms,
        });
        total_syms_count += n_syms;
    }

    // =====================================================================
    // 3. KeyActions: per-key nActs array + Action records
    // =====================================================================
    let mut key_actions_lookup: Vec<(u8, u8, u8)> = Vec::new();
    for &(kc, real_mod, _vmod) in MODIFIER_KEYS {
        if is_lock_key(kc) {
            key_actions_lookup.push((kc, SA_LOCK_MODS, real_mod));
        } else {
            key_actions_lookup.push((kc, SA_SET_MODS, real_mod));
        }
    }
    if num_groups > 1 {
        for &(kc, group_idx) in &state.xkb.group_switch_keys {
            key_actions_lookup.retain(|(k, _, _)| *k != kc);
            key_actions_lookup.push((kc, SA_LOCK_GROUP, group_idx));
        }
    }

    let acts_rtrn_count: Vec<u8> = (MIN_KEY_CODE..=MAX_KEY_CODE)
        .map(|kc| {
            if key_actions_lookup.iter().any(|(k, _, _)| *k == kc) {
                1
            } else {
                0
            }
        })
        .collect();
    let total_actions: u16 = acts_rtrn_count.iter().map(|&n| n as u16).sum();

    let mut acts_rtrn_acts: Vec<Action> = Vec::new();
    for kc in MIN_KEY_CODE..=MAX_KEY_CODE {
        if let Some(&(_, action_type, param)) =
            key_actions_lookup.iter().find(|(k, _, _)| *k == kc)
        {
            match action_type {
                SA_SET_GROUP | SA_LOCK_GROUP => {
                    acts_rtrn_acts.push(Action::from(SASetGroup {
                        type_: SAType::from(action_type),
                        flags: SA::from(0u8),
                        group: param as i8,
                    }));
                }
                _ => {
                    let (vmods_high, vmods_low) =
                        MODIFIER_KEYS
                            .iter()
                            .find(|(k, _, _)| *k == kc)
                            .map_or((0u8, 0u8), |&(_, _, vmod_idx)| {
                                if vmod_idx != 0xFF {
                                    let bits: u16 = 1 << vmod_idx;
                                    ((bits >> 8) as u8, (bits & 0xFF) as u8)
                                } else {
                                    (0, 0)
                                }
                            });
                    acts_rtrn_acts.push(Action::from(SASetMods {
                        type_: SAType::from(action_type),
                        flags: SA::from(0u8),
                        mask: ModMask::from(param as u16),
                        real_mods: ModMask::from(param as u16),
                        vmods_high: VModsHigh::from(vmods_high),
                        vmods_low: VModsLow::from(vmods_low),
                    }));
                }
            }
        }
    }

    let key_actions_section = GetMapMapKeyActions {
        acts_rtrn_count,
        acts_rtrn_acts,
    };

    // =====================================================================
    // 4. KeyBehaviors: lock-keys only (CapsLock, NumLock)
    // =====================================================================
    let behavior_keys = [66u8, 77u8];
    let behaviors_rtrn: Vec<SetBehavior> = behavior_keys
        .iter()
        .map(|&kc| SetBehavior {
            keycode: kc,
            behavior: Behavior::from(CommonBehavior {
                type_: KB_LOCK,
                data: 0,
            }),
        })
        .collect();
    let n_key_behaviors = behaviors_rtrn.len() as u8;
    let total_key_behaviors = n_key_behaviors;

    // =====================================================================
    // 5. VirtualMods: per-vmod modifier mapping
    // =====================================================================
    // bits 0,1,3 = Alt(Mod1=0x08), NumLock(Mod2=0x10), Super(Mod4=0x40)
    let virtual_mods: u16 = (1 << 0) | (1 << 1) | (1 << 3);
    let vmods_rtrn: Vec<ModMask> = vec![
        ModMask::from(0x08u16),
        ModMask::from(0x10u16),
        ModMask::from(0x40u16),
    ];

    // =====================================================================
    // 6. ExplicitComponents
    // =====================================================================
    let explicit_rtrn: Vec<SetExplicit> = MODIFIER_KEYS
        .iter()
        .map(|&(kc, _, _)| SetExplicit {
            keycode: kc,
            // XkbExplicitInterpretMask (0x10) | XkbExplicitAutoRepeatMask (0x20)
            explicit: Explicit::from(0x30u8),
        })
        .collect();
    let n_key_explicit = explicit_rtrn.len() as u8;
    let total_key_explicit = n_key_explicit;

    // =====================================================================
    // 7. ModifierMap
    // =====================================================================
    let mut modmap_seen: Vec<u8> = Vec::new();
    let mut modmap_rtrn: Vec<KeyModMap> = Vec::new();
    for &(kc, real_mod, _) in MODIFIER_KEYS {
        if !modmap_seen.contains(&kc) {
            modmap_seen.push(kc);
            modmap_rtrn.push(KeyModMap {
                keycode: kc,
                mods: ModMask::from(real_mod as u16),
            });
        }
    }
    let n_mod_map_keys = modmap_rtrn.len() as u8;
    let total_mod_map_keys = n_mod_map_keys;

    // =====================================================================
    // 8. VirtualModMap
    // =====================================================================
    let mut vmodmap_seen: Vec<u8> = Vec::new();
    let mut vmodmap_rtrn: Vec<KeyVModMap> = Vec::new();
    for &(kc, _, vmod_idx) in MODIFIER_KEYS {
        if vmod_idx != 0xFF && !vmodmap_seen.contains(&kc) {
            vmodmap_seen.push(kc);
            vmodmap_rtrn.push(KeyVModMap {
                keycode: kc,
                vmods: VMod::from(1u16 << vmod_idx),
            });
        }
    }
    let n_vmod_map_keys = vmodmap_rtrn.len() as u8;
    let total_vmod_map_keys = n_vmod_map_keys;

    serialize_var_reply(
        &GetMapReply {
            device_id: 3,
            sequence: seq,
            length: 0,
            min_key_code: MIN_KEY_CODE,
            max_key_code: MAX_KEY_CODE,
            first_type: 0,
            n_types,
            total_types: n_types,
            first_key_sym: MIN_KEY_CODE,
            total_syms: total_syms_count,
            n_key_syms: N_KEYS as u8,
            first_key_action: MIN_KEY_CODE,
            total_actions,
            n_key_actions: N_KEYS as u8,
            first_key_behavior: MIN_KEY_CODE,
            n_key_behaviors,
            total_key_behaviors,
            first_key_explicit: MIN_KEY_CODE,
            n_key_explicit,
            total_key_explicit,
            first_mod_map_key: MIN_KEY_CODE,
            n_mod_map_keys,
            total_mod_map_keys,
            first_v_mod_map_key: MIN_KEY_CODE,
            n_v_mod_map_keys: n_vmod_map_keys,
            total_v_mod_map_keys: total_vmod_map_keys,
            virtual_mods: VMod::from(virtual_mods),
            map: GetMapMap {
                types_rtrn: Some(types_rtrn),
                syms_rtrn: Some(syms_rtrn),
                key_actions: Some(key_actions_section),
                behaviors_rtrn: Some(behaviors_rtrn),
                vmods_rtrn: Some(vmods_rtrn),
                explicit_rtrn: Some(explicit_rtrn),
                modmap_rtrn: Some(modmap_rtrn),
                vmodmap_rtrn: Some(vmodmap_rtrn),
            },
        },
        state.byte_order(),
    )
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
            state.xkb.key_types.insert(
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
        let mut keymap = state.custom_keymap.lock().unwrap();
        for sm in syms {
            keymap.insert(kc, sm.syms);
            kc = kc.wrapping_add(1);
            if kc == 0 {
                break;
            }
        }
        drop(keymap);
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
                state.xkb.key_actions.insert(kc, actions);
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
                state.xkb.key_behaviors.insert(
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
                    state.xkb.vmod_bindings[bit as usize] = b;
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
                state.xkb.explicit.insert(e.keycode, flags);
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
                state.xkb.modmap.insert(m.keycode, mods);
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
                state.xkb.vmodmap.insert(v.keycode, bits);
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
