// Derived from waylandcraft — https://github.com/EVV1E/waylandcraft
// Upstream file:   native/src/seat.rs
// Upstream commit: 233d1431e6acbad1d0c47dfba44d971ce0cebfe8
// GPLv3 — see crates/wayland-server/NOTICE
//
// Changed from upstream: the trailing ~300 lines (zwp_relative_pointer,
// zwp_pointer_constraints, wp_cursor_shape) are dropped — out of scope
// for a browser-driven compositor that never grabs or warps a pointer,
// and they carry the only `let` chains in the file, which would force
// edition 2024 on a workspace that is uniformly 2021. `create_globals`
// therefore advertises wl_seat and nothing else: a global whose
// `Dispatch` impl no longer exists is worse than a missing one, since
// clients bind it and then die on the first request.
//
// Three fixes on top of the port:
//   * `wl_seat.release` used to fall into upstream's catch-all arm and
//     get answered with a `missing_capability` protocol error, which
//     kills any client that tidies up its seat. It is a no-op now, and
//     only `get_touch` (a capability we really do not advertise)
//     errors.
//   * `keyboard.key` sent `key - 8` on a `u32`, which panics in debug
//     and wraps in release for any keycode below 8. It goes through
//     `translate::x11_keycode_to_evdev` now.
//   * `wl_keyboard.enter` carried X11 keycodes in its `keys` array
//     while `wl_keyboard.key` carried evdev ones — the two must be in
//     the same space, so the array is converted like everything else.
//
// The keymap is also RMLVO-configurable from the environment rather
// than hardcoded to the system default, because the container this
// runs in has no `/etc/default/keyboard` for xkbcommon to read.

use std::collections::HashSet;
use std::ffi::CString;
use std::io;
use std::ops::DerefMut;
use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_keyboard::{
    self, KeyState, KeymapFormat, WlKeyboard,
};
use smithay::reexports::wayland_server::protocol::wl_pointer::{
    self, Axis, AxisSource, ButtonState, WlPointer,
};
use smithay::reexports::wayland_server::protocol::wl_seat::{self, WlSeat};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::utils::SealedFile;
use tracing::{debug, trace, warn};
use xkbcommon::xkb::{self, Keymap};

use crate::state::State;
use crate::translate;
use crate::utils::{get_time, new_serial};

/// The compositor's single seat: one pointer, one keyboard, no touch.
///
/// Hand-rolled rather than smithay's `SeatState`, following upstream.
/// smithay's version is built around a real input backend feeding
/// `PointerHandle`/`KeyboardHandle` with focus derived from a scene
/// graph; here focus is dictated by the browser (it tells us which
/// window an event is for) and there is no scene graph at all. Driving
/// the protocol objects directly is both shorter and closer to what
/// actually happens.
pub(crate) struct SeatState {
    /// Every `wl_pointer` and `wl_keyboard` any client has created.
    /// Events are broadcast across these and filtered by focus, which
    /// is why focus lives in the per-object data rather than here.
    pointers: Vec<WlPointer>,
    keyboards: Vec<WlKeyboard>,
    /// Whether the seat's keyboard is "live". Defaults to **false**
    /// upstream and every key is silently swallowed until it is set —
    /// `server::compositor_thread` calls [`activate_keyboard`] once,
    /// and that call is load-bearing.
    kb_active: bool,
    /// The seat's single focused surface, mirrored here as well as in
    /// each `KeyboardData`.
    ///
    /// The per-object copy alone is not enough: it only exists on the
    /// `wl_keyboard`s that were live at the moment focus moved, so a
    /// keyboard created *after* its client's window was focused starts
    /// with `focus: None` and — since `keyboard_key` skips a keyboard
    /// with no focus — silently receives no keys at all until focus
    /// leaves and comes back. Clients that create the keyboard lazily
    /// rather than on `wl_seat.capabilities` (SDL, Electron/Ozone), and
    /// any client that does `release` + `get_keyboard` mid-session, hit
    /// exactly that. `GetKeyboard` consults this field and sends the
    /// missing `enter` itself.
    focus: Option<WlSurface>,
    /// X11 keycodes currently down, so a client that takes focus
    /// mid-chord learns about it from `wl_keyboard.enter`.
    pressed_keys: HashSet<u32>,
    /// The sealed memfd handed to every `wl_keyboard`. Held for the
    /// life of the seat: `wl_keyboard.keymap` passes the fd by
    /// duplication, so closing it early would only break *future*
    /// bindings — which is exactly the kind of bug that shows up as
    /// "the fifth application to start has no keyboard".
    keymap_file: SealedFile,
    xkb_state: xkb::State,
}

