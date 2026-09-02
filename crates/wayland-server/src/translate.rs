//! Pure input translation — **portable**, no smithay, no `cfg`.
//!
//! The frontend speaks X11's input vocabulary (that is what the
//! browser-side protocol was built against), and Wayland's seat
//! speaks evdev's. The mapping is small but every entry in it is a
//! silent-failure candidate, so it lives here as pure functions with
//! host-runnable tests rather than inline in the Linux-only seat code.
//!

/// evdev button codes, from `linux/input-event-codes.h`.
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
pub const BTN_SIDE: u32 = 0x116;
pub const BTN_EXTRA: u32 = 0x117;

/// Which wl_pointer axis a wheel event scrolls, and in which
/// direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

/// Map an X11 button number onto an evdev button code.
///
/// The ordering trap: X11 numbers buttons left/middle/right = 1/2/3,
/// but evdev's codes run left/right/middle = 0x110/0x111/0x112. Get
/// this backwards and middle-click and right-click swap over — which
/// looks like "paste happens on right-click", with nothing in any log
/// to explain it.
///
/// Buttons 4–7 are the X11 wheel encoding and are *not* buttons on
/// Wayland; they return `None` here and are handled by
/// [`axis_for_button`] instead.
pub fn x11_button_to_evdev(button: u8) -> Option<u32> {
    match button {
        1 => Some(BTN_LEFT),
        2 => Some(BTN_MIDDLE),
        3 => Some(BTN_RIGHT),
        8 => Some(BTN_SIDE),
        9 => Some(BTN_EXTRA),
        _ => None,
    }
}

/// Map an X11 wheel button onto a wl_pointer axis + discrete step.
///
/// `InputEvent` has no axis variant at all — X11 encodes the wheel as
/// button presses (4 = up, 5 = down, 6 = left, 7 = right), and the
/// frontend faithfully forwards that. The returned value is in
/// notches: the seat turns `±1.0` into `axis_value120 = ±120` plus a
/// continuous `axis` of `±10.0`, which is exactly one detent.
///
/// The matching `ButtonRelease` for 4–7 must be swallowed by the
/// caller; emitting a second axis event would double every scroll.
pub fn axis_for_button(button: u8) -> Option<(ScrollAxis, f64)> {
    match button {
        4 => Some((ScrollAxis::Vertical, -1.0)),
        5 => Some((ScrollAxis::Vertical, 1.0)),
        6 => Some((ScrollAxis::Horizontal, -1.0)),
        7 => Some((ScrollAxis::Horizontal, 1.0)),
        _ => None,
    }
}

/// Convert an X11 keycode into the evdev keycode `wl_keyboard.key`
/// carries.
///
/// X11 keycodes are evdev keycodes plus 8 — a constant offset baked in
/// when XKB replaced the old core keyboard model, and one the frontend
/// already applies: `frontend/src/inputProtocol.ts`'s
/// `X11_KEYCODE_BY_CODE` table *is* the evdev table shifted by 8, and
/// its numeric fallback literally adds 8. So the whole "translation"
/// on our side is subtracting it back off.
///
/// `None` for anything below 8. That is not a theoretical case: the
/// frontend drops unmappable keys by sending nothing, but a wire
/// decoding bug or a hand-crafted client could deliver 0, and a plain
/// `key - 8` on a `u32` panics in debug and wraps to ~4 billion in
/// release — either way one malformed event takes down a compositor
/// serving every client in the sidecar.
pub fn x11_keycode_to_evdev(keycode: u32) -> Option<u32> {
    keycode.checked_sub(8)
}

/// X11 `KeyButMask` modifier bits, as the frontend packs them in
/// `InputEvent::*::state` (`frontend/src/inputProtocol.ts`,
/// `modifierMask`). The button bits (0x100 / 0x200 / 0x400) share the
/// same field but never collide with these four.
pub const MOD_SHIFT: u16 = 0x01;
pub const MOD_CTRL: u16 = 0x04;
pub const MOD_ALT: u16 = 0x08;
pub const MOD_SUPER: u16 = 0x40;

