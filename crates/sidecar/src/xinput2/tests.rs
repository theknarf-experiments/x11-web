use std::collections::HashMap;

use tracing::debug;
use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto;
use x11rb_protocol::x11_utils::TryParse;

use x11_web_protocol::InputEvent;

use super::*;

/// Build the bytes a real client would send for an XIQueryDevice
/// request, then verify our handler produces a reply that x11rb can
/// parse back into a structurally identical XIDeviceInfo set.
#[test]
fn query_device_roundtrip() {
    let mut selections = Vec::new();
    let mut valuators = ValuatorState {
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
        &mut valuators,
        &mut selections,
        &mut PendingSynthetic::default(),
        &mut MASTER_POINTER_ID.clone(),
        &mut HashMap::new(),
        &mut 0x62,
        &mut HashMap::new(),
        &mut Vec::new(),
        &mut false,
        &mut false,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut None,
        1024,
        768,
        0x62,
        false,
        &std::collections::HashMap::new(),
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
    assert_eq!(mp.classes.len(), 9); // button + 4 valuators (x,y,scroll_v,scroll_h) + 2 scrolls + touch + gesture

    // The x and y valuators should report our current cursor position.
    let valuators_in_reply: Vec<&xi::DeviceClassDataValuator> = mp
        .classes
        .iter()
        .filter_map(|c| c.data.as_valuator())
        .collect();
    assert_eq!(valuators_in_reply.len(), 4); // x, y, scroll_v, scroll_h
    assert_eq!(valuators_in_reply[0].number, 0); // x
    assert_eq!(valuators_in_reply[0].value.integral, 42);
    assert_eq!(valuators_in_reply[1].number, 1); // y
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
    let mut valuators = ValuatorState::default();
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
        &mut valuators,
        &mut selections,
        &mut PendingSynthetic::default(),
        &mut MASTER_POINTER_ID.clone(),
        &mut HashMap::new(),
        &mut 0x62,
        &mut HashMap::new(),
        &mut Vec::new(),
        &mut false,
        &mut false,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut None,
        1024,
        768,
        0x62,
        false,
        &std::collections::HashMap::new(),
    );
    let (reply, _) = xi::XIQueryVersionReply::try_parse(&bytes).unwrap();
    assert_eq!(reply.sequence, 7);
    assert_eq!(reply.major_version, 2);
    assert_eq!(reply.minor_version, 3); // we negotiate down to ≤2.4
}

#[test]
fn select_events_records_subscription() {
    let mut selections = Vec::new();
    let mut valuators = ValuatorState::default();
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
        &mut valuators,
        &mut selections,
        &mut PendingSynthetic::default(),
        &mut MASTER_POINTER_ID.clone(),
        &mut HashMap::new(),
        &mut 0x62,
        &mut HashMap::new(),
        &mut Vec::new(),
        &mut false,
        &mut false,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut None,
        1024,
        768,
        0x62,
        false,
        &std::collections::HashMap::new(),
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
        false,
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
    let events = build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
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
    let events = build_xi_events_for(&mut valuators, &selections, &chain, 9, root, &input, false);
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
    let events = build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
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
    let events = build_xi_events_for(&mut valuators, &selections, &chain, 5, 0x62, &input, false);
    assert!(
        events.is_empty(),
        "scroll-button release shouldn't emit a second motion event"
    );
}

#[test]
fn raw_motion_event_is_exactly_32_bytes() {
    let bytes = build_raw_motion_event(0, false);
    // Verify exact wire layout. We sometimes refer to this in
    // bytes during debugging.
    debug!("synthetic raw motion bytes: {bytes:02x?}");
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
    assert_eq!(
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        0
    );
    // event_type at bytes 8..10 = RAW_MOTION_EVENT (17)
    assert_eq!(
        u16::from_le_bytes([bytes[8], bytes[9]]),
        xi::RAW_MOTION_EVENT
    );
}