/// Per-`wl_pointer` state.
struct PointerData {
    /// Surface holding pointer focus. Always owned by the same client
    /// as the `wl_pointer` itself — the protocol has no way to describe
    /// another client's surface to this one.
    focus: Option<WlSurface>,
    /// Serial of the last `enter`. `wl_pointer.set_cursor` is only
    /// valid against it; anything else is a stale request from before
    /// the pointer moved away and must be ignored.
    last_enter: Option<u32>,
    /// Last position sent, in wl_fixed units, so a mouse that reports
    /// at 1 kHz doesn't produce 1000 identical motion events per second
    /// for a pointer that hasn't actually moved a subpixel.
    last_motion: Option<(i32, i32)>,
}

type PointerRef = Arc<Mutex<PointerData>>;

/// Per-`wl_keyboard` state.
struct KeyboardData {
    focus: Option<WlSurface>,
}

type KeyboardRef = Arc<Mutex<KeyboardData>>;

/// An xkb RMLVO keymap specification.
#[derive(Debug, Default)]
struct Rmlvo {
    rules: String,
    model: String,
    layout: String,
    variant: String,
    options: String,
}

impl Rmlvo {
    /// Read the standard `XKB_DEFAULT_*` variables, defaulting the
    /// layout to `us`.
    ///
    /// libxkbcommon reads these itself when handed empty names, so this
    /// looks redundant — it is not. In a `debian:bookworm-slim`
    /// container there is no `/etc/default/keyboard`, so xkbcommon's
    /// "system default" resolution has nothing to resolve against and
    /// the layout it lands on is a build-time constant of the library.
    /// Naming `us` explicitly makes the result the same everywhere, and
    /// makes it visible in the log — a compositor whose keymap silently
    /// differs from the browser's produces wrong *characters* for right
    /// keycodes, which reads as a font or encoding bug.
    fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let layout = match var("XKB_DEFAULT_LAYOUT") {
            l if l.is_empty() => "us".to_string(),
            l => l,
        };
        Self {
            rules: var("XKB_DEFAULT_RULES"),
            model: var("XKB_DEFAULT_MODEL"),
            layout,
            variant: var("XKB_DEFAULT_VARIANT"),
            options: var("XKB_DEFAULT_OPTIONS"),
        }
    }
}

/// Serialise a keymap into a memfd the client can only ever read.
///
/// `wl_keyboard.keymap` hands over a file descriptor, and the client
/// mmaps it. Sealing is what stops a client mapping it writable and
/// corrupting the keymap for every other client sharing the fd.
fn create_keymap_file(keymap: &Keymap) -> io::Result<SealedFile> {
    let keymap_str = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
    let contents = CString::new(keymap_str).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("keymap has a NUL: {e}"))
    })?;
    SealedFile::with_content(c"x11-web-keymap", &contents)
}

