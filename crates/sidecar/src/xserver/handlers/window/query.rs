//! Query/geometry window handlers (opcodes 14, 15).

use super::*;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xproto::{GetGeometryRequest, QueryTreeRequest};

// ---------------------------------------------------------------------------
// Opcode 14: GetGeometry
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_geometry(state: &mut ClientState, req: &GetGeometryRequest) -> Vec<u8> {
    let seq = state.sequence;
    let drawable = req.drawable;

    // Check windows first, then pixmaps
    if let Some(win) = state.windows.get(&drawable) {
        return ReplyBuf::fixed(seq, state.msb_first)
            .set_data_byte(win.depth)
            .set_u32(8, state.root_window)
            .set_i16(12, win.x)
            .set_i16(14, win.y)
            .set_u16(16, win.width)
            .set_u16(18, win.height)
            .set_u16(20, win.border_width)
            .build();
    }

    if let Some(pixmap) = state.pixmaps.get(&drawable) {
        return ReplyBuf::fixed(seq, state.msb_first)
            .set_data_byte(pixmap.depth)
            .set_u32(8, state.root_window)
            .set_u16(16, pixmap.width)
            .set_u16(18, pixmap.height)
            .build();
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

    if !state.windows.contains_key(&wid) {
        return build_error(WINDOW_ERROR, seq, wid, 15, 0);
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

    let n_children = children.len() as u16;
    let mut reply = ReplyBuf::with_extra(seq, children.len() * 4, state.msb_first)
        .set_u32(8, state.root_window);

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    reply = reply.set_u32(12, parent).set_u16(16, n_children);

    for (i, &child) in children.iter().enumerate() {
        let off = 32 + i * 4;
        reply = reply.set_u32(off, child);
    }

    reply.build()
}
