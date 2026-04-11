//! XInput / XInput2 protocol implementation.
//!
//! We dispatch and reply to enough of the XI 1.x and XI 2.x request set
//! to keep modern toolkits (Xt, GDK 3, Qt, Mozilla widgets) happy.
//!
//! For wire-format ground truth we re-use `x11rb_protocol::protocol::xinput`
//! types (parsed from the upstream X11 XML protocol description) and let
//! their `Serialize` impls produce the bytes. This guarantees we never
//! drift from the canonical layout.

use tracing::{debug, warn};

use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto;
use x11rb_protocol::x11_utils::{RequestHeader, Serialize as _};

use x11_web_protocol::InputEvent;

/// Major opcode we register for XInputExtension in QueryExtension. 131 is
/// the conventional value used by the upstream X server, but the actual
/// number doesn't matter — clients pick it up from QueryExtension.
pub const XI_MAJOR_OPCODE: u8 = 131;
/// Range reserved for XI legacy events. We never emit those (we use the
/// XI2 generic-event path) but the value still has to be advertised.
pub const XI_FIRST_EVENT: u8 = 105;
pub const XI_FIRST_ERROR: u8 = 152;

/// Device IDs we expose. Two master devices is the minimum modern XI
/// clients (e.g. GTK 3) expect.
pub const MASTER_POINTER_ID: xi::DeviceId = 2;
pub const MASTER_KEYBOARD_ID: xi::DeviceId = 3;

/// Per-window XI2 event subscription. One per `(window, deviceid)` tuple.
#[derive(Clone, Debug)]
pub struct XiSelection {
    pub window: u32,
    pub deviceid: xi::DeviceId,
    pub mask: Vec<xi::XIEventMask>,
}

impl XiSelection {
    pub fn wants(&self, evtype: u16) -> bool {
        // The XI2 mask is a bitfield indexed by event type number.
        // x11rb represents it as a Vec<u32> (one u32 per 32 event types).
        let bit = evtype as u32;
        let word = (bit / 32) as usize;
        let in_word = bit % 32;
        self.mask
            .get(word)
            .map(|w| (u32::from(*w) >> in_word) & 1 != 0)
            .unwrap_or(false)
    }
}

fn fp1616(v: i16) -> xi::Fp1616 {
    (v as i32) << 16
}

fn fp3232(int: i32) -> xi::Fp3232 {
    xi::Fp3232 { integral: int, frac: 0 }
}

/// Per-axis valuator state we track for the master pointer. The X server
/// uses these to populate `XIValuatorClassInfo.value` in `XIQueryDevice`
/// replies and the per-event valuator data in motion / button events.
///
/// `scroll_v` / `scroll_h` accumulate over the lifetime of the connection
/// — XI2 clients compute scroll deltas from successive valuator values.
/// When a wheel event arrives we bump these by `1.0` (matching the
/// `increment` we report in our scroll classes) per discrete wheel notch.
#[derive(Clone, Debug, Default)]
pub struct ValuatorState {
    pub x: i32,
    pub y: i32,
    pub scroll_v: i32,
    pub scroll_h: i32,
}

/// Axis numbers we use for the master pointer's valuator/scroll
/// classes. Valuator 0 / 1 are the absolute X / Y axes (emitted as
/// `XIValuatorClass` entries in our `XIQueryDevice` reply); 2 / 3
/// are the vertical / horizontal scroll axes (`XIScrollClass`).
pub const AXIS_SCROLL_V: u16 = 2;
pub const AXIS_SCROLL_H: u16 = 3;

/// Build a `Reply` byte slice with the standard 32-byte header populated.
/// `xi_reply_type` is the value placed at byte 1 of the reply (set to the
/// XI minor opcode of the request being answered, matching the upstream
/// xserver convention).
#[allow(dead_code)]
fn xi_reply_header(seq: u16, xi_reply_type: u8, length_units: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 32 + (length_units as usize) * 4];
    buf[0] = 1; // X_Reply
    buf[1] = xi_reply_type;
    buf[2..4].copy_from_slice(&seq.to_le_bytes());
    buf[4..8].copy_from_slice(&length_units.to_le_bytes());
    buf
}

/// Serialize an x11rb XInput reply, then patch up its `length` field
/// (in 4-byte units after the 32-byte header). x11rb's `Serialize` impls
/// don't compute `length` automatically — it has to match the actual
/// number of trailing bytes or XCB hits "Too much data requested".
fn serialize_xi_reply<R: x11rb_protocol::x11_utils::Serialize>(reply: &R) -> Vec<u8> {
    let mut buf = Vec::new();
    reply.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    debug_assert!(buf.len() >= 32, "XI reply must be at least 32 bytes");
    let length_units = ((buf.len() - 32) / 4) as u32;
    buf[4..8].copy_from_slice(&length_units.to_le_bytes());
    buf
}

