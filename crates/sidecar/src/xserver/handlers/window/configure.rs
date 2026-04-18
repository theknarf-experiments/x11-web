//! Configure/reparent/circulate window handlers (opcodes 7, 12, 13).

use super::*;
use super::{update_sibling_visibility, win_gravity_delta};
use crate::xserver::core::require_len;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{
    CirculateNotifyEvent, ClientMessageData, ClientMessageEvent, ConfigureNotifyEvent,
    ConfigureRequestEvent, ExposeEvent, GravityNotifyEvent, MapNotifyEvent, ReparentNotifyEvent,
    ResizeRequestEvent, UnmapNotifyEvent,
};

// ---------------------------------------------------------------------------
// Opcode 7: ReparentWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_reparent_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 7);

    let window = state.read_u32(data, 4);
    let new_parent = state.read_u32(data, 8);
    let x = state.read_i16(data, 12);
    let y = state.read_i16(data, 14);

    if !state.windows.contains_key(&window) {
        return build_error(BAD_WINDOW, state.sequence, window, 7, 0);
    }
    if !state.windows.contains_key(&new_parent) {
        return build_error(BAD_WINDOW, state.sequence, new_parent, 7, 0);
    }

    // Per X11 spec: it is a BadMatch error to reparent a window to itself
    // or to one of its own descendants (would create a circular tree).
    if window == new_parent || crate::xserver::is_descendant_of(&state.windows, new_parent, window)
    {
        return build_error(BAD_MATCH, state.sequence, window, 7, 0);
    }

    let bo = state.msb_first;
    let old_parent = state.windows.get(&window).map(|w| w.parent).unwrap_or(0);

    // Per X11 spec: if the window is mapped, perform an automatic UnmapWindow first.
    // This means generating proper UnmapNotify events.
    let was_mapped = state.windows.get(&window).is_some_and(|w| w.mapped);
    if was_mapped {
        // Generate UnmapNotify to the window itself (StructureNotifyMask)
        {
            let unmap_event = serialize_event(&UnmapNotifyEvent {
                response_type: UNMAP_NOTIFY_EVENT,
                sequence: seq,
                event: window,
                window,
                from_configure: false,
            }, bo);
            if state
                .windows
                .get(&window)
                .is_some_and(|w| w.event_mask & STRUCTURE_NOTIFY_MASK != 0)
            {
                state.pending_events.push(unmap_event.clone());
            }
            state.broadcast_event(window, STRUCTURE_NOTIFY_MASK, &unmap_event);
        }
        // Generate UnmapNotify to the old parent (SubstructureNotifyMask)
        if old_parent != 0 {
            let parent_unmap = serialize_event(&UnmapNotifyEvent {
                response_type: UNMAP_NOTIFY_EVENT,
                sequence: seq,
                event: old_parent,
                window,
                from_configure: false,
            }, bo);
            if state
                .windows
                .get(&old_parent)
                .is_some_and(|w| w.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0)
            {
                state.pending_events.push(parent_unmap.clone());
            }
            state.broadcast_event(old_parent, SUBSTRUCTURE_NOTIFY_MASK, &parent_unmap);
        }

        if let Some(win) = state.windows.get_mut(&window) {
            win.mapped = false;
        }
    }

    // Remove from old parent's children_order
    if let Some(old_parent_win) = state.windows.get_mut(&old_parent) {
        old_parent_win.children_order.retain(|&c| c != window);
    }

    // Update parent and position
    if let Some(win) = state.windows.get_mut(&window) {
        win.parent = new_parent;
        win.x = x;
        win.y = y;
    }

    // Add to new parent's children_order (on top of stacking order)
    if let Some(new_parent_win) = state.windows.get_mut(&new_parent) {
        new_parent_win.children_order.push(window);
    }

    let override_redirect = state
        .windows
        .get(&window)
        .is_some_and(|w| w.override_redirect);

    // Build ReparentNotify event template
    let build_reparent_notify = |event_window: u32| -> Vec<u8> {
        serialize_event(&ReparentNotifyEvent {
            response_type: REPARENT_NOTIFY_EVENT,
            sequence: seq,
            event: event_window,
            window,
            parent: new_parent,
            x,
            y,
            override_redirect,
        }, bo)
    };

    let mut events = Vec::new();

    // Send ReparentNotify to the window itself (StructureNotifyMask)
    {
        let event = build_reparent_notify(window);
        if state
            .windows
            .get(&window)
            .is_some_and(|w| w.event_mask & STRUCTURE_NOTIFY_MASK != 0)
        {
            events.extend_from_slice(&event);
        }
        state.broadcast_event(window, STRUCTURE_NOTIFY_MASK, &event);
    }

    // Send ReparentNotify to old parent (SubstructureNotifyMask)
    {
        let event = build_reparent_notify(old_parent);
        if state
            .windows
            .get(&old_parent)
            .is_some_and(|w| w.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0)
        {
            state.pending_events.push(event.clone());
        }
        state.broadcast_event(old_parent, SUBSTRUCTURE_NOTIFY_MASK, &event);
    }

    // Send ReparentNotify to new parent (SubstructureNotifyMask)
    if old_parent != new_parent {
        let event = build_reparent_notify(new_parent);
        if state
            .windows
            .get(&new_parent)
            .is_some_and(|w| w.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0)
        {
            state.pending_events.push(event.clone());
        }
        state.broadcast_event(new_parent, SUBSTRUCTURE_NOTIFY_MASK, &event);
    }

    // Per X11 spec: if the window was originally mapped, perform an automatic
    // MapWindow on it after the reparent. This generates proper MapNotify events
    // and handles SubstructureRedirect (WM redirect) for the new parent.
    if was_mapped {
        // Build a synthetic MapWindow request and dispatch it
        let mut map_data = [0u8; 8];
        map_data[0] = 8; // MapWindow opcode
        state.write_u16(&mut map_data, 2, 2); // request length = 2
        state.write_u32(&mut map_data, 4, window);
        let map_events = super::map::handle_map_window(state, &map_data, seq);
        events.extend_from_slice(&map_events);
    }

    events
}