/// X11 keycodes of the left-hand modifier keys, used when a modifier
/// has to be *synthesised* (see [`ModifierSynth`]). Left rather than
/// right for no better reason than that it is what a human would have
/// pressed; xkb treats the pair identically.
pub const KEYCODE_SHIFT_L: u32 = 50;
pub const KEYCODE_CTRL_L: u32 = 37;
pub const KEYCODE_ALT_L: u32 = 64;
pub const KEYCODE_SUPER_L: u32 = 133;

/// Every modifier keycode in the standard evdev/X11 layout, in
/// `(keycode, bit)` pairs. Both the left and right key of each pair
/// map to the same mask bit, because the frontend's mask has no
/// handedness — `e.shiftKey` is one boolean.
///
/// 108 (`AltR`) doubles as `ISO_Level3_Shift` (AltGr) on most layouts,
/// which xkb treats as Mod5 rather than Mod1. Counting it as "alt is
/// really held" is deliberately approximate: the only consequence is
/// that we decline to synthesise an alt press while AltGr is down,
/// which is the conservative direction.
const MODIFIER_KEYCODES: [(u32, u16); 8] = [
    (KEYCODE_SHIFT_L, MOD_SHIFT),
    (62, MOD_SHIFT), // ShiftR
    (KEYCODE_CTRL_L, MOD_CTRL),
    (105, MOD_CTRL), // ControlR
    (KEYCODE_ALT_L, MOD_ALT),
    (108, MOD_ALT), // AltR / ISO_Level3_Shift
    (KEYCODE_SUPER_L, MOD_SUPER),
    (134, MOD_SUPER), // SuperR
];

/// The four modifiers we can synthesise, paired with the keycode used
/// to do it. Ordered shift-first so a synthesised `Shift` lands before
/// a synthesised `Ctrl` — irrelevant to xkb, but it makes traces read
/// the way a human would have typed.
const SYNTHESISABLE: [(u16, u32); 4] = [
    (MOD_SHIFT, KEYCODE_SHIFT_L),
    (MOD_CTRL, KEYCODE_CTRL_L),
    (MOD_ALT, KEYCODE_ALT_L),
    (MOD_SUPER, KEYCODE_SUPER_L),
];

/// Which modifier bit an X11 keycode corresponds to, if any.
pub fn modifier_bit_for_keycode(keycode: u32) -> Option<u16> {
    MODIFIER_KEYCODES
        .iter()
        .find(|(k, _)| *k == keycode)
        .map(|(_, b)| *b)
}

/// One synthetic key event to feed the seat before the real one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SynthKey {
    /// X11 keycode — the same space the frontend sends, so the caller
    /// hands it to the seat exactly as it would a real event.
    pub keycode: u32,
    pub pressed: bool,
}

/// Reconciles the frontend's X11 modifier mask against the modifier
/// keys that have actually been pressed.
///
/// ## Why this has to exist
///
/// X11 stamps a modifier mask onto every input event, so an X server
/// can honour `state = ShiftMask` even if it never saw a Shift
/// keydown. Wayland has no such field: `wl_keyboard.modifiers` is
/// derived by the compositor from the xkb state machine, which is
/// driven purely by the key stream. A client's idea of "shift is down"
/// therefore comes from us having delivered a Shift *key* event, and
/// from nothing else.
///
/// The frontend does assert modifiers that never arrived as keys.
/// `frontend/src/inputProtocol.ts::impliedShiftMask` sets the shift bit
/// for any character that is unreachable unshifted — because
/// Playwright's `keyboard.type(":")` dispatches `code="Semicolon"` with
/// `shiftKey` unset. Without reconciliation the client receives `;`,
/// the test asserts on `:`, and nothing anywhere logs an error.
///
/// ## Why synthesised keys are tracked separately
///
/// A real held Shift and a synthesised one must never be confused. If
/// they shared one counter, a `Shift` the user is genuinely holding
/// would be released the moment one event arrived with the bit clear
/// (which happens routinely: a `mousemove` carries `mouseButtonMask`
/// only, with no modifier bits at all). So `real` is only ever touched
/// by [`observe`](ModifierSynth::observe) — actual modifier key events
/// from the frontend — and `synth` only by
/// [`reconcile`](ModifierSynth::reconcile).
#[derive(Debug, Default, Clone)]
pub struct ModifierSynth {
    /// Bits held because the frontend sent a real modifier keydown.
    real: u16,
    /// Bits held because we invented a keydown for them.
    synth: u16,
}

