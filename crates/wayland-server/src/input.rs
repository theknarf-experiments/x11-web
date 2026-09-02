//! `InputEvent` → `wl_seat`.
//!
//! Written from scratch — upstream waylandcraft's equivalent is a set
//! of JNI entrypoints called from Java with Android `MotionEvent`s
//! already decoded, so there is nothing to derive and no GPLv3 header
//! on this file. The pure translation tables it leans on live in
//! [`crate::translate`], where they are unit-tested on the macOS host.
//!
//! ## The shape of the problem
//!
//! The browser speaks X11's input vocabulary, because that is what the
//! frontend was built against for the X11 sidecar. Wayland's seat
//! speaks evdev's. Three of the differences are not mere renaming:
//!
//!   * **Buttons.** X11 numbers them left/middle/right = 1/2/3; evdev's
//!     codes run left/right/middle. See
//!     [`translate::x11_button_to_evdev`].
//!   * **Scrolling.** `InputEvent` has no axis variant at all — X11
//!     encodes the wheel as presses of buttons 4–7, so a scroll arrives
//!     here as a button event and has to be turned back into an axis.
//!     The matching *release* is then swallowed, or every notch counts
//!     twice.
//!   * **Modifiers.** X11 stamps a modifier mask on every event;
//!     Wayland has no such field and clients derive modifiers purely
//!     from the key stream. See [`translate::ModifierSynth`].
//!
//! ## Addressing
//!
//! Every event arrives addressed to a window UUID, not to a screen
//! position — the browser has already decided which window the user is
//! interacting with, because it is the one compositing them onto the
//! canvas. So there is no global pointer position here and no
//! stacking-order hit test: coordinates are window-local, and the only
//! search is *within* the addressed window's surface tree, to find
//! which subsurface is under the point.

use std::collections::HashMap;

