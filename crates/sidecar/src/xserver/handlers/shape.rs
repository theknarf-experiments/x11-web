//! SHAPE extension handler — full implementation per the SHAPE 1.1 spec.
//!
//! Supports bounding, clip, and input shapes using rectangle lists. Shapes
//! are stored per-window and ShapeNotify events are delivered to subscribed
//! clients.

use tracing::debug;
use super::parse_minor;

use super::super::client::ClientState;
use super::super::types::RegionRect;
use crate::xserver::event::serialize_event;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::shape::{
    CombineRequest, GetRectanglesRequest, InputSelectedRequest, MaskRequest, NotifyEvent as ShapeNotifyEvent,
    OffsetRequest, QueryExtentsRequest, QueryVersionRequest, RectanglesRequest, SelectInputRequest,
    SK,
};

/// SHAPE kind constants.
const SHAPE_BOUNDING: u8 = 0;
const SHAPE_CLIP: u8 = 1;
const SHAPE_INPUT: u8 = 2;

/// SHAPE operation constants.
const SHAPE_SET: u8 = 0;
const SHAPE_UNION: u8 = 1;
const SHAPE_INTERSECT: u8 = 2;
const SHAPE_SUBTRACT: u8 = 3;
const SHAPE_INVERT: u8 = 4;

/// SHAPE event code (first event for the extension).
const SHAPE_NOTIFY_EVENT: u8 = 64;

