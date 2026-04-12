use std::collections::HashMap;
use x11_web_protocol::{DisplayUpdate, InputEvent};
use super::client::ClientState;
use super::core::*;
use super::types::*;

/// Crossing event mode constants.
pub(crate) const CROSSING_MODE_NORMAL: u8 = 0;
pub(crate) const CROSSING_MODE_GRAB: u8 = 1;
pub(crate) const CROSSING_MODE_UNGRAB: u8 = 2;

/// Check pointer barriers and clamp motion if a barrier is crossed.
/// Returns the adjusted (x, y) position after barrier enforcement.
pub(crate) fn enforce_barriers(
    barriers: &HashMap<u32, PointerBarrier>,
    old_x: i16, old_y: i16,
    new_x: i16, new_y: i16,
) -> (i16, i16) {
    let mut final_x = new_x;
    let mut final_y = new_y;

    for barrier in barriers.values() {
        // Barrier direction bits:
        // bit 0 = PositiveX (blocks motion from left to right)
        // bit 1 = PositiveY (blocks motion from top to bottom)
        // bit 2 = NegativeX (blocks motion from right to left)
        // bit 3 = NegativeY (blocks motion from bottom to top)
        // If directions == 0, blocks all directions.

        if barrier.x1 == barrier.x2 {
            // Vertical barrier (blocks horizontal motion)
            let bx = barrier.x1;
            let by_min = barrier.y1.min(barrier.y2);
            let by_max = barrier.y1.max(barrier.y2);

            // Check if motion crosses this vertical line within the y range
            if final_y >= by_min && final_y <= by_max {
                let crosses_right = old_x <= bx && final_x > bx;
                let crosses_left = old_x >= bx && final_x < bx;

                let blocked = if barrier.directions == 0 {
                    crosses_right || crosses_left
                } else {
                    (crosses_right && (barrier.directions & 1) != 0)
                        || (crosses_left && (barrier.directions & 4) != 0)
                };

                if blocked {
                    final_x = bx;
                }
            }
        } else if barrier.y1 == barrier.y2 {
            // Horizontal barrier (blocks vertical motion)
            let by = barrier.y1;
            let bx_min = barrier.x1.min(barrier.x2);
            let bx_max = barrier.x1.max(barrier.x2);

            if final_x >= bx_min && final_x <= bx_max {
                let crosses_down = old_y <= by && final_y > by;
                let crosses_up = old_y >= by && final_y < by;

                let blocked = if barrier.directions == 0 {
                    crosses_down || crosses_up
                } else {
                    (crosses_down && (barrier.directions & 2) != 0)
                        || (crosses_up && (barrier.directions & 8) != 0)
                };

                if blocked {
                    final_y = by;
                }
            }
        }
    }

    (final_x, final_y)
}

/// Walk the window tree to find the correct event target per X11 spec Section 7.
///
/// Per the spec, device events (KeyPress, KeyRelease, ButtonPress, ButtonRelease,
/// MotionNotify) are delivered as follows:
/// 1. Find the deepest mapped window containing the pointer position (the "source").
/// 2. From the source, walk UP through ancestors. The first window that selects
///    for the event (via event_mask) receives it.
/// 3. If any ancestor's `do_not_propagate_mask` includes the event type, stop
///    propagation — the event is discarded.
/// 4. If no window in the chain selects for the event, the event is discarded.
pub(crate) fn find_event_subwindow(
    windows: &HashMap<u32, WindowState>,
    parent: u32,
    rel_x: i16,
    rel_y: i16,
    required_mask: u32,
) -> (u32, i16, i16) {
    // Step 1: Descend to find the deepest mapped window containing the point.
    // This ignores event masks — we just want the geometry hit.
    let (source_window, source_rx, source_ry) = find_deepest_window(windows, parent, rel_x, rel_y);

    // Step 2: From the source, walk UP through ancestors looking for a window
    // that selects for this event. Respect do_not_propagate_mask.
    let mut current = source_window;
    // Track accumulated offsets for coordinate translation as we walk up.
    let mut accum_x = source_rx;
    let mut accum_y = source_ry;
    for _ in 0..64 {
        if let Some(w) = windows.get(&current) {
            // Does this window select for the event?
            if w.event_mask & required_mask != 0 {
                return (current, accum_x, accum_y);
            }
            // Does this window block propagation for this event?
            if w.do_not_propagate_mask & required_mask != 0 {
                // Event is consumed/blocked — return the source with no delivery.
                // We still return the source window so crossing events etc. work,
                // but with no event mask match the caller will see it didn't match.
                // Actually, per spec we should discard. Return parent as fallback
                // with empty event (caller checks if event_mask matched).
                return (source_window, source_rx, source_ry);
            }
            // Move up to parent
            let p = w.parent;
            if p == 0 || p == current {
                break; // Reached root or cycle
            }
            // Translate coordinates back to parent-relative
            accum_x += w.x;
            accum_y += w.y;
            current = p;
        } else {
            break;
        }
    }

    // No window in the chain selected for this event. Return the source window
    // anyway — the caller should check if the target window actually has the mask.
    (source_window, source_rx, source_ry)
}

/// Find the deepest mapped window containing the given point, ignoring event masks.
/// Used as the first step of X11 event targeting per spec Section 7.
fn find_deepest_window(
    windows: &HashMap<u32, WindowState>,
    parent: u32,
    rel_x: i16,
    rel_y: i16,
) -> (u32, i16, i16) {
    fn descend(
        windows: &HashMap<u32, WindowState>,
        parent: u32,
        rel_x: i16,
        rel_y: i16,
        result: &mut (u32, i16, i16),
    ) {
        // This window is the deepest so far
        *result = (parent, rel_x, rel_y);

        // Check children in stacking order (top to bottom for hit testing).
        // Use children_order if available (it's bottom-to-top, so reverse).
        if let Some(pw) = windows.get(&parent) {
            let children_order = &pw.children_order;
            for &child_id in children_order.iter().rev() {
                if let Some(child) = windows.get(&child_id) {
                    if !child.mapped { continue; }
                    let cx = child.x;
                    let cy = child.y;
                    let cw = child.width as i16;
                    let ch = child.height as i16;
                    if rel_x >= cx && rel_x < cx + cw && rel_y >= cy && rel_y < cy + ch {
                        // Check input shape if present
                        if let Some(ref shape) = child.input_shape {
                            if !point_in_shape(shape, rel_x - cx, rel_y - cy) {
                                continue;
                            }
                        }
                        descend(windows, child_id, rel_x - cx, rel_y - cy, result);
                        return; // First (topmost) hit wins
                    }
                }
            }
        } else {
            // Fallback: scan all children (shouldn't happen for well-formed trees)
            let children: Vec<&WindowState> = windows
                .values()
                .filter(|w| w.parent == parent && w.mapped)
                .collect();
            for child in children {
                let cx = child.x;
                let cy = child.y;
                let cw = child.width as i16;
                let ch = child.height as i16;
                if rel_x >= cx && rel_x < cx + cw && rel_y >= cy && rel_y < cy + ch {
                    descend(windows, child.id, rel_x - cx, rel_y - cy, result);
                    return;
                }
            }
        }
    }

    let mut result = (parent, rel_x, rel_y);
    descend(windows, parent, rel_x, rel_y, &mut result);
    result
}

