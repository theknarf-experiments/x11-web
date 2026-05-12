//! GC operations (opcodes 55-60).

use super::*;
use x11rb_protocol::protocol::xproto::{
    ChangeGCRequest, CopyGCRequest, CreateGCAux, CreateGCRequest, FreeGCRequest,
    SetClipRectanglesRequest, SetDashesRequest, WindowClass, GC,
};

/// Apply parsed GC value-list fields to a `GcState`.
fn apply_create_gc_aux(gc: &mut GcState, aux: &CreateGCAux) {
    if let Some(f) = aux.function {
        gc.function = u32::from(f) as u8;
    }
    if let Some(v) = aux.plane_mask {
        gc.plane_mask = v;
    }
    if let Some(v) = aux.foreground {
        gc.foreground = v;
    }
    if let Some(v) = aux.background {
        gc.background = v;
    }
    if let Some(v) = aux.line_width {
        gc.line_width = v as u16;
    }
    if let Some(v) = aux.line_style {
        gc.line_style = u32::from(v) as u8;
    }
    if let Some(v) = aux.cap_style {
        gc.cap_style = u32::from(v) as u8;
    }
    if let Some(v) = aux.join_style {
        gc.join_style = u32::from(v) as u8;
    }
    if let Some(v) = aux.fill_style {
        gc.fill_style = u32::from(v) as u8;
    }
    if let Some(v) = aux.fill_rule {
        gc.fill_rule = u32::from(v) as u8;
    }
    if let Some(v) = aux.tile {
        gc.tile = v;
    }
    if let Some(v) = aux.stipple {
        gc.stipple = v;
    }
    if let Some(v) = aux.tile_stipple_x_origin {
        gc.ts_x = v as i16;
    }
    if let Some(v) = aux.tile_stipple_y_origin {
        gc.ts_y = v as i16;
    }
    if let Some(v) = aux.font {
        gc.font_id = v;
    }
    if let Some(v) = aux.subwindow_mode {
        gc.subwindow_mode = u32::from(v) as u8;
    }
    if let Some(v) = aux.graphics_exposures {
        gc.graphics_exposures = v != 0;
    }
    if let Some(v) = aux.clip_x_origin {
        gc.clip_x = v as i16;
    }
    if let Some(v) = aux.clip_y_origin {
        gc.clip_y = v as i16;
    }
    if let Some(v) = aux.clip_mask {
        gc.clip_mask = v;
    }
    if let Some(v) = aux.dash_offset {
        gc.dash_offset = v as u16;
    }
    if let Some(v) = aux.dashes {
        gc.dashes = v as u8;
    }
    if let Some(v) = aux.arc_mode {
        gc.arc_mode = u32::from(v) as u8;
    }
}