pub(crate) fn handle_shape_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("SHAPE minor opcode: {minor}");
    let shape_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 128, minor as u16)
    };

    match minor {
        // 0: QueryVersion
        0 => {
            let _req = parse_minor!(QueryVersionRequest, data, state, seq, 128, 0);
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // major version
                .set_u16(10, 1) // minor version
                .build()
        }

        // 1: Rectangles — set shape from a list of rectangles
        1 => {
            let req = parse_minor!(RectanglesRequest, data, state, seq, 128, 1);
            let operation = u8::from(req.operation);
            let kind = u8::from(req.destination_kind);
            let ordering = u8::from(req.ordering);
            // ordering is an optimization hint (0=UnSorted, 1=YSorted, 2=YXSorted,
            // 3=YXBanded).  We don't reorder internally, but we must reject
            // out-of-range values with BadValue per the SHAPE 1.1 spec.
            if ordering > 3 {
                return shape_err(crate::xserver::core::VALUE_ERROR, ordering as u32);
            }
            let window_id = req.destination_window;
            let x_offset = req.x_offset;
            let y_offset = req.y_offset;

            // Parse rectangles from the typed request
            let mut rects = Vec::with_capacity(req.rectangles.len());
            for r in req.rectangles.iter() {
                rects.push(RegionRect {
                    x: r.x + x_offset,
                    y: r.y + y_offset,
                    width: r.width,
                    height: r.height,
                });
            }

            // If no rectangles are given, reset to default (unshaped)
            let new_shape = if rects.is_empty() && operation == SHAPE_SET {
                None
            } else {
                Some(rects)
            };

            apply_shape(state, window_id, kind, operation, new_shape);
            send_shape_notify(state, window_id, kind, seq);

            Vec::new()
        }

        // 2: Mask — set shape from a pixmap bitmap
        2 => {
            let req = parse_minor!(MaskRequest, data, state, seq, 128, 2);
            let operation = u8::from(req.operation);
            let kind = u8::from(req.destination_kind);
            let window_id = req.destination_window;
            let x_offset = req.x_offset;
            let y_offset = req.y_offset;
            let pixmap_id = req.source_bitmap;

            let new_shape = if pixmap_id == 0 {
                // None pixmap => reset to default shape
                None
            } else if !state.pixmaps.contains_key(&pixmap_id) {
                return shape_err(crate::xserver::core::PIXMAP_ERROR, pixmap_id);
            } else {
                // Extract shape from pixmap: non-zero pixels form the shape
                let shape = extract_shape_from_pixmap(state, pixmap_id, x_offset, y_offset);
                Some(shape)
            };

            apply_shape(state, window_id, kind, operation, new_shape);
            send_shape_notify(state, window_id, kind, seq);

            Vec::new()
        }

        // 3: Combine — combine shapes from another window
        3 => {
            let req = parse_minor!(CombineRequest, data, state, seq, 128, 3);
            let operation = u8::from(req.operation);
            let dest_kind = u8::from(req.destination_kind);
            let src_kind = u8::from(req.source_kind);
            let dest_window = req.destination_window;
            let x_offset = req.x_offset;
            let y_offset = req.y_offset;
            let src_window = req.source_window;

            // Get source shape
            let src_rects = get_window_shape(state, src_window, src_kind).map(|rects| {
                rects
                    .iter()
                    .map(|r| RegionRect {
                        x: r.x + x_offset,
                        y: r.y + y_offset,
                        width: r.width,
                        height: r.height,
                    })
                    .collect::<Vec<_>>()
            });

            apply_shape(state, dest_window, dest_kind, operation, src_rects);
            send_shape_notify(state, dest_window, dest_kind, seq);

            Vec::new()
        }

        // 4: Offset — translate a shape
        4 => {
            let req = parse_minor!(OffsetRequest, data, state, seq, 128, 4);
            let kind = u8::from(req.destination_kind);
            let window_id = req.destination_window;
            let x_offset = req.x_offset;
            let y_offset = req.y_offset;

            if let Some(win) = state.windows.get_mut(&window_id) {
                let shape = match kind {
                    SHAPE_BOUNDING => &mut win.bounding_shape,
                    SHAPE_CLIP => &mut win.clip_shape,
                    SHAPE_INPUT => &mut win.input_shape,
                    _ => {
                        return shape_err(crate::xserver::core::VALUE_ERROR, kind as u32)
                    }
                };
                if let Some(rects) = shape {
                    for r in rects.iter_mut() {
                        r.x = r.x.saturating_add(x_offset);
                        r.y = r.y.saturating_add(y_offset);
                    }
                }
            }

            send_shape_notify(state, window_id, kind, seq);

            Vec::new()
        }

        // 5: QueryExtents — get bounding and clip shape extents
        5 => {
            let req = parse_minor!(QueryExtentsRequest, data, state, seq, 128, 5);
            let window_id = req.destination_window;

            let (bounding_shaped, bx, by, bw, bh) = if let Some(win) = state.windows.get(&window_id)
            {
                if let Some(ref rects) = win.bounding_shape {
                    let ext = compute_extents(rects);
                    (true, ext.x, ext.y, ext.width, ext.height)
                } else {
                    (false, 0i16, 0i16, win.width, win.height)
                }
            } else {
                (false, 0i16, 0i16, state.screen_width, state.screen_height)
            };

            let (clip_shaped, cx, cy, cw, ch) = if let Some(win) = state.windows.get(&window_id) {
                if let Some(ref rects) = win.clip_shape {
                    let ext = compute_extents(rects);
                    (true, ext.x, ext.y, ext.width, ext.height)
                } else {
                    (false, bx, by, bw, bh)
                }
            } else {
                (false, bx, by, bw, bh)
            };

            ReplyBuf::fixed(seq, state.msb_first)
                .set_u8(8, bounding_shaped as u8)
                .set_u8(9, clip_shaped as u8)
                .set_i16(12, bx)
                .set_i16(14, by)
                .set_u16(16, bw)
                .set_u16(18, bh)
                .set_i16(20, cx)
                .set_i16(22, cy)
                .set_u16(24, cw)
                .set_u16(26, ch)
                .build()
        }

        // 6: SelectInput — subscribe to ShapeNotify events
        6 => {
            let req = parse_minor!(SelectInputRequest, data, state, seq, 128, 6);
            let window_id = req.destination_window;
            let enable = req.enable;

            if let Some(win) = state.windows.get_mut(&window_id) {
                // Use a dummy client ID (sequence number) to track subscription
                let client_marker = seq as u32;
                if enable {
                    if !win.shape_select_clients.contains(&client_marker) {
                        win.shape_select_clients.push(client_marker);
                    }
                } else {
                    win.shape_select_clients.retain(|&c| c != client_marker);
                }
            }

            Vec::new()
        }

        // 7: InputSelected — query if shape events are selected
        7 => {
            let req = parse_minor!(InputSelectedRequest, data, state, seq, 128, 7);
            let window_id = req.destination_window;

            let enabled = state
                .windows
                .get(&window_id)
                .map(|w| !w.shape_select_clients.is_empty())
                .unwrap_or(false);

            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(enabled as u8)
                .build()
        }

        // 8: GetRectangles — get the shape rectangles for a window
        8 => {
            let req = parse_minor!(GetRectanglesRequest, data, state, seq, 128, 8);
            let window_id = req.window;
            let kind = u8::from(req.source_kind);

            let rects = if let Some(win) = state.windows.get(&window_id) {
                match kind {
                    SHAPE_BOUNDING => win.bounding_shape.as_deref(),
                    SHAPE_CLIP => win.clip_shape.as_deref(),
                    SHAPE_INPUT => win.input_shape.as_deref(),
                    _ => None,
                }
                .map(|r| r.to_vec())
                .unwrap_or_else(|| {
                    // No shape set — return single rectangle covering the window
                    vec![RegionRect {
                        x: 0,
                        y: 0,
                        width: win.width,
                        height: win.height,
                    }]
                })
            } else {
                vec![RegionRect {
                    x: 0,
                    y: 0,
                    width: state.screen_width,
                    height: state.screen_height,
                }]
            };

            let n_rects = rects.len() as u32;
            let rects_bytes = rects.len() * 8;
            let padded = (rects_bytes + 3) & !3;
            let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
                .set_data_byte(0) // ordering = UnSorted
                .set_u32(8, n_rects);

            for (i, r) in rects.iter().enumerate() {
                let off = 32 + i * 8;
                reply = reply
                    .set_i16(off, r.x)
                    .set_i16(off + 2, r.y)
                    .set_u16(off + 4, r.width)
                    .set_u16(off + 6, r.height);
            }
            reply.build()
        }

        _ => {
            debug!("SHAPE: unhandled minor opcode {minor}");
            shape_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}

/// Get the shape rectangles for a window, or None if unshaped.
fn get_window_shape(state: &ClientState, window_id: u32, kind: u8) -> Option<Vec<RegionRect>> {
    let win = state.windows.get(&window_id)?;
    match kind {
        SHAPE_BOUNDING => win.bounding_shape.clone().or_else(|| {
            Some(vec![RegionRect {
                x: 0,
                y: 0,
                width: win.width,
                height: win.height,
            }])
        }),
        SHAPE_CLIP => win.clip_shape.clone().or_else(|| {
            win.bounding_shape.clone().or_else(|| {
                Some(vec![RegionRect {
                    x: 0,
                    y: 0,
                    width: win.width,
                    height: win.height,
                }])
            })
        }),
        SHAPE_INPUT => win.input_shape.clone().or_else(|| {
            win.bounding_shape.clone().or_else(|| {
                Some(vec![RegionRect {
                    x: 0,
                    y: 0,
                    width: win.width,
                    height: win.height,
                }])
            })
        }),
        _ => None,
    }
}

/// Apply a shape operation to a window.
fn apply_shape(
    state: &mut ClientState,
    window_id: u32,
    kind: u8,
    operation: u8,
    new_rects: Option<Vec<RegionRect>>,
) {
    let win = match state.windows.get_mut(&window_id) {
        Some(w) => w,
        None => return,
    };

    let target = match kind {
        SHAPE_BOUNDING => &mut win.bounding_shape,
        SHAPE_CLIP => &mut win.clip_shape,
        SHAPE_INPUT => &mut win.input_shape,
        _ => return,
    };

    match operation {
        SHAPE_SET => {
            *target = new_rects;
        }
        SHAPE_UNION => {
            let existing = target.take().unwrap_or_default();
            let mut combined = existing;
            if let Some(new) = new_rects {
                combined.extend(new);
            }
            *target = if combined.is_empty() {
                None
            } else {
                Some(combined)
            };
        }
        SHAPE_INTERSECT => {
            if let Some(ref existing) = target {
                if let Some(ref new) = new_rects {
                    let result = intersect_rects(existing, new);
                    *target = if result.is_empty() {
                        None
                    } else {
                        Some(result)
                    };
                } else {
                    // Intersect with full window = keep existing
                }
            }
            // If target is None (full window), intersecting with new = new
            else if new_rects.is_some() {
                *target = new_rects;
            }
        }
        SHAPE_SUBTRACT => {
            if let Some(ref existing) = target {
                if let Some(ref new) = new_rects {
                    let result = subtract_rects(existing, new);
                    *target = if result.is_empty() {
                        None
                    } else {
                        Some(result)
                    };
                }
            } else if let Some(ref new) = new_rects {
                // Subtract from full window: create full rect then subtract
                let full = vec![RegionRect {
                    x: 0,
                    y: 0,
                    width: win.width,
                    height: win.height,
                }];
                let result = subtract_rects(&full, new);
                *target = if result.is_empty() {
                    None
                } else {
                    Some(result)
                };
            }
        }
        SHAPE_INVERT => {
            // Invert: result = new - existing
            if let Some(ref existing) = target {
                if let Some(ref new) = new_rects {
                    let result = subtract_rects(new, existing);
                    *target = if result.is_empty() {
                        None
                    } else {
                        Some(result)
                    };
                }
            } else {
                // existing is full window, invert = empty
                *target = Some(Vec::new());
            }
        }
        _ => {}
    }
}

/// Extract a shape from a pixmap: scan for non-zero pixels and build rectangles.
fn extract_shape_from_pixmap(
    state: &ClientState,
    pixmap_id: u32,
    x_offset: i16,
    y_offset: i16,
) -> Vec<RegionRect> {
    let pix = match state.pixmaps.get(&pixmap_id) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let w = pix.width as usize;
    let h = pix.height as usize;
    let data = pix.framebuffer.data();
    let mut rects = Vec::new();

    // Simple row-span extraction: scan each row for contiguous non-zero pixel runs
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let pixel_offset = (y * w + x) * 4;
            if pixel_offset + 4 <= data.len() {
                let a = data[pixel_offset + 3];
                let r = data[pixel_offset + 2];
                let g = data[pixel_offset + 1];
                let b = data[pixel_offset];
                if a != 0 || r != 0 || g != 0 || b != 0 {
                    // Start of a span
                    let start_x = x;
                    while x < w {
                        let po = (y * w + x) * 4;
                        if po + 4 > data.len() {
                            break;
                        }
                        let pa = data[po + 3];
                        let pr = data[po + 2];
                        let pg = data[po + 1];
                        let pb = data[po];
                        if pa == 0 && pr == 0 && pg == 0 && pb == 0 {
                            break;
                        }
                        x += 1;
                    }
                    rects.push(RegionRect {
                        x: start_x as i16 + x_offset,
                        y: y as i16 + y_offset,
                        width: (x - start_x) as u16,
                        height: 1,
                    });
                } else {
                    x += 1;
                }
            } else {
                x += 1;
            }
        }
    }

    // Coalesce vertically adjacent spans with same x range
    coalesce_rects(&mut rects);
    rects
}

