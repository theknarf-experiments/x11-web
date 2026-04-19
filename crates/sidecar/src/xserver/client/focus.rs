//! Focus and window management helpers for ClientState.

use x11_web_protocol::DisplayUpdate;
use x11rb_protocol::protocol::xproto::{
    ClientMessageData, ClientMessageEvent, FocusInEvent, KeymapNotifyEvent, NotifyDetail,
    NotifyMode,
};

use super::super::core::{
    CLIENT_MESSAGE_EVENT, FOCUS_IN_EVENT, FOCUS_OUT_EVENT, KEYMAP_NOTIFY_EVENT,
};
use super::super::core::EventMask;
use super::super::types::*;
use super::ClientState;
use crate::xserver::event::serialize_event;

impl ClientState {
    /// Revert focus away from the given window (called when it's destroyed or unmapped).
    /// Follows X11 revert-to semantics: None→0, PointerRoot→1, Parent→nearest existing ancestor.
    pub(crate) fn revert_focus_from(&mut self, window: u32) {
        if self.focus_window != window {
            return;
        }
        let new_focus = match self.focus_revert_to {
            0 => 0, // RevertToNone
            1 => 1, // RevertToPointerRoot
            2 => {
                // RevertToParent: walk up the window tree to find the nearest
                // ancestor that still exists (it may have been destroyed too).
                let mut candidate = self
                    .windows
                    .get(&window)
                    .map(|w| w.parent)
                    .unwrap_or(self.root_window);
                for _ in 0..128 {
                    if candidate == 0 || candidate == self.root_window {
                        break;
                    }
                    if self.windows.contains_key(&candidate) {
                        break;
                    }
                    // This ancestor no longer exists; fall back to root.
                    candidate = self.root_window;
                }
                if candidate == 0 {
                    self.root_window
                } else {
                    candidate
                }
            }
            _ => self.root_window,
        };
        self.set_focus_window(new_focus);
    }

    /// Update the focus window and broadcast if changed.
    pub(crate) fn set_focus_window(&mut self, new_focus: u32) {
        let prev_focus = self.focus_window;
        let prev_uuid = self.top_level_uuid_for(prev_focus);
        self.focus_window = new_focus;
        let next_uuid = self.top_level_uuid_for(new_focus);
        if prev_uuid != next_uuid {
            self.broadcast_focus(next_uuid);
        }

        // Update _NET_ACTIVE_WINDOW on the root window (EWMH)
        if prev_focus != new_focus {
            let net_active_atom = self.intern_atom("_NET_ACTIVE_WINDOW", false);
            let data = new_focus.to_le_bytes().to_vec();
            if let Some(root) = self.windows.get_mut(&self.root_window) {
                root.properties.insert(
                    net_active_atom,
                    PropertyValue {
                        prop_type: 33, // WINDOW
                        format: 32,
                        data,
                    },
                );
            }
            self.update_net_client_list();
        }

        // Generate FocusOut/FocusIn events with proper detail modes per X11 spec.
        if prev_focus != new_focus {
            let bo = self.msb_first;
            let seq = self.sequence;

            // Determine detail modes for FocusOut and FocusIn
            let (out_detail, in_detail, virtual_path) =
                self.compute_focus_detail(prev_focus, new_focus);

            // FocusOut on old focus window
            if prev_focus != 0 && prev_focus != 1 {
                self.send_focus_event(FOCUS_OUT_EVENT, out_detail, prev_focus, bo, seq);

                // Send FocusOut with Virtual detail to intermediate ancestors
                for &vw in &virtual_path {
                    if vw != prev_focus && vw != new_focus {
                        let vd = if out_detail == 0 || out_detail == 2 {
                            1 // Virtual
                        } else {
                            4 // NonlinearVirtual
                        };
                        self.send_focus_event(FOCUS_OUT_EVENT, vd, vw, bo, seq);
                    }
                }
            } else if prev_focus == 1 {
                // Focus was PointerRoot: send FocusOut(PointerRoot) to root
                self.send_focus_event(FOCUS_OUT_EVENT, 6, self.root_window, bo, seq);
            }

            // FocusIn on new focus window
            if new_focus != 0 && new_focus != 1 {
                // Send FocusIn with Virtual detail to intermediate ancestors
                for &vw in virtual_path.iter().rev() {
                    if vw != prev_focus && vw != new_focus {
                        let vd = if in_detail == 0 || in_detail == 2 {
                            1 // Virtual
                        } else {
                            4 // NonlinearVirtual
                        };
                        self.send_focus_event(FOCUS_IN_EVENT, vd, vw, bo, seq);
                    }
                }

                self.send_focus_event(FOCUS_IN_EVENT, in_detail, new_focus, bo, seq);

                // KeymapNotify after FocusIn if selected
                if let Some(win) = self.windows.get(&new_focus) {
                    if win.event_mask & EventMask::KEYMAP_STATE != EventMask::NO_EVENT {
                        let mut keys = [0u8; 31];
                        keys.copy_from_slice(&self.pressed_keys[1..32]);
                        let km_event = serialize_event(
                            &KeymapNotifyEvent {
                                response_type: KEYMAP_NOTIFY_EVENT,
                                keys,
                            },
                            bo,
                        );
                        self.pending_events.push(km_event);
                    }
                }
            } else if new_focus == 1 {
                // Focus set to PointerRoot: send FocusIn(PointerRoot) to root
                self.send_focus_event(FOCUS_IN_EVENT, 6, self.root_window, bo, seq);
            }
        }
    }

