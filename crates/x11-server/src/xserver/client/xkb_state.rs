//! XKB modifier and group state tracking types and helpers.

use std::collections::HashMap;
use std::time::Instant;

/// Per-connection XKB extension state. Lives on `ClientState::xkb`;
/// reads and writes happen through `state.xkb.*`.
///
/// Bundles the live modifier/group/controls tracking with the larger
/// SetMap / SetNames / SetCompatMap / SetIndicatorMap bookkeeping that
/// XKB clients can override.
#[derive(Default)]
pub(crate) struct XkbState {
    // -- live modifier / group state --------------------------------------
    /// Base modifiers (from currently pressed modifier keys).
    pub(crate) base_mods: u8,
    /// Latched modifiers (cleared on next non-modifier key press).
    pub(crate) latched_mods: u8,
    /// Locked modifiers (toggled by lock keys like CapsLock).
    pub(crate) locked_mods: u8,
    /// Base group index.
    pub(crate) base_group: i16,
    /// Latched group index.
    pub(crate) latched_group: i16,
    /// Locked group index.
    pub(crate) locked_group: i16,
    /// XKB controls state.
    pub(crate) controls: XkbControls,
    /// BounceKeys: timestamp of last release per keycode (for debounce).
    pub(crate) bounce_key_release_time: HashMap<u8, Instant>,
    /// StickyKeys: modifiers that have been "stuck" (latched by press-release)
    /// and will apply to the next non-modifier key, then clear.
    pub(crate) sticky_mods: u8,

    // -- map / SetMap bookkeeping ----------------------------------------
    /// Extra keyboard groups/layouts (groups 1-3). Each entry is a
    /// HashMap<keycode, Vec<keysym>> for that group. Group 0 is the built-in
    /// US-QWERTY layout from `keycode_to_keysym`.
    pub(crate) extra_groups: Vec<HashMap<u8, Vec<u32>>>,
    /// Group-switch keys: (keycode, target_group_index).
    /// When multi-layout is active, pressing these keys switches the active group.
    pub(crate) group_switch_keys: Vec<(u8, u8)>,
    /// Custom key types set by SetMap (keyed by type index).
    pub(crate) key_types: HashMap<u8, XkbKeyType>,
    /// Per-key action lists set by SetMap (keyed by keycode).
    pub(crate) key_actions: HashMap<u8, Vec<XkbAction>>,
    /// Per-key behaviors set by SetMap (keyed by keycode).
    pub(crate) key_behaviors: HashMap<u8, XkbKeyBehavior>,
    /// Per-key explicit override flags set by SetMap (keyed by keycode).
    pub(crate) explicit: HashMap<u8, u8>,
    /// Per-key modifier map set by SetMap (keyed by keycode).
    pub(crate) modmap: HashMap<u8, u8>,
    /// Per-key virtual modifier map set by SetMap (keyed by keycode).
    pub(crate) vmodmap: HashMap<u8, u16>,
    /// Virtual modifier bindings (16 entries for mod1-mod16).
    pub(crate) vmod_bindings: [u8; 16],

    // -- indicator state -------------------------------------------------
    /// XKB indicator state (32 bits, one per named indicator).
    pub(crate) indicators: u32,
    /// XKB indicator maps: indicator_index → (which_groups, groups, which_mods, mods).
    pub(crate) indicator_maps: Vec<XkbIndicatorMap>,
    /// XKB named indicator settings set by SetNamedIndicator.
    /// Maps indicator name atom to (indicator_index, change_state, led_state,
    ///   affect_which, change_which, affect_map_mask, map).
    pub(crate) named_indicators: HashMap<u32, XkbNamedIndicator>,

    // -- name overrides (SetNames) --------------------------------------
    /// XKB symbolic names stored by SetNames (which_bit → atom).
    /// Bits: 0=Keycodes, 1=Geometry, 2=Symbols, 3=PhysSymbols, 4=Types, 5=Compat.
    pub(crate) names_atoms: HashMap<u8, u32>,
    /// Per-type name atoms (overridden by SetNames).
    pub(crate) type_names: Vec<u32>,
    /// Per-type per-level name atoms (overridden by SetNames).
    pub(crate) kt_level_names: Vec<Vec<u32>>,
    /// Group name atoms (overridden by SetNames).
    pub(crate) group_names: Vec<u32>,
    /// Indicator name atoms (overridden by SetNames).
    pub(crate) indicator_name_atoms: Vec<u32>,
    /// Virtual modifier name atoms (overridden by SetNames).
    pub(crate) vmod_names: Vec<u32>,
    /// Per-key name overrides (overridden by SetNames).
    pub(crate) key_names: HashMap<u8, [u8; 4]>,
    /// Key alias pairs (overridden by SetNames).
    pub(crate) key_aliases: Vec<([u8; 4], [u8; 4])>,