#[test]
fn select_events_on_root_marks_synthetic_raw_motion() {
    let root_window = 0x62u32;
    let mut selections = Vec::new();
    let mut pending = PendingSynthetic::default();
    let mut valuators = ValuatorState::default();

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
        &mut valuators,
        &mut selections,
        &mut pending,
        &mut MASTER_POINTER_ID.clone(),
        &mut HashMap::new(),
        &mut root_window.clone(),
        &mut HashMap::new(),
        &mut Vec::new(),
        &mut false,
        &mut false,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut None,
        1024,
        768,
        root_window,
        false,
        &std::collections::HashMap::new(),
    );
    assert!(pending.raw_motion, "synthetic RawMotion should be marked");

    // The actual wire format must be parseable by x11rb.
    let bytes = build_raw_motion_event(42, false);
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
        build_xi_events_for(&mut valuators, &selections, &chain, 0, 0x62, &input, false).is_empty()
    );
}

/// Helper to call handle_request with all the XI2 state fields.
fn call_handle_request(
    request: &[u8],
    seq: u16,
    xi_state: &mut XiState,
    focus_window: &mut u32,
) -> Vec<u8> {
    handle_request(
        request,
        seq,
        &mut xi_state.valuators,
        &mut xi_state.selections,
        &mut xi_state.pending,
        &mut xi_state.client_pointer,
        &mut xi_state.device_properties,
        focus_window,
        &mut xi_state.active_grabs,
        &mut xi_state.passive_grabs,
        &mut xi_state.pointer_frozen,
        &mut xi_state.keyboard_frozen,
        &mut xi_state.frozen_pointer_events,
        &mut xi_state.frozen_keyboard_events,
        &mut xi_state.xi1_dont_propagate,
        1024,
        768,
        0x62,
        false,
        &std::collections::HashMap::new(),
    )
}

#[test]
fn xi_grab_device_tracks_active_grab() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // Build XIGrabDevice request: [major, minor, length, body...]
    // body: window(4) + time(4) + cursor(4) + deviceid(2) +
    //   mode(1) + paired_device_mode(1) + owner_events(1) + pad(1) + mask_len(2) + mask(4)
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
    req.extend_from_slice(&0xdead_beefu32.to_le_bytes()); // window
    req.extend_from_slice(&0u32.to_le_bytes()); // time
    req.extend_from_slice(&0u32.to_le_bytes()); // cursor
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    req.push(1); // grab_mode = Async
    req.push(1); // paired_device_mode = Async
    req.push(1); // owner_events = true
    req.push(0); // pad
    req.extend_from_slice(&1u16.to_le_bytes()); // mask_len
    req.extend_from_slice(&0u16.to_le_bytes()); // pad
    let bits = (1u32 << xi::BUTTON_PRESS_EVENT) | (1u32 << xi::MOTION_EVENT);
    req.extend_from_slice(&bits.to_le_bytes()); // mask

    let reply_bytes = call_handle_request(&req, 1, &mut xi_state, &mut focus);
    let (reply, _) = xi::XIGrabDeviceReply::try_parse(&reply_bytes).unwrap();
    assert_eq!(reply.status, xproto::GrabStatus::SUCCESS);
    assert!(xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
    assert_eq!(
        xi_state.active_grabs[&MASTER_POINTER_ID].grab_window,
        0xdead_beef
    );
    assert!(xi_state.active_grabs[&MASTER_POINTER_ID].owner_events);
}

#[test]
fn xi_grab_device_returns_already_grabbed() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // Insert an existing grab.
    xi_state.active_grabs.insert(
        MASTER_POINTER_ID,
        Xi2ActiveGrab {
            deviceid: MASTER_POINTER_ID,
            grab_window: 0x100,
            event_mask: vec![],
            owner_events: false,
            paired_device_mode: 1,
            grab_mode: 1,
        },
    );

    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
    req.extend_from_slice(&0x200u32.to_le_bytes());
    req.extend_from_slice(&0u32.to_le_bytes());
    req.extend_from_slice(&0u32.to_le_bytes());
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes());
    req.push(1);
    req.push(1);
    req.push(0);
    req.push(0);
    req.extend_from_slice(&0u16.to_le_bytes());
    req.extend_from_slice(&0u16.to_le_bytes());

    let reply_bytes = call_handle_request(&req, 2, &mut xi_state, &mut focus);
    let (reply, _) = xi::XIGrabDeviceReply::try_parse(&reply_bytes).unwrap();
    assert_eq!(reply.status, xproto::GrabStatus::ALREADY_GRABBED);
    // Original grab should still be present.
    assert_eq!(xi_state.active_grabs[&MASTER_POINTER_ID].grab_window, 0x100);
}