impl SeatState {
    pub(crate) fn new() -> io::Result<Self> {
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let desc = Rmlvo::from_env();

        // Fall back to the all-empty (pure system default) spec if the
        // configured one doesn't compile. A typo in XKB_DEFAULT_LAYOUT
        // should degrade to "keys work, in a layout you didn't ask
        // for", not to "the sidecar refuses to start".
        let keymap = compile(&xkb_context, &desc).or_else(|| {
            warn!(
                ?desc,
                "xkb keymap did not compile; falling back to the system default"
            );
            compile(&xkb_context, &Rmlvo::default())
        });
        let Some(keymap) = keymap else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no xkb keymap could be compiled — is xkb-data installed?",
            ));
        };
        debug!(layout = %desc.layout, "xkb keymap compiled");

        // Both of these take their own reference to the compiled
        // keymap (`xkb_state_new` and the serialised text respectively),
        // so neither the `Keymap` nor the `Context` needs to outlive
        // this function. There is no runtime keymap switching in this
        // slice — upstream's `change_keymap_*` family existed to follow
        // the Android IME's layout, which has no analogue here.
        let xkb_state = xkb::State::new(&keymap);
        let keymap_file = create_keymap_file(&keymap)?;
        drop(xkb_context);

        Ok(SeatState {
            pointers: Vec::new(),
            keyboards: Vec::new(),
            kb_active: false,
            focus: None,
            pressed_keys: HashSet::new(),
            keymap_file,
            xkb_state,
        })
    }

    /// Advertise `wl_seat` — and only `wl_seat`. See the file header.
    pub(crate) fn create_globals(&self, disp: &DisplayHandle) {
        // Version 8: the first with `axis_value120`, which is how a
        // modern client is told "one wheel notch" rather than having to
        // infer it from a continuous scroll delta.
        disp.create_global::<State, WlSeat, ()>(8, ());
    }

    // ---- pointer ------------------------------------------------

    /// `wl_pointer.frame` groups the events since the last frame into
    /// one atomic update. Clients from v5 on are entitled to assume
    /// nothing takes effect until it arrives, so every event-emitting
    /// method below ends with one.
    fn pointer_frame(&self, pointer: &WlPointer) {
        if pointer.version() >= wl_pointer::EVT_FRAME_SINCE {
            pointer.frame();
        }
    }

    /// Move pointer focus to `surface` (or clear it), emitting the
    /// `leave`/`enter` pair that Wayland requires.
    ///
    /// The two passes are not one loop: every pointer that is focused
    /// on the *wrong* surface has to leave before any pointer enters
    /// the new one, or a client that owns both surfaces can observe an
    /// enter and a leave in the wrong order.
    fn pointer_focus(&mut self, surface: Option<&WlSurface>, x: f64, y: f64) {
        let serial = new_serial();

        self.for_all_pointers(|pointer, data| {
            let Some(focus) = &data.focus else {
                return;
            };
            let unfocus = match surface {
                Some(s) => s != focus,
                None => true,
            };
            if unfocus {
                pointer.leave(serial, focus);
                self.pointer_frame(pointer);
                data.focus = None;
                data.last_enter = None;
                data.last_motion = None;
            }
        });

        let Some(surface) = surface else {
            return;
        };

        self.for_all_pointers(|pointer, data| {
            // Already where it should be.
            if data.focus.as_ref().is_some_and(|s| s == surface) {
                return;
            }
            // A pointer can only ever be told about its own client's
            // surfaces; for anyone else this is simply not their event.
            if surface.client() != pointer.client() {
                return;
            }

            pointer.enter(serial, surface, x, y);
            self.pointer_frame(pointer);
            data.focus = Some(surface.clone());
            data.last_enter = Some(serial);
            data.last_motion = None;
        });
    }

    /// Focus `surface` and move the pointer to `(x, y)` within it, in
    /// surface-local coordinates.
    ///
    /// This is the entry point the browser drives: it always knows
    /// where the pointer is, so focus and motion are one operation.
    pub(crate) fn pointer_motion_focus(&mut self, surface: Option<&WlSurface>, x: f64, y: f64) {
        // A surface can die between the frontend deciding to address it
        // and the event arriving here — one round trip through the
        // backend is plenty of time for a client to exit.
        let surface = surface.filter(|s| s.is_alive());

        self.pointer_focus(surface, x, y);
        if surface.is_none() {
            return;
        }
        self.pointer_motion(x, y);
    }

    fn pointer_motion(&mut self, x: f64, y: f64) {
        let time = get_time();
        // Deduplicate in wl_fixed's own resolution (1/256 px): the
        // browser sends integer canvas coordinates, so anything finer
        // would compare values that can never differ.
        let pos = ((x * 256.0) as i32, (y * 256.0) as i32);
        self.for_all_pointers(|pointer, data| {
            if data.focus.is_none() || data.last_motion == Some(pos) {
                return;
            }
            pointer.motion(time, x, y);
            self.pointer_frame(pointer);
            data.last_motion = Some(pos);
        });
    }

    /// Press or release one evdev button code (`BTN_LEFT` &c.).
    pub(crate) fn pointer_button(&self, button: u32, state: ButtonState) {
        let serial = new_serial();
        self.for_all_pointers(|pointer, data| {
            if data.focus.is_none() {
                return;
            }
            pointer.button(serial, get_time(), button, state);
            self.pointer_frame(pointer);
        });
    }

    /// Scroll by `value` notches on `axis`.
    ///
    /// Three encodings of the same scroll, because clients pick the
    /// best one their version supports: `axis_value120` (v5+, 120 units
    /// per notch, the modern high-resolution form), `axis_discrete`
    /// (v5-7, whole notches), and the continuous `axis` every version
    /// understands. `value * 10.0` for the continuous form is upstream's
    /// figure and matches what libinput reports for one detent.
    pub(crate) fn pointer_axis(&self, axis: Axis, value: f64) {
        let val120 = (value * 120.0).floor() as i32;
        if val120 == 0 {
            return;
        }

        self.for_all_pointers(|pointer, data| {
            if data.focus.is_none() {
                return;
            }
            let version = pointer.version();
            if version >= wl_pointer::EVT_AXIS_SOURCE_SINCE {
                // Declaring the source as a wheel (rather than a
                // touchpad) is what stops toolkits applying kinetic
                // scrolling to what is a discrete browser wheel event.
                pointer.axis_source(AxisSource::Wheel);
            }
            if version >= wl_pointer::EVT_AXIS_VALUE120_SINCE {
                pointer.axis_value120(axis, val120);
            } else if version >= wl_pointer::EVT_AXIS_DISCRETE_SINCE {
                pointer.axis_discrete(axis, value.floor() as i32);
            }
            pointer.axis(get_time(), axis, value * 10.0);
            self.pointer_frame(pointer);
        });
    }

    // ---- keyboard -----------------------------------------------

    /// Feed a key into the xkb state machine and the held-key set.
    ///
    /// Separate from [`keyboard_key`](Self::keyboard_key) because the
    /// xkb state has to be updated *before* the event is sent: the
    /// `modifiers` event that follows a key is derived from it, and a
    /// client that receives `Shift` down with the pre-Shift modifier
    /// mask will happily produce a lowercase letter.
    pub(crate) fn keyboard_update_xkb(&mut self, key: u32, pressed: bool) {
        let dir = match pressed {
            true => xkb::KeyDirection::Down,
            false => xkb::KeyDirection::Up,
        };
        self.xkb_state.update_key(xkb::Keycode::new(key), dir);

        if pressed {
            self.pressed_keys.insert(key);
        } else {
            self.pressed_keys.remove(&key);
        }
    }

    /// Give keyboard focus to `surface`.
    ///
    /// Every keyboard belonging to another client is made to leave
    /// first — Wayland allows exactly one focused surface per seat, and
    /// a client left holding focus keeps acting on keys it is no longer
    /// being sent.
    pub(crate) fn keyboard_focus(&mut self, surface: &WlSurface) {
        if !surface.is_alive() {
            return;
        }
        let Some(client) = surface.client() else {
            return;
        };
        let serial = new_serial();

        self.for_all_keyboards(|keyboard, data| {
            let Some(keyboard_client) = keyboard.client() else {
                return;
            };

            if keyboard_client != client {
                if let Some(focus) = data.focus.take() {
                    // Only if it is still alive: `wl_keyboard.leave`
                    // carries the surface as an argument, and naming a
                    // destroyed object is a protocol error on the
                    // client side. Reachable because focus is moved
                    // *from* a window as it is being torn down.
                    if focus.is_alive() {
                        keyboard.leave(serial, &focus);
                    }
                }
                return;
            }

            // Same client from here on, so the surface is one this
            // keyboard is allowed to be told about.
            if let Some(focus) = data.focus.take() {
                if &focus == surface {
                    data.focus = Some(focus);
                    return;
                }
                if focus.is_alive() {
                    keyboard.leave(serial, &focus);
                }
            }

            keyboard.enter(serial, surface, self.serialize_pressed_keys());
            data.focus = Some(surface.clone());
            self.send_modifiers(keyboard, serial);
        });
        self.focus = Some(surface.clone());
    }

    pub(crate) fn keyboard_unfocus(&mut self) {
        let serial = new_serial();
        self.for_all_keyboards(|keyboard, data| {
            if let Some(focus) = data.focus.take() {
                // See `keyboard_focus`: the surface may already be dead.
                if focus.is_alive() {
                    keyboard.leave(serial, &focus);
                }
            }
        });
        self.focus = None;
    }

    /// Send one key event to whoever holds keyboard focus.
    ///
    /// `key` is an **X11** keycode, the same space the frontend sends
    /// and the same one [`keyboard_update_xkb`](Self::keyboard_update_xkb)
    /// takes; the evdev conversion happens here, once, at the wire.
    pub(crate) fn keyboard_key(&self, key: u32, state: KeyState) {
        if !self.kb_active {
            return;
        }
        let Some(evdev) = translate::x11_keycode_to_evdev(key) else {
            warn!(
                key,
                "dropping key event: keycode is below the X11/evdev offset"
            );
            return;
        };
        let serial = new_serial();
        self.for_all_keyboards(|keyboard, data| {
            if data.focus.is_none() {
                return;
            }
            keyboard.key(serial, get_time(), evdev, state);
            // After, not before: the modifier mask a client applies to
            // a key is the one in effect *including* that key.
            self.send_modifiers(keyboard, serial);
        });
    }

    /// The `keys` array of `wl_keyboard.enter`: the keycodes already
    /// held when focus arrives, so a client entering mid-chord doesn't
    /// think the keyboard is idle.
    ///
    /// Upstream serialised the raw X11 keycodes here while
    /// `wl_keyboard.key` carried evdev ones. The two are the same array
    /// space by protocol definition, so this converts.
    fn serialize_pressed_keys(&self) -> Vec<u8> {
        if !self.kb_active {
            return Vec::new();
        }
        self.pressed_keys
            .iter()
            .filter_map(|&k| translate::x11_keycode_to_evdev(k))
            .flat_map(|k| k.to_ne_bytes())
            .collect()
    }

    /// Re-deliver `leave` + `enter` to whoever has focus.
    ///
    /// Needed whenever the *content* of a focus changes rather than its
    /// target: activating the keyboard, or swapping the keymap. There
    /// is no protocol event for "your held-keys array is now different",
    /// so the focus is cycled to resend it.
    fn keyboard_refocus(&mut self) {
        let serial = new_serial();
        self.for_all_keyboards(|keyboard, data| {
            let Some(focus) = &data.focus else {
                return;
            };
            if !focus.is_alive() {
                return;
            }
            keyboard.leave(serial, focus);
            keyboard.enter(serial, focus, self.serialize_pressed_keys());
            self.send_modifiers(keyboard, serial);
        });
    }

    /// Make the keyboard live. **Must be called once at startup.**
    ///
    /// `kb_active` exists upstream so Minecraft can hand the keyboard
    /// back and forth between the game and the guest compositor. Here
    /// there is nothing to hand it to, but the flag is kept (rather
    /// than deleted) because it is what makes `keyboard_key` a no-op
    /// before the event loop is up — and because forgetting the call is
    /// a failure with no symptom at all: keys are accepted, routed,
    /// translated, and dropped one line before the wire.
    pub(crate) fn activate_keyboard(&mut self) {
        if self.kb_active {
            return;
        }
        self.kb_active = true;
        self.keyboard_refocus();
    }

    /// Publish the current xkb modifier state.
    ///
    /// This is the *only* way a Wayland client learns that Shift is
    /// down — there is no per-event modifier field like X11's `state`
    /// mask. See `translate::ModifierSynth` for the consequences.
    fn send_modifiers(&self, keyboard: &WlKeyboard, serial: u32) {
        let layout = self.xkb_state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        if !self.kb_active {
            // Inactive keyboard: report no modifiers held, whatever the
            // xkb state thinks, so a client can't be left believing
            // Ctrl is stuck down.
            keyboard.modifiers(serial, 0, 0, 0, layout);
            return;
        }
        keyboard.modifiers(
            serial,
            self.xkb_state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            self.xkb_state.serialize_mods(xkb::STATE_MODS_LATCHED),
            self.xkb_state.serialize_mods(xkb::STATE_MODS_LOCKED),
            layout,
        );
    }

    // ---- iteration ----------------------------------------------

    fn for_all_pointers<F>(&self, mut f: F)
    where
        F: FnMut(&WlPointer, &mut PointerData),
    {
        for pointer in &self.pointers {
            let Some(cell) = pointer.data::<PointerRef>() else {
                continue;
            };
            let Ok(mut guard) = cell.lock() else {
                continue;
            };
            f(pointer, guard.deref_mut());
        }
    }

    fn for_all_keyboards<F>(&self, mut f: F)
    where
        F: FnMut(&WlKeyboard, &mut KeyboardData),
    {
        for keyboard in &self.keyboards {
            let Some(cell) = keyboard.data::<KeyboardRef>() else {
                continue;
            };
            let Ok(mut guard) = cell.lock() else {
                continue;
            };
            f(keyboard, guard.deref_mut());
        }
    }
}