    // -- device info (SetDeviceInfo) ------------------------------------
    /// Per-button action mappings set by SetDeviceInfo (keyed by button index).
    pub(crate) button_actions: HashMap<u8, [u8; 8]>,
    /// LED feedback info blob from SetDeviceInfo (echoed by GetDeviceInfo).
    pub(crate) device_led_info: Vec<u8>,

    // -- compat / interpretations ---------------------------------------
    /// Compatibility map — symbol interpretations (SI entries).
    /// Populated with defaults, overridable via SetCompatMap.
    pub(crate) compat_si: Vec<XkbSymInterpretation>,
    /// Group compatibility entries (4 groups).
    pub(crate) group_compat: [XkbGroupCompat; 4],

    // -- per-client event subscription ----------------------------------
    /// Per-client event mask (SelectEvents). Bitmask of XKB event types this
    /// client wants to receive. Bit positions correspond to XkbEventType values
    /// (e.g., bit 0 = NewKeyboardNotify, bit 1 = MapNotify, bit 11 = StateNotify).
    pub(crate) event_mask: u32,
}

// `Default` is derived: every field's natural default is the right
// one. `compat_si` in particular starts EMPTY and is filled in by the
// connection initializer (`handlers::xkb::default_compat_si`) rather
// than here, so this module doesn't have to depend on the handler
// module just to construct itself.

impl XkbState {
    /// Effective (combined) modifiers: base | latched | locked.
    pub(crate) fn effective_mods(&self) -> u8 {
        self.base_mods | self.latched_mods | self.locked_mods
    }

    /// Effective group: (base + latched + locked) clamped to [0, 3].
    pub(crate) fn effective_group(&self) -> i16 {
        (self.base_group + self.latched_group + self.locked_group).clamp(0, 3)
    }

    /// Check if BounceKeys should reject this key press (debounce).
    /// Returns true if the key should be rejected.
    pub(crate) fn bounce_keys_reject(&self, keycode: u8) -> bool {
        use crate::xserver::handlers::xkb::XKB_BOUNCE_KEYS_MASK;
        if (self.controls.enabled_ctrls & XKB_BOUNCE_KEYS_MASK) == 0 {
            return false;
        }
        if let Some(&release_time) = self.bounce_key_release_time.get(&keycode) {
            let elapsed = release_time.elapsed().as_millis() as u16;
            elapsed < self.controls.debounce_delay
        } else {
            false
        }
    }

    /// Update modifier state when a key is pressed.
    /// Returns the modifier mask for the keycode, if any.
    pub(crate) fn key_press(&mut self, keycode: u8) -> u8 {
        use crate::xserver::handlers::xkb::XKB_STICKY_KEYS_MASK;
        let sticky_enabled = (self.controls.enabled_ctrls & XKB_STICKY_KEYS_MASK) != 0;

        let mod_bit = keycode_to_modifier(keycode);
        if mod_bit != 0 {
            if is_lock_key(keycode) {
                // Lock keys toggle on press
                self.locked_mods ^= mod_bit;
            } else if sticky_enabled {
                // StickyKeys: latch the modifier instead of requiring it held
                self.sticky_mods |= mod_bit;
                self.latched_mods |= mod_bit;
            } else {
                self.base_mods |= mod_bit;
            }
        } else if sticky_enabled && self.sticky_mods != 0 {
            // Non-modifier key with StickyKeys active: clear sticky state after use
            // (the latched_mods will be applied to this key event, then cleared)
            self.sticky_mods = 0;
            self.latched_mods = 0;
        }
        mod_bit
    }