#[test]
fn xi_ungrab_device_releases_grab() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    xi_state.active_grabs.insert(
        MASTER_POINTER_ID,
        Xi2ActiveGrab {
            deviceid: MASTER_POINTER_ID,
            grab_window: 0x100,
            event_mask: vec![],
            owner_events: false,
            paired_device_mode: 1,
            grab_mode: 0, // Sync
        },
    );
    xi_state.pointer_frozen = true;

    // XIUngrabDevice: [major, minor, length, time(4), deviceid(2), pad(2)]
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_UNGRAB_DEVICE_REQUEST, 3, 0];
    req.extend_from_slice(&0u32.to_le_bytes()); // time
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    req.extend_from_slice(&0u16.to_le_bytes()); // pad

    call_handle_request(&req, 3, &mut xi_state, &mut focus);
    assert!(!xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
    assert!(!xi_state.pointer_frozen);
}

#[test]
fn xi_passive_grab_registers_and_unregisters() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // XIPassiveGrabDevice: time(4) + grab_window(4) + cursor(4) +
    //   detail(4) + deviceid(2) + num_modifiers(2) + mask_len(2) +
    //   grab_type(1) + grab_mode(1) + paired_device_mode(1) +
    //   owner_events(1) + pad(2) + mask(4) + modifiers(4)
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_PASSIVE_GRAB_DEVICE_REQUEST, 10, 0];
    req.extend_from_slice(&0u32.to_le_bytes()); // time
    req.extend_from_slice(&0x400001u32.to_le_bytes()); // grab_window
    req.extend_from_slice(&0u32.to_le_bytes()); // cursor
    req.extend_from_slice(&1u32.to_le_bytes()); // detail = button 1
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    req.extend_from_slice(&1u16.to_le_bytes()); // num_modifiers = 1
    req.extend_from_slice(&1u16.to_le_bytes()); // mask_len = 1
    req.push(1); // grab_type = Button
    req.push(1); // grab_mode = Async
    req.push(1); // paired_device_mode = Async
    req.push(1); // owner_events = true
    req.extend_from_slice(&0u16.to_le_bytes()); // pad
    let bits = 1u32 << xi::BUTTON_PRESS_EVENT;
    req.extend_from_slice(&bits.to_le_bytes()); // mask
    req.extend_from_slice(&0u32.to_le_bytes()); // modifiers = AnyModifier? No, 0 = no modifiers

    let reply_bytes = call_handle_request(&req, 4, &mut xi_state, &mut focus);
    assert!(!reply_bytes.is_empty());
    assert_eq!(xi_state.passive_grabs.len(), 1);
    assert_eq!(xi_state.passive_grabs[0].detail, 1);
    assert_eq!(xi_state.passive_grabs[0].grab_window, 0x400001);
    assert_eq!(xi_state.passive_grabs[0].grab_type, 1);

    // Now ungrab it.
    let mut ungrab_req = vec![XI_MAJOR_OPCODE, xi::XI_PASSIVE_UNGRAB_DEVICE_REQUEST, 5, 0];
    ungrab_req.extend_from_slice(&0x400001u32.to_le_bytes()); // grab_window
    ungrab_req.extend_from_slice(&1u32.to_le_bytes()); // detail = button 1
    ungrab_req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    ungrab_req.extend_from_slice(&1u16.to_le_bytes()); // num_modifiers = 1
    ungrab_req.push(1); // grab_type = Button
    ungrab_req.extend_from_slice(&[0, 0, 0]); // pad
    ungrab_req.extend_from_slice(&0u32.to_le_bytes()); // modifier = 0

    call_handle_request(&ungrab_req, 5, &mut xi_state, &mut focus);
    assert!(xi_state.passive_grabs.is_empty());
}

