//! Map/unmap window handlers (opcodes 8, 9, 10, 11).

use super::*;
use super::update_sibling_visibility;

// ---------------------------------------------------------------------------
// Opcode 8: MapWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_map_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 { return build_error(BAD_LENGTH, seq, 0, 8, 0); }
    let wid = state.read_u32(data, 4);
    info!("MapWindow called: wid={wid:#x} exists={}", state.windows.contains_key(&wid));

    let mut events = Vec::new();

    if !state.windows.contains_key(&wid) {
        warn!("MapWindow: id={wid:#x} NOT FOUND in client {}", state.client_id);
        return build_error(BAD_WINDOW, seq, wid, 8, 0);
    }

    // Check if this is a top-level window (parent == root) and a WM is active.
    // If so, redirect as a MapRequest event to the WM instead of mapping directly.
    // override_redirect windows bypass the WM redirect.
    let is_top_level = state.windows.get(&wid).is_some_and(|w| w.parent == state.root_window);
    let is_override_redirect = state.windows.get(&wid).is_some_and(|w| w.override_redirect);

    if is_top_level && !is_override_redirect {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                // Only redirect if the WM is a *different* client
                wm.client_id.as_ref().is_some_and(|id| id != &state.client_id)
            } else {
                false
            }
        };

        if should_redirect {
            info!(
                "MapWindow: redirecting wid={wid:#x} as MapRequest to WM"
            );
            // Build MapRequest event (code 20)
            let mut map_request = [0u8; 32];
            map_request[0] = MAP_REQUEST_EVENT;
            // map_request[1] = 0; // unused
            // sequence number will be the WM's -- but we use 0 since the server
            // inserts events asynchronously.
            state.write_u32(&mut map_request, 4, state.root_window); // parent
            state.write_u32(&mut map_request, 8, wid); // window

            // Per X11 spec, deliver MapRequest to all clients that have
            // SubstructureRedirectMask on the parent (typically just the WM).
            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(map_request.to_vec());
                }
            }
            // Also broadcast to any other clients with SubstructureRedirectMask
            let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(state.root_window);
            state.broadcast_event(parent, SUBSTRUCTURE_REDIRECT_MASK, &map_request);
            // Don't map the window -- the WM will do it.
            return events;
        }
    }

    let Some(wid_str) = state.window_uuid(wid) else {
        warn!("MapWindow: no UUID for {wid:#x}, skipping");
        return events;
    };
    let wm_state_atom = state.intern_atom("WM_STATE", false);
    let msb_first = state.msb_first;

    // Pre-extract background fill info before the main mutable borrow.
    // Complex fills (ParentRelative, pixmap tiling) need separate data extraction.
    let bg_info: Option<(Option<u32>, u32, i16, i16, u32, u16, u16)> = state.windows.get(&wid).map(|w| {
        (w.background_pixmap, w.background_pixel, w.x, w.y, w.parent, w.width, w.height)
    });

    // For ParentRelative, copy parent pixel data before mutating
    let parent_pixel_data: Option<(Vec<u8>, u32, u32)> = bg_info.as_ref().and_then(|(bg_pix, _, _, _, parent_id, _, _)| {
        if *bg_pix == Some(1) {
            state.windows.get(parent_id).map(|p| (p.framebuffer.data().to_vec(), p.framebuffer.width(), p.framebuffer.height()))
        } else {
            None
        }
    });

    // For pixmap tiling, copy pixmap data before mutating
    let tile_pixel_data: Option<(Vec<u8>, u32, u32)> = bg_info.as_ref().and_then(|(bg_pix, _, _, _, _, _, _)| {
        match bg_pix {
            Some(pid) if *pid > 1 => {
                state.pixmaps.get(pid).map(|p| (p.framebuffer.data().to_vec(), p.width as u32, p.height as u32))
            }
            _ => None,
        }
    });

    if let Some(win) = state.windows.get_mut(&wid) {
        info!("MapWindow: id={wid:#x} {}x{} mapped={}", win.width, win.height, win.mapped);
        let is_top_level = win.parent == state.root_window && win.class == 1 && !win.override_redirect;
        win.mapped = true;

        let w = win.width;
        let h = win.height;
        let bs = win.backing_store;

        // Backing store: restore saved pixels instead of filling with background
        let restored = if bs > 0 {
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
                                    if src_off + 4 <= parent_data.len() && dst_off + 4 <= dst.len() {
                                        dst[dst_off..dst_off + 4].copy_from_slice(&parent_data[src_off..src_off + 4]);
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
                                0, 0, w, h, pix_data, pix_w, pix_h,
                                0, 0, 3, 0xFFFFFFFF,
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

        // Set WM_STATE based on initial_state
        if is_top_level {
            let wm_state_val = if initial_state == 3 { 3u32 } else { 1u32 }; // IconicState or NormalState
            let mut wm_state_data = vec![0u8; 8];
            write_u32_bo(&mut wm_state_data, 0, wm_state_val, false); // LE
            win.properties.insert(wm_state_atom, PropertyValue {
                prop_type: wm_state_atom,
                format: 32,
                data: wm_state_data,
            });
        }

        let override_redirect = win.override_redirect;
        let event_mask = win.event_mask;
        let width = win.width;
        let height = win.height;

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowMapped { window_id: wid_str.clone(), is_top_level, override_redirect },
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
        let mut map_event = [0u8; 32];
        map_event[0] = MAP_NOTIFY_EVENT;
        write_u16_bo(&mut map_event, 2, seq, msb_first);
        write_u32_bo(&mut map_event, 4, wid, msb_first); // event window
        write_u32_bo(&mut map_event, 8, wid, msb_first); // window
        map_event[12] = if override_redirect { 1 } else { 0 };
        events.extend_from_slice(&map_event);

        // Send MapNotify to parent (SubstructureNotifyMask)
        let parent_id = win.parent;
        {
            let mut parent_event = [0u8; 32];
            parent_event[0] = MAP_NOTIFY_EVENT;
            write_u16_bo(&mut parent_event, 2, seq, msb_first);
            write_u32_bo(&mut parent_event, 4, parent_id, msb_first); // event = parent
            write_u32_bo(&mut parent_event, 8, wid, msb_first); // window = child
            parent_event[12] = if override_redirect { 1 } else { 0 };

            // Local delivery
            if let Some(parent_win) = state.windows.get(&parent_id) {
                if parent_win.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0 {
                    state.pending_events.push(parent_event.to_vec());
                }
            }

            // Cross-connection broadcast to other clients watching this parent
            state.broadcast_event(parent_id, SUBSTRUCTURE_NOTIFY_MASK, &parent_event);
            // Also broadcast StructureNotify to other clients watching the window itself
            state.broadcast_event(wid, STRUCTURE_NOTIFY_MASK, &map_event);
        }

        // Send VisibilityNotify with real occlusion computation
        {
            let vis_state = crate::xserver::compute_visibility(&state.windows, wid);
            if let Some(win) = state.windows.get_mut(&wid) {
                win.visibility = vis_state;
            }
            if event_mask & VISIBILITY_CHANGE_MASK != 0 {
                let mut vis_event = [0u8; 32];
                vis_event[0] = VISIBILITY_NOTIFY_EVENT;
                write_u16_bo(&mut vis_event, 2, seq, msb_first);
                write_u32_bo(&mut vis_event, 4, wid, msb_first);
                vis_event[8] = vis_state;
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

            let self_selected = event_mask & EXPOSURE_MASK != 0;
            // Total expose events: 1 (self, if selected) + descendants.len()
            let total = if self_selected { 1 } else { 0 } + descendants.len();

            let mut expose_event = [0u8; 32];
            expose_event[0] = EXPOSE_EVENT;
            write_u16_bo(&mut expose_event, 2, seq, msb_first);
            write_u32_bo(&mut expose_event, 4, wid, msb_first);
            // x=0, y=0 already zero
            write_u16_bo(&mut expose_event, 12, width, msb_first);
            write_u16_bo(&mut expose_event, 14, height, msb_first);
            write_u16_bo(&mut expose_event, 16, total.saturating_sub(1) as u16, msb_first); // count: remaining
            if self_selected {
                events.extend_from_slice(&expose_event);
            }

            for (i, (desc_id, dw, dh)) in descendants.iter().enumerate() {
                let desc_mask = state.windows.get(desc_id).map(|w| w.event_mask).unwrap_or(0);
                if desc_mask & EXPOSURE_MASK != 0 {
                    let mut exp = [0u8; 32];
                    exp[0] = EXPOSE_EVENT;
                    write_u16_bo(&mut exp, 2, seq, msb_first);
                    write_u32_bo(&mut exp, 4, *desc_id, msb_first);
                    write_u16_bo(&mut exp, 12, *dw, msb_first);
                    write_u16_bo(&mut exp, 14, *dh, msb_first);
                    let base = if self_selected { 1 } else { 0 };
                    let remaining = total.saturating_sub(base + 1 + i) as u16;
                    write_u16_bo(&mut exp, 16, remaining, msb_first); // count: remaining
                    events.extend_from_slice(&exp);
                }
            }

            // Broadcast Expose to other clients that selected ExposureMask on this window
            state.broadcast_event(wid, EXPOSURE_MASK, &expose_event);
        }
    }

    // After the mutable borrow is released, set EWMH properties
    let is_top_level_for_ewmh = state.windows.get(&wid)
        .is_some_and(|w| w.parent == state.root_window && w.class == 1 && !w.override_redirect);

    // Set _NET_WM_STATE to empty (NormalState) if not already set
    let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties.entry(net_wm_state_atom).or_insert_with(|| PropertyValue {
                prop_type: 4, // ATOM
                format: 32,
                data: Vec::new(),
            });
    }

    // Set _NET_WM_ALLOWED_ACTIONS for top-level windows
    if is_top_level_for_ewmh {
        state.set_allowed_actions(wid);
    }

    // Enforce WM_TRANSIENT_FOR stacking: transient windows go above their parent (ICCCM §4.1.2.6)
    let transient_for = state.windows.get(&wid).and_then(|w| w.transient_for);
    if let Some(parent_wid) = transient_for {
        let root = state.root_window;
        if let Some(root_win) = state.windows.get_mut(&root) {
            let has_parent = root_win.children_order.contains(&parent_wid);
            let has_child = root_win.children_order.contains(&wid);
            if has_parent && has_child {
                // Remove transient window and re-insert it just above its parent
                root_win.children_order.retain(|&c| c != wid);
                if let Some(pos) = root_win.children_order.iter().position(|&c| c == parent_wid) {
                    root_win.children_order.insert(pos + 1, wid);
                } else {
                    root_win.children_order.push(wid);
                }
            }
        }
    }

    // Update _NET_CLIENT_LIST on root
    state.update_net_client_list();

    // Send WM_TAKE_FOCUS to the newly mapped window (ICCCM)
    if is_top_level_for_ewmh {
        state.send_wm_take_focus(wid);
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 9: MapSubwindows
// ---------------------------------------------------------------------------

pub(crate) fn handle_map_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 { return build_error(BAD_LENGTH, seq, 0, 9, 0); }
    let parent = state.read_u32(data, 4);

    if !state.windows.contains_key(&parent) {
        return build_error(BAD_WINDOW, seq, parent, 9, 0);
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
        // Construct a fake MapWindow request for each child
        let mut fake_data = [0u8; 8];
        fake_data[0] = 8; // MapWindow opcode
        state.write_u16(&mut fake_data, 2, 2u16); // length = 2
        state.write_u32(&mut fake_data, 4, child_id);
        let events = handle_map_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}

// ---------------------------------------------------------------------------
// Opcode 10: UnmapWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_unmap_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 { return build_error(BAD_LENGTH, seq, 0, 10, 0); }
    let wid = state.read_u32(data, 4);

    if !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, seq, wid, 10, 0);
    }

    let mut events = Vec::new();

    // Extract info we need before mutating
    let (is_top_level, parent_id) = state.windows.get(&wid)
        .map(|w| (w.parent == state.root_window, w.parent))
        .unwrap_or((false, 0));
    let bo = state.msb_first;

    if let Some(win) = state.windows.get_mut(&wid) {
        // Save framebuffer pixels if backing store is enabled.
        // Apply backing_planes mask: only preserve bits in backing_planes,
        // filling other bits with backing_pixel on restore.
        if win.backing_store > 0 && win.mapped {
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

    if let Some(uuid) = state.window_uuid(wid) {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowUnmapped { window_id: uuid },
        ));
    }

    // UnmapNotify to the window itself (StructureNotifyMask)
    let unmap_event = {
        let mut event = [0u8; 32];
        event[0] = UNMAP_NOTIFY_EVENT;
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, wid, bo);
        write_u32_bo(&mut event, 8, wid, bo);
        events.extend_from_slice(&event);
        event
    };

    // Send UnmapNotify to parent (SubstructureNotifyMask)
    if parent_id != 0 {
        let mut parent_event = [0u8; 32];
        parent_event[0] = UNMAP_NOTIFY_EVENT;
        write_u16_bo(&mut parent_event, 2, seq, bo);
        write_u32_bo(&mut parent_event, 4, parent_id, bo);
        write_u32_bo(&mut parent_event, 8, wid, bo);

        let parent_wants_notify = state.windows.get(&parent_id)
            .is_some_and(|w| w.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0);
        if parent_wants_notify {
            events.extend_from_slice(&parent_event);
        }

        // Cross-connection broadcast
        state.broadcast_event(parent_id, SUBSTRUCTURE_NOTIFY_MASK, &parent_event);
        state.broadcast_event(wid, STRUCTURE_NOTIFY_MASK, &unmap_event);
    }

    // Set WM_STATE = WithdrawnState for top-level windows (ICCCM)
    if is_top_level {
        state.set_wm_state(wid, 0); // WithdrawnState
    }

    // ICCCM §4.1.2.6: When a transient-for parent is unmapped, also unmap its transient children.
    // Collect transient children first to avoid borrow issues.
    let transient_children: Vec<u32> = state.windows.values()
        .filter(|w| w.transient_for == Some(wid) && w.mapped)
        .map(|w| w.id)
        .collect();
    for child_id in transient_children {
        let mut fake_data = [0u8; 8];
        fake_data[0] = 10; // UnmapWindow opcode
        state.write_u16(&mut fake_data, 2, 2u16);
        state.write_u32(&mut fake_data, 4, child_id);
        let _child_events = handle_unmap_window(state, &fake_data, seq);
    }

    // Update _NET_CLIENT_LIST on root
    state.update_net_client_list();

    // Revert focus if this was the focus window
    state.revert_focus_from(wid);

    // Unmapping a window may unobscure siblings — recalculate visibility
    update_sibling_visibility(state, wid, seq, bo);

    events
}

// ---------------------------------------------------------------------------
// Opcode 11: UnmapSubwindows
// ---------------------------------------------------------------------------

pub(crate) fn handle_unmap_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, seq, 0, 11, 0);
    }
    let parent = state.read_u32(data, 4);

    if !state.windows.contains_key(&parent) {
        return build_error(BAD_WINDOW, seq, parent, 11, 0);
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
        let mut fake_data = [0u8; 8];
        fake_data[0] = 10; // UnmapWindow opcode
        state.write_u16(&mut fake_data, 2, 2u16);
        state.write_u32(&mut fake_data, 4, child_id);
        let events = handle_unmap_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}