/// Build the bytes for the master pointer's `XIDeviceInfo`. The screen
/// dimensions bound the valuator axes — XI clients use these to clamp
/// or normalise pointer position values.
fn build_master_pointer_info(
    valuators: &ValuatorState,
    screen_width: u16,
    screen_height: u16,
) -> xi::XIDeviceInfo {
    let button_class = xi::DeviceClass {
        len: 0, // overwritten below
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Button(xi::DeviceClassDataButton {
            // 7-button pointer: left/middle/right + scroll up/down/left/right.
            // Empty state mask (no buttons currently pressed) padded to a
            // multiple of 32 bits.
            state: vec![0],
            labels: vec![0; 7],
        }),
    };

    let valuator_x = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: 0,
            label: 0, // None
            min: fp3232(0),
            max: fp3232(i32::from(screen_width)),
            value: fp3232(valuators.x),
            resolution: 1,
            mode: xi::ValuatorMode::ABSOLUTE,
        }),
    };

    let valuator_y = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: 1,
            label: 0,
            min: fp3232(0),
            max: fp3232(i32::from(screen_height)),
            value: fp3232(valuators.y),
            resolution: 1,
            mode: xi::ValuatorMode::ABSOLUTE,
        }),
    };

    // The scroll axes have to be exposed as XIValuatorClass entries
    // *as well as* XIScrollClass entries. GDK (and other XI2 clients
    // following the spec) interprets each XIScrollClass as "this axis,
    // already declared as a valuator, additionally has scroll-axis
    // semantics". GDK will assert
    //   `n_valuator < gdk_device_get_n_axes(device)`
    // and abort the device init if the scroll class references a
    // valuator number for which no XIValuatorClass exists — which is
    // exactly the failure that prevented Firefox from starting up.
    //
    // Scroll valuators are RELATIVE (clients compute deltas from
    // successive values) and unbounded (min == max == 0).
    let valuator_scroll_v = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: AXIS_SCROLL_V,
            label: 0,
            min: fp3232(0),
            max: fp3232(0),
            value: fp3232(valuators.scroll_v),
            resolution: 1,
            mode: xi::ValuatorMode::RELATIVE,
        }),
    };

    let valuator_scroll_h = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Valuator(xi::DeviceClassDataValuator {
            number: AXIS_SCROLL_H,
            label: 0,
            min: fp3232(0),
            max: fp3232(0),
            value: fp3232(valuators.scroll_h),
            resolution: 1,
            mode: xi::ValuatorMode::RELATIVE,
        }),
    };

    let scroll_v = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Scroll(xi::DeviceClassDataScroll {
            number: AXIS_SCROLL_V,
            scroll_type: xi::ScrollType::VERTICAL,
            flags: 0u32.into(),
            increment: fp3232(1),
        }),
    };

    let scroll_h = xi::DeviceClass {
        len: 0,
        sourceid: MASTER_POINTER_ID,
        data: xi::DeviceClassData::Scroll(xi::DeviceClassDataScroll {
            number: AXIS_SCROLL_H,
            scroll_type: xi::ScrollType::HORIZONTAL,
            flags: 0u32.into(),
            increment: fp3232(1),
        }),
    };

    // Class order matters for GDK: it walks the list once, and for
    // each XIScrollClass it expects the corresponding XIValuatorClass
    // to have *already* been seen (so `gdk_device_get_n_axes` already
    // covers the referenced axis). Put the four valuators first, then
    // the two scroll classes, then the button class.
    let mut classes = vec![
        button_class,
        valuator_x,
        valuator_y,
        valuator_scroll_v,
        valuator_scroll_h,
        scroll_v,
        scroll_h,
    ];
    fill_class_lengths(&mut classes);

    xi::XIDeviceInfo {
        deviceid: MASTER_POINTER_ID,
        type_: xi::DeviceType::MASTER_POINTER,
        attachment: MASTER_KEYBOARD_ID,
        enabled: true,
        name: b"Virtual core pointer".to_vec(),
        classes,
    }
}

fn build_master_keyboard_info() -> xi::XIDeviceInfo {
    let mut classes = vec![xi::DeviceClass {
        len: 0,
        sourceid: MASTER_KEYBOARD_ID,
        data: xi::DeviceClassData::Key(xi::DeviceClassDataKey { keys: vec![] }),
    }];
    fill_class_lengths(&mut classes);

    xi::XIDeviceInfo {
        deviceid: MASTER_KEYBOARD_ID,
        type_: xi::DeviceType::MASTER_KEYBOARD,
        attachment: MASTER_POINTER_ID,
        enabled: true,
        name: b"Virtual core keyboard".to_vec(),
        classes,
    }
}

/// Walk a list of `DeviceClass` and back-fill each `len` field with the
/// number of 4-byte units the class occupies on the wire (including the
/// 8-byte common header).
fn fill_class_lengths(classes: &mut [xi::DeviceClass]) {
    for c in classes.iter_mut() {
        let mut buf = Vec::new();
        c.serialize_into(&mut buf);
        debug_assert!(buf.len() % 4 == 0, "class wire length not 4-aligned");
        c.len = (buf.len() / 4) as u16;
    }
}