#[test]
fn xi_allow_events_async_thaws_pointer() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    xi_state.pointer_frozen = true;
    xi_state.frozen_pointer_events.push(vec![1, 2, 3]);

    // XIAllowEvents: [major, minor, length, time(4), deviceid(2), mode(1), pad(1)]
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_ALLOW_EVENTS_REQUEST, 3, 0];
    req.extend_from_slice(&0u32.to_le_bytes()); // time
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    req.push(0); // mode = AsyncDevice
    req.push(0); // pad

    call_handle_request(&req, 6, &mut xi_state, &mut focus);
    assert!(!xi_state.pointer_frozen);
}

#[test]
fn xi_get_focus_returns_actual_focus() {
    let mut xi_state = XiState::default();
    let mut focus = 0xdead_beefu32;

    let req = vec![
        XI_MAJOR_OPCODE,
        xi::XI_GET_FOCUS_REQUEST,
        2,
        0,
        MASTER_KEYBOARD_ID as u8,
        0,
        0,
        0,
    ];

    let reply_bytes = call_handle_request(&req, 7, &mut xi_state, &mut focus);
    let (reply, _) = xi::XIGetFocusReply::try_parse(&reply_bytes).unwrap();
    assert_eq!(reply.focus, 0xdead_beef);
}

#[test]
fn xi_get_selected_events_returns_subscriptions() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // Add a subscription.
    xi_state.selections.push(XiSelection {
        window: 0x400001,
        deviceid: 1,
        mask: vec![(1u32 << xi::BUTTON_PRESS_EVENT).into()],
    });

    // Build XIGetSelectedEvents request: [major, minor, length, window(4)]
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GET_SELECTED_EVENTS_REQUEST, 2, 0];
    req.extend_from_slice(&0x400001u32.to_le_bytes());

    let reply_bytes = call_handle_request(&req, 8, &mut xi_state, &mut focus);
    let (reply, _) = xi::XIGetSelectedEventsReply::try_parse(&reply_bytes).unwrap();
    assert_eq!(reply.masks.len(), 1);
    assert_eq!(reply.masks[0].deviceid, 1);
}

#[test]
fn xi_passive_grab_check_matches() {
    let mut xi_state = XiState::default();
    xi_state.passive_grabs.push(Xi2PassiveGrab {
        deviceid: MASTER_POINTER_ID,
        grab_window: 0x400001,
        detail: 1,         // Button 1
        grab_type: 1,      // Button
        modifiers: 0x8000, // AnyModifier
        event_mask: vec![],
        owner_events: false,
        paired_device_mode: 1,
        grab_mode: 1,
    });

    // Should match: button 1 on window 0x400001 with any modifier.
    let result = xi_state.check_passive_grab(
        MASTER_POINTER_ID,
        1,    // detail = button 1
        1,    // grab_type = Button
        0x04, // modifiers = Control
        &[0x400001, 0x62],
    );
    assert!(result.is_some());

    // Should NOT match: button 2.
    let result = xi_state.check_passive_grab(
        MASTER_POINTER_ID,
        2, // detail = button 2
        1,
        0,
        &[0x400001, 0x62],
    );
    assert!(result.is_none());
}

#[test]
fn xi_sync_grab_freezes_pointer() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // Grab in synchronous mode (grab_mode = 0).
    let mut req = vec![XI_MAJOR_OPCODE, xi::XI_GRAB_DEVICE_REQUEST, 8, 0];
    req.extend_from_slice(&0x400001u32.to_le_bytes()); // window
    req.extend_from_slice(&0u32.to_le_bytes()); // time
    req.extend_from_slice(&0u32.to_le_bytes()); // cursor
    req.extend_from_slice(&MASTER_POINTER_ID.to_le_bytes()); // deviceid
    req.push(0); // grab_mode = Sync
    req.push(1); // paired_device_mode = Async
    req.push(0); // owner_events
    req.push(0); // pad
    req.extend_from_slice(&0u16.to_le_bytes()); // mask_len
    req.extend_from_slice(&0u16.to_le_bytes()); // pad

    call_handle_request(&req, 10, &mut xi_state, &mut focus);
    assert!(xi_state.pointer_frozen);
    assert!(xi_state.active_grabs.contains_key(&MASTER_POINTER_ID));
}

// ---- XI 1.x tests --------------------------------------------------

