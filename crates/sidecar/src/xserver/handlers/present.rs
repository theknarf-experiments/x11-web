//! XC-MISC and Present extension handlers.

use tracing::{debug, info};

use super::super::client::ClientState;
use super::super::types::PresentSubscription;

// Present event mask bits (from the Present extension spec).
const PRESENT_COMPLETE_NOTIFY_MASK: u32 = 1;
const PRESENT_IDLE_NOTIFY_MASK: u32 = 2;
const PRESENT_CONFIG_NOTIFY_MASK: u32 = 4;

// Present option flags for PresentPixmap.
const PRESENT_OPTION_ASYNC: u32 = 1;
const PRESENT_OPTION_COPY: u32 = 2;

// Present capability flags.
const PRESENT_CAPABILITY_ASYNC: u32 = 1;

pub(crate) fn handle_xc_misc_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XC-MISC minor opcode: {minor}");

    match minor {
        0 => {
            // GetVersion: reply with version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1); // major version
            state.write_u16(&mut reply, 10, 1); // minor version
            reply.to_vec()
        }
        1 => {
            // GetXIDRange: reply with a contiguous range of resource IDs
            // from this client's allocation space.
            let mask: u32 = 0x003FFFFF;
            let current_offset = state.next_xid.wrapping_sub(state.resource_id_base) & mask;
            let remaining = mask.saturating_sub(current_offset) + 1;
            let range_size = remaining.min(65536);
            let start_id = state.resource_id_base | (current_offset & mask);
            // Advance the counter
            state.next_xid = state.resource_id_base | ((current_offset + range_size) & mask);

            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, start_id); // start_id
            state.write_u32(&mut reply, 12, range_size); // count
            reply.to_vec()
        }
        2 => {
            // GetXIDList: return requested number of individual resource IDs
            let count = if data.len() >= 8 {
                state.read_u32(data, 4)
            } else {
                0
            };
            let mask: u32 = 0x003FFFFF;
            let current_offset = state.next_xid.wrapping_sub(state.resource_id_base) & mask;
            let remaining = mask.saturating_sub(current_offset) + 1;
            let ids_to_return = count.min(4096).min(remaining);
            let extra_bytes = (ids_to_return as usize) * 4;
            let padded = (extra_bytes + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (padded / 4) as u32);
            state.write_u32(&mut reply, 8, ids_to_return); // ids_count
            // Allocate sequential IDs from this client's range
            for i in 0..ids_to_return {
                let offset = 32 + (i as usize) * 4;
                let id = state.resource_id_base | ((current_offset + i) & mask);
                state.write_u32(&mut reply, offset, id);
            }
            // Advance the counter
            state.next_xid = state.resource_id_base | ((current_offset + ids_to_return) & mask);
            reply
        }
        _ => {
            debug!("Unhandled XC-MISC minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                141, minor as u16, state.msb_first,
            )
        }
    }
}