    /// Update modifier state when a key is released.
    pub(crate) fn key_release(&mut self, keycode: u8) {
        use crate::xserver::handlers::xkb::{XKB_BOUNCE_KEYS_MASK, XKB_STICKY_KEYS_MASK};
        let sticky_enabled = (self.controls.enabled_ctrls & XKB_STICKY_KEYS_MASK) != 0;

        let mod_bit = keycode_to_modifier(keycode);
        if mod_bit != 0 && !is_lock_key(keycode)
            && !sticky_enabled {
                self.base_mods &= !mod_bit;
                // Clear latched modifiers on release of the modifier key
                self.latched_mods &= !mod_bit;
            }
            // For StickyKeys: modifier stays latched until a non-modifier is pressed

        // BounceKeys: record release time for debounce
        if (self.controls.enabled_ctrls & XKB_BOUNCE_KEYS_MASK) != 0 {
            self.bounce_key_release_time.insert(keycode, Instant::now());
        }
    }
}

/// MouseKeys: map numpad keycodes to pointer delta (dx, dy).
/// Returns Some((dx, dy)) if this key is a mouse movement key, None otherwise.
/// Uses the KP_ keys from evdev keycodes.
pub(crate) fn mousekeys_movement(keycode: u8) -> Option<(i16, i16)> {
    match keycode {
        79 => Some((-1, -1)), // KP_7 (Home) → up-left
        80 => Some((0, -1)),  // KP_8 (Up) → up
        81 => Some((1, -1)),  // KP_9 (PgUp) → up-right
        83 => Some((-1, 0)),  // KP_4 (Left) → left
        // 84 = KP_5 (Begin) → button click (not movement)
        85 => Some((1, 0)),  // KP_6 (Right) → right
        87 => Some((-1, 1)), // KP_1 (End) → down-left
        88 => Some((0, 1)),  // KP_2 (Down) → down
        89 => Some((1, 1)),  // KP_3 (PgDn) → down-right
        _ => None,
    }
}

/// MouseKeys: check if this keycode triggers a button click (KP_5/KP_Begin).
pub(crate) fn mousekeys_is_click(keycode: u8) -> bool {
    keycode == 84 // KP_5 (Begin)
}

/// Map a keycode to its X11 modifier bit, or 0 if not a modifier.
///
/// Backed by libxkbcommon's evdev/us layout, so any keymap-recognised
/// modifier key (including layout-specific ones) is reported correctly.
pub(crate) fn keycode_to_modifier(keycode: u8) -> u8 {
    crate::xserver::handlers::default_keymap::keycode_to_modifier_bit(keycode)
}

/// Whether a keycode is a lock-type key (toggles on press, doesn't track release).
pub(crate) fn is_lock_key(keycode: u8) -> bool {
    crate::xserver::handlers::default_keymap::is_lock_keycode(keycode)
}

/// XKB indicator map entry: ties an indicator (LED) to modifier/group state.
#[derive(Clone, Debug, Default)]
pub(crate) struct XkbIndicatorMap {
    /// Which modifier components to consider.
    pub(crate) which_mods: u8,
    /// Modifier mask to match against.
    pub(crate) mods: u8,
    /// Which group components to consider.
    pub(crate) which_groups: u8,
    /// Group mask: bits 0-3 for groups 0-3.
    pub(crate) groups: u8,
    /// Control mask for controls-based indicators.
    pub(crate) ctrls: u32,
}

/// XKB named indicator: settings stored by SetNamedIndicator.
/// Tracks whether an indicator's state and/or map has been explicitly set.
/// Fields are written by SetNamedIndicator and will be consumed by
/// GetNamedIndicator when that handler is extended to echo client overrides.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub(crate) struct XkbNamedIndicator {
    /// Indicator index (0-31).
    pub(crate) index: u8,
    /// Whether to explicitly drive the indicator state (ledState).
    pub(crate) change_state: bool,
    /// Explicit LED state if change_state is true.
    pub(crate) led_state: bool,
    /// Affect-which bitmask (which aspects of the map to update).
    pub(crate) affect_which: u8,
    /// Change-which bitmask (which map fields to change).
    pub(crate) change_which: u8,
    /// Current indicator map for this named indicator.
    pub(crate) map: XkbIndicatorMap,
}

/// XKB key type definition: describes how modifiers select shift levels.
/// Stored by SetMap for round-trip fidelity with GetMap.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct XkbKeyType {
    pub(crate) mods_mask: u8,
    pub(crate) mods_mods: u8,
    pub(crate) num_levels: u8,
    pub(crate) map: Vec<XkbKTMapEntry>,
    pub(crate) preserve: Vec<XkbModsWire>,
}

