//! XFIXES region operations.

use super::super::{parse_minor, parse_or_void};
use tracing::debug;

/// FetchRegion reply layout: a 32-byte reply header, then 8 bytes of extents
/// (x1/y1/x2/y2 as i16), then a packed array of 8-byte RECTANGLE entries.
mod fetch_region_layout {
    /// Where the rectangle array begins within the reply buffer (after the
    /// 32-byte header and the 8-byte extents block).
    pub(super) const RECTS_START: usize = 32 + 8;
    /// Wire size of one RECTANGLE entry (x:i16, y:i16, w:u16, h:u16).
    pub(super) const RECT_SIZE: usize = 8;
}

use super::super::super::client::ClientState;
use super::super::super::types::{RegionRect, XFixesRegion};
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xfixes::{
    CopyRegionRequest, CreateRegionFromBitmapRequest, CreateRegionFromGCRequest,
    CreateRegionFromPictureRequest, CreateRegionFromWindowRequest, CreateRegionRequest,
    DestroyRegionRequest, ExpandRegionRequest, FetchRegionRequest, IntersectRegionRequest,
    InvertRegionRequest, RegionExtentsRequest, SetGCClipRegionRequest, SetPictureClipRegionRequest,
    SetRegionRequest, SetWindowShapeRegionRequest, SubtractRegionRequest, TranslateRegionRequest,
    UnionRegionRequest,
};

/// 5: CreateRegion
pub(crate) fn handle_create_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateRegionRequest, data, state, seq, 138, 5);
    let region_id = req.region;
    let rects: Vec<RegionRect> = req
        .rectangles
        .iter()
        .map(|r| RegionRect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        })
        .collect();
    let num_rects = rects.len();
    state
        .xfixes_regions
        .insert(region_id, XFixesRegion::from_rects(rects));
    debug!("CreateRegion: id={region_id:#x} rects={num_rects}");
    Vec::new()
}

/// 6: CreateRegionFromBitmap
pub(crate) fn handle_create_region_from_bitmap(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(CreateRegionFromBitmapRequest, data, state, seq, 138, 6);
    let region_id = req.region;
    let bitmap_id = req.bitmap;
    // Create region from pixmap bitmap — use full pixmap bounds
    let region = if let Some(pm) = state.pixmaps.get(&bitmap_id) {
        XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: pm.width,
            height: pm.height,
        }])
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 7: CreateRegionFromWindow
pub(crate) fn handle_create_region_from_window(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(CreateRegionFromWindowRequest, data, state, seq, 138, 7);
    let region_id = req.region;
    let window_id = req.window;
    let region = if let Some(w) = state.windows.get(&window_id) {
        XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: w.width,
            height: w.height,
        }])
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 8: CreateRegionFromGC
pub(crate) fn handle_create_region_from_gc(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(CreateRegionFromGCRequest, data, state, seq, 138, 8);
    let region_id = req.region;
    let gc_id = req.gc;
    let region = if let Some(gc) = state.gcs.get(&gc_id) {
        if gc.clip_rects.is_empty() {
            XFixesRegion::new()
        } else {
            XFixesRegion::from_rects(
                gc.clip_rects
                    .iter()
                    .map(|&(x, y, w, h)| RegionRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    })
                    .collect(),
            )
        }
    } else {
        XFixesRegion::new()
    };
    state.xfixes_regions.insert(region_id, region);
    Vec::new()
}