/// Coalesce vertically adjacent rectangles with the same x range.
fn coalesce_rects(rects: &mut Vec<RegionRect>) {
    if rects.len() < 2 {
        return;
    }
    rects.sort_by(|a, b| a.x.cmp(&b.x).then(a.y.cmp(&b.y)));
    let mut i = 0;
    while i + 1 < rects.len() {
        let j = i + 1;
        if rects[i].x == rects[j].x
            && rects[i].width == rects[j].width
            && rects[i].y + rects[i].height as i16 == rects[j].y
        {
            rects[i].height += rects[j].height;
            rects.remove(j);
        } else {
            i += 1;
        }
    }
}

/// Send ShapeNotify event to subscribed clients.
fn send_shape_notify(state: &mut ClientState, window_id: u32, kind: u8, seq: u16) {
    let (shaped, ext) = if let Some(win) = state.windows.get(&window_id) {
        let shape = match kind {
            SHAPE_BOUNDING => &win.bounding_shape,
            SHAPE_CLIP => &win.clip_shape,
            SHAPE_INPUT => &win.input_shape,
            _ => return,
        };
        match shape {
            Some(rects) => (true, compute_extents(rects)),
            None => (
                false,
                RegionRect {
                    x: 0,
                    y: 0,
                    width: win.width,
                    height: win.height,
                },
            ),
        }
    } else {
        return;
    };

    let has_subscribers = state
        .windows
        .get(&window_id)
        .map(|w| !w.shape_select_clients.is_empty())
        .unwrap_or(false);

    if has_subscribers {
        let event = serialize_event(&ShapeNotifyEvent {
            response_type: SHAPE_NOTIFY_EVENT,
            shape_kind: SK::from(kind),
            sequence: seq,
            affected_window: window_id,
            extents_x: ext.x,
            extents_y: ext.y,
            extents_width: ext.width,
            extents_height: ext.height,
            server_time: state.timestamp(),
            shaped,
        }, state.msb_first);
        state.pending_events.push(event);
    }
}