/// XKB key type map entry: modifier combination → level.
/// Stored by SetMap for round-trip fidelity with GetMap.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct XkbKTMapEntry {
    pub(crate) active: bool,
    pub(crate) mods_mask: u8,
    pub(crate) level: u8,
    pub(crate) mods_mods: u8,
    pub(crate) mods_vmods: u16,
}

/// XKB modifier wire format: mask + real_mods + vmods.
/// Stored by SetMap for round-trip fidelity with GetMap.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct XkbModsWire {
    pub(crate) mask: u8,
    pub(crate) real_mods: u8,
    pub(crate) vmods: u16,
}

/// XKB action: describes what happens when a key is pressed.
/// Each action is 8 bytes on the wire; we store them as raw bytes
/// for faithful round-trip with GetMap.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct XkbAction {
    pub(crate) raw: [u8; 8],
}

/// XKB symbol interpretation (SI) entry.
///
/// Maps keysym + modifier state → action. Used by compat compilation to
/// derive per-key actions from keysym semantics when no explicit action
/// is assigned to a key.
///
/// Wire format: XkbSymInterpretWireDesc (16 bytes).
#[derive(Clone, Debug)]
pub(crate) struct XkbSymInterpretation {
    /// Keysym to match (0 = wildcard, matches any keysym).
    pub(crate) sym: u32,
    /// Real modifier mask to match against.
    pub(crate) mods: u8,
    /// Match operation:
    ///   0 = NoneOf, 1 = AnyOfOrNone, 2 = AnyOf,
    ///   3 = AllOf, 4 = Exactly.
    pub(crate) match_op: u8,
    /// Virtual modifier index (0-15), or 0xFF if none.
    pub(crate) virtual_mod: u8,
    /// Flags (e.g., XkbSI_AutoRepeat=1, XkbSI_LockingKey=2).
    pub(crate) flags: u8,
    /// Action to apply when this SI matches (raw 8-byte wire format).
    pub(crate) action: [u8; 8],
}

/// XKB group compatibility entry.
///
/// Maps modifier state to group switching behavior. One per group (0-3).
/// Wire format: 4 bytes (mods, realMods, vmods_hi, vmods_lo).
#[derive(Clone, Debug, Default)]
pub(crate) struct XkbGroupCompat {
    /// Modifier mask.
    pub(crate) mods: u8,
    /// Real modifier mask.
    pub(crate) real_mods: u8,
    /// Virtual modifier bitmask (16 bits).
    pub(crate) vmods: u16,
}

/// XKB key behavior: autorepeat, lock, radio group, etc.
/// Stored by SetMap for round-trip fidelity with GetMap.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct XkbKeyBehavior {
    pub(crate) behavior_type: u8,
    pub(crate) data: u8,
}

/// XKB keyboard controls (RepeatKeys, SlowKeys, BounceKeys, StickyKeys, MouseKeys).
#[derive(Clone)]
pub(crate) struct XkbControls {
    /// Bitmask of enabled controls (XkbControlsEnabledMask).
    pub(crate) enabled_ctrls: u32,
    /// RepeatKeys: delay in ms before auto-repeat starts.
    pub(crate) repeat_delay: u16,
    /// RepeatKeys: interval in ms between auto-repeat events.
    pub(crate) repeat_interval: u16,
    /// SlowKeys: delay in ms before a key press is accepted.
    pub(crate) slow_keys_delay: u16,
    /// BounceKeys: debounce interval in ms.
    pub(crate) debounce_delay: u16,
    /// MouseKeys: default button for mouse key events.
    pub(crate) mk_dflt_btn: u8,
    /// MouseKeys: delay before first mouse key repeat.
    pub(crate) mk_delay: u16,
    /// MouseKeys: interval between repeats.
    pub(crate) mk_interval: u16,
    /// MouseKeys: time to max speed.
    pub(crate) mk_time_to_max: u16,
    /// MouseKeys: maximum speed.
    pub(crate) mk_max_speed: u16,
    /// MouseKeys: acceleration curve.
    pub(crate) mk_curve: i16,
    /// AccessX: timeout before disabling accessibility features.
    pub(crate) ax_timeout: u16,
    /// AccessX: controls to enable on timeout.
    pub(crate) ax_options: u32,
    /// Per-key repeat bitmap (32 bytes, 256 bits).
    pub(crate) per_key_repeat: [u8; 32],
    /// Number of groups (1-4).
    pub(crate) num_groups: u8,
}

