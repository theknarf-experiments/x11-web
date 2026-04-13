//! GC operations (opcodes 55-60).

use super::*;
use crate::xserver::core::require_len;

/// Parse GC value-list and apply to `gc`.
/// Returns `Some((error_bit, bad_value))` if a value fails X11 spec validation,
/// or `None` when all values are valid.
fn parse_gc_values(gc: &mut GcState, value_mask: u32, data: &[u8], msb_first: bool) -> Option<(u8, u32)> {
    let mut offset = 0;
    for bit in 0..23 {
        if value_mask & (1 << bit) != 0
            && offset + 4 <= data.len() {
                let val = read_u32_bo(data, offset, msb_first);
                // Validate enumerated fields per the X11 protocol spec
                match bit {
                    0 if val > 15 => return Some((bit as u8, val)),
                    5 if val > 2 => return Some((bit as u8, val)),
                    6 if val > 3 => return Some((bit as u8, val)),
                    7 if val > 2 => return Some((bit as u8, val)),
                    8 if val > 3 => return Some((bit as u8, val)),
                    9 if val > 1 => return Some((bit as u8, val)),
                    15 if val > 1 => return Some((bit as u8, val)),
                    22 if val > 1 => return Some((bit as u8, val)),
                    _ => {}
                }
                match bit {
                    0 => gc.function = val as u8,
                    1 => gc.plane_mask = val,
                    2 => gc.foreground = val,
                    3 => gc.background = val,
                    4 => gc.line_width = val as u16,
                    5 => gc.line_style = val as u8,
                    6 => gc.cap_style = val as u8,
                    7 => gc.join_style = val as u8,
                    8 => gc.fill_style = val as u8,
                    9 => gc.fill_rule = val as u8,
                    10 => gc.tile = val,
                    11 => gc.stipple = val,
                    12 => gc.ts_x = val as i16,
                    13 => gc.ts_y = val as i16,
                    14 => gc.font_id = val,
                    15 => gc.subwindow_mode = val as u8,
                    16 => gc.graphics_exposures = val != 0,
                    17 => gc.clip_x = val as i16,
                    18 => gc.clip_y = val as i16,
                    19 => gc.clip_mask = val,
                    20 => gc.dash_offset = val as u16,
                    21 => gc.dashes = val as u8,
                    22 => gc.arc_mode = val as u8,
                    _ => {}
                }
                offset += 4;
            }
    }
    None
}

