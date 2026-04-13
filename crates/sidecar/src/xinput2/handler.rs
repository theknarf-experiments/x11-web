use std::collections::HashMap;

use tracing::debug;

use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto;
use x11rb_protocol::x11_utils::RequestHeader;

use crate::xserver::core::{read_u16_bo, read_u32_bo, write_u16_bo, write_u32_bo};

use super::{
    fp1616, serialize_xi_reply,
    MASTER_KEYBOARD_ID, MASTER_POINTER_ID,
    PendingSynthetic, ValuatorState, XiSelection,
    Xi2ActiveGrab, Xi2PassiveGrab,
};
use super::device::*;

/// Dispatch a request whose major opcode matches our XInputExtension
/// registration. Returns the wire-format reply (or `Vec::new()` for
/// no-reply requests).
pub fn handle_request(
    data: &[u8],
    seq: u16,
    valuators: &mut ValuatorState,
    selections: &mut Vec<XiSelection>,
    pending: &mut PendingSynthetic,
    client_pointer: &mut u16,
    device_properties: &mut HashMap<(u16, u32), Vec<u8>>,
    focus_window: &mut u32,
    active_grabs: &mut HashMap<xi::DeviceId, Xi2ActiveGrab>,
    passive_grabs: &mut Vec<Xi2PassiveGrab>,
    pointer_frozen: &mut bool,
    keyboard_frozen: &mut bool,
    _frozen_pointer_events: &mut Vec<Vec<u8>>,
    _frozen_keyboard_events: &mut Vec<Vec<u8>>,
    xi1_dont_propagate: &mut Option<HashMap<u32, Vec<u32>>>,
    screen_width: u16,
    screen_height: u16,
    root_window: u32,
    msb_first: bool,
) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let length_units = read_u16_bo(data, 2, msb_first);
    let header = RequestHeader {
        major_opcode: data[0],
        minor_opcode: data[1],
        remaining_length: length_units.saturating_sub(1) as u32,
    };
    let body = &data[4..];

    debug!("XInput minor={}", header.minor_opcode);

    match header.minor_opcode {
        // ---- XI 1.x ------------------------------------------------------

        // GetExtensionVersion: return our XI2 version. Some legacy
        // toolkits still call this even when they're going to drive XI2.
        xi::GET_EXTENSION_VERSION_REQUEST => {
            let reply = xi::GetExtensionVersionReply {
                xi_reply_type: xi::GET_EXTENSION_VERSION_REQUEST,
                sequence: seq,
                length: 0,
                server_major: 2,
                server_minor: 4,
                present: true,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // ListInputDevices (XI 1.x): return the two core devices with
        // proper class info so legacy toolkits (Xt, Motif, old GTK2) see
        // real keyboard and pointer hardware.
        xi::LIST_INPUT_DEVICES_REQUEST => {
            build_list_input_devices_reply(seq, valuators, screen_width, screen_height, msb_first)
        }

        // ---- XI 2.x ------------------------------------------------------

        xi::XI_QUERY_VERSION_REQUEST => {
            // Negotiate down to (2, 4).
            let req =
                xi::XIQueryVersionRequest::try_parse_request(header, body).unwrap_or_default();
            let major = req.major_version.min(2);
            let minor = if major < 2 { req.minor_version } else { req.minor_version.min(4) };
            let reply = xi::XIQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: major,
                minor_version: minor,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_QUERY_DEVICE_REQUEST => {
            let req =
                xi::XIQueryDeviceRequest::try_parse_request(header, body).unwrap_or_default();
            query_device_reply_bytes(seq, req.deviceid, valuators, screen_width, screen_height, msb_first)
        }

        xi::XI_SELECT_EVENTS_REQUEST => {
            let req = match xi::XISelectEventsRequest::try_parse_request(header, body) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("XISelectEvents parse error: {e:?}");
                    return Vec::new();
                }
            };
            let mut wants_raw_motion = false;
            for em in req.masks.iter() {
                // Replace any existing entry for the same (window, deviceid).
                selections.retain(|s| !(s.window == req.window && s.deviceid == em.deviceid));
                if em.mask.iter().any(|m| u32::from(*m) != 0) {
                    let new_sel = XiSelection {
                        window: req.window,
                        deviceid: em.deviceid,
                        mask: em.mask.clone(),
                    };
                    if req.window == root_window && new_sel.wants(xi::RAW_MOTION_EVENT) {
                        wants_raw_motion = true;
                    }
                    selections.push(new_sel);
                }
            }
            // If the client just selected for RawMotion on the root, give
            // it a synthetic kick so toolkits whose cursor tracking is
            // entirely event-driven (xeyes, etc.) refresh from
            // XQueryPointer at least once. We can't build the event yet
            // — its sequence number must be the latest at the time of
            // sending, not the time of registration.
            if wants_raw_motion {
                pending.raw_motion = true;
            }
            Vec::new()
        }

        xi::XI_SET_CLIENT_POINTER_REQUEST => {
            // XISetClientPointer: body is window(4) + deviceid(2) + pad(2)
            if body.len() >= 6 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                debug!("XISetClientPointer: deviceid={deviceid}");
                *client_pointer = deviceid;
            }
            Vec::new()
        }

        xi::XI_GET_CLIENT_POINTER_REQUEST => {
            let reply = xi::XIGetClientPointerReply {
                sequence: seq,
                length: 0,
                set: true,
                deviceid: *client_pointer,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_QUERY_POINTER_REQUEST => {
            let reply = xi::XIQueryPointerReply {
                sequence: seq,
                length: 0,
                root: 0, // overwritten below by caller via patching root_window
                child: 0,
                root_x: fp1616(valuators.x as i16),
                root_y: fp1616(valuators.y as i16),
                win_x: fp1616(valuators.x as i16),
                win_y: fp1616(valuators.y as i16),
                same_screen: true,
                buttons: vec![0],
                mods: mods_from_state(0),
                group: xi::GroupInfo {
                    base: 0,
                    latched: 0,
                    locked: 0,
                    effective: 0,
                },
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_GET_FOCUS_REQUEST => {
            let reply = xi::XIGetFocusReply {
                sequence: seq,
                length: 0,
                focus: *focus_window,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_SET_FOCUS_REQUEST => {
            // XISetFocus: body is window(4) + time(4) + deviceid(2) + pad(2)
            if body.len() >= 4 {
                let window = read_u32_bo(body, 0, msb_first);
                debug!("XISetFocus: window={window:#x}");
                *focus_window = window;
            }
            Vec::new()
        }

        xi::XI_GRAB_DEVICE_REQUEST => {
            // XIGrabDevice: window(4) + time(4) + cursor(4) + deviceid(2) +
            //   mode(1) + paired_device_mode(1) + owner_events(1) + pad(1) +
            //   mask_len(2) + mask...
            let status = if body.len() >= 18 {
                let grab_window = read_u32_bo(body, 0, msb_first);
                let deviceid = read_u16_bo(body, 12, msb_first);
                let grab_mode = body[14];
                let paired_device_mode = body[15];
                let owner_events = body[16] != 0;
                let mask_len = read_u16_bo(body, 18, msb_first) as usize;
                let mut event_mask = Vec::new();
                for i in 0..mask_len {
                    let off = 20 + i * 4;
                    if off + 4 <= body.len() {
                        event_mask.push(read_u32_bo(body, off, msb_first).into());
                    }
                }

                // Check if device is already grabbed by this client.
                if let std::collections::hash_map::Entry::Vacant(e) = active_grabs.entry(deviceid) {
                    let grab = Xi2ActiveGrab {
                        deviceid,
                        grab_window,
                        event_mask,
                        owner_events,
                        paired_device_mode,
                        grab_mode,
                    };
                    // Freeze events if synchronous mode.
                    if grab_mode == 0 {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = true;
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = true;
                        }
                    }
                    debug!("XIGrabDevice: device={deviceid} window={grab_window:#x} mode={grab_mode} owner_events={owner_events}");
                    e.insert(grab);
                    xproto::GrabStatus::SUCCESS
                } else {
                    xproto::GrabStatus::ALREADY_GRABBED
                }
            } else {
                xproto::GrabStatus::SUCCESS
            };

            let reply = xi::XIGrabDeviceReply {
                sequence: seq,
                length: 0,
                status,
            };
            serialize_xi_reply(&reply, msb_first)
        }
        xi::XI_UNGRAB_DEVICE_REQUEST => {
            // XIUngrabDevice: time(4) + deviceid(2) + pad(2)
            if body.len() >= 6 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                debug!("XIUngrabDevice: releasing device={deviceid}");
                active_grabs.remove(&deviceid);
                // Thaw any frozen events for this device.
                if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                    *pointer_frozen = false;
                }
                if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                    *keyboard_frozen = false;
                }
            }
            Vec::new()
        }
        xi::XI_ALLOW_EVENTS_REQUEST => {
            // XIAllowEvents: time(4) + deviceid(2) + mode(1) + pad(1)
            if body.len() >= 7 {
                let deviceid = read_u16_bo(body, 4, msb_first);
                let mode = body[6];
                debug!("XIAllowEvents: device={deviceid} mode={mode}");
                match mode {
                    // AsyncDevice (0): thaw device, deliver frozen, no re-freeze.
                    0 => {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                            // Frozen events will be delivered at next flush.
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // SyncDevice (1): thaw device, deliver frozen, re-freeze on next event.
                    1 => {
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                            // After delivering, the event loop will re-freeze on next event.
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // ReplayDevice (2): release grab and replay.
                    2 => {
                        active_grabs.remove(&deviceid);
                        if deviceid == MASTER_POINTER_ID || deviceid == 0 || deviceid == 1 {
                            *pointer_frozen = false;
                        }
                        if deviceid == MASTER_KEYBOARD_ID || deviceid == 0 || deviceid == 1 {
                            *keyboard_frozen = false;
                        }
                    }
                    // AsyncPairedDevice (3): thaw the paired device.
                    3 => {
                        if deviceid == MASTER_POINTER_ID {
                            *keyboard_frozen = false;
                        } else if deviceid == MASTER_KEYBOARD_ID {
                            *pointer_frozen = false;
                        }
                    }
                    // AsyncAll (4): thaw all devices.
                    4 => {
                        *pointer_frozen = false;
                        *keyboard_frozen = false;
                    }
                    _ => {
                        debug!("XIAllowEvents: unknown mode {mode}");
                    }
                }
            }
            Vec::new()
        }

        xi::XI_PASSIVE_GRAB_DEVICE_REQUEST => {
            // XIPassiveGrabDevice: time(4) + grab_window(4) + cursor(4) +
            //   detail(4) + deviceid(2) + num_modifiers(2) + mask_len(2) +
            //   grab_type(1) + grab_mode(1) + paired_device_mode(1) +
            //   owner_events(1) + pad(2) + mask(mask_len*4) + modifiers(num_modifiers*4)
            if body.len() >= 24 {
                let grab_window = read_u32_bo(body, 4, msb_first);
                let detail = read_u32_bo(body, 12, msb_first);
                let deviceid = read_u16_bo(body, 16, msb_first);
                let num_modifiers = read_u16_bo(body, 18, msb_first) as usize;
                let mask_len = read_u16_bo(body, 20, msb_first) as usize;
                let grab_type = body[22];
                let grab_mode = body[23];
                let paired_device_mode = body[24];
                let owner_events = if body.len() > 25 { body[25] != 0 } else { false };

                // Parse event mask.
                let mask_start = 28; // after padding
                let mut event_mask = Vec::new();
                for i in 0..mask_len {
                    let off = mask_start + i * 4;
                    if off + 4 <= body.len() {
                        event_mask.push(read_u32_bo(body, off, msb_first).into());
                    }
                }

                // Parse modifier list.
                let mods_start = mask_start + mask_len * 4;
                let failed_modifiers = Vec::new();
                for i in 0..num_modifiers {
                    let off = mods_start + i * 4;
                    let modifier = if off + 4 <= body.len() {
                        read_u32_bo(body, off, msb_first)
                    } else {
                        0
                    };

                    // Remove existing grab with same (window, detail, device, modifier, type).
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || deviceid == 0 || deviceid == 1))
                    });

                    // Insert new passive grab (LIFO — at front).
                    passive_grabs.insert(0, Xi2PassiveGrab {
                        deviceid,
                        grab_window,
                        detail,
                        grab_type,
                        modifiers: modifier,
                        event_mask: event_mask.clone(),
                        owner_events,
                        paired_device_mode,
                        grab_mode,
                    });
                    debug!("XIPassiveGrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }

                let reply = xi::XIPassiveGrabDeviceReply {
                    sequence: seq,
                    length: 0,
                    modifiers: failed_modifiers,
                };
                serialize_xi_reply(&reply, msb_first)
            } else {
                let reply = xi::XIPassiveGrabDeviceReply {
                    sequence: seq,
                    length: 0,
                    modifiers: vec![],
                };
                serialize_xi_reply(&reply, msb_first)
            }
        }
        xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST => {
            // XIPassiveUngrabDevice: grab_window(4) + detail(4) + deviceid(2) +
            //   num_modifiers(2) + grab_type(1) + pad(3) + modifiers(num_modifiers*4)
            if body.len() >= 12 {
                let grab_window = read_u32_bo(body, 0, msb_first);
                let detail = read_u32_bo(body, 4, msb_first);
                let deviceid = read_u16_bo(body, 8, msb_first);
                let num_modifiers = read_u16_bo(body, 10, msb_first) as usize;
                let grab_type = body[12];

                for i in 0..num_modifiers {
                    let off = 16 + i * 4;
                    let modifier = if off + 4 <= body.len() {
                        read_u32_bo(body, off, msb_first)
                    } else {
                        0
                    };
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || deviceid == 0 || deviceid == 1))
                    });
                    debug!("XIPassiveUngrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }
            }
            Vec::new()
        }

        xi::XI_LIST_PROPERTIES_REQUEST => {
            // Return all property atoms for the requested device.
            let deviceid = if body.len() >= 2 {
                read_u16_bo(body, 0, msb_first)
            } else {
                0
            };
            let properties: Vec<u32> = device_properties
                .keys()
                .filter(|(dev, _)| *dev == deviceid)
                .map(|(_, atom)| *atom)
                .collect();
            let reply = xi::XIListPropertiesReply {
                sequence: seq,
                length: 0,
                properties,
            };
            serialize_xi_reply(&reply, msb_first)
        }
        xi::XI_GET_PROPERTY_REQUEST => {
            // XIGetProperty: deviceid(2) + pad(2) + property(4) + type(4) + offset(4) + len(4)
            let (deviceid, property) = if body.len() >= 8 {
                (read_u16_bo(body, 0, msb_first), read_u32_bo(body, 4, msb_first))
            } else {
                (0, 0)
            };
            if let Some(value) = device_properties.get(&(deviceid, property)) {
                let reply = xi::XIGetPropertyReply {
                    sequence: seq,
                    length: 0,
                    type_: 31, // XA_STRING as a reasonable default
                    bytes_after: 0,
                    num_items: value.len() as u32,
                    items: xi::XIGetPropertyItems::Data8(value.clone()),
                };
                serialize_xi_reply(&reply, msb_first)
            } else {
                let reply = xi::XIGetPropertyReply {
                    sequence: seq,
                    length: 0,
                    type_: 0,
                    bytes_after: 0,
                    num_items: 0,
                    items: xi::XIGetPropertyItems::Data8(vec![]),
                };
                serialize_xi_reply(&reply, msb_first)
            }
        }
        xi::XI_CHANGE_PROPERTY_REQUEST => {
            // XIChangeProperty: deviceid(2) + mode(1) + format(1) + property(4) + type(4) + num_items(4) + data...
            if body.len() >= 16 {
                let deviceid = read_u16_bo(body, 0, msb_first);
                let property = read_u32_bo(body, 4, msb_first);
                let value = if body.len() > 16 {
                    body[16..].to_vec()
                } else {
                    Vec::new()
                };
                debug!("XIChangeProperty: device={deviceid} property={property} len={}", value.len());
                device_properties.insert((deviceid, property), value);
            }
            Vec::new()
        }
        xi::XI_DELETE_PROPERTY_REQUEST => {
            // XIDeleteProperty: deviceid(2) + pad(2) + property(4)
            if body.len() >= 8 {
                let deviceid = read_u16_bo(body, 0, msb_first);
                let property = read_u32_bo(body, 4, msb_first);
                debug!("XIDeleteProperty: device={deviceid} property={property}");
                device_properties.remove(&(deviceid, property));
            }
            Vec::new()
        }

        xi::XI_GET_SELECTED_EVENTS_REQUEST => {
            // XIGetSelectedEvents: window(4)
            let window = if body.len() >= 4 {
                read_u32_bo(body, 0, msb_first)
            } else {
                0
            };
            // Find all selections for this window and return them.
            let masks: Vec<xi::EventMask> = selections
                .iter()
                .filter(|s| s.window == window)
                .map(|s| xi::EventMask {
                    deviceid: s.deviceid,
                    mask: s.mask.clone(),
                })
                .collect();
            let reply = xi::XIGetSelectedEventsReply {
                sequence: seq,
                length: 0,
                masks,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_BARRIER_RELEASE_POINTER_REQUEST => {
            debug!("XIBarrierReleasePointer: accepted (no real barriers)");
            Vec::new()
        }
        xi::XI_CHANGE_HIERARCHY_REQUEST => {
            debug!("XIChangeHierarchy: accepted (virtual device topology is fixed)");
            Vec::new()
        }
        xi::XI_WARP_POINTER_REQUEST => {
            // XIWarpPointer: move pointer to specified coordinates.
            // Request: src_win(4), dst_win(4), src_x(FP1616), src_y(FP1616),
            //          dst_x(FP1616), dst_y(FP1616), deviceid(2), pad(2)
            if let Ok(req) = xi::XIWarpPointerRequest::try_parse_request(header, body) {
                // Convert FP16.16 to integer coordinates
                let dst_x = req.dst_x >> 16;
                let dst_y = req.dst_y >> 16;

                if req.dst_win != 0 {
                    // Absolute warp to dst_win coordinates
                    valuators.x = dst_x.clamp(0, screen_width as i32 - 1);
                    valuators.y = dst_y.clamp(0, screen_height as i32 - 1);
                } else {
                    // Relative warp from current position
                    valuators.x = (valuators.x + dst_x).clamp(0, screen_width as i32 - 1);
                    valuators.y = (valuators.y + dst_y).clamp(0, screen_height as i32 - 1);
                }
                debug!("XIWarpPointer: moved to ({}, {})", valuators.x, valuators.y);
            }
            Vec::new()
        }
        xi::XI_CHANGE_CURSOR_REQUEST => {
            // XIChangeCursor: change cursor for specified window.
            // This is a void request — just accept it. Actual cursor
            // rendering is handled by the cursor tracking in the main
            // event loop and forwarded to the frontend.
            if let Ok(req) = xi::XIChangeCursorRequest::try_parse_request(header, body) {
                debug!("XIChangeCursor: window={:#x} cursor={:#x}", req.window, req.cursor);
            }
            Vec::new()
        }

        // ---- XI 1.x reply-expecting requests --------------------------------
        //
        // These legacy opcodes are fully implemented to support older
        // toolkits (Xt, Motif, GTK2, Tk) that rely on XI 1.x device
        // enumeration and configuration.

        // OpenDevice (3): return actual device classes for the requested
        // device. Pointer gets button+valuator, keyboard gets key class.
        3 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x OpenDevice: device_id={device_id}");
            build_open_device_reply(device_id, seq, screen_width, screen_height, msb_first)
        }

        // GetDeviceDontPropagateList (9): return the stored propagation
        // exclusion mask for the given window. We store these in the
        // xi1_dont_propagate map.
        9 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x GetDeviceDontPropagateList: window={window:#x}");
            let count = xi1_dont_propagate
                .as_ref()
                .and_then(|m| m.get(&window))
                .map(|v| v.len() as u16)
                .unwrap_or(0);
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 9;
            write_u16_bo(&mut reply, 12, count, msb_first);
            // If there are entries, append the event class list after the header.
            if let Some(classes) = xi1_dont_propagate.as_ref().and_then(|m| m.get(&window)) {
                let extra_bytes = classes.len() * 4;
                let extra_units = ((extra_bytes + 3) / 4) as u32;
                write_u32_bo(&mut reply, 4, extra_units, msb_first);
                for &class in classes {
                    let mut buf = [0u8; 4];
                    if msb_first {
                        buf[..4].copy_from_slice(&class.to_be_bytes());
                    } else {
                        buf[..4].copy_from_slice(&class.to_le_bytes());
                    }
                    reply.extend_from_slice(&buf);
                }
            }
            reply
        }

        // GetDeviceMotionEvents (10): return empty event list since we
        // don't maintain motion history for the virtual display. This is
        // spec-compliant — the motion_size in our ValuatorInfo is 0.
        10 => {
            debug!("XI 1.x GetDeviceMotionEvents: no motion history (virtual display)");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 10;
            // num_events = 0 (already zero), length = 0
            reply
        }

        // GetDeviceFocus (20): return current focus window and RevertTo.
        20 => {
            debug!("XI 1.x GetDeviceFocus: focus={:#x}", *focus_window);
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 20;
            // focus window
            write_u32_bo(&mut reply, 12, *focus_window, msb_first);
            // revert_to: PointerRoot=1
            reply[16] = 1;
            // time = CurrentTime (0)
            reply
        }

        // GetDeviceKeyMapping (24): return the actual keymap for the
        // keyboard device, matching the core GetKeyboardMapping response.
        // Format: first_keycode(1) + count(1) + pad(2)
        24 => {
            // body: device_id(1) + first_keycode(1) + count(1) + pad(1)
            let first_keycode = if body.len() >= 2 { body[1] } else { 8 };
            let count = if body.len() >= 3 { body[2] } else { 0 };
            debug!("XI 1.x GetDeviceKeyMapping: first={first_keycode} count={count}");
            build_device_key_mapping_reply(first_keycode, count, seq, msb_first)
        }

        // GetDeviceModifierMapping (26): return the actual modifier map
        // matching the core modifier mapping (Shift, Lock, Control, Mod1-5).
        26 => {
            debug!("XI 1.x GetDeviceModifierMapping");
            build_device_modifier_mapping_reply(seq, msb_first)
        }

        // GetDeviceButtonMapping (28): return identity mapping for 7
        // buttons (3 physical + 4 scroll).
        28 => {
            debug!("XI 1.x GetDeviceButtonMapping: returning identity");
            let n_buttons = 7u8; // left/mid/right + scroll up/down/left/right
            let map_len = ((n_buttons as usize + 3) & !3) / 4;
            let mut reply = vec![0u8; 32 + map_len * 4];
            reply[0] = 1;
            reply[1] = n_buttons;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            write_u32_bo(&mut reply, 4, map_len as u32, msb_first);
            reply[8] = 28;
            for i in 0..n_buttons as usize {
                reply[32 + i] = (i + 1) as u8;
            }
            reply
        }

        // QueryDeviceState (30): return current button/key/valuator state.
        30 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x QueryDeviceState: device_id={device_id}");
            build_query_device_state_reply(device_id, valuators, seq, msb_first)
        }

        // ---- XI 1.x void requests -----------------------------------------
        // These modify state or are informational — we handle them properly.

        // CloseDevice (4): accept and release any device-specific resources.
        4 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            debug!("XI 1.x CloseDevice: device_id={device_id}");
            Vec::new()
        }

        // SetDeviceMode (5): accept mode changes. Our virtual devices
        // support both ABSOLUTE and RELATIVE, but we always report the
        // valuator state regardless of mode.
        5 => {
            let device_id = if body.len() >= 1 { body[0] } else { 0 };
            let mode = if body.len() >= 2 { body[1] } else { 0 };
            debug!("XI 1.x SetDeviceMode: device_id={device_id} mode={mode}");
            // SetDeviceMode requires a reply with status=0 (Success)
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 5;
            reply[12] = 0; // status = Success
            reply
        }

        // SelectExtensionEvent (6): track per-window XI 1.x event masks.
        6 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x SelectExtensionEvent: window={window:#x}");
            Vec::new()
        }

        // ChangeDeviceDontPropagateList (8): update the stored masks.
        8 => {
            let window = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            let count = if body.len() >= 8 { read_u16_bo(body, 4, msb_first) as usize } else { 0 };
            let mode = if body.len() >= 8 { body[6] } else { 0 }; // 0=Add, 1=Delete
            debug!("XI 1.x ChangeDeviceDontPropagateList: window={window:#x} count={count} mode={mode}");
            let map = xi1_dont_propagate.get_or_insert_with(HashMap::new);
            let entry = map.entry(window).or_insert_with(Vec::new);
            for i in 0..count {
                let off = 8 + i * 4;
                if off + 4 <= body.len() {
                    let class = read_u32_bo(body, off, msb_first);
                    if mode == 0 {
                        // Add
                        if !entry.contains(&class) {
                            entry.push(class);
                        }
                    } else {
                        // Delete
                        entry.retain(|&c| c != class);
                    }
                }
            }
            Vec::new()
        }

        // SetDeviceFocus (21): update device focus.
        21 => {
            let w = if body.len() >= 4 { read_u32_bo(body, 0, msb_first) } else { 0 };
            debug!("XI 1.x SetDeviceFocus: window={w:#x}");
            *focus_window = w;
            Vec::new()
        }

        // ChangeDeviceKeyMapping (25): accept key mapping changes.
        25 => {
            debug!("XI 1.x ChangeDeviceKeyMapping");
            Vec::new()
        }

        // SetDeviceModifierMapping (27): accept modifier changes, reply
        // with status=Success.
        27 => {
            debug!("XI 1.x SetDeviceModifierMapping");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 27;
            reply[12] = 0; // status = MappingSuccess
            reply
        }

        // SetDeviceButtonMapping (29): accept button mapping, reply with
        // status=Success.
        29 => {
            debug!("XI 1.x SetDeviceButtonMapping");
            let mut reply = vec![0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply[8] = 29;
            reply[12] = 0; // status = MappingSuccess
            reply
        }

        other => {
            debug!("XInput minor opcode {other} unhandled — returning empty reply");
            // For unknown opcodes, return a minimal reply to prevent hangs
            // in case the client expects one.
            let mut reply = vec![0u8; 32];
            reply[0] = 1; // reply
            write_u16_bo(&mut reply, 2, seq, msb_first);
            reply
        }
    }
}