impl ModifierSynth {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a real modifier key event from the frontend. Non-modifier
    /// keycodes are ignored.
    ///
    /// A real press supersedes a synthetic one for the same modifier:
    /// the synth bit is cleared without emitting a release, because the
    /// key is still down as far as xkb is concerned and the user's own
    /// keyup will lift it. Emitting a release here would produce
    /// press-release-press for one physical keystroke.
    pub fn observe(&mut self, keycode: u32, pressed: bool) {
        let Some(bit) = modifier_bit_for_keycode(keycode) else {
            return;
        };
        if pressed {
            self.real |= bit;
            self.synth &= !bit;
        } else {
            self.real &= !bit;
        }
    }

    /// Diff `mask` against what is held and return the synthetic key
    /// events to deliver *before* the event that carried `mask`.
    ///
    /// `about_to_send` is the keycode of that event when it is itself a
    /// key event, and it matters: on a Shift keydown the browser
    /// reports `shiftKey = true`, so a naive diff would synthesise a
    /// Shift press immediately before the real Shift press and the
    /// client would see the key twice. The modifier a key event is
    /// *about* is therefore excluded from this pass and settled by
    /// [`observe`](Self::observe) afterwards. Pass `None` for pointer
    /// events.
    pub fn reconcile(&mut self, mask: u16, about_to_send: Option<u32>) -> Vec<SynthKey> {
        let excluded = about_to_send.and_then(modifier_bit_for_keycode);
        let mut out = Vec::new();
        for (bit, keycode) in SYNTHESISABLE {
            if excluded == Some(bit) {
                continue;
            }
            let want = mask & bit != 0;
            let have_real = self.real & bit != 0;
            let have_synth = self.synth & bit != 0;

            if want && !have_real && !have_synth {
                self.synth |= bit;
                out.push(SynthKey {
                    keycode,
                    pressed: true,
                });
            } else if !want && have_synth {
                self.synth &= !bit;
                out.push(SynthKey {
                    keycode,
                    pressed: false,
                });
            }
            // `!want && have_real` falls through untouched — that is
            // the whole point of keeping the two sets apart.
        }
        out
    }

    /// Lift every modifier this type invented, leaving genuinely-held
    /// ones alone.
    ///
    /// Called once a synthesised modifier has done its job — after the
    /// key release of the character it was invented for. Without it a
    /// synthesised Shift stays down until some later event happens to
    /// arrive with the bit clear, and if the user types `:` and then
    /// stops, the client sits with Shift latched: the next character
    /// they type comes out shifted, minutes later, with nothing to
    /// connect it to. Every source of a synthetic modifier is
    /// per-character (`impliedShiftMask` in the frontend), so there is
    /// nothing to lose by scoping the hold to that character.
    pub fn release_synthetic(&mut self) -> Vec<SynthKey> {
        let mut out = Vec::new();
        for (bit, keycode) in SYNTHESISABLE {
            if self.synth & bit != 0 {
                self.synth &= !bit;
                out.push(SynthKey {
                    keycode,
                    pressed: false,
                });
            }
        }
        out
    }

    /// Bits currently held by real frontend key events. Test/diagnostic
    /// accessor.
    pub fn real(&self) -> u16 {
        self.real
    }