// ---------------------------------------------------------------------------
// Opcode 55: CreateGC
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 55);

    let gc_id = state.read_u32(data, 4);

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(gc_id) {
        return build_error(BAD_ID_CHOICE, state.sequence, gc_id, 55, 0);
    }

    let drawable = state.read_u32(data, 8);
    let value_mask = state.read_u32(data, 12);

    // Per X11 spec, the drawable determines the root and depth for the GC.
    // It must be a valid window or pixmap.
    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(BAD_DRAWABLE, state.sequence, drawable, 55, 0);
    }

    // Validate: ID must not already be in use
    if state.gcs.contains_key(&gc_id) {
        return build_error(BAD_ID_CHOICE, state.sequence, gc_id, 55, 0);
    }

    let mut gc = GcState::default();
    if let Some((_error_bit, bad_value)) = parse_gc_values(&mut gc, value_mask, &data[16..], state.msb_first) {
        return build_error(BAD_VALUE, state.sequence, bad_value, 55, 0);
    }

    // If clip_mask was set to a pixmap, resolve it to a bitmap and
    // populate clip_rects from the bitmap so all drawing code works.
    // Setting clip_mask replaces any clip rectangles per X11 spec.
    if value_mask & (1 << 19) != 0 {
        if gc.clip_mask != 0 {
            gc.clip_mask_bitmap = state.resolve_clip_mask(gc.clip_mask);
            if let Some(ref bm) = gc.clip_mask_bitmap {
                gc.clip_rects = bm.to_clip_rects(gc.clip_x, gc.clip_y);
            } else {
                gc.clip_rects.clear();
            }
        } else {
            gc.clip_mask_bitmap = None;
            gc.clip_rects.clear();
        }
    }

    state.gcs.insert(gc_id, gc);

    // Register in shared GC registry for cross-connection access
    state.register_shared_gc(gc_id);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 56: ChangeGC
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 56);

    let gc_id = state.read_u32(data, 4);
    let value_mask = state.read_u32(data, 8);

    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 56, 0);
    }

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        if let Some((_error_bit, bad_value)) = parse_gc_values(gc, value_mask, &data[12..], state.msb_first) {
            return build_error(BAD_VALUE, state.sequence, bad_value, 56, 0);
        }
    }

    // If clip_mask was changed, resolve it to a bitmap and populate clip_rects.
    // Setting clip_mask replaces any clip rectangles per X11 spec.
    if value_mask & (1 << 19) != 0 {
        let mask_id = state.gcs.get(&gc_id).map(|gc| gc.clip_mask).unwrap_or(0);
        let clip_x = state.gcs.get(&gc_id).map(|gc| gc.clip_x).unwrap_or(0);
        let clip_y = state.gcs.get(&gc_id).map(|gc| gc.clip_y).unwrap_or(0);
        let bitmap = if mask_id != 0 {
            state.resolve_clip_mask(mask_id)
        } else {
            None
        };
        if let Some(gc) = state.gcs.get_mut(&gc_id) {
            if let Some(ref bm) = bitmap {
                gc.clip_rects = bm.to_clip_rects(clip_x, clip_y);
            } else {
                gc.clip_rects.clear();
            }
            gc.clip_mask_bitmap = bitmap;
        }
    }
    // If clip_x/clip_y changed but clip_mask wasn't re-set, update bitmap rects.
    if (value_mask & ((1 << 17) | (1 << 18)) != 0) && (value_mask & (1 << 19) == 0) {
        if let Some(gc) = state.gcs.get_mut(&gc_id) {
            if let Some(ref bm) = gc.clip_mask_bitmap {
                gc.clip_rects = bm.to_clip_rects(gc.clip_x, gc.clip_y);
            }
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 57: CopyGC
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 57);

    let src_gc = state.read_u32(data, 4);
    let dst_gc = state.read_u32(data, 8);
    let value_mask = state.read_u32(data, 12);

    if !state.gcs.contains_key(&src_gc) {
        return build_error(BAD_GC, state.sequence, src_gc, 57, 0);
    }
    if !state.gcs.contains_key(&dst_gc) {
        return build_error(BAD_GC, state.sequence, dst_gc, 57, 0);
    }

    let src = match state.gcs.get(&src_gc) {
        Some(g) => g.clone(),
        None => return Vec::new(),
    };

    if let Some(dst) = state.gcs.get_mut(&dst_gc) {
        if value_mask & (1 << 0) != 0 { dst.function = src.function; }
        if value_mask & (1 << 1) != 0 { dst.plane_mask = src.plane_mask; }
        if value_mask & (1 << 2) != 0 { dst.foreground = src.foreground; }
        if value_mask & (1 << 3) != 0 { dst.background = src.background; }
        if value_mask & (1 << 4) != 0 { dst.line_width = src.line_width; }
        if value_mask & (1 << 5) != 0 { dst.line_style = src.line_style; }
        if value_mask & (1 << 6) != 0 { dst.cap_style = src.cap_style; }
        if value_mask & (1 << 7) != 0 { dst.join_style = src.join_style; }
        if value_mask & (1 << 8) != 0 { dst.fill_style = src.fill_style; }
        if value_mask & (1 << 9) != 0 { dst.fill_rule = src.fill_rule; }
        if value_mask & (1 << 10) != 0 { dst.tile = src.tile; }
        if value_mask & (1 << 11) != 0 { dst.stipple = src.stipple; }
        if value_mask & (1 << 12) != 0 { dst.ts_x = src.ts_x; }
        if value_mask & (1 << 13) != 0 { dst.ts_y = src.ts_y; }
        if value_mask & (1 << 14) != 0 { dst.font_id = src.font_id; }
        if value_mask & (1 << 15) != 0 { dst.subwindow_mode = src.subwindow_mode; }
        if value_mask & (1 << 16) != 0 { dst.graphics_exposures = src.graphics_exposures; }
        if value_mask & (1 << 17) != 0 { dst.clip_x = src.clip_x; }
        if value_mask & (1 << 18) != 0 { dst.clip_y = src.clip_y; }
        if value_mask & (1 << 19) != 0 {
            dst.clip_mask = src.clip_mask;
            dst.clip_mask_bitmap = src.clip_mask_bitmap.clone();
        }
        if value_mask & (1 << 20) != 0 { dst.dash_offset = src.dash_offset; }
        if value_mask & (1 << 21) != 0 {
            dst.dashes = src.dashes;
            dst.dash_list = src.dash_list.clone();
        }
        if value_mask & (1 << 22) != 0 { dst.arc_mode = src.arc_mode; }
        // clip_rects follow the clip origin fields (bits 17-19)
        if value_mask & ((1 << 17) | (1 << 18) | (1 << 19)) != 0 {
            dst.clip_rects = src.clip_rects.clone();
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 58: SetDashes
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_dashes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 58);

    let gc_id = state.read_u32(data, 4);
    let dash_offset = state.read_u16(data, 8);
    let n_dashes = state.read_u16(data, 10) as usize;

    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 58, 0);
    }

    // Validate that the declared dash data fits within the request
    if 12 + n_dashes > data.len() {
        return build_error(BAD_LENGTH, state.sequence, 0, 58, 0);
    }

    // Per X11 spec: n_dashes must be > 0 and each dash value must be non-zero.
    if n_dashes == 0 {
        return build_error(BAD_VALUE, state.sequence, 0, 58, 0);
    }
    let dash_data = &data[12..12 + n_dashes];
    if dash_data.contains(&0) {
        return build_error(BAD_VALUE, state.sequence, 0, 58, 0);
    }

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        gc.dash_offset = dash_offset;
        gc.dash_list = dash_data.to_vec();
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 59: SetClipRectangles
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_clip_rectangles(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 12, state.sequence, 59);

    let _ordering = data[1];
    let gc_id = state.read_u32(data, 4);

    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 59, 0);
    }

    let clip_x = state.read_i16(data, 8);
    let clip_y = state.read_i16(data, 10);

    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = state.read_i16(data, offset);
        let y = state.read_i16(data, offset + 2);
        let w = state.read_u16(data, offset + 4);
        let h = state.read_u16(data, offset + 6);
        // Apply clip origin offset per X11 spec: each rectangle is relative
        // to the clip origin (clip_x, clip_y).
        rects.push((x + clip_x, y + clip_y, w, h));
        offset += 8;
    }

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        gc.clip_x = clip_x;
        gc.clip_y = clip_y;
        gc.clip_rects = rects;
        // Per X11 spec: SetClipRectangles replaces any pixmap clip mask.
        gc.clip_mask = 0;
        gc.clip_mask_bitmap = None;
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 60: FreeGC
// ---------------------------------------------------------------------------

pub(crate) fn handle_free_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 60);
    let gc_id = state.read_u32(data, 4);
    if !state.gcs.contains_key(&gc_id) {
        return build_error(BAD_GC, state.sequence, gc_id, 60, 0);
    }
    state.gcs.remove(&gc_id);
    // Unregister from shared registry
    state.unregister_shared_gc(gc_id);
    state.recycle_xid(gc_id);
    Vec::new()
}