impl Default for XkbControls {
    fn default() -> Self {
        use crate::xserver::types::keycode_bitset;
        let mut per_key_repeat = [0xFFu8; keycode_bitset::SIZE];
        // Disable auto-repeat for modifier keys by default.
        for kc in [
            37u8, // Ctrl_L
            50,   // Shift_L
            62,   // Shift_R
            64,   // Alt_L
            66,   // Caps_Lock
            77,   // Num_Lock
            105,  // Ctrl_R
            108,  // Alt_R
            133,  // Super_L
            134,  // Super_R
        ] {
            keycode_bitset::clear(&mut per_key_repeat, kc);
        }

        Self {
            // RepeatKeys enabled by default (bit 0 = XkbRepeatKeysMask)
            enabled_ctrls: 1 << 0, // XkbRepeatKeysMask
            repeat_delay: 660,
            repeat_interval: 40,
            slow_keys_delay: 300,
            debounce_delay: 300,
            mk_dflt_btn: 0,
            mk_delay: 160,
            mk_interval: 40,
            mk_time_to_max: 30,
            mk_max_speed: 30,
            mk_curve: 500,
            ax_timeout: 120,
            ax_options: 0,
            per_key_repeat,
            num_groups: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_mods_combines_all_sources() {
        let mut s = XkbState::default();
        s.base_mods = 0x01; // Shift
        s.latched_mods = 0x04; // Control
        s.locked_mods = 0x02; // CapsLock
        assert_eq!(s.effective_mods(), 0x07);
    }

    #[test]
    fn effective_group_clamped() {
        let mut s = XkbState::default();
        s.base_group = 2;
        s.latched_group = 3;
        s.locked_group = 0;
        // 2 + 3 + 0 = 5, clamped to 3
        assert_eq!(s.effective_group(), 3);
    }

    #[test]
    fn effective_group_negative_clamped() {
        let mut s = XkbState::default();
        s.base_group = -2;
        s.latched_group = 0;
        s.locked_group = 0;
        assert_eq!(s.effective_group(), 0);
    }

    #[test]
    fn key_press_sets_base_mods_for_shift() {
        let mut s = XkbState::default();
        let mod_bit = s.key_press(50); // Shift_L
        assert_eq!(mod_bit, 0x01);
        assert_eq!(s.base_mods, 0x01);
    }

    #[test]
    fn key_press_toggles_lock_for_capslock() {
        let mut s = XkbState::default();
        s.key_press(66); // CapsLock
        assert_eq!(s.locked_mods, 0x02);
        // Press again → toggles off
        s.key_press(66);
        assert_eq!(s.locked_mods, 0x00);
    }

    #[test]
    fn key_release_clears_base_mods() {
        let mut s = XkbState::default();
        s.key_press(50); // Shift_L press
        assert_eq!(s.base_mods, 0x01);
        s.key_release(50); // Shift_L release
        assert_eq!(s.base_mods, 0x00);
    }

    #[test]
    fn key_release_does_not_toggle_lock_keys() {
        let mut s = XkbState::default();
        s.key_press(66); // CapsLock press → locked_mods = 0x02
        s.key_release(66); // CapsLock release → should NOT toggle again
        assert_eq!(s.locked_mods, 0x02);
    }

    #[test]
    fn non_modifier_key_returns_zero() {
        let mut s = XkbState::default();
        let mod_bit = s.key_press(38); // 'a' key
        assert_eq!(mod_bit, 0);
        assert_eq!(s.base_mods, 0);
    }

    #[test]
    fn modifier_keycode_mapping() {
        assert_eq!(keycode_to_modifier(50), 0x01); // Shift_L
        assert_eq!(keycode_to_modifier(62), 0x01); // Shift_R
        assert_eq!(keycode_to_modifier(66), 0x02); // CapsLock
        assert_eq!(keycode_to_modifier(37), 0x04); // Ctrl_L
        assert_eq!(keycode_to_modifier(105), 0x04); // Ctrl_R
        assert_eq!(keycode_to_modifier(64), 0x08); // Alt_L
        assert_eq!(keycode_to_modifier(108), 0x08); // Alt_R
        assert_eq!(keycode_to_modifier(77), 0x10); // NumLock
        assert_eq!(keycode_to_modifier(133), 0x40); // Super_L
        assert_eq!(keycode_to_modifier(134), 0x40); // Super_R
        assert_eq!(keycode_to_modifier(38), 0); // 'a'
        assert_eq!(keycode_to_modifier(0), 0); // invalid
    }

    #[test]
    fn lock_key_detection() {
        assert!(is_lock_key(66)); // CapsLock
        assert!(is_lock_key(77)); // NumLock
        assert!(!is_lock_key(50)); // Shift_L
        assert!(!is_lock_key(37)); // Ctrl_L
        assert!(!is_lock_key(38)); // 'a'
    }

    #[test]
    fn controls_default_has_repeat_keys() {
        let c = XkbControls::default();
        assert_ne!(c.enabled_ctrls & (1 << 0), 0); // RepeatKeys enabled
        assert_eq!(c.repeat_delay, 660);
        assert_eq!(c.repeat_interval, 40);
        assert_eq!(c.num_groups, 1);
    }

    #[test]
    fn controls_default_modifier_keys_dont_repeat() {
        let c = XkbControls::default();
        // Shift_L (keycode 50) should NOT repeat
        assert_eq!(c.per_key_repeat[50 / 8] & (1 << (50 % 8)), 0);
        // CapsLock (keycode 66) should NOT repeat
        assert_eq!(c.per_key_repeat[66 / 8] & (1 << (66 % 8)), 0);
        // Regular key (keycode 38 = 'a') SHOULD repeat
        assert_ne!(c.per_key_repeat[38 / 8] & (1 << (38 % 8)), 0);
    }

    // -----------------------------------------------------------------------
    // StickyKeys tests
    // -----------------------------------------------------------------------

    #[test]
    fn sticky_keys_latches_modifier_on_press() {
        let mut s = XkbState::default();
        s.controls.enabled_ctrls |= 1 << 3; // StickyKeys
                                            // Press Shift_L
        s.key_press(50);
        assert_eq!(s.sticky_mods, 0x01);
        assert_eq!(s.latched_mods, 0x01);
        assert_eq!(s.base_mods, 0); // StickyKeys: NOT in base_mods
    }

    #[test]
    fn sticky_keys_clears_on_non_modifier_press() {
        let mut s = XkbState::default();
        s.controls.enabled_ctrls |= 1 << 3; // StickyKeys
                                            // Press and release Shift_L
        s.key_press(50);
        s.key_release(50);
        assert_eq!(s.latched_mods, 0x01); // Still latched
                                          // Now press 'a' — sticky should clear
        s.key_press(38);
        assert_eq!(s.sticky_mods, 0);
        assert_eq!(s.latched_mods, 0);
    }

    #[test]
    fn sticky_keys_modifier_persists_until_non_modifier() {
        let mut s = XkbState::default();
        s.controls.enabled_ctrls |= 1 << 3; // StickyKeys
                                            // Press Shift, release, press Ctrl — both should be latched
        s.key_press(50); // Shift
        s.key_release(50);
        s.key_press(37); // Ctrl
        assert_eq!(s.sticky_mods, 0x01 | 0x04);
        assert_eq!(s.latched_mods, 0x01 | 0x04);
    }

    // -----------------------------------------------------------------------
    // BounceKeys tests
    // -----------------------------------------------------------------------

    #[test]
    fn bounce_keys_rejects_rapid_repress() {
        let mut s = XkbState::default();
        s.controls.enabled_ctrls |= 1 << 2; // BounceKeys
        s.controls.debounce_delay = 300;
        // Simulate release of key 38
        s.key_release(38);
        // Immediately try to press again — should be rejected
        assert!(s.bounce_keys_reject(38));
    }

    #[test]
    fn bounce_keys_accepts_after_delay() {
        let mut s = XkbState::default();
        s.controls.enabled_ctrls |= 1 << 2; // BounceKeys
        s.controls.debounce_delay = 1; // 1ms
                                       // Simulate release
        s.bounce_key_release_time
            .insert(38, Instant::now() - std::time::Duration::from_millis(10));
        // After 10ms > 1ms debounce — should be accepted
        assert!(!s.bounce_keys_reject(38));
    }

    #[test]
    fn bounce_keys_disabled_does_not_reject() {
        let mut s = XkbState::default();
        // BounceKeys NOT enabled
        s.key_release(38);
        assert!(!s.bounce_keys_reject(38));
    }

    // -----------------------------------------------------------------------
    // MouseKeys tests
    // -----------------------------------------------------------------------

    #[test]
    fn xkb_control_masks_match_spec() {
        // Verify that the XKB boolean control mask values match the X11 spec:
        // RepeatKeys=bit0, SlowKeys=bit1, BounceKeys=bit2, StickyKeys=bit3,
        // MouseKeys=bit4, MouseKeysAccel=bit5, AccessXKeys=bit6
        let c = XkbControls::default();
        // RepeatKeys should be bit 0
        assert_eq!(c.enabled_ctrls & 1, 1, "RepeatKeys should be bit 0");
        // Other controls should not be enabled by default
        assert_eq!(c.enabled_ctrls & (1 << 1), 0, "SlowKeys should be off");
        assert_eq!(c.enabled_ctrls & (1 << 2), 0, "BounceKeys should be off");
        assert_eq!(c.enabled_ctrls & (1 << 3), 0, "StickyKeys should be off");
        assert_eq!(c.enabled_ctrls & (1 << 4), 0, "MouseKeys should be off");
    }

    #[test]
    fn bounce_keys_uses_correct_mask_bit2() {
        let mut s = XkbState::default();
        // Enable only bit 2 (BounceKeys)
        s.controls.enabled_ctrls = 1 << 2;
        s.controls.debounce_delay = 300;
        s.key_release(38);
        assert!(
            s.bounce_keys_reject(38),
            "BounceKeys at bit 2 should reject"
        );

        // Verify bit 4 (MouseKeys) does NOT trigger BounceKeys
        let mut s2 = XkbState::default();
        s2.controls.enabled_ctrls = 1 << 4; // MouseKeys, not BounceKeys
        s2.controls.debounce_delay = 300;
        s2.key_release(38);
        assert!(
            !s2.bounce_keys_reject(38),
            "MouseKeys bit should not trigger BounceKeys"
        );
    }

    #[test]
    fn sticky_keys_uses_correct_mask_bit3() {
        let mut s = XkbState::default();
        // Enable only bit 3 (StickyKeys)
        s.controls.enabled_ctrls = 1 << 3;
        s.key_press(50); // Shift_L
        assert_eq!(
            s.sticky_mods, 0x01,
            "StickyKeys at bit 3 should latch Shift"
        );
        assert_eq!(s.base_mods, 0, "StickyKeys should NOT set base_mods");

        // Verify bit 6 (AccessXKeys) does NOT trigger StickyKeys
        let mut s2 = XkbState::default();
        s2.controls.enabled_ctrls = 1 << 6; // AccessXKeys, not StickyKeys
        s2.key_press(50); // Shift_L
        assert_eq!(
            s2.sticky_mods, 0,
            "AccessXKeys bit should not trigger StickyKeys"
        );
        assert_eq!(
            s2.base_mods, 0x01,
            "Without StickyKeys, Shift should go to base_mods"
        );
    }

    #[test]
    fn mousekeys_numpad_movement() {
        assert_eq!(mousekeys_movement(80), Some((0, -1))); // KP_8 = Up
        assert_eq!(mousekeys_movement(88), Some((0, 1))); // KP_2 = Down
        assert_eq!(mousekeys_movement(83), Some((-1, 0))); // KP_4 = Left
        assert_eq!(mousekeys_movement(85), Some((1, 0))); // KP_6 = Right
        assert_eq!(mousekeys_movement(79), Some((-1, -1))); // KP_7 = Up-Left
        assert_eq!(mousekeys_movement(81), Some((1, -1))); // KP_9 = Up-Right
        assert_eq!(mousekeys_movement(87), Some((-1, 1))); // KP_1 = Down-Left
        assert_eq!(mousekeys_movement(89), Some((1, 1))); // KP_3 = Down-Right
        assert_eq!(mousekeys_movement(38), None); // 'a' = not a mouse key
    }

    #[test]
    fn mousekeys_click_is_kp5() {
        assert!(mousekeys_is_click(84)); // KP_5
        assert!(!mousekeys_is_click(80)); // KP_8 is movement, not click
        assert!(!mousekeys_is_click(38)); // 'a' is not a mouse key
    }
}