    /// Helper to send a single focus event if the window has FocusChangeMask selected.
    fn send_focus_event(&mut self, event_type: u8, detail: u8, window: u32, bo: bool, seq: u16) {
        let has_mask = self
            .windows
            .get(&window)
            .is_some_and(|w| w.event_mask & EventMask::FOCUS_CHANGE != EventMask::NO_EVENT);
        if has_mask || window == self.root_window {
            let event = serialize_event(
                &FocusInEvent {
                    response_type: event_type,
                    detail: NotifyDetail::from(detail),
                    sequence: seq,
                    event: window,
                    mode: NotifyMode::NORMAL,
                },
                bo,
            );
            self.pending_events.push(event);
        }
    }

    /// Compute focus detail modes and the virtual path between two focus windows.
    /// Returns (out_detail, in_detail, virtual_ancestors).
    fn compute_focus_detail(&self, old_focus: u32, new_focus: u32) -> (u8, u8, Vec<u32>) {
        // Special cases for None (0) and PointerRoot (1) per X11 spec §12.5.
        //
        // Detail codes: 0=Ancestor, 1=Virtual, 2=Inferior, 3=Nonlinear,
        //               4=NonlinearVirtual, 5=Pointer, 6=PointerRoot, 7=None
        if old_focus == 0 {
            // From None:
            if new_focus == 1 {
                return (7, 6, Vec::new()); // None → PointerRoot: out=None, in=PointerRoot on root
            }
            return (7, 3, Vec::new()); // None → window: out=None, in=Nonlinear
        }
        if old_focus == 1 {
            // From PointerRoot:
            if new_focus == 0 {
                return (6, 7, Vec::new()); // PointerRoot → None: out=PointerRoot on root, in=None
            }
            return (6, 3, Vec::new()); // PointerRoot → window: out=PointerRoot on root, in=Nonlinear
        }
        if new_focus == 0 {
            return (3, 7, Vec::new()); // window → None: out=Nonlinear, in=None
        }
        if new_focus == 1 {
            return (3, 6, Vec::new()); // window → PointerRoot: out=Nonlinear, in=PointerRoot on root
        }

        // Build ancestor chains for both windows
        let old_chain = self.ancestor_chain(old_focus);
        let new_chain = self.ancestor_chain(new_focus);

        // Check ancestor/descendant relationships
        let old_is_ancestor = old_chain.is_empty() || new_chain.contains(&old_focus);
        let new_is_ancestor = new_chain.is_empty() || old_chain.contains(&new_focus);

        if old_is_ancestor && new_chain.contains(&old_focus) {
            // old focus is an ancestor of new focus
            // FocusOut detail = Inferior, FocusIn detail = Ancestor
            let path: Vec<u32> = new_chain
                .iter()
                .take_while(|&&w| w != old_focus)
                .copied()
                .collect();
            return (2, 0, path);
        }
        if new_is_ancestor && old_chain.contains(&new_focus) {
            // new focus is an ancestor of old focus
            // FocusOut detail = Ancestor, FocusIn detail = Inferior
            let path: Vec<u32> = old_chain
                .iter()
                .take_while(|&&w| w != new_focus)
                .copied()
                .collect();
            return (0, 2, path);
        }

        // Nonlinear: find the least common ancestor (LCA) and build virtual path
        let mut virtual_path = Vec::new();
        // Walk old_chain to find LCA
        for &ow in &old_chain {
            if new_chain.contains(&ow) || ow == self.root_window {
                // Found LCA — collect intermediate windows
                for &w in &old_chain {
                    if w == ow {
                        break;
                    }
                    virtual_path.push(w);
                }
                virtual_path.push(ow);
                // Collect from new side
                let new_intermediates: Vec<u32> = new_chain
                    .iter()
                    .take_while(|&&w| w != ow)
                    .copied()
                    .collect();
                for w in new_intermediates.into_iter().rev() {
                    virtual_path.push(w);
                }
                break;
            }
        }
        (3, 3, virtual_path) // Nonlinear
    }

