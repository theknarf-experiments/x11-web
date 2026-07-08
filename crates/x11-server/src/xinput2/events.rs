use x11rb_protocol::protocol::xinput as xi;
use x11rb_protocol::protocol::xproto::GE_GENERIC_EVENT;

use x11_web_protocol::InputEvent;

use crate::xserver::core::write_u32_bo;

use super::device::mods_from_state;
use super::{
    fp1616, fp3232, AxisValue, ValuatorState, Xi2PassiveGrab, XiSelection, AXIS_SCROLL_H,
    AXIS_SCROLL_V, MASTER_KEYBOARD_ID, MASTER_POINTER_ID, XI_MAJOR_OPCODE,
};

/// XI2 grab_type values for `Xi2PassiveGrab.grab_type`.
const XI_GRAB_TYPE_BUTTON: u8 = 1;
const XI_GRAB_TYPE_KEYCODE: u8 = 2;

/// XI2 wildcard for "any modifier" in `Xi2PassiveGrab.modifiers`.
const XI_ANY_MODIFIER: u32 = 1 << 31;
/// Core X11 wildcard for "any modifier" — toolkits sometimes pass this
/// (1 << 15) into XIPassiveGrabDevice instead of the proper 1 << 31.
const CORE_ANY_MODIFIER: u32 = 1 << 15;

/// Test whether an XI2 event mask vector covers a given event type.
/// Mirror of `XiSelection::wants` but for raw XIEventMask slices.
fn mask_wants(mask: &[xi::XIEventMask], evtype: u16) -> bool {
    let bit = evtype as u32;
    let word = (bit / 32) as usize;
    let in_word = bit % 32;
    mask.get(word)
        .map(|w| (u32::from(*w) >> in_word) & 1 != 0)
        .unwrap_or(false)
}