#[test]
fn list_input_devices_returns_two_devices() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // Build ListInputDevices request: [opcode, minor=2, length=1, pad(4)]
    let req = vec![
        XI_MAJOR_OPCODE,
        xi::LIST_INPUT_DEVICES_REQUEST,
        1,
        0, // length = 1
    ];

    let reply_bytes = call_handle_request(&req, 1, &mut xi_state, &mut focus);
    assert!(
        !reply_bytes.is_empty(),
        "ListInputDevices should return a reply"
    );

    // Parse the reply using x11rb.
    let (reply, _) = xi::ListInputDevicesReply::try_parse(&reply_bytes)
        .expect("ListInputDevicesReply must parse");
    assert_eq!(reply.devices.len(), 2, "should have pointer + keyboard");
    assert_eq!(reply.names.len(), 2, "should have 2 device names");

    // Pointer device
    assert_eq!(reply.devices[0].device_id, MASTER_POINTER_ID as u8);
    assert_eq!(reply.devices[0].device_use, xi::DeviceUse::IS_X_POINTER);
    assert_eq!(reply.devices[0].num_class_info, 2); // button + valuator

    // Keyboard device
    assert_eq!(reply.devices[1].device_id, MASTER_KEYBOARD_ID as u8);
    assert_eq!(reply.devices[1].device_use, xi::DeviceUse::IS_X_KEYBOARD);
    assert_eq!(reply.devices[1].num_class_info, 1); // key

    // Check input classes
    assert_eq!(reply.infos.len(), 3); // 2 pointer classes + 1 keyboard class

    // Pointer button class
    let button_info = reply.infos[0]
        .info
        .as_button()
        .expect("first should be button");
    assert_eq!(button_info.num_buttons, N_POINTER_BUTTONS);

    // Pointer valuator class
    let valuator_info = reply.infos[1]
        .info
        .as_valuator()
        .expect("second should be valuator");
    assert_eq!(valuator_info.axes.len(), 4); // X, Y, ScrollV, ScrollH

    // Keyboard key class
    let key_info = reply.infos[2].info.as_key().expect("third should be key");
    assert_eq!(key_info.min_keycode, MIN_KEYCODE);
    assert_eq!(key_info.max_keycode, MAX_KEYCODE);
}

#[test]
fn open_device_pointer_returns_classes() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // OpenDevice request: [opcode, minor=3, length=1, 0, device_id=2, pad...]
    let req = vec![
        XI_MAJOR_OPCODE,
        3, // OpenDevice
        1,
        0,
        MASTER_POINTER_ID as u8,
        0,
        0,
        0,
    ];

    let reply = call_handle_request(&req, 1, &mut xi_state, &mut focus);
    assert!(
        reply.len() >= 32,
        "OpenDevice should return at least 32 bytes"
    );
    assert_eq!(reply[0], 1); // reply
    assert_eq!(reply[1], 3); // xi_reply_type = OpenDevice
    assert_eq!(reply[8], 2); // num_classes = 2 (button + valuator)
}

#[test]
fn open_device_keyboard_returns_key_class() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    let req = vec![
        XI_MAJOR_OPCODE,
        3, // OpenDevice
        1,
        0,
        MASTER_KEYBOARD_ID as u8,
        0,
        0,
        0,
    ];

    let reply = call_handle_request(&req, 2, &mut xi_state, &mut focus);
    assert!(reply.len() >= 32);
    assert_eq!(reply[8], 1); // num_classes = 1 (key only)
}

#[test]
fn get_device_key_mapping_returns_keysyms() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // GetDeviceKeyMapping: minor=24
    // body: device_id(1) + first_keycode(1) + count(1) + pad(1)
    let req = vec![
        XI_MAJOR_OPCODE,
        24, // GetDeviceKeyMapping
        2,
        0, // length = 2 (8 bytes)
        MASTER_KEYBOARD_ID as u8,
        38, // first_keycode = 38 (key 'a')
        1,  // count = 1
        0,  // pad
    ];

    let reply = call_handle_request(&req, 3, &mut xi_state, &mut focus);
    assert!(
        reply.len() >= 48,
        "should have 32-byte header + 4 keysyms × 4 bytes"
    );
    assert_eq!(reply[1], 4); // keysyms_per_keycode = 4

    // First keysym for keycode 38 should be 'a' (0x61)
    let keysym0 = u32::from_le_bytes([reply[32], reply[33], reply[34], reply[35]]);
    assert_eq!(keysym0, 0x61, "keycode 38 should map to 'a'");
    // Second keysym should be 'A' (0x41)
    let keysym1 = u32::from_le_bytes([reply[36], reply[37], reply[38], reply[39]]);
    assert_eq!(keysym1, 0x41, "shifted keycode 38 should map to 'A'");
}

