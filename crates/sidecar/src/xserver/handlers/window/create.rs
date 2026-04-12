//! Create/destroy window handlers (opcodes 1, 4, 5).

use super::*;

// ---------------------------------------------------------------------------
// Opcode 1: CreateWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_window(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 32 {
        return build_error(BAD_LENGTH, _seq, 0, 1, 0);
    }

    let wid = state.read_u32(data, 4);

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(wid) {
        return build_error(BAD_ID_CHOICE, _seq, wid, 1, 0);
    }

    let parent = state.read_u32(data, 8);
    let x = state.read_i16(data, 12);
    let y = state.read_i16(data, 14);
    let width = state.read_u16(data, 16);
    let height = state.read_u16(data, 18);
    let border_width = state.read_u16(data, 20);
    let class = state.read_u16(data, 22);
    let visual = state.read_u32(data, 24);
    let value_mask = state.read_u32(data, 28);

    // Validate value-list length matches the bitmask
    let n_values = value_mask.count_ones() as usize;
    let required_len = 32 + n_values * 4;
    if data.len() < required_len {
        return build_error(BAD_LENGTH, _seq, 0, 1, 0);
    }

    // Validate window dimensions are within reasonable bounds
    if width > 32767 || height > 32767 {
        return build_error(BAD_VALUE, _seq, width as u32, 1, 0);
    }

    let mut background_pixel = 0u32;
    let mut background_pixmap: Option<u32> = None;
    let mut border_pixel = 0u32;
    let mut border_pixmap: Option<u32> = None;
    let mut bit_gravity: u8 = 0;  // Forget
    let mut win_gravity: u8 = 1;  // NorthWest (default per spec)
    let mut backing_store: u8 = 0; // NotUseful
    let mut backing_planes: u32 = 0xFFFFFFFF; // all planes
    let mut backing_pixel: u32 = 0;
    let mut save_under = false;
    let mut event_mask = 0u32;
    let mut do_not_propagate_mask = 0u32;
    let mut override_redirect = false;
    let mut cursor_id: Option<u32> = None;
    let mut colormap_id: u32 = 0; // 0 = CopyFromParent (inherits parent's colormap)

    // Parse value list
    let mut offset = 32;
    for bit in 0..15 {
        if value_mask & (1 << bit) != 0
            && offset + 4 <= data.len() {
                let val = state.read_u32(data, offset);
                match bit {
                    0 => {
                        // background-pixmap: 0=None, 1=ParentRelative, else pixmap ID
                        background_pixmap = Some(val);
                    }
                    1 => background_pixel = val,
                    2 => {
                        // border-pixmap: 0=CopyFromParent, else pixmap ID
                        border_pixmap = Some(val);
                    }
                    3 => border_pixel = val,
                    4 => {
                        if val > 10 { return build_error(BAD_VALUE, _seq, val, 1, 0); }
                        bit_gravity = val as u8;
                    }
                    5 => {
                        if val > 10 { return build_error(BAD_VALUE, _seq, val, 1, 0); }
                        win_gravity = val as u8;
                    }
                    6 => {
                        if val > 2 { return build_error(BAD_VALUE, _seq, val, 1, 0); }
                        backing_store = val as u8;
                    }
                    7 => backing_planes = val,
                    8 => backing_pixel = val,
                    9 => override_redirect = val != 0,
                    10 => save_under = val != 0,
                    11 => event_mask = val,
                    12 => do_not_propagate_mask = val,
                    13 => colormap_id = val, // colormap
                    14 => {
                        if val != 0 {
                            // Validate cursor ID exists
                            if !state.cursors.contains_key(&val) {
                                return build_error(BAD_CURSOR, _seq, val, 1, 0);
                            }
                            cursor_id = Some(val);
                        }
                    }
                    _ => {}
                }
                offset += 4;
            }
    }

    // Per X11 spec Section 12.3: SubstructureRedirectMask and ResizeRedirectMask
    // may only be selected by ONE client on a given window.  Check for conflicts
    // before inserting the new window so we never leave partial state behind.
    if event_mask != 0 {
        if let Some(_conflict) = state.event_broadcaster
            .check_redirect_conflict(parent, event_mask, &state.client_id)
        {
            return build_error(BAD_ACCESS, _seq, 0, 1, 0);
        }
        // Also check on the new window itself (no other client can have selected
        // the redirect masks on wid yet since wid is brand-new, but validate
        // against the window's own event_mask for completeness).
    }

    let use_visual = if visual == 0 { ROOT_VISUAL } else { visual };

    info!("CreateWindow: id={wid:#x} parent={parent:#x} {x},{y} {width}x{height} depth={} class={class} visual={visual:#x} bg={background_pixel:#x}", data[1]);

    // InputOnly windows (class=2) must not have backgrounds, borders, or framebuffers.
    // They exist only to receive events. Per spec, depth must be 0 for InputOnly.
    let is_input_only = class == 2;
    let fb = if is_input_only {
        Framebuffer::new(0, 0) // zero-size: no pixel storage
    } else {
        Framebuffer::new(width as u32, height as u32)
    };

    state.windows.insert(
        wid,
        WindowState {
            id: wid,
            parent,
            x,
            y,
            width,
            height,
            border_width: if is_input_only { 0 } else { border_width },
            visual: if is_input_only { 0 } else { use_visual },
            class,
            mapped: false,
            event_mask,
            do_not_propagate_mask,
            background_pixel: if is_input_only { 0 } else { background_pixel },
            background_pixmap: if is_input_only { None } else { background_pixmap },
            border_pixel: if is_input_only { 0 } else { border_pixel },
            border_pixmap: if is_input_only { None } else { border_pixmap },
            override_redirect,
            redirected: false,
            framebuffer: fb,
            properties: HashMap::new(),
            owner_client_id: state.client_id.clone(),
            cursor: cursor_id,
            children_order: Vec::new(),
            retained_temporary: false,
            bounding_shape: None,
            clip_shape: None,
            input_shape: None,
            shape_select_clients: Vec::new(),
            colormap: colormap_id,
            backing_store: if is_input_only { 0 } else { backing_store },
            backing_planes: if is_input_only { 0xFFFFFFFF } else { backing_planes },
            backing_pixel: if is_input_only { 0 } else { backing_pixel },
            save_under: if is_input_only { false } else { save_under },
            visibility: 0,
            backing_pixmap: None,
            wm_hints_initial_state: None,
            transient_for: None,
            sync_request_counter: None,
            sync_request_value: 0,
            window_type: WindowType::Normal,
            strut: None,
        },
    );

    // Register the creating client with the event broadcaster so that
    // cross-client operations (e.g. another client calling ChangeProperty on
    // this window) can deliver events back to the owner.
    if event_mask != 0 {
        state.subscribe_to_window_events(wid, event_mask);
    }

    // Store gravity values
    state.bit_gravity.insert(wid, bit_gravity);
    state.win_gravity.insert(wid, win_gravity);

    // Add new window to parent's children_order
    if let Some(parent_win) = state.windows.get_mut(&parent) {
        parent_win.children_order.push(wid);
    }

    // Set _NET_FRAME_EXTENTS = (0,0,0,0) on new windows -- GTK3 checks this.
    let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties.insert(atom_frame, PropertyValue {
            prop_type: 6, // CARDINAL
            format: 32,
            data: vec![0; 16], // left, right, top, bottom = 0
        });
    }

    let is_top_level = parent == state.root_window && class == 1 && !override_redirect;
    let wid_str = state.get_or_create_window_uuid(wid);
    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowCreated {
            window_id: wid_str,
            x,
            y,
            width,
            height,
            is_top_level,
            override_redirect,
            border_width: if is_input_only { 0 } else { border_width },
            border_pixel: if is_input_only { 0 } else { border_pixel },
        },
    ));

    // Send CreateNotify to parent if it has SubstructureNotifyMask
    if let Some(parent_win) = state.windows.get(&parent) {
        if parent_win.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0 {
            let mut event = [0u8; 32];
            event[0] = CREATE_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, _seq);
            state.write_u32(&mut event, 4, parent);
            state.write_u32(&mut event, 8, wid);
            state.write_i16(&mut event, 12, x);
            state.write_i16(&mut event, 14, y);
            state.write_u16(&mut event, 16, width);
            state.write_u16(&mut event, 18, height);
            state.write_u16(&mut event, 20, border_width);
            event[22] = if override_redirect { 1 } else { 0 };
            state.pending_events.push(event.to_vec());

            // Cross-connection broadcast: other clients watching SubstructureNotify on parent
            state.broadcast_event(parent, SUBSTRUCTURE_NOTIFY_MASK, &event);
        }
    }

    Vec::new() // No reply for CreateWindow
}

