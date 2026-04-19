//! Window management handlers (opcodes 1-15).

// Re-export parent scope for submodules
pub(super) use super::*;

use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::xproto::{ExposeEvent, VisibilityNotifyEvent};

mod attributes;
mod configure;
mod create;
mod map;
mod query;

pub(crate) use attributes::{
    handle_change_save_set, handle_change_window_attributes, handle_get_window_attributes,
};
pub(crate) use configure::{
    handle_circulate_window, handle_configure_window, handle_reparent_window,
};
pub(crate) use create::{handle_create_window, handle_destroy_subwindows, handle_destroy_window};
pub(crate) use map::{
    handle_map_subwindows, handle_map_window, handle_unmap_subwindows, handle_unmap_window,
};
pub(crate) use query::{handle_get_geometry, handle_query_tree};

// ---------------------------------------------------------------------------
// Window gravity helpers (shared by map.rs and configure.rs)
// ---------------------------------------------------------------------------

/// Compute how a child window should move when its parent is resized,
/// based on the child's win_gravity attribute.
///   0=Unmap, 1=NorthWest, 2=North, 3=NorthEast, 4=West, 5=Center,
///   6=East, 7=SouthWest, 8=South, 9=SouthEast, 10=Static
pub(crate) fn win_gravity_delta(gravity: u8, dw: i16, dh: i16) -> (i16, i16) {
    match gravity {
        0 => (0, 0),           // Unmap — window gets unmapped, no repositioning
        1 => (0, 0),           // NorthWest — stays relative to top-left
        2 => (dw / 2, 0),      // North — moves with center of top edge
        3 => (dw, 0),          // NorthEast — stays relative to top-right
        4 => (0, dh / 2),      // West — moves with center of left edge
        5 => (dw / 2, dh / 2), // Center — moves with center
        6 => (dw, dh / 2),     // East — moves with center of right edge
        7 => (0, dh),          // SouthWest — stays relative to bottom-left
        8 => (dw / 2, dh),     // South — moves with center of bottom edge
        9 => (dw, dh),         // SouthEast — stays relative to bottom-right
        10 => (0, 0),          // Static — does not move
        _ => (0, 0),
    }
}

/// Restack a window within its parent's children_order based on its window type stacking layer.
/// Windows are placed at the top of their stacking layer, preserving order within the same layer.
pub(crate) fn restack_by_window_type(state: &mut ClientState, wid: u32, parent_id: u32) {
    let target_layer = state
        .windows
        .get(&wid)
        .map(effective_stacking_layer)
        .unwrap_or(2);

    // Remove and collect children in one pass
    if let Some(parent) = state.windows.get_mut(&parent_id) {
        parent.children_order.retain(|&c| c != wid);
    }

    // Collect layer info for all siblings
    let children: Vec<(u32, u8)> = state
        .windows
        .get(&parent_id)
        .map(|p| {
            p.children_order
                .iter()
                .map(|&c| {
                    let layer = state
                        .windows
                        .get(&c)
                        .map(effective_stacking_layer)
                        .unwrap_or(2);
                    (c, layer)
                })
                .collect()
        })
        .unwrap_or_default();

    // Find insertion point: after the last window with layer <= target_layer
    let insert_pos = children
        .iter()
        .rposition(|(_, layer)| *layer <= target_layer)
        .map(|pos| pos + 1)
        .unwrap_or(0);

    if let Some(parent) = state.windows.get_mut(&parent_id) {
        parent.children_order.insert(insert_pos, wid);
    }
}

/// Compute the effective stacking layer for a window, considering both
/// window type and _NET_WM_STATE_ABOVE/_NET_WM_STATE_BELOW state atoms.
pub(crate) fn effective_stacking_layer(win: &WindowState) -> u8 {
    let base_layer = win.window_type.stacking_layer();

    // Check for _NET_WM_STATE_ABOVE / _NET_WM_STATE_BELOW in the window's _NET_WM_STATE property
    // _NET_WM_STATE atom is 92, ABOVE is 102, BELOW is 103
    if let Some(prop) = win.properties.get(&92) {
        if prop.format == 32 {
            for chunk in prop.data.chunks_exact(4) {
                let atom = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                if atom == 102 {
                    // _NET_WM_STATE_ABOVE: promote to at least layer 3
                    return base_layer.max(3);
                }
                if atom == 103 {
                    // _NET_WM_STATE_BELOW: demote to layer 1
                    return 1;
                }
            }
        }
    }

    base_layer
}

