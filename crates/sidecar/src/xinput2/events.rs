use x11rb_protocol::protocol::xinput as xi;

use x11_web_protocol::InputEvent;

use crate::xserver::core::write_u32_bo;

use super::device::mods_from_state;
use super::{
    fp1616, fp3232, AxisValue, ValuatorState, XiSelection, AXIS_SCROLL_H, AXIS_SCROLL_V,
    MASTER_POINTER_ID, XI_MAJOR_OPCODE,
};

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
pub fn build_raw_motion_event(sequence: u16, msb_first: bool) -> Vec<u8> {
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
    super::serialize_xi_reply(&event, msb_first)
}

/// Build an XI2 `ButtonPressEvent` for the wire (also used for
/// `ButtonRelease` and `Motion` since their structures are identical).
///
/// `axes` is the list of axis updates to embed in the event's valuator
/// data. Pass an empty slice for events that don't carry axis info.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_xi_pointer_event(
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
    msb_first: bool,
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
    super::serialize_xi_reply(&event, msb_first)
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
    msb_first: bool,
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

    let (device_type, raw_type, detail, x, y, button_bit, mods, axes) = if let Some((axis, value)) =
        scroll_axis
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
            InputEvent::ButtonPress {
                button,
                x,
                y,
                state,
            } => (
                xi::BUTTON_PRESS_EVENT,
                xi::RAW_BUTTON_PRESS_EVENT,
                button as u32,
                x,
                y,
                Some(button),
                state,
                Vec::new(),
            ),
            InputEvent::ButtonRelease {
                button,
                x,
                y,
                state,
            } => {
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
            // Touch events use the same wire format as button events (XI2 spec §4.5).
            // The detail field carries the touch ID.
            InputEvent::TouchBegin {
                touch_id,
                x,
                y,
                state,
            } => (
                xi::TOUCH_BEGIN_EVENT,
                xi::RAW_TOUCH_BEGIN_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            InputEvent::TouchUpdate {
                touch_id,
                x,
                y,
                state,
            } => (
                xi::TOUCH_UPDATE_EVENT,
                xi::RAW_TOUCH_UPDATE_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            InputEvent::TouchEnd {
                touch_id,
                x,
                y,
                state,
            } => (
                xi::TOUCH_END_EVENT,
                xi::RAW_TOUCH_END_EVENT,
                touch_id,
                x,
                y,
                None,
                state,
                Vec::new(),
            ),
            // Gesture events are handled separately below (different wire format).
            InputEvent::GestureSwipe { .. } | InputEvent::GesturePinch { .. } => {
                return build_gesture_events(input, selections, chain, seq, root_window, msb_first);
            }
            _ => return Vec::new(),
        }
    };

    let mut out = Vec::new();

    // Regular device event: targets the deepest window in the chain
    // whose selection covers this event type.
    let device_target = chain.iter().copied().find(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID)
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
            msb_first,
        ));
    }

    // Raw event: any window in the chain with a matching raw selection
    // triggers a single delivery (raw events are window-independent).
    let any_raw = chain.iter().any(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID)
                && s.wants(raw_type)
        })
    });

    if any_raw {
        out.push(build_raw_pointer_event(raw_type, seq, detail, msb_first));
    }

    out
}

/// Build XI2 gesture events (GestureSwipe/GesturePinch).
/// These use the GestureSwipeBeginEvent/GesturePinchBeginEvent structures.
pub(crate) fn build_gesture_events(
    input: &InputEvent,
    selections: &[XiSelection],
    chain: &[u32],
    seq: u16,
    root_window: u32,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    let (event_type, detail) = match input {
        InputEvent::GestureSwipe { phase, fingers, .. } => {
            let evtype = match phase {
                x11_web_protocol::GesturePhase::Begin => xi::GESTURE_SWIPE_BEGIN_EVENT,
                x11_web_protocol::GesturePhase::Update => xi::GESTURE_SWIPE_UPDATE_EVENT,
                x11_web_protocol::GesturePhase::End => xi::GESTURE_SWIPE_END_EVENT,
            };
            (evtype, *fingers as u32)
        }
        InputEvent::GesturePinch { phase, fingers, .. } => {
            let evtype = match phase {
                x11_web_protocol::GesturePhase::Begin => xi::GESTURE_PINCH_BEGIN_EVENT,
                x11_web_protocol::GesturePhase::Update => xi::GESTURE_PINCH_UPDATE_EVENT,
                x11_web_protocol::GesturePhase::End => xi::GESTURE_PINCH_END_EVENT,
            };
            (evtype, *fingers as u32)
        }
        _ => return Vec::new(),
    };

    // Find target window
    let target = chain.iter().copied().find(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID)
                && s.wants(event_type)
        })
    });

    let Some(event_window) = target else {
        return Vec::new();
    };

    // Build using the pointer event structure (gesture events share the same
    // wire format as button/motion events in XI2).
    let ev = build_xi_pointer_event(
        event_type,
        seq,
        detail,
        MASTER_POINTER_ID,
        MASTER_POINTER_ID,
        root_window,
        event_window,
        0,
        0,
        0,
        0,
        0,
        0,
        None,
        &[],
        msb_first,
    );
    vec![ev]
}

/// Build a raw pointer event (`XI_RawMotion`/`XI_RawButtonPress`/
/// `XI_RawButtonRelease`). Raw events have no event window and no
/// coordinates — clients that want a position call XQueryPointer.
pub fn build_raw_pointer_event(
    event_type: u16,
    sequence: u16,
    detail: u32,
    msb_first: bool,
) -> Vec<u8> {
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
    super::serialize_xi_reply(&event, msb_first)
}

/// Patch `root_window` into the `root` field of an XIQueryPointer reply
/// produced by `handle_request`. The reply is built before we know which
/// root window the X server is using; this lets the dispatch site fix it
/// up before sending.
pub fn patch_query_pointer_root(buf: &mut [u8], root_window: u32, msb_first: bool) {
    if buf.len() >= 12 {
        write_u32_bo(buf, 8, root_window, msb_first);
    }
}
