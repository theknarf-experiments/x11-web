//! XKB compatibility map: GetCompatMap, SetCompatMap, and compat compilation.

use tracing::debug;

use super::super::super::client::{ClientState, XkbGroupCompat, XkbSymInterpretation};
use super::{MAX_KEY_CODE, MIN_KEY_CODE, SA_LOCK_MODS, SA_NO_ACTION, SA_SET_MODS};

// XKB SI flags
const SI_LOCKING_KEY: u8 = 2;

// X11 keysym values used by the default symbol-interpretation table.
// Mirror the entries from /usr/include/X11/keysymdef.h.
const KEY_SHIFT_L: u32 = 0xFFE1;
const KEY_SHIFT_R: u32 = 0xFFE2;
const KEY_CONTROL_L: u32 = 0xFFE3;
const KEY_CONTROL_R: u32 = 0xFFE4;
const KEY_CAPS_LOCK: u32 = 0xFFE5;
const KEY_ALT_L: u32 = 0xFFE9;
const KEY_ALT_R: u32 = 0xFFEA;
const KEY_SUPER_L: u32 = 0xFFEB;
const KEY_SUPER_R: u32 = 0xFFEC;
const KEY_NUM_LOCK: u32 = 0xFF7F;
const KEY_ISO_LEVEL3_SHIFT: u32 = 0xFE03;

// XKB compat match operations — wire-stable u8 IDs from
// `x11rb::xkb::SymInterpretMatch`. Verified by a test below.
const MATCH_NONE_OF: u8 = 0;
const MATCH_ANY_OF_OR_NONE: u8 = 1;
const MATCH_ANY_OF: u8 = 2;
const MATCH_ALL_OF: u8 = 3;
const MATCH_EXACTLY: u8 = 4;

#[cfg(test)]
mod match_op_tests {
    use super::*;
    use x11rb_protocol::protocol::xkb::SymInterpretMatch;

    #[test]
    fn match_consts_match_x11rb() {
        assert_eq!(MATCH_NONE_OF, u8::from(SymInterpretMatch::NONE_OF));
        assert_eq!(
            MATCH_ANY_OF_OR_NONE,
            u8::from(SymInterpretMatch::ANY_OF_OR_NONE)
        );
        assert_eq!(MATCH_ANY_OF, u8::from(SymInterpretMatch::ANY_OF));
        assert_eq!(MATCH_ALL_OF, u8::from(SymInterpretMatch::ALL_OF));
        assert_eq!(MATCH_EXACTLY, u8::from(SymInterpretMatch::EXACTLY));
    }
}

/// Build the default set of symbol interpretations based on xkeyboard-config's
/// compat/complete. These map standard modifier keysyms to their actions.
pub(crate) fn default_si_table() -> Vec<XkbSymInterpretation> {
    let mut table = Vec::with_capacity(12);

    // Helper to build an SI entry
    let si = |sym: u32,
              mods: u8,
              match_op: u8,
              vmod: u8,
              flags: u8,
              action_type: u8,
              action_param: u8,
              vmod_bits: u16|
     -> XkbSymInterpretation {
        let mut action = [0u8; 8];
        action[0] = action_type;
        action[1] = 0; // action flags
        action[2] = action_param; // mask/mods
        action[3] = action_param; // realMods
        if vmod_bits != 0 {
            action[4] = (vmod_bits >> 8) as u8;
            action[5] = (vmod_bits & 0xFF) as u8;
        }
        XkbSymInterpretation {
            sym,
            mods,
            match_op,
            virtual_mod: vmod,
            flags,
            action,
        }
    };

    // Shift_L / Shift_R → SA_SetMods(Shift)
    table.push(si(
        KEY_SHIFT_L,
        0x01,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_SET_MODS,
        0x01,
        0,
    ));
    table.push(si(
        KEY_SHIFT_R,
        0x01,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_SET_MODS,
        0x01,
        0,
    ));
    // Caps_Lock → SA_LockMods(Lock)
    table.push(si(
        KEY_CAPS_LOCK,
        0x02,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        SI_LOCKING_KEY,
        SA_LOCK_MODS,
        0x02,
        0,
    ));
    // Control_L, Control_R → SA_SetMods(Control)
    table.push(si(
        KEY_CONTROL_L,
        0x04,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_SET_MODS,
        0x04,
        0,
    ));
    table.push(si(
        KEY_CONTROL_R,
        0x04,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_SET_MODS,
        0x04,
        0,
    ));
    // Alt_L, Alt_R → SA_SetMods(Mod1), vmod Alt(0)
    table.push(si(
        KEY_ALT_L,
        0x08,
        MATCH_ANY_OF_OR_NONE,
        0,
        0,
        SA_SET_MODS,
        0x08,
        1 << 0,
    ));
    table.push(si(
        KEY_ALT_R,
        0x08,
        MATCH_ANY_OF_OR_NONE,
        0,
        0,
        SA_SET_MODS,
        0x08,
        1 << 0,
    ));
    // Num_Lock → SA_LockMods(Mod2), vmod NumLock(1)
    table.push(si(
        KEY_NUM_LOCK,
        0x10,
        MATCH_ANY_OF_OR_NONE,
        1,
        SI_LOCKING_KEY,
        SA_LOCK_MODS,
        0x10,
        1 << 1,
    ));
    // Super_L, Super_R → SA_SetMods(Mod4), vmod Super(3)
    table.push(si(
        KEY_SUPER_L,
        0x40,
        MATCH_ANY_OF_OR_NONE,
        3,
        0,
        SA_SET_MODS,
        0x40,
        1 << 3,
    ));
    table.push(si(
        KEY_SUPER_R,
        0x40,
        MATCH_ANY_OF_OR_NONE,
        3,
        0,
        SA_SET_MODS,
        0x40,
        1 << 3,
    ));
    // ISO_Level3_Shift → SA_SetMods(Mod5)
    table.push(si(
        KEY_ISO_LEVEL3_SHIFT,
        0x80,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_SET_MODS,
        0x80,
        0,
    ));
    // Wildcard catch-all → SA_NoAction (ensures libxkbcommon accepts the compat map)
    table.push(si(
        0,
        0x00,
        MATCH_ANY_OF_OR_NONE,
        0xFF,
        0,
        SA_NO_ACTION,
        0x00,
        0,
    ));

    table
}