/// Build the byte payload for `XIQueryDevice` containing both master
/// devices.
fn query_device_reply_bytes(
    seq: u16,
    requested: xi::DeviceId,
    valuators: &ValuatorState,
    screen_width: u16,
    screen_height: u16,
) -> Vec<u8> {
    // Resolve which devices the client asked for. 0 = AllDevices, 1 = AllMaster.
    let mp = build_master_pointer_info(valuators, screen_width, screen_height);
    let mk = build_master_keyboard_info();
    let infos: Vec<xi::XIDeviceInfo> = match requested {
        0 | 1 => vec![mp, mk],
        MASTER_POINTER_ID => vec![mp],
        MASTER_KEYBOARD_ID => vec![mk],
        // Unknown — return both as a graceful fallback.
        _ => vec![mp, mk],
    };

    let reply = xi::XIQueryDeviceReply {
        sequence: seq,
        length: 0, // patched by serialize_xi_reply
        infos,
    };
    serialize_xi_reply(&reply)
}

/// Convert our internal pointer mask byte (state from `InputEvent`) into
/// the XI2 `ModifierInfo` struct. We currently track only the X11 core
/// modifier bits in the mask, so the latched/locked fields stay zero.
fn mods_from_state(state: u16) -> xi::ModifierInfo {
    xi::ModifierInfo {
        base: 0,
        latched: 0,
        locked: 0,
        effective: u32::from(state),
    }
}

/// Build an XI2 `RawMotion` event. Raw events have no event window —
/// they're delivered to every client that selected on the root window.
/// Used by toolkits (notably Xt-based xeyes) that drive their cursor
/// tracking purely from XI2 raw events.
///
/// `sequence` should be the sequence number of the most-recently-processed
/// request at the time the event is *sent* to the client. XCB validates
/// event sequence numbers against its outstanding-request tracker, and a
/// stale sequence number causes `Unknown sequence number while processing
/// queue` and a hard abort.
pub fn build_raw_motion_event(sequence: u16) -> Vec<u8> {
    let event = xi::RawMotionEvent {
        response_type: 35, // GenericEvent
        extension: XI_MAJOR_OPCODE,
        sequence,
        length: 0,
        event_type: xi::RAW_MOTION_EVENT,
        deviceid: MASTER_POINTER_ID,
        time: 0,
        detail: 0,
        sourceid: MASTER_POINTER_ID,
        flags: 0u32.into(),
        valuator_mask: vec![],
        axisvalues: vec![],
        axisvalues_raw: vec![],
    };
    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    let length_units = ((buf.len() - 32) / 4) as u32;
    buf[4..8].copy_from_slice(&length_units.to_le_bytes());
    buf
}

/// Marker placed in the per-client `XiState` to request that a synthetic
/// RawMotion event be emitted at the next flush, using whatever the
/// current sequence number is at that time.
#[derive(Default)]
pub struct PendingSynthetic {
    pub raw_motion: bool,
}

/// One axis value to report inside an XI2 device event's valuator data.
#[derive(Clone, Copy, Debug)]
pub struct AxisValue {
    pub axis: u16,
    pub value: i32,
}

/// Build an XI2 `ButtonPressEvent` for the wire (also used for
/// `ButtonRelease` and `Motion` since their structures are identical).
///
/// `axes` is the list of axis updates to embed in the event's valuator
/// data. Pass an empty slice for events that don't carry axis info.
#[allow(clippy::too_many_arguments)]
fn build_xi_pointer_event(
    event_type: u16,
    seq: u16,
    detail: u32,
    deviceid: xi::DeviceId,
    sourceid: xi::DeviceId,
    root: u32,
    event_window: u32,
    child: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    mods: u16,
    button_held_bit: Option<u8>,
    axes: &[AxisValue],
) -> Vec<u8> {
    // Buttons mask is variable-length: enough words to cover the highest
    // button number we ever need (7).
    let mut button_mask = vec![0u32; 1];
    if let Some(b) = button_held_bit {
        let b = b as usize;
        if b < 32 {
            button_mask[0] |= 1 << b;
        }
    }

    // Build the valuator mask + axisvalues. The mask is a bitfield indexed
    // by axis number; for each set bit there's one Fp3232 value in
    // `axisvalues`, in axis-number order (low bit first).
    let mut max_axis = 0u16;
    for ax in axes {
        if ax.axis > max_axis {
            max_axis = ax.axis;
        }
    }
    let mask_words = if axes.is_empty() {
        0
    } else {
        ((max_axis as usize / 32) + 1).max(1)
    };
    let mut valuator_mask = vec![0u32; mask_words];
    for ax in axes {
        let bit = ax.axis as usize;
        valuator_mask[bit / 32] |= 1 << (bit % 32);
    }
    // axisvalues must be in low-axis-first order; sort.
    let mut sorted = axes.to_vec();
    sorted.sort_by_key(|a| a.axis);
    let axisvalues: Vec<xi::Fp3232> = sorted.iter().map(|a| fp3232(a.value)).collect();

    let event = xi::ButtonPressEvent {
        response_type: 35, // GenericEvent
        extension: XI_MAJOR_OPCODE,
        sequence: seq,
        length: 0, // patched after first serialize
        event_type,
        deviceid,
        time: 0, // CurrentTime
        detail,
        root,
        event: event_window,
        child,
        root_x: fp1616(root_x),
        root_y: fp1616(root_y),
        event_x: fp1616(event_x),
        event_y: fp1616(event_y),
        sourceid,
        flags: 0u32.into(),
        mods: mods_from_state(mods),
        group: xi::GroupInfo {
            base: 0,
            latched: 0,
            locked: 0,
            effective: 0,
        },
        button_mask,
        valuator_mask,
        axisvalues,
    };

    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    // Pad to a 4-byte boundary.
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    // X events are 32 bytes for legacy events; for GenericEvent the
    // `length` field counts additional 4-byte units beyond the 32-byte
    // header. Patch it.
    let length_units = ((buf.len() - 32) / 4) as u32;
    buf[4..8].copy_from_slice(&length_units.to_le_bytes());
    buf
}