/// Propagate a keyboard event up the window tree from the focus window.
/// Returns the window that should receive the event.
/// Per X11 spec: keyboard events propagate from focus window to ancestors
/// until a window selecting for the event is found, or do_not_propagate blocks it.
pub(crate) fn propagate_keyboard_event(
    windows: &HashMap<u32, WindowState>,
    focus_window: u32,
    required_mask: u32,
) -> u32 {
    let mut current = focus_window;
    for _ in 0..64 {
        if let Some(w) = windows.get(&current) {
            if w.event_mask & required_mask != 0 {
                return current;
            }
            if w.do_not_propagate_mask & required_mask != 0 {
                return focus_window; // Blocked; caller will see no mask match
            }
            let p = w.parent;
            if p == 0 || p == current {
                break;
            }
            current = p;
        } else {
            break;
        }
    }
    focus_window
}

pub(crate) fn build_crossing_events(state: &mut ClientState, new_window: u32, x: i16, y: i16, event_x: i16, event_y: i16) -> Vec<u8> {
    build_crossing_events_with_mode(state, new_window, x, y, event_x, event_y, CROSSING_MODE_NORMAL)
}

/// Crossing event detail constants per X11 spec Section 7.4.
const DETAIL_ANCESTOR: u8 = 0;
const DETAIL_VIRTUAL: u8 = 1;
const DETAIL_INFERIOR: u8 = 2;
const DETAIL_NONLINEAR: u8 = 3;
const DETAIL_NONLINEAR_VIRTUAL: u8 = 4;

/// Build the ancestor path from `window` up to (and including) the root.
/// Returns [window, parent, grandparent, ..., root].
fn ancestor_path(windows: &HashMap<u32, WindowState>, window: u32, root: u32) -> Vec<u32> {
    let mut path = vec![window];
    let mut cur = window;
    for _ in 0..64 {
        if cur == root || cur == 0 { break; }
        if let Some(w) = windows.get(&cur) {
            let p = w.parent;
            if p == 0 || p == cur { break; }
            path.push(p);
            cur = p;
        } else {
            break;
        }
    }
    path
}

/// Compute window-local coordinates for a given window by walking up to root.
fn window_local_coords(windows: &HashMap<u32, WindowState>, window: u32, root: u32, abs_x: i16, abs_y: i16) -> (i16, i16) {
    let mut ox = 0i32;
    let mut oy = 0i32;
    let mut cur = window;
    for _ in 0..128 {
        if cur == root || cur == 0 { break; }
        if let Some(w) = windows.get(&cur) {
            ox += w.x as i32;
            oy += w.y as i32;
            cur = w.parent;
        } else { break; }
    }
    ((abs_x as i32 - ox) as i16, (abs_y as i32 - oy) as i16)
}

/// Build a single crossing event with proper child field.
fn make_crossing_event(
    windows: &HashMap<u32, WindowState>,
    event_code: u8, detail: u8, mode: u8,
    seq: u16, timestamp: u32, root_window: u32,
    event_window: u32, abs_x: i16, abs_y: i16,
    bo: bool, focus_window: u32,
) -> [u8; 32] {
    let (ev_x, ev_y) = window_local_coords(windows, event_window, root_window, abs_x, abs_y);

    // Per X11 spec: child field is the child of event_window that contains the
    // pointer, or None (0) if the pointer is directly in event_window.
    let child = if let Some(win) = windows.get(&event_window) {
        win.children_order.iter().rev().copied().find(|&cid| {
            if let Some(c) = windows.get(&cid) {
                if !c.mapped { return false; }
                let cx = c.x;
                let cy = c.y;
                ev_x >= cx && ev_x < cx + c.width as i16
                    && ev_y >= cy && ev_y < cy + c.height as i16
            } else {
                false
            }
        }).unwrap_or(0)
    } else {
        0
    };

    // Determine focus bit: true if event_window is an ancestor of (or is) the focus window.
    let has_focus = {
        let mut cur = focus_window;
        let mut found = false;
        for _ in 0..64 {
            if cur == event_window { found = true; break; }
            if cur == 0 { break; }
            if let Some(w) = windows.get(&cur) {
                cur = w.parent;
            } else { break; }
        }
        found
    };

    let mut event = [0u8; 32];
    event[0] = event_code;
    event[1] = detail;
    write_u16_bo(&mut event, 2, seq, bo);
    write_u32_bo(&mut event, 4, timestamp, bo);
    write_u32_bo(&mut event, 8, root_window, bo);
    write_u32_bo(&mut event, 12, event_window, bo);
    write_u32_bo(&mut event, 16, child, bo);
    write_i16_bo(&mut event, 20, abs_x, bo);  // root_x
    write_i16_bo(&mut event, 22, abs_y, bo);  // root_y
    write_i16_bo(&mut event, 24, ev_x, bo);   // event_x
    write_i16_bo(&mut event, 26, ev_y, bo);   // event_y
    event[30] = mode;
    event[31] = 0x01 | if has_focus { 0x01 } else { 0x00 }; // same_screen=1, focus
    event
}

/// Emit a crossing event if the window selects for it.
fn emit_crossing(
    windows: &HashMap<u32, WindowState>,
    events: &mut Vec<u8>,
    event_code: u8, detail: u8, mode: u8,
    seq: u16, timestamp: u32, root_window: u32,
    window: u32, abs_x: i16, abs_y: i16,
    bo: bool, focus_window: u32,
) {
    let required_mask = if event_code == ENTER_NOTIFY_EVENT { ENTER_WINDOW_MASK } else { LEAVE_WINDOW_MASK };
    if let Some(win) = windows.get(&window) {
        if win.event_mask & required_mask != 0 {
            let ev = make_crossing_event(windows, event_code, detail, mode, seq, timestamp, root_window, window, abs_x, abs_y, bo, focus_window);
            events.extend_from_slice(&ev);
        }
    }
}