/// Build an XKB GetCompatMap reply from the dynamic compat map stored in state.
pub(crate) fn build_xkb_get_compat_map_reply(
    state: &mut ClientState,
    seq: u16,
    device_id: u8,
) -> Vec<u8> {
    use crate::xserver::reply::serialize_var_reply;
    use x11rb_protocol::protocol::xkb::{
        GetCompatMapReply, ModDef, SAType, SIAction, SetOfGroup, SymInterpret, VMod, VModsLow,
    };
    use x11rb_protocol::protocol::xproto::ModMask;

    let si_rtrn: Vec<SymInterpret> = state
        .xkb
        .compat_si
        .iter()
        .map(|si| {
            let action_bytes: [u8; 7] = si.action[1..8].try_into().unwrap_or([0; 7]);
            SymInterpret {
                sym: si.sym,
                mods: ModMask::from(si.mods as u16),
                match_: si.match_op,
                virtual_mod: VModsLow::from(si.virtual_mod),
                flags: si.flags,
                action: SIAction {
                    type_: SAType::from(si.action[0]),
                    data: action_bytes,
                },
            }
        })
        .collect();

    let group_rtrn: Vec<ModDef> = state
        .xkb
        .group_compat
        .iter()
        .map(|gc| ModDef {
            mask: ModMask::from(gc.mods as u16),
            real_mods: ModMask::from(gc.real_mods as u16),
            vmods: VMod::from(gc.vmods),
        })
        .collect();

    serialize_var_reply(
        &GetCompatMapReply {
            device_id,
            sequence: seq,
            length: 0,
            groups_rtrn: SetOfGroup::from(0x0Fu8), // all 4 groups
            first_si_rtrn: 0,
            n_total_si: si_rtrn.len() as u16,
            si_rtrn,
            group_rtrn,
        },
        state.byte_order(),
    )
}

