//! SHM sync, window sync, dirty flushing, and damage notification for ClientState.

use x11_web_protocol::DisplayUpdate;

use super::super::types::*;
use super::ClientState;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::damage::{self, ReportLevel};
use x11rb_protocol::protocol::xproto::{Rectangle, WindowClass};

impl ClientState {
    /// Sync SHM-backed pixmap data before reading.
    pub(crate) fn sync_shm_pixmap(&mut self, drawable: u32) {
        let target = self.resolve_drawable(drawable);
        let (shmseg, offset, width, height) = {
            let pix = match self.pixmaps.get(&target) {
                Some(p) => p,
                None => return,
            };
            let backing = match &pix.shm_backing {
                Some(b) => b,
                None => return,
            };
            (
                backing.shmseg,
                backing.offset,
                pix.width as usize,
                pix.height as usize,
            )
        };

        let seg = match self.shm_segments.get(&shmseg) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "sync_shm_pixmap: SHM segment {shmseg} not found for pixmap {target:#x}"
                );
                return;
            }
        };

        let bpp = 4usize;
        let stride = width * bpp;
        let total_bytes = stride * height;

        if offset + total_bytes > seg.size {
            tracing::warn!(
                "sync_shm_pixmap: out of bounds (offset={offset} + size={total_bytes} > seg.size={})",
                seg.size
            );
            return;
        }

        let Some(pix) = self.pixmaps.get_mut(&target) else {
            tracing::warn!("sync_shm_pixmap: pixmap {target:#x} not found");
            return;
        };
        let fb = &mut pix.framebuffer;
        let fb_data = fb.data_mut();
        unsafe {
            let src_ptr = seg.addr.add(offset);
            let copy_len = total_bytes.min(fb_data.len());
            std::ptr::copy_nonoverlapping(src_ptr, fb_data.as_mut_ptr(), copy_len);
        }
    }

    // -----------------------------------------------------------------------
    // Window sync and flush
    // -----------------------------------------------------------------------

    /// Sync local windows with the shared store.
    pub(crate) fn sync_windows(&mut self) {
        if let Ok(mut shared) = self.shared_windows.lock() {
            // 1. shared → local: pull in foreign windows we don't yet have, and
            //    drop foreign windows that have disappeared from shared (their
            //    owning client disconnected and removed them).
            let shared_keys: std::collections::HashSet<u32> = shared.keys().copied().collect();
            let foreign_to_remove: Vec<u32> = self
                .windows
                .iter()
                .filter(|(wid, win)| {
                    // A window is "foreign" if some other client created it.
                    // If that client disconnects in Destroy mode, shared_windows
                    // loses the entry; we have to drop our cached copy too,
                    // otherwise step 3 below will resurrect it.
                    !win.owner_client_id.is_empty()
                        && win.owner_client_id != self.client_id
                        && !shared_keys.contains(wid)
                })
                .map(|(wid, _)| *wid)
                .collect();
            for wid in foreign_to_remove {
                self.windows.remove(&wid);
            }

            for (&wid, shared_win) in shared.iter() {
                if let Some(local_win) = self.windows.get_mut(&wid) {
                    if shared_win.mapped && !local_win.mapped {
                        local_win.mapped = true;
                    }
                    if shared_win.redirected {
                        local_win.redirected = true;
                    }
                    for (&atom, val) in shared_win.properties.iter() {
                        local_win
                            .properties
                            .entry(atom)
                            .or_insert_with(|| val.clone());
                    }
                } else {
                    self.windows.insert(wid, shared_win.clone());
                }
            }

            // 2. local → shared: push our owned windows into the shared store.
            //    Don't re-publish foreign windows we cached — that would
            //    resurrect entries the owning client already destroyed.
            for (&wid, local_win) in self.windows.iter() {
                let is_mine = local_win.owner_client_id.is_empty()
                    || local_win.owner_client_id == self.client_id;
                if let Some(shared_win) = shared.get_mut(&wid) {
                    if local_win.mapped {
                        shared_win.mapped = true;
                    }
                    if local_win.redirected {
                        shared_win.redirected = true;
                    }
                    for (&atom, val) in local_win.properties.iter() {
                        shared_win.properties.insert(atom, val.clone());
                    }
                    shared_win.x = local_win.x;
                    shared_win.y = local_win.y;
                    shared_win.width = local_win.width;
                    shared_win.height = local_win.height;
                } else if is_mine {
                    shared.insert(wid, local_win.clone());
                }
            }

            // 3. Drop shared entries that no client has locally any more.
            //    Only safe for windows we own (otherwise we'd evict another
            //    client's window from shared just because we don't have it
            //    cached locally).
            let shared_ids: Vec<u32> = shared.keys().copied().collect();
            for wid in shared_ids {
                if !self.windows.contains_key(&wid) {
                    let owner_is_me = shared
                        .get(&wid)
                        .map(|w| {
                            w.owner_client_id.is_empty() || w.owner_client_id == self.client_id
                        })
                        .unwrap_or(false);
                    if owner_is_me {
                        shared.remove(&wid);
                    }
                }
            }
        }
    }

    /// Send dirty framebuffer regions for all mapped windows as PutImage updates.
    ///
    /// Per the COMPOSITE spec, redirected windows are NOT composited onto their
    /// parent — they remain as separate off-screen surfaces. A compositing
    /// manager reads their content via NameWindowPixmap and composites them
    /// using the RENDER extension.
    pub(crate) fn flush_dirty_windows(&mut self) {
        // Phase 1: Composite non-redirected child windows onto their top-level ancestor.
        // Redirected children keep their own framebuffer untouched.
        let children: Vec<(u32, u16, u16, bool)> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped
                    && w.framebuffer.is_dirty()
                    && w.parent != self.root_window
                    && w.parent != 0
                    && w.class == u16::from(WindowClass::INPUT_OUTPUT)
            })
            .map(|(_, w)| (w.id, w.width, w.height, w.redirected))
            .collect();

        for (child_id, cw, ch, redirected) in &children {
            if *redirected {
                // Redirected child: do NOT composite onto parent.
                // Just clear the dirty flag — the compositor reads via NameWindowPixmap.
                if let Some(child) = self.windows.get_mut(child_id) {
                    child.framebuffer.clear_dirty();
                }
                continue;
            }

            let mut target = *child_id;
            let mut off_x: i32 = 0;
            let mut off_y: i32 = 0;
            for _ in 0..10 {
                let (parent, wx, wy) = match self.windows.get(&target) {
                    Some(w) if w.parent != self.root_window && w.parent != 0 => {
                        (w.parent, w.x as i32, w.y as i32)
                    }
                    _ => break,
                };
                off_x += wx;
                off_y += wy;
                target = parent;
            }

            if target == *child_id {
                continue;
            }

            let pixels = self
                .windows
                .get(child_id)
                .map(|child| child.framebuffer.extract_pixels(0, 0, *cw, *ch));

            if let Some(pixels) = pixels {
                if let Some(ancestor) = self.windows.get_mut(&target) {
                    ancestor
                        .framebuffer
                        .put_image(off_x as i16, off_y as i16, *cw, *ch, &pixels);
                }
            }

            if let Some(child) = self.windows.get_mut(child_id) {
                child.framebuffer.clear_dirty();
            }
        }

        // Phase 2: Flush top-level (root children) windows.
        // All mapped, dirty top-level windows are forwarded to the frontend.
        // Even COMPOSITE-redirected windows are sent — our server IS the final
        // display stage, so the frontend must receive all window content.
        let window_ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped && w.framebuffer.is_dirty() && w.parent == self.root_window && w.class == u16::from(WindowClass::INPUT_OUTPUT)
            })
            .map(|(id, _)| *id)
            .collect();

        for wid in window_ids {
            let Some(wid_str) = self.window_uuid(wid) else {
                continue;
            };
            if let Some(win) = self.windows.get_mut(&wid) {
                if let Some((x, y, w, h, mut pixels)) = win.framebuffer.take_dirty_pixels() {
                    // Accumulate damage for DAMAGE subscribers regardless of redirect state.
                    let damage_rect = super::super::types::RegionRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    };
                    let damage_region =
                        super::super::types::XFixesRegion::from_rects(vec![damage_rect]);
                    let win_width = win.width;
                    let win_height = win.height;
                    let damage_matches: Vec<(u32, u8)> = self
                        .damage_regions
                        .iter_mut()
                        .filter(|(_, info)| info.drawable == wid)
                        .map(|(&did, info)| {
                            info.accumulated = info.accumulated.union(&damage_region);
                            (did, info.level)
                        })
                        .collect();

                    let bo = self.msb_first;
                    let seq = self.sequence;
                    for (damage_id, level) in damage_matches {
                        let event = serialize_event(&damage::NotifyEvent {
                            response_type: 91,
                            level: ReportLevel::from(level),
                            sequence: seq,
                            drawable: wid,
                            damage: damage_id,
                            timestamp: 0,
                            area: Rectangle { x: x as i16, y: y as i16, width: w, height: h },
                            geometry: Rectangle { x: 0, y: 0, width: win_width, height: win_height },
                        }, bo);
                        self.pending_events.push(event);
                    }

                    // Apply shape clipping: mask pixels outside the bounding/clip shape
                    if let Some(shape) = win.effective_render_shape() {
                        for py in 0..h as i16 {
                            for px in 0..w as i16 {
                                if !point_in_shape(shape, x + px, y + py) {
                                    let offset = (py as usize * w as usize + px as usize) * 4;
                                    if offset + 3 < pixels.len() {
                                        pixels[offset] = 0; // B
                                        pixels[offset + 1] = 0; // G
                                        pixels[offset + 2] = 0; // R
                                        pixels[offset + 3] = 0; // A
                                    }
                                }
                            }
                        }
                    }

                    let owner = if win.owner_client_id.is_empty() {
                        self.client_id.clone()
                    } else {
                        win.owner_client_id.clone()
                    };
                    let _ = self.update_tx.send((
                        owner,
                        DisplayUpdate::PutImage {
                            window_id: wid_str.clone(),
                            x,
                            y,
                            width: w,
                            height: h,
                            data: pixels,
                        },
                    ));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Damage notification
    // -----------------------------------------------------------------------

    /// Queue DamageNotify events for subscriptions on the given drawable.
    ///
    /// Per the DAMAGE spec, each notification also accumulates the damaged
    /// rectangle into the DamageInfo's region so that DamageSubtract can
    /// compute proper region differences.
    pub(crate) fn notify_damage(&mut self, drawable: u32, x: i16, y: i16, width: u16, height: u16) {
        let resolved = self.resolve_drawable(drawable);

        // Accumulate damage into matching DamageInfo regions.
        let damage_rect = super::super::types::RegionRect {
            x,
            y,
            width,
            height,
        };
        let damage_region = super::super::types::XFixesRegion::from_rects(vec![damage_rect]);

        let matches: Vec<(u32, u8)> = self
            .damage_regions
            .iter_mut()
            .filter(|(_, info)| info.drawable == resolved)
            .map(|(&did, info)| {
                info.accumulated = info.accumulated.union(&damage_region);
                (did, info.level)
            })
            .collect();

        let bo = self.msb_first;
        let seq = self.sequence;
        let timestamp = self.timestamp();

        // Resolve the drawable geometry for the geometry field.
        let (geom_x, geom_y, geom_w, geom_h) = if let Some(win) = self.windows.get(&resolved) {
            (win.x, win.y, win.width, win.height)
        } else {
            (x, y, width, height)
        };

        for (damage_id, level) in matches {
            let event = serialize_event(&damage::NotifyEvent {
                response_type: 91,
                level: ReportLevel::from(level),
                sequence: seq,
                drawable: resolved,
                damage: damage_id,
                timestamp,
                area: Rectangle { x, y, width, height },
                geometry: Rectangle { x: geom_x, y: geom_y, width: geom_w, height: geom_h },
            }, bo);
            self.pending_events.push(event);
        }
    }
}
