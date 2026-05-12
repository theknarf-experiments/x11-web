//! Query/geometry window handlers (opcodes 14, 15).

use super::*;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xproto::{
    GetGeometryReply, GetGeometryRequest, QueryTreeReply, QueryTreeRequest,
};

// ---------------------------------------------------------------------------
// Opcode 14: GetGeometry
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_geometry(state: &mut ClientState, req: &GetGeometryRequest) -> Vec<u8> {
    let seq = state.sequence;
    let drawable = req.drawable;

    if let Some(win) = state.windows.get(&drawable) {
        return serialize_reply(
            &GetGeometryReply {
                depth: win.depth,
                sequence: seq,
                length: 0,
                root: state.root_window,
                x: win.x,
                y: win.y,
                width: win.width,
                height: win.height,
                border_width: win.border_width,
            },
            state.byte_order(),
        );
    }

    if let Some(pixmap) = state.pixmaps.get(&drawable) {
        return serialize_reply(
            &GetGeometryReply {
                depth: pixmap.depth,
                sequence: seq,
                length: 0,
                root: state.root_window,
                x: 0,
                y: 0,
                width: pixmap.width,
                height: pixmap.height,
                border_width: 0,
            },
            state.byte_order(),
        );
    }

    // Cross-client fallback: foreign windows live in the shared store.
    if let Some(sw) = state
        .shared_windows
        .lock()
        .ok()
        .and_then(|sw| sw.get(&drawable).cloned())
    {
        return serialize_reply(
            &GetGeometryReply {
                depth: sw.depth,
                sequence: seq,
                length: 0,
                root: state.root_window,
                x: sw.x,
                y: sw.y,
                width: sw.width,
                height: sw.height,
                border_width: sw.border_width,
            },
            state.byte_order(),
        );
    }

    // Drawable not found - return BadDrawable error
    build_error(DRAWABLE_ERROR, seq, drawable, 14, 0)
}

// ---------------------------------------------------------------------------
// Opcode 15: QueryTree
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_tree(state: &mut ClientState, req: &QueryTreeRequest) -> Vec<u8> {
    let seq = state.sequence;
    let wid = req.window;

    // Cross-client fallback: foreign windows live in shared_windows.
    if !state.windows.contains_key(&wid) {
        let shared_win = state
            .shared_windows
            .lock()
            .ok()
            .and_then(|sw| sw.get(&wid).cloned());
        if let Some(sw) = shared_win {
            state.windows.insert(wid, sw);
        } else {
            return build_error(WINDOW_ERROR, seq, wid, 15, 0);
        }
    }

    // Return children in bottom-to-top stacking order per X11 spec.
    // Use the parent's children_order vec which tracks stacking order.
    let children: Vec<u32> = state
        .windows
        .get(&wid)
        .map(|w| {
            // children_order may not include all children (e.g. cross-connection windows).
            // Start with the stacking order, then append any children not in it.
            let mut ordered = w.children_order.clone();
            let all_children: Vec<u32> = state
                .windows
                .values()
                .filter(|c| c.parent == wid && !ordered.contains(&c.id))
                .map(|c| c.id)
                .collect();
            ordered.extend(all_children);
            ordered
        })
        .unwrap_or_default();

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    serialize_var_reply(
        &QueryTreeReply {
            sequence: seq,
            length: 0,
            root: state.root_window,
            parent,
            children,
        },
        state.byte_order(),
    )
}