#[test]
fn query_device_state_pointer_returns_valuators() {
    let mut xi_state = XiState::default();
    xi_state.valuators.x = 100;
    xi_state.valuators.y = 200;
    let mut focus = 0x62u32;

    // QueryDeviceState: minor=30, device_id=pointer
    let req = vec![XI_MAJOR_OPCODE, 30, 1, 0, MASTER_POINTER_ID as u8, 0, 0, 0];

    let reply = call_handle_request(&req, 4, &mut xi_state, &mut focus);
    assert!(reply.len() >= 32);
    assert_eq!(reply[12], 2); // num_classes = 2 (button + valuator)
}

#[test]
fn dont_propagate_list_roundtrip() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    // ChangeDeviceDontPropagateList: add event class 42 to window 0x100
    let mut req = vec![
        XI_MAJOR_OPCODE,
        8, // ChangeDeviceDontPropagateList
        3,
        0, // length
    ];
    req.extend_from_slice(&0x100u32.to_le_bytes()); // window
    req.extend_from_slice(&1u16.to_le_bytes()); // count = 1
    req.push(0); // mode = Add
    req.push(0); // pad
    req.extend_from_slice(&42u32.to_le_bytes()); // event class

    call_handle_request(&req, 5, &mut xi_state, &mut focus);

    // Verify with GetDeviceDontPropagateList
    let mut get_req = vec![
        XI_MAJOR_OPCODE,
        9, // GetDeviceDontPropagateList
        2,
        0, // length
    ];
    get_req.extend_from_slice(&0x100u32.to_le_bytes()); // window

    let reply = call_handle_request(&get_req, 6, &mut xi_state, &mut focus);
    assert!(reply.len() >= 32);
    // count should be 1
    let count = u16::from_le_bytes([reply[12], reply[13]]);
    assert_eq!(count, 1, "should have 1 event class in the list");
}

#[test]
fn get_device_button_mapping_identity() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    let req = vec![
        XI_MAJOR_OPCODE,
        28, // GetDeviceButtonMapping
        1,
        0,
        MASTER_POINTER_ID as u8,
        0,
        0,
        0,
    ];

    let reply = call_handle_request(&req, 7, &mut xi_state, &mut focus);
    assert!(reply.len() >= 32);
    // n_buttons is in reply[1]
    let n_buttons = reply[1];
    assert_eq!(n_buttons, 7, "should have 7 buttons");
    // Check identity mapping
    for i in 0..n_buttons as usize {
        assert_eq!(
            reply[32 + i],
            (i + 1) as u8,
            "button {i} should map to {}",
            i + 1
        );
    }
}

#[test]
fn get_device_modifier_mapping_has_modifiers() {
    let mut xi_state = XiState::default();
    let mut focus = 0x62u32;

    let req = vec![
        XI_MAJOR_OPCODE,
        26, // GetDeviceModifierMapping
        1,
        0,
        MASTER_KEYBOARD_ID as u8,
        0,
        0,
        0,
    ];

    let reply = call_handle_request(&req, 8, &mut xi_state, &mut focus);
    assert!(reply.len() >= 32);
    let keycodes_per_mod = reply[1];
    assert_eq!(keycodes_per_mod, 2, "should have 2 keycodes per modifier");
    // Shift modifier should map to keycodes 50 and 62
    assert_eq!(reply[32], 50, "Shift_L");
    assert_eq!(reply[33], 62, "Shift_R");
    // Control should map to 37 and 105
    assert_eq!(reply[36], 37, "Control_L");
    assert_eq!(reply[37], 105, "Control_R");
}
