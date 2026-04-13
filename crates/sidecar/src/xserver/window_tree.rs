//! Window tree utility functions: ancestor chain, descendant checks, visibility.

use std::collections::HashMap;

use super::types::WindowState;

/// Walk from `start` up through `parent` links collecting the chain of window IDs.
pub(crate) fn ancestor_chain(windows: &HashMap<u32, WindowState>, start: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut cur = start;
    for _ in 0..128 {
        chain.push(cur);
        match windows.get(&cur).map(|w| w.parent) {
            Some(p) if p != 0 && p != cur => cur = p,
            _ => break,
        }
    }
    chain
}

/// Check if window `child` is a descendant of window `ancestor`.
pub(crate) fn is_descendant_of(windows: &HashMap<u32, WindowState>, child: u32, ancestor: u32) -> bool {
    let mut current = child;
    for _ in 0..128 {
        let parent = match windows.get(&current) {
            Some(w) => w.parent,
            None => return false,
        };
        if parent == ancestor {
            return true;
        }
        if parent == 0 {
            return false;
        }
        current = parent;
    }
    false
}

/// Compute the visibility state of a window based on its siblings' stacking order.
/// Returns: 0 = Unobscured, 1 = PartiallyObscured, 2 = FullyObscured.
pub(crate) fn compute_visibility(windows: &HashMap<u32, WindowState>, wid: u32) -> u8 {
    let (parent_id, wx, wy, ww, wh, mapped) = match windows.get(&wid) {
        Some(w) => (w.parent, w.x as i32, w.y as i32, w.width as i32, w.height as i32, w.mapped),
        None => return 2,
    };
    if !mapped || ww == 0 || wh == 0 {
        return 2; // FullyObscured — not visible
    }

    let children = match windows.get(&parent_id) {
        Some(p) => p.children_order.clone(),
        None => return 0, // No parent → root-level, assume unobscured
    };

    // Find our position in the stacking order
    let our_idx = match children.iter().position(|&c| c == wid) {
        Some(i) => i,
        None => return 0,
    };

    // Check all siblings above us (higher index = on top)
    let mut obscured_area = 0i64;
    let total_area = ww as i64 * wh as i64;
    let mut partially = false;

    for &sibling_id in &children[our_idx + 1..] {
        let sibling = match windows.get(&sibling_id) {
            Some(s) if s.mapped => s,
            _ => continue,
        };

        let sx = sibling.x as i32;
        let sy = sibling.y as i32;
        let sw = sibling.width as i32;
        let sh = sibling.height as i32;

        // Compute intersection
        let ix1 = wx.max(sx);
        let iy1 = wy.max(sy);
        let ix2 = (wx + ww).min(sx + sw);
        let iy2 = (wy + wh).min(sy + sh);

        if ix1 < ix2 && iy1 < iy2 {
            let overlap = (ix2 - ix1) as i64 * (iy2 - iy1) as i64;
            obscured_area += overlap;
            partially = true;
        }
    }

    if !partially {
        0 // Unobscured
    } else if obscured_area >= total_area {
        2 // FullyObscured (conservative: not accounting for overlap between siblings)
    } else {
        1 // PartiallyObscured
    }
}