pub(crate) fn build_crossing_events_with_mode(state: &mut ClientState, new_window: u32, x: i16, y: i16, _event_x: i16, _event_y: i16, mode: u8) -> Vec<u8> {
    let mut events = Vec::new();
    let old_window = state.last_entered_window;
    if old_window == new_window {
        return events;
    }

    let seq = state.sequence;
    let bo = state.msb_first;
    let timestamp = state.timestamp();
    let root_window = state.root_window;
    let focus_window = state.focus_window;

    // Build ancestor paths: [window, parent, ..., root]
    let old_path = ancestor_path(&state.windows, old_window, root_window);
    let new_path = ancestor_path(&state.windows, new_window, root_window);

    // Determine relationship between old and new windows.
    let old_is_ancestor_of_new = new_path.contains(&old_window);
    let new_is_ancestor_of_old = old_path.contains(&new_window);

    if old_is_ancestor_of_new {
        // Case B: old is ancestor of new (pointer moved to a descendant).
        // LeaveNotify(old, detail=Inferior)
        emit_crossing(&state.windows, &mut events, LEAVE_NOTIFY_EVENT, DETAIL_INFERIOR, mode, seq, timestamp, root_window, old_window, x, y, bo, focus_window);

        // EnterNotify(Virtual) for each intermediate window from old's child down to new's parent.
        // new_path = [new, ..., old's_child, old, ...]
        // Find old_window in new_path, then intermediates are between old and new (exclusive both).
        if let Some(old_pos) = new_path.iter().position(|&w| w == old_window) {
            // Intermediates: new_path[1..old_pos] in reverse (from old's child toward new's parent)
            for &intermediate in new_path[1..old_pos].iter().rev() {
                emit_crossing(&state.windows, &mut events, ENTER_NOTIFY_EVENT, DETAIL_VIRTUAL, mode, seq, timestamp, root_window, intermediate, x, y, bo, focus_window);
            }
        }

        // EnterNotify(new, detail=Ancestor)
        emit_crossing(&state.windows, &mut events, ENTER_NOTIFY_EVENT, DETAIL_ANCESTOR, mode, seq, timestamp, root_window, new_window, x, y, bo, focus_window);

    } else if new_is_ancestor_of_old {
        // Case A: new is ancestor of old (pointer moved to an ancestor).
        // LeaveNotify(old, detail=Ancestor)
        emit_crossing(&state.windows, &mut events, LEAVE_NOTIFY_EVENT, DETAIL_ANCESTOR, mode, seq, timestamp, root_window, old_window, x, y, bo, focus_window);

        // LeaveNotify(Virtual) for each intermediate from old's parent up to new's child.
        if let Some(new_pos) = old_path.iter().position(|&w| w == new_window) {
            // Intermediates: old_path[1..new_pos] (from old's parent toward new's child)
            for &intermediate in &old_path[1..new_pos] {
                emit_crossing(&state.windows, &mut events, LEAVE_NOTIFY_EVENT, DETAIL_VIRTUAL, mode, seq, timestamp, root_window, intermediate, x, y, bo, focus_window);
            }
        }

        // EnterNotify(new, detail=Inferior)
        emit_crossing(&state.windows, &mut events, ENTER_NOTIFY_EVENT, DETAIL_INFERIOR, mode, seq, timestamp, root_window, new_window, x, y, bo, focus_window);

    } else {
        // Case C: Nonlinear (neither is ancestor of the other).
        // Find Lowest Common Ancestor (LCA).
        let old_set: std::collections::HashSet<u32> = old_path.iter().copied().collect();
        let lca = new_path.iter().copied().find(|w| old_set.contains(w)).unwrap_or(root_window);

        // LeaveNotify(old, detail=Nonlinear)
        emit_crossing(&state.windows, &mut events, LEAVE_NOTIFY_EVENT, DETAIL_NONLINEAR, mode, seq, timestamp, root_window, old_window, x, y, bo, focus_window);

        // LeaveNotify(NonlinearVirtual) for intermediates from old's parent up to LCA's child.
        if let Some(lca_pos) = old_path.iter().position(|&w| w == lca) {
            for &intermediate in &old_path[1..lca_pos] {
                emit_crossing(&state.windows, &mut events, LEAVE_NOTIFY_EVENT, DETAIL_NONLINEAR_VIRTUAL, mode, seq, timestamp, root_window, intermediate, x, y, bo, focus_window);
            }
        }

        // EnterNotify(NonlinearVirtual) for intermediates from LCA's child down to new's parent.
        if let Some(lca_pos) = new_path.iter().position(|&w| w == lca) {
            for &intermediate in new_path[1..lca_pos].iter().rev() {
                emit_crossing(&state.windows, &mut events, ENTER_NOTIFY_EVENT, DETAIL_NONLINEAR_VIRTUAL, mode, seq, timestamp, root_window, intermediate, x, y, bo, focus_window);
            }
        }

        // EnterNotify(new, detail=Nonlinear)
        emit_crossing(&state.windows, &mut events, ENTER_NOTIFY_EVENT, DETAIL_NONLINEAR, mode, seq, timestamp, root_window, new_window, x, y, bo, focus_window);
    }

    // KeymapNotify follows EnterNotify per X11 spec.
    if let Some(win) = state.windows.get(&new_window) {
        if win.event_mask & KEYMAP_STATE_MASK != 0 {
            let mut km_event = [0u8; 32];
            km_event[0] = KEYMAP_NOTIFY_EVENT;
            km_event[1..32].copy_from_slice(&state.pressed_keys[1..32]);
            events.extend_from_slice(&km_event);
        }
    }

    state.last_entered_window = new_window;
    events
}

/// Build a single crossing event (EnterNotify or LeaveNotify) for a specific window.
/// Used by grab activation/deactivation to send mode=Grab/Ungrab crossing events.
pub(crate) fn build_single_crossing_event(state: &ClientState, event_code: u8, window: u32, mode: u8) -> Option<[u8; 32]> {
    let required_mask = if event_code == ENTER_NOTIFY_EVENT { ENTER_WINDOW_MASK } else { LEAVE_WINDOW_MASK };
    let win = state.windows.get(&window)?;
    if win.event_mask & required_mask == 0 {
        return None;
    }

    let timestamp = state.timestamp();
    let ev = make_crossing_event(
        &state.windows,
        event_code,
        DETAIL_NONLINEAR, // Grab/Ungrab crossing events use Nonlinear per spec
        mode, state.sequence, timestamp, state.root_window,
        window, state.pointer_x, state.pointer_y,
        state.msb_first, state.focus_window,
    );
    Some(ev)
}

