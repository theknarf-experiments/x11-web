//! XFIXES region operations.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::super::types::{XFixesRegion, RegionRect};
use crate::xserver::core::require_len;

/// 5: CreateRegion
pub(crate) fn handle_create_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 138, 5, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let num_rects = (data.len() - 8) / 8;
    let mut rects = Vec::with_capacity(num_rects);
    for i in 0..num_rects {
        let off = 8 + i * 8;
        let x = state.read_i16(data, off);
        let y = state.read_i16(data, off + 2);
        let w = state.read_u16(data, off + 4);
        let h = state.read_u16(data, off + 6);
        rects.push(RegionRect { x, y, width: w, height: h });
    }
    state.xfixes_regions.insert(region_id, XFixesRegion::from_rects(rects));
    debug!("CreateRegion: id={region_id:#x} rects={num_rects}");
    Vec::new()
}

/// 6: CreateRegionFromBitmap
pub(crate) fn handle_create_region_from_bitmap(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 6, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let bitmap_id = state.read_u32(data, 8);
    // Create region from pixmap bitmap — use full pixmap bounds
    let region = if let Some(pm) = state.pixmaps.get(&bitmap_id) {
        XFixesRegion::from_rects(vec![RegionRect {
            x: 0, y: 0, width: pm.width, height: pm.height,
        }])
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 7: CreateRegionFromWindow
pub(crate) fn handle_create_region_from_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 7, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let window_id = state.read_u32(data, 8);
    let region = if let Some(w) = state.windows.get(&window_id) {
        XFixesRegion::from_rects(vec![RegionRect {
            x: 0, y: 0, width: w.width, height: w.height,
        }])
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 8: CreateRegionFromGC
pub(crate) fn handle_create_region_from_gc(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 8, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let gc_id = state.read_u32(data, 8);
    let region = if let Some(gc) = state.gcs.get(&gc_id) {
        if gc.clip_rects.is_empty() {
            XFixesRegion::new()
        } else {
            XFixesRegion::from_rects(
                gc.clip_rects.iter().map(|&(x, y, w, h)| RegionRect { x, y, width: w, height: h }).collect()
            )
        }
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 9: CreateRegionFromPicture
pub(crate) fn handle_create_region_from_picture(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 9, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let picture_id = state.read_u32(data, 8);
    // Try to use the picture's clip region first; fall back to drawable bounds.
    let region = if let Some(clip_rects) = state.render.picture_clip_rects(picture_id) {
        XFixesRegion::from_rects(
            clip_rects.iter().map(|&(x, y, w, h)| RegionRect { x, y, width: w, height: h }).collect()
        )
    } else if let Some(drawable) = state.render.picture_drawable(picture_id) {
        // Resolve drawable dimensions from pixmaps or windows
        if let Some(pm) = state.pixmaps.get(&drawable) {
            XFixesRegion::from_rects(vec![RegionRect {
                x: 0, y: 0, width: pm.width, height: pm.height,
            }])
        } else if let Some(w) = state.windows.get(&drawable) {
            XFixesRegion::from_rects(vec![RegionRect {
                x: 0, y: 0, width: w.width, height: w.height,
            }])
        } else {
            XFixesRegion::new()
        }
    } else {
        XFixesRegion::new()
    };
    debug!("CreateRegionFromPicture: region={region_id:#x} picture={picture_id:#x}");
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 10: DestroyRegion
pub(crate) fn handle_destroy_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 138, 10, state.msb_first);
    let region_id = state.read_u32(data, 4);
    state.xfixes_regions.remove(&region_id);
    state.recycle_xid(region_id);
    debug!("DestroyRegion: id={region_id:#x}");
    Vec::new()
}

/// 11: SetRegion
pub(crate) fn handle_set_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 138, 11, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let num_rects = (data.len() - 8) / 8;
    let mut rects = Vec::with_capacity(num_rects);
    for i in 0..num_rects {
        let off = 8 + i * 8;
        let x = state.read_i16(data, off);
        let y = state.read_i16(data, off + 2);
        let w = state.read_u16(data, off + 4);
        let h = state.read_u16(data, off + 6);
        rects.push(RegionRect { x, y, width: w, height: h });
    }
    state.xfixes_regions.insert(region_id, XFixesRegion::from_rects(rects));
    Vec::new()
}

/// 12: CopyRegion
pub(crate) fn handle_copy_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 12, state.msb_first);
    let src_id = state.read_u32(data, 4);
    let dst_id = state.read_u32(data, 8);
    let region = state.xfixes_regions.get(&src_id).cloned().unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst_id, region);
    Vec::new()
}