// ---------------------------------------------------------------------------
// Opcode 12: ConfigureWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_configure_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 12);

    let wid = state.read_u32(data, 4);
    let value_mask = state.read_u16(data, 8);

    // Validate window exists.
    if wid != state.root_window && !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, seq, wid, 12, 0);
    }

    // Validate value-list length matches the bitmask
    let n_values = (value_mask as u32).count_ones() as usize;
    let required_len = 12 + n_values * 4;
    require_len!(data, required_len, seq, 12);

    // Per X11 spec: if the window's parent has SubstructureRedirectMask set by
    // another client, ConfigureWindow generates ConfigureRequest instead of
    // actually configuring. This applies to ALL windows, not just top-level,
    // unless override_redirect is set.
    let is_override_redirect = state.windows.get(&wid).is_some_and(|w| w.override_redirect);
    let parent_id = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);

    if !is_override_redirect && parent_id != 0 {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                wm.client_id
                    .as_ref()
                    .is_some_and(|id| id != &state.client_id)
            } else {
                false
            }
        };

        // Also check if any client has SubstructureRedirectMask on the parent
        let parent_has_redirect = state
            .windows
            .get(&parent_id)
            .is_some_and(|p| p.event_mask & SUBSTRUCTURE_REDIRECT_MASK != 0);

        if should_redirect || parent_has_redirect {
            info!("ConfigureWindow: redirecting wid={wid:#x} as ConfigureRequest (parent={parent_id:#x})");

            // Parse the values from the request to populate the ConfigureRequest event.
            let mut x: i16 = 0;
            let mut y: i16 = 0;
            let mut width: u16 = 0;
            let mut height: u16 = 0;
            let mut border_width: u16 = 0;
            let mut sibling: u32 = 0;
            let mut stack_mode: u8 = 0;

            // Pre-fill with current values from the window
            if let Some(win) = state.windows.get(&wid) {
                x = win.x;
                y = win.y;
                width = win.width;
                height = win.height;
                border_width = win.border_width;
            }

            let mut offset = 12;
            for bit in 0..7u16 {
                if value_mask & (1 << bit) != 0 && offset + 4 <= data.len() {
                    let val = state.read_u32(data, offset);
                    match bit {
                        0 => x = val as i16,
                        1 => y = val as i16,
                        2 => width = val as u16,
                        3 => height = val as u16,
                        4 => border_width = val as u16,
                        5 => sibling = val,
                        6 => stack_mode = val as u8,
                        _ => {}
                    }
                    offset += 4;
                }
            }

            // Build ConfigureRequest event (code 23)
            let event = serialize_event(&ConfigureRequestEvent {
                response_type: CONFIGURE_REQUEST_EVENT,
                stack_mode: stack_mode.into(),
                sequence: 0,
                parent: parent_id,
                window: wid,
                sibling,
                x,
                y,
                width,
                height,
                border_width,
                value_mask: value_mask.into(),
            }, state.msb_first);

            // Per X11 spec, deliver ConfigureRequest to all clients that have
            // SubstructureRedirectMask on the parent.
            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(event.clone());
                }
            }
            state.broadcast_event(parent_id, SUBSTRUCTURE_REDIRECT_MASK, &event);
            return Vec::new();
        }
    }

    // Per X11 spec: if another client has selected ResizeRedirectMask on this
    // window, suppress width/height changes and generate ResizeRequest(25)
    // instead. Position, border, and stacking changes still proceed normally.
    let resize_redirected = {
        let has_resize_redirect = state.event_broadcaster.has_mask_subscriber(
            wid,
            RESIZE_REDIRECT_MASK,
            &state.client_id,
        );
        has_resize_redirect && (value_mask & 0x0C != 0) // bits 2 (width) or 3 (height)
    };
    // Strip width/height bits from value_mask if resize is redirected
    let value_mask = if resize_redirected {
        // Parse the requested width/height for the ResizeRequest event
        let mut req_width = state.windows.get(&wid).map(|w| w.width).unwrap_or(1);
        let mut req_height = state.windows.get(&wid).map(|w| w.height).unwrap_or(1);
        let mut scan_offset = 12;
        for bit in 0..7u16 {
            if value_mask & (1 << bit) != 0 && scan_offset + 4 <= data.len() {
                let val = state.read_u32(data, scan_offset);
                if bit == 2 {
                    req_width = val as u16;
                }
                if bit == 3 {
                    req_height = val as u16;
                }
                scan_offset += 4;
            }
        }

        let bo = state.msb_first;
        let event = serialize_event(&ResizeRequestEvent {
            response_type: RESIZE_REQUEST_EVENT,
            sequence: seq,
            window: wid,
            width: req_width,
            height: req_height,
        }, bo);
        state.broadcast_event(wid, RESIZE_REDIRECT_MASK, &event);

        // Clear width (bit 2) and height (bit 3) from mask so they aren't applied
        value_mask & !0x0C
    } else {
        value_mask
    };

    let mut offset = 12;
    let mut changed = false;
    let mut sibling: u32 = 0;
    let mut stack_mode: Option<u8> = None;
    let wid_str = state.window_uuid(wid);
    let msb_first = state.msb_first;

    // Get size hints for this window (ICCCM WM_NORMAL_HINTS compliance)
    let size_hints = state.get_size_hints(wid);

    // _NET_WM_SYNC_REQUEST: if the window has a sync counter and supports the
    // protocol, increment the counter and send a ClientMessage before resizing.
    let resizing = value_mask & 0x0C != 0; // bits 2 (width) or 3 (height) set
    if resizing {
        let sync_counter = state.windows.get(&wid).and_then(|w| w.sync_request_counter);
        if let Some(counter_id) = sync_counter {
            let net_wm_sync_request_atom = state.intern_atom("_NET_WM_SYNC_REQUEST", false);
            let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
            let supports_sync = state.window_supports_protocol(wid, net_wm_sync_request_atom);
            if supports_sync {
                // Increment the sync request value
                let new_value = state
                    .windows
                    .get(&wid)
                    .map(|w| w.sync_request_value.wrapping_add(1))
                    .unwrap_or(1);
                if let Some(win) = state.windows.get_mut(&wid) {
                    win.sync_request_value = new_value;
                }
                // Update the SYNC counter value
                let lo = new_value as u32;
                let hi = (new_value >> 32) as i32;
                if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
                    counter.value_lo = lo;
                    counter.value_hi = hi;
                }

                // Send _NET_WM_SYNC_REQUEST ClientMessage
                let bo = state.msb_first;
                let seq_num = state.sequence;
                let timestamp = state.timestamp();
                let cm = serialize_event(&ClientMessageEvent {
                    response_type: CLIENT_MESSAGE_EVENT,
                    format: 32,
                    sequence: seq_num,
                    window: wid,
                    type_: wm_protocols_atom,
                    data: ClientMessageData::from([
                        net_wm_sync_request_atom,
                        timestamp,
                        lo,
                        hi as u32,
                        0,
                    ]),
                }, bo);
                state.pending_events.push(cm);
            }
        }
    }

    // Capture old dimensions before the mutable borrow for gravity calculations
    let (old_w, old_h, old_children) = state
        .windows
        .get(&wid)
        .map(|w| (w.width, w.height, w.children_order.clone()))
        .unwrap_or((0, 0, Vec::new()));

    if let Some(win) = state.windows.get_mut(&wid) {
        for bit in 0..7 {
            if value_mask & (1 << bit) != 0 && offset + 4 <= data.len() {
                let val = read_u32_bo(data, offset, msb_first);
                match bit {
                    0 => {
                        win.x = val as i16;
                        changed = true;
                    }
                    1 => {
                        win.y = val as i16;
                        changed = true;
                    }
                    2 => {
                        // Per X11 spec: width must be non-zero
                        if val == 0 {
                            return build_error(BAD_VALUE, seq, 0, 12, 0);
                        }
                        let mut w = val as u16;
                        // Apply size hints (ICCCM §4.1.2.3)
                        if let Some(ref hints) = size_hints {
                            // Round to width_inc steps relative to base_width
                            if hints.width_inc > 1 {
                                let base = if hints.base_width > 0 {
                                    hints.base_width
                                } else {
                                    hints.min_width
                                };
                                if w > base {
                                    let over = (w - base) % hints.width_inc;
                                    if over != 0 {
                                        w -= over;
                                    }
                                }
                            }
                            if hints.min_width > 0 && w < hints.min_width {
                                w = hints.min_width;
                            }
                            if hints.max_width > 0 && w > hints.max_width {
                                w = hints.max_width;
                            }
                        }
                        win.width = w;
                        changed = true;
                    }
                    3 => {
                        // Per X11 spec: height must be non-zero
                        if val == 0 {
                            return build_error(BAD_VALUE, seq, 0, 12, 0);
                        }
                        let mut h = val as u16;
                        // Apply size hints (ICCCM §4.1.2.3)
                        if let Some(ref hints) = size_hints {
                            // Round to height_inc steps relative to base_height
                            if hints.height_inc > 1 {
                                let base = if hints.base_height > 0 {
                                    hints.base_height
                                } else {
                                    hints.min_height
                                };
                                if h > base {
                                    let over = (h - base) % hints.height_inc;
                                    if over != 0 {
                                        h -= over;
                                    }
                                }
                            }
                            if hints.min_height > 0 && h < hints.min_height {
                                h = hints.min_height;
                            }
                            if hints.max_height > 0 && h > hints.max_height {
                                h = hints.max_height;
                            }
                        }
                        win.height = h;
                        changed = true;
                    }
                    4 => {
                        win.border_width = val as u16;
                    }
                    5 => sibling = val,
                    6 => {
                        // Per X11 spec: valid stack modes are 0-4
                        if val > 4 {
                            return build_error(BAD_VALUE, seq, val, 12, 0);
                        }
                        stack_mode = Some(val as u8);
                    }
                    _ => {}
                }
                offset += 4;
            }
        }

        if changed {
            let new_w = win.width;
            let new_h = win.height;

            // Resize the framebuffer if the window dimensions changed
            if new_w as u32 != win.framebuffer.width() || new_h as u32 != win.framebuffer.height() {
                let bg = state.bit_gravity.get(&wid).copied().unwrap_or(0);
                win.framebuffer
                    .resize_with_gravity(new_w as u32, new_h as u32, bg);
            }

            if let Some(ref uuid) = wid_str {
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowConfigured {
                        window_id: uuid.clone(),
                        x: win.x,
                        y: win.y,
                        width: win.width,
                        height: win.height,
                        border_width: win.border_width,
                        border_pixel: win.border_pixel,
                    },
                ));
            }
        }
    }

    // Apply win_gravity to children when parent was resized
    if changed {
        let new_w = state.windows.get(&wid).map(|w| w.width).unwrap_or(old_w);
        let new_h = state.windows.get(&wid).map(|w| w.height).unwrap_or(old_h);
        let dw = new_w as i16 - old_w as i16;
        let dh = new_h as i16 - old_h as i16;

        if (dw != 0 || dh != 0) && !old_children.is_empty() {
            let bo = state.msb_first;

            // Collect children with Unmap gravity (0) for unmap/remap handling.
            let mut unmap_children: Vec<u32> = Vec::new();

            for child_id in &old_children {
                let wg = state.win_gravity.get(child_id).copied().unwrap_or(1);

                // Unmap gravity (0): unmap the child, then remap it after resize.
                if wg == 0 {
                    if let Some(child) = state.windows.get_mut(child_id) {
                        if child.mapped {
                            child.mapped = false;
                            unmap_children.push(*child_id);

                            // Send UnmapNotify for the child
                            let unmap_evt = serialize_event(&UnmapNotifyEvent {
                                response_type: UNMAP_NOTIFY_EVENT,
                                sequence: seq,
                                event: *child_id,
                                window: *child_id,
                                from_configure: false,
                            }, bo);
                            state.pending_events.push(unmap_evt.clone());
                            state.broadcast_event(*child_id, STRUCTURE_NOTIFY_MASK, &unmap_evt);

                            let parent_unmap = serialize_event(&UnmapNotifyEvent {
                                response_type: UNMAP_NOTIFY_EVENT,
                                sequence: seq,
                                event: wid,
                                window: *child_id,
                                from_configure: false,
                            }, bo);
                            state.broadcast_event(wid, SUBSTRUCTURE_NOTIFY_MASK, &parent_unmap);
                        }
                    }
                    continue;
                }

                let (dx, dy) = win_gravity_delta(wg, dw, dh);
                if dx != 0 || dy != 0 {
                    let (cx, cy) = if let Some(child) = state.windows.get_mut(child_id) {
                        child.x = child.x.saturating_add(dx);
                        child.y = child.y.saturating_add(dy);
                        (child.x, child.y)
                    } else {
                        continue;
                    };
                    let event = serialize_event(&GravityNotifyEvent {
                        response_type: GRAVITY_NOTIFY_EVENT,
                        sequence: seq,
                        event: *child_id,
                        window: wid,
                        x: cx,
                        y: cy,
                    }, bo);
                    state.pending_events.push(event.clone());

                    // Cross-connection broadcast: StructureNotify on the child
                    state.broadcast_event(*child_id, STRUCTURE_NOTIFY_MASK, &event);
                    // Cross-connection broadcast: SubstructureNotify on the parent
                    let parent_event = serialize_event(&GravityNotifyEvent {
                        response_type: GRAVITY_NOTIFY_EVENT,
                        sequence: seq,
                        event: wid,
                        window: wid,
                        x: cx,
                        y: cy,
                    }, bo);
                    state.broadcast_event(wid, SUBSTRUCTURE_NOTIFY_MASK, &parent_event);
                }
            }

            // Re-map children that had Unmap gravity after the resize is complete.
            for child_id in &unmap_children {
                if let Some(child) = state.windows.get_mut(child_id) {
                    child.mapped = true;
                }

                let map_evt = serialize_event(&MapNotifyEvent {
                    response_type: MAP_NOTIFY_EVENT,
                    sequence: seq,
                    event: *child_id,
                    window: *child_id,
                    override_redirect: false,
                }, bo);
                state.pending_events.push(map_evt.clone());
                state.broadcast_event(*child_id, STRUCTURE_NOTIFY_MASK, &map_evt);

                let parent_map = serialize_event(&MapNotifyEvent {
                    response_type: MAP_NOTIFY_EVENT,
                    sequence: seq,
                    event: wid,
                    window: *child_id,
                    override_redirect: false,
                }, bo);
                state.broadcast_event(wid, SUBSTRUCTURE_NOTIFY_MASK, &parent_map);
            }
        }
    }

    // Handle stacking order changes.
    // Above=0, Below=1, TopIf=2, BottomIf=3, Opposite=4
    let stacking_changed = if let Some(mode) = stack_mode {
        let parent_id = state.windows.get(&wid).map(|w| w.parent);

        // Per X11 spec §12.6: if a sibling is specified, it must be an actual
        // sibling (same parent) of the window being configured. Return BadMatch
        // if the sibling doesn't share the same parent.
        if sibling != 0 {
            if let Some(pid) = parent_id {
                let sibling_is_sibling = state
                    .windows
                    .get(&sibling)
                    .map(|s| s.parent == pid)
                    .unwrap_or(false);
                if !sibling_is_sibling {
                    return build_error(BAD_MATCH, seq, 0, 12, 0);
                }
            }
        }

        if let Some(parent_id) = parent_id {
            let raised = if mode == 0 {
                // Above: raise to top of stacking layer (or above sibling)
                if sibling != 0 {
                    // Explicit sibling: place above it (per X11 spec, client knows what it wants)
                    if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                        parent_win.children_order.retain(|&c| c != wid);
                        if let Some(pos) =
                            parent_win.children_order.iter().position(|&c| c == sibling)
                        {
                            parent_win.children_order.insert(pos + 1, wid);
                        } else {
                            parent_win.children_order.push(wid);
                        }
                    }
                } else {
                    // No sibling: raise to top of this window's stacking layer
                    super::restack_by_window_type(state, wid, parent_id);
                }
                true
            } else if mode == 1 {
                // Below: lower to bottom of stacking layer (or below sibling)
                if sibling != 0 {
                    // Explicit sibling: place below it
                    if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                        parent_win.children_order.retain(|&c| c != wid);
                        if let Some(pos) =
                            parent_win.children_order.iter().position(|&c| c == sibling)
                        {
                            parent_win.children_order.insert(pos, wid);
                        } else {
                            parent_win.children_order.insert(0, wid);
                        }
                    }
                } else {
                    // No sibling: lower to bottom of this window's stacking layer
                    let target_layer = state
                        .windows
                        .get(&wid)
                        .map(super::effective_stacking_layer)
                        .unwrap_or(2);

                    if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                        parent_win.children_order.retain(|&c| c != wid);
                    }

                    let children: Vec<(u32, u8)> = state
                        .windows
                        .get(&parent_id)
                        .map(|p| {
                            p.children_order
                                .iter()
                                .map(|&c| {
                                    let layer = state
                                        .windows
                                        .get(&c)
                                        .map(super::effective_stacking_layer)
                                        .unwrap_or(2);
                                    (c, layer)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    // Insert at the first position where layer >= target_layer
                    let insert_pos = children
                        .iter()
                        .position(|(_, layer)| *layer >= target_layer)
                        .unwrap_or(children.len());

                    if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                        parent_win.children_order.insert(insert_pos, wid);
                    }
                }
                false
            } else {
                // TopIf/BottomIf/Opposite: proper X11 occlusion semantics.
                // children_order is bottom-to-top, so later index = above.

                // Gather geometry for the target window.
                let win_geom = state.windows.get(&wid).map(|w| {
                    let bw = w.border_width as i32;
                    (
                        w.x as i32,
                        w.y as i32,
                        w.width as i32 + 2 * bw,
                        w.height as i32 + 2 * bw,
                    )
                });

                // Helper: check if two bounding rects overlap.
                let rects_overlap = |ax: i32,
                                     ay: i32,
                                     aw: i32,
                                     ah: i32,
                                     bx: i32,
                                     by: i32,
                                     bw: i32,
                                     bh: i32|
                 -> bool {
                    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
                };

                // Get children_order and build sibling geometries.
                let children = state
                    .windows
                    .get(&parent_id)
                    .map(|w| w.children_order.clone())
                    .unwrap_or_default();
                let win_idx = children.iter().position(|&c| c == wid);

                if let (Some((wx, wy, ww, wh)), Some(win_idx)) = (win_geom, win_idx) {
                    // Check if any sibling above occludes us (for TopIf / Opposite).
                    let occluded_by_above = |specific_sibling: u32| -> bool {
                        if specific_sibling != 0 {
                            // Only check the specified sibling, and only if it's above.
                            if let Some(sib_idx) =
                                children.iter().position(|&c| c == specific_sibling)
                            {
                                if sib_idx > win_idx {
                                    if let Some(s) = state.windows.get(&specific_sibling) {
                                        let sbw = s.border_width as i32;
                                        return rects_overlap(
                                            wx,
                                            wy,
                                            ww,
                                            wh,
                                            s.x as i32,
                                            s.y as i32,
                                            s.width as i32 + 2 * sbw,
                                            s.height as i32 + 2 * sbw,
                                        );
                                    }
                                }
                            }
                            false
                        } else {
                            // Check all siblings above.
                            for &sib_id in &children[win_idx + 1..] {
                                if let Some(s) = state.windows.get(&sib_id) {
                                    let sbw = s.border_width as i32;
                                    if rects_overlap(
                                        wx,
                                        wy,
                                        ww,
                                        wh,
                                        s.x as i32,
                                        s.y as i32,
                                        s.width as i32 + 2 * sbw,
                                        s.height as i32 + 2 * sbw,
                                    ) {
                                        return true;
                                    }
                                }
                            }
                            false
                        }
                    };

                    // Check if we occlude any sibling below (for BottomIf / Opposite).
                    let occludes_below = |specific_sibling: u32| -> bool {
                        if specific_sibling != 0 {
                            // Only check the specified sibling, and only if it's below.
                            if let Some(sib_idx) =
                                children.iter().position(|&c| c == specific_sibling)
                            {
                                if sib_idx < win_idx {
                                    if let Some(s) = state.windows.get(&specific_sibling) {
                                        let sbw = s.border_width as i32;
                                        return rects_overlap(
                                            wx,
                                            wy,
                                            ww,
                                            wh,
                                            s.x as i32,
                                            s.y as i32,
                                            s.width as i32 + 2 * sbw,
                                            s.height as i32 + 2 * sbw,
                                        );
                                    }
                                }
                            }
                            false
                        } else {
                            // Check all siblings below.
                            for &sib_id in &children[..win_idx] {
                                if let Some(s) = state.windows.get(&sib_id) {
                                    let sbw = s.border_width as i32;
                                    if rects_overlap(
                                        wx,
                                        wy,
                                        ww,
                                        wh,
                                        s.x as i32,
                                        s.y as i32,
                                        s.width as i32 + 2 * sbw,
                                        s.height as i32 + 2 * sbw,
                                    ) {
                                        return true;
                                    }
                                }
                            }
                            false
                        }
                    };

                    if mode == 2 {
                        // TopIf: raise only if occluded by a sibling (or the specified sibling).
                        if occluded_by_above(sibling) {
                            if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                                parent_win.children_order.retain(|&c| c != wid);
                                parent_win.children_order.push(wid);
                            }
                            true
                        } else {
                            false
                        }
                    } else if mode == 3 {
                        // BottomIf: lower only if we occlude a sibling (or the specified sibling).
                        if occludes_below(sibling) {
                            if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                                parent_win.children_order.retain(|&c| c != wid);
                                parent_win.children_order.insert(0, wid);
                            }
                            false
                        } else {
                            false
                        }
                    } else {
                        // Opposite (mode == 4): raise if occluded, lower if occluding, else no-op.
                        if occluded_by_above(sibling) {
                            if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                                parent_win.children_order.retain(|&c| c != wid);
                                parent_win.children_order.push(wid);
                            }
                            true
                        } else if occludes_below(sibling) {
                            if let Some(parent_win) = state.windows.get_mut(&parent_id) {
                                parent_win.children_order.retain(|&c| c != wid);
                                parent_win.children_order.insert(0, wid);
                            }
                            false
                        } else {
                            false
                        }
                    }
                } else {
                    // Window not found in children_order; no-op.
                    false
                }
            };

            // Send WindowRaised to frontend when the window is raised
            if raised {
                if let Some(ref uuid) = wid_str {
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowRaised {
                            window_id: uuid.clone(),
                        },
                    ));
                }
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    // Send ConfigureNotify when geometry or stacking changed
    if changed || stacking_changed {
        if let Some(win) = state.windows.get(&wid) {
            let parent_id = win.parent;
            let override_redirect = win.override_redirect;
            let x = win.x;
            let y = win.y;
            let width = win.width;
            let height = win.height;
            let border_width = win.border_width;

            // Compute the above_sibling: the window directly below this one in
            // the parent's stacking order (per X11 spec ConfigureNotify).
            let above_sibling = state
                .windows
                .get(&parent_id)
                .and_then(|parent| {
                    let pos = parent.children_order.iter().position(|&id| id == wid)?;
                    if pos > 0 {
                        Some(parent.children_order[pos - 1])
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            // _NET_WM_SYNC_REQUEST: if the window has a sync counter and size is
            // changing, send a WM_PROTOCOLS ClientMessage with _NET_WM_SYNC_REQUEST
            // before the ConfigureNotify per EWMH spec. This lets the client
            // synchronize its repainting with the resize.
            if width != old_w || height != old_h {
                let sync_counter = state.windows.get(&wid).and_then(|w| w.sync_request_counter);
                if let Some(_counter_id) = sync_counter {
                    let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
                    let sync_request_atom = state.intern_atom("_NET_WM_SYNC_REQUEST", false);

                    // Increment the sync request value
                    let new_value = state
                        .windows
                        .get(&wid)
                        .map(|w| w.sync_request_value + 1)
                        .unwrap_or(1);
                    if let Some(win) = state.windows.get_mut(&wid) {
                        win.sync_request_value = new_value;
                    }

                    let lo = (new_value & 0xFFFFFFFF) as u32;
                    let hi = (new_value >> 32) as u32;
                    let timestamp = state.timestamp();

                    let sync_msg = serialize_event(&ClientMessageEvent {
                        response_type: CLIENT_MESSAGE_EVENT,
                        format: 32,
                        sequence: 0,
                        window: wid,
                        type_: wm_protocols_atom,
                        data: ClientMessageData::from([
                            sync_request_atom,
                            timestamp,
                            lo,
                            hi,
                            0,
                        ]),
                    }, msb_first);

                    state.pending_events.push(sync_msg.clone());
                    // Also deliver to cross-connection clients
                    state.broadcast_event(wid, STRUCTURE_NOTIFY_MASK, &sync_msg);
                }
            }

            // Build the ConfigureNotify event
            let event = serialize_event(&ConfigureNotifyEvent {
                response_type: CONFIGURE_NOTIFY_EVENT,
                sequence: seq,
                event: wid,
                window: wid,
                above_sibling,
                x,
                y,
                width,
                height,
                border_width,
                override_redirect,
            }, msb_first);

            // Also send to parent with SubstructureNotifyMask
            {
                let parent_event = serialize_event(&ConfigureNotifyEvent {
                    response_type: CONFIGURE_NOTIFY_EVENT,
                    sequence: seq,
                    event: parent_id,
                    window: wid,
                    above_sibling,
                    x,
                    y,
                    width,
                    height,
                    border_width,
                    override_redirect,
                }, msb_first);
                if let Some(parent_win) = state.windows.get(&parent_id) {
                    if parent_win.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0 {
                        state.pending_events.push(parent_event.clone());
                    }
                }
                // Cross-connection broadcast: SubstructureNotify on parent
                state.broadcast_event(parent_id, SUBSTRUCTURE_NOTIFY_MASK, &parent_event);
            }
            // Deliver StructureNotify to the window's own client if subscribed
            if let Some(win) = state.windows.get(&wid) {
                if win.event_mask & STRUCTURE_NOTIFY_MASK != 0 {
                    state.pending_events.push(event.clone());
                }
            }
            // Cross-connection broadcast: StructureNotify on the window itself
            state.broadcast_event(wid, STRUCTURE_NOTIFY_MASK, &event);

            // Generate Expose event when window size changed (apps need this to redraw)
            let size_changed = width != old_w || height != old_h;
            if size_changed {
                let win_mask = state.windows.get(&wid).map(|w| w.event_mask).unwrap_or(0);

                // Also expose mapped descendants
                let descendants: Vec<(u32, u16, u16)> = state
                    .windows
                    .values()
                    .filter(|w| {
                        w.mapped && w.id != wid && is_descendant_of(&state.windows, w.id, wid)
                    })
                    .filter(|w| w.event_mask & EXPOSURE_MASK != 0)
                    .map(|w| (w.id, w.width, w.height))
                    .collect();

                // Total expose events: 1 (self, if selected) + descendants.len()
                let self_selected = win_mask & EXPOSURE_MASK != 0;
                let total = if self_selected { 1 } else { 0 } + descendants.len();

                // Build the Expose event for cross-connection broadcast
                let expose = serialize_event(&ExposeEvent {
                    response_type: EXPOSE_EVENT,
                    sequence: seq,
                    window: wid,
                    x: 0,
                    y: 0,
                    width,
                    height,
                    count: (total - 1) as u16,
                }, msb_first);
                if self_selected {
                    state.pending_events.push(expose.clone());
                }
                // Broadcast to other clients that selected ExposureMask
                state.broadcast_event(wid, EXPOSURE_MASK, &expose);

                for (i, (desc_id, dw, dh)) in descendants.iter().enumerate() {
                    let base = if self_selected { 1 } else { 0 };
                    let remaining = (total - base - 1 - i) as u16;
                    let exp = serialize_event(&ExposeEvent {
                        response_type: EXPOSE_EVENT,
                        sequence: seq,
                        window: *desc_id,
                        x: 0,
                        y: 0,
                        width: *dw,
                        height: *dh,
                        count: remaining,
                    }, msb_first);
                    state.pending_events.push(exp);
                }
            }

            // Notify Present extension subscribers about the reconfiguration
            super::extensions::send_present_config_notify(
                state, wid, x, y, width, height, 0, 0, // off_x, off_y
                width, height, // pixmap_width, pixmap_height = window size
                0,      // pixmap_flags
            );

            // Recalculate and send VisibilityNotify for affected siblings.
            // Geometry changes (move/resize) can also affect occlusion, not
            // just stacking order changes.
            if stacking_changed || changed {
                update_sibling_visibility(state, wid, seq, msb_first);
            }

            return event;
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 13: CirculateWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_circulate_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 13);

    let direction = data[1]; // 0 = RaiseLowest, 1 = LowerHighest
    let window = state.read_u32(data, 4);

    if !state.windows.contains_key(&window) {
        return build_error_bo(BAD_WINDOW, seq, window, 13, 0, state.msb_first);
    }

    // Get the parent's children_order to find the target child
    let children: Vec<u32> = state
        .windows
        .get(&window)
        .map(|w| w.children_order.clone())
        .unwrap_or_default();

    if children.len() < 2 {
        // Nothing to circulate with fewer than 2 children
        return Vec::new();
    }

    // Find the mapped children only
    let mapped_children: Vec<u32> = children
        .iter()
        .filter(|&&c| state.windows.get(&c).map(|w| w.mapped).unwrap_or(false))
        .copied()
        .collect();

    if mapped_children.len() < 2 {
        return Vec::new();
    }

    let target_child = if direction == 0 {
        // RaiseLowest: find the lowest (first in stacking order) mapped child and raise it
        mapped_children[0]
    } else {
        // LowerHighest: find the highest (last in stacking order) mapped child and lower it
        mapped_children[mapped_children.len() - 1]
    };

    // Check if parent has SubstructureRedirectMask - if so, generate CirculateRequest instead
    let parent_redirect = state
        .windows
        .get(&window)
        .map(|w| w.event_mask & SUBSTRUCTURE_REDIRECT_MASK != 0)
        .unwrap_or(false);

    let bo = state.msb_first;

    if parent_redirect {
        // Generate CirculateRequest event (code 27) instead of performing the operation
        let event = serialize_event(&CirculateNotifyEvent {
            response_type: CIRCULATE_REQUEST_EVENT,
            sequence: seq,
            event: window,
            window: target_child,
            place: direction.into(),
        }, bo);
        state.pending_events.push(event.clone());
        // Per X11 spec, deliver CirculateRequest to all SubstructureRedirectMask selectors
        state.broadcast_event(window, SUBSTRUCTURE_REDIRECT_MASK, &event);
        return Vec::new();
    }

    // Actually perform the circulation
    if let Some(parent_win) = state.windows.get_mut(&window) {
        if direction == 0 {
            // RaiseLowest: move target_child to end of children_order (top of stack)
            parent_win.children_order.retain(|&c| c != target_child);
            parent_win.children_order.push(target_child);
        } else {
            // LowerHighest: move target_child to beginning of children_order (bottom of stack)
            parent_win.children_order.retain(|&c| c != target_child);
            parent_win.children_order.insert(0, target_child);
        }
    }

    // Send WindowRaised to frontend when a window is raised to top
    if direction == 0 {
        if let Some(uuid) = state.window_uuid(target_child) {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowRaised { window_id: uuid },
            ));
        }
    }

    // Generate CirculateNotify event (code 26)
    // Deliver to: the window itself (StructureNotify) and parent (SubstructureNotify)
    let place = if direction == 0 { 0u8 } else { 1u8 }; // 0=Top, 1=Bottom
    let structure_mask = state
        .windows
        .get(&target_child)
        .map(|w| w.event_mask & STRUCTURE_NOTIFY_MASK != 0)
        .unwrap_or(false);
    let substructure_mask = state
        .windows
        .get(&window)
        .map(|w| w.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0)
        .unwrap_or(false);

    {
        let event = serialize_event(&CirculateNotifyEvent {
            response_type: CIRCULATE_NOTIFY_EVENT,
            sequence: seq,
            event: target_child,
            window: target_child,
            place: place.into(),
        }, bo);
        if structure_mask {
            state.pending_events.push(event.clone());
        }
        // Cross-connection broadcast: StructureNotify on the circulated child
        state.broadcast_event(target_child, STRUCTURE_NOTIFY_MASK, &event);
    }

    {
        let event = serialize_event(&CirculateNotifyEvent {
            response_type: CIRCULATE_NOTIFY_EVENT,
            sequence: seq,
            event: window,
            window: target_child,
            place: place.into(),
        }, bo);
        if substructure_mask {
            state.pending_events.push(event.clone());
        }
        // Cross-connection broadcast: SubstructureNotify on the parent
        state.broadcast_event(window, SUBSTRUCTURE_NOTIFY_MASK, &event);
    }

    Vec::new()
}