/// 9: CreateRegionFromPicture
pub(crate) fn handle_create_region_from_picture(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(CreateRegionFromPictureRequest, data, state, seq, 138, 9);
    let region_id = req.region;
    let picture_id = req.picture;
    // Try to use the picture's clip region first; fall back to drawable bounds.
    let region = if let Some(clip_rects) = state.render.picture_clip_rects(picture_id) {
        XFixesRegion::from_rects(
            clip_rects
                .iter()
                .map(|&(x, y, w, h)| RegionRect {
                    x,
                    y,
                    width: w,
                    height: h,
                })
                .collect(),
        )
    } else if let Some(drawable) = state.render.picture_drawable(picture_id) {
        // Resolve drawable dimensions from pixmaps or windows
        if let Some(pm) = state.pixmaps.get(&drawable) {
            XFixesRegion::from_rects(vec![RegionRect {
                x: 0,
                y: 0,
                width: pm.width,
                height: pm.height,
            }])
        } else if let Some(w) = state.windows.get(&drawable) {
            XFixesRegion::from_rects(vec![RegionRect {
                x: 0,
                y: 0,
                width: w.width,
                height: w.height,
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
    let req = parse_minor!(DestroyRegionRequest, data, state, seq, 138, 10);
    let region_id = req.region;
    state.xfixes_regions.remove(&region_id);
    state.recycle_xid(region_id);
    debug!("DestroyRegion: id={region_id:#x}");
    Vec::new()
}

/// 11: SetRegion
pub(crate) fn handle_set_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SetRegionRequest, data, state, seq, 138, 11);
    let region_id = req.region;
    let rects: Vec<RegionRect> = req
        .rectangles
        .iter()
        .map(|r| RegionRect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        })
        .collect();
    state
        .xfixes_regions
        .insert(region_id, XFixesRegion::from_rects(rects));
    Vec::new()
}

/// 12: CopyRegion
pub(crate) fn handle_copy_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CopyRegionRequest, data, state, seq, 138, 12);
    let src_id = req.source;
    let dst_id = req.destination;
    let region = state
        .xfixes_regions
        .get(&src_id)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst_id, region);
    Vec::new()
}

/// 13: UnionRegion
pub(crate) fn handle_union_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(UnionRegionRequest, data, state, seq, 138, 13);
    let src1 = req.source1;
    let src2 = req.source2;
    let dst = req.destination;
    let r1 = state
        .xfixes_regions
        .get(&src1)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    let r2 = state
        .xfixes_regions
        .get(&src2)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.union(&r2));
    Vec::new()
}

/// 14: IntersectRegion
pub(crate) fn handle_intersect_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(IntersectRegionRequest, data, state, seq, 138, 14);
    let src1 = req.source1;
    let src2 = req.source2;
    let dst = req.destination;
    let r1 = state
        .xfixes_regions
        .get(&src1)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    let r2 = state
        .xfixes_regions
        .get(&src2)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.intersect(&r2));
    Vec::new()
}

/// 15: SubtractRegion
pub(crate) fn handle_subtract_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SubtractRegionRequest, data, state, seq, 138, 15);
    let src1 = req.source1;
    let src2 = req.source2;
    let dst = req.destination;
    let r1 = state
        .xfixes_regions
        .get(&src1)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    let r2 = state
        .xfixes_regions
        .get(&src2)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    state.xfixes_regions.insert(dst, r1.subtract(&r2));
    Vec::new()
}

/// 16: InvertRegion
pub(crate) fn handle_invert_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(InvertRegionRequest, data, state, seq, 138, 16);
    let src = req.source;
    let dst = req.destination;
    let r = state
        .xfixes_regions
        .get(&src)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    let bounds = RegionRect {
        x: req.bounds.x,
        y: req.bounds.y,
        width: req.bounds.width,
        height: req.bounds.height,
    };
    state.xfixes_regions.insert(dst, r.invert(&bounds));
    Vec::new()
}

/// 17: TranslateRegion
pub(crate) fn handle_translate_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(TranslateRegionRequest, data, state, seq, 138, 17);
    let region_id = req.region;
    let dx = req.dx;
    let dy = req.dy;
    if let Some(r) = state.xfixes_regions.get_mut(&region_id) {
        r.translate(dx, dy);
    }
    Vec::new()
}

/// 18: RegionExtents
pub(crate) fn handle_region_extents(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(RegionExtentsRequest, data, state, seq, 138, 18);
    let src = req.source;
    let dst = req.destination;
    let ext = state
        .xfixes_regions
        .get(&src)
        .map(|r| r.extents())
        .unwrap_or(RegionRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    state
        .xfixes_regions
        .insert(dst, XFixesRegion::from_rects(vec![ext]));
    Vec::new()
}

/// 19: FetchRegion
pub(crate) fn handle_fetch_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(FetchRegionRequest, data, state, seq, 138, 19);
    let region_id = req.region;
    let region = state.xfixes_regions.get(&region_id);
    let (ext, rects) = match region {
        Some(r) => (r.extents(), &r.rects[..]),
        None => (
            RegionRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            &[] as &[RegionRect],
        ),
    };
    let rects_bytes = rects.len() * fetch_region_layout::RECT_SIZE;
    let extra = 8 + rects_bytes; // 8 bytes extents + rect data
    let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
        // Extents: x1, y1, x2, y2 as i16
        .set_i16(8, ext.x)
        .set_i16(10, ext.y)
        .set_i16(12, ext.x + ext.width as i16)
        .set_i16(14, ext.y + ext.height as i16);
    // Rectangles
    for (i, r) in rects.iter().enumerate() {
        let off = fetch_region_layout::RECTS_START + i * fetch_region_layout::RECT_SIZE;
        reply = reply
            .set_i16(off, r.x)
            .set_i16(off + 2, r.y)
            .set_u16(off + 4, r.width)
            .set_u16(off + 6, r.height);
    }
    reply.build()
}