/// Parse and apply a SetCompatMap request via the typed `xkb::SetCompatMapRequest`.
pub(crate) fn handle_xkb_set_compat_map(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    use x11rb_protocol::protocol::xkb::SetCompatMapRequest;
    use x11rb_protocol::x11_utils::Serialize;

    let req = match SetCompatMapRequest::try_parse_request(
        crate::xserver::request::request_header(data),
        &data[4..],
    ) {
        Ok(r) => r,
        Err(_) => {
            debug!("XKB SetCompatMap: parse error");
            return Vec::new();
        }
    };

    let recompute = req.recompute_actions;
    let truncate = req.truncate_si;
    let groups = u8::from(req.groups);
    let first_si = req.first_si as usize;
    let n_si = req.si.len();
    debug!(
        "XKB SetCompatMap: recompute={recompute} truncate={truncate} \
         groups={groups:#04x} firstSI={first_si} nSI={n_si}"
    );

    let new_entries: Vec<XkbSymInterpretation> = req
        .si
        .iter()
        .map(|si| XkbSymInterpretation {
            sym: si.sym,
            mods: u16::from(si.mods) as u8,
            match_op: si.match_,
            virtual_mod: u8::from(si.virtual_mod),
            flags: si.flags,
            action: si.action.serialize(),
        })
        .collect();

    if truncate {
        state.xkb.compat_si.truncate(first_si);
        state.xkb.compat_si.extend(new_entries);
    } else {
        let end = first_si + n_si;
        while state.xkb.compat_si.len() < end {
            state.xkb.compat_si.push(XkbSymInterpretation {
                sym: 0,
                mods: 0,
                match_op: MATCH_ANY_OF_OR_NONE,
                virtual_mod: 0xFF,
                flags: 0,
                action: [SA_NO_ACTION, 0, 0, 0, 0, 0, 0, 0],
            });
        }
        for (i, entry) in new_entries.into_iter().enumerate() {
            state.xkb.compat_si[first_si + i] = entry;
        }
    }

    // Apply per-group compat entries (one ModDef per set bit in `groups`).
    let mut group_iter = req.group_maps.iter();
    for g in 0..4u8 {
        if groups & (1 << g) != 0 {
            if let Some(gm) = group_iter.next() {
                state.xkb.group_compat[g as usize] = XkbGroupCompat {
                    mods: u16::from(gm.mask) as u8,
                    real_mods: u16::from(gm.real_mods) as u8,
                    vmods: u16::from(gm.vmods),
                };
            }
        }
    }

    if recompute {
        recompute_compat_actions(state);
    }

    Vec::new()
}

/// Recompute per-key actions from the compat map.
///
/// For each key that does NOT have an explicit action (set via SetMap),
/// look up its keysym in the compat SI table and derive the action.
/// This is the core of XKB compat compilation per the XKB spec §15.3.
fn recompute_compat_actions(state: &mut ClientState) {
    // Walk all keycodes in the keymap range
    for keycode in MIN_KEY_CODE..=MAX_KEY_CODE {
        // Skip keys that have explicit actions (set by SetMap with XkbExplicit)
        if state.xkb.explicit.get(&keycode).copied().unwrap_or(0) & 0x01 != 0 {
            // XkbExplicitKeyAction (bit 0) — client set this action explicitly
            continue;
        }

        // Look up the keysym for this keycode (group 0, level 0)
        let keysym = lookup_keysym_for_key(state, keycode);
        if keysym == 0 {
            continue;
        }

        // Find matching SI entry
        if let Some(si) = find_matching_si(
            &state.xkb.compat_si,
            keysym,
            state.xkb.modmap.get(&keycode).copied().unwrap_or(0),
        ) {
            let action = si.action;
            let virtual_mod = si.virtual_mod;
            let flags = si.flags;

            // Apply the action
            state.xkb.key_actions.insert(
                keycode,
                vec![crate::xserver::client::xkb_state::XkbAction { raw: action }],
            );

            // If the SI specifies a virtual modifier, update the vmodmap
            if virtual_mod != 0xFF && virtual_mod < 16 {
                let vmod_bit = 1u16 << virtual_mod;
                state.xkb.vmodmap.insert(keycode, vmod_bit);
            }

            // Update key behavior based on SI flags
            if flags & SI_LOCKING_KEY != 0 {
                state.xkb.key_behaviors.insert(
                    keycode,
                    crate::xserver::client::xkb_state::XkbKeyBehavior {
                        behavior_type: super::KB_LOCK,
                        data: 0,
                    },
                );
            }
        }
    }

    debug!(
        "XKB compat recompute: updated actions for {} keys",
        state.xkb.key_actions.len()
    );
}

/// Look up the primary keysym for a keycode (group 0, level 0).
fn lookup_keysym_for_key(state: &ClientState, keycode: u8) -> u32 {
    // Check custom keymap first (from ChangeKeyboardMapping or SetMap)
    if let Some(syms) = state.keyboard.custom_keymap.lock().unwrap().get(&keycode) {
        if let Some(&sym) = syms.first() {
            return sym;
        }
    }
    // Fall back to built-in keycode→keysym table
    let (primary, _shifted) = crate::xserver::handlers::keycode_to_keysym(keycode);
    primary
}