    /// Build the ancestor chain from a window to root (exclusive of the window itself).
    fn ancestor_chain(&self, window: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut current = window;
        for _ in 0..128 {
            match self.windows.get(&current) {
                Some(w) if w.parent != 0 && w.parent != current => {
                    chain.push(w.parent);
                    current = w.parent;
                }
                _ => break,
            }
        }
        chain
    }

    /// Send a `WindowFocused` update to the frontend.
    pub(crate) fn broadcast_focus(&self, window_id: Option<String>) {
        let _ = self.update_tx.send((
            self.client_id.clone(),
            DisplayUpdate::WindowFocused { window_id },
        ));
    }

    /// Update _NET_CLIENT_LIST and _NET_CLIENT_LIST_STACKING on root window.
    /// Called when windows are mapped, unmapped, or destroyed.
    pub(crate) fn update_net_client_list(&mut self) {
        let net_client_list_atom = self.intern_atom("_NET_CLIENT_LIST", false);
        let net_client_list_stacking_atom = self.intern_atom("_NET_CLIENT_LIST_STACKING", false);

        // _NET_CLIENT_LIST: all mapped top-level windows in deterministic (sorted) order
        let mut client_windows: Vec<u32> = self
            .windows
            .values()
            .filter(|w| w.parent == self.root_window && w.class == 1 && w.mapped)
            .map(|w| w.id)
            .collect();
        client_windows.sort(); // Deterministic order

        let data: Vec<u8> = client_windows
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();

        // _NET_CLIENT_LIST_STACKING: mapped top-level windows in z-order (bottom to top)
        // Use root's children_order which tracks actual stacking order.
        let stacking_windows: Vec<u32> = self
            .windows
            .get(&self.root_window)
            .map(|root| {
                root.children_order
                    .iter()
                    .filter(|&&cid| {
                        self.windows
                            .get(&cid)
                            .is_some_and(|w| w.class == 1 && w.mapped)
                    })
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        let stacking_data: Vec<u8> = stacking_windows
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();

        if let Some(root) = self.windows.get_mut(&self.root_window) {
            root.properties.insert(
                net_client_list_atom,
                PropertyValue {
                    prop_type: 33, // WINDOW
                    format: 32,
                    data,
                },
            );
            root.properties.insert(
                net_client_list_stacking_atom,
                PropertyValue {
                    prop_type: 33,
                    format: 32,
                    data: stacking_data,
                },
            );
        }
    }

    // -----------------------------------------------------------------------
    // WM / ICCCM / EWMH helpers
    // -----------------------------------------------------------------------

    /// Set WM_STATE property on a window (ICCCM).
    /// state: 0=WithdrawnState, 1=NormalState, 3=IconicState
    pub(crate) fn set_wm_state(&mut self, window: u32, wm_state_val: u32) {
        let wm_state_atom = self.intern_atom("WM_STATE", false);
        let mut data = vec![0u8; 8];
        // WM_STATE is stored in LE for consistency with setup reply
        data[0..4].copy_from_slice(&wm_state_val.to_le_bytes());
        // icon_window = None (0)
        if let Some(win) = self.windows.get_mut(&window) {
            win.properties.insert(
                wm_state_atom,
                PropertyValue {
                    prop_type: wm_state_atom,
                    format: 32,
                    data,
                },
            );
        }
    }

    /// Set _NET_WM_ALLOWED_ACTIONS on a window.
    pub(crate) fn set_allowed_actions(&mut self, window: u32) {
        let allowed_atom = self.intern_atom("_NET_WM_ALLOWED_ACTIONS", false);
        let actions: Vec<u32> = vec![
            self.intern_atom("_NET_WM_ACTION_MOVE", false),
            self.intern_atom("_NET_WM_ACTION_RESIZE", false),
            self.intern_atom("_NET_WM_ACTION_MINIMIZE", false),
            self.intern_atom("_NET_WM_ACTION_SHADE", false),
            self.intern_atom("_NET_WM_ACTION_MAXIMIZE_HORZ", false),
            self.intern_atom("_NET_WM_ACTION_MAXIMIZE_VERT", false),
            self.intern_atom("_NET_WM_ACTION_FULLSCREEN", false),
            self.intern_atom("_NET_WM_ACTION_CLOSE", false),
        ];
        let data: Vec<u8> = actions.iter().flat_map(|a| a.to_le_bytes()).collect();
        if let Some(win) = self.windows.get_mut(&window) {
            win.properties.insert(
                allowed_atom,
                PropertyValue {
                    prop_type: 4, // ATOM
                    format: 32,
                    data,
                },
            );
        }
    }

    /// Check if a window supports a specific WM_PROTOCOLS atom.
    pub(crate) fn window_supports_protocol(&self, window: u32, protocol_atom: u32) -> bool {
        let wm_protocols_atom = self.intern_atom("WM_PROTOCOLS", true);
        if wm_protocols_atom == 0 {
            return false;
        }
        self.windows
            .get(&window)
            .and_then(|w| w.properties.get(&wm_protocols_atom))
            .map(|pv| {
                if pv.format == 32 {
                    pv.data.chunks_exact(4).any(|chunk| {
                        u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                            == protocol_atom
                    })
                } else {
                    false
                }
            })
            .unwrap_or(false)
    }

    /// Send WM_TAKE_FOCUS ClientMessage to a window if it supports it.
    pub(crate) fn send_wm_take_focus(&mut self, window: u32) {
        let wm_take_focus_atom = self.intern_atom("WM_TAKE_FOCUS", false);
        if self.window_supports_protocol(window, wm_take_focus_atom) {
            let wm_protocols_atom = self.intern_atom("WM_PROTOCOLS", false);
            let bo = self.msb_first;
            let seq = self.sequence;
            let timestamp = self.timestamp();

            let cm = serialize_event(
                &ClientMessageEvent {
                    response_type: CLIENT_MESSAGE_EVENT,
                    format: 32,
                    sequence: seq,
                    window,
                    type_: wm_protocols_atom,
                    data: ClientMessageData::from([
                        wm_take_focus_atom,
                        timestamp,
                        0,
                        0,
                        0,
                    ]),
                },
                bo,
            );
            self.pending_events.push(cm);
        }
    }

    /// Send _NET_WM_PING ClientMessage to a window if it supports it.
    /// Per EWMH spec, the WM sends this to check if a window is responding.
    /// The client should respond by sending the same message back to the root.
    pub(crate) fn send_wm_ping(&mut self, window: u32) {
        let net_wm_ping_atom = self.intern_atom("_NET_WM_PING", false);
        if self.window_supports_protocol(window, net_wm_ping_atom) {
            let wm_protocols_atom = self.intern_atom("WM_PROTOCOLS", false);
            let bo = self.msb_first;
            let seq = self.sequence;
            let timestamp = self.timestamp();

            let cm = serialize_event(
                &ClientMessageEvent {
                    response_type: CLIENT_MESSAGE_EVENT,
                    format: 32,
                    sequence: seq,
                    window,
                    type_: wm_protocols_atom,
                    data: ClientMessageData::from([
                        net_wm_ping_atom,
                        timestamp,
                        window, // window being pinged
                        0,
                        0,
                    ]),
                },
                bo,
            );
            self.pending_events.push(cm);
        }
    }

    /// Get WM_NORMAL_HINTS (size hints) for a window.
    pub(crate) fn get_size_hints(&self, window: u32) -> Option<SizeHints> {
        let wm_normal_hints_atom = self.intern_atom("WM_NORMAL_HINTS", true);
        if wm_normal_hints_atom == 0 {
            return None;
        }
        let pv = self
            .windows
            .get(&window)?
            .properties
            .get(&wm_normal_hints_atom)?;
        if pv.format != 32 || pv.data.len() < 72 {
            return None;
        }

        let flags = u32::from_le_bytes([pv.data[0], pv.data[1], pv.data[2], pv.data[3]]);
        let mut hints = SizeHints::default();

        // PMinSize flag (bit 4)
        if flags & (1 << 4) != 0 && pv.data.len() >= 32 {
            hints.min_width =
                u32::from_le_bytes([pv.data[20], pv.data[21], pv.data[22], pv.data[23]]) as u16;
            hints.min_height =
                u32::from_le_bytes([pv.data[24], pv.data[25], pv.data[26], pv.data[27]]) as u16;
        }
        // PMaxSize flag (bit 5)
        if flags & (1 << 5) != 0 && pv.data.len() >= 40 {
            hints.max_width =
                u32::from_le_bytes([pv.data[28], pv.data[29], pv.data[30], pv.data[31]]) as u16;
            hints.max_height =
                u32::from_le_bytes([pv.data[32], pv.data[33], pv.data[34], pv.data[35]]) as u16;
        }
        // PResizeInc flag (bit 6)
        if flags & (1 << 6) != 0 && pv.data.len() >= 48 {
            hints.width_inc =
                u32::from_le_bytes([pv.data[36], pv.data[37], pv.data[38], pv.data[39]]) as u16;
            hints.height_inc =
                u32::from_le_bytes([pv.data[40], pv.data[41], pv.data[42], pv.data[43]]) as u16;
        }
        // PBaseSize flag (bit 8)
        if flags & (1 << 8) != 0 && pv.data.len() >= 64 {
            hints.base_width =
                u32::from_le_bytes([pv.data[52], pv.data[53], pv.data[54], pv.data[55]]) as u16;
            hints.base_height =
                u32::from_le_bytes([pv.data[56], pv.data[57], pv.data[58], pv.data[59]]) as u16;
        }

        Some(hints)
    }

    /// Recalculate _NET_WORKAREA on the root window based on all windows with
    /// _NET_WM_STRUT or _NET_WM_STRUT_PARTIAL properties.
    /// Workarea = full screen minus reserved strut areas.
    pub(crate) fn recalculate_workarea(&mut self) {
        let sw = self.screen_width as u32;
        let sh = self.screen_height as u32;

        // Accumulate max strut on each edge from all windows
        let (mut left, mut right, mut top, mut bottom) = (0u32, 0u32, 0u32, 0u32);

        for win in self.windows.values() {
            if let Some(strut) = &win.strut {
                left = left.max(strut[0]);
                right = right.max(strut[1]);
                top = top.max(strut[2]);
                bottom = bottom.max(strut[3]);
            }
        }

        let x = left;
        let y = top;
        let w = sw.saturating_sub(left + right);
        let h = sh.saturating_sub(top + bottom);

        // _NET_WORKAREA is 4 CARDINAL values per desktop; we have 1 desktop
        let net_workarea_atom = self.intern_atom("_NET_WORKAREA", false);
        let mut data = vec![0u8; 16];
        data[0..4].copy_from_slice(&x.to_le_bytes());
        data[4..8].copy_from_slice(&y.to_le_bytes());
        data[8..12].copy_from_slice(&w.to_le_bytes());
        data[12..16].copy_from_slice(&h.to_le_bytes());

        if let Some(root) = self.windows.get_mut(&self.root_window) {
            root.properties.insert(
                net_workarea_atom,
                PropertyValue {
                    prop_type: 6, // CARDINAL
                    format: 32,
                    data,
                },
            );
        }
    }
}