/// Dispatch a request whose major opcode matches our XInputExtension
/// registration. Returns the wire-format reply (or `Vec::new()` for
/// no-reply requests).
pub fn handle_request(
    data: &[u8],
    seq: u16,
    valuators: &ValuatorState,
    selections: &mut Vec<XiSelection>,
    pending: &mut PendingSynthetic,
    screen_width: u16,
    screen_height: u16,
    root_window: u32,
) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let length_units = u16::from_le_bytes([data[2], data[3]]);
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
            serialize_xi_reply(&reply)
        }

        // ListInputDevices (XI 1.x): return zero devices. Modern apps
        // call this only on init and immediately move to XI2.
        xi::LIST_INPUT_DEVICES_REQUEST => {
            let reply = xi::ListInputDevicesReply {
                xi_reply_type: xi::LIST_INPUT_DEVICES_REQUEST,
                sequence: seq,
                length: 0,
                devices: vec![],
                infos: vec![],
                names: vec![],
            };
            serialize_xi_reply(&reply)
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
            serialize_xi_reply(&reply)
        }

        xi::XI_QUERY_DEVICE_REQUEST => {
            let req =
                xi::XIQueryDeviceRequest::try_parse_request(header, body).unwrap_or_default();
            query_device_reply_bytes(seq, req.deviceid, valuators, screen_width, screen_height)
        }

        xi::XI_SELECT_EVENTS_REQUEST => {
            let req = match xi::XISelectEventsRequest::try_parse_request(header, body) {
                Ok(r) => r,
                Err(e) => {
                    warn!("XISelectEvents parse error: {e:?}");
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

        xi::XI_SET_CLIENT_POINTER_REQUEST => Vec::new(),

        xi::XI_GET_CLIENT_POINTER_REQUEST => {
            let reply = xi::XIGetClientPointerReply {
                sequence: seq,
                length: 0,
                set: true,
                deviceid: MASTER_POINTER_ID,
            };
            serialize_xi_reply(&reply)
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
            serialize_xi_reply(&reply)
        }

        xi::XI_GET_FOCUS_REQUEST => {
            let reply = xi::XIGetFocusReply {
                sequence: seq,
                length: 0,
                focus: 0,
            };
            serialize_xi_reply(&reply)
        }

        xi::XI_SET_FOCUS_REQUEST => Vec::new(),

        xi::XI_GRAB_DEVICE_REQUEST => {
            let reply = xi::XIGrabDeviceReply {
                sequence: seq,
                length: 0,
                status: xproto::GrabStatus::SUCCESS,
            };
            serialize_xi_reply(&reply)
        }
        xi::XI_UNGRAB_DEVICE_REQUEST => Vec::new(),
        xi::XI_ALLOW_EVENTS_REQUEST => Vec::new(),

        xi::XI_PASSIVE_GRAB_DEVICE_REQUEST => {
            let reply = xi::XIPassiveGrabDeviceReply {
                sequence: seq,
                length: 0,
                modifiers: vec![],
            };
            serialize_xi_reply(&reply)
        }
        xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST => Vec::new(),

        xi::XI_LIST_PROPERTIES_REQUEST => {
            let reply = xi::XIListPropertiesReply {
                sequence: seq,
                length: 0,
                properties: vec![],
            };
            serialize_xi_reply(&reply)
        }
        xi::XI_GET_PROPERTY_REQUEST => {
            let reply = xi::XIGetPropertyReply {
                sequence: seq,
                length: 0,
                type_: 0,
                bytes_after: 0,
                num_items: 0,
                items: xi::XIGetPropertyItems::Data8(vec![]),
            };
            serialize_xi_reply(&reply)
        }
        xi::XI_CHANGE_PROPERTY_REQUEST => Vec::new(),
        xi::XI_DELETE_PROPERTY_REQUEST => Vec::new(),

        xi::XI_GET_SELECTED_EVENTS_REQUEST => {
            let reply = xi::XIGetSelectedEventsReply {
                sequence: seq,
                length: 0,
                masks: vec![],
            };
            serialize_xi_reply(&reply)
        }

        xi::XI_BARRIER_RELEASE_POINTER_REQUEST => Vec::new(),
        xi::XI_CHANGE_HIERARCHY_REQUEST => Vec::new(),
        xi::XI_WARP_POINTER_REQUEST => Vec::new(),
        xi::XI_CHANGE_CURSOR_REQUEST => Vec::new(),

        // ---- XI 1.x stubs (rarely-used legacy paths) ---------------------

        // Anything else: silently swallow. The XI 1.x ecosystem is full
        // of obscure requests; returning nothing keeps clients from
        // hanging on missing replies.
        other => {
            debug!("XInput minor opcode {other} unhandled — silently ignoring");
            Vec::new()
        }
    }
}

/// Patch `root_window` into the `root` field of an XIQueryPointer reply
/// produced by `handle_request`. The reply is built before we know which
/// root window the X server is using; this lets the dispatch site fix it
/// up before sending.
pub fn patch_query_pointer_root(buf: &mut [u8], root_window: u32) {
    if buf.len() >= 12 {
        buf[8..12].copy_from_slice(&root_window.to_le_bytes());
    }
}

/// For an InputEvent, build all XI2 GenericEvent bytes that should be
/// delivered alongside the core event:
///
/// - For pointer motion / button 1-3: a regular `XIDeviceEvent`
///   (`XI_Motion` / `XI_ButtonPress` / `XI_ButtonRelease`) plus the
///   corresponding raw event.
///
/// - For scroll-wheel buttons 4-7: an `XI_Motion` event with the scroll
///   valuators updated (vertical or horizontal axis), plus a matching
///   `XI_RawMotion`. This is the path modern XI2 clients (Firefox/GTK 3+)
///   use for scroll — they read the delta on the scroll-class axis from
///   successive motion events and ignore button events for buttons 4-7.
///
/// `chain` is `[top_level, parent, ..., root]`.
///
/// `valuators` is mutated: scroll buttons bump `scroll_v`/`scroll_h`.
pub fn build_xi_events_for(
    valuators: &mut ValuatorState,
    selections: &[XiSelection],
    chain: &[u32],
    seq: u16,
    root_window: u32,
    input: &InputEvent,
) -> Vec<Vec<u8>> {
    // Map scroll-wheel button events into a synthetic motion event with
    // the matching scroll valuator bumped. Buttons 4 (up) and 5 (down)
    // are vertical, 6 (left) and 7 (right) are horizontal.
    let scroll_axis: Option<(u16, i32)> = match *input {
        InputEvent::ButtonPress { button: 4, .. } => {
            valuators.scroll_v -= 1;
            Some((AXIS_SCROLL_V, valuators.scroll_v))
        }
        InputEvent::ButtonPress { button: 5, .. } => {
            valuators.scroll_v += 1;
            Some((AXIS_SCROLL_V, valuators.scroll_v))
        }
        InputEvent::ButtonPress { button: 6, .. } => {
            valuators.scroll_h -= 1;
            Some((AXIS_SCROLL_H, valuators.scroll_h))
        }
        InputEvent::ButtonPress { button: 7, .. } => {
            valuators.scroll_h += 1;
            Some((AXIS_SCROLL_H, valuators.scroll_h))
        }
        _ => None,
    };

    let (device_type, raw_type, detail, x, y, button_bit, mods, axes) = if let Some(
        (axis, value),
    ) = scroll_axis
    {
        let (x, y, mods) = match *input {
            InputEvent::ButtonPress { x, y, state, .. } => (x, y, state),
            _ => (0, 0, 0),
        };
        (
            xi::MOTION_EVENT,
            xi::RAW_MOTION_EVENT,
            0,
            x,
            y,
            None,
            mods,
            vec![AxisValue { axis, value }],
        )
    } else {
        match *input {
            InputEvent::ButtonPress { button, x, y, state } => (
                xi::BUTTON_PRESS_EVENT,
                xi::RAW_BUTTON_PRESS_EVENT,
                button as u32,
                x,
                y,
                Some(button),
                state,
                Vec::new(),
            ),
            InputEvent::ButtonRelease { button, x, y, state } => {
                // Suppress XI events for scroll-button releases — they
                // were translated to motion events on the press side.
                if (4..=7).contains(&button) {
                    return Vec::new();
                }
                (
                    xi::BUTTON_RELEASE_EVENT,
                    xi::RAW_BUTTON_RELEASE_EVENT,
                    button as u32,
                    x,
                    y,
                    Some(button),
                    state,
                    Vec::new(),
                )
            }
            InputEvent::MotionNotify { x, y, state } => (
                xi::MOTION_EVENT,
                xi::RAW_MOTION_EVENT,
                0,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            _ => return Vec::new(),
        }
    };

    let mut out = Vec::new();

    // Regular device event: targets the deepest window in the chain
    // whose selection covers this event type.
    let device_target = chain.iter().copied().find(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0
                    || s.deviceid == 1
                    || s.deviceid == MASTER_POINTER_ID)
                && s.wants(device_type)
        })
    });

    if let Some(event_window) = device_target {
        out.push(build_xi_pointer_event(
            device_type,
            seq,
            detail,
            MASTER_POINTER_ID,
            MASTER_POINTER_ID,
            root_window,
            event_window,
            0,
            x,
            y,
            x,
            y,
            mods,
            button_bit,
            &axes,
        ));
    }

    // Raw event: any window in the chain with a matching raw selection
    // triggers a single delivery (raw events are window-independent).
    let any_raw = chain.iter().any(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0
                    || s.deviceid == 1
                    || s.deviceid == MASTER_POINTER_ID)
                && s.wants(raw_type)
        })
    });

    if any_raw {
        out.push(build_raw_pointer_event(raw_type, seq, detail));
    }

    out
}