// ---------------------------------------------------------------------------
// Opcode 55: CreateGC
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_gc(state: &mut ClientState, req: &CreateGCRequest) -> Vec<u8> {
    let gc_id = req.cid;
    let drawable = req.drawable;
    let value_list = &req.value_list;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(gc_id) {
        return build_error(ID_CHOICE_ERROR, state.sequence, gc_id, 55, 0);
    }

    // Enforce per-client GC resource limit
    if !state.can_create_gc() {
        return build_error(ALLOC_ERROR, state.sequence, gc_id, 55, 0);
    }

    // Per X11 spec, the drawable determines the root and depth for the GC.
    // It must be a valid window or pixmap.
    if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
        return build_error(DRAWABLE_ERROR, state.sequence, drawable, 55, 0);
    }

    // Per X11 spec: CreateGC on an InputOnly window generates BadMatch
    // because InputOnly windows have no drawable surface.
    if state
        .windows
        .get(&drawable)
        .is_some_and(|w| w.class == u16::from(WindowClass::INPUT_ONLY))
    {
        return build_error(MATCH_ERROR, state.sequence, drawable, 55, 0);
    }

    // Validate: ID must not already be in use
    if state.gcs.contains_key(&gc_id) {
        return build_error(ID_CHOICE_ERROR, state.sequence, gc_id, 55, 0);
    }

    let mut gc = GcState::default();
    apply_create_gc_aux(&mut gc, value_list);

    // If clip_mask was set to a pixmap, resolve it to a bitmap and
    // populate clip_rects from the bitmap so all drawing code works.
    // Setting clip_mask replaces any clip rectangles per X11 spec.
    if value_list.clip_mask.is_some() {
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

pub(crate) fn handle_change_gc(state: &mut ClientState, req: &ChangeGCRequest) -> Vec<u8> {
    let gc_id = req.gc;
    let value_list = &req.value_list;

    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 56, 0);
    }

    // ChangeGCAux has the same fields as CreateGCAux -- reuse apply helper
    // by constructing a CreateGCAux from the ChangeGCAux fields.
    let aux = CreateGCAux {
        function: value_list.function,
        plane_mask: value_list.plane_mask,
        foreground: value_list.foreground,
        background: value_list.background,
        line_width: value_list.line_width,
        line_style: value_list.line_style,
        cap_style: value_list.cap_style,
        join_style: value_list.join_style,
        fill_style: value_list.fill_style,
        fill_rule: value_list.fill_rule,
        tile: value_list.tile,
        stipple: value_list.stipple,
        tile_stipple_x_origin: value_list.tile_stipple_x_origin,
        tile_stipple_y_origin: value_list.tile_stipple_y_origin,
        font: value_list.font,
        subwindow_mode: value_list.subwindow_mode,
        graphics_exposures: value_list.graphics_exposures,
        clip_x_origin: value_list.clip_x_origin,
        clip_y_origin: value_list.clip_y_origin,
        clip_mask: value_list.clip_mask,
        dash_offset: value_list.dash_offset,
        dashes: value_list.dashes,
        arc_mode: value_list.arc_mode,
    };

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        apply_create_gc_aux(gc, &aux);
    }

    // If clip_mask was changed, resolve it to a bitmap and populate clip_rects.
    // Setting clip_mask replaces any clip rectangles per X11 spec.
    if value_list.clip_mask.is_some() {
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
    let clip_origin_changed =
        value_list.clip_x_origin.is_some() || value_list.clip_y_origin.is_some();
    if clip_origin_changed && value_list.clip_mask.is_none() {
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

pub(crate) fn handle_copy_gc(state: &mut ClientState, req: &CopyGCRequest) -> Vec<u8> {
    let src_gc = req.src_gc;
    let dst_gc = req.dst_gc;
    let value_mask = u32::from(req.value_mask);

    if !state.gcs.contains_key(&src_gc) {
        return build_error(G_CONTEXT_ERROR, state.sequence, src_gc, 57, 0);
    }
    if !state.gcs.contains_key(&dst_gc) {
        return build_error(G_CONTEXT_ERROR, state.sequence, dst_gc, 57, 0);
    }

    let src = match state.gcs.get(&src_gc) {
        Some(g) => g.clone(),
        None => return Vec::new(),
    };

    if let Some(dst) = state.gcs.get_mut(&dst_gc) {
        if value_mask & u32::from(GC::FUNCTION) != 0 {
            dst.function = src.function;
        }
        if value_mask & u32::from(GC::PLANE_MASK) != 0 {
            dst.plane_mask = src.plane_mask;
        }
        if value_mask & u32::from(GC::FOREGROUND) != 0 {
            dst.foreground = src.foreground;
        }
        if value_mask & u32::from(GC::BACKGROUND) != 0 {
            dst.background = src.background;
        }
        if value_mask & u32::from(GC::LINE_WIDTH) != 0 {
            dst.line_width = src.line_width;
        }
        if value_mask & u32::from(GC::LINE_STYLE) != 0 {
            dst.line_style = src.line_style;
        }
        if value_mask & u32::from(GC::CAP_STYLE) != 0 {
            dst.cap_style = src.cap_style;
        }
        if value_mask & u32::from(GC::JOIN_STYLE) != 0 {
            dst.join_style = src.join_style;
        }
        if value_mask & u32::from(GC::FILL_STYLE) != 0 {
            dst.fill_style = src.fill_style;
        }
        if value_mask & u32::from(GC::FILL_RULE) != 0 {
            dst.fill_rule = src.fill_rule;
        }
        if value_mask & u32::from(GC::TILE) != 0 {
            dst.tile = src.tile;
        }
        if value_mask & u32::from(GC::STIPPLE) != 0 {
            dst.stipple = src.stipple;
        }
        if value_mask & u32::from(GC::TILE_STIPPLE_ORIGIN_X) != 0 {
            dst.ts_x = src.ts_x;
        }
        if value_mask & u32::from(GC::TILE_STIPPLE_ORIGIN_Y) != 0 {
            dst.ts_y = src.ts_y;
        }
        if value_mask & u32::from(GC::FONT) != 0 {
            dst.font_id = src.font_id;
        }
        if value_mask & u32::from(GC::SUBWINDOW_MODE) != 0 {
            dst.subwindow_mode = src.subwindow_mode;
        }
        if value_mask & u32::from(GC::GRAPHICS_EXPOSURES) != 0 {
            dst.graphics_exposures = src.graphics_exposures;
        }
        if value_mask & u32::from(GC::CLIP_ORIGIN_X) != 0 {
            dst.clip_x = src.clip_x;
        }
        if value_mask & u32::from(GC::CLIP_ORIGIN_Y) != 0 {
            dst.clip_y = src.clip_y;
        }
        if value_mask & u32::from(GC::CLIP_MASK) != 0 {
            dst.clip_mask = src.clip_mask;
            dst.clip_mask_bitmap = src.clip_mask_bitmap.clone();
        }
        if value_mask & u32::from(GC::DASH_OFFSET) != 0 {
            dst.dash_offset = src.dash_offset;
        }
        if value_mask & u32::from(GC::DASH_LIST) != 0 {
            dst.dashes = src.dashes;
            dst.dash_list = src.dash_list.clone();
        }
        if value_mask & u32::from(GC::ARC_MODE) != 0 {
            dst.arc_mode = src.arc_mode;
        }
        // clip_rects follow the clip origin fields
        let clip_bits =
            u32::from(GC::CLIP_ORIGIN_X) | u32::from(GC::CLIP_ORIGIN_Y) | u32::from(GC::CLIP_MASK);
        if value_mask & clip_bits != 0 {
            dst.clip_rects = src.clip_rects.clone();
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 58: SetDashes
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_dashes(state: &mut ClientState, req: &SetDashesRequest) -> Vec<u8> {
    let gc_id = req.gc;

    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 58, 0);
    }

    // Per X11 spec: n_dashes must be > 0 and each dash value must be non-zero.
    if req.dashes.is_empty() {
        return build_error(VALUE_ERROR, state.sequence, 0, 58, 0);
    }
    if req.dashes.contains(&0) {
        return build_error(VALUE_ERROR, state.sequence, 0, 58, 0);
    }

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        gc.dash_offset = req.dash_offset;
        gc.dash_list = req.dashes.to_vec();
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 59: SetClipRectangles
// ---------------------------------------------------------------------------

pub(crate) fn handle_set_clip_rectangles(
    state: &mut ClientState,
    req: &SetClipRectanglesRequest,
) -> Vec<u8> {
    let gc_id = req.gc;

    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 59, 0);
    }

    let clip_x = req.clip_x_origin;
    let clip_y = req.clip_y_origin;

    // Apply clip origin offset per X11 spec: each rectangle is relative
    // to the clip origin (clip_x, clip_y).
    let rects: Vec<(i16, i16, u16, u16)> = req
        .rectangles
        .iter()
        .map(|r| (r.x + clip_x, r.y + clip_y, r.width, r.height))
        .collect();

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

pub(crate) fn handle_free_gc(state: &mut ClientState, req: &FreeGCRequest) -> Vec<u8> {
    let gc_id = req.gc;
    if !state.gcs.contains_key(&gc_id) {
        return build_error(G_CONTEXT_ERROR, state.sequence, gc_id, 60, 0);
    }
    state.gcs.remove(&gc_id);
    // Unregister from shared registry
    state.unregister_shared_gc(gc_id);
    state.recycle_xid(gc_id);
    Vec::new()
}