/// Find the first matching SI entry for a given keysym and modifier state.
///
/// Per XKB spec §15.3.1, SI entries are checked in order. The first match wins.
/// Match criteria:
///   - sym == 0 means wildcard (matches any keysym)
///   - match_op determines how mods are compared
fn find_matching_si(
    si_list: &[XkbSymInterpretation],
    keysym: u32,
    key_mods: u8,
) -> Option<&XkbSymInterpretation> {
    for si in si_list {
        // Check keysym match
        if si.sym != 0 && si.sym != keysym {
            continue;
        }

        // Check modifier match based on match operation
        let matches = match si.match_op {
            MATCH_NONE_OF => (key_mods & si.mods) == 0,
            MATCH_ANY_OF_OR_NONE => true, // always matches
            MATCH_ANY_OF => si.mods == 0 || (key_mods & si.mods) != 0,
            MATCH_ALL_OF => (key_mods & si.mods) == si.mods,
            MATCH_EXACTLY => key_mods == si.mods,
            _ => false,
        };

        if matches {
            return Some(si);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_si_table_has_standard_entries() {
        let table = default_si_table();
        // Should have at least the standard modifier keys + wildcard
        assert!(table.len() >= 12);

        // First entry should be Shift_L
        assert_eq!(table[0].sym, 0xFFE1);
        assert_eq!(table[0].mods, 0x01);
        assert_eq!(table[0].action[0], SA_SET_MODS);

        // Last entry should be wildcard (sym=0)
        assert_eq!(table.last().unwrap().sym, 0);
        assert_eq!(table.last().unwrap().action[0], SA_NO_ACTION);
    }

    #[test]
    fn test_find_matching_si_exact_keysym() {
        let table = default_si_table();
        // Look up Caps_Lock keysym
        let result = find_matching_si(&table, 0xFFE5, 0x02);
        assert!(result.is_some());
        let si = result.unwrap();
        assert_eq!(si.sym, 0xFFE5);
        assert_eq!(si.action[0], SA_LOCK_MODS);
    }

    #[test]
    fn test_find_matching_si_wildcard() {
        let table = default_si_table();
        // Look up an unknown keysym — should match wildcard
        let result = find_matching_si(&table, 0x0041, 0); // 'A'
        assert!(result.is_some());
        assert_eq!(result.unwrap().sym, 0); // wildcard
    }

    #[test]
    fn test_find_matching_si_none_of() {
        let si_list = vec![XkbSymInterpretation {
            sym: 0x0041,
            mods: 0x01,
            match_op: MATCH_NONE_OF,
            virtual_mod: 0xFF,
            flags: 0,
            action: [SA_SET_MODS, 0, 0x01, 0x01, 0, 0, 0, 0],
        }];
        // With Shift held (mods=0x01), NoneOf should NOT match
        assert!(find_matching_si(&si_list, 0x0041, 0x01).is_none());
        // Without Shift, NoneOf SHOULD match
        assert!(find_matching_si(&si_list, 0x0041, 0x00).is_some());
    }

    #[test]
    fn test_find_matching_si_all_of() {
        let si_list = vec![XkbSymInterpretation {
            sym: 0x0041,
            mods: 0x05,
            match_op: MATCH_ALL_OF, // Shift+Control
            virtual_mod: 0xFF,
            flags: 0,
            action: [SA_SET_MODS, 0, 0x05, 0x05, 0, 0, 0, 0],
        }];
        // Only Shift → doesn't match
        assert!(find_matching_si(&si_list, 0x0041, 0x01).is_none());
        // Shift+Control → matches
        assert!(find_matching_si(&si_list, 0x0041, 0x05).is_some());
        // Shift+Control+Alt → matches (superset of required)
        assert!(find_matching_si(&si_list, 0x0041, 0x0D).is_some());
    }

    #[test]
    fn test_find_matching_si_exactly() {
        let si_list = vec![XkbSymInterpretation {
            sym: 0x0041,
            mods: 0x01,
            match_op: MATCH_EXACTLY,
            virtual_mod: 0xFF,
            flags: 0,
            action: [SA_SET_MODS, 0, 0x01, 0x01, 0, 0, 0, 0],
        }];
        // Exactly Shift → matches
        assert!(find_matching_si(&si_list, 0x0041, 0x01).is_some());
        // Shift+Control → doesn't match (extra modifier)
        assert!(find_matching_si(&si_list, 0x0041, 0x05).is_none());
    }
}
