//! SendEvent handler — opcode 25.

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 25: SendEvent
// ---------------------------------------------------------------------------

pub(crate) fn handle_send_event(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 44, state.sequence, 25);

    let propagate = data[1] != 0;
    let destination = state.read_u32(data, 4);
    let event_mask = state.read_u32(data, 8);

    // The event data is 32 bytes starting at offset 12
    let mut event = data[12..44].to_vec();
    // Mark as synthetic (bit 7 of the event code)
    event[0] |= 0x80;

    // Resolve destination:
    // 0 = PointerWindow: deliver to the window containing the pointer
    // 1 = InputFocus: deliver to the focus window
    // Otherwise: deliver to the specified window
    let target = match destination {
        0 => {
            // Find the window containing the pointer.
            let px = state.pointer_x;
            let py = state.pointer_y;
            let mut found = state.root_window;
            for (_, win) in state.windows.iter() {
                if win.mapped
                    && win.parent == state.root_window
                    && px >= win.x
                    && py >= win.y
                    && px < win.x + win.width as i16
                    && py < win.y + win.height as i16
                {
                    found = win.id;
                    break;
                }
            }
            found
        }
        1 => state.focus_window,
        w => w,
    };

    let event_type = event[0] & 0x7F; // strip synthetic bit for logging
    debug!(
        "SendEvent: type={} dest={:#x} target={:#x}",
        event_type, destination, target
    );

    // Intercept XIM (X Input Method) client messages directed at the XIM window.
    if event_type == CLIENT_MESSAGE_EVENT && event.len() >= 32
        && super::super::xim::maybe_handle_xim_message(state, &event) {
            return Vec::new();
        }

    // Intercept EWMH _NET_WM_STATE ClientMessage sent to root window.
    // Per EWMH spec: clients send ClientMessage to root to request state changes.
    if event_type == CLIENT_MESSAGE_EVENT && target == state.root_window && event.len() >= 32 {
        let format = event[1] & 0x7F;
        if format == 32 {
            let msg_type = state.read_u32(&event, 8);
            let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
            if msg_type == net_wm_state_atom {
                // _NET_WM_STATE ClientMessage:
                // data[0] = action (0=remove, 1=add, 2=toggle)
                // data[1] = first property atom
                // data[2] = second property atom (or 0)
                // data[3] = source indication
                let action = state.read_u32(&event, 12);
                let prop1 = state.read_u32(&event, 16);
                let prop2 = state.read_u32(&event, 20);
                let source_window = state.read_u32(&event, 4);

                let fullscreen_atom = state.intern_atom("_NET_WM_STATE_FULLSCREEN", false);
                let max_vert_atom = state.intern_atom("_NET_WM_STATE_MAXIMIZED_VERT", false);
                let max_horz_atom = state.intern_atom("_NET_WM_STATE_MAXIMIZED_HORZ", false);
                let hidden_atom = state.intern_atom("_NET_WM_STATE_HIDDEN", false);

                let atoms_to_change = if prop2 != 0 { vec![prop1, prop2] } else { vec![prop1] };

                // Get current state atoms (stored as LE)
                let mut current_atoms: Vec<u32> = state.windows.get(&source_window)
                    .and_then(|w| w.properties.get(&net_wm_state_atom))
                    .map(|pv| {
                        pv.data.chunks_exact(4)
                            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect()
                    })
                    .unwrap_or_default();

                for atom in &atoms_to_change {
                    match action {
                        0 => { current_atoms.retain(|a| a != atom); } // Remove
                        1 => { if !current_atoms.contains(atom) { current_atoms.push(*atom); } } // Add
                        2 => { // Toggle
                            if current_atoms.contains(atom) {
                                current_atoms.retain(|a| a != atom);
                            } else {
                                current_atoms.push(*atom);
                            }
                        }
                        _ => {}
                    }
                }

                // Update the property (stored as LE)
                let data_bytes: Vec<u8> = current_atoms.iter().flat_map(|a| a.to_le_bytes()).collect();
                if let Some(win) = state.windows.get_mut(&source_window) {
                    win.properties.insert(net_wm_state_atom, PropertyValue {
                        prop_type: 4,
                        format: 32,
                        data: data_bytes,
                    });
                }

                // Determine the new WM state and broadcast to frontend
                let new_state = if current_atoms.contains(&fullscreen_atom) {
                    x11_web_protocol::WindowWmState::Fullscreen
                } else if current_atoms.contains(&max_vert_atom) || current_atoms.contains(&max_horz_atom) {
                    x11_web_protocol::WindowWmState::Maximized
                } else if current_atoms.contains(&hidden_atom) {
                    x11_web_protocol::WindowWmState::Minimized
                } else {
                    x11_web_protocol::WindowWmState::Normal
                };

                if let Some(uuid) = state.window_uuid(source_window) {
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: uuid,
                            state: new_state,
                        },
                    ));
                }

                return Vec::new();
            }

            // WM_PROTOCOLS response: handle _NET_WM_PING pong
            let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
            if msg_type == wm_protocols_atom {
                let protocol_atom = state.read_u32(&event, 12);
                let net_wm_ping_atom = state.intern_atom("_NET_WM_PING", false);
                if protocol_atom == net_wm_ping_atom {
                    // _NET_WM_PING pong: client is alive. Record the response timestamp.
                    let source_window = state.read_u32(&event, 4);
                    debug!("_NET_WM_PING pong received from window {source_window:#x}");
                    return Vec::new();
                }
            }

            // _NET_CLOSE_WINDOW: graceful close request
            let net_close_atom = state.intern_atom("_NET_CLOSE_WINDOW", false);
            if msg_type == net_close_atom {
                let source_window = state.read_u32(&event, 4);
                let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
                let wm_delete_atom = state.intern_atom("WM_DELETE_WINDOW", false);
                if state.window_supports_protocol(source_window, wm_delete_atom) {
                    let bo = state.msb_first;
                    let mut cm = [0u8; 32];
                    cm[0] = CLIENT_MESSAGE_EVENT;
                    cm[1] = 32;
                    state.write_u16(&mut cm, 2, state.sequence);
                    state.write_u32(&mut cm, 4, source_window);
                    state.write_u32(&mut cm, 8, wm_protocols_atom);
                    state.write_u32(&mut cm, 12, wm_delete_atom);
                    state.write_u32(&mut cm, 16, state.timestamp());
                    let _ = bo; // byte order already used by write_u32
                    state.pending_events.push(cm.to_vec());
                }
                return Vec::new();
            }

            // _NET_REQUEST_FRAME_EXTENTS: respond with frame extents
            let net_request_frame_atom = state.intern_atom("_NET_REQUEST_FRAME_EXTENTS", false);
            if msg_type == net_request_frame_atom {
                let source_window = state.read_u32(&event, 4);
                let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
                if let Some(win) = state.windows.get_mut(&source_window) {
                    // Set frame extents to 0 (server-side decoration handled by frontend)
                    win.properties.insert(atom_frame, PropertyValue {
                        prop_type: 6, // CARDINAL
                        format: 32,
                        data: vec![0; 16],
                    });
                }
                return Vec::new();
            }

            // _NET_ACTIVE_WINDOW: focus request from another app/pager
            let net_active_atom = state.intern_atom("_NET_ACTIVE_WINDOW", false);
            if msg_type == net_active_atom {
                let source_window = state.read_u32(&event, 4);
                state.set_focus_window(source_window);
                // Send WM_TAKE_FOCUS if the target supports it
                state.send_wm_take_focus(source_window);
                return Vec::new();
            }

            // _NET_WM_MOVERESIZE: interactive move/resize initiated by client
            let net_wm_moveresize_atom = state.intern_atom("_NET_WM_MOVERESIZE", false);
            if msg_type == net_wm_moveresize_atom {
                let source_window = state.read_u32(&event, 4);
                let _x_root = state.read_u32(&event, 12);
                let _y_root = state.read_u32(&event, 16);
                let direction = state.read_u32(&event, 20);
                debug!("_NET_WM_MOVERESIZE: window={source_window:#x} direction={direction}");
                // Direction 11 = _NET_WM_MOVERESIZE_CANCEL
                // Other values initiate move/resize which the frontend handles
                if direction != 11 {
                    if let Some(uuid) = state.window_uuid(source_window) {
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            DisplayUpdate::WindowStateChanged {
                                window_id: uuid,
                                state: x11_web_protocol::WindowWmState::Normal,
                            },
                        ));
                    }
                }
                return Vec::new();
            }

            // _NET_MOVERESIZE_WINDOW: programmatic move/resize from pager/client
            let net_moveresize_window_atom = state.intern_atom("_NET_MOVERESIZE_WINDOW", false);
            if msg_type == net_moveresize_window_atom {
                let source_window = state.read_u32(&event, 4);
                let flags = state.read_u32(&event, 12);
                let x = state.read_u32(&event, 16) as i16;
                let y = state.read_u32(&event, 20) as i16;
                let width = state.read_u32(&event, 24) as u16;
                let height = state.read_u32(&event, 28) as u16;
                debug!("_NET_MOVERESIZE_WINDOW: window={source_window:#x} flags={flags:#x} {x},{y} {width}x{height}");
                if let Some(win) = state.windows.get_mut(&source_window) {
                    // flags bits 8-11 indicate which fields are present
                    if flags & (1 << 8) != 0 { win.x = x; }
                    if flags & (1 << 9) != 0 { win.y = y; }
                    if flags & (1 << 10) != 0 { win.width = width; }
                    if flags & (1 << 11) != 0 { win.height = height; }
                }
                if let Some(uuid) = state.window_uuid(source_window) {
                    let bw = state.windows.get(&source_window).map(|w| w.border_width).unwrap_or(0);
                    let bp = state.windows.get(&source_window).map(|w| w.border_pixel).unwrap_or(0);
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowConfigured {
                            window_id: uuid,
                            x,
                            y,
                            width,
                            height,
                            border_width: bw,
                            border_pixel: bp,
                        },
                    ));
                }
                return Vec::new();
            }

            // _NET_RESTACK_WINDOW: restack request from pager
            let net_restack_atom = state.intern_atom("_NET_RESTACK_WINDOW", false);
            if msg_type == net_restack_atom {
                let source_window = state.read_u32(&event, 4);
                // Raise the window
                if let Some(uuid) = state.window_uuid(source_window) {
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowRaised {
                            window_id: uuid,
                        },
                    ));
                }
                return Vec::new();
            }

            // WM_CHANGE_STATE: ICCCM iconic state request
            let wm_change_state_atom = state.intern_atom("WM_CHANGE_STATE", false);
            if msg_type == wm_change_state_atom {
                let source_window = state.read_u32(&event, 4);
                let desired_state = state.read_u32(&event, 12);
                debug!("WM_CHANGE_STATE: window={source_window:#x} state={desired_state}");
                if desired_state == 3 {
                    // IconicState: minimize the window
                    let hidden_atom = state.intern_atom("_NET_WM_STATE_HIDDEN", false);
                    let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
                    let data = hidden_atom.to_le_bytes().to_vec();
                    if let Some(win) = state.windows.get_mut(&source_window) {
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4,
                            format: 32,
                            data,
                        });
                    }
                    if let Some(uuid) = state.window_uuid(source_window) {
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            DisplayUpdate::WindowStateChanged {
                                window_id: uuid,
                                state: x11_web_protocol::WindowWmState::Minimized,
                            },
                        ));
                    }
                }
                return Vec::new();
            }

            // _NET_SYSTEM_TRAY_OPCODE: system tray protocol messages
            // Opcode values: 0 = SYSTEM_TRAY_REQUEST_DOCK
            //                1 = SYSTEM_TRAY_BEGIN_MESSAGE
            //                2 = SYSTEM_TRAY_CANCEL_MESSAGE
            let tray_opcode_atom = state.intern_atom("_NET_SYSTEM_TRAY_OPCODE", false);
            if msg_type == tray_opcode_atom {
                let opcode = state.read_u32(&event, 12);
                if opcode == 0 {
                    // SYSTEM_TRAY_REQUEST_DOCK: reparent the icon window into our tray
                    let icon_window = state.read_u32(&event, 16);
                    debug!("SYSTEM_TRAY_REQUEST_DOCK: icon_window={icon_window:#x}");

                    // Map the icon window if not already mapped
                    if let Some(win) = state.windows.get_mut(&icon_window) {
                        if !win.mapped {
                            win.mapped = true;
                        }
                    }

                    // Send XEMBED_EMBEDDED_NOTIFY to the icon window
                    // XEmbed message format: ClientMessage with _XEMBED type
                    // data[0] = timestamp, data[1] = XEMBED_EMBEDDED_NOTIFY (0),
                    // data[2] = embedder window
                    let xembed_atom = state.intern_atom("_XEMBED", false);
                    let timestamp = state.timestamp();
                    let mut xembed_event = [0u8; 32];
                    xembed_event[0] = CLIENT_MESSAGE_EVENT;
                    xembed_event[1] = 32; // format
                    state.write_u32(&mut xembed_event, 4, icon_window);
                    state.write_u32(&mut xembed_event, 8, xembed_atom);
                    state.write_u32(&mut xembed_event, 12, timestamp);
                    state.write_u32(&mut xembed_event, 16, 0); // XEMBED_EMBEDDED_NOTIFY
                    state.write_u32(&mut xembed_event, 20, crate::xserver::types::SYSTEM_TRAY_WINDOW);

                    if !state.event_router.send_event(icon_window, xembed_event.to_vec()) {
                        state.pending_events.push(xembed_event.to_vec());
                    }
                }
                return Vec::new();
            }
        }
    }

    // Resolve the actual delivery target, respecting propagate and event_mask
    // per the X11 protocol specification §10.6.
    let delivery_target = if propagate {
        // Propagate mode: walk up the window tree from `target` until we find
        // a window where event_mask intersects the window's selected event mask
        // (including masks from other clients via EventBroadcaster),
        // or until we hit root, or until do_not_propagate_mask blocks the event.
        let event_mask_bit = event_type_to_mask(event_type);
        let mut current = target;
        let mut found = None;
        for _ in 0..128 {
            let win = match state.windows.get(&current) {
                Some(w) => w,
                None => break,
            };
            // If the window's do_not_propagate_mask blocks this event type,
            // stop propagation — no delivery.
            if event_mask_bit != 0 && (win.do_not_propagate_mask & event_mask_bit) != 0 {
                debug!(
                    "SendEvent propagate: blocked by do_not_propagate_mask on {:#x}",
                    current
                );
                break;
            }
            // Check both local and remote clients' masks on this window.
            let combined_mask = win.event_mask | state.event_broadcaster.all_event_masks(current);
            if event_mask != 0 && (combined_mask & event_mask) != 0 {
                found = Some(current);
                break;
            }
            // If event_mask is 0 and propagate is true, the spec says
            // propagate until a client selects ANY event (unusual, but handle it).
            if event_mask == 0 && combined_mask != 0 {
                found = Some(current);
                break;
            }
            // Reached the root — stop.
            if current == state.root_window {
                break;
            }
            // Move to parent.
            current = win.parent;
        }
        match found {
            Some(w) => w,
            None => {
                debug!(
                    "SendEvent propagate: no window found for event type {}, discarding",
                    event_type
                );
                return Vec::new();
            }
        }
    } else {
        // Non-propagate mode:
        // If event_mask is 0, deliver unconditionally to target.
        // If event_mask is non-zero, deliver only if the target window has
        // selected at least one of the matching event types.
        if event_mask != 0 {
            if let Some(win) = state.windows.get(&target) {
                if (win.event_mask & event_mask) == 0 {
                    debug!(
                        "SendEvent: target {:#x} event_mask {:#x} does not match requested {:#x}, discarding",
                        target, win.event_mask, event_mask
                    );
                    return Vec::new();
                }
            }
            // If window not found in state, deliver anyway (may be root or special window).
        }
        target
    };

    // Cache clipboard data for persistence: when a CLIPBOARD owner sends a
    // SelectionNotify with a non-None property, the data on the requestor's
    // window property is the clipboard content. Save it so the server can serve
    // it after the owning client disconnects.
    const CLIPBOARD_ATOM: u32 = 134;
    if event_type == SELECTION_NOTIFY_EVENT && event.len() >= 24 {
        let sel_atom = state.read_u32(&event, 12);
        let target_atom = state.read_u32(&event, 16);
        let property_atom = state.read_u32(&event, 20);
        if sel_atom == CLIPBOARD_ATOM && property_atom != 0 {
            let requestor_wid = state.read_u32(&event, 8);
            // Try local windows first, then fall back to shared windows
            // (for cross-connection transfers where the requestor is on
            // a different connection).
            let prop_data = state.windows.get(&requestor_wid)
                .and_then(|w| w.properties.get(&property_atom))
                .map(|pv| pv.data.clone())
                .or_else(|| {
                    state.shared_windows.lock().ok()
                        .and_then(|sw| sw.get(&requestor_wid)
                            .and_then(|w| w.properties.get(&property_atom))
                            .map(|pv| pv.data.clone()))
                });
            if let Some(data) = prop_data {
                if let Ok(mut pc) = state.persistent_clipboard.lock() {
                    let entry = pc.entry(CLIPBOARD_ATOM)
                        .or_insert_with(|| PersistentClipboardEntry {
                            targets: HashMap::new(),
                            timestamp: state.timestamp(),
                        });
                    entry.targets.insert(target_atom, data);
                    entry.timestamp = state.timestamp();
                }
            }
        }
    }

    // Check if the delivery target window belongs to this connection or another.
    // For cross-connection event delivery (required by XDND, ICCCM selections,
    // ClientMessage), try the EventRouter first.
    let is_local = state.x11_to_uuid.contains_key(&delivery_target)
        || delivery_target == state.root_window;

    if is_local {
        // Target window is owned by this connection — deliver locally.
        state.pending_events.push(event);
    } else {
        // Try cross-connection delivery via the EventRouter.
        // This handles XDND (XdndEnter, XdndPosition, XdndStatus, XdndDrop,
        // XdndFinished, XdndLeave), ICCCM SelectionNotify, and any other
        // ClientMessage sent to windows on other connections.
        if !state.event_router.send_event(delivery_target, event.clone()) {
            // No route found — the target window may be a child window
            // of a top-level we know about, or it may be on this connection
            // but not registered (e.g., sub-windows). Fall back to local delivery.
            debug!(
                "SendEvent: no cross-connection route for target {:#x}, delivering locally",
                delivery_target
            );
            state.pending_events.push(event);
        }
    }

    Vec::new()
}