/// Build a raw pointer event (`XI_RawMotion`/`XI_RawButtonPress`/
/// `XI_RawButtonRelease`). Raw events have no event window and no
/// coordinates — clients that want a position call XQueryPointer.
pub fn build_raw_pointer_event(event_type: u16, sequence: u16, detail: u32) -> Vec<u8> {
    let event = xi::RawButtonPressEvent {
        response_type: 35,
        extension: XI_MAJOR_OPCODE,
        sequence,
        length: 0,
        event_type,
        deviceid: MASTER_POINTER_ID,
        time: 0,
        detail,
        sourceid: MASTER_POINTER_ID,
        flags: 0u32.into(),
        valuator_mask: vec![],
        axisvalues: vec![],
        axisvalues_raw: vec![],
    };
    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
    let length_units = ((buf.len() - 32) / 4) as u32;
    buf[4..8].copy_from_slice(&length_units.to_le_bytes());
    buf
}

/// Per-client XI state stored on `ClientState`.
#[derive(Default)]
pub struct XiState {
    pub valuators: ValuatorState,
    pub selections: Vec<XiSelection>,
    /// Synthetic events that should be emitted at the next pending-event
    /// flush, using the *current* sequence number rather than a stale one.
    pub pending: PendingSynthetic,
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb_protocol::x11_utils::TryParse;

