//! Remaining small helpers on ClientState: INCR transfers, window UUIDs,
//! event broadcasting, atom interning.

use uuid::Uuid;
use x11rb_protocol::x11_utils::ByteOrder;

use super::ClientState;

impl ClientState {
    /// Convert the per-client `msb_first` bool into the typed [`ByteOrder`]
    /// expected by `TryParseEndian` / `SerializeEndian`. Use this whenever
    /// driving the generator-emitted endian-aware parse or serialise paths.
    #[inline]
    pub(crate) fn byte_order(&self) -> ByteOrder {
        crate::xserver::reply::byte_order_of(self.msb_first)
    }

    /// Write the pointer position to local and shared state in one shot.
    /// Use this everywhere the pointer moves (XTEST motion, WarpPointer,
    /// barrier clamps, frontend pointer input) so cross-connection clients
    /// querying the position see the up-to-date value.
    #[inline]
    pub(crate) fn set_pointer(&mut self, x: i16, y: i16) {
        self.pointer_x = x;
        self.pointer_y = y;
        if let Ok(mut p) = self.shared_pointer.lock() {
            *p = (x, y);
        }
    }

    /// Pull the latest pointer position from shared state into local.
    /// Call before answering pointer reads (QueryPointer, GetMotionEvents)
    /// so a FakeInput from another client is visible.
    #[inline]
    pub(crate) fn refresh_pointer_from_shared(&mut self) {
        if let Ok(p) = self.shared_pointer.lock() {
            self.pointer_x = p.0;
            self.pointer_y = p.1;
        }
    }

    /// Mirror the new focus window into shared state. Called by
    /// `set_focus_window` after the local update + event fan-out, so
    /// xdotool's `windowfocus` is observable to xterm's later
    /// `GetInputFocus`.
    #[inline]
    pub(crate) fn write_focus_to_shared(&self, focus: u32) {
        if let Ok(mut f) = self.shared_focus.lock() {
            *f = focus;
        }
    }

    /// Pull the latest global focus window from shared state into
    /// local. Used by `GetInputFocus` so the reply reflects any
    /// `SetInputFocus` from another connection.
    #[inline]
    pub(crate) fn refresh_focus_from_shared(&mut self) {
        if let Ok(f) = self.shared_focus.lock() {
            self.focus_window = *f;
        }
    }

    // -----------------------------------------------------------------------
    // Resource limit checks
    // -----------------------------------------------------------------------

    /// Returns true if the client can create another window.
    pub(crate) fn can_create_window(&self) -> bool {
        self.windows.len() < self.resource_limits.max_windows
    }

    /// Returns true if the client can create another pixmap.
    pub(crate) fn can_create_pixmap(&self) -> bool {
        self.pixmaps.len() < self.resource_limits.max_pixmaps
    }

    /// Returns true if the client can create another GC.
    pub(crate) fn can_create_gc(&self) -> bool {
        self.gcs.len() < self.resource_limits.max_gcs
    }

    /// Returns true if the client can create another colormap.
    pub(crate) fn can_create_colormap(&self) -> bool {
        self.colormaps.len() < self.resource_limits.max_colormaps
    }

    /// Returns true if the client can create another cursor.
    pub(crate) fn can_create_cursor(&self) -> bool {
        self.cursors.len() < self.resource_limits.max_cursors
    }

    /// Record a motion history entry, enforcing the max limit.
    pub(crate) fn record_motion_history(&mut self, timestamp: u32, x: i16, y: i16) {
        if self.pointer.motion_history.len() >= self.resource_limits.max_motion_history {
            self.pointer.motion_history.remove(0);
        }
        self.pointer.motion_history.push((timestamp, x, y));
    }

    /// Remove INCR transfers that have been inactive for longer than `timeout`.
    /// Per X11 spec, stale incremental selection transfers should be cleaned up
    /// if the requestor stops consuming chunks.
    pub(crate) fn cleanup_stale_incr_transfers(&mut self, timeout: std::time::Duration) {
        self.selection.incr_transfers
            .retain(|t| t.last_activity.elapsed() < timeout);
    }

    /// Add a new INCR transfer, enforcing a maximum limit.
    /// If the limit is reached, stale transfers are cleaned up first.
    /// Returns false if the transfer could not be added (limit still exceeded after cleanup).
    pub(crate) fn push_incr_transfer(
        &mut self,
        transfer: super::super::types::IncrTransfer,
    ) -> bool {
        const MAX_INCR_TRANSFERS: usize = 100;
        if self.selection.incr_transfers.len() >= MAX_INCR_TRANSFERS {
            self.cleanup_stale_incr_transfers(std::time::Duration::from_secs(5));
        }
        if self.selection.incr_transfers.len() >= MAX_INCR_TRANSFERS {
            return false; // Still at limit after cleanup
        }
        self.selection.incr_transfers.push(transfer);
        true
    }

