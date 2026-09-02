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

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use smithay::reexports::calloop;
use x11_web_protocol::InputEvent;

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
    }
}
