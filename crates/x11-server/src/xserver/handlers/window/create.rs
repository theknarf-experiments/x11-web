//! Create/destroy window handlers (opcodes 1, 4, 5).

use super::*;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{
    BackingStore, CreateNotifyEvent, CreateWindowRequest, DestroyNotifyEvent,
    DestroySubwindowsRequest, DestroyWindowRequest, WindowClass,
};

// ---------------------------------------------------------------------------
// Opcode 1: CreateWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_window(state: &mut ClientState, req: &CreateWindowRequest) -> Vec<u8> {
    let _seq = state.sequence;
    let wid = req.wid;
    let parent = req.parent;
    let x = req.x;
    let y = req.y;
    let width = req.width;
    let height = req.height;
    let border_width = req.border_width;
    let raw_class: u16 = req.class.into();
    let req_depth = req.depth;
    let visual = req.visual;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(wid) {
        return build_error(ID_CHOICE_ERROR, _seq, wid, 1, 0);
    }
    // Per X11 spec: the resource ID must not already be in use on the
    // server. Check both this client's local windows and the shared
    // registry (which holds windows from any client).
    if state.windows.contains_key(&wid)
        || state
            .shared_windows
            .lock()
            .ok()
            .is_some_and(|s| s.contains_key(&wid))
    {
        return build_error(ID_CHOICE_ERROR, _seq, wid, 1, 0);
    }

    // Enforce per-client window resource limit
    if !state.can_create_window() {
        return build_error(ALLOC_ERROR, _seq, wid, 1, 0);
    }

    // Per X11 spec: class 0 = CopyFromParent, resolved to parent's class.
    // Root window is always InputOutput.
    let class = if raw_class == u16::from(WindowClass::COPY_FROM_PARENT) {
        state
            .windows
            .get(&parent)
            .map(|w| w.class)
            .unwrap_or(u16::from(WindowClass::INPUT_OUTPUT))
    } else {
        raw_class
    };

    // Validate parent window exists (root window or a window we know about).
    if parent != state.root_window && !state.windows.contains_key(&parent) {
        // Also check shared windows for cross-connection parents.
        let parent_exists = state
            .shared_windows
            .lock()
            .ok()
            .is_some_and(|s| s.contains_key(&parent));
        if !parent_exists {
            return build_error(WINDOW_ERROR, _seq, parent, 1, 0);
        }
    }

    // Per X11 spec: width and height must be non-zero and fit in 16 bits.
    // Zero-size windows are rejected with BadValue.
    if width == 0 || height == 0 || width > 32767 || height > 32767 {
        return build_error(
            VALUE_ERROR,
            _seq,
            if width == 0 { 0 } else { width as u32 },
            1,
            0,
        );
    }

    // Per X11 spec: if depth is specified (non-zero) and doesn't match the
    // visual's depth, return BadMatch.  0 means CopyFromParent.
    let is_input_only_class = class == u16::from(WindowClass::INPUT_ONLY);
    if req_depth != 0 && !is_input_only_class {
        let use_visual = if visual == 0 { ROOT_VISUAL } else { visual };
        let visual_depth = crate::xserver::core::depth_for_visual(use_visual);
        if req_depth != visual_depth {
            return build_error(MATCH_ERROR, _seq, 0, 1, 0);
        }
    }

    // Extract value_list fields from the parsed request
    let vl = &*req.value_list;

    let background_pixmap: Option<u32> = vl.background_pixmap;
    let background_pixel: u32 = vl.background_pixel.unwrap_or(0);
    let border_pixmap: Option<u32> = vl.border_pixmap;
    let border_pixel: u32 = vl.border_pixel.unwrap_or(0);

    let bit_gravity_val: u32 = vl.bit_gravity.map(u32::from).unwrap_or(0);
    if vl.bit_gravity.is_some() && bit_gravity_val > 10 {
        return build_error(VALUE_ERROR, _seq, bit_gravity_val, 1, 0);
    }
    let bit_gravity: u8 = bit_gravity_val as u8;

    let win_gravity_val: u32 = vl.win_gravity.map(u32::from).unwrap_or(1);
    if vl.win_gravity.is_some() && win_gravity_val > 10 {
        return build_error(VALUE_ERROR, _seq, win_gravity_val, 1, 0);
    }
    let win_gravity: u8 = win_gravity_val as u8; // default NorthWest (1)

    let backing_store_val: u32 = vl
        .backing_store
        .map(u32::from)
        .unwrap_or(u32::from(BackingStore::NOT_USEFUL));
    if vl.backing_store.is_some() && backing_store_val > u32::from(BackingStore::ALWAYS) {
        return build_error(VALUE_ERROR, _seq, backing_store_val, 1, 0);
    }
    let backing_store: u8 = backing_store_val as u8;

    let backing_planes: u32 = vl.backing_planes.unwrap_or(0xFFFFFFFF);
    let backing_pixel: u32 = vl.backing_pixel.unwrap_or(0);
    let override_redirect: bool = vl.override_redirect.unwrap_or(0) != 0;
    let save_under: bool = vl.save_under.unwrap_or(0) != 0;
    let event_mask: u32 = vl.event_mask.map(u32::from).unwrap_or(0);
    // Note: x11rb spells this "do_not_propogate_mask" (upstream typo)
    let do_not_propagate_mask: u32 = vl.do_not_propogate_mask.map(u32::from).unwrap_or(0);
    let colormap_id: u32 = vl.colormap.unwrap_or(0);

    let cursor_id: Option<u32> = if let Some(c) = vl.cursor {
        if c != 0 {
            // Validate cursor ID exists
            if !state.cursors.contains_key(&c) {
                return build_error(CURSOR_ERROR, _seq, c, 1, 0);
            }
            Some(c)
        } else {
            None
        }
    } else {
        None
    };

    // Per X11 spec Section 12.3: SubstructureRedirectMask and ResizeRedirectMask
    // may only be selected by ONE client on a given window.  Check for conflicts
    // before inserting the new window so we never leave partial state behind.
    if event_mask != 0 {
        if let Some(_conflict) =
            state
                .event_broadcaster
                .check_redirect_conflict(parent, event_mask, &state.client_id)
        {
            return build_error(ACCESS_ERROR, _seq, 0, 1, 0);
        }
        // Also check on the new window itself (no other client can have selected
        // the redirect masks on wid yet since wid is brand-new, but validate
        // against the window's own event_mask for completeness).
    }

    // visual=CopyFromParent (0) means inherit from parent; resolve it at
    // create time so we always store a real visual ID. InputOnly windows
    // legally pass CopyFromParent and must get the parent's visual too —
    // GTK/GDK clients call XGetWindowAttributes on these windows and pass
    // the visual through XVisualIDFromVisual; if we stored 0, Xlib's
    // _XVIDtoVisual returns NULL and the client crashes (see Firefox/GTK3
    // SIGSEGV).
    let parent_visual = state
        .windows
        .get(&parent)
        .map(|p| p.visual)
        .filter(|v| *v != 0)
        .unwrap_or(ROOT_VISUAL);
    let use_visual = if visual == 0 { parent_visual } else { visual };

    info!("CreateWindow: id={wid:#x} parent={parent:#x} {x},{y} {width}x{height} depth={req_depth} class={class} visual={visual:#x} bg={background_pixel:#x}");

    // InputOnly windows must not have backgrounds, borders, or framebuffers.
    // They exist only to receive events. Per spec, depth must be 0 for InputOnly.
    let is_input_only = class == u16::from(WindowClass::INPUT_ONLY);
    let use_depth = if is_input_only {
        0
    } else {
        crate::xserver::core::depth_for_visual(use_visual)
    };
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
            // Even InputOnly windows must report a real visual ID; clients
            // (GDK/Xlib) feed it back through XVisualIDFromVisual.
            visual: use_visual,
            depth: use_depth,
            class,
            mapped: false,
            event_mask,
            do_not_propagate_mask,
            background_pixel: if is_input_only { 0 } else { background_pixel },
            background_pixmap: if is_input_only {
                None
            } else {
                background_pixmap
            },
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
            backing_store: if is_input_only {
                u32::from(BackingStore::NOT_USEFUL) as u8
            } else {
                backing_store
            },
            backing_planes: if is_input_only {
                0xFFFFFFFF
            } else {
                backing_planes
            },
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
            wm_hints_input: None,
            wm_hints_window_group: None,
            modal: false,
            saved_geometry: None,
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
    // Set _NET_WM_PID if we know the client's process ID (EWMH §5.6)
    let atom_pid = state.intern_atom("_NET_WM_PID", false);
    // Set WM_CLIENT_MACHINE (ICCCM §4.1.2.9)
    let atom_machine = state.intern_atom("WM_CLIENT_MACHINE", false);
    let client_pid = state.peer_pid;
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties.insert(
            atom_frame,
            PropertyValue {
                prop_type: crate::xserver::atoms::predef::CARDINAL,
                format: 32,
                data: vec![0; 16], // left, right, top, bottom = 0
            },
        );
        // _NET_WM_PID
        if client_pid > 0 {
            win.properties.insert(
                atom_pid,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: client_pid.to_le_bytes().to_vec(),
                },
            );
        }
        // WM_CLIENT_MACHINE: hostname
        let hostname =
            std::fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "localhost".to_string());
        let hostname = hostname.trim();
        win.properties.insert(
            atom_machine,
            PropertyValue {
                prop_type: crate::xserver::atoms::predef::STRING,
                format: 8,
                data: hostname.as_bytes().to_vec(),
            },
        );
    }

    let is_top_level = parent == state.root_window
        && class == u16::from(WindowClass::INPUT_OUTPUT)
        && !override_redirect;
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
            resizable: true,
        },
    ));

    // Send CreateNotify (SubstructureNotifyMask on parent). Build the event
    // unconditionally so we can broadcast across connections — the local-mask
    // check only gates whether THIS client also wants the event in its own
    // queue. Without the unconditional broadcast, a window manager / observer
    // sitting on a different connection never sees CreateNotify, only the
    // subsequent MapNotify, breaking ICCCM substructure tracking.
    let event = serialize_event(
        &CreateNotifyEvent {
            response_type: CREATE_NOTIFY_EVENT,
            sequence: _seq,
            parent,
            window: wid,
            x,
            y,
            width,
            height,
            border_width,
            override_redirect,
        },
        state.msb_first,
    );

    // Local delivery: only if this client itself selected SubstructureNotify
    // on the parent. Cross-connection broadcast already filters by
    // source_client_id so we don't double-deliver to ourselves.
    state.deliver_event(parent, EventMask::SUBSTRUCTURE_NOTIFY, &event);

    Vec::new() // No reply for CreateWindow
}

