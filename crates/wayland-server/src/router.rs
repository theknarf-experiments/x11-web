//! Embedder → compositor command channel.
//!
//! Mirrors `x11-web-x11-server`'s `WindowRouter`
//! (crates/x11-server/src/xserver/types/routing.rs) so that
//! `crates/sidecar-wayland` reads the same as `crates/sidecar`:
//! `send_input` / `send_resize` take a window UUID, return `bool`
//! synchronously, and never block.
//!
//! Two shared cells, both behind `std::sync::Mutex` (not tokio's —
//! these are locked from synchronous code on both sides, including
//! from inside calloop callbacks where there is no runtime to await
//! on):
//!
//!   * the calloop sender, `None` until the compositor thread has
//!     built its event loop and called [`WindowRouter::install`]. The
//!     `Mutex` is doing double duty here: `calloop::channel::Sender`
//!     is `Send` but not `Sync`, and the router has to be `Sync` to
//!     be cloned into tokio tasks.
//!   * the set of live window UUIDs, so a send to a window that has
//!     been destroyed (or never existed) returns `false` rather than
//!     silently queueing a command the compositor will drop. This is
//!     the same contract the X11 router offers, where an unregistered
//!     UUID simply isn't in the routes map.
//!
//! Commands sent before the compositor is up return `false`. That is
//! deliberate and matches X11: there is no window to address yet, so
//! there is nothing to buffer.
//!
//! ## The third thing this type does: flow control
//!
//! The router is also the *only* `Send + Sync` handle shared by the
//! compositor thread and the embedder, so it is where the two agree on
//! how many frames may be in flight. See
//! [`WindowRouter::frame_shipped`] and [`MAX_FRAMES_IN_FLIGHT`] — the
//! render tick's per-window throttle reads its credit from here, which
//! is what stops a 60 fps client outrunning a WebP encoder and growing
//! the update channel without bound.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use smithay::reexports::calloop;
use x11_web_protocol::InputEvent;

/// How many un-shipped `PutImage`s a single window may have
/// outstanding before the render tick stops producing for it.
///
/// Two, not one: one frame can be in the encoder while the next is
/// queued behind it, so a steady-state client never sees a bubble. One
/// would serialise "encode" against "composite" and halve the ceiling;
/// anything much larger stops being a bound in any useful sense — at
/// 1280x800 a raw RGBA frame is ~4 MB.
pub(crate) const MAX_FRAMES_IN_FLIGHT: u32 = 2;

/// Everything the embedder can ask the compositor to do. Every
/// variant is `Send` — that is the whole point of routing through a
/// channel rather than reaching into compositor state, which is
/// `!Send` and lives on its own thread.
///
pub(crate) enum Command {
    Input {
        window_id: String,
        event: InputEvent,
    },
    Resize {
        window_id: String,
        width: u16,
        height: u16,
    },
    /// Drives wl_output's advertised mode. The X11 side has an
    /// equivalent (`ScreenSizeRx`) that nothing currently writes to;
    /// here it is a plain command because polling a tokio `watch`
    /// from inside a calloop dispatch is pure friction.
    ScreenSize { width: u16, height: u16 },
}

#[derive(Clone)]
pub struct WindowRouter {
    tx: Arc<Mutex<Option<calloop::channel::Sender<Command>>>>,
    live: Arc<Mutex<HashSet<String>>>,
    /// Set by the embedder while something is actually draining the
    /// update channel. False means "produce, but do not queue" — see
    /// [`WindowRouter::set_consuming`].
    consuming: Arc<AtomicBool>,
    /// Per-window count of emitted-but-not-yet-shipped `PutImage`s.
    pending: Arc<Mutex<HashMap<String, u32>>>,
}