/// Compile an RMLVO spec, or `None` if xkb rejects it.
fn compile(context: &xkb::Context, desc: &Rmlvo) -> Option<Keymap> {
    Keymap::new_from_names(
        context,
        &desc.rules,
        &desc.model,
        &desc.layout,
        &desc.variant,
        // `Some("")` and `None` are not the same to xkbcommon: the
        // former asks for *no* options, the latter for the default set.
        if desc.options.is_empty() {
            None
        } else {
            Some(desc.options.clone())
        },
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
}

impl GlobalDispatch<WlSeat, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<WlSeat>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat: WlSeat = data_init.init(resource, ());
        if seat.version() >= wl_seat::EVT_NAME_SINCE {
            seat.name("x11-web-seat".into());
        }

        // Pointer and keyboard, never touch. Advertising a capability
        // we then refuse in `get_touch` would be a protocol violation;
        // not advertising it makes toolkits take their mouse path,
        // which is the one the browser actually drives.
        let mut caps = wl_seat::Capability::empty();
        caps.insert(wl_seat::Capability::Pointer);
        caps.insert(wl_seat::Capability::Keyboard);
        seat.capabilities(caps);
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        seat_resource: &WlSeat,
        request: wl_seat::Request,
        _data: &(),
        _disp: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                let data = Arc::new(Mutex::new(PointerData {
                    focus: None,
                    last_enter: None,
                    last_motion: None,
                }));
                let pointer: WlPointer = data_init.init(id, data);
                state.seat.pointers.push(pointer);
            }
            wl_seat::Request::GetKeyboard { id } => {
                let data = Arc::new(Mutex::new(KeyboardData { focus: None }));
                let keyboard: WlKeyboard = data_init.init(id, data);
                state.seat.keyboards.push(keyboard.clone());

                // The keymap must be the very first thing a keyboard
                // receives: every subsequent `key` is meaningless
                // without it, and clients that get a key first tend to
                // treat the keyboard as broken rather than waiting.
                let keymap = &state.seat.keymap_file;
                keyboard.keymap(KeymapFormat::XkbV1, keymap.as_fd(), keymap.size() as u32);

                if keyboard.version() >= wl_keyboard::EVT_REPEAT_INFO_SINCE {
                    // 25 keys/s after a 600 ms delay — the X11 default,
                    // and the repeat is the *client's* job: we send one
                    // press and one release, nothing in between.
                    keyboard.repeat_info(25, 600);
                }

                // Catch this keyboard up to the seat's current focus.
                // `keyboard_focus` only ever iterates the keyboards that
                // existed when focus moved, so without this a client
                // whose window is already focused — the normal case for
                // anything that binds wl_seat, maps a window, and only
                // then asks for a keyboard — would sit with
                // `focus: None` forever and `keyboard_key` would skip
                // it. No symptom except "this app ignores the keyboard".
                let focus = state
                    .seat
                    .focus
                    .clone()
                    .filter(|s| s.is_alive() && s.client() == keyboard.client());
                if let Some(surface) = focus {
                    let serial = new_serial();
                    keyboard.enter(serial, &surface, state.seat.serialize_pressed_keys());
                    if let Some(cell) = keyboard.data::<KeyboardRef>() {
                        if let Ok(mut guard) = cell.lock() {
                            guard.focus = Some(surface);
                        }
                    }
                    state.seat.send_modifiers(&keyboard, serial);
                }
            }
            // Not a capability we advertise, so this is the one request
            // that legitimately earns a protocol error.
            wl_seat::Request::GetTouch { .. } => {
                seat_resource.post_error(
                    wl_seat::Error::MissingCapability,
                    "this seat has no touch capability",
                );
            }
            // Upstream let this fall into the catch-all and answered it
            // with `missing_capability`, killing any client that
            // released its seat on shutdown.
            wl_seat::Request::Release => {}
            other => trace!(?other, "unhandled wl_seat request"),
        }
    }
}