    /// Build the bytes a real client would send for an XIQueryDevice
    /// request, then verify our handler produces a reply that x11rb can
    /// parse back into a structurally identical XIDeviceInfo set.
    #[test]
    fn query_device_roundtrip() {
        let mut selections = Vec::new();
        let valuators = ValuatorState {
            x: 42,
            y: 99,
            ..Default::default()
        };

        // Build a valid XIQueryDevice request: [opcode, minor, length, deviceid(2), pad(2)]
        let request = vec![
            XI_MAJOR_OPCODE,
            xi::XI_QUERY_DEVICE_REQUEST,
            2,
            0, // length = 2 (8 bytes)
            0,
            0, // deviceid = 0 (AllDevices)
            0,
            0, // pad
        ];

        let bytes = handle_request(
            &request,
            17,
            &valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            1024,
            768,
            0x62,
        );
        assert!(!bytes.is_empty(), "expected a reply");

        let (reply, _) = xi::XIQueryDeviceReply::try_parse(&bytes)
            .expect("XIQueryDeviceReply must round-trip through x11rb");
        assert_eq!(reply.sequence, 17);
        assert_eq!(reply.infos.len(), 2);

        // Master pointer
        let mp = &reply.infos[0];
        assert_eq!(mp.deviceid, MASTER_POINTER_ID);
        assert_eq!(mp.type_, xi::DeviceType::MASTER_POINTER);
        assert_eq!(mp.attachment, MASTER_KEYBOARD_ID);
        assert!(mp.enabled);
        assert_eq!(mp.name, b"Virtual core pointer");
        assert_eq!(mp.classes.len(), 5);

        // The x and y valuators should report our current cursor position.
        let valuators_in_reply: Vec<&xi::DeviceClassDataValuator> = mp
            .classes
            .iter()
            .filter_map(|c| c.data.as_valuator())
            .collect();
        assert_eq!(valuators_in_reply.len(), 2);
        assert_eq!(valuators_in_reply[0].number, 0);
        assert_eq!(valuators_in_reply[0].value.integral, 42);
        assert_eq!(valuators_in_reply[1].number, 1);
        assert_eq!(valuators_in_reply[1].value.integral, 99);

        // Two scroll classes (vertical + horizontal).
        let scrolls: Vec<&xi::DeviceClassDataScroll> = mp
            .classes
            .iter()
            .filter_map(|c| c.data.as_scroll())
            .collect();
        assert_eq!(scrolls.len(), 2);
        assert_eq!(scrolls[0].scroll_type, xi::ScrollType::VERTICAL);
        assert_eq!(scrolls[1].scroll_type, xi::ScrollType::HORIZONTAL);

        // Master keyboard
        let mk = &reply.infos[1];
        assert_eq!(mk.deviceid, MASTER_KEYBOARD_ID);
        assert_eq!(mk.type_, xi::DeviceType::MASTER_KEYBOARD);
        assert_eq!(mk.attachment, MASTER_POINTER_ID);
    }