/// Compute bounding extents of a rectangle list.
fn compute_extents(rects: &[RegionRect]) -> RegionRect {
    if rects.is_empty() {
        return RegionRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let mut x1 = i16::MAX;
    let mut y1 = i16::MAX;
    let mut x2 = i16::MIN;
    let mut y2 = i16::MIN;
    for r in rects {
        x1 = x1.min(r.x);
        y1 = y1.min(r.y);
        x2 = x2.max(r.x.saturating_add(r.width as i16));
        y2 = y2.max(r.y.saturating_add(r.height as i16));
    }
    RegionRect {
        x: x1,
        y: y1,
        width: (x2 - x1) as u16,
        height: (y2 - y1) as u16,
    }
}

/// Pairwise rectangle intersection.
fn intersect_rects(a: &[RegionRect], b: &[RegionRect]) -> Vec<RegionRect> {
    let mut result = Vec::new();
    for ra in a {
        for rb in b {
            let x1 = ra.x.max(rb.x);
            let y1 = ra.y.max(rb.y);
            let x2 = (ra.x + ra.width as i16).min(rb.x + rb.width as i16);
            let y2 = (ra.y + ra.height as i16).min(rb.y + rb.height as i16);
            if x2 > x1 && y2 > y1 {
                result.push(RegionRect {
                    x: x1,
                    y: y1,
                    width: (x2 - x1) as u16,
                    height: (y2 - y1) as u16,
                });
            }
        }
    }
    result
}

/// Subtract rectangles in `b` from rectangles in `a`.
fn subtract_rects(a: &[RegionRect], b: &[RegionRect]) -> Vec<RegionRect> {
    let mut result = a.to_vec();
    for sub in b {
        let mut new_result = Vec::new();
        for r in &result {
            subtract_single_rect(r, sub, &mut new_result);
        }
        result = new_result;
    }
    result
}

/// Subtract rectangle `sub` from rectangle `r`, appending result fragments.
fn subtract_single_rect(r: &RegionRect, sub: &RegionRect, out: &mut Vec<RegionRect>) {
    let rx2 = r.x + r.width as i16;
    let ry2 = r.y + r.height as i16;
    let sx2 = sub.x + sub.width as i16;
    let sy2 = sub.y + sub.height as i16;

    // No overlap — keep original
    if sub.x >= rx2 || sx2 <= r.x || sub.y >= ry2 || sy2 <= r.y {
        out.push(*r);
        return;
    }

    // Top strip
    if sub.y > r.y {
        out.push(RegionRect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: (sub.y - r.y) as u16,
        });
    }

    // Middle left
    let mid_y = r.y.max(sub.y);
    let mid_y2 = ry2.min(sy2);
    if mid_y < mid_y2 {
        if sub.x > r.x {
            out.push(RegionRect {
                x: r.x,
                y: mid_y,
                width: (sub.x - r.x) as u16,
                height: (mid_y2 - mid_y) as u16,
            });
        }
        if sx2 < rx2 {
            out.push(RegionRect {
                x: sx2,
                y: mid_y,
                width: (rx2 - sx2) as u16,
                height: (mid_y2 - mid_y) as u16,
            });
        }
    }

    // Bottom strip
    if sy2 < ry2 {
        out.push(RegionRect {
            x: r.x,
            y: sy2,
            width: r.width,
            height: (ry2 - sy2) as u16,
        });
    }
}