impl Dispatch<WlPointer, PointerRef> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _pointer: &WlPointer,
        request: wl_pointer::Request,
        data: &PointerRef,
        _disp: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            // The browser draws its own cursor over the canvas, so a
            // client's cursor surface has nowhere to go. The serial is
            // still validated: answering a stale set_cursor would be
            // wrong even when the answer is "nothing".
            wl_pointer::Request::SetCursor { serial, .. } => {
                let last_enter = data.lock().ok().and_then(|d| d.last_enter);
                if last_enter != Some(serial) {
                    return;
                }
                trace!("ignoring wl_pointer.set_cursor; the browser owns the cursor");
            }
            wl_pointer::Request::Release => {}
            other => trace!(?other, "unhandled wl_pointer request"),
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, pointer: &WlPointer, _data: &PointerRef) {
        state.seat.pointers.retain(|p| p != pointer);
    }
}

impl Dispatch<WlKeyboard, KeyboardRef> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _keyboard: &WlKeyboard,
        request: wl_keyboard::Request,
        _data: &KeyboardRef,
        _disp: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_keyboard::Request::Release => {}
            other => trace!(?other, "unhandled wl_keyboard request"),
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, keyboard: &WlKeyboard, _data: &KeyboardRef) {
        state.seat.keyboards.retain(|k| k != keyboard);
    }
}