/// Resolve the target window for a pointer event, respecting active pointer grabs.
///
/// Per X11 spec §11.5:
/// - No grab: normal event delivery via find_event_subwindow.
/// - Grab with owner_events=true: try normal delivery first; if no window in the
///   hierarchy selects for the event, fall back to grab_window (using grab event_mask).
/// - Grab with owner_events=false: always deliver to grab_window.
fn resolve_pointer_event_target(
    state: &ClientState,
    top_level: u32,
    x: i16, y: i16,
    required_mask: u32,
) -> (u32, i16, i16) {
    if let Some(ref grab) = state.grabs.pointer_grab {
        if grab.owner_events {
            // owner_events=true: try normal delivery first
            let (win, ex, ey) = find_event_subwindow(&state.windows, top_level, x, y, required_mask);
            // Check if the found window actually selects for this event
            let selects = state.windows.get(&win)
                .map(|w| w.event_mask & required_mask != 0)
                .unwrap_or(false)
                || (win == state.root_window);
            if selects {
                return (win, ex, ey);
            }
            // No window selects — fall through to grab_window, but only if grab's
            // event_mask includes this event type.
            if grab.event_mask & required_mask != 0 {
                let gw = grab.grab_window;
                let (gx, gy) = window_local_coords(&state.windows, gw, state.root_window, x, y);
                return (gw, gx, gy);
            }
            // Grab doesn't select for this event type either — discard
            return (win, ex, ey);
        } else {
            // owner_events=false: always deliver to grab_window
            if grab.event_mask & required_mask != 0 {
                let gw = grab.grab_window;
                let (gx, gy) = window_local_coords(&state.windows, gw, state.root_window, x, y);
                return (gw, gx, gy);
            }
            // Grab doesn't select for this event type — discard (return root, caller
            // will see no mask match and produce an empty event)
            return (state.root_window, x, y);
        }
    }
    // No active grab — normal delivery
    find_event_subwindow(&state.windows, top_level, x, y, required_mask)
}

/// Resolve the target window for a keyboard event, respecting active keyboard grabs.
///
/// Per X11 spec §11.5:
/// - No grab: keyboard events go to focus window, propagating up.
/// - Grab with owner_events=true: try normal delivery first; fall back to grab_window.
/// - Grab with owner_events=false: always deliver to grab_window.
fn resolve_keyboard_event_target(
    state: &ClientState,
    top_level: u32,
    required_mask: u32,
) -> (u32, i16, i16) {
    if let Some(ref grab) = state.grabs.keyboard_grab {
        if grab.owner_events {
            // owner_events=true: try normal delivery (focus window propagation)
            let focus = state.focus_window;
            if focus != 0 && focus != state.root_window {
                let target = propagate_keyboard_event(&state.windows, focus, required_mask);
                let selects = state.windows.get(&target)
                    .map(|w| w.event_mask & required_mask != 0)
                    .unwrap_or(false);
                if selects {
                    return (target, 0, 0);
                }
            }
            // No window selects — deliver to grab_window
            return (grab.grab_window, 0, 0);
        } else {
            // owner_events=false: always deliver to grab_window
            return (grab.grab_window, 0, 0);
        }
    }
    // No active grab — normal keyboard delivery
    let focus = state.focus_window;
    let target = if focus != 0 && focus != state.root_window {
        propagate_keyboard_event(&state.windows, focus, required_mask)
    } else {
        top_level
    };
    (target, 0, 0)
}

/// Compute the child field for keyboard events per X11 spec.
/// If event_window is an ancestor of focus_window, returns the direct child
/// of event_window on the path to focus_window. Otherwise returns 0.
fn keyboard_event_child(
    windows: &HashMap<u32, WindowState>,
    event_window: u32,
    focus_window: u32,
) -> u32 {
    if event_window == focus_window {
        return 0;
    }
    // Walk from focus_window up to event_window, tracking the previous step
    let mut cur = focus_window;
    let mut prev = focus_window;
    for _ in 0..128 {
        if cur == event_window {
            return prev;
        }
        if cur == 0 {
            return 0;
        }
        if let Some(w) = windows.get(&cur) {
            prev = cur;
            cur = w.parent;
        } else {
            return 0;
        }
    }
    0
}