    #[test]
    fn query_version_round_trips() {
        let mut selections = Vec::new();
        let valuators = ValuatorState::default();
        // [opcode, minor, length, major(2), minor(2)]
        let request = vec![
            XI_MAJOR_OPCODE,
            xi::XI_QUERY_VERSION_REQUEST,
            2,
            0,
            2,
            0,
            3,
            0, // requesting 2.3
        ];
        let bytes = handle_request(
            &request,
            7,
            &valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            1024,
            768,
            0x62,
        );
        let (reply, _) = xi::XIQueryVersionReply::try_parse(&bytes).unwrap();
        assert_eq!(reply.sequence, 7);
        assert_eq!(reply.major_version, 2);
        assert_eq!(reply.minor_version, 3); // we negotiate down to ≤2.4
    }

    #[test]
    fn select_events_records_subscription() {
        let mut selections = Vec::new();
        let valuators = ValuatorState::default();
        // Build XISelectEvents request with a single EventMask asking for
        // XI_Motion (event type 6) and XI_ButtonPress (event type 4) on
        // window 0xdeadbeef for AllMaster (deviceid 1).
        //
        //   1 opcode
        //   1 minor (46)
        //   2 length
        //   4 window
        //   2 num_masks
        //   2 pad
        //   2 deviceid
        //   2 mask_len (in 4-byte units)
        //   4 mask
        let mut req = vec![
            XI_MAJOR_OPCODE,
            xi::XI_SELECT_EVENTS_REQUEST,
            5, // length in 4-byte units = (4 + 4 + 4 + 4) / 4 = 4? + header = 5
            0,
        ];
        req.extend_from_slice(&0xdead_beefu32.to_le_bytes()); // window
        req.extend_from_slice(&1u16.to_le_bytes()); // num_masks
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        req.extend_from_slice(&1u16.to_le_bytes()); // deviceid = AllMasterDevices
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len (1 4-byte word)
        let bits = (1u32 << xi::BUTTON_PRESS_EVENT) | (1u32 << xi::MOTION_EVENT);
        req.extend_from_slice(&bits.to_le_bytes());

        let bytes = handle_request(
            &req,
            0,
            &valuators,
            &mut selections,
            &mut PendingSynthetic::default(),
            1024,
            768,
            0x62,
        );
        assert!(bytes.is_empty(), "XISelectEvents has no reply");
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].window, 0xdead_beef);
        assert_eq!(selections[0].deviceid, 1);
        assert!(selections[0].wants(xi::BUTTON_PRESS_EVENT));
        assert!(selections[0].wants(xi::MOTION_EVENT));
        assert!(!selections[0].wants(xi::KEY_PRESS_EVENT));
    }

    #[test]
    fn xi_pointer_event_parses_back() {
        let bytes = build_xi_pointer_event(
            xi::BUTTON_PRESS_EVENT,
            123,
            5, // detail = scroll-down button
            MASTER_POINTER_ID,
            MASTER_POINTER_ID,
            0x62,
            0x40_0001,
            0,
            10,
            20,
            30,
            40,
            0,
            Some(5),
            &[],
        );
        let (event, _) = xi::ButtonPressEvent::try_parse(&bytes).unwrap();
        assert_eq!(event.response_type, 35);
        assert_eq!(event.extension, XI_MAJOR_OPCODE);
        assert_eq!(event.event_type, xi::BUTTON_PRESS_EVENT);
        assert_eq!(event.deviceid, MASTER_POINTER_ID);
        assert_eq!(event.detail, 5);
        assert_eq!(event.event, 0x40_0001);
        assert_eq!(event.event_x, 30 << 16);
        assert_eq!(event.event_y, 40 << 16);
    }

    #[test]
    fn build_xi_events_for_emits_device_event_on_matching_window() {
        let mut valuators = ValuatorState::default();
        let selections = vec![XiSelection {
            window: 0x40_0001,
            deviceid: 1, // AllMaster
            mask: vec![(1u32 << xi::BUTTON_PRESS_EVENT).into()],
        }];
        let chain = [0x40_0002u32, 0x40_0001, 0x62];
        let input = InputEvent::ButtonPress {
            button: 1,
            x: 100,
            y: 200,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input);
        assert_eq!(events.len(), 1);
        let (event, _) = xi::ButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(event.event, 0x40_0001);
        assert_eq!(event.event_type, xi::BUTTON_PRESS_EVENT);
    }

    #[test]
    fn build_xi_events_for_emits_raw_motion_for_root_subscription() {
        // xeyes-style: subscribe for RawMotion on the root window only.
        let mut valuators = ValuatorState::default();
        let root = 0x62u32;
        let selections = vec![XiSelection {
            window: root,
            deviceid: 1,
            mask: vec![(1u32 << xi::RAW_MOTION_EVENT).into()],
        }];
        let chain = [0x40_0001u32, root];
        let input = InputEvent::MotionNotify {
            x: 50,
            y: 60,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 9, root, &input);
        assert_eq!(events.len(), 1);
        let (raw, _) = xi::RawButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(raw.event_type, xi::RAW_MOTION_EVENT);
        assert_eq!(raw.deviceid, MASTER_POINTER_ID);
    }

    #[test]
    fn scroll_button_press_emits_motion_with_valuator_update() {
        // Firefox/GTK 3+ expect scroll wheel as XI_Motion events with the
        // scroll-class valuator updated, NOT as XI_ButtonPress button 5.
        let mut valuators = ValuatorState::default();
        let win = 0x40_0001u32;
        let selections = vec![XiSelection {
            window: win,
            deviceid: 1,
            mask: vec![(1u32 << xi::MOTION_EVENT).into()],
        }];
        let chain = [win, 0x62u32];
        let input = InputEvent::ButtonPress {
            button: 5, // scroll down
            x: 100,
            y: 200,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input);
        assert_eq!(events.len(), 1);
        let (event, _) = xi::ButtonPressEvent::try_parse(&events[0]).unwrap();
        assert_eq!(event.event_type, xi::MOTION_EVENT);
        assert_eq!(event.event, win);
        // Vertical scroll axis is 2; mask should have bit 2 set.
        assert_eq!(event.valuator_mask, vec![1 << 2]);
        // After one scroll-down notch, scroll_v should be 1.
        assert_eq!(valuators.scroll_v, 1);
        assert_eq!(event.axisvalues.len(), 1);
        assert_eq!(event.axisvalues[0].integral, 1);
    }

    #[test]
    fn scroll_button_release_is_suppressed() {
        let mut valuators = ValuatorState::default();
        let selections = vec![XiSelection {
            window: 0x40_0001,
            deviceid: 1,
            mask: vec![(1u32 << xi::MOTION_EVENT).into()],
        }];
        let chain = [0x40_0001u32, 0x62];
        let input = InputEvent::ButtonRelease {
            button: 5,
            x: 0,
            y: 0,
            state: 0,
        };
        let events =
            build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input);
        assert!(
            events.is_empty(),
            "scroll-button release shouldn't emit a second motion event"
        );
    }

    #[test]
    fn raw_motion_event_is_exactly_32_bytes() {
        let bytes = build_raw_motion_event(0);
        // Verify exact wire layout. We sometimes refer to this in
        // bytes during debugging.
        eprintln!("synthetic raw motion bytes: {bytes:02x?}");
        assert_eq!(
            bytes.len(),
            32,
            "RawMotion with no valuators should be exactly 32 bytes on the wire"
        );
        // GenericEvent header
        assert_eq!(bytes[0], 35); // GenericEvent
        assert_eq!(bytes[1], XI_MAJOR_OPCODE);
        // sequence at bytes 2..4 = 0
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0);
        // length (extra 4-byte units) at bytes 4..8 = 0
        assert_eq!(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 0);
        // event_type at bytes 8..10 = RAW_MOTION_EVENT (17)
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), xi::RAW_MOTION_EVENT);
    }

    #[test]
    fn select_events_on_root_marks_synthetic_raw_motion() {
        let root_window = 0x62u32;
        let mut selections = Vec::new();
        let mut pending = PendingSynthetic::default();
        let valuators = ValuatorState::default();

        // Build XISelectEvents for the root window with XI_RawMotion (17).
        let mut req = vec![XI_MAJOR_OPCODE, xi::XI_SELECT_EVENTS_REQUEST, 5, 0];
        req.extend_from_slice(&root_window.to_le_bytes());
        req.extend_from_slice(&1u16.to_le_bytes()); // num_masks
        req.extend_from_slice(&0u16.to_le_bytes()); // pad
        req.extend_from_slice(&1u16.to_le_bytes()); // deviceid = AllMaster
        req.extend_from_slice(&1u16.to_le_bytes()); // mask_len
        let bits = 1u32 << xi::RAW_MOTION_EVENT;
        req.extend_from_slice(&bits.to_le_bytes());

        let _reply = handle_request(
            &req,
            0,
            &valuators,
            &mut selections,
            &mut pending,
            1024,
            768,
            root_window,
        );
        assert!(pending.raw_motion, "synthetic RawMotion should be marked");

        // The actual wire format must be parseable by x11rb.
        let bytes = build_raw_motion_event(42);
        let (event, _) = xi::RawButtonPressEvent::try_parse(&bytes).unwrap();
        assert_eq!(event.response_type, 35);
        assert_eq!(event.extension, XI_MAJOR_OPCODE);
        assert_eq!(event.event_type, xi::RAW_MOTION_EVENT);
        assert_eq!(event.deviceid, MASTER_POINTER_ID);
        assert_eq!(event.sequence, 42);
    }

    #[test]
    fn build_xi_events_for_returns_empty_without_subscription() {
        let mut valuators = ValuatorState::default();
        let selections = vec![];
        let chain = [0x40_0001u32, 0x62];
        let input = InputEvent::ButtonPress {
            button: 1,
            x: 0,
            y: 0,
            state: 0,
        };
        assert!(
            build_xi_events_for(&mut valuators, &selections, &chain, 0, 0x62, &input)
                .is_empty()
        );
    }
}