/// 13: UnionRegion
pub(crate) fn handle_union_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 138, 13, state.msb_first);
    let src1 = state.read_u32(data, 4);
    let src2 = state.read_u32(data, 8);
    let dst = state.read_u32(data, 12);
    let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
    let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.union(&r2));
    Vec::new()
}

/// 14: IntersectRegion
pub(crate) fn handle_intersect_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 138, 14, state.msb_first);
    let src1 = state.read_u32(data, 4);
    let src2 = state.read_u32(data, 8);
    let dst = state.read_u32(data, 12);
    let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
    let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.intersect(&r2));
    Vec::new()
}

/// 15: SubtractRegion
pub(crate) fn handle_subtract_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 138, 15, state.msb_first);
    let src1 = state.read_u32(data, 4);
    let src2 = state.read_u32(data, 8);
    let dst = state.read_u32(data, 12);
    let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
    let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.subtract(&r2));
    Vec::new()
}

/// 16: InvertRegion
pub(crate) fn handle_invert_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 20, seq, 138, 16, state.msb_first);
    let src = state.read_u32(data, 4);
    let bx = state.read_i16(data, 8);
    let by = state.read_i16(data, 10);
    let bw = state.read_u16(data, 12);
    let bh = state.read_u16(data, 14);
    let dst = state.read_u32(data, 16);
    let r = state.xfixes_regions.get(&src).cloned().unwrap_or_else(XFixesRegion::new);
    let bounds = RegionRect { x: bx, y: by, width: bw, height: bh };
    state.xfixes_regions.insert(dst, r.invert(&bounds));
    Vec::new()
}

/// 17: TranslateRegion
pub(crate) fn handle_translate_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 17, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let dx = state.read_i16(data, 8);
    let dy = state.read_i16(data, 10);
    if let Some(r) = state.xfixes_regions.get_mut(&region_id) {
        r.translate(dx, dy);
    }
    Vec::new()
}

/// 18: RegionExtents
pub(crate) fn handle_region_extents(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 138, 18, state.msb_first);
    let src = state.read_u32(data, 4);
    let dst = state.read_u32(data, 8);
    let ext = state.xfixes_regions.get(&src)
        .map(|r| r.extents())
        .unwrap_or(RegionRect { x: 0, y: 0, width: 0, height: 0 });
    state.xfixes_regions.insert(dst, XFixesRegion::from_rects(vec![ext]));
    Vec::new()
}

/// 19: FetchRegion
pub(crate) fn handle_fetch_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 138, 19, state.msb_first);
    let region_id = state.read_u32(data, 4);
    let region = state.xfixes_regions.get(&region_id);
    let (ext, rects) = match region {
        Some(r) => (r.extents(), &r.rects[..]),
        None => (RegionRect { x: 0, y: 0, width: 0, height: 0 }, &[] as &[RegionRect]),
    };
    let rects_bytes = rects.len() * 8;
    let extra = 8 + rects_bytes; // 8 bytes extents + rect data
    let total = 32 + extra;
    let length_units = (extra / 4) as u32;
    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_units);
    // Extents: x1, y1, x2, y2 as i16
    state.write_i16(&mut reply, 8, ext.x);
    state.write_i16(&mut reply, 10, ext.y);
    state.write_i16(&mut reply, 12, ext.x + ext.width as i16);
    state.write_i16(&mut reply, 14, ext.y + ext.height as i16);
    // Rectangles
    for (i, r) in rects.iter().enumerate() {
        let off = 32 + 8 + i * 8;
        state.write_i16(&mut reply, off, r.x);
        state.write_i16(&mut reply, off + 2, r.y);
        state.write_u16(&mut reply, off + 4, r.width);
        state.write_u16(&mut reply, off + 6, r.height);
    }
    reply
}