pub(crate) fn build_x11_input_event(state: &mut ClientState, input: &InputEvent, top_level: u32) -> Vec<u8> {
    match input {
        InputEvent::MotionNotify { x, y, .. }
        | InputEvent::ButtonPress { x, y, .. }
        | InputEvent::ButtonRelease { x, y, .. } => {
            state.pointer_x = *x;
            state.pointer_y = *y;
            // Record motion history for GetMotionEvents
            if matches!(input, InputEvent::MotionNotify { .. }) {
                let ts = state.timestamp();
                if state.motion_history.len() >= 256 {
                    state.motion_history.remove(0);
                }
                state.motion_history.push((ts, *x, *y));
            }
        }
        _ => {}
    }

    let seq = state.sequence;
    let bo = state.msb_first;
    let root_window = state.root_window;
    let mut event = [0u8; 32];
    let timestamp: u32 = state.timestamp();

    // Determine event target, respecting active grabs and owner_events per X11 spec §11.5.
    //
    // When an active grab is in effect:
    //   owner_events=true:  Try normal event delivery first. If no window selects for
    //                       the event, redirect to grab_window using grab's event_mask.
    //   owner_events=false: Always deliver to grab_window using grab's event_mask.
    //
    // Pointer events check pointer_grab; keyboard events check keyboard_grab.
    let (event_window, event_x, event_y) = match input {
        InputEvent::MotionNotify { x, y, .. } => {
            resolve_pointer_event_target(state, top_level, *x, *y, POINTER_MOTION_MASK)
        }
        InputEvent::ButtonPress { x, y, .. } => {
            resolve_pointer_event_target(state, top_level, *x, *y, BUTTON_PRESS_MASK)
        }
        InputEvent::ButtonRelease { x, y, .. } => {
            resolve_pointer_event_target(state, top_level, *x, *y, BUTTON_RELEASE_MASK)
        }
        InputEvent::KeyPress { .. } => {
            resolve_keyboard_event_target(state, top_level, KEY_PRESS_MASK)
        }
        InputEvent::KeyRelease { .. } => {
            resolve_keyboard_event_target(state, top_level, KEY_RELEASE_MASK)
        }
        _ => (top_level, 0, 0),
    };

    // Generate crossing events for pointer movement between windows
    let mut crossing_events = Vec::new();
    if let InputEvent::MotionNotify { x, y, .. } = input {
        crossing_events = build_crossing_events(state, event_window, *x, *y, event_x, event_y);
    }

    // POINTER_MOTION_HINT_MASK suppression per X11 spec §7.1
    // After delivering one MotionNotify to a window with hint mask, suppress
    // further motion events until QueryPointer/GetMotionEvents/button/crossing.
    if matches!(input, InputEvent::MotionNotify { .. }) {
        if !crossing_events.is_empty() {
            state.motion_hint_suppressed = false;
        }
        let has_hint = state.windows.get(&event_window)
            .map(|w| w.event_mask & POINTER_MOTION_HINT_MASK != 0)
            .unwrap_or(false);
        if has_hint {
            if state.motion_hint_suppressed {
                return crossing_events;
            }
            state.motion_hint_suppressed = true;
        }
    }

    match input {
        InputEvent::MotionNotify { x, y, state: mask } => {
            event[0] = MOTION_NOTIFY_EVENT;
            event[1] = 0;
            write_u16_bo(&mut event, 2, seq, bo);
            write_u32_bo(&mut event, 4, timestamp, bo);
            write_u32_bo(&mut event, 8, root_window, bo);
            write_u32_bo(&mut event, 12, event_window, bo);
            write_u32_bo(&mut event, 16, 0u32, bo);
            write_i16_bo(&mut event, 20, *x, bo);
            write_i16_bo(&mut event, 22, *y, bo);
            write_i16_bo(&mut event, 24, event_x, bo);
            write_i16_bo(&mut event, 26, event_y, bo);
            write_u16_bo(&mut event, 28, *mask, bo);
            event[30] = 1;
        }
        InputEvent::ButtonPress { button, x, y, state: mask } => {
            state.motion_hint_suppressed = false;
            event[0] = BUTTON_PRESS_EVENT;
            event[1] = *button;
            write_u16_bo(&mut event, 2, seq, bo);
            write_u32_bo(&mut event, 4, timestamp, bo);
            write_u32_bo(&mut event, 8, root_window, bo);
            write_u32_bo(&mut event, 12, event_window, bo);
            write_u32_bo(&mut event, 16, 0u32, bo);
            write_i16_bo(&mut event, 20, *x, bo);
            write_i16_bo(&mut event, 22, *y, bo);
            write_i16_bo(&mut event, 24, event_x, bo);
            write_i16_bo(&mut event, 26, event_y, bo);
            write_u16_bo(&mut event, 28, *mask, bo);
            event[30] = 1;
        }
        InputEvent::ButtonRelease { button, x, y, state: mask } => {
            state.motion_hint_suppressed = false;
            event[0] = BUTTON_RELEASE_EVENT;
            event[1] = *button;
            write_u16_bo(&mut event, 2, seq, bo);
            write_u32_bo(&mut event, 4, timestamp, bo);
            write_u32_bo(&mut event, 8, root_window, bo);
            write_u32_bo(&mut event, 12, event_window, bo);
            write_u32_bo(&mut event, 16, 0u32, bo);
            write_i16_bo(&mut event, 20, *x, bo);
            write_i16_bo(&mut event, 22, *y, bo);
            write_i16_bo(&mut event, 24, event_x, bo);
            write_i16_bo(&mut event, 26, event_y, bo);
            write_u16_bo(&mut event, 28, *mask, bo);
            event[30] = 1;
        }
        InputEvent::KeyPress { keycode, state: mask } => {
            event[0] = KEY_PRESS_EVENT;
            event[1] = *keycode as u8;
            write_u16_bo(&mut event, 2, seq, bo);
            write_u32_bo(&mut event, 4, timestamp, bo);
            write_u32_bo(&mut event, 8, root_window, bo);
            write_u32_bo(&mut event, 12, event_window, bo);
            let child = keyboard_event_child(&state.windows, event_window, state.focus_window);
            write_u32_bo(&mut event, 16, child, bo);
            write_i16_bo(&mut event, 20, state.pointer_x, bo); // root_x
            write_i16_bo(&mut event, 22, state.pointer_y, bo); // root_y
            let (kev_x, kev_y) = window_local_coords(&state.windows, event_window, root_window, state.pointer_x, state.pointer_y);
            write_i16_bo(&mut event, 24, kev_x, bo); // event_x
            write_i16_bo(&mut event, 26, kev_y, bo); // event_y
            write_u16_bo(&mut event, 28, *mask, bo);
            event[30] = 1;
        }
        InputEvent::KeyRelease { keycode, state: mask } => {
            event[0] = KEY_RELEASE_EVENT;
            event[1] = *keycode as u8;
            write_u16_bo(&mut event, 2, seq, bo);
            write_u32_bo(&mut event, 4, timestamp, bo);
            write_u32_bo(&mut event, 8, root_window, bo);
            write_u32_bo(&mut event, 12, event_window, bo);
            let child = keyboard_event_child(&state.windows, event_window, state.focus_window);
            write_u32_bo(&mut event, 16, child, bo);
            write_i16_bo(&mut event, 20, state.pointer_x, bo); // root_x
            write_i16_bo(&mut event, 22, state.pointer_y, bo); // root_y
            let (kev_x, kev_y) = window_local_coords(&state.windows, event_window, root_window, state.pointer_x, state.pointer_y);
            write_i16_bo(&mut event, 24, kev_x, bo); // event_x
            write_i16_bo(&mut event, 26, kev_y, bo); // event_y
            write_u16_bo(&mut event, 28, *mask, bo);
            event[30] = 1;
        }
        InputEvent::WindowManage { action } => {
            // Handle window management actions via ICCCM/EWMH protocols.
            use x11_web_protocol::WindowWmState;
            let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
            let wm_delete_atom = state.intern_atom("WM_DELETE_WINDOW", false);
            let _wm_take_focus_atom = state.intern_atom("WM_TAKE_FOCUS", false);
            let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
            let net_wm_state_maximized_vert = state.intern_atom("_NET_WM_STATE_MAXIMIZED_VERT", false);
            let net_wm_state_maximized_horz = state.intern_atom("_NET_WM_STATE_MAXIMIZED_HORZ", false);
            let net_wm_state_fullscreen = state.intern_atom("_NET_WM_STATE_FULLSCREEN", false);
            let net_wm_state_hidden = state.intern_atom("_NET_WM_STATE_HIDDEN", false);

            // Check if window supports WM_DELETE_WINDOW protocol
            let supports_delete = state.window_supports_protocol(top_level, wm_delete_atom);

            match action {
                WindowWmState::Normal => {
                    // Remove all state atoms
                    if let Some(win) = state.windows.get_mut(&top_level) {
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4, // ATOM
                            format: 32,
                            data: Vec::new(),
                        });
                    }
                    state.set_wm_state(top_level, 1); // NormalState
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: state.window_uuid(top_level).unwrap_or_default(),
                            state: WindowWmState::Normal,
                        },
                    ));
                    // Send WM_TAKE_FOCUS if supported
                    state.send_wm_take_focus(top_level);
                }
                WindowWmState::Minimized => {
                    // Set _NET_WM_STATE_HIDDEN and WM_STATE = IconicState
                    if let Some(win) = state.windows.get_mut(&top_level) {
                        let atom_data = net_wm_state_hidden.to_le_bytes().to_vec();
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4,
                            format: 32,
                            data: atom_data,
                        });
                    }
                    state.set_wm_state(top_level, 3); // IconicState
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: state.window_uuid(top_level).unwrap_or_default(),
                            state: WindowWmState::Minimized,
                        },
                    ));
                }
                WindowWmState::Maximized => {
                    // Set _NET_WM_STATE_MAXIMIZED_VERT + HORZ
                    if let Some(win) = state.windows.get_mut(&top_level) {
                        let mut atom_data = Vec::new();
                        atom_data.extend_from_slice(&net_wm_state_maximized_vert.to_le_bytes());
                        atom_data.extend_from_slice(&net_wm_state_maximized_horz.to_le_bytes());
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4,
                            format: 32,
                            data: atom_data,
                        });
                    }
                    state.set_wm_state(top_level, 1); // NormalState
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: state.window_uuid(top_level).unwrap_or_default(),
                            state: WindowWmState::Maximized,
                        },
                    ));
                }
                WindowWmState::Fullscreen => {
                    if let Some(win) = state.windows.get_mut(&top_level) {
                        let atom_data = net_wm_state_fullscreen.to_le_bytes().to_vec();
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4,
                            format: 32,
                            data: atom_data,
                        });
                    }
                    state.set_wm_state(top_level, 1); // NormalState
                    let _ = state.update_tx.send((
                        state.client_id.clone(),
                        DisplayUpdate::WindowStateChanged {
                            window_id: state.window_uuid(top_level).unwrap_or_default(),
                            state: WindowWmState::Fullscreen,
                        },
                    ));
                }
                WindowWmState::Close => {
                    // ICCCM graceful close: send WM_DELETE_WINDOW ClientMessage
                    if supports_delete {
                        let mut cm = [0u8; 32];
                        cm[0] = CLIENT_MESSAGE_EVENT;
                        cm[1] = 32; // format
                        write_u16_bo(&mut cm, 2, seq, bo);
                        write_u32_bo(&mut cm, 4, top_level, bo);
                        write_u32_bo(&mut cm, 8, wm_protocols_atom, bo);
                        write_u32_bo(&mut cm, 12, wm_delete_atom, bo);
                        write_u32_bo(&mut cm, 16, timestamp, bo);
                        return cm.to_vec();
                    }
                    // Window doesn't support WM_DELETE_WINDOW -- destroy it directly
                    let destroy_data = {
                        let mut d = [0u8; 8];
                        d[0] = 4; // DestroyWindow opcode
                        state.write_u16(&mut d, 2, 2u16);
                        state.write_u32(&mut d, 4, top_level);
                        d
                    };
                    return super::handle_request(state, &destroy_data);
                }
            }

            // Send a _NET_WM_STATE ClientMessage to the window per EWMH
            let mut cm = [0u8; 32];
            cm[0] = CLIENT_MESSAGE_EVENT;
            cm[1] = 32; // format
            write_u16_bo(&mut cm, 2, seq, bo);
            write_u32_bo(&mut cm, 4, top_level, bo);
            write_u32_bo(&mut cm, 8, net_wm_state_atom, bo);
            return cm.to_vec();
        }
        InputEvent::CompositionEvent { phase, text } => {
            super::handlers::xim::handle_composition_event(state, phase, text);
            return Vec::new();
        }
        InputEvent::MenuActivate { .. }
        | InputEvent::DndBridge { .. }
        | InputEvent::TouchBegin { .. }
        | InputEvent::TouchUpdate { .. }
        | InputEvent::TouchEnd { .. }
        | InputEvent::GestureSwipe { .. }
        | InputEvent::GesturePinch { .. } => return Vec::new(),
    }

    let mut result = crossing_events;
    result.extend_from_slice(&event);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::Framebuffer;

    /// Create a minimal WindowState for testing.
    fn test_window(id: u32, parent: u32, x: i16, y: i16, w: u16, h: u16, event_mask: u32) -> WindowState {
        WindowState {
            id, parent, x, y, width: w, height: h,
            border_width: 0, visual: 0x21, class: 1, mapped: true,
            event_mask,
            do_not_propagate_mask: 0,
            background_pixel: 0, background_pixmap: None,
            border_pixel: 0, border_pixmap: None,
            override_redirect: false, redirected: false,
            framebuffer: Framebuffer::new(w as u32, h as u32),
            properties: HashMap::new(),
            owner_client_id: String::new(),
            cursor: None, children_order: Vec::new(),
            retained_temporary: false,
            bounding_shape: None, clip_shape: None, input_shape: None,
            shape_select_clients: Vec::new(),
            colormap: 0, backing_store: 0, backing_planes: 0xFFFFFFFF,
            backing_pixel: 0, save_under: false, visibility: 0,
            backing_pixmap: None, wm_hints_initial_state: None,
            transient_for: None, sync_request_counter: None, sync_request_value: 0,
            window_type: crate::xserver::types::WindowType::Normal,
        }
    }

    #[test]
    fn test_ancestor_path_simple() {
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, 0));
        windows.insert(10, test_window(10, root, 0, 0, 100, 100, 0));
        windows.insert(20, test_window(20, 10, 0, 0, 50, 50, 0));

        let path = ancestor_path(&windows, 20, root);
        assert_eq!(path, vec![20, 10, root]);
    }

    #[test]
    fn test_ancestor_path_root_is_self() {
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, 0));

        let path = ancestor_path(&windows, root, root);
        assert_eq!(path, vec![root]);
    }

    #[test]
    fn test_crossing_detail_nonlinear_sibling_windows() {
        // Two sibling windows under root. Moving between them = Nonlinear.
        let mut windows = HashMap::new();
        let root = 0x62;
        let both_mask = ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, 0);
        root_win.children_order = vec![10, 20];
        windows.insert(root, root_win);
        windows.insert(10, test_window(10, root, 0, 0, 100, 100, both_mask));
        windows.insert(20, test_window(20, root, 200, 0, 100, 100, both_mask));

        // Simulate: old=10, new=20 (siblings under root)
        let old_path = ancestor_path(&windows, 10, root);
        let new_path = ancestor_path(&windows, 20, root);
        let old_is_ancestor = new_path.contains(&10);
        let new_is_ancestor = old_path.contains(&20);
        assert!(!old_is_ancestor);
        assert!(!new_is_ancestor);
        // This is Case C: Nonlinear
    }

    #[test]
    fn test_crossing_detail_ancestor_descendant() {
        // Parent -> child crossing = Inferior (for parent) / Ancestor (for child)
        let mut windows = HashMap::new();
        let root = 0x62;
        let both_mask = ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, both_mask);
        root_win.children_order = vec![10];
        windows.insert(root, root_win);
        let mut parent_win = test_window(10, root, 10, 10, 200, 200, both_mask);
        parent_win.children_order = vec![20];
        windows.insert(10, parent_win);
        windows.insert(20, test_window(20, 10, 5, 5, 50, 50, both_mask));

        // Moving from parent (10) to child (20):
        // old_path = [10, root], new_path = [20, 10, root]
        let old_path = ancestor_path(&windows, 10, root);
        let new_path = ancestor_path(&windows, 20, root);
        let old_is_ancestor = new_path.contains(&10);
        let new_is_ancestor = old_path.contains(&20);
        assert!(old_is_ancestor); // 10 is ancestor of 20
        assert!(!new_is_ancestor);
        // Case B: old (10) is ancestor of new (20)
        // LeaveNotify(10, detail=Inferior), EnterNotify(20, detail=Ancestor)
    }

    #[test]
    fn test_crossing_generates_virtual_events() {
        // Build: root -> A -> B -> C
        // Move from root to C: should generate Virtual events for A and B.
        let mut windows = HashMap::new();
        let root = 0x62;
        let both_mask = ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, both_mask);
        root_win.children_order = vec![100];
        windows.insert(root, root_win);
        let mut a = test_window(100, root, 0, 0, 500, 500, both_mask);
        a.children_order = vec![200];
        windows.insert(100, a);
        let mut b = test_window(200, 100, 0, 0, 300, 300, both_mask);
        b.children_order = vec![300];
        windows.insert(200, b);
        windows.insert(300, test_window(300, 200, 0, 0, 100, 100, both_mask));

        // old_path = [root], new_path = [300, 200, 100, root]
        let new_path = ancestor_path(&windows, 300, root);
        assert_eq!(new_path, vec![300, 200, 100, root]);

        // root is ancestor of 300 (Case B)
        // LeaveNotify(root, Inferior)
        // EnterNotify(100, Virtual), EnterNotify(200, Virtual)
        // EnterNotify(300, Ancestor)
        // Intermediates: new_path[1..3] in reverse = [200, 100] -> reversed = [100, 200]
        let lca_pos = new_path.iter().position(|&w| w == root).unwrap();
        assert_eq!(lca_pos, 3);
        let intermediates: Vec<u32> = new_path[1..lca_pos].iter().rev().copied().collect();
        assert_eq!(intermediates, vec![100, 200]);
    }

    #[test]
    fn test_crossing_nonlinear_virtual_events() {
        // Build: root -> A -> C, root -> B -> D
        // Move from C to D: Nonlinear, with NonlinearVirtual for A and B.
        let mut windows = HashMap::new();
        let root = 0x62;
        let both_mask = ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, both_mask);
        root_win.children_order = vec![100, 200];
        windows.insert(root, root_win);
        let mut a = test_window(100, root, 0, 0, 400, 400, both_mask);
        a.children_order = vec![300];
        windows.insert(100, a);
        windows.insert(300, test_window(300, 100, 0, 0, 100, 100, both_mask));
        let mut b = test_window(200, root, 500, 0, 400, 400, both_mask);
        b.children_order = vec![400];
        windows.insert(200, b);
        windows.insert(400, test_window(400, 200, 0, 0, 100, 100, both_mask));

        // old=300, new=400
        let old_path = ancestor_path(&windows, 300, root);
        let new_path = ancestor_path(&windows, 400, root);
        assert_eq!(old_path, vec![300, 100, root]);
        assert_eq!(new_path, vec![400, 200, root]);

        // LCA = root
        let old_set: std::collections::HashSet<u32> = old_path.iter().copied().collect();
        let lca = new_path.iter().copied().find(|w| old_set.contains(w)).unwrap();
        assert_eq!(lca, root);

        // Leave intermediates (old's parent up to LCA's child): old_path[1..lca_pos] = [100]
        let old_lca_pos = old_path.iter().position(|&w| w == lca).unwrap();
        let leave_intermediates: Vec<u32> = old_path[1..old_lca_pos].to_vec();
        assert_eq!(leave_intermediates, vec![100]); // NonlinearVirtual LeaveNotify

        // Enter intermediates (LCA's child down to new's parent): new_path[1..lca_pos] reversed = [200]
        let new_lca_pos = new_path.iter().position(|&w| w == lca).unwrap();
        let enter_intermediates: Vec<u32> = new_path[1..new_lca_pos].iter().rev().copied().collect();
        assert_eq!(enter_intermediates, vec![200]); // NonlinearVirtual EnterNotify
    }

    #[test]
    fn test_window_local_coords() {
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, 0));
        windows.insert(10, test_window(10, root, 100, 200, 300, 300, 0));
        windows.insert(20, test_window(20, 10, 50, 60, 100, 100, 0));

        // Window 20 is at abs (150, 260). Pointer at (170, 280).
        let (ex, ey) = window_local_coords(&windows, 20, root, 170, 280);
        assert_eq!(ex, 170 - 150); // 20
        assert_eq!(ey, 280 - 260); // 20
    }

    #[test]
    fn test_find_deepest_window() {
        let mut windows = HashMap::new();
        let root = 0x62;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, 0);
        root_win.children_order = vec![10];
        windows.insert(root, root_win);
        let mut parent = test_window(10, root, 100, 100, 200, 200, 0);
        parent.children_order = vec![20];
        windows.insert(10, parent);
        windows.insert(20, test_window(20, 10, 50, 50, 80, 80, 0));

        // Point (160, 160) is in window 20 (parent at 100,100; child at 50,50 relative = 150,150 abs)
        let (wid, rx, ry) = find_deepest_window(&windows, root, 160, 160);
        assert_eq!(wid, 20);
        assert_eq!(rx, 10); // 160 - 100 - 50 = 10
        assert_eq!(ry, 10);
    }

    #[test]
    fn test_find_deepest_window_misses_child() {
        let mut windows = HashMap::new();
        let root = 0x62;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, 0);
        root_win.children_order = vec![10];
        windows.insert(root, root_win);
        let mut parent = test_window(10, root, 100, 100, 200, 200, 0);
        parent.children_order = vec![20];
        windows.insert(10, parent);
        windows.insert(20, test_window(20, 10, 50, 50, 10, 10, 0));

        // Point (120, 120) is in parent but NOT in child (child is at 150-160, 150-160)
        let (wid, rx, ry) = find_deepest_window(&windows, root, 120, 120);
        assert_eq!(wid, 10);
        assert_eq!(rx, 20); // 120 - 100
        assert_eq!(ry, 20);
    }

    #[test]
    fn test_propagate_keyboard_event_finds_selecting_ancestor() {
        let mut windows = HashMap::new();
        let root = 0x62;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, KEY_PRESS_MASK);
        root_win.children_order = vec![10];
        windows.insert(root, root_win);
        let mut parent = test_window(10, root, 0, 0, 200, 200, 0); // doesn't select
        parent.children_order = vec![20];
        windows.insert(10, parent);
        windows.insert(20, test_window(20, 10, 0, 0, 50, 50, 0)); // doesn't select

        // Keyboard event on window 20 should propagate up to root.
        let target = propagate_keyboard_event(&windows, 20, KEY_PRESS_MASK);
        assert_eq!(target, root);
    }

    #[test]
    fn test_propagate_keyboard_blocked_by_do_not_propagate() {
        let mut windows = HashMap::new();
        let root = 0x62;
        let mut root_win = test_window(root, 0, 0, 0, 1024, 768, KEY_PRESS_MASK);
        root_win.children_order = vec![10];
        windows.insert(root, root_win);
        let mut parent = test_window(10, root, 0, 0, 200, 200, 0);
        parent.do_not_propagate_mask = KEY_PRESS_MASK; // blocks propagation
        parent.children_order = vec![20];
        windows.insert(10, parent);
        windows.insert(20, test_window(20, 10, 0, 0, 50, 50, 0));

        // Should be blocked at window 10, returning focus window (20).
        let target = propagate_keyboard_event(&windows, 20, KEY_PRESS_MASK);
        assert_eq!(target, 20);
    }

    #[test]
    fn test_enforce_barriers_vertical() {
        let mut barriers = HashMap::new();
        barriers.insert(1, PointerBarrier {
            barrier_id: 1, window: 0x62,
            x1: 500, y1: 0, x2: 500, y2: 768,
            directions: 1, // PositiveX (blocks left-to-right)
            device_ids: Vec::new(),
        });

        // Moving right across barrier at x=500
        let (fx, fy) = enforce_barriers(&barriers, 490, 100, 510, 100);
        assert_eq!(fx, 500); // clamped
        assert_eq!(fy, 100);

        // Moving left across barrier (not blocked by PositiveX)
        let (fx, fy) = enforce_barriers(&barriers, 510, 100, 490, 100);
        assert_eq!(fx, 490); // not clamped
        assert_eq!(fy, 100);
    }

    #[test]
    fn test_enforce_barriers_horizontal() {
        let mut barriers = HashMap::new();
        barriers.insert(1, PointerBarrier {
            barrier_id: 1, window: 0x62,
            x1: 0, y1: 400, x2: 1024, y2: 400,
            directions: 0, // all directions
            device_ids: Vec::new(),
        });

        let (fx, fy) = enforce_barriers(&barriers, 500, 390, 500, 410);
        assert_eq!(fx, 500);
        assert_eq!(fy, 400); // clamped

        let (fx, fy) = enforce_barriers(&barriers, 500, 410, 500, 390);
        assert_eq!(fx, 500);
        assert_eq!(fy, 400); // clamped (all directions)
    }

    #[test]
    fn test_keyboard_event_child_direct_focus() {
        // When event_window == focus_window, child should be 0
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, 0));
        windows.insert(10, test_window(10, root, 0, 0, 100, 100, KEY_PRESS_MASK));
        assert_eq!(keyboard_event_child(&windows, 10, 10), 0);
    }

    #[test]
    fn test_keyboard_event_child_ancestor() {
        // event_window=root, focus_window=child => child field = direct child of root on path
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, KEY_PRESS_MASK));
        windows.insert(10, test_window(10, root, 0, 0, 200, 200, 0));
        windows.insert(20, test_window(20, 10, 0, 0, 50, 50, 0));

        // root is ancestor of 20, path is 20->10->root
        // child of root on path = 10
        assert_eq!(keyboard_event_child(&windows, root, 20), 10);
    }

    #[test]
    fn test_keyboard_event_child_not_ancestor() {
        // event_window is not an ancestor of focus => child = 0
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, 0));
        windows.insert(10, test_window(10, root, 0, 0, 100, 100, KEY_PRESS_MASK));
        windows.insert(20, test_window(20, root, 200, 0, 100, 100, 0));

        // 10 is sibling of 20, not ancestor
        assert_eq!(keyboard_event_child(&windows, 10, 20), 0);
    }

    #[test]
    fn test_propagate_keyboard_to_parent() {
        // Focus window doesn't select KeyPress, parent does => propagate to parent
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, KEY_PRESS_MASK));
        windows.insert(10, test_window(10, root, 0, 0, 100, 100, 0)); // no KeyPressMask

        let target = propagate_keyboard_event(&windows, 10, KEY_PRESS_MASK);
        assert_eq!(target, root);
    }

    #[test]
    fn test_propagate_keyboard_blocked_by_dnp_mask() {
        // Focus window has do_not_propagate blocking KeyPress
        let mut windows = HashMap::new();
        let root = 0x62;
        windows.insert(root, test_window(root, 0, 0, 0, 1024, 768, KEY_PRESS_MASK));
        let mut child = test_window(10, root, 0, 0, 100, 100, 0);
        child.do_not_propagate_mask = KEY_PRESS_MASK;
        windows.insert(10, child);

        let target = propagate_keyboard_event(&windows, 10, KEY_PRESS_MASK);
        // Should return focus_window (blocked, no match)
        assert_eq!(target, 10);
    }
}
