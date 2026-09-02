//! Pure input translation — **portable**, no smithay, no `cfg`.
//!
//! The frontend speaks X11's input vocabulary (that is what the
//! browser-side protocol was built against), and Wayland's seat
//! speaks evdev's. The mapping is small but every entry in it is a
//! silent-failure candidate, so it lives here as pure functions with
//! host-runnable tests rather than inline in the Linux-only seat code.
//!
//! STAGE: Input — this module still needs `ModifierSynth`: Wayland
//! carries no per-event modifier field (clients derive modifiers
//! purely from the xkb key stream), but the frontend sends an X11
//! `state` mask that can assert a modifier which never arrived as a
//! key event — Playwright's `keyboard.type(":")` sets the implied
//! shift mask with no Shift keydown, and without reconciliation the
//! client receives `;`. `ModifierSynth` must diff the incoming mask
//! against the currently-held set and synthesise press/release of
//! `ShiftL = 50`, `CtrlL = 37`, `AltL = 64`, `SuperL = 133`, tracking
//! synthesised codes separately from genuinely-held ones so a real
//! held modifier is never stomped.

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
}