/// Find a passive XI2 grab that matches the current event. Returns a
/// reference to the grab so the caller can pull the `grab_window` and
/// `event_mask` out for delivery.
///
/// A passive grab matches when:
/// - The grab type matches (Button=1, Keycode=2).
/// - The device matches (or grab is for `XIAllDevices` / `XIAllMasterDevices`).
/// - The detail (keycode/button) matches (or grab uses `0` = AnyKey/AnyButton).
/// - The modifier state matches (or grab uses `XIAnyModifier`).
/// - The grab window is in the propagation chain — either the event
///   window itself or one of its ancestors. Without this, a grab on one
///   toplevel would fire on every other toplevel's events.
fn find_passive_grab<'a>(
    passive_grabs: &'a [Xi2PassiveGrab],
    grab_type: u8,
    deviceid: xi::DeviceId,
    detail: u32,
    modifiers: u16,
    chain: &[u32],
) -> Option<&'a Xi2PassiveGrab> {
    passive_grabs.iter().find(|g| {
        g.grab_type == grab_type
            && (g.deviceid == 0 || g.deviceid == 1 || g.deviceid == deviceid)
            && (g.detail == 0 || g.detail == detail)
            && (g.modifiers == XI_ANY_MODIFIER
                || g.modifiers == CORE_ANY_MODIFIER
                || (g.modifiers as u16) == modifiers)
            && chain.contains(&g.grab_window)
    })
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
pub fn build_raw_motion_event(sequence: u16, msb_first: bool) -> Vec<u8> {
    let event = xi::RawMotionEvent {
        response_type: GE_GENERIC_EVENT,
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
    time: u32,
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
        response_type: GE_GENERIC_EVENT,
        extension: XI_MAJOR_OPCODE,
        sequence: seq,
        length: 0, // patched after first serialize
        event_type,
        deviceid,
        time,
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
    passive_grabs: &[Xi2PassiveGrab],
    chain: &[u32],
    seq: u16,
    time: u32,
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
            // Keyboard events use a different code path with the master
            // keyboard device IDs and the XI2 KeyPressEvent struct.
            InputEvent::KeyPress { keycode, state } => {
                return build_xi_key_events(
                    xi::KEY_PRESS_EVENT,
                    xi::RAW_KEY_PRESS_EVENT,
                    keycode,
                    state,
                    selections,
                    passive_grabs,
                    chain,
                    seq,
                    time,
                    root_window,
                    msb_first,
                );
            }
            InputEvent::KeyRelease { keycode, state } => {
                return build_xi_key_events(
                    xi::KEY_RELEASE_EVENT,
                    xi::RAW_KEY_RELEASE_EVENT,
                    keycode,
                    state,
                    selections,
                    passive_grabs,
                    chain,
                    seq,
                    time,
                    root_window,
                    msb_first,
                );
            }
            _ => return Vec::new(),
        }
    };

    let mut out = Vec::new();

    // Passive button grabs: when set on `grab_window` for a specific
    // button/modifier combo, button events redirect to the grab window
    // with the grab's event mask. Toolkits use this for click-to-focus
    // and drag-and-drop activation, so consult before the per-window
    // selection chain.
    let grab_target = if matches!(
        *input,
        InputEvent::ButtonPress { .. } | InputEvent::ButtonRelease { .. }
    ) {
        find_passive_grab(
            passive_grabs,
            XI_GRAB_TYPE_BUTTON,
            MASTER_POINTER_ID,
            detail,
            mods,
            chain,
        )
    } else {
        None
    };

    // Regular device event: passive grab takes priority. Otherwise
    // target the deepest window in the chain whose selection covers
    // this event type.
    let device_target = if let Some(grab) = grab_target {
        if mask_wants(&grab.event_mask, device_type) {
            Some(grab.grab_window)
        } else {
            None
        }
    } else {
        chain.iter().copied().find(|w| {
            selections.iter().any(|s| {
                s.window == *w
                    && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID)
                    && s.wants(device_type)
            })
        })
    };

    if let Some(event_window) = device_target {
        out.push(build_xi_pointer_event(
            device_type,
            seq,
            time,
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
        0, // gesture events: no server timestamp plumbed here yet
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
        response_type: GE_GENERIC_EVENT,
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

/// Build an XI2 Enter (7) / Leave (8) crossing event with real pointer
/// coordinates for delivery to a single window.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_xi_crossing_event(
    event_type: u16,
    seq: u16,
    time: u32,
    root_window: u32,
    event_window: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    msb_first: bool,
) -> Vec<u8> {
    let event = xi::EnterEvent {
        response_type: GE_GENERIC_EVENT,
        extension: XI_MAJOR_OPCODE,
        sequence: seq,
        length: 0,
        event_type,
        deviceid: MASTER_POINTER_ID,
        time,
        sourceid: MASTER_POINTER_ID,
        mode: xi::NotifyMode::NORMAL,
        detail: xi::NotifyDetail::NONLINEAR,
        root: root_window,
        event: event_window,
        child: 0,
        root_x: fp1616(root_x),
        root_y: fp1616(root_y),
        event_x: fp1616(event_x),
        event_y: fp1616(event_y),
        same_screen: true,
        focus: false,
        mods: mods_from_state(0),
        group: xi::GroupInfo {
            base: 0,
            latched: 0,
            locked: 0,
            effective: 0,
        },
        buttons: vec![0],
    };
    super::serialize_xi_reply(&event, msb_first)
}

/// XI2 Enter/Leave events for selections along the leave/enter window
/// chains. GTK3's XI2 device manager tracks which window contains the
/// pointer *exclusively* through these — without an XI_Enter, GDK
/// never considers the pointer inside the window and pointer events
/// are quietly dropped before reaching widgets (the "Firefox renders
/// but ignores every click" failure mode).
#[allow(clippy::too_many_arguments)]
pub fn build_xi_crossing_events_for(
    selections: &[XiSelection],
    leave_chain: &[u32],
    enter_chain: &[u32],
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    seq: u16,
    root_window: u32,
    time: u32,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let device_matches =
        |s: &XiSelection| s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_POINTER_ID;

    for &window in leave_chain {
        if enter_chain.contains(&window) {
            // Still inside this ancestor — no Leave for it.
            continue;
        }
        let wants = selections
            .iter()
            .any(|s| s.window == window && device_matches(s) && s.wants(xi::LEAVE_EVENT));
        if wants {
            out.push(build_xi_crossing_event(
                xi::LEAVE_EVENT,
                seq,
                time,
                root_window,
                window,
                root_x,
                root_y,
                event_x,
                event_y,
                msb_first,
            ));
        }
    }
    for &window in enter_chain {
        if leave_chain.contains(&window) {
            continue;
        }
        let wants = selections
            .iter()
            .any(|s| s.window == window && device_matches(s) && s.wants(xi::ENTER_EVENT));
        if wants {
            out.push(build_xi_crossing_event(
                xi::ENTER_EVENT,
                seq,
                time,
                root_window,
                window,
                root_x,
                root_y,
                event_x,
                event_y,
                msb_first,
            ));
        }
    }
    out
}

/// Build an XI2 FocusIn / FocusOut event for delivery to a single window.
/// The wire format is shared with EnterEvent (x11rb aliases `FocusInEvent`
/// to `EnterEvent`).
///
/// `event_type` is `xi::FOCUS_IN_EVENT` (9) or `xi::FOCUS_OUT_EVENT` (10).
/// `detail` follows the X11 focus-event detail codes (0=Ancestor,
/// 1=Virtual, 2=Inferior, 3=Nonlinear, 4=NonlinearVirtual). `mode` is
/// the NotifyMode (0=Normal, 1=Grab, 2=Ungrab, 3=WhileGrabbed).
pub(crate) fn build_xi_focus_event(
    event_type: u16,
    detail: u8,
    mode: u8,
    seq: u16,
    root_window: u32,
    event_window: u32,
    msb_first: bool,
) -> Vec<u8> {
    let event = xi::EnterEvent {
        response_type: GE_GENERIC_EVENT,
        extension: XI_MAJOR_OPCODE,
        sequence: seq,
        length: 0,
        event_type,
        deviceid: MASTER_KEYBOARD_ID,
        time: 0,
        sourceid: MASTER_KEYBOARD_ID,
        mode: xi::NotifyMode::from(mode),
        detail: xi::NotifyDetail::from(detail),
        root: root_window,
        event: event_window,
        child: 0,
        root_x: fp1616(0),
        root_y: fp1616(0),
        event_x: fp1616(0),
        event_y: fp1616(0),
        same_screen: true,
        focus: true,
        mods: mods_from_state(0),
        group: xi::GroupInfo {
            base: 0,
            latched: 0,
            locked: 0,
            effective: 0,
        },
        buttons: vec![0],
    };
    super::serialize_xi_reply(&event, msb_first)
}

/// Compute every (window, event_type) pair in this client's XI selections
/// that wants an XI focus event when focus changes from `prev` to `next`.
///
/// We deliberately stay simple: any window in `next`'s subtree (or its
/// ancestor chain up to root) that selected `XI_FocusIn` gets a FocusIn,
/// and the symmetric set on `prev` gets a FocusOut. Detail codes are set
/// to `Nonlinear` (3) — toolkits don't usually act on the precise detail,
/// they just need *some* FocusIn to mark the device focused so XI2 keys
/// flow into widgets.
pub fn build_xi_focus_events_for(
    selections: &[XiSelection],
    windows_under_prev: &[u32],
    windows_under_next: &[u32],
    seq: u16,
    root_window: u32,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();

    for &window in windows_under_prev {
        let wants = selections.iter().any(|s| {
            s.window == window
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_KEYBOARD_ID)
                && s.wants(xi::FOCUS_OUT_EVENT)
        });
        if wants {
            out.push(build_xi_focus_event(
                xi::FOCUS_OUT_EVENT,
                3, // Nonlinear
                0, // Normal
                seq,
                root_window,
                window,
                msb_first,
            ));
        }
    }
    for &window in windows_under_next {
        let wants = selections.iter().any(|s| {
            s.window == window
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_KEYBOARD_ID)
                && s.wants(xi::FOCUS_IN_EVENT)
        });
        if wants {
            out.push(build_xi_focus_event(
                xi::FOCUS_IN_EVENT,
                3, // Nonlinear
                0, // Normal
                seq,
                root_window,
                window,
                msb_first,
            ));
        }
    }

    out
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

/// Build XI2 KeyPress / KeyRelease events. Selections must match the
/// master *keyboard* device for these. Without this, GTK3 / Firefox /
/// any GDK-3 client that subscribed via `XISelectEvents(KEY_PRESS)`
/// silently drops every keystroke.
///
/// Passive grabs registered via `XIPassiveGrabDevice` take priority
/// over per-window selections — that's how GTK3 receives accelerator
/// keystrokes (it grabs Down / Up / Tab / Return on its toplevel and
/// expects every matching keystroke to land on that window with the
/// grab's event mask, regardless of where the focus subtree's selections
/// were set).
#[allow(clippy::too_many_arguments)]
pub fn build_xi_key_events(
    event_type: u16,
    raw_type: u16,
    keycode: u32,
    mods_state: u16,
    selections: &[XiSelection],
    passive_grabs: &[Xi2PassiveGrab],
    chain: &[u32],
    seq: u16,
    time: u32,
    root_window: u32,
    msb_first: bool,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();

    let grab = find_passive_grab(
        passive_grabs,
        XI_GRAB_TYPE_KEYCODE,
        MASTER_KEYBOARD_ID,
        keycode,
        mods_state,
        chain,
    );

    let device_target = if let Some(g) = grab {
        if mask_wants(&g.event_mask, event_type) {
            Some(g.grab_window)
        } else {
            None
        }
    } else {
        chain.iter().copied().find(|w| {
            selections.iter().any(|s| {
                s.window == *w
                    && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_KEYBOARD_ID)
                    && s.wants(event_type)
            })
        })
    };

    if let Some(event_window) = device_target {
        let event = xi::KeyPressEvent {
            response_type: GE_GENERIC_EVENT,
            extension: XI_MAJOR_OPCODE,
            sequence: seq,
            length: 0,
            event_type,
            deviceid: MASTER_KEYBOARD_ID,
            time,
            detail: keycode,
            root: root_window,
            event: event_window,
            child: 0,
            root_x: fp1616(0),
            root_y: fp1616(0),
            event_x: fp1616(0),
            event_y: fp1616(0),
            sourceid: MASTER_KEYBOARD_ID,
            flags: 0u32.into(),
            mods: mods_from_state(mods_state),
            group: xi::GroupInfo {
                base: 0,
                latched: 0,
                locked: 0,
                effective: 0,
            },
            button_mask: vec![],
            valuator_mask: vec![],
            axisvalues: vec![],
        };
        out.push(super::serialize_xi_reply(&event, msb_first));
    }

    let any_raw = chain.iter().any(|w| {
        selections.iter().any(|s| {
            s.window == *w
                && (s.deviceid == 0 || s.deviceid == 1 || s.deviceid == MASTER_KEYBOARD_ID)
                && s.wants(raw_type)
        })
    });

    if any_raw {
        let raw = xi::RawKeyPressEvent {
            response_type: GE_GENERIC_EVENT,
            extension: XI_MAJOR_OPCODE,
            sequence: seq,
            length: 0,
            event_type: raw_type,
            deviceid: MASTER_KEYBOARD_ID,
            time: 0,
            detail: keycode,
            sourceid: MASTER_KEYBOARD_ID,
            flags: 0u32.into(),
            valuator_mask: vec![],
            axisvalues: vec![],
            axisvalues_raw: vec![],
        };
        out.push(super::serialize_xi_reply(&raw, msb_first));
    }

    out
}