/// Recalculate VisibilityNotify for all siblings of a window after stacking changes.
pub(crate) fn update_sibling_visibility(
    state: &mut ClientState,
    wid: u32,
    seq: u16,
    msb_first: bool,
) {
    // Save-under: when a save_under window is unmapped or reconfigured,
    // suppress Expose events for siblings since their framebuffer content
    // is preserved (per X11 spec, the server saved their contents).
    // In our architecture every window has its own Framebuffer, so content
    // is always intact when a save_under window reveals siblings.
    let save_under_active = state
        .windows
        .get(&wid)
        .map(|w| w.save_under)
        .unwrap_or(false);

    let parent_id = match state.windows.get(&wid) {
        Some(w) => w.parent,
        None => return,
    };
    let siblings = match state.windows.get(&parent_id) {
        Some(p) => p.children_order.clone(),
        None => return,
    };

    for &sib_id in &siblings {
        let old_vis = state
            .windows
            .get(&sib_id)
            .map(|w| w.visibility)
            .unwrap_or(0);
        let new_vis = crate::xserver::compute_visibility(&state.windows, sib_id);
        if new_vis != old_vis {
            if let Some(win) = state.windows.get_mut(&sib_id) {
                win.visibility = new_vis;
                if win.event_mask & EventMask::VISIBILITY_CHANGE != EventMask::NO_EVENT {
                    let vis_event = serialize_event(&VisibilityNotifyEvent {
                        response_type: VISIBILITY_NOTIFY_EVENT,
                        sequence: seq,
                        window: sib_id,
                        state: new_vis.into(),
                    }, msb_first);
                    state.pending_events.push(vis_event);
                }
            }
            // Broadcast to other clients watching this window
            let vis_event = serialize_event(&VisibilityNotifyEvent {
                response_type: VISIBILITY_NOTIFY_EVENT,
                sequence: seq,
                window: sib_id,
                state: new_vis.into(),
            }, msb_first);
            state.broadcast_event(sib_id, u32::from(EventMask::VISIBILITY_CHANGE), &vis_event);

            // Generate Expose events for siblings that became more visible
            // (newly-uncovered regions need repainting).
            // old_vis > new_vis means the window became less obscured:
            //   2 (FullyObscured) -> 1 (PartiallyObscured) or 0 (Unobscured)
            //   1 (PartiallyObscured) -> 0 (Unobscured)
            //
            // Per X11 spec: if the server maintains backing store for this window,
            // the contents are preserved and Expose events are NOT generated.
            // In our architecture each window has its own Framebuffer, so when
            // backing_store > 0 the pixels are already intact — skip Expose.
            //
            // Save-under suppression: if the triggering window had save_under=true,
            // the server preserved sibling contents, so no Expose is needed.
            if old_vis > new_vis && !save_under_active {
                let has_backing = state
                    .windows
                    .get(&sib_id)
                    .map(|w| w.backing_store > 0)
                    .unwrap_or(false);

                if !has_backing {
                    let (sib_w, sib_h, sib_mask) = state
                        .windows
                        .get(&sib_id)
                        .map(|w| (w.width, w.height, w.event_mask))
                        .unwrap_or((0, 0, 0));
                    if sib_mask & EventMask::EXPOSURE != EventMask::NO_EVENT {
                        let expose = serialize_event(&ExposeEvent {
                            response_type: EXPOSE_EVENT,
                            sequence: seq,
                            window: sib_id,
                            x: 0,
                            y: 0,
                            width: sib_w,
                            height: sib_h,
                            count: 0,
                        }, msb_first);
                        state.pending_events.push(expose);
                    }
                    // Also broadcast to other clients that selected ExposureMask
                    let (bc_w, bc_h) = state.windows.get(&sib_id)
                        .map(|w| (w.width, w.height))
                        .unwrap_or((0, 0));
                    let expose_bc = serialize_event(&ExposeEvent {
                        response_type: EXPOSE_EVENT,
                        sequence: seq,
                        window: sib_id,
                        x: 0,
                        y: 0,
                        width: bc_w,
                        height: bc_h,
                        count: 0,
                    }, msb_first);
                    state.broadcast_event(sib_id, u32::from(EventMask::EXPOSURE), &expose_bc);
                }
            }
        }
    }
}