/// 20: SetGCClipRegion
pub(crate) fn handle_set_gc_clip_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 138, 20, state.msb_first);
    let gc_id = state.read_u32(data, 4);
    let region_id = state.read_u32(data, 8);
    let x_origin = state.read_i16(data, 12);
    let y_origin = state.read_i16(data, 14);
    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        if region_id == 0 {
            // None - clear clip
            gc.clip_rects.clear();
            gc.clip_x = 0;
            gc.clip_y = 0;
        } else if let Some(region) = state.xfixes_regions.get(&region_id) {
            gc.clip_x = x_origin;
            gc.clip_y = y_origin;
            gc.clip_rects = region.rects.iter()
                .map(|r| (r.x + x_origin, r.y + y_origin, r.width, r.height))
                .collect();
        }
    }
    Vec::new()
}

/// 21: SetWindowShapeRegion
pub(crate) fn handle_set_window_shape_region(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 20 {
        let window_id = state.read_u32(data, 4);
        let kind = data[8]; // Shape kind (0=Bounding, 1=Clip, 2=Input)
        let x_offset = state.read_i16(data, 12);
        let y_offset = state.read_i16(data, 14);
        let region_id = state.read_u32(data, 16);

        if let Some(win) = state.windows.get_mut(&window_id) {
            let shape = if region_id == 0 {
                // None region = reset to default (unshaped)
                None
            } else { state.xfixes_regions.get(&region_id).map(|region| region.rects.iter().map(|r| RegionRect {
                    x: r.x + x_offset,
                    y: r.y + y_offset,
                    width: r.width,
                    height: r.height,
                }).collect()) };

            match kind {
                0 => win.bounding_shape = shape,
                1 => win.clip_shape = shape,
                2 => win.input_shape = shape,
                _ => {}
            }
            debug!("XFIXES SetWindowShapeRegion: window={window_id:#x} kind={kind} region={region_id:#x}");
        }
    }
    Vec::new()
}

/// 22: SetPictureClipRegion
pub(crate) fn handle_set_picture_clip_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 138, 22, state.msb_first);
    let pic_id = state.read_u32(data, 4);
    let region_id = state.read_u32(data, 8);
    let x_origin = state.read_i16(data, 12);
    let y_origin = state.read_i16(data, 14);
    if region_id == 0 {
        state.render.set_picture_clip_region(pic_id, None, 0, 0);
    } else if let Some(region) = state.xfixes_regions.get(&region_id) {
        let clip_rects: Vec<(i16, i16, u16, u16)> = region.rects.iter()
            .map(|r| (r.x + x_origin, r.y + y_origin, r.width, r.height))
            .collect();
        state.render.set_picture_clip_region(pic_id, Some(clip_rects), x_origin, y_origin);
    }
    Vec::new()
}

/// 28: ExpandRegion
pub(crate) fn handle_expand_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 20, seq, 138, 28, state.msb_first);
    let src = state.read_u32(data, 4);
    let dst = state.read_u32(data, 8);
    let left = state.read_u16(data, 12) as i16;
    let right = state.read_u16(data, 14) as i16;
    let top = state.read_u16(data, 16) as i16;
    let bottom = state.read_u16(data, 18) as i16;
    let region = state.xfixes_regions.get(&src).cloned().unwrap_or_else(XFixesRegion::new);
    let expanded = XFixesRegion::from_rects(
        region.rects.iter().map(|r| RegionRect {
            x: r.x.saturating_sub(left),
            y: r.y.saturating_sub(top),
            width: r.width.saturating_add((left + right) as u16),
            height: r.height.saturating_add((top + bottom) as u16),
        }).collect()
    );
    state.xfixes_regions.insert(dst, expanded);
    Vec::new()
}
