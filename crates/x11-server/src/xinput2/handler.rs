use std::collections::HashMap;

use tracing::debug;

use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto;
use x11rb_protocol::x11_utils::RequestHeader;

use crate::xserver::core::read_u16_bo;

use super::device::*;
use super::{
    fp1616, serialize_xi_reply, PendingSynthetic, ValuatorState, Xi2ActiveGrab, Xi2PassiveGrab,
    XiSelection, MASTER_KEYBOARD_ID, MASTER_POINTER_ID,
};

/// XInput's "All Devices" virtual device IDs: 0 (XIAllDevices) and 1
/// (XIAllMasterDevices) both target the entire master pair.
const XI_ALL_DEVICES: xi::DeviceId = 0;
const XI_ALL_MASTER_DEVICES: xi::DeviceId = 1;

#[inline]
fn is_any_master_id(id: xi::DeviceId) -> bool {
    id == XI_ALL_DEVICES || id == XI_ALL_MASTER_DEVICES
}

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
    custom_keymap: &std::collections::HashMap<u8, Vec<u32>>,
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
            let minor = if major < 2 {
                req.minor_version
            } else {
                req.minor_version.min(4)
            };
            let reply = xi::XIQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: major,
                minor_version: minor,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        xi::XI_QUERY_DEVICE_REQUEST => {
            let req = xi::XIQueryDeviceRequest::try_parse_request(header, body).unwrap_or_default();
            query_device_reply_bytes(
                seq,
                req.deviceid,
                valuators,
                screen_width,
                screen_height,
                msb_first,
            )
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
            if let Ok(req) = xi::XISetClientPointerRequest::try_parse_request(header, body) {
                debug!("XISetClientPointer: deviceid={}", req.deviceid);
                *client_pointer = req.deviceid;
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
            if let Ok(req) = xi::XISetFocusRequest::try_parse_request(header, body) {
                debug!("XISetFocus: window={:#x}", req.window);
                *focus_window = req.window;
            }
            Vec::new()
        }

        xi::XI_GRAB_DEVICE_REQUEST => {
            let status = if let Ok(req) = xi::XIGrabDeviceRequest::try_parse_request(header, body) {
                let deviceid = req.deviceid;
                let grab_window = req.window;
                let grab_mode = u8::from(req.mode);
                let paired_device_mode = u8::from(req.paired_device_mode);
                let owner_events = u8::from(req.owner_events) != 0;
                let event_mask: Vec<xi::XIEventMask> = req.mask.iter().map(|&m| m.into()).collect();

                // Check if device is already grabbed by this client.
                if let std::collections::hash_map::Entry::Vacant(e) = active_grabs.entry(deviceid) {
                    if grab_mode == 0 {
                        if deviceid == MASTER_POINTER_ID || is_any_master_id(deviceid) {
                            *pointer_frozen = true;
                        }
                        if deviceid == MASTER_KEYBOARD_ID || is_any_master_id(deviceid) {
                            *keyboard_frozen = true;
                        }
                    }
                    debug!("XIGrabDevice: device={deviceid} window={grab_window:#x} mode={grab_mode} owner_events={owner_events}");
                    e.insert(Xi2ActiveGrab {
                        deviceid,
                        grab_window,
                        event_mask,
                        owner_events,
                        paired_device_mode,
                        grab_mode,
                    });
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
            if let Ok(req) = xi::XIUngrabDeviceRequest::try_parse_request(header, body) {
                let deviceid = req.deviceid;
                debug!("XIUngrabDevice: releasing device={deviceid}");
                active_grabs.remove(&deviceid);
                // Thaw any frozen events for this device.
                if deviceid == MASTER_POINTER_ID || is_any_master_id(deviceid) {
                    *pointer_frozen = false;
                }
                if deviceid == MASTER_KEYBOARD_ID || is_any_master_id(deviceid) {
                    *keyboard_frozen = false;
                }
            }
            Vec::new()
        }
        xi::XI_ALLOW_EVENTS_REQUEST => {
            if let Ok(req) = xi::XIAllowEventsRequest::try_parse_request(header, body) {
                let deviceid = req.deviceid;
                let mode = req.event_mode;
                let is_pointer = deviceid == MASTER_POINTER_ID || is_any_master_id(deviceid);
                let is_keyboard = deviceid == MASTER_KEYBOARD_ID || is_any_master_id(deviceid);
                debug!("XIAllowEvents: device={deviceid} mode={:?}", mode);
                match mode {
                    // Thaw device, deliver frozen, no re-freeze.
                    xi::EventMode::ASYNC_DEVICE => {
                        if is_pointer {
                            *pointer_frozen = false;
                        }
                        if is_keyboard {
                            *keyboard_frozen = false;
                        }
                    }
                    // Thaw device, deliver frozen, re-freeze on next event.
                    xi::EventMode::SYNC_DEVICE => {
                        if is_pointer {
                            *pointer_frozen = false;
                        }
                        if is_keyboard {
                            *keyboard_frozen = false;
                        }
                    }
                    // Release grab and replay.
                    xi::EventMode::REPLAY_DEVICE => {
                        active_grabs.remove(&deviceid);
                        if is_pointer {
                            *pointer_frozen = false;
                        }
                        if is_keyboard {
                            *keyboard_frozen = false;
                        }
                    }
                    // Thaw the paired device.
                    xi::EventMode::ASYNC_PAIRED_DEVICE => {
                        if deviceid == MASTER_POINTER_ID {
                            *keyboard_frozen = false;
                        } else if deviceid == MASTER_KEYBOARD_ID {
                            *pointer_frozen = false;
                        }
                    }
                    // Thaw both devices in the master pair.
                    xi::EventMode::ASYNC_PAIR => {
                        *pointer_frozen = false;
                        *keyboard_frozen = false;
                    }
                    other => {
                        debug!("XIAllowEvents: unhandled mode {:?}", other);
                    }
                }
            }
            Vec::new()
        }

        xi::XI_PASSIVE_GRAB_DEVICE_REQUEST => {
            let failed_modifiers: Vec<xi::GrabModifierInfo> = Vec::new();
            if let Ok(req) = xi::XIPassiveGrabDeviceRequest::try_parse_request(header, body) {
                let grab_window = req.grab_window;
                let detail = req.detail;
                let deviceid = req.deviceid;
                let grab_type = u8::from(req.grab_type);
                let grab_mode = u8::from(req.grab_mode);
                let paired_device_mode = u8::from(req.paired_device_mode);
                let owner_events = u8::from(req.owner_events) != 0;
                let event_mask: Vec<xi::XIEventMask> = req.mask.iter().map(|&m| m.into()).collect();

                for &modifier in req.modifiers.iter() {
                    // Remove existing grab with same (window, detail, device, modifier, type).
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || is_any_master_id(deviceid)))
                    });

                    // Insert new passive grab (LIFO — at front).
                    passive_grabs.insert(
                        0,
                        Xi2PassiveGrab {
                            deviceid,
                            grab_window,
                            detail,
                            grab_type,
                            modifiers: modifier,
                            event_mask: event_mask.clone(),
                            owner_events,
                            paired_device_mode,
                            grab_mode,
                        },
                    );
                    debug!("XIPassiveGrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }
            }
            let reply = xi::XIPassiveGrabDeviceReply {
                sequence: seq,
                length: 0,
                modifiers: failed_modifiers,
            };
            serialize_xi_reply(&reply, msb_first)
        }
        xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST => {
            if let Ok(req) = xi::XIPassiveUngrabDeviceRequest::try_parse_request(header, body) {
                let grab_window = req.grab_window;
                let detail = req.detail;
                let deviceid = req.deviceid;
                let grab_type = u8::from(req.grab_type);
                for &modifier in req.modifiers.iter() {
                    passive_grabs.retain(|g| {
                        !(g.grab_window == grab_window
                            && g.detail == detail
                            && g.grab_type == grab_type
                            && g.modifiers == modifier
                            && (g.deviceid == deviceid || is_any_master_id(deviceid)))
                    });
                    debug!("XIPassiveUngrabDevice: device={deviceid} window={grab_window:#x} detail={detail} type={grab_type} mod={modifier:#x}");
                }
            }
            Vec::new()
        }

        xi::XI_LIST_PROPERTIES_REQUEST => {
            let deviceid = xi::XIListPropertiesRequest::try_parse_request(header, body)
                .map(|r| r.deviceid)
                .unwrap_or(0);
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
            let (deviceid, property) = xi::XIGetPropertyRequest::try_parse_request(header, body)
                .map(|r| (r.deviceid, r.property))
                .unwrap_or((0, 0));
            if let Some(value) = device_properties.get(&(deviceid, property)) {
                let reply = xi::XIGetPropertyReply {
                    sequence: seq,
                    length: 0,
                    // XA_STRING is the reasonable default for unknown property types.
                    type_: crate::xserver::atoms::predef::STRING,
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
            if let Ok(req) = xi::XIChangePropertyRequest::try_parse_request(header, body) {
                /// Wire-format size of the XIChangeProperty fixed header
                /// preceding the variable-length items array (deviceid (2) +
                /// mode (1) + format (1) + property (4) + type (4) +
                /// num_items (4) = 16 bytes).
                const XI_CHANGE_PROPERTY_HEADER_SIZE: usize = 16;
                // The typed parser already validated deviceid/property/format/num_items.
                // We keep the raw items bytes (post-header) so XIGetProperty
                // can echo them back unchanged.
                let value = body
                    .get(XI_CHANGE_PROPERTY_HEADER_SIZE..)
                    .map(|s| s.to_vec())
                    .unwrap_or_default();
                debug!(
                    "XIChangeProperty: device={} property={} len={}",
                    req.deviceid,
                    req.property,
                    value.len()
                );
                device_properties.insert((req.deviceid, req.property), value);
            }
            Vec::new()
        }
        xi::XI_DELETE_PROPERTY_REQUEST => {
            if let Ok(req) = xi::XIDeletePropertyRequest::try_parse_request(header, body) {
                debug!(
                    "XIDeleteProperty: device={} property={}",
                    req.deviceid, req.property
                );
                device_properties.remove(&(req.deviceid, req.property));
            }
            Vec::new()
        }

        xi::XI_GET_SELECTED_EVENTS_REQUEST => {
            let window = xi::XIGetSelectedEventsRequest::try_parse_request(header, body)
                .map(|r| r.window)
                .unwrap_or(0);
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
                debug!(
                    "XIChangeCursor: window={:#x} cursor={:#x}",
                    req.window, req.cursor
                );
            }
            Vec::new()
        }

        // ---- XI 1.x reply-expecting requests --------------------------------
        //
        // These legacy opcodes are fully implemented to support older
        // toolkits (Xt, Motif, GTK2, Tk) that rely on XI 1.x device
        // enumeration and configuration.

        // OpenDevice: return actual device classes for the requested
        // device. Pointer gets button+valuator, keyboard gets key class.
        xi::OPEN_DEVICE_REQUEST => {
            let device_id = xi::OpenDeviceRequest::try_parse_request(header, body)
                .map(|r| r.device_id)
                .unwrap_or(0);
            debug!("XI 1.x OpenDevice: device_id={device_id}");
            build_open_device_reply(device_id, seq, screen_width, screen_height, msb_first)
        }

        // GetDeviceDontPropagateList: return the stored propagation
        // exclusion mask for the given window. We store these in the
        // xi1_dont_propagate map.
        xi::GET_DEVICE_DONT_PROPAGATE_LIST_REQUEST => {
            let window = xi::GetDeviceDontPropagateListRequest::try_parse_request(header, body)
                .map(|r| r.window)
                .unwrap_or(0);
            debug!("XI 1.x GetDeviceDontPropagateList: window={window:#x}");
            let classes = xi1_dont_propagate
                .as_ref()
                .and_then(|m| m.get(&window))
                .cloned()
                .unwrap_or_default();
            let reply = xi::GetDeviceDontPropagateListReply {
                xi_reply_type: xi::GET_DEVICE_DONT_PROPAGATE_LIST_REQUEST,
                sequence: seq,
                length: 0,
                classes,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // GetDeviceMotionEvents: return empty event list since we
        // don't maintain motion history for the virtual display. This is
        // spec-compliant — the motion_size in our ValuatorInfo is 0.
        xi::GET_DEVICE_MOTION_EVENTS_REQUEST => {
            debug!("XI 1.x GetDeviceMotionEvents: no motion history (virtual display)");
            let reply = xi::GetDeviceMotionEventsReply {
                xi_reply_type: xi::GET_DEVICE_MOTION_EVENTS_REQUEST,
                sequence: seq,
                length: 0,
                num_axes: 0,
                device_mode: xi::ValuatorMode::ABSOLUTE,
                events: Vec::new(),
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // GetDeviceFocus: return current focus window and RevertTo.
        xi::GET_DEVICE_FOCUS_REQUEST => {
            debug!("XI 1.x GetDeviceFocus: focus={:#x}", *focus_window);
            let reply = xi::GetDeviceFocusReply {
                xi_reply_type: xi::GET_DEVICE_FOCUS_REQUEST,
                sequence: seq,
                length: 0,
                focus: *focus_window,
                time: 0,
                revert_to: xproto::InputFocus::POINTER_ROOT,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // GetDeviceKeyMapping: return the actual keymap for the keyboard
        // device, matching the core GetKeyboardMapping response.
        xi::GET_DEVICE_KEY_MAPPING_REQUEST => {
            let req = xi::GetDeviceKeyMappingRequest::try_parse_request(header, body).ok();
            let first_keycode = req.as_ref().map(|r| r.first_keycode).unwrap_or(8);
            let count = req.as_ref().map(|r| r.count).unwrap_or(0);
            debug!("XI 1.x GetDeviceKeyMapping: first={first_keycode} count={count}");
            build_device_key_mapping_reply(first_keycode, count, seq, msb_first, custom_keymap)
        }

        // GetDeviceModifierMapping: return the actual modifier map
        // matching the core modifier mapping (Shift, Lock, Control, Mod1-5).
        xi::GET_DEVICE_MODIFIER_MAPPING_REQUEST => {
            debug!("XI 1.x GetDeviceModifierMapping");
            build_device_modifier_mapping_reply(seq, msb_first)
        }

        // GetDeviceButtonMapping: return identity mapping for 7 buttons
        // (3 physical + 4 scroll).
        xi::GET_DEVICE_BUTTON_MAPPING_REQUEST => {
            debug!("XI 1.x GetDeviceButtonMapping: returning identity");
            let reply = xi::GetDeviceButtonMappingReply {
                xi_reply_type: xi::GET_DEVICE_BUTTON_MAPPING_REQUEST,
                sequence: seq,
                length: 0,
                map: (1..=7).collect(),
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // QueryDeviceState: return current button/key/valuator state.
        xi::QUERY_DEVICE_STATE_REQUEST => {
            let device_id = xi::QueryDeviceStateRequest::try_parse_request(header, body)
                .map(|r| r.device_id)
                .unwrap_or(0);
            debug!("XI 1.x QueryDeviceState: device_id={device_id}");
            build_query_device_state_reply(device_id, valuators, seq, msb_first)
        }

        // ---- XI 1.x void requests -----------------------------------------
        // These modify state or are informational — we handle them properly.

        // CloseDevice: accept and release any device-specific resources.
        xi::CLOSE_DEVICE_REQUEST => {
            let device_id = xi::CloseDeviceRequest::try_parse_request(header, body)
                .map(|r| r.device_id)
                .unwrap_or(0);
            debug!("XI 1.x CloseDevice: device_id={device_id}");
            Vec::new()
        }

        // SetDeviceMode: accept mode changes. Our virtual devices support
        // both ABSOLUTE and RELATIVE, but we always report the valuator
        // state regardless of mode.
        xi::SET_DEVICE_MODE_REQUEST => {
            let req = xi::SetDeviceModeRequest::try_parse_request(header, body).ok();
            let device_id = req.as_ref().map(|r| r.device_id).unwrap_or(0);
            let mode = req.map(|r| u8::from(r.mode)).unwrap_or(0);
            debug!("XI 1.x SetDeviceMode: device_id={device_id} mode={mode}");
            let reply = xi::SetDeviceModeReply {
                xi_reply_type: xi::SET_DEVICE_MODE_REQUEST,
                sequence: seq,
                length: 0,
                status: xproto::GrabStatus::SUCCESS,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // SelectExtensionEvent: track per-window XI 1.x event masks.
        xi::SELECT_EXTENSION_EVENT_REQUEST => {
            let window = xi::SelectExtensionEventRequest::try_parse_request(header, body)
                .map(|r| r.window)
                .unwrap_or(0);
            debug!("XI 1.x SelectExtensionEvent: window={window:#x}");
            Vec::new()
        }

        // ChangeDeviceDontPropagateList: update the stored masks.
        xi::CHANGE_DEVICE_DONT_PROPAGATE_LIST_REQUEST => {
            if let Ok(req) =
                xi::ChangeDeviceDontPropagateListRequest::try_parse_request(header, body)
            {
                let window = req.window;
                let mode = req.mode;
                debug!(
                    "XI 1.x ChangeDeviceDontPropagateList: window={window:#x} count={} mode={:?}",
                    req.classes.len(),
                    mode,
                );
                let map = xi1_dont_propagate.get_or_insert_with(HashMap::new);
                let entry = map.entry(window).or_default();
                for &class in req.classes.iter() {
                    if mode == xi::PropagateMode::ADD_TO_LIST {
                        if !entry.contains(&class) {
                            entry.push(class);
                        }
                    } else {
                        entry.retain(|&c| c != class);
                    }
                }
            }
            Vec::new()
        }

        // SetDeviceFocus: update device focus.
        xi::SET_DEVICE_FOCUS_REQUEST => {
            let w = xi::SetDeviceFocusRequest::try_parse_request(header, body)
                .map(|r| r.focus)
                .unwrap_or(0);
            debug!("XI 1.x SetDeviceFocus: window={w:#x}");
            *focus_window = w;
            Vec::new()
        }

        // ChangeDeviceKeyMapping: accept key mapping changes.
        xi::CHANGE_DEVICE_KEY_MAPPING_REQUEST => {
            debug!("XI 1.x ChangeDeviceKeyMapping");
            Vec::new()
        }

        // SetDeviceModifierMapping: accept modifier changes, reply
        // with status=Success.
        xi::SET_DEVICE_MODIFIER_MAPPING_REQUEST => {
            debug!("XI 1.x SetDeviceModifierMapping");
            let reply = xi::SetDeviceModifierMappingReply {
                xi_reply_type: xi::SET_DEVICE_MODIFIER_MAPPING_REQUEST,
                sequence: seq,
                length: 0,
                status: xproto::MappingStatus::SUCCESS,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        // SetDeviceButtonMapping: accept button mapping, reply with
        // status=Success.
        xi::SET_DEVICE_BUTTON_MAPPING_REQUEST => {
            debug!("XI 1.x SetDeviceButtonMapping");
            let reply = xi::SetDeviceButtonMappingReply {
                xi_reply_type: xi::SET_DEVICE_BUTTON_MAPPING_REQUEST,
                sequence: seq,
                length: 0,
                status: xproto::MappingStatus::SUCCESS,
            };
            serialize_xi_reply(&reply, msb_first)
        }

        other => {
            debug!("XInput minor opcode {other} unhandled — returning empty reply");
            // Minimal reply to prevent hangs if the client expects one.
            crate::xserver::reply::ReplyBuf::fixed(seq, msb_first).build()
        }
    }
}
