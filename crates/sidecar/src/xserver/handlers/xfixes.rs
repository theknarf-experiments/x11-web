//! XFIXES extension handler.

use tracing::debug;

use super::super::client::ClientState;
use super::super::types::{XFixesRegion, RegionRect};

pub(crate) fn handle_xfixes_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XFIXES minor opcode: {minor}");

    match minor {
        // 0: QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 5u32);
            state.write_u32(&mut reply, 12, 0u32);
            reply.to_vec()
        }

        // 1: ChangeSaveSet (extended) — XFixes extension of core ChangeSaveSet.
        // Request layout (after the extension opcode byte):
        //   byte 1:    minor opcode (already consumed)
        //   byte 2-3:  request length (words)
        //   byte 4-7:  window (u32)
        //   byte 8:    mode   (0=SetModeInsert, 1=SetModeDelete)
        //   byte 9:    target (0=SaveSetNearest, 1=SaveSetRoot)
        //   byte 10:   map    (0=SaveSetMap, 1=SaveSetUnmap, 2=SaveSetUnmap)
        1 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    138, 1, state.msb_first,
                );
            }
            let window = state.read_u32(data, 4);
            let mode   = data[8];
            // target and map are advisory hints; we record mode only.
            let _target = if data.len() > 9 { data[9] } else { 0 };
            let _map    = if data.len() > 10 { data[10] } else { 0 };

            match mode {
                0 => {
                    // SetModeInsert
                    if !state.save_set.contains(&window) {
                        state.save_set.push(window);
                    }
                }
                1 => {
                    // SetModeDelete
                    state.save_set.retain(|&w| w != window);
                }
                _ => {
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::BAD_VALUE, seq, mode as u32,
                        138, 1, state.msb_first,
                    );
                }
            }
            debug!("XFIXES ChangeSaveSet: window={window:#x} mode={mode}");
            Vec::new()
        }

        // 2: SelectSelectionInput — subscribe to selection owner change events
        2 => {
            if data.len() >= 16 {
                let window = state.read_u32(data, 4);
                let selection = state.read_u32(data, 8);
                let event_mask = state.read_u32(data, 12);
                debug!("XFIXES SelectSelectionInput: window={window:#x} selection={selection:#x} mask={event_mask:#x}");
                if event_mask != 0 {
                    state.selection_event_subscribers.insert(selection, event_mask);
                } else {
                    state.selection_event_subscribers.remove(&selection);
                }
            }
            Vec::new()
        }

        // 3: SelectCursorInput — subscribe to cursor change events
        3 => {
            if data.len() >= 12 {
                let window = state.read_u32(data, 4);
                let event_mask = state.read_u32(data, 8);
                debug!("XFIXES SelectCursorInput: window={window:#x} mask={event_mask:#x}");
                state.cursor_event_subscribers.insert(window, event_mask != 0);
            }
            Vec::new()
        }

        // 4: GetCursorImage — return actual current cursor bitmap
        4 => {
            // Try to find current cursor info
            let cursor_id = state.current_cursor;
            let (width, height, hotspot_x, hotspot_y, argb_data) =
                if cursor_id != 0 {
                    if let Some(info) = state.cursor_info.get(&cursor_id) {
                        if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                            (info.width, info.height, info.hotspot_x, info.hotspot_y, info.argb_data.clone())
                        } else {
                            // Cursor exists but no bitmap — return 1x1 transparent
                            (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
                        }
                    } else {
                        (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
                    }
                } else {
                    // Default cursor — return 1x1 transparent
                    (1u16, 1u16, 0u16, 0u16, vec![0u8; 4])
                };

            let pixels_len = (width as usize) * (height as usize) * 4;
            let extra = 24 + pixels_len;
            let total = 32 + extra;
            let length_units = (extra / 4) as u32;
            let mut reply = vec![0u8; total];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, length_units);
            state.write_i16(&mut reply, 8, state.pointer_x);   // x
            state.write_i16(&mut reply, 10, state.pointer_y);  // y
            state.write_u16(&mut reply, 12, width);
            state.write_u16(&mut reply, 14, height);
            state.write_u16(&mut reply, 16, hotspot_x);
            state.write_u16(&mut reply, 18, hotspot_y);
            state.write_u32(&mut reply, 20, state.cursor_serial);
            // Copy ARGB pixel data
            let copy_len = pixels_len.min(argb_data.len());
            reply[32..32 + copy_len].copy_from_slice(&argb_data[..copy_len]);
            reply
        }

        // 5: CreateRegion
        5 => {
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 6: CreateRegionFromBitmap
        6 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 7: CreateRegionFromWindow
        7 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 8: CreateRegionFromGC
        8 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 9: CreateRegionFromPicture
        9 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 10: DestroyRegion
        10 => {
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let region_id = state.read_u32(data, 4);
            state.xfixes_regions.remove(&region_id);
            debug!("DestroyRegion: id={region_id:#x}");
            Vec::new()
        }

        // 11: SetRegion
        11 => {
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 12: CopyRegion
        12 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let src_id = state.read_u32(data, 4);
            let dst_id = state.read_u32(data, 8);
            let region = state.xfixes_regions.get(&src_id).cloned().unwrap_or_else(XFixesRegion::new);
            state.xfixes_regions.insert(dst_id, region);
            Vec::new()
        }

        // 13: UnionRegion
        13 => {
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let src1 = state.read_u32(data, 4);
            let src2 = state.read_u32(data, 8);
            let dst = state.read_u32(data, 12);
            let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
            let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
            state.xfixes_regions.insert(dst, r1.union(&r2));
            Vec::new()
        }

        // 14: IntersectRegion
        14 => {
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let src1 = state.read_u32(data, 4);
            let src2 = state.read_u32(data, 8);
            let dst = state.read_u32(data, 12);
            let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
            let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
            state.xfixes_regions.insert(dst, r1.intersect(&r2));
            Vec::new()
        }

        // 15: SubtractRegion
        15 => {
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let src1 = state.read_u32(data, 4);
            let src2 = state.read_u32(data, 8);
            let dst = state.read_u32(data, 12);
            let r1 = state.xfixes_regions.get(&src1).cloned().unwrap_or_else(XFixesRegion::new);
            let r2 = state.xfixes_regions.get(&src2).cloned().unwrap_or_else(XFixesRegion::new);
            state.xfixes_regions.insert(dst, r1.subtract(&r2));
            Vec::new()
        }

        // 16: InvertRegion
        16 => {
            if data.len() < 20 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 17: TranslateRegion
        17 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let region_id = state.read_u32(data, 4);
            let dx = state.read_i16(data, 8);
            let dy = state.read_i16(data, 10);
            if let Some(r) = state.xfixes_regions.get_mut(&region_id) {
                r.translate(dx, dy);
            }
            Vec::new()
        }

        // 18: RegionExtents
        18 => {
            if data.len() < 12 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
            let src = state.read_u32(data, 4);
            let dst = state.read_u32(data, 8);
            let ext = state.xfixes_regions.get(&src)
                .map(|r| r.extents())
                .unwrap_or(RegionRect { x: 0, y: 0, width: 0, height: 0 });
            state.xfixes_regions.insert(dst, XFixesRegion::from_rects(vec![ext]));
            Vec::new()
        }

        // 19: FetchRegion
        19 => {
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 20: SetGCClipRegion
        20 => {
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 21: SetWindowShapeRegion
        21 => {
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
                    } else if let Some(region) = state.xfixes_regions.get(&region_id) {
                        Some(region.rects.iter().map(|r| RegionRect {
                            x: r.x + x_offset,
                            y: r.y + y_offset,
                            width: r.width,
                            height: r.height,
                        }).collect())
                    } else {
                        None
                    };

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

        // 22: SetPictureClipRegion
        22 => {
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 23: SetCursorName - associate a name with a cursor
        23 => {
            if data.len() >= 12 {
                let cursor_id = state.read_u32(data, 4);
                let name_len = state.read_u16(data, 8) as usize;
                if data.len() >= 12 + name_len {
                    let name = String::from_utf8_lossy(&data[12..12 + name_len]).to_string();
                    debug!("XFIXES SetCursorName: cursor={cursor_id:#x} name={name:?}");
                    // Store name in existing cursor info, or create a minimal entry
                    if let Some(info) = state.cursor_info.get_mut(&cursor_id) {
                        info.name = name;
                    } else {
                        use super::super::types::CursorInfo;
                        state.cursor_info.insert(cursor_id, CursorInfo {
                            css_name: String::new(),
                            source_pixmap: 0,
                            mask_pixmap: 0,
                            fore_red: 0, fore_green: 0, fore_blue: 0,
                            back_red: 0, back_green: 0, back_blue: 0,
                            hotspot_x: 0, hotspot_y: 0,
                            argb_data: Vec::new(),
                            width: 0, height: 0,
                            name,
                            anim_frames: Vec::new(),
                        });
                    }
                }
            }
            Vec::new()
        }

        // 24: GetCursorName
        24 => {
            let cursor_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            let name = state.cursor_info.get(&cursor_id)
                .map(|info| info.name.clone())
                .unwrap_or_default();
            let atom = if !name.is_empty() {
                let mut atoms = state.atoms.lock().unwrap();
                atoms.intern(&name, true)
            } else {
                0
            };
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len();
            let pad = (4 - (name_len % 4)) % 4;
            let extra = name_len + pad;
            let total = 32 + extra;
            let length_units = (extra / 4) as u32;
            let mut reply = vec![0u8; total];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, length_units);
            state.write_u32(&mut reply, 8, atom); // cursor name atom
            state.write_u16(&mut reply, 12, name_len as u16); // nbytes
            if !name_bytes.is_empty() {
                reply[32..32 + name_len].copy_from_slice(name_bytes);
            }
            reply
        }

        // 25: GetCursorImageAndName
        25 => {
            let cursor_id = state.current_cursor;
            let (width, height, hotspot_x, hotspot_y, argb_data, name) =
                if cursor_id != 0 {
                    if let Some(info) = state.cursor_info.get(&cursor_id) {
                        if !info.argb_data.is_empty() && info.width > 0 && info.height > 0 {
                            (info.width, info.height, info.hotspot_x, info.hotspot_y,
                             info.argb_data.clone(), info.name.clone())
                        } else {
                            (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], info.name.clone())
                        }
                    } else {
                        (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], String::new())
                    }
                } else {
                    (1u16, 1u16, 0u16, 0u16, vec![0u8; 4], String::new())
                };

            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len();
            let name_atom = if !name.is_empty() {
                let mut atoms = state.atoms.lock().unwrap();
                atoms.intern(&name, true)
            } else {
                0
            };
            let pixels_len = (width as usize) * (height as usize) * 4;
            let name_pad = (4 - (name_len % 4)) % 4;
            // Reply body after the 32-byte header:
            //   x(2) y(2) width(2) height(2) hotspot_x(2) hotspot_y(2) serial(4) atom(4) name_len(2) pad(2)
            //   = 24 bytes of fields, then pixels, then name + padding
            let extra = 24 + pixels_len + name_len + name_pad;
            let total = 32 + extra;
            let length_units = (extra / 4) as u32;
            let mut reply = vec![0u8; total];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, length_units);
            state.write_i16(&mut reply, 8, state.pointer_x);   // x
            state.write_i16(&mut reply, 10, state.pointer_y);  // y
            state.write_u16(&mut reply, 12, width);
            state.write_u16(&mut reply, 14, height);
            state.write_u16(&mut reply, 16, hotspot_x);
            state.write_u16(&mut reply, 18, hotspot_y);
            state.write_u32(&mut reply, 20, state.cursor_serial);
            state.write_u32(&mut reply, 24, name_atom);
            state.write_u16(&mut reply, 28, name_len as u16);
            // Pixel data starts at 32
            let copy_len = pixels_len.min(argb_data.len());
            reply[32..32 + copy_len].copy_from_slice(&argb_data[..copy_len]);
            // Name data follows pixels
            let name_offset = 32 + pixels_len;
            if !name_bytes.is_empty() {
                reply[name_offset..name_offset + name_len].copy_from_slice(name_bytes);
            }
            reply
        }

        // 26: ChangeCursor - replace one cursor with another
        26 => {
            if data.len() >= 12 {
                let source_cursor = state.read_u32(data, 4);
                let dest_cursor = state.read_u32(data, 8);
                debug!("XFIXES ChangeCursor: source={source_cursor:#x} dest={dest_cursor:#x}");
                // Update all windows that use dest_cursor to use source_cursor instead
                let windows_to_update: Vec<u32> = state.windows.iter()
                    .filter(|(_, w)| w.cursor == Some(dest_cursor))
                    .map(|(id, _)| *id)
                    .collect();
                for wid in windows_to_update {
                    if let Some(w) = state.windows.get_mut(&wid) {
                        w.cursor = Some(source_cursor);
                    }
                }
                // Copy cursor info from source to dest
                if let Some(info) = state.cursor_info.get(&source_cursor).cloned() {
                    state.cursor_info.insert(dest_cursor, info);
                }
                if let Some(css) = state.cursors.get(&source_cursor).cloned() {
                    state.cursors.insert(dest_cursor, css);
                }
            }
            Vec::new()
        }

        // 27: ChangeCursorByName - replace cursor matching a name
        27 => {
            if data.len() >= 12 {
                let source_cursor = state.read_u32(data, 4);
                let name_len = state.read_u16(data, 8) as usize;
                if data.len() >= 12 + name_len {
                    let name = String::from_utf8_lossy(&data[12..12 + name_len]).to_string();
                    debug!("XFIXES ChangeCursorByName: source={source_cursor:#x} name={name:?}");
                    // Find all cursors that have the matching name
                    let matching_cursor_ids: Vec<u32> = state.cursor_info.iter()
                        .filter(|(_, info)| info.name == name)
                        .map(|(id, _)| *id)
                        .collect();
                    // Replace each matching cursor with source_cursor's info
                    if let Some(source_info) = state.cursor_info.get(&source_cursor).cloned() {
                        let source_css = state.cursors.get(&source_cursor).cloned();
                        for cid in &matching_cursor_ids {
                            state.cursor_info.insert(*cid, source_info.clone());
                            if let Some(ref css) = source_css {
                                state.cursors.insert(*cid, css.clone());
                            }
                            // Update windows using this cursor
                            let windows_to_update: Vec<u32> = state.windows.iter()
                                .filter(|(_, w)| w.cursor == Some(*cid))
                                .map(|(id, _)| *id)
                                .collect();
                            for wid in windows_to_update {
                                if let Some(w) = state.windows.get_mut(&wid) {
                                    w.cursor = Some(source_cursor);
                                }
                            }
                        }
                    }
                }
            }
            Vec::new()
        }

        // 28: ExpandRegion
        28 => {
            if data.len() < 20 {
                return crate::xserver::core::build_error_bo(crate::xserver::core::BAD_LENGTH, seq, data.len() as u32, 138, minor as u16, state.msb_first);
            }
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

        // 29: HideCursor
        29 => {
            if data.len() >= 8 {
                let window_id = state.read_u32(data, 4);
                state.cursor_hidden = state.cursor_hidden.saturating_add(1);
                debug!("XFIXES HideCursor: window={window_id:#x} nesting={}", state.cursor_hidden);
                // On first hide, send cursor changed to "none"
                if state.cursor_hidden == 1 {
                    if let Some(uuid) = state.top_level_uuid_for(window_id).or_else(|| state.window_uuid(window_id)) {
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            x11_web_protocol::DisplayUpdate::CursorChanged {
                                window_id: uuid,
                                cursor: "none".to_string(),
                            },
                        ));
                    }
                }
            }
            Vec::new()
        }

        // 30: ShowCursor
        30 => {
            if data.len() >= 8 {
                let window_id = state.read_u32(data, 4);
                state.cursor_hidden = state.cursor_hidden.saturating_sub(1);
                debug!("XFIXES ShowCursor: window={window_id:#x} nesting={}", state.cursor_hidden);
                // When nesting reaches 0, restore the real cursor
                if state.cursor_hidden == 0 {
                    // Resolve the real cursor name from current_cursor or fall back to "default"
                    let real_cursor = if state.current_cursor != 0 {
                        state.cursors.get(&state.current_cursor)
                            .cloned()
                            .or_else(|| state.cursor_info.get(&state.current_cursor).map(|i| i.css_name.clone()))
                            .unwrap_or_else(|| "default".to_string())
                    } else {
                        "default".to_string()
                    };
                    if let Some(uuid) = state.top_level_uuid_for(window_id).or_else(|| state.window_uuid(window_id)) {
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            x11_web_protocol::DisplayUpdate::CursorChanged {
                                window_id: uuid,
                                cursor: real_cursor,
                            },
                        ));
                    }
                }
            }
            Vec::new()
        }

        // 31: CreatePointerBarrier - create a barrier that constrains pointer movement
        31 => {
            if data.len() >= 28 {
                let barrier_id = state.read_u32(data, 4);
                let window = state.read_u32(data, 8);
                let x1 = state.read_i16(data, 12);
                let y1 = state.read_i16(data, 14);
                let x2 = state.read_i16(data, 16);
                let y2 = state.read_i16(data, 18);
                let directions = state.read_u32(data, 20);
                let num_devices = state.read_u16(data, 24) as usize;
                let mut device_ids = Vec::with_capacity(num_devices);
                for i in 0..num_devices {
                    let off = 28 + i * 2;
                    if off + 2 <= data.len() {
                        device_ids.push(state.read_u16(data, off));
                    }
                }
                debug!("XFIXES CreatePointerBarrier: id={barrier_id:#x} window={window:#x} ({x1},{y1})-({x2},{y2}) dirs={directions:#x} devices={num_devices}");
                state.barriers.insert(barrier_id, super::super::types::PointerBarrier {
                    barrier_id,
                    window,
                    x1,
                    y1,
                    x2,
                    y2,
                    directions,
                    device_ids,
                });
            }
            Vec::new()
        }

        // 32: DeletePointerBarrier - remove a barrier
        32 => {
            if data.len() >= 8 {
                let barrier_id = state.read_u32(data, 4);
                debug!("XFIXES DeletePointerBarrier: id={barrier_id:#x}");
                state.barriers.remove(&barrier_id);
            }
            Vec::new()
        }

        // 33: SetClientDisconnectMode - set client disconnect behavior
        33 => {
            if data.len() >= 8 {
                let mode = state.read_u32(data, 4);
                debug!("XFIXES SetClientDisconnectMode: mode={mode:#x}");
                state.disconnect_mode = mode;
            }
            Vec::new()
        }

        // 34: GetClientDisconnectMode
        34 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, state.disconnect_mode);
            reply.to_vec()
        }

        _ => {
            debug!("XFIXES: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                138, minor as u16, state.msb_first,
            )
        }
    }
}
