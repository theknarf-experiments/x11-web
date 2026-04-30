//! UUID → window route table.
//!
//! The enumerator owns the writer side: every time a CGWindow is
//! created, configured, or destroyed it updates the entry under that
//! window's UUID. The WS recv loop is the reader: when a frontend
//! `InputEvent` arrives addressed to a UUID, the dispatcher looks up
//! the route to learn the target pid and the window's screen origin
//! (so window-local browser coordinates can be translated back into
//! screen-space points for `CGEventCreateMouseEvent`).
//!
//! Shape mirrors the X11 sidecar's `WindowRouter` in spirit but is
//! intentionally simpler: macOS windows aren't multi-client and we
//! don't need per-event channels — the input path is synchronous.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use core_graphics::window::CGWindowID;

use crate::windows::WindowBounds;

/// What the input dispatcher needs to know about a window to route a
/// click into it.
#[derive(Debug, Clone, Copy)]
pub struct WindowRoute {
    pub cg_id: CGWindowID,
    pub pid: i32,
    /// Window's bounding rect in screen points (top-left origin).
    /// `bounds.x/y` is the screen-point offset we add to the
    /// window-local coordinates a browser frontend sends.
    pub bounds: WindowBounds,
}

#[derive(Clone, Default)]
pub struct WindowRouter {
    inner: Arc<RwLock<HashMap<String, WindowRoute>>>,
}

impl WindowRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, uuid: String, route: WindowRoute) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(uuid, route);
        }
    }

    pub fn update_bounds(&self, uuid: &str, bounds: WindowBounds) {
        if let Ok(mut g) = self.inner.write() {
            if let Some(r) = g.get_mut(uuid) {
                r.bounds = bounds;
            }
        }
    }

    pub fn remove(&self, uuid: &str) {
        if let Ok(mut g) = self.inner.write() {
            g.remove(uuid);
        }
    }

    pub fn lookup(&self, uuid: &str) -> Option<WindowRoute> {
        self.inner.read().ok().and_then(|g| g.get(uuid).copied())
    }
}