    // -----------------------------------------------------------------------
    // Window UUID management
    // -----------------------------------------------------------------------

    /// Get or create a UUID for a top-level X11 window.
    pub(crate) fn get_or_create_window_uuid(&mut self, x11_wid: u32) -> String {
        if let Some(uuid) = self.x11_to_uuid.get(&x11_wid) {
            return uuid.clone();
        }
        let uuid = Uuid::new_v4().to_string();
        self.x11_to_uuid.insert(x11_wid, uuid.clone());
        self.window_router
            .register(&uuid, x11_wid, &self.message_tx);
        self.event_router.register(x11_wid, &self.wm_events_tx);
        self.menu.tracker
            .window_index()
            .register(x11_wid, uuid.clone(), self.client_id.clone());
        uuid
    }

    /// Get the UUID for a window. Returns None if the window was never registered.
    pub(crate) fn window_uuid(&self, x11_wid: u32) -> Option<String> {
        self.x11_to_uuid.get(&x11_wid).cloned()
    }

    /// Broadcast an event to other connections that have selected the given
    /// event mask on the specified window. Accepts either a raw u32 bit set
    /// or an `EventMask` (which converts to u32).
    pub(crate) fn broadcast_event(
        &self,
        window_id: u32,
        event_mask_bit: impl Into<u32>,
        event: &[u8],
    ) {
        self.event_broadcaster
            .broadcast(window_id, event_mask_bit.into(), event, &self.client_id);
    }

    /// Returns true if this client has selected `mask` (one or more bits) on `wid`.
    /// Used to gate local delivery of generated events.
    #[inline]
    pub(crate) fn window_selects(&self, wid: u32, mask: crate::xserver::core::EventMask) -> bool {
        self.windows
            .get(&wid)
            .is_some_and(|w| w.event_mask & mask != crate::xserver::core::EventMask::NO_EVENT)
    }

    /// Push `event` to local pending if this client selected `mask` on `window`,
    /// then broadcast to all other clients that selected `mask` on `window`.
    /// Convenience wrapper over the recurring window_selects + broadcast pair.
    pub(crate) fn deliver_event(
        &mut self,
        window: u32,
        mask: crate::xserver::core::EventMask,
        event: &[u8],
    ) {
        if self.window_selects(window, mask) {
            self.pending_events.push(event.to_vec());
        }
        self.broadcast_event(window, mask, event);
    }

    /// Subscribe this client to cross-connection events on a window.
    /// Called when ChangeWindowAttributes sets an event_mask on a window
    /// that this client doesn't own.
    pub(crate) fn subscribe_to_window_events(&self, window_id: u32, event_mask: u32) {
        self.event_broadcaster.subscribe(
            window_id,
            &self.client_id,
            event_mask,
            &self.wm_events_tx,
        );
    }

    /// Walk the parent chain from `x11_wid` up to the immediate child of
    /// the root window — that's the top-level for click-to-focus. Returns
    /// None if `x11_wid` doesn't belong to any window we know about.
    pub(crate) fn top_level_for(&self, x11_wid: u32) -> Option<u32> {
        if x11_wid == 0 || x11_wid == self.root_window {
            return None;
        }
        let mut current = x11_wid;
        for _ in 0..crate::xserver::window_tree::MAX_TREE_DEPTH {
            let parent = self.windows.get(&current).map(|w| w.parent)?;
            if parent == self.root_window || parent == 0 {
                return Some(current);
            }
            current = parent;
        }
        None
    }

    /// Walk the parent chain from `x11_wid` to find the nearest top-level
    /// window that has a registered UUID.
    pub(crate) fn top_level_uuid_for(&self, x11_wid: u32) -> Option<String> {
        if x11_wid == 0 || x11_wid == self.root_window {
            return None;
        }
        let mut current = x11_wid;
        for _ in 0..crate::xserver::window_tree::MAX_TREE_DEPTH {
            if let Some(uuid) = self.x11_to_uuid.get(&current) {
                return Some(uuid.clone());
            }
            match self.windows.get(&current) {
                Some(w) if w.parent != self.root_window && w.parent != 0 => {
                    current = w.parent;
                }
                _ => return None,
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Atom helpers
    // -----------------------------------------------------------------------

    /// Intern an atom name, returning its global ID.
    pub(crate) fn intern_atom(&self, name: &str, only_if_exists: bool) -> u32 {
        self.atoms.lock().unwrap().intern(name, only_if_exists)
    }

    /// Get the name of an atom by its global ID.
    pub(crate) fn get_atom_name(&self, atom: u32) -> Option<String> {
        self.atoms
            .lock()
            .unwrap()
            .get_name(atom)
            .map(|s| s.to_string())
    }
}