// ---------------------------------------------------------------------------
// Opcode 4: DestroyWindow
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_window(
    state: &mut ClientState,
    req: &DestroyWindowRequest,
) -> Vec<u8> {
    let wid = req.window;

    if !state.windows.contains_key(&wid) {
        return build_error(WINDOW_ERROR, state.sequence, wid, 4, 0);
    }

    // Check if window had struts (for workarea recalculation after removal)
    let had_strut = state.windows.get(&wid).is_some_and(|w| w.strut.is_some());

    // Generate DestroyNotify events before removing the window
    let parent_id = state.windows.get(&wid).map(|w| w.parent);
    if let Some(parent_id) = parent_id {
        // Send DestroyNotify to the window itself (StructureNotifyMask)
        {
            let event = serialize_event(
                &DestroyNotifyEvent {
                    response_type: DESTROY_NOTIFY_EVENT,
                    sequence: state.sequence,
                    event: wid,
                    window: wid,
                },
                state.msb_first,
            );

            state.deliver_event(wid, EventMask::STRUCTURE_NOTIFY, &event);
        }

        // Send DestroyNotify to parent (SubstructureNotifyMask)
        {
            let event = serialize_event(
                &DestroyNotifyEvent {
                    response_type: DESTROY_NOTIFY_EVENT,
                    sequence: state.sequence,
                    event: parent_id,
                    window: wid,
                },
                state.msb_first,
            );
            state.deliver_event(parent_id, EventMask::SUBSTRUCTURE_NOTIFY, &event);
        }
    }

    // Revert focus if this was the focus window
    state.revert_focus_from(wid);

    // Per X11 spec §12.5.2: DestroyWindow destroys the window AND all of its
    // subwindows recursively. Collect all descendants (depth-first).
    let mut all_descendants = Vec::new();
    {
        let mut stack: Vec<u32> = state
            .windows
            .get(&wid)
            .map(|w| w.children_order.clone())
            .unwrap_or_default();
        while let Some(child) = stack.pop() {
            all_descendants.push(child);
            if let Some(w) = state.windows.get(&child) {
                stack.extend(w.children_order.iter().copied());
            }
        }
    }

    // Revert focus if it points to any descendant being destroyed
    for &desc in &all_descendants {
        if state.focus_window == desc {
            state.revert_focus_from(desc);
        }
    }

    // Destroy all descendants first (bottom-up in hierarchy)
    for desc in all_descendants.iter().rev() {
        let desc = *desc;
        // Send DestroyNotify for each descendant
        if let Some(desc_parent) = state.windows.get(&desc).map(|w| w.parent) {
            let event = serialize_event(
                &DestroyNotifyEvent {
                    response_type: DESTROY_NOTIFY_EVENT,
                    sequence: state.sequence,
                    event: desc,
                    window: desc,
                },
                state.msb_first,
            );
            state.deliver_event(desc, EventMask::STRUCTURE_NOTIFY, &event);

            // SubstructureNotify on the parent
            let pevent = serialize_event(
                &DestroyNotifyEvent {
                    response_type: DESTROY_NOTIFY_EVENT,
                    sequence: state.sequence,
                    event: desc_parent,
                    window: desc,
                },
                state.msb_first,
            );
            state.broadcast_event(desc_parent, EventMask::SUBSTRUCTURE_NOTIFY, &pevent);
        }

        state.windows.remove(&desc);
        state.recycle_xid(desc);
        state.gtk_menu_paths.remove(&desc);
        state.menu_tracker.window_index().unregister(desc);
        if let Ok(mut shared) = state.shared_windows.lock() {
            shared.remove(&desc);
        }
        if let Some(uuid) = state.x11_to_uuid.remove(&desc) {
            state
                .window_router
                .unregister_all(std::slice::from_ref(&uuid));
            state.menu_tracker.detach(&uuid);
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowDestroyed { window_id: uuid },
            ));
        }
    }

    // Remove from parent's children_order
    if let Some(parent_id) = parent_id {
        if let Some(parent_win) = state.windows.get_mut(&parent_id) {
            parent_win.children_order.retain(|&c| c != wid);
        }
    }

    state.windows.remove(&wid);
    state.recycle_xid(wid);
    state.gtk_menu_paths.remove(&wid);
    state.menu_tracker.window_index().unregister(wid);

    // Remove from shared window registry so other connections see the destroy immediately
    if let Ok(mut shared) = state.shared_windows.lock() {
        shared.remove(&wid);
    }

    if let Some(uuid) = state.x11_to_uuid.remove(&wid) {
        state
            .window_router
            .unregister_all(std::slice::from_ref(&uuid));
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

pub(crate) fn handle_destroy_subwindows(
    state: &mut ClientState,
    req: &DestroySubwindowsRequest,
) -> Vec<u8> {
    let parent = req.window;

    if !state.windows.contains_key(&parent) {
        return build_error(WINDOW_ERROR, state.sequence, parent, 5, 0);
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
            state
                .window_router
                .unregister_all(std::slice::from_ref(&uuid));
            state.menu_tracker.detach(&uuid);
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowDestroyed { window_id: uuid },
            ));
        }
    }

    Vec::new()
}