// ---------------------------------------------------------------------------
// Opcode 4: DestroyWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_window(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 { return build_error(BAD_LENGTH, state.sequence, 0, 4, 0); }
    let wid = state.read_u32(data, 4);

    if !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, state.sequence, wid, 4, 0);
    }

    // Check if window had struts (for workarea recalculation after removal)
    let had_strut = state.windows.get(&wid).is_some_and(|w| w.strut.is_some());

    // Generate DestroyNotify events before removing the window
    let parent_id = state.windows.get(&wid).map(|w| w.parent);
    if let Some(parent_id) = parent_id {
        // Send DestroyNotify to the window itself (StructureNotifyMask)
        {
            let mut event = [0u8; 32];
            event[0] = DESTROY_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, wid);
            state.write_u32(&mut event, 8, wid);

            if let Some(win) = state.windows.get(&wid) {
                if win.event_mask & STRUCTURE_NOTIFY_MASK != 0 {
                    state.pending_events.push(event.to_vec());
                }
            }
            // Cross-connection broadcast: StructureNotify on the window
            state.broadcast_event(wid, STRUCTURE_NOTIFY_MASK, &event);
        }

        // Send DestroyNotify to parent (SubstructureNotifyMask)
        {
            let mut event = [0u8; 32];
            event[0] = DESTROY_NOTIFY_EVENT;
            state.write_u16(&mut event, 2, state.sequence);
            state.write_u32(&mut event, 4, parent_id);
            state.write_u32(&mut event, 8, wid);

            if let Some(parent_win) = state.windows.get(&parent_id) {
                if parent_win.event_mask & SUBSTRUCTURE_NOTIFY_MASK != 0 {
                    state.pending_events.push(event.to_vec());
                }
            }
            // Cross-connection broadcast: SubstructureNotify on the parent
            state.broadcast_event(parent_id, SUBSTRUCTURE_NOTIFY_MASK, &event);
        }
    }

    // Revert focus if this was the focus window
    state.revert_focus_from(wid);

    // Remove from parent's children_order
    if let Some(parent_id) = parent_id {
        if let Some(parent_win) = state.windows.get_mut(&parent_id) {
            parent_win.children_order.retain(|&c| c != wid);
        }
    }

    state.windows.remove(&wid);
    state.gtk_menu_paths.remove(&wid);
    state.menu_tracker.window_index().unregister(wid);

    // Remove from shared window registry so other connections see the destroy immediately
    if let Ok(mut shared) = state.shared_windows.lock() {
        shared.remove(&wid);
    }

    if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
        state.window_router.unregister_all(std::slice::from_ref(&uuid));
        state.menu_tracker.detach(&uuid);
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowDestroyed { window_id: uuid },
        ));
    }

    // Update _NET_CLIENT_LIST on root
    state.update_net_client_list();

    // If the destroyed window had struts, recalculate workarea
    if had_strut {
        state.recalculate_workarea();
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 5: DestroySubwindows
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_subwindows(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return build_error(BAD_LENGTH, state.sequence, 0, 5, 0);
    }
    let parent = state.read_u32(data, 4);

    if !state.windows.contains_key(&parent) {
        return build_error(BAD_WINDOW, state.sequence, parent, 5, 0);
    }

    // Collect all direct children first
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent)
        .map(|w| w.id)
        .collect();

    // Recursively collect all descendants (depth-first)
    let mut all_descendants = Vec::new();
    let mut stack = children.clone();
    while let Some(wid) = stack.pop() {
        all_descendants.push(wid);
        let grandchildren: Vec<u32> = state
            .windows
            .values()
            .filter(|w| w.parent == wid)
            .map(|w| w.id)
            .collect();
        stack.extend(grandchildren);
    }

    // Revert focus if it points to any of the descendants being destroyed
    if all_descendants.contains(&state.focus_window) {
        state.revert_focus_from(state.focus_window);
    }

    // Remove all descendants from shared window registry
    if let Ok(mut shared) = state.shared_windows.lock() {
        for &wid in &all_descendants {
            shared.remove(&wid);
        }
    }

    // Destroy all descendants
    for wid in all_descendants {
        state.windows.remove(&wid);
        state.gtk_menu_paths.remove(&wid);
        state.menu_tracker.window_index().unregister(wid);
        if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
            state.window_router.unregister_all(std::slice::from_ref(&uuid));
            state.menu_tracker.detach(&uuid);
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowDestroyed { window_id: uuid },
            ));
        }
    }

    Vec::new()
}