use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_keyboard::KeyState;
use smithay::reexports::wayland_server::protocol::wl_pointer::{Axis, ButtonState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::{with_surface_tree_upward, TraversalAction};
use smithay::wayland::shell::xdg::ToplevelSurface;
use tracing::trace;
use x11_web_protocol::{InputEvent, WindowWmState};

use crate::state::State;
use crate::surface::{self, SurfaceBuffer};
use crate::translate::{self, ScrollAxis};
use crate::windows::{self, WindowKind};

/// Deliver one browser input event to the addressed window.
///
/// Called from the compositor thread, from the `Command::Input` arm of
/// `server::apply_command`. An event for a window that has already gone
/// is dropped rather than being an error: the frontend can legitimately
/// have a keystroke in flight when a client exits.
pub(crate) fn apply(state: &mut State, window_uuid: &str, event: InputEvent) {
    let Some(id) = state.windows.id_for_uuid(window_uuid) else {
        trace!(window_uuid, "input for an unknown window; dropping");
        return;
    };

    match event {
        // Hover deliberately does *not* focus: the frontend sends
        // motion continuously while the cursor crosses the canvas, and
        // focus-follows-mouse would make the keyboard jump between
        // windows the user is only passing over.
        InputEvent::MotionNotify { x, y, state: mask } => {
            reconcile_modifiers(state, mask, None);
            point_at(state, &id, x, y);
        }

        InputEvent::ButtonPress {
            button,
            x,
            y,
            state: mask,
        } => {
            focus_window(state, &id);
            reconcile_modifiers(state, mask, None);
            point_at(state, &id, x, y);
            match (
                translate::x11_button_to_evdev(button),
                translate::axis_for_button(button),
            ) {
                (Some(code), _) => state.seat.pointer_button(code, ButtonState::Pressed),
                (None, Some((axis, value))) => state.seat.pointer_axis(wl_axis(axis), value),
                (None, None) => trace!(button, "no wl_pointer mapping for this X11 button"),
            }
        }

        InputEvent::ButtonRelease {
            button,
            x,
            y,
            state: mask,
        } => {
            reconcile_modifiers(state, mask, None);
            point_at(state, &id, x, y);
            match (
                translate::x11_button_to_evdev(button),
                translate::axis_for_button(button),
            ) {
                (Some(code), _) => state.seat.pointer_button(code, ButtonState::Released),
                // Swallowed on purpose. X11 encodes one wheel notch as
                // a press *and* a release of button 4/5/6/7; the press
                // already became a complete `wl_pointer.axis`, so
                // emitting anything here would double every scroll.
                (None, Some(_)) => {}
                (None, None) => trace!(button, "no wl_pointer mapping for this X11 button"),
            }
        }

        InputEvent::KeyPress {
            keycode,
            state: mask,
        } => key(state, &id, keycode, mask, true),
        InputEvent::KeyRelease {
            keycode,
            state: mask,
        } => key(state, &id, keycode, mask, false),

        InputEvent::WindowManage { action } => manage(state, &id, action),

        // Everything below has no counterpart in the slice's protocol
        // set. Dropped at `trace` rather than `warn`: these fire at
        // input rates, and a warn-per-event would bury the log the
        // moment someone touch-scrolls a canvas.
        //
        //   MenuActivate     — needs the D-Bus menu mirror, which this
        //                      sidecar does not run (no MenuTracker).
        //   DndBridge        — needs wl_data_device (upstream ddm.rs),
        //                      explicitly out of scope.
        //   Touch*/Gesture*  — the seat advertises no touch capability.
        //   CompositionEvent — needs text-input-v3 / input-method-v2.
        other => trace!(?other, "input event has no wayland mapping; dropping"),
    }
}

// ---------------------------------------------------------------
// pointer
// ---------------------------------------------------------------

/// Move the pointer to `(x, y)` in the addressed window's coordinate
/// space, entering whichever surface of its tree is under that point.
fn point_at(state: &mut State, id: &ObjectId, x: i16, y: i16) {
    let Some(root) = root_surface_for(state, id) else {
        return;
    };
    // Resolved before the seat is touched so the borrow of
    // `state.surfaces` ends here; `hit` owns everything it returns.
    let hit = hit_test(&state.surfaces, &root, x as i32, y as i32);
    state
        .seat
        .pointer_motion_focus(Some(&hit.surface), hit.sx, hit.sy);
}

/// The surface under a window-local point, and the point expressed in
/// that surface's own coordinates.
struct Hit {
    surface: WlSurface,
    sx: f64,
    sy: f64,
}

/// Find the topmost surface of `root`'s tree containing `(wx, wy)`.
///
/// Coordinates in play, and they are easy to confuse:
///
///   * **window** space — what the browser sends, origin at the
///     top-left of what the frontend draws, which is the client's
///     `xdg_surface.set_window_geometry` rectangle when it set one;
///   * **surface** space — what `wl_pointer.enter`/`motion` carry,
///     origin at the top-left of each surface's own buffer.
///
/// The root surface sits at window-space `(-geo.x, -geo.y)`, because
/// window space *is* surface space shifted by the geometry origin. A
/// CSD client with a 30 px shadow margin has `geo.x = 30`, so a click
/// at window `(0, 0)` is surface `(30, 30)` — getting this backwards
/// puts every click in a GTK window 30 px up and to the left of where
/// the user aimed, which reads as "the buttons are subtly misaligned".
///
/// # Deadlock hazard
///
/// The traversal closures may not call anything that locks a surface's
/// user data — `with_states`, `get_parent`, `get_children`.
/// `with_surface_tree_upward` already holds that mutex while it calls
/// them, and re-entering it wedges the compositor thread with no error
/// anywhere: clients stay connected, the socket stays up, every window
/// freezes. `surface::subsurface_offset` takes `&SurfaceData` for
/// exactly this reason.
fn hit_test(
    surfaces: &HashMap<ObjectId, SurfaceBuffer>,
    root: &WlSurface,
    wx: i32,
    wy: i32,
) -> Hit {
    let Some(root_buf) = surfaces.get(&root.id()) else {
        // No pixels yet, so no geometry to speak of and nothing to
        // search. Pass the point straight through.
        return Hit {
            surface: root.clone(),
            sx: wx as f64,
            sy: wy as f64,
        };
    };
    let (rw, rh) = root_buf.logical_size();
    let geo = windows::window_rect(root, rw, rh);

    let mut visited: Vec<(WlSurface, i32, i32)> = Vec::new();
    with_surface_tree_upward(
        root,
        (0i32, 0i32),
        |_s, data, parent_off| {
            let (dx, dy) = surface::subsurface_offset(data);
            TraversalAction::DoChildren((parent_off.0 + dx, parent_off.1 + dy))
        },
        |s, data, parent_off| {
            // Recomputed rather than read off the filter's return
            // value, which is the offset produced for the *parent* —
            // the same subtlety `windows::composite_and_emit` documents.
            let (dx, dy) = surface::subsurface_offset(data);
            visited.push((s.clone(), parent_off.0 + dx, parent_off.1 + dy));
        },
        |_s, _data, _off| true,
    );

    // Reverse order: the walk visits parents before children, which is
    // back-to-front stacking, so the last surface containing the point
    // is the one on top and the one the user aimed at.
    for (s, ox, oy) in visited.iter().rev() {
        let Some(sb) = surfaces.get(&s.id()) else {
            continue;
        };
        let (px, py) = (ox - geo.x, oy - geo.y);
        if wx >= px && wy >= py && wx < px + sb.width && wy < py + sb.height {
            return Hit {
                surface: s.clone(),
                sx: (wx - px) as f64,
                sy: (wy - py) as f64,
            };
        }
    }

    // Nothing covers the point — possible when a client's subsurfaces
    // don't tile its geometry, or during a resize when the browser's
    // idea of the window is a frame ahead of ours. Address the root
    // anyway rather than dropping the event: a click that lands
    // slightly outside a surface is far better than a click that
    // vanishes, and clients cope with out-of-bounds pointer
    // coordinates (they get them during every drag).
    Hit {
        surface: root.clone(),
        sx: (wx + geo.x) as f64,
        sy: (wy + geo.y) as f64,
    }
}

fn wl_axis(axis: ScrollAxis) -> Axis {
    match axis {
        ScrollAxis::Vertical => Axis::VerticalScroll,
        ScrollAxis::Horizontal => Axis::HorizontalScroll,
    }
}

// ---------------------------------------------------------------
// keyboard
// ---------------------------------------------------------------

fn key(state: &mut State, id: &ObjectId, keycode: u32, mask: u16, pressed: bool) {
    // A key for a window we don't consider focused means the frontend
    // and the compositor disagree; the frontend is authoritative, since
    // it is the thing the user is actually looking at.
    focus_window(state, id);

    // Order matters and is not obvious: the synthetic modifiers must
    // land *before* the key they modify, or the client applies the old
    // modifier mask to it and produces the unshifted character.
    reconcile_modifiers(state, mask, Some(keycode));

    state.modifiers.observe(keycode, pressed);
    state.seat.keyboard_update_xkb(keycode, pressed);
    state.seat.keyboard_key(keycode, key_state(pressed));

    // A synthesised modifier is scoped to the character it was
    // invented for, so it is lifted the moment that character is
    // released. Otherwise it stays down until some later event happens
    // to arrive with the bit clear — and a user who types `:` and then
    // stops gets a client sitting with Shift latched, which shows up
    // minutes later as a spuriously capitalised keystroke.
    //
    // Skipped when the key *is* a modifier: its own release is what
    // lifts it, and `observe` has already recorded that.
    if !pressed && translate::modifier_bit_for_keycode(keycode).is_none() {
        let lifted = state.modifiers.release_synthetic();
        for k in lifted {
            trace!(keycode = k.keycode, "lifting synthesised modifier");
            state.seat.keyboard_update_xkb(k.keycode, false);
            state.seat.keyboard_key(k.keycode, KeyState::Released);
        }
    }
}

/// Emit whatever synthetic modifier key events the frontend's mask
/// implies but its key stream never delivered.
fn reconcile_modifiers(state: &mut State, mask: u16, about_to_send: Option<u32>) {
    // Computed first so the `&mut state.modifiers` borrow is over
    // before `state.seat` is touched.
    let synthetic = state.modifiers.reconcile(mask, about_to_send);
    for k in synthetic {
        trace!(
            keycode = k.keycode,
            pressed = k.pressed,
            "synthesising modifier"
        );
        state.seat.keyboard_update_xkb(k.keycode, k.pressed);
        state.seat.keyboard_key(k.keycode, key_state(k.pressed));
    }
}

fn key_state(pressed: bool) -> KeyState {
    if pressed {
        KeyState::Pressed
    } else {
        KeyState::Released
    }
}

// ---------------------------------------------------------------
// focus and window management
// ---------------------------------------------------------------

/// Make `id` the focused window, if it is a toplevel and isn't already.
///
/// Popups are skipped deliberately. `windows::set_focus` resolves its
/// argument against the *toplevel* list, so handing it a popup would
/// resolve to no surface and unfocus the keyboard entirely — clicking a
/// menu item would take focus away from the application that opened it.
fn focus_window(state: &mut State, id: &ObjectId) {
    let is_toplevel = state
        .windows
        .entries
        .get(id)
        .is_some_and(|e| e.kind == WindowKind::Toplevel);
    if !is_toplevel || state.windows.focused.as_ref() == Some(id) {
        return;
    }
    windows::set_focus(state, Some(id.clone()));
}

/// Apply a titlebar action from the frontend.
fn manage(state: &mut State, id: &ObjectId, action: WindowWmState) {
    let Some(toplevel) = toplevel_for(state, id) else {
        trace!("window management action for a non-toplevel window; dropping");
        return;
    };

    match action {
        // Graceful close: `xdg_toplevel.close` is a *request*, and a
        // client with unsaved work is entitled to ignore it. The window
        // disappears when the client destroys it, not here.
        WindowWmState::Close => {
            toplevel.send_close();
            return;
        }
        WindowWmState::Maximized => {
            let size = state.output.size();
            toplevel.with_pending_state(|s| {
                s.size = Some(size);
                s.states.set(xdg_toplevel::State::Maximized);
                s.states.unset(xdg_toplevel::State::Fullscreen);
            });
            toplevel.send_pending_configure();
        }
        WindowWmState::Fullscreen => {
            let size = state.output.size();
            toplevel.with_pending_state(|s| {
                s.size = Some(size);
                s.states.set(xdg_toplevel::State::Fullscreen);
                s.states.unset(xdg_toplevel::State::Maximized);
            });
            toplevel.send_pending_configure();
        }
        WindowWmState::Normal => {
            // `None` hands sizing back to the client: we never recorded
            // a pre-maximize geometry because the frontend owns window
            // placement and size.
            toplevel.with_pending_state(|s| {
                s.size = None;
                s.states.unset(xdg_toplevel::State::Maximized);
                s.states.unset(xdg_toplevel::State::Fullscreen);
            });
            toplevel.send_pending_configure();
            // Restoring from minimized has to put the window back on
            // the canvas; nothing else will, because the client never
            // knew it was hidden. See `windows::emit_minimized`.
            if state
                .windows
                .entries
                .get(id)
                .is_some_and(|e| e.wm_state == WindowWmState::Minimized)
            {
                windows::emit_restored(state, id);
            }
        }
        WindowWmState::Minimized => {
            windows::emit_minimized(state, id);
        }
    }

    windows::emit_state(state, id, action);
}

// ---------------------------------------------------------------
// lookups
// ---------------------------------------------------------------

fn toplevel_for(state: &State, id: &ObjectId) -> Option<ToplevelSurface> {
    state
        .xdg_state
        .toplevel_surfaces()
        .iter()
        .find(|t| t.wl_surface().id() == *id)
        .cloned()
}

/// The `wl_surface` at the root of the addressed window's tree —
/// toplevel or popup, since the browser addresses both.
fn root_surface_for(state: &State, id: &ObjectId) -> Option<WlSurface> {
    state
        .xdg_state
        .toplevel_surfaces()
        .iter()
        .map(|t| t.wl_surface())
        .chain(
            state
                .xdg_state
                .popup_surfaces()
                .iter()
                .map(|p| p.wl_surface()),
        )
        .find(|s| s.id() == *id)
        .cloned()
}