/// 20: SetGCClipRegion
pub(crate) fn handle_set_gc_clip_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SetGCClipRegionRequest, data, state, seq, 138, 20);
    let gc_id = req.gc;
    let region_id = req.region;
    let x_origin = req.x_origin;
    let y_origin = req.y_origin;
    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        if region_id == 0 {
            // None - clear clip
            gc.clip_rects.clear();
            gc.clip_x = 0;
            gc.clip_y = 0;
        } else if let Some(region) = state.xfixes_regions.get(&region_id) {
            gc.clip_x = x_origin;
            gc.clip_y = y_origin;
            gc.clip_rects = region
                .rects
                .iter()
                .map(|r| (r.x + x_origin, r.y + y_origin, r.width, r.height))
                .collect();
        }
    }
    Vec::new()
}

/// 21: SetWindowShapeRegion
pub(crate) fn handle_set_window_shape_region(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }
    let req = parse_or_void!(SetWindowShapeRegionRequest, data);
    let window_id = req.dest;
    let kind: u8 = req.dest_kind.into();
    let x_offset = req.x_offset;
    let y_offset = req.y_offset;
    let region_id = req.region;

    if let Some(win) = state.windows.get_mut(&window_id) {
        let shape = if region_id == 0 {
            // None region = reset to default (unshaped)
            None
        } else {
            state.xfixes_regions.get(&region_id).map(|region| {
                region
                    .rects
                    .iter()
                    .map(|r| RegionRect {
                        x: r.x + x_offset,
                        y: r.y + y_offset,
                        width: r.width,
                        height: r.height,
                    })
                    .collect()
            })
        };

        match kind {
            0 => win.bounding_shape = shape,
            1 => win.clip_shape = shape,
            2 => win.input_shape = shape,
            _ => {}
        }
        debug!(
            "XFIXES SetWindowShapeRegion: window={window_id:#x} kind={kind} region={region_id:#x}"
        );
    }
    Vec::new()
}

/// 22: SetPictureClipRegion
pub(crate) fn handle_set_picture_clip_region(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(SetPictureClipRegionRequest, data, state, seq, 138, 22);
    let pic_id = req.picture;
    let region_id = req.region;
    let x_origin = req.x_origin;
    let y_origin = req.y_origin;
    if region_id == 0 {
        state.render.set_picture_clip_region(pic_id, None, 0, 0);
    } else if let Some(region) = state.xfixes_regions.get(&region_id) {
        let clip_rects: Vec<(i16, i16, u16, u16)> = region
            .rects
            .iter()
            .map(|r| (r.x + x_origin, r.y + y_origin, r.width, r.height))
            .collect();
        state
            .render
            .set_picture_clip_region(pic_id, Some(clip_rects), x_origin, y_origin);
    }
    Vec::new()
}

/// 28: ExpandRegion
pub(crate) fn handle_expand_region(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ExpandRegionRequest, data, state, seq, 138, 28);
    let src = req.source;
    let dst = req.destination;
    let left = req.left as i16;
    let right = req.right as i16;
    let top = req.top as i16;
    let bottom = req.bottom as i16;
    let region = state
        .xfixes_regions
        .get(&src)
        .cloned()
        .unwrap_or_else(XFixesRegion::new);
    let expanded = XFixesRegion::from_rects(
        region
            .rects
            .iter()
            .map(|r| RegionRect {
                x: r.x.saturating_sub(left),
                y: r.y.saturating_sub(top),
                width: r.width.saturating_add((left + right) as u16),
                height: r.height.saturating_add((top + bottom) as u16),
            })
            .collect(),
    );
    state.xfixes_regions.insert(dst, expanded);
    Vec::new()
}
