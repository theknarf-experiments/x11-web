//! Map/unmap window handlers (opcodes 8, 9, 10, 11).

use super::update_sibling_visibility;
use super::*;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{
    BackingStore, ExposeEvent, MapNotifyEvent, MapRequestEvent, MapSubwindowsRequest,
    MapWindowRequest, UnmapNotifyEvent, UnmapSubwindowsRequest, UnmapWindowRequest, Visibility,
    VisibilityNotifyEvent, WindowClass,
};

// ---------------------------------------------------------------------------
// Opcode 8: MapWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_map_window(state: &mut ClientState, req: &MapWindowRequest) -> Vec<u8> {
    let seq = state.sequence;
    let wid = req.window;
    debug!(
        "MapWindow called: wid={wid:#x} exists={}",
        state.windows.contains_key(&wid)
    );

    let mut events = Vec::new();

    if !state.windows.contains_key(&wid) {
        warn!(
            "MapWindow: id={wid:#x} NOT FOUND in client {}",
            state.client_id
        );
        return build_error(WINDOW_ERROR, seq, wid, 8, 0);
    }

    // Per X11 spec: if the window's parent has SubstructureRedirectMask set by
    // another client, MapWindow generates MapRequest instead of actually mapping.
    // This applies to ALL windows (not just top-level), unless override_redirect.
    let is_override_redirect = state.windows.get(&wid).is_some_and(|w| w.override_redirect);
    let parent_id = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);

    if !is_override_redirect && parent_id != 0 {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                // Redirect if a WM has registered AND is a different client
                wm.client_id
                    .as_ref()
                    .is_some_and(|id| id != &state.client_id)
            } else {
                false
            }
        };

        // Also check if any client has SubstructureRedirectMask on the parent
        // (even without a formal WM registration — per X11 spec the mask is
        // what matters, not a WM registration handshake).
        if should_redirect || state.window_selects(parent_id, EventMask::SUBSTRUCTURE_REDIRECT) {
            info!("MapWindow: redirecting wid={wid:#x} as MapRequest (parent={parent_id:#x})");
            // Build MapRequest event (code 20)
            let map_request = serialize_event(
                &MapRequestEvent {
                    response_type: MAP_REQUEST_EVENT,
                    sequence: 0,
                    parent: parent_id,
                    window: wid,
                },
                state.msb_first,
            );

            // Deliver MapRequest to the WM if registered
            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(map_request.to_vec());
                }
            }
            // Per X11 spec, MapRequest is delivered to ALL clients with
            // SubstructureRedirectMask on the parent — including the same
            // client that issued the MapWindow. `deliver_event` handles
            // both local pending-events queue and the cross-connection
            // broadcast.
            state.deliver_event(parent_id, EventMask::SUBSTRUCTURE_REDIRECT, &map_request);
            // Don't map the window -- the WM/redirector will do it.
            return events;
        }
    }

    // Per X11 spec: "If the window is already mapped, the request has no effect."
    if state.windows.get(&wid).is_some_and(|w| w.mapped) {
        debug!("MapWindow: id={wid:#x} already mapped, no-op");
        return events;
    }

    let Some(wid_str) = state.window_uuid(wid) else {
        warn!("MapWindow: no UUID for {wid:#x}, skipping");
        return events;
    };
    let wm_state_atom = state.intern_atom("WM_STATE", false);
    let msb_first = state.msb_first;

    // Pre-extract background fill info before the main mutable borrow.
    // Complex fills (ParentRelative, pixmap tiling) need separate data extraction.
    struct BgInfo {
        bg_pixmap: Option<u32>,
        parent: u32,
    }
    let bg_info: Option<BgInfo> = state.windows.get(&wid).map(|w| BgInfo {
        bg_pixmap: w.background_pixmap,
        parent: w.parent,
    });

    // For ParentRelative, copy parent pixel data before mutating
    let parent_pixel_data: Option<(Vec<u8>, u32, u32)> = bg_info.as_ref().and_then(|info| {
        if info.bg_pixmap == Some(1) {
            state.windows.get(&info.parent).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.framebuffer.width(),
                    p.framebuffer.height(),
                )
            })
        } else {
            None
        }
    });

    // For pixmap tiling, copy pixmap data before mutating
    let tile_pixel_data: Option<(Vec<u8>, u32, u32)> =
        bg_info.as_ref().and_then(|info| match info.bg_pixmap {
            Some(pid) if pid > 1 => state.pixmaps.get(&pid).map(|p| {
                (
                    p.framebuffer.data().to_vec(),
                    p.width as u32,
                    p.height as u32,
                )
            }),
            _ => None,
        });

    if let Some(win) = state.windows.get_mut(&wid) {
        debug!(
            "MapWindow: id={wid:#x} {}x{} mapped={}",
            win.width, win.height, win.mapped
        );
        let is_top_level = win.parent == state.root_window
            && win.class == u16::from(WindowClass::INPUT_OUTPUT)
            && !win.override_redirect;
        win.mapped = true;

        let w = win.width;
        let h = win.height;
        let bs = win.backing_store;

        // Backing store: restore saved pixels instead of filling with background
        let restored = if bs != u32::from(BackingStore::NOT_USEFUL) as u8 {
            if let Some(saved) = win.backing_pixmap.take() {
                let fb_w = win.framebuffer.width();
                let fb_h = win.framebuffer.height();
                if saved.len() == (fb_w * fb_h * 4) as usize {
                    win.framebuffer.data_mut()[..saved.len()].copy_from_slice(&saved);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if !restored {
            match win.background_pixmap {
                Some(0) => {} // None: contents undefined
                Some(1) => {
                    // ParentRelative: copy matching area from parent's framebuffer
                    if let Some((ref parent_data, pw, ph)) = parent_pixel_data {
                        let px = win.x;
                        let py = win.y;
                        let dst_stride = win.framebuffer.width() as usize * 4;
                        let dst = win.framebuffer.data_mut();
                        for dy in 0..h as i32 {
                            for dx in 0..w as i32 {
                                let sx = px as i32 + dx;
                                let sy = py as i32 + dy;
                                if sx >= 0 && sy >= 0 && (sx as u32) < pw && (sy as u32) < ph {
                                    let src_off = ((sy as u32) * pw + sx as u32) as usize * 4;
                                    let dst_off = dy as usize * dst_stride + dx as usize * 4;
                                    if src_off + 4 <= parent_data.len() && dst_off + 4 <= dst.len()
                                    {
                                        dst[dst_off..dst_off + 4]
                                            .copy_from_slice(&parent_data[src_off..src_off + 4]);
                                    }
                                }
                            }
                        }
                    }
                }
                Some(_pixmap_id) => {
                    // Tile the pixmap across the window
                    if let Some((ref pix_data, pix_w, pix_h)) = tile_pixel_data {
                        if pix_w > 0 && pix_h > 0 {
                            win.framebuffer.fill_rect_tiled(
                                0, 0, w, h, pix_data, pix_w, pix_h, 0, 0, 3, 0xFFFFFFFF,
                            );
                        }
                    } else {
                        // Pixmap not found, fall back to background pixel
                        win.framebuffer.fill_rect(0, 0, w, h, win.background_pixel);
                    }
                }
                None => {
                    // No background pixmap: fill with background pixel color
                    win.framebuffer.fill_rect(0, 0, w, h, win.background_pixel);
                }
            }
        }

        // Honor WM_HINTS initial_state: 3=IconicState starts minimized
        let initial_state = win.wm_hints_initial_state.unwrap_or(1); // default NormalState

        // Set WM_STATE per ICCCM §4.1.3.1: set on all mapped windows.
        // Top-level windows may have IconicState (3) from WM_HINTS;
        // child windows default to NormalState (1).
        {
            let wm_state_val = if is_top_level && initial_state == 3 {
                3u32
            } else {
                1u32
            };
            let mut wm_state_data = wm_state_val.to_le_bytes().to_vec();
            wm_state_data.extend_from_slice(&[0; 4]); // icon_window = None
            win.properties.insert(
                wm_state_atom,
                PropertyValue {
                    prop_type: wm_state_atom,
                    format: 32,
                    data: wm_state_data,
                },
            );
        }

        let override_redirect = win.override_redirect;
        let event_mask = win.event_mask;
        let width = win.width;
        let height = win.height;

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowMapped {
                window_id: wid_str.clone(),
                is_top_level,
                override_redirect,
            },
        ));

        // If initial_state is IconicState (3), immediately send Minimized state to frontend
        if is_top_level && initial_state == 3 {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowStateChanged {
                    window_id: wid_str.clone(),
                    state: x11_web_protocol::WindowWmState::Minimized,
                },
            ));
        }

        // Send MapNotify to the window itself (StructureNotifyMask)
        let map_event = serialize_event(
            &MapNotifyEvent {
                response_type: MAP_NOTIFY_EVENT,
                sequence: seq,
                event: wid,
                window: wid,
                override_redirect,
            },
            msb_first,
        );
        if event_mask & EventMask::STRUCTURE_NOTIFY != EventMask::NO_EVENT {
            events.extend_from_slice(&map_event);
        }

        // Send MapNotify to parent (SubstructureNotifyMask)
        let parent_id = win.parent;
        {
            let parent_event = serialize_event(
                &MapNotifyEvent {
                    response_type: MAP_NOTIFY_EVENT,
                    sequence: seq,
                    event: parent_id,
                    window: wid,
                    override_redirect,
                },
                msb_first,
            );

            state.deliver_event(parent_id, EventMask::SUBSTRUCTURE_NOTIFY, &parent_event);
            // Also broadcast StructureNotify to other clients watching the window itself
            state.broadcast_event(wid, EventMask::STRUCTURE_NOTIFY, &map_event);
        }

        // Send VisibilityNotify with real occlusion computation
        {
            let vis_state = crate::xserver::compute_visibility(&state.windows, wid);
            if let Some(win) = state.windows.get_mut(&wid) {
                win.visibility = vis_state;
            }
            if event_mask & EventMask::VISIBILITY_CHANGE != EventMask::NO_EVENT {
                let vis_event = serialize_event(
                    &VisibilityNotifyEvent {
                        response_type: VISIBILITY_NOTIFY_EVENT,
                        sequence: seq,
                        window: wid,
                        state: Visibility::from(vis_state),
                    },
                    msb_first,
                );
                events.extend_from_slice(&vis_event);
            }
        }

        // Send Expose event only if backing store didn't restore the contents
        if !restored {
            // Also send Expose to all mapped descendant windows that selected ExposureMask.
            let descendants: Vec<(u32, u16, u16)> = state
                .windows
                .values()
                .filter(|w| w.mapped && w.id != wid && is_descendant_of(&state.windows, w.id, wid))
                .map(|w| (w.id, w.width, w.height))
                .collect();

            let self_selected = event_mask & EventMask::EXPOSURE != EventMask::NO_EVENT;
            // Total expose events: 1 (self, if selected) + descendants.len()
            let total = if self_selected { 1 } else { 0 } + descendants.len();

            let expose_event = serialize_event(
                &ExposeEvent {
                    response_type: EXPOSE_EVENT,
                    sequence: seq,
                    window: wid,
                    x: 0,
                    y: 0,
                    width,
                    height,
                    count: total.saturating_sub(1) as u16,
                },
                msb_first,
            );
            if self_selected {
                events.extend_from_slice(&expose_event);
            }

            for (i, (desc_id, dw, dh)) in descendants.iter().enumerate() {
                let desc_mask = state
                    .windows
                    .get(desc_id)
                    .map(|w| w.event_mask)
                    .unwrap_or(0);
                if desc_mask & EventMask::EXPOSURE != EventMask::NO_EVENT {
                    let base = if self_selected { 1 } else { 0 };
                    let remaining = total.saturating_sub(base + 1 + i) as u16;
                    let exp = serialize_event(
                        &ExposeEvent {
                            response_type: EXPOSE_EVENT,
                            sequence: seq,
                            window: *desc_id,
                            x: 0,
                            y: 0,
                            width: *dw,
                            height: *dh,
                            count: remaining,
                        },
                        msb_first,
                    );
                    events.extend_from_slice(&exp);
                }
            }

            // Broadcast Expose to other clients that selected ExposureMask on this window
            state.broadcast_event(wid, EventMask::EXPOSURE, &expose_event);
        }
    }

    // After the mutable borrow is released, set EWMH properties
    let is_top_level_for_ewmh = state.windows.get(&wid).is_some_and(|w| {
        w.parent == state.root_window
            && w.class == u16::from(WindowClass::INPUT_OUTPUT)
            && !w.override_redirect
    });

    // Set _NET_WM_STATE to empty (NormalState) if not already set
    let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties
            .entry(net_wm_state_atom)
            .or_insert_with(|| PropertyValue {
                prop_type: crate::xserver::atoms::predef::ATOM,
                format: 32,
                data: Vec::new(),
            });
    }

    // Set _NET_WM_ALLOWED_ACTIONS for top-level windows
    if is_top_level_for_ewmh {
        state.set_allowed_actions(wid);
    }

    // Mark the window dirty so the local mapped=true / new EWMH properties
    // get flushed to shared_windows on the next sync tick. Without this the
    // dirty-set optimisation in sync_windows would skip this window — only
    // CreateWindow currently marks dirty by default.
    state.mark_window_shared_dirty(wid);

    // Enforce stacking based on window type layers (EWMH _NET_WM_WINDOW_TYPE)
    // This places dock/tooltip/notification windows in their correct layer.
    {
        let parent_id = state.windows.get(&wid).map(|w| w.parent);
        if let Some(parent_id) = parent_id {
            super::restack_by_window_type(state, wid, parent_id);
        }
    }

    // Enforce WM_TRANSIENT_FOR stacking: transient windows go above their parent (ICCCM §4.1.2.6)
    // This runs after type-based stacking so transient dialogs end up above their parent
    // within the same stacking layer.
    let transient_for = state.windows.get(&wid).and_then(|w| w.transient_for);
    if let Some(parent_wid) = transient_for {
        let root = state.root_window;
        if let Some(root_win) = state.windows.get_mut(&root) {
            let has_parent = root_win.children_order.contains(&parent_wid);
            let has_child = root_win.children_order.contains(&wid);
            if has_parent && has_child {
                // Remove transient window and re-insert it just above its parent
                root_win.children_order.retain(|&c| c != wid);
                if let Some(pos) = root_win
                    .children_order
                    .iter()
                    .position(|&c| c == parent_wid)
                {
                    root_win.children_order.insert(pos + 1, wid);
                } else {
                    root_win.children_order.push(wid);
                }
            }
        }
    }

    // Update _NET_CLIENT_LIST on root
    state.update_net_client_list();

    // ICCCM focus model: respect WM_HINTS input field (§4.1.7).
    // Passive / Locally Active: input=true → call SetInputFocus
    // Globally Active: input=false, supports WM_TAKE_FOCUS → only send WM_TAKE_FOCUS
    // No Input: input=false, no WM_TAKE_FOCUS → don't focus at all
    if is_top_level_for_ewmh {
        let accepts_input = state
            .windows
            .get(&wid)
            .and_then(|w| w.wm_hints_input)
            .unwrap_or(true); // ICCCM default: accepts focus if not specified
        if accepts_input {
            state.set_focus_window(wid);
        }
        state.send_wm_take_focus(wid);
        state.send_wm_ping(wid);
    }

    // Per X11 spec §7: when a window is mapped and contains the pointer,
    // generate EnterNotify/LeaveNotify crossing events.
    {
        let pointer_x = state.pointer_x;
        let pointer_y = state.pointer_y;
        if let Some(win) = state.windows.get(&wid) {
            if win.mapped {
                let abs_x = win.x as i16;
                let abs_y = win.y as i16;
                let abs_x2 = abs_x + win.width as i16;
                let abs_y2 = abs_y + win.height as i16;
                if pointer_x >= abs_x
                    && pointer_x < abs_x2
                    && pointer_y >= abs_y
                    && pointer_y < abs_y2
                {
                    let crossing = crate::xserver::input::build_crossing_events(
                        state,
                        wid,
                        pointer_x,
                        pointer_y,
                        pointer_x - abs_x,
                        pointer_y - abs_y,
                    );
                    events.extend(crossing);
                }
            }
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 9: MapSubwindows
// ---------------------------------------------------------------------------

pub(crate) fn handle_map_subwindows(
    state: &mut ClientState,
    req: &MapSubwindowsRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let parent = req.window;

    if !state.windows.contains_key(&parent) {
        return build_error(WINDOW_ERROR, seq, parent, 9, 0);
    }

    // Collect child window IDs first to avoid borrow issues
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && !w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        let child_req = MapWindowRequest { window: child_id };
        all_events.extend(handle_map_window(state, &child_req));
    }

    all_events
}

// ---------------------------------------------------------------------------
// Opcode 10: UnmapWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_unmap_window(state: &mut ClientState, req: &UnmapWindowRequest) -> Vec<u8> {
    let seq = state.sequence;
    let wid = req.window;

    if !state.windows.contains_key(&wid) {
        return build_error(WINDOW_ERROR, seq, wid, 10, 0);
    }

    // Per X11 spec: "If the window is already unmapped, the request has no effect."
    if state.windows.get(&wid).is_some_and(|w| !w.mapped) {
        debug!("UnmapWindow: id={wid:#x} already unmapped, no-op");
        return Vec::new();
    }

    let mut events = Vec::new();

    // Extract info we need before mutating
    let (is_top_level, parent_id) = state
        .windows
        .get(&wid)
        .map(|w| (w.parent == state.root_window, w.parent))
        .unwrap_or((false, 0));
    let bo = state.msb_first;

    if let Some(win) = state.windows.get_mut(&wid) {
        // Save framebuffer pixels if backing store is enabled.
        // Apply backing_planes mask: only preserve bits in backing_planes,
        // filling other bits with backing_pixel on restore.
        if win.backing_store != u32::from(BackingStore::NOT_USEFUL) as u8 && win.mapped {
            let planes = win.backing_planes;
            if planes == 0xFFFFFFFF {
                // All planes — simple full copy
                win.backing_pixmap = Some(win.framebuffer.data().to_vec());
            } else {
                // Mask: preserve only the bits specified by backing_planes
                let src = win.framebuffer.data();
                let mut saved = Vec::with_capacity(src.len());
                let pixel_bytes = win.backing_pixel.to_le_bytes();
                for chunk in src.chunks_exact(4) {
                    let s = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    let bp = u32::from_le_bytes(pixel_bytes);
                    // Preserve planes in backing_planes from framebuffer,
                    // fill remaining planes from backing_pixel
                    let combined = (s & planes) | (bp & !planes);
                    saved.extend_from_slice(&combined.to_le_bytes());
                }
                win.backing_pixmap = Some(saved);
            }
        }
        win.mapped = false;
    }

    // Make sure the mapped=false transition reaches shared_windows.
    state.mark_window_shared_dirty(wid);

    if let Some(uuid) = state.window_uuid(wid) {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowUnmapped { window_id: uuid },
        ));
    }

    // UnmapNotify to the window itself (StructureNotifyMask)
    let unmap_event = {
        let event = serialize_event(
            &UnmapNotifyEvent {
                response_type: UNMAP_NOTIFY_EVENT,
                sequence: seq,
                event: wid,
                window: wid,
                from_configure: false,
            },
            bo,
        );
        let win_mask = state.windows.get(&wid).map(|w| w.event_mask).unwrap_or(0);
        if win_mask & EventMask::STRUCTURE_NOTIFY != EventMask::NO_EVENT {
            events.extend_from_slice(&event);
        }
        event
    };

    // Send UnmapNotify to parent (SubstructureNotifyMask)
    if parent_id != 0 {
        let parent_event = serialize_event(
            &UnmapNotifyEvent {
                response_type: UNMAP_NOTIFY_EVENT,
                sequence: seq,
                event: parent_id,
                window: wid,
                from_configure: false,
            },
            bo,
        );

        let parent_wants_notify = state.window_selects(parent_id, EventMask::SUBSTRUCTURE_NOTIFY);
        if parent_wants_notify {
            events.extend_from_slice(&parent_event);
        }

        // Cross-connection broadcast
        state.broadcast_event(parent_id, EventMask::SUBSTRUCTURE_NOTIFY, &parent_event);
        state.broadcast_event(wid, EventMask::STRUCTURE_NOTIFY, &unmap_event);
    }

    // Set WM_STATE = WithdrawnState for top-level windows (ICCCM)
    if is_top_level {
        state.set_wm_state(wid, 0); // WithdrawnState
    }

    // ICCCM §4.1.2.6: When a transient-for parent is unmapped, also unmap its transient children.
    // Collect transient children first to avoid borrow issues.
    let transient_children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.transient_for == Some(wid) && w.mapped)
        .map(|w| w.id)
        .collect();
    for child_id in transient_children {
        let unmap_req = UnmapWindowRequest { window: child_id };
        let _child_events = handle_unmap_window(state, &unmap_req);
    }

    // Update _NET_CLIENT_LIST on root
    state.update_net_client_list();

    // Revert focus if this was the focus window
    state.revert_focus_from(wid);

    // Unmapping a window may unobscure siblings — recalculate visibility
    update_sibling_visibility(state, wid, seq, bo);

    // Per X11 spec §7: when a window containing the pointer is unmapped,
    // generate LeaveNotify/EnterNotify crossing events. The pointer
    // effectively enters the parent or root.
    if state.last_entered_window == wid {
        let pointer_x = state.pointer_x;
        let pointer_y = state.pointer_y;
        // The pointer now enters the parent window
        let target = if parent_id != 0 {
            parent_id
        } else {
            state.root_window
        };
        let crossing = crate::xserver::input::build_crossing_events(
            state, target, pointer_x, pointer_y, pointer_x, pointer_y,
        );
        events.extend(crossing);
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 11: UnmapSubwindows
// ---------------------------------------------------------------------------

pub(crate) fn handle_unmap_subwindows(
    state: &mut ClientState,
    req: &UnmapSubwindowsRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let parent = req.window;

    if !state.windows.contains_key(&parent) {
        return build_error(WINDOW_ERROR, seq, parent, 11, 0);
    }

    // Collect all mapped children
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        let child_req = UnmapWindowRequest { window: child_id };
        all_events.extend(handle_unmap_window(state, &child_req));
    }

    all_events
}