/// Handle X Present extension requests (major opcode 148).
pub(crate) fn handle_present_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Present minor opcode: {minor}");

    match minor {
        // QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 1); // major version
            state.write_u32(&mut reply, 12, 2); // minor version
            reply.to_vec()
        }
        // Pixmap (PresentPixmap) — the critical operation
        1 => {
            if data.len() < 72 {
                debug!("PresentPixmap: request too short ({} bytes)", data.len());
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    148, minor as u16, state.msb_first,
                );
            }
            let window = state.read_u32(data, 4);
            let pixmap = state.read_u32(data, 8);
            let serial = state.read_u32(data, 12);
            let valid_area = state.read_u32(data, 16);
            let update_area = state.read_u32(data, 20);
            let x_off = state.read_i16(data, 24);
            let y_off = state.read_i16(data, 26);
            // target_crtc at 28..32, wait_fence at 32..36, idle_fence at 36..40
            let wait_fence = state.read_u32(data, 32);
            let idle_fence = state.read_u32(data, 36);
            let options = state.read_u32(data, 40);

            // Handle wait_fence: software server has no hardware vblank, so we
            // trigger untriggered fences immediately and proceed.  Per spec,
            // unknown fence IDs are not an error for PresentPixmap.
            if wait_fence != 0 {
                if let Some(fence) = state.sync_state.fences.get_mut(&wait_fence) {
                    if !fence.triggered {
                        fence.triggered = true;
                        debug!(
                            "PresentPixmap: wait_fence {:#x} was not triggered, triggering now (software server)",
                            wait_fence
                        );
                    }
                } else {
                    debug!("PresentPixmap: wait_fence {:#x} unknown, treating as triggered", wait_fence);
                }
            }

            let is_async = (options & PRESENT_OPTION_ASYNC) != 0;
            let is_copy = (options & PRESENT_OPTION_COPY) != 0;

            info!(
                "PresentPixmap: window={:#x} pixmap={:#x} serial={} x_off={} y_off={} options={:#x}",
                window, pixmap, serial, x_off, y_off, options
            );

            if is_async {
                debug!("PresentPixmap: async presentation requested");
            }

            // Copy pixels from the source pixmap to the destination window.
            // We need to clone the pixel data first because we can't borrow
            // both the pixmap and window framebuffers simultaneously.
            // Sync SHM pixmaps before reading
            state.sync_shm_pixmap(pixmap);

            // Determine region-limited copy bounds from valid_area / update_area.
            // For complex (multi-rect) regions we collect all rectangles so each
            // one can be copied individually.  For simple single-rect regions we
            // fall back to a single clip rectangle (same as before).
            let region_id = if update_area != 0 {
                update_area
            } else if valid_area != 0 {
                valid_area
            } else {
                0
            };
            let region_rects: Option<Vec<(i16, i16, u16, u16)>> = if region_id != 0 {
                state.xfixes_regions.get(&region_id).and_then(|region| {
                    if region.rects.is_empty() {
                        None
                    } else {
                        let v: Vec<_> = region
                            .rects
                            .iter()
                            .filter(|r| r.width > 0 && r.height > 0)
                            .map(|r| (r.x, r.y, r.width, r.height))
                            .collect();
                        if v.is_empty() { None } else { Some(v) }
                    }
                })
            } else {
                None
            };
            // For the sub-region extraction below we still need a single bounding
            // clip.  Use extents when there are multiple rects (the per-rect copy
            // happens later); for a single rect, just use it directly.
            let region_clip: Option<(i16, i16, u16, u16)> = region_rects.as_ref().map(|rects| {
                if rects.len() == 1 {
                    rects[0]
                } else {
                    let min_x = rects.iter().map(|r| r.0).min().unwrap();
                    let min_y = rects.iter().map(|r| r.1).min().unwrap();
                    let max_x = rects.iter().map(|r| r.0 as i32 + r.2 as i32).max().unwrap();
                    let max_y = rects.iter().map(|r| r.1 as i32 + r.3 as i32).max().unwrap();
                    (min_x, min_y, (max_x - min_x as i32) as u16, (max_y - min_y as i32) as u16)
                }
            });
            if let Some((rx, ry, rw, rh)) = region_clip {
                debug!(
                    "PresentPixmap: region clip x={} y={} w={} h={} ({} rects)",
                    rx, ry, rw, rh,
                    region_rects.as_ref().map_or(0, |r| r.len()),
                );
            }

            let src_info = {
                let resolved = state.resolve_drawable(pixmap);
                if let Some(win) = state.windows.get(&resolved) {
                    Some((
                        win.framebuffer.width() as u16,
                        win.framebuffer.height() as u16,
                        win.framebuffer.data().to_vec(),
                        24u8,
                    ))
                } else if let Some(pix) = state.pixmaps.get(&resolved) {
                    Some((
                        pix.framebuffer.width() as u16,
                        pix.framebuffer.height() as u16,
                        pix.framebuffer.data().to_vec(),
                        pix.depth,
                    ))
                } else {
                    debug!("PresentPixmap: source pixmap {:#x} not found", pixmap);
                    None
                }
            };

            if let Some((src_w, src_h, mut src_data, src_depth)) = src_info {
                // For depth-1 pixmaps, convert 1-bit values to proper RGB:
                // pixel != 0 -> white (0xFFFFFF), pixel == 0 -> black (0x000000)
                if src_depth <= 1 {
                    for i in (0..src_data.len()).step_by(4) {
                        if i + 3 < src_data.len() {
                            let is_set = src_data[i] != 0 || src_data[i + 1] != 0 || src_data[i + 2] != 0;
                            let val = if is_set { 0xFF } else { 0x00 };
                            src_data[i] = val;     // B
                            src_data[i + 1] = val; // G
                            src_data[i + 2] = val; // R
                            src_data[i + 3] = 0xFF;
                        }
                    }
                }

                // Build the list of rectangles to copy.  For multi-rect
                // regions each rectangle is copied individually; for a single
                // rect (or no region) we copy once using the bounding area.
                let copy_rects: Vec<(i16, i16, u16, u16)> = if let Some(ref rects) = region_rects {
                    if rects.len() > 1 {
                        // Multi-rect region: clamp each rect to source bounds.
                        rects
                            .iter()
                            .map(|&(rx, ry, rw, rh)| {
                                let cx = rx.max(0);
                                let cy = ry.max(0);
                                let cw = rw.min(src_w.saturating_sub(cx as u16));
                                let ch = rh.min(src_h.saturating_sub(cy as u16));
                                (cx, cy, cw, ch)
                            })
                            .filter(|&(_, _, w, h)| w > 0 && h > 0)
                            .collect()
                    } else {
                        // Single rect from region.
                        let (rx, ry, rw, rh) = rects[0];
                        let cx = rx.max(0);
                        let cy = ry.max(0);
                        let cw = rw.min(src_w.saturating_sub(cx as u16));
                        let ch = rh.min(src_h.saturating_sub(cy as u16));
                        vec![(cx, cy, cw, ch)]
                    }
                } else {
                    // No region constraint: full pixmap.
                    vec![(0i16, 0i16, src_w, src_h)]
                };

                // Determine the target (top-level) window for propagation.
                let (target_wid, parent_dx, parent_dy) = {
                    let mut wid = window;
                    let mut tx: i32 = 0;
                    let mut ty: i32 = 0;
                    for _ in 0..10 {
                        let parent = state.windows.get(&wid).map(|w| w.parent);
                        match parent {
                            Some(p) if p != state.root_window && p != 0 => {
                                if let Some(w) = state.windows.get(&wid) {
                                    tx += w.x as i32;
                                    ty += w.y as i32;
                                }
                                wid = p;
                            }
                            _ => break,
                        }
                    }
                    (wid, tx, ty)
                };

                // Copy each rectangle from the source pixmap to the window(s).
                let src_stride = src_w as usize * 4;
                let mut damage_min_x = i16::MAX;
                let mut damage_min_y = i16::MAX;
                let mut damage_max_x = i16::MIN;
                let mut damage_max_y = i16::MIN;

                for (eff_x, eff_y, eff_w, eff_h) in &copy_rects {
                    let dst_stride = *eff_w as usize * 4;
                    let needs_sub = *eff_x != 0 || *eff_y != 0 || *eff_w != src_w || *eff_h != src_h;
                    let (copy_data, copy_w, copy_h, copy_x_off, copy_y_off) = if needs_sub {
                        let mut sub = vec![0u8; dst_stride * *eff_h as usize];
                        for row in 0..*eff_h as usize {
                            let sy = *eff_y as usize + row;
                            if sy >= src_h as usize { break; }
                            let s_start = sy * src_stride + *eff_x as usize * 4;
                            let s_end = s_start + dst_stride;
                            if s_end <= src_data.len() {
                                let d_start = row * dst_stride;
                                sub[d_start..d_start + dst_stride]
                                    .copy_from_slice(&src_data[s_start..s_end]);
                            }
                        }
                        (sub, *eff_w, *eff_h, x_off + *eff_x, y_off + *eff_y)
                    } else {
                        (src_data.clone(), src_w, src_h, x_off, y_off)
                    };

                    // Copy to the child window (keeps its framebuffer up-to-date)
                    if let Some(win) = state.windows.get_mut(&window) {
                        win.framebuffer.put_image(copy_x_off, copy_y_off, copy_w, copy_h, &copy_data);
                    }

                    // Also copy to the top-level parent so the frontend displays it
                    if target_wid != window {
                        let total_x_off = (copy_x_off as i32 + parent_dx) as i16;
                        let total_y_off = (copy_y_off as i32 + parent_dy) as i16;
                        if let Some(parent_win) = state.windows.get_mut(&target_wid) {
                            parent_win.framebuffer.put_image(total_x_off, total_y_off, copy_w, copy_h, &copy_data);
                        }
                    }

                    // Accumulate damage bounds
                    damage_min_x = damage_min_x.min(copy_x_off);
                    damage_min_y = damage_min_y.min(copy_y_off);
                    damage_max_x = damage_max_x.max(copy_x_off + copy_w as i16);
                    damage_max_y = damage_max_y.max(copy_y_off + copy_h as i16);
                }

                if !state.windows.contains_key(&window) {
                    debug!("PresentPixmap: destination window {:#x} not found", window);
                }

                if copy_rects.len() > 1 {
                    info!(
                        "PresentPixmap: copied {} rects to window {:#x}",
                        copy_rects.len(), window
                    );
                } else if target_wid != window {
                    info!(
                        "PresentPixmap: propagated from child {:#x} to parent {:#x}",
                        window, target_wid
                    );
                } else {
                    info!(
                        "PresentPixmap: copied {}x{} to window {:#x}",
                        copy_rects[0].2, copy_rects[0].3, window
                    );
                }

                // Notify damage for the bounding area of all copied rects.
                if damage_min_x < damage_max_x && damage_min_y < damage_max_y {
                    state.notify_damage(
                        window,
                        damage_min_x,
                        damage_min_y,
                        (damage_max_x - damage_min_x) as u16,
                        (damage_max_y - damage_min_y) as u16,
                    );
                }
            }

            // Set idle_fence to triggered state — signals the client that the
            // pixmap buffer is free to reuse.
            if idle_fence != 0 {
                if let Some(fence) = state.sync_state.fences.get_mut(&idle_fence) {
                    fence.triggered = true;
                    debug!("PresentPixmap: triggered idle_fence {:#x}", idle_fence);
                }
            }

            // Increment MSC for each presentation
            state.present_msc += 1;
            // Send PresentCompleteNotify if the client subscribed via SelectInput
            let matching_subs: Vec<(u32, u32)> = state
                .present_subscriptions
                .iter()
                .filter(|(_, sub)| sub.window == window && (sub.event_mask & PRESENT_COMPLETE_NOTIFY_MASK) != 0)
                .map(|(&eid, sub)| (eid, sub.window))
                .collect();

            let ust = state.server_start.elapsed().as_micros() as u64;
            let msc = state.present_msc;
            for (event_id, _win) in &matching_subs {
                // GenericEvent format for PresentCompleteNotify (XGE wire format):
                // 32 bytes header + extra words for UST/MSC fields.
                // Total = 40 bytes => extra_length = (40 - 32) / 4 = 2 words.
                let mut event = vec![0u8; 40];
                event[0] = 35; // GenericEvent
                event[1] = 148; // Present extension major opcode
                state.write_u16(&mut event, 2, seq);
                state.write_u32(&mut event, 4, 2); // extra length in 4-byte words
                state.write_u16(&mut event, 8, 1); // CompleteNotify event type
                event[10] = 0; // kind = Pixmap
                event[11] = if is_copy { 1 } else { 0 }; // mode: 0=Copy, 1=Flip
                state.write_u32(&mut event, 12, *event_id); // event_id
                state.write_u32(&mut event, 16, window); // window
                state.write_u32(&mut event, 20, serial); // serial
                // UST: 64-bit microseconds since server start
                state.write_u32(&mut event, 24, (ust & 0xFFFF_FFFF) as u32); // UST low
                state.write_u32(&mut event, 28, (ust >> 32) as u32); // UST high
                // MSC: 64-bit in extra data
                state.write_u32(&mut event, 32, (msc & 0xFFFF_FFFF) as u32); // MSC low
                state.write_u32(&mut event, 36, (msc >> 32) as u32); // MSC high
                state.pending_events.push(event);
            }

            // Send PresentIdleNotify: the pixmap is no longer in use.
            // Only sent if not using copy semantics (with copy, the server
            // does not claim ownership of the pixmap).
            if !is_copy {
                let idle_subs: Vec<u32> = state
                    .present_subscriptions
                    .iter()
                    .filter(|(_, sub)| sub.window == window && (sub.event_mask & PRESENT_IDLE_NOTIFY_MASK) != 0)
                    .map(|(&eid, _)| eid)
                    .collect();

                for event_id in idle_subs {
                    let mut event = [0u8; 32];
                    event[0] = 35; // GenericEvent
                    event[1] = 148; // Present major opcode
                    state.write_u16(&mut event, 2, seq);
                    // bytes 4-7: length = 0 (no extra data beyond 32 bytes)
                    state.write_u16(&mut event, 8, 2); // evtype = IdleNotify
                    // bytes 10-11: pad
                    state.write_u32(&mut event, 12, event_id); // event id
                    state.write_u32(&mut event, 16, window); // window
                    state.write_u32(&mut event, 20, serial); // serial
                    state.write_u32(&mut event, 24, pixmap); // pixmap
                    state.write_u32(&mut event, 28, idle_fence); // idle_fence (0 if none)
                    state.pending_events.push(event.to_vec());
                }
            }

            Vec::new() // PresentPixmap has no reply
        }
        // NotifyMSC
        2 => {
            if data.len() < 40 {
                debug!("PresentNotifyMSC: request too short");
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    148, minor as u16, state.msb_first,
                );
            }
            let window = state.read_u32(data, 4);
            let serial = state.read_u32(data, 8);
            // target_msc at bytes 16-23, divisor at 24-31, remainder at 32-39
            // (64-bit values, we only use the low 32 bits)
            let _target_msc = state.read_u32(data, 16);
            let _divisor = state.read_u32(data, 24);
            let _remainder = state.read_u32(data, 32);

            debug!(
                "PresentNotifyMSC: window={:#x} serial={} msc={}",
                window, serial, state.present_msc
            );

            // Send PresentCompleteNotify immediately with current MSC.
            // Per spec, if target_msc == 0 && divisor == 0, notify immediately.
            // For a software server we always notify immediately since we have no
            // hardware vblank to wait for.
            let matching_subs: Vec<(u32, u32)> = state
                .present_subscriptions
                .iter()
                .filter(|(_, sub)| sub.window == window && (sub.event_mask & PRESENT_COMPLETE_NOTIFY_MASK) != 0)
                .map(|(&eid, sub)| (eid, sub.window))
                .collect();

            let msc = state.present_msc;
            let ust = state.server_start.elapsed().as_micros() as u64;
            for (event_id, _win) in matching_subs {
                // GenericEvent for PresentCompleteNotify (XGE wire format):
                // 40 bytes total => extra_length = 2 words.
                let mut event = vec![0u8; 40];
                event[0] = 35; // GenericEvent
                event[1] = 148; // Present extension major opcode
                state.write_u16(&mut event, 2, seq);
                state.write_u32(&mut event, 4, 2); // extra length in 4-byte words
                state.write_u16(&mut event, 8, 1); // CompleteNotify event type
                event[10] = 1; // kind = MSC
                event[11] = 0; // mode
                state.write_u32(&mut event, 12, event_id);
                state.write_u32(&mut event, 16, window);
                state.write_u32(&mut event, 20, serial);
                // UST: 64-bit microseconds since server start
                state.write_u32(&mut event, 24, (ust & 0xFFFF_FFFF) as u32);
                state.write_u32(&mut event, 28, (ust >> 32) as u32);
                // MSC: 64-bit
                state.write_u32(&mut event, 32, (msc & 0xFFFF_FFFF) as u32);
                state.write_u32(&mut event, 36, (msc >> 32) as u32);
                state.pending_events.push(event);
            }
            Vec::new()
        }
        // SelectInput
        3 => {
            if data.len() < 16 {
                debug!("PresentSelectInput: request too short ({} bytes)", data.len());
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    148, minor as u16, state.msb_first,
                );
            }
            let event_id = state.read_u32(data, 4);
            let window = state.read_u32(data, 8);
            let event_mask = state.read_u32(data, 12);

            debug!(
                "PresentSelectInput: event_id={:#x} window={:#x} event_mask={:#x}",
                event_id, window, event_mask
            );

            if event_mask == 0 {
                // Unsubscribe
                state.present_subscriptions.remove(&event_id);
            } else {
                state.present_subscriptions.insert(
                    event_id,
                    PresentSubscription { window, event_mask },
                );
            }
            Vec::new() // SelectInput has no reply
        }
        // QueryCapabilities
        4 => {
            let _target = if data.len() >= 8 {
                state.read_u32(data, 4)
            } else {
                0
            };
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, PRESENT_CAPABILITY_ASYNC); // async: we always present asynchronously
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled Present minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                148, minor as u16, state.msb_first,
            )
        }
    }
}