    /// Bits currently held by synthesised key events. Test/diagnostic
    /// accessor.
    pub fn synthesised(&self) -> u16 {
        self.synth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_middle_and_right_are_swapped_relative_to_evdev() {
        // The whole reason this function exists. X11 button 2 is the
        // middle button; evdev 0x111 (BTN_RIGHT) sits between LEFT
        // and MIDDLE numerically.
        assert_eq!(x11_button_to_evdev(1), Some(0x110));
        assert_eq!(x11_button_to_evdev(2), Some(0x112));
        assert_eq!(x11_button_to_evdev(3), Some(0x111));
    }

    #[test]
    fn back_forward_buttons_map_to_side_extra() {
        assert_eq!(x11_button_to_evdev(8), Some(BTN_SIDE));
        assert_eq!(x11_button_to_evdev(9), Some(BTN_EXTRA));
    }

    #[test]
    fn wheel_buttons_are_not_buttons() {
        for b in 4..=7u8 {
            assert_eq!(x11_button_to_evdev(b), None, "button {b}");
            assert!(axis_for_button(b).is_some(), "button {b}");
        }
    }

    #[test]
    fn wheel_directions() {
        // X11 button 4 is "scroll up", which on Wayland is a negative
        // vertical axis delta.
        assert_eq!(axis_for_button(4), Some((ScrollAxis::Vertical, -1.0)));
        assert_eq!(axis_for_button(5), Some((ScrollAxis::Vertical, 1.0)));
        assert_eq!(axis_for_button(6), Some((ScrollAxis::Horizontal, -1.0)));
        assert_eq!(axis_for_button(7), Some((ScrollAxis::Horizontal, 1.0)));
        assert_eq!(axis_for_button(1), None);
    }

    #[test]
    fn x11_keycodes_are_evdev_plus_eight() {
        // Spot-checked against linux/input-event-codes.h rather than
        // against the formula, so a wrong *offset* would be caught and
        // not just a wrong subtraction.
        assert_eq!(x11_keycode_to_evdev(9), Some(1), "Escape = KEY_ESC");
        assert_eq!(x11_keycode_to_evdev(38), Some(30), "a = KEY_A");
        assert_eq!(x11_keycode_to_evdev(36), Some(28), "Return = KEY_ENTER");
        assert_eq!(
            x11_keycode_to_evdev(KEYCODE_SHIFT_L),
            Some(42),
            "ShiftL = KEY_LEFTSHIFT"
        );
        assert_eq!(
            x11_keycode_to_evdev(KEYCODE_CTRL_L),
            Some(29),
            "ControlL = KEY_LEFTCTRL"
        );
        assert_eq!(
            x11_keycode_to_evdev(KEYCODE_ALT_L),
            Some(56),
            "AltL = KEY_LEFTALT"
        );
        assert_eq!(
            x11_keycode_to_evdev(KEYCODE_SUPER_L),
            Some(125),
            "SuperL = KEY_LEFTMETA"
        );
    }

    #[test]
    fn keycodes_below_the_offset_are_rejected_not_wrapped() {
        // The failure mode this guards: `key - 8` on a u32 wraps to
        // 4294967288 in release builds and panics in debug.
        assert_eq!(x11_keycode_to_evdev(0), None);
        assert_eq!(x11_keycode_to_evdev(7), None);
        assert_eq!(x11_keycode_to_evdev(8), Some(0));
    }

    #[test]
    fn both_handednesses_map_to_one_mask_bit() {
        assert_eq!(modifier_bit_for_keycode(50), Some(MOD_SHIFT));
        assert_eq!(modifier_bit_for_keycode(62), Some(MOD_SHIFT));
        assert_eq!(modifier_bit_for_keycode(37), Some(MOD_CTRL));
        assert_eq!(modifier_bit_for_keycode(105), Some(MOD_CTRL));
        assert_eq!(modifier_bit_for_keycode(64), Some(MOD_ALT));
        assert_eq!(modifier_bit_for_keycode(133), Some(MOD_SUPER));
        assert_eq!(modifier_bit_for_keycode(38), None, "'a' is not a modifier");
    }

    #[test]
    fn implied_shift_is_synthesised_and_then_lifted() {
        // The Playwright `keyboard.type(":")` case: a Semicolon
        // keypress whose mask claims shift, with no Shift keydown
        // anywhere in the stream.
        let mut m = ModifierSynth::new();
        let semicolon = 47;

        let pre = m.reconcile(MOD_SHIFT, Some(semicolon));
        assert_eq!(
            pre,
            vec![SynthKey {
                keycode: KEYCODE_SHIFT_L,
                pressed: true
            }]
        );
        assert_eq!(m.synthesised(), MOD_SHIFT);

        // Same key again with shift still asserted: no second press.
        assert!(m.reconcile(MOD_SHIFT, Some(semicolon)).is_empty());

        // The key release carries the mask too; the lift only comes
        // once an event arrives with the bit clear.
        let post = m.reconcile(0, Some(semicolon));
        assert_eq!(
            post,
            vec![SynthKey {
                keycode: KEYCODE_SHIFT_L,
                pressed: false
            }]
        );
        assert_eq!(m.synthesised(), 0);
    }

    #[test]
    fn a_real_held_modifier_is_never_synthesised_or_released() {
        let mut m = ModifierSynth::new();

        // Real Shift keydown. The browser already reports shiftKey on
        // the Shift keydown itself, so the mask asserts the bit — and
        // the whole point of `about_to_send` is that this must not
        // produce a synthetic press of the very key being sent.
        assert!(m.reconcile(MOD_SHIFT, Some(KEYCODE_SHIFT_L)).is_empty());
        m.observe(KEYCODE_SHIFT_L, true);
        assert_eq!(m.real(), MOD_SHIFT);
        assert_eq!(m.synthesised(), 0);

        // A shifted letter while it is held: nothing to do.
        assert!(m.reconcile(MOD_SHIFT, Some(38)).is_empty());

        // A bare mousemove carries `mouseButtonMask` only — no
        // modifier bits at all. A shared counter would release the
        // user's Shift here; separate sets must not.
        assert!(m.reconcile(0x100, None).is_empty());
        assert_eq!(m.real(), MOD_SHIFT);

        // Only the real keyup lifts it.
        m.observe(KEYCODE_SHIFT_L, false);
        assert_eq!(m.real(), 0);
    }

    #[test]
    fn a_real_press_supersedes_an_outstanding_synthetic_one() {
        let mut m = ModifierSynth::new();
        // Synthesised from an implied mask...
        assert_eq!(m.reconcile(MOD_CTRL, None).len(), 1);
        assert_eq!(m.synthesised(), MOD_CTRL);

        // ...then the user really presses Ctrl. The synthetic hold is
        // absorbed rather than released: the key is already down as far
        // as xkb is concerned, and the user's own keyup will lift it.
        m.observe(KEYCODE_CTRL_L, true);
        assert_eq!(m.synthesised(), 0);
        assert_eq!(m.real(), MOD_CTRL);
        assert!(
            m.reconcile(0, None).is_empty(),
            "a genuinely held Ctrl must survive an event with no modifier bits"
        );
    }

    #[test]
    fn releasing_synthetics_leaves_real_holds_alone() {
        let mut m = ModifierSynth::new();
        // Ctrl genuinely held, Shift only implied by the mask.
        m.observe(KEYCODE_CTRL_L, true);
        assert_eq!(m.reconcile(MOD_SHIFT | MOD_CTRL, Some(47)).len(), 1);

        let lifted = m.release_synthetic();
        assert_eq!(
            lifted,
            vec![SynthKey {
                keycode: KEYCODE_SHIFT_L,
                pressed: false
            }]
        );
        assert_eq!(m.synthesised(), 0);
        assert_eq!(m.real(), MOD_CTRL, "the real Ctrl must survive");
        assert!(m.release_synthetic().is_empty(), "idempotent");
    }

    #[test]
    fn ctrl_click_synthesises_on_a_pointer_event() {
        // Pointer events carry the same mask, and Ctrl+click is the
        // reason to reconcile on them: the modifier bits sit alongside
        // the button bits (0x100/0x200/0x400) without colliding.
        let mut m = ModifierSynth::new();
        let pre = m.reconcile(MOD_CTRL | 0x100, None);
        assert_eq!(
            pre,
            vec![SynthKey {
                keycode: KEYCODE_CTRL_L,
                pressed: true
            }]
        );
    }

    #[test]
    fn several_modifiers_are_synthesised_shift_first() {
        let mut m = ModifierSynth::new();
        let pre = m.reconcile(MOD_SHIFT | MOD_ALT | MOD_SUPER, None);
        assert_eq!(
            pre.iter().map(|k| k.keycode).collect::<Vec<_>>(),
            vec![KEYCODE_SHIFT_L, KEYCODE_ALT_L, KEYCODE_SUPER_L]
        );
        assert!(pre.iter().all(|k| k.pressed));

        // Dropping one lifts exactly one.
        let post = m.reconcile(MOD_SHIFT, None);
        assert_eq!(
            post,
            vec![
                SynthKey {
                    keycode: KEYCODE_ALT_L,
                    pressed: false
                },
                SynthKey {
                    keycode: KEYCODE_SUPER_L,
                    pressed: false
                }
            ]
        );
        assert_eq!(m.synthesised(), MOD_SHIFT);
    }
}
