//! Remaining small helpers on ClientState: INCR transfers, window UUIDs,
//! event broadcasting, atom interning.

use uuid::Uuid;

use super::ClientState;

impl ClientState {
    /// Remove INCR transfers that have been inactive for longer than `timeout`.
    /// Per X11 spec, stale incremental selection transfers should be cleaned up
    /// if the requestor stops consuming chunks.
    pub(crate) fn cleanup_stale_incr_transfers(&mut self, timeout: std::time::Duration) {
        self.incr_transfers.retain(|t| t.last_activity.elapsed() < timeout);
    }

    /// Add a new INCR transfer, enforcing a maximum limit.
    /// If the limit is reached, stale transfers are cleaned up first.
    /// Returns false if the transfer could not be added (limit still exceeded after cleanup).
    #[allow(dead_code)]
    pub(crate) fn push_incr_transfer(&mut self, transfer: super::super::types::IncrTransfer) -> bool {
        const MAX_INCR_TRANSFERS: usize = 100;
        if self.incr_transfers.len() >= MAX_INCR_TRANSFERS {
            self.cleanup_stale_incr_transfers(std::time::Duration::from_secs(5));
        }
        if self.incr_transfers.len() >= MAX_INCR_TRANSFERS {
            return false; // Still at limit after cleanup
        }
        self.incr_transfers.push(transfer);
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
        self.window_router.register(&uuid, x11_wid, &self.message_tx);
        self.event_router.register(x11_wid, &self.wm_events_tx);
        self.menu_tracker
            .window_index()
            .register(x11_wid, uuid.clone(), self.client_id.clone());
        uuid
    }

    /// Get the UUID for a window. Returns None if the window was never registered.
    pub(crate) fn window_uuid(&self, x11_wid: u32) -> Option<String> {
        self.x11_to_uuid.get(&x11_wid).cloned()
    }

    /// Broadcast an event to other connections that have selected the given
    /// event mask on the specified window.
    pub(crate) fn broadcast_event(&self, window_id: u32, event_mask_bit: u32, event: &[u8]) {
        self.event_broadcaster.broadcast(window_id, event_mask_bit, event, &self.client_id);
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

    /// Walk the parent chain from `x11_wid` to find the nearest top-level
    /// window that has a registered UUID.
    pub(crate) fn top_level_uuid_for(&self, x11_wid: u32) -> Option<String> {
        if x11_wid == 0 || x11_wid == self.root_window {
            return None;
        }
        let mut current = x11_wid;
        for _ in 0..128 {
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
        self.atoms.lock().unwrap().get_name(atom).map(|s| s.to_string())
    }
}
