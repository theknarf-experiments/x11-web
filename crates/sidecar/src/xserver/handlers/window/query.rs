//! Query/geometry window handlers (opcodes 14, 15).

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 14: GetGeometry
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_geometry(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 14);
    let drawable = state.read_u32(data, 4);

    // Check windows first, then pixmaps
    if let Some(win) = state.windows.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = win.depth;
        state.write_u16(&mut reply, 2, seq);
        state.write_u32(&mut reply, 8, state.root_window);
        state.write_i16(&mut reply, 12, win.x);
        state.write_i16(&mut reply, 14, win.y);
        state.write_u16(&mut reply, 16, win.width);
        state.write_u16(&mut reply, 18, win.height);
        state.write_u16(&mut reply, 20, win.border_width);
        return reply.to_vec();
    }

    if let Some(pixmap) = state.pixmaps.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = pixmap.depth;
        state.write_u16(&mut reply, 2, seq);
        state.write_u32(&mut reply, 8, state.root_window);
        state.write_u16(&mut reply, 16, pixmap.width);
        state.write_u16(&mut reply, 18, pixmap.height);
        return reply.to_vec();
    }

    // Drawable not found - return BadDrawable error
    build_error(BAD_DRAWABLE, seq, drawable, 14, 0)
}

// ---------------------------------------------------------------------------
// Opcode 15: QueryTree
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_tree(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 15);
    let wid = state.read_u32(data, 4);

    if !state.windows.contains_key(&wid) {
        return build_error(BAD_WINDOW, seq, wid, 15, 0);
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
    let reply_len = 32 + children.len() * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, children.len() as u32);
    state.write_u32(&mut reply, 8, state.root_window);

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    state.write_u32(&mut reply, 12, parent);
    state.write_u16(&mut reply, 16, n_children);

    for (i, &child) in children.iter().enumerate() {
        let off = 32 + i * 4;
        state.write_u32(&mut reply, off, child);
    }

    reply
}
