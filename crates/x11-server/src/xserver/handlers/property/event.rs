//! SendEvent handler — opcode 25.

use super::*;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{
    ClientMessageEvent, ConfigureNotifyEvent, ExposeEvent, PropertyNotifyEvent, SendEventRequest,
};

// XEmbed protocol message types (per the freedesktop.org XEmbed 0.5 spec).
// Sent in `data.l[1]` of a ClientMessage with type `_XEMBED`.
const XEMBED_EMBEDDED_NOTIFY: u32 = 0;
const XEMBED_WINDOW_ACTIVATE: u32 = 1;
const XEMBED_WINDOW_DEACTIVATE: u32 = 2;
const XEMBED_REQUEST_FOCUS: u32 = 3;
const XEMBED_FOCUS_IN: u32 = 4;
const XEMBED_FOCUS_OUT: u32 = 5;
const XEMBED_FOCUS_NEXT: u32 = 6;
const XEMBED_FOCUS_PREV: u32 = 7;
const XEMBED_MODALITY_ON: u32 = 10;
const XEMBED_MODALITY_OFF: u32 = 11;
const XEMBED_REGISTER_ACCELERATOR: u32 = 12;
const XEMBED_UNREGISTER_ACCELERATOR: u32 = 13;
const XEMBED_ACTIVATE_ACCELERATOR: u32 = 14;

// ---------------------------------------------------------------------------
// Opcode 25: SendEvent
// ---------------------------------------------------------------------------