/// Send PresentConfigNotify to all subscribers of a window when it is reconfigured.
/// Call this from window configuration handlers (ConfigureWindow, ResizeWindow, etc.).
pub(crate) fn send_present_config_notify(
    state: &mut ClientState,
    window: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    off_x: i16,
    off_y: i16,
    pixmap_width: u16,
    pixmap_height: u16,
    pixmap_flags: u32,
) {
    let subs: Vec<u32> = state
        .present_subscriptions
        .iter()
        .filter(|(_, sub)| sub.window == window && (sub.event_mask & PRESENT_CONFIG_NOTIFY_MASK) != 0)
        .map(|(&eid, _)| eid)
        .collect();

    if subs.is_empty() {
        return;
    }

    let seq = state.sequence;

    for event_id in subs {
        // PresentConfigNotify uses the XGE (GenericEvent) wire format.
        // Full layout is 48 bytes = 32-byte header + 16 bytes extra.
        // extra_length = (48 - 32) / 4 = 4 words.
        let mut event = vec![0u8; 48];
        event[0] = 35; // GenericEvent
        event[1] = 148; // Present major opcode
        state.write_u16(&mut event, 2, seq);
        state.write_u32(&mut event, 4, 4); // extra length in 4-byte words
        state.write_u16(&mut event, 8, 3); // evtype = ConfigNotify
        // bytes 10-11: pad
        state.write_u32(&mut event, 12, event_id);
        state.write_u32(&mut event, 16, window);
        state.write_i16(&mut event, 20, x);
        state.write_i16(&mut event, 22, y);
        state.write_u16(&mut event, 24, width);
        state.write_u16(&mut event, 26, height);
        state.write_i16(&mut event, 28, off_x);
        state.write_i16(&mut event, 30, off_y);
        // Extended fields (bytes 32-47):
        state.write_u16(&mut event, 32, pixmap_width);
        state.write_u16(&mut event, 34, pixmap_height);
        state.write_u32(&mut event, 36, pixmap_flags);
        // bytes 40-47: pad (already zero)
        state.pending_events.push(event);
    }
}