impl Default for WindowRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowRouter {
    pub fn new() -> Self {
        Self {
            tx: Arc::new(Mutex::new(None)),
            live: Arc::new(Mutex::new(HashSet::new())),
            consuming: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Declare whether the embedder is currently draining the update
    /// channel — `true` for the lifetime of a backend session, `false`
    /// otherwise.
    ///
    /// This is what makes the flow control below safe rather than a
    /// deadlock waiting to happen. The credit scheme is a round trip:
    /// the compositor stops producing for a window until the embedder
    /// says the last frame went out. With nobody on the other end
    /// (before the first successful dial, between reconnects, or in
    /// `e2e/scripts/wayland-smoke.sh`, which runs the sidecar with no
    /// backend at all) that round trip never completes and every window
    /// would freeze after two frames.
    ///
    /// So while this is `false` the compositor keeps compositing and
    /// keeps releasing frame callbacks — clients animate exactly as
    /// before — but **drops** the resulting updates instead of queueing
    /// them for a reader that does not exist. That is also the correct
    /// answer on its own terms: the pre-existing behaviour was to
    /// accumulate frames, unbounded, for a backend that may never
    /// connect. The compositor notices the false→true edge and forces
    /// full damage on every window, so a session always opens with
    /// complete frames rather than with whatever changed since.
    pub fn set_consuming(&self, on: bool) {
        self.consuming.store(on, Ordering::Release);
    }

    /// Return one window's frame credit, after the update has been
    /// handed to the wire.
    ///
    /// Must be called exactly once per `PutImage` the embedder receives
    /// while consuming, or that window throttles itself to a halt.
    /// Missing the call is not fatal — the next `set_consuming(false)`
    /// resets every counter — but it is a stall until then.
    pub fn frame_shipped(&self, window_uuid: &str) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if let Some(n) = pending.get_mut(window_uuid) {
            *n = n.saturating_sub(1);
        }
    }

    /// Route an input event to the window with this UUID. Returns
    /// `false` if the window is unknown or the compositor isn't up.
    pub fn send_input(&self, window_uuid: &str, event: InputEvent) -> bool {
        self.dispatch(window_uuid, |window_id| Command::Input { window_id, event })
    }

    /// Ask the window's xdg_toplevel to reconfigure to this size.
    /// The size change is only *reported* back once the client has
    /// acked and committed a buffer at the new size.
    pub fn send_resize(&self, window_uuid: &str, width: u16, height: u16) -> bool {
        self.dispatch(window_uuid, |window_id| Command::Resize {
            window_id,
            width,
            height,
        })
    }

    /// Resize the virtual output. Not window-addressed, so it has no
    /// liveness precondition — it only needs the compositor to be up.
    pub fn set_screen_size(&self, width: u16, height: u16) -> bool {
        self.send_command(Command::ScreenSize { width, height })
    }

    fn dispatch(&self, window_uuid: &str, make: impl FnOnce(String) -> Command) -> bool {
        match self.live.lock() {
            Ok(live) if live.contains(window_uuid) => {}
            // A poisoned lock or an unknown UUID are both "no route",
            // which is exactly what the X11 router reports.
            _ => return false,
        }
        self.send_command(make(window_uuid.to_string()))
    }

    fn send_command(&self, cmd: Command) -> bool {
        match self.tx.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(tx) => tx.send(cmd).is_ok(),
                None => false,
            },
            Err(_) => false,
        }
    }

    /// Called once from the compositor thread, after the calloop
    /// source for the receiving half has been inserted.
    pub(crate) fn install(&self, tx: calloop::channel::Sender<Command>) {
        if let Ok(mut guard) = self.tx.lock() {
            *guard = Some(tx);
        }
    }

    /// Called from the compositor thread when a window becomes
    /// addressable (its first buffer commit) and when it goes away.
    pub(crate) fn track(&self, window_uuid: &str) {
        if let Ok(mut live) = self.live.lock() {
            live.insert(window_uuid.to_string());
        }
    }

    pub(crate) fn untrack(&self, window_uuid: &str) {
        if let Ok(mut live) = self.live.lock() {
            live.remove(window_uuid);
        }
        // Otherwise a long session leaks one map entry per window that
        // ever existed, and a UUID could in principle be re-tracked
        // carrying a dead window's credit debt.
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(window_uuid);
        }
    }

    // ---- flow control, compositor side ---------------------------

    pub(crate) fn is_consuming(&self) -> bool {
        self.consuming.load(Ordering::Acquire)
    }

    /// Whether this window has spare credit to emit another frame.
    /// A poisoned lock reads as "yes": dropping frames forever because
    /// of a panic elsewhere is worse than briefly over-producing.
    pub(crate) fn may_emit(&self, window_uuid: &str) -> bool {
        match self.pending.lock() {
            Ok(pending) => pending.get(window_uuid).copied().unwrap_or(0) < MAX_FRAMES_IN_FLIGHT,
            Err(_) => true,
        }
    }

    pub(crate) fn frame_emitted(&self, window_uuid: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending.entry(window_uuid.to_string()).or_insert(0) += 1;
        }
    }

    /// Forget every outstanding credit. Called by the render tick on a
    /// consuming-flag transition, which is what guarantees a dropped
    /// session cannot leave a window permanently throttled.
    pub(crate) fn clear_pending(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the credit scheme: production stops after
    /// `MAX_FRAMES_IN_FLIGHT` and resumes the moment one is shipped.
    #[test]
    fn credit_bounds_production_and_is_returned_by_shipping() {
        let r = WindowRouter::new();
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            assert!(r.may_emit("w"), "should have credit before the cap");
            r.frame_emitted("w");
        }
        assert!(!r.may_emit("w"), "must stall at the cap");
        r.frame_shipped("w");
        assert!(r.may_emit("w"), "shipping one frame must free one slot");
    }

    /// Credit is per window: one busy client must not throttle another.
    #[test]
    fn credit_is_per_window() {
        let r = WindowRouter::new();
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            r.frame_emitted("busy");
        }
        assert!(!r.may_emit("busy"));
        assert!(r.may_emit("idle"));
    }

    /// The safety valve. A session that dies mid-frame leaves credit
    /// outstanding; without this the window would never draw again.
    #[test]
    fn clearing_pending_unsticks_a_stalled_window() {
        let r = WindowRouter::new();
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            r.frame_emitted("w");
        }
        assert!(!r.may_emit("w"));
        r.clear_pending();
        assert!(r.may_emit("w"));
    }

    /// `frame_shipped` on a window with nothing outstanding must not
    /// wrap `u32` into ~4 billion credits.
    #[test]
    fn shipping_more_than_was_emitted_does_not_underflow() {
        let r = WindowRouter::new();
        r.frame_emitted("w");
        r.frame_shipped("w");
        r.frame_shipped("w");
        r.frame_shipped("unknown-window");
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            assert!(r.may_emit("w"));
            r.frame_emitted("w");
        }
        assert!(!r.may_emit("w"), "the cap must still be the cap");
    }

    /// Destroying a window drops its route *and* its credit, so the map
    /// cannot grow for the life of the process.
    #[test]
    fn untrack_forgets_both_the_route_and_the_credit() {
        let r = WindowRouter::new();
        r.track("w");
        r.frame_emitted("w");
        r.untrack("w");
        assert_eq!(r.pending.lock().unwrap().len(), 0);
        assert_eq!(r.live.lock().unwrap().len(), 0);
    }

    /// Nothing is consuming until an embedder says so — which is what
    /// keeps an unconnected sidecar from stalling every window.
    #[test]
    fn consuming_defaults_off_and_round_trips() {
        let r = WindowRouter::new();
        assert!(!r.is_consuming());
        r.set_consuming(true);
        assert!(r.is_consuming());
        r.set_consuming(false);
        assert!(!r.is_consuming());
    }
}