pub(crate) fn handle_send_event(state: &mut ClientState, req: &SendEventRequest) -> Vec<u8> {
    let propagate = req.propagate;
    let destination = req.destination;
    let event_mask = u32::from(req.event_mask);

    // The event data is 32 bytes parsed by x11rb
    let mut event: Vec<u8> = req.event.to_vec();
    // Mark as synthetic
    event[0] |= crate::xserver::core::SEND_EVENT_FLAG;

    // Resolve destination:
    // 0 = PointerWindow: deliver to the window containing the pointer
    // 1 = InputFocus: deliver to the focus window
    // Otherwise: deliver to the specified window
    let target = match destination {
        0 => {
            // Find the deepest mapped window containing the pointer.
            // Walk the window tree from root, descending into children
            // (in reverse stacking order = top window first) to find the
            // most specific window under the pointer.
            let px = state.pointer_x;
            let py = state.pointer_y;
            let mut found = state.root_window;
            let mut current = state.root_window;
            'outer: loop {
                let children = state
                    .windows
                    .get(&current)
                    .map(|w| w.children_order.clone())
                    .unwrap_or_default();
                let mut descended = false;
                // Iterate in reverse stacking order (top of stack = last in list)
                for &child_id in children.iter().rev() {
                    if let Some(child) = state.windows.get(&child_id) {
                        if child.mapped
                            && px >= child.x
                            && py >= child.y
                            && px < child.x.saturating_add(child.width as i16)
                            && py < child.y.saturating_add(child.height as i16)
                        {
                            found = child_id;
                            current = child_id;
                            descended = true;
                            break;
                        }
                    }
                }
                if !descended {
                    break 'outer;
                }
            }
            found
        }
        1 => state.focus_window,
        w => w,
    };

    let event_type = event[0] & 0x7F; // strip synthetic bit for logging

    // Per X11 spec: event type must be >= 2 (types 0-1 are errors/replies, not events)
    if event_type < 2 {
        return build_error(VALUE_ERROR, state.sequence, event_type as u32, 25, 0);
    }

    debug!(
        "SendEvent: type={} dest={:#x} target={:#x}",
        event_type, destination, target
    );

    // Intercept XIM (X Input Method) client messages directed at the XIM window.
    if event_type == CLIENT_MESSAGE_EVENT
        && event.len() >= 32
        && super::super::xim::maybe_handle_xim_message(state, &event)
    {
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
                let modal_atom = state.intern_atom("_NET_WM_STATE_MODAL", false);
                let above_atom = state.intern_atom("_NET_WM_STATE_ABOVE", false);
                let _demands_attention_atom =
                    state.intern_atom("_NET_WM_STATE_DEMANDS_ATTENTION", false);

                let atoms_to_change = if prop2 != 0 {
                    vec![prop1, prop2]
                } else {
                    vec![prop1]
                };

                // Get current state atoms (stored as LE)
                let mut current_atoms: Vec<u32> = state
                    .windows
                    .get(&source_window)
                    .and_then(|w| w.properties.get(&net_wm_state_atom))
                    .map(|pv| {
                        pv.data
                            .chunks_exact(4)
                            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect()
                    })
                    .unwrap_or_default();

                for atom in &atoms_to_change {
                    match action {
                        0 => {
                            current_atoms.retain(|a| a != atom);
                        } // Remove
                        1 => {
                            if !current_atoms.contains(atom) {
                                current_atoms.push(*atom);
                            }
                        } // Add
                        2 => {
                            // Toggle
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
                let data_bytes: Vec<u8> =
                    current_atoms.iter().flat_map(|a| a.to_le_bytes()).collect();
                if let Some(win) = state.windows.get_mut(&source_window) {
                    win.properties.insert(
                        net_wm_state_atom,
                        PropertyValue {
                            prop_type: crate::xserver::atoms::predef::ATOM,
                            format: 32,
                            data: data_bytes,
                        },
                    );
                }

                // Generate PropertyNotify for clients watching PropertyChangeMask
                {
                    let pn_event = serialize_event(
                        &PropertyNotifyEvent {
                            response_type: PROPERTY_NOTIFY_EVENT,
                            sequence: state.sequence,
                            window: source_window,
                            atom: net_wm_state_atom,
                            time: state.timestamp(),
                            state: 0u8.into(), // NewValue
                        },
                        state.msb_first,
                    );
                    state.deliver_event(source_window, EventMask::PROPERTY_CHANGE, &pn_event);
                }

                // Determine the new WM state and broadcast to frontend
                let is_fullscreen = current_atoms.contains(&fullscreen_atom);
                let is_maximized = current_atoms.contains(&max_vert_atom)
                    || current_atoms.contains(&max_horz_atom);

                let new_state = if is_fullscreen {
                    x11_web_protocol::WindowWmState::Fullscreen
                } else if is_maximized {
                    x11_web_protocol::WindowWmState::Maximized
                } else if current_atoms.contains(&hidden_atom) {
                    x11_web_protocol::WindowWmState::Minimized
                } else {
                    x11_web_protocol::WindowWmState::Normal
                };

                // Per EWMH spec: when the window manager changes fullscreen or
                // maximized state it MUST resize the window accordingly and send
                // a synthetic ConfigureNotify so the client knows its new size.
                let needs_resize = is_fullscreen || is_maximized;
                let had_saved = state
                    .windows
                    .get(&source_window)
                    .is_some_and(|w| w.saved_geometry.is_some());

                if needs_resize && !had_saved {
                    // Save current geometry before resizing
                    if let Some(win) = state.windows.get(&source_window) {
                        let saved = (win.x, win.y, win.width, win.height);
                        if let Some(win) = state.windows.get_mut(&source_window) {
                            win.saved_geometry = Some(saved);
                        }
                    }
                    // Resize to fill screen
                    let sw = state.screen_width;
                    let sh = state.screen_height;
                    if let Some(win) = state.windows.get_mut(&source_window) {
                        win.x = 0;
                        win.y = 0;
                        win.width = sw;
                        win.height = sh;
                        win.framebuffer.resize_with_gravity(sw as u32, sh as u32, 0);
                    }
                    // Send ConfigureNotify to the client
                    {
                        let override_redirect = state
                            .windows
                            .get(&source_window)
                            .is_some_and(|w| w.override_redirect);
                        let border_width = state
                            .windows
                            .get(&source_window)
                            .map(|w| w.border_width)
                            .unwrap_or(0);
                        let cn = serialize_event(
                            &ConfigureNotifyEvent {
                                response_type: CONFIGURE_NOTIFY_EVENT,
                                sequence: state.sequence,
                                event: source_window,
                                window: source_window,
                                above_sibling: 0,
                                x: 0,
                                y: 0,
                                width: sw,
                                height: sh,
                                border_width,
                                override_redirect,
                            },
                            state.msb_first,
                        );
                        state.deliver_event(source_window, EventMask::STRUCTURE_NOTIFY, &cn);

                        // Send Expose so the client redraws at the new size
                        if state.window_selects(source_window, EventMask::EXPOSURE) {
                            let expose = serialize_event(
                                &ExposeEvent {
                                    response_type: EXPOSE_EVENT,
                                    sequence: state.sequence,
                                    window: source_window,
                                    x: 0,
                                    y: 0,
                                    width: sw,
                                    height: sh,
                                    count: 0,
                                },
                                state.msb_first,
                            );
                            state.pending_events.push(expose);
                        }
                    }
                    // Notify frontend of new geometry
                    if let Some(uuid) = state.window_uuid(source_window) {
                        let bw = state
                            .windows
                            .get(&source_window)
                            .map(|w| w.border_width)
                            .unwrap_or(0);
                        let bp = state
                            .windows
                            .get(&source_window)
                            .map(|w| w.border_pixel)
                            .unwrap_or(0);
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            DisplayUpdate::WindowConfigured {
                                window_id: uuid,
                                x: 0,
                                y: 0,
                                width: sw,
                                height: sh,
                                border_width: bw,
                                border_pixel: bp,
                                resizable: true,
                            },
                        ));
                    }
                } else if !needs_resize && had_saved {
                    // Restore saved geometry when leaving fullscreen/maximized
                    let saved = state
                        .windows
                        .get(&source_window)
                        .and_then(|w| w.saved_geometry);
                    if let Some((sx, sy, sw, sh)) = saved {
                        if let Some(win) = state.windows.get_mut(&source_window) {
                            win.x = sx;
                            win.y = sy;
                            win.width = sw;
                            win.height = sh;
                            win.saved_geometry = None;
                            win.framebuffer.resize_with_gravity(sw as u32, sh as u32, 0);
                        }
                        // Send ConfigureNotify to the client
                        {
                            let override_redirect = state
                                .windows
                                .get(&source_window)
                                .is_some_and(|w| w.override_redirect);
                            let border_width = state
                                .windows
                                .get(&source_window)
                                .map(|w| w.border_width)
                                .unwrap_or(0);
                            let cn = serialize_event(
                                &ConfigureNotifyEvent {
                                    response_type: CONFIGURE_NOTIFY_EVENT,
                                    sequence: state.sequence,
                                    event: source_window,
                                    window: source_window,
                                    above_sibling: 0,
                                    x: sx,
                                    y: sy,
                                    width: sw,
                                    height: sh,
                                    border_width,
                                    override_redirect,
                                },
                                state.msb_first,
                            );
                            state.deliver_event(source_window, EventMask::STRUCTURE_NOTIFY, &cn);

                            // Expose for redraw
                            if state.window_selects(source_window, EventMask::EXPOSURE) {
                                let expose = serialize_event(
                                    &ExposeEvent {
                                        response_type: EXPOSE_EVENT,
                                        sequence: state.sequence,
                                        window: source_window,
                                        x: 0,
                                        y: 0,
                                        width: sw,
                                        height: sh,
                                        count: 0,
                                    },
                                    state.msb_first,
                                );
                                state.pending_events.push(expose);
                            }
                        }
                        // Notify frontend
                        if let Some(uuid) = state.window_uuid(source_window) {
                            let bw = state
                                .windows
                                .get(&source_window)
                                .map(|w| w.border_width)
                                .unwrap_or(0);
                            let bp = state
                                .windows
                                .get(&source_window)
                                .map(|w| w.border_pixel)
                                .unwrap_or(0);
                            let _ = state.update_tx.send((
                                state.client_id.clone(),
                                DisplayUpdate::WindowConfigured {
                                    window_id: uuid,
                                    x: sx,
                                    y: sy,
                                    width: sw,
                                    height: sh,
                                    border_width: bw,
                                    border_pixel: bp,
                                    resizable: true,
                                },
                            ));
                        }
                    }
                }

                if let Some(uuid) = state.window_uuid(source_window) {
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: uuid.clone(),
                            state: new_state,
                        },
                    ));

                    // MODAL: raise window above its transient-for parent (EWMH §_NET_WM_STATE_MODAL)
                    // Also update the modal flag on WindowState for input blocking.
                    {
                        let is_modal = current_atoms.contains(&modal_atom);
                        if let Some(win) = state.windows.get_mut(&source_window) {
                            win.modal = is_modal;
                        }
                    }
                    if current_atoms.contains(&modal_atom) || current_atoms.contains(&above_atom) {
                        // Raise this window to the top of the stack
                        if let Some(children) = state
                            .windows
                            .get(&state.root_window)
                            .map(|r| r.children_order.clone())
                        {
                            if children.contains(&source_window) {
                                if let Some(root) = state.windows.get_mut(&state.root_window) {
                                    root.children_order.retain(|&w| w != source_window);
                                    root.children_order.push(source_window);
                                }
                            }
                        }
                    }
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
                    let cm = serialize_event(
                        &ClientMessageEvent {
                            response_type: CLIENT_MESSAGE_EVENT,
                            format: 32,
                            sequence: state.sequence,
                            window: source_window,
                            type_: wm_protocols_atom,
                            data: [wm_delete_atom, state.timestamp(), 0, 0, 0].into(),
                        },
                        state.msb_first,
                    );
                    state.pending_events.push(cm);
                }
                return Vec::new();
            }

            // _NET_REQUEST_FRAME_EXTENTS: respond with frame extents
            let net_request_frame_atom = state.intern_atom("_NET_REQUEST_FRAME_EXTENTS", false);
            if msg_type == net_request_frame_atom {
                let source_window = state.read_u32(&event, 4);
                let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
                // Compute frame extents from border_width. The frontend renders
                // decorations (title bar etc.) but the X11 border_width is part of
                // the protocol-visible frame.
                let bw = state
                    .windows
                    .get(&source_window)
                    .map(|w| w.border_width as u32)
                    .unwrap_or(0);
                if let Some(win) = state.windows.get_mut(&source_window) {
                    // _NET_FRAME_EXTENTS: left, right, top, bottom (CARD32 each, stored LE)
                    let mut data = Vec::with_capacity(16);
                    data.extend_from_slice(&bw.to_le_bytes()); // left
                    data.extend_from_slice(&bw.to_le_bytes()); // right
                    data.extend_from_slice(&bw.to_le_bytes()); // top
                    data.extend_from_slice(&bw.to_le_bytes()); // bottom
                    win.properties.insert(
                        atom_frame,
                        PropertyValue {
                            prop_type: crate::xserver::atoms::predef::CARDINAL,
                            format: 32,
                            data,
                        },
                    );
                }
                // Generate PropertyNotify for the frame extents change
                {
                    let pn_event = serialize_event(
                        &PropertyNotifyEvent {
                            response_type: PROPERTY_NOTIFY_EVENT,
                            sequence: state.sequence,
                            window: source_window,
                            atom: atom_frame,
                            time: state.timestamp(),
                            state: 0u8.into(), // NewValue
                        },
                        state.msb_first,
                    );
                    state.deliver_event(source_window, EventMask::PROPERTY_CHANGE, &pn_event);
                }
                return Vec::new();
            }

            // _NET_ACTIVE_WINDOW: focus request from another app/pager
            let net_active_atom = state.intern_atom("_NET_ACTIVE_WINDOW", false);
            if msg_type == net_active_atom {
                let source_window = state.read_u32(&event, 4);
                // Respect ICCCM input focus model (WM_HINTS input field)
                let accepts_input = state
                    .windows
                    .get(&source_window)
                    .and_then(|w| w.wm_hints_input)
                    .unwrap_or(true);
                if accepts_input {
                    state.set_focus_window(source_window);
                }
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
                    if flags & (1 << 8) != 0 {
                        win.x = x;
                    }
                    if flags & (1 << 9) != 0 {
                        win.y = y;
                    }
                    if flags & (1 << 10) != 0 {
                        win.width = width;
                    }
                    if flags & (1 << 11) != 0 {
                        win.height = height;
                    }
                }
                if let Some(uuid) = state.window_uuid(source_window) {
                    let bw = state
                        .windows
                        .get(&source_window)
                        .map(|w| w.border_width)
                        .unwrap_or(0);
                    let bp = state
                        .windows
                        .get(&source_window)
                        .map(|w| w.border_pixel)
                        .unwrap_or(0);
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
                            resizable: true,
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
                        DisplayUpdate::WindowRaised { window_id: uuid },
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
                        win.properties.insert(
                            net_wm_state_atom,
                            PropertyValue {
                                prop_type: crate::xserver::atoms::predef::ATOM,
                                format: 32,
                                data,
                            },
                        );
                    }
                    // Generate PropertyNotify for the state change
                    {
                        let pn = serialize_event(
                            &PropertyNotifyEvent {
                                response_type: PROPERTY_NOTIFY_EVENT,
                                sequence: state.sequence,
                                window: source_window,
                                atom: net_wm_state_atom,
                                time: state.timestamp(),
                                state: 0u8.into(), // NewValue
                            },
                            state.msb_first,
                        );
                        state.deliver_event(source_window, EventMask::PROPERTY_CHANGE, &pn);
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
                } else if desired_state == 1 {
                    // NormalState: restore/un-minimize the window
                    let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
                    let hidden_atom = state.intern_atom("_NET_WM_STATE_HIDDEN", false);
                    // Remove _NET_WM_STATE_HIDDEN from the state
                    if let Some(win) = state.windows.get_mut(&source_window) {
                        if let Some(prop) = win.properties.get_mut(&net_wm_state_atom) {
                            let atoms: Vec<u32> = prop
                                .data
                                .chunks_exact(4)
                                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                                .filter(|&a| a != hidden_atom)
                                .collect();
                            prop.data = atoms.iter().flat_map(|a| a.to_le_bytes()).collect();
                        }
                    }
                    // Generate PropertyNotify for the state change
                    {
                        let pn = serialize_event(
                            &PropertyNotifyEvent {
                                response_type: PROPERTY_NOTIFY_EVENT,
                                sequence: state.sequence,
                                window: source_window,
                                atom: net_wm_state_atom,
                                time: state.timestamp(),
                                state: 0u8.into(), // NewValue
                            },
                            state.msb_first,
                        );
                        state.deliver_event(source_window, EventMask::PROPERTY_CHANGE, &pn);
                    }
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

                    // Send XEMBED_EMBEDDED_NOTIFY to the icon window via _XEMBED ClientMessage.
                    let xembed_atom = state.intern_atom("_XEMBED", false);
                    let timestamp = state.timestamp();
                    let xembed_event = serialize_event(
                        &ClientMessageEvent {
                            response_type: CLIENT_MESSAGE_EVENT,
                            format: 32,
                            sequence: 0,
                            window: icon_window,
                            type_: xembed_atom,
                            data: [
                                timestamp,
                                0, /* XEMBED_EMBEDDED_NOTIFY */
                                crate::xserver::types::SYSTEM_TRAY_WINDOW,
                                0,
                                0,
                            ]
                            .into(),
                        },
                        state.msb_first,
                    );

                    if !state
                        .event_router
                        .send_event(icon_window, xembed_event.clone())
                    {
                        state.pending_events.push(xembed_event);
                    }
                }
                return Vec::new();
            }

            // _XEMBED: XEmbed protocol messages for embedded window management.
            // These control focus, activation, and lifecycle of embedded windows
            // (system tray icons, plugin windows, embedded terminals).
            let xembed_atom = state.intern_atom("_XEMBED", false);
            if msg_type == xembed_atom {
                let _timestamp = state.read_u32(&event, 12);
                let xembed_message = state.read_u32(&event, 16);
                let detail = state.read_u32(&event, 20);
                let _data1 = if event.len() >= 28 {
                    state.read_u32(&event, 24)
                } else {
                    0
                };
                let _data2 = if event.len() >= 32 {
                    state.read_u32(&event, 28)
                } else {
                    0
                };

                match xembed_message {
                    XEMBED_EMBEDDED_NOTIFY => {
                        debug!("XEMBED_EMBEDDED_NOTIFY: target={target:#x} embedder={detail:#x}");
                        // The window has been embedded — update _XEMBED_INFO
                        // to mark it as mapped/visible.
                    }
                    XEMBED_WINDOW_ACTIVATE => {
                        debug!("XEMBED_WINDOW_ACTIVATE: target={target:#x}");
                        // Forward as FocusIn if the target window exists.
                    }
                    XEMBED_WINDOW_DEACTIVATE => {
                        debug!("XEMBED_WINDOW_DEACTIVATE: target={target:#x}");
                    }
                    XEMBED_REQUEST_FOCUS => {
                        debug!("XEMBED_REQUEST_FOCUS: target={target:#x}");
                        // An embedded window requests focus. Send XEMBED_FOCUS_IN back.
                        let reply_xembed_atom = state.intern_atom("_XEMBED", false);
                        let ts = state.timestamp();
                        let focus_event = serialize_event(
                            &ClientMessageEvent {
                                response_type: CLIENT_MESSAGE_EVENT,
                                format: 32,
                                sequence: 0,
                                window: target,
                                type_: reply_xembed_atom,
                                data: [
                                    ts,
                                    XEMBED_FOCUS_IN,
                                    1, /* XEMBED_FOCUS_CURRENT */
                                    0,
                                    0,
                                ]
                                .into(),
                            },
                            state.msb_first,
                        );
                        if !state.event_router.send_event(target, focus_event.clone()) {
                            state.pending_events.push(focus_event);
                        }
                    }
                    XEMBED_FOCUS_IN => {
                        debug!("XEMBED_FOCUS_IN: target={target:#x} detail={detail}");
                    }
                    XEMBED_FOCUS_OUT => {
                        debug!("XEMBED_FOCUS_OUT: target={target:#x}");
                    }
                    XEMBED_FOCUS_NEXT | XEMBED_FOCUS_PREV => {
                        debug!("XEMBED_FOCUS_NEXT/PREV: target={target:#x}");
                    }
                    XEMBED_MODALITY_ON | XEMBED_MODALITY_OFF => {
                        debug!(
                            "XEMBED_MODALITY: target={target:#x} on={}",
                            xembed_message == XEMBED_MODALITY_ON
                        );
                    }
                    XEMBED_REGISTER_ACCELERATOR
                    | XEMBED_UNREGISTER_ACCELERATOR
                    | XEMBED_ACTIVATE_ACCELERATOR => {
                        debug!("XEMBED_ACCELERATOR: target={target:#x} msg={xembed_message}");
                    }
                    _ => {
                        debug!("XEMBED: unknown message {xembed_message} target={target:#x}");
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
        for _ in 0..crate::xserver::window_tree::MAX_TREE_DEPTH {
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
            let prop_data = state
                .windows
                .get(&requestor_wid)
                .and_then(|w| w.properties.get(&property_atom))
                .map(|pv| pv.data.clone())
                .or_else(|| {
                    state.shared_windows.lock().ok().and_then(|sw| {
                        sw.get(&requestor_wid)
                            .and_then(|w| w.properties.get(&property_atom))
                            .map(|pv| pv.data.clone())
                    })
                });
            if let Some(data) = prop_data {
                if let Ok(mut pc) = state.persistent_clipboard.lock() {
                    let entry =
                        pc.entry(CLIPBOARD_ATOM)
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
    let is_local =
        state.x11_to_uuid.contains_key(&delivery_target) || delivery_target == state.root_window;

    if is_local {
        // Target window is owned by this connection — deliver locally.
        state.pending_events.push(event);
    } else {
        // Try cross-connection delivery via the EventRouter.
        // This handles XDND (XdndEnter, XdndPosition, XdndStatus, XdndDrop,
        // XdndFinished, XdndLeave), ICCCM SelectionNotify, and any other
        // ClientMessage sent to windows on other connections.
        if !state
            .event_router
            .send_event(delivery_target, event.clone())
        {
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
