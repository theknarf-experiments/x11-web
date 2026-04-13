//! Drawable, GC, and pixmap resolution helpers for ClientState.

use crate::framebuffer::Framebuffer;

use super::super::types::*;
use super::ClientState;

impl ClientState {
    /// Map a color value for the drawable's depth.
    pub(crate) fn map_color_for_drawable(&self, drawable: u32, color: u32) -> u32 {
        let depth = if let Some(p) = self.pixmaps.get(&drawable) {
            p.depth
        } else if let Some(w) = self.windows.get(&drawable) {
            w.depth
        } else {
            24
        };
        if depth <= 1 {
            if color != 0 {
                0xFFFFFF
            } else {
                0x000000
            }
        } else {
            color
        }
    }

    /// Get a mutable reference to the framebuffer for a drawable.
    ///
    /// For pixmaps with alias_window set (COMPOSITE NameWindowPixmap), this
    /// returns the aliased window's live framebuffer so that operations on the
    /// pixmap directly modify the window's off-screen surface.
    pub(crate) fn get_framebuffer_mut(&mut self, drawable: u32) -> Option<&mut Framebuffer> {
        // Resolve alias: if this is a NameWindowPixmap, redirect to the window.
        let target = self
            .pixmaps
            .get(&drawable)
            .and_then(|p| p.alias_window)
            .unwrap_or(drawable);
        if let Some(win) = self.windows.get_mut(&target) {
            return Some(&mut win.framebuffer);
        }
        if let Some(pix) = self.pixmaps.get_mut(&target) {
            return Some(&mut pix.framebuffer);
        }
        None
    }

    /// Get the size of a drawable (window or pixmap), including cross-connection lookup.
    pub(crate) fn get_drawable_size(&self, drawable: u32) -> Option<(u16, u16)> {
        if let Some(win) = self.windows.get(&drawable) {
            return Some((win.width, win.height));
        }
        if let Some(pix) = self.pixmaps.get(&drawable) {
            return Some((pix.width, pix.height));
        }
        // Cross-connection pixmap lookup
        if let Ok(shared) = self.shared_pixmaps.lock() {
            if let Some(meta) = shared.get(&drawable) {
                return Some((meta.width, meta.height));
            }
        }
        None
    }

    /// Check if a drawable exists (local windows, local pixmaps, or shared pixmaps).
    #[allow(dead_code)]
    pub(crate) fn drawable_exists(&self, drawable: u32) -> bool {
        self.windows.contains_key(&drawable)
            || self.pixmaps.contains_key(&drawable)
            || self
                .shared_pixmaps
                .lock()
                .ok()
                .is_some_and(|s| s.contains_key(&drawable))
    }

    /// Look up a GC by ID, including cross-connection shared GCs.
    #[allow(dead_code)]
    pub(crate) fn get_gc(&self, gc_id: u32) -> Option<GcState> {
        if let Some(gc) = self.gcs.get(&gc_id) {
            return Some(gc.clone());
        }
        // Cross-connection GC lookup
        if let Ok(shared) = self.shared_gcs.lock() {
            return shared.get(&gc_id).cloned();
        }
        None
    }

    /// Resolve a drawable ID to the actual drawable ID (following aliases).
    pub(crate) fn resolve_drawable(&self, drawable: u32) -> u32 {
        if let Some(pix) = self.pixmaps.get(&drawable) {
            if let Some(alias_wid) = pix.alias_window {
                return alias_wid;
            }
        }
        drawable
    }

    /// Get a read-only reference to the framebuffer for a drawable (window or pixmap).
    ///
    /// For pixmaps with alias_window set (COMPOSITE NameWindowPixmap), this
    /// returns the aliased window's live framebuffer.
    pub(crate) fn get_framebuffer(
        &self,
        drawable: u32,
    ) -> Option<&crate::framebuffer::Framebuffer> {
        // Resolve alias: if this is a NameWindowPixmap, redirect to the window.
        let target = self
            .pixmaps
            .get(&drawable)
            .and_then(|p| p.alias_window)
            .unwrap_or(drawable);
        if let Some(win) = self.windows.get(&target) {
            return Some(&win.framebuffer);
        }
        if let Some(pix) = self.pixmaps.get(&target) {
            return Some(&pix.framebuffer);
        }
        None
    }

    /// Extract pixels from a window with IncludeInferiors semantics.
    ///
    /// When `subwindow_mode == 1` (IncludeInferiors), the source pixels
    /// include the composited content of all mapped child windows drawn
    /// on top of the parent. This is used by CopyArea and GetImage when
    /// the GC has IncludeInferiors set.
    pub(crate) fn extract_pixels_include_inferiors(
        &self,
        window_id: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Vec<u8> {
        let w = width as usize;
        let h = height as usize;

        // Start with the parent window's own pixels
        let mut result = if let Some(win) = self.windows.get(&window_id) {
            win.framebuffer.extract_pixels(x, y, width, height)
        } else if let Some(pix) = self.pixmaps.get(&window_id) {
            pix.framebuffer.extract_pixels(x, y, width, height)
        } else {
            return vec![0u8; w * h * 4];
        };

        // Composite mapped child windows on top (in stacking order, bottom to top)
        if let Some(parent_win) = self.windows.get(&window_id) {
            let children = parent_win.children_order.clone();
            for child_id in children {
                if let Some(child) = self.windows.get(&child_id) {
                    if !child.mapped {
                        continue;
                    }
                    // Child position relative to parent
                    let cx = child.x as i32;
                    let cy = child.y as i32;
                    let cw = child.width as usize;
                    let ch = child.height as usize;
                    let child_data = child.framebuffer.data();

                    // Blit child onto result at the correct offset
                    for row in 0..ch {
                        let dst_row = cy + row as i32 - y as i32;
                        if dst_row < 0 || dst_row >= h as i32 {
                            continue;
                        }
                        for col in 0..cw {
                            let dst_col = cx + col as i32 - x as i32;
                            if dst_col < 0 || dst_col >= w as i32 {
                                continue;
                            }
                            let src_off = (row * cw + col) * 4;
                            let dst_off = (dst_row as usize * w + dst_col as usize) * 4;
                            if src_off + 3 < child_data.len() && dst_off + 3 < result.len() {
                                // Alpha-composite: child pixels override parent
                                // (X11 windows are opaque by default)
                                result[dst_off] = child_data[src_off];
                                result[dst_off + 1] = child_data[src_off + 1];
                                result[dst_off + 2] = child_data[src_off + 2];
                                result[dst_off + 3] = child_data[src_off + 3];
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Resolve a pixmap ID into a `ClipMaskBitmap`.
    ///
    /// Per the X11 spec, the clip mask pixmap must be depth-1.  We convert
    /// the pixmap's framebuffer (which stores RGBA) to a 1-bit-per-pixel
    /// bitmap where any non-zero pixel maps to a 1-bit.
    /// Returns `None` if the pixmap ID is 0 (None) or doesn't exist.
    pub(crate) fn resolve_clip_mask(
        &self,
        pixmap_id: u32,
    ) -> Option<super::super::types::ClipMaskBitmap> {
        if pixmap_id == 0 {
            return None;
        }
        let pix = self.pixmaps.get(&pixmap_id)?;
        let w = pix.width as usize;
        let h = pix.height as usize;
        let stride = w.div_ceil(8);
        let mut bits = vec![0u8; stride * h];
        let data = pix.framebuffer.data();
        for y in 0..h {
            for x in 0..w {
                let off = (y * w + x) * 4;
                // A pixel is "set" (1-bit) if any channel is non-zero.
                // For depth-1 pixmaps the foreground is stored as non-zero.
                let pixel_set = if off + 3 < data.len() {
                    data[off] != 0 || data[off + 1] != 0 || data[off + 2] != 0
                } else {
                    false
                };
                if pixel_set {
                    bits[y * stride + x / 8] |= 1 << (x % 8);
                }
            }
        }
        Some(super::super::types::ClipMaskBitmap {
            width: pix.width,
            height: pix.height,
            bits,
        })
    }

    // -----------------------------------------------------------------------
    // Shared resource management
    // -----------------------------------------------------------------------

    /// Register a pixmap in the shared registry for cross-connection access.
    pub(crate) fn register_shared_pixmap(
        &self,
        pixmap_id: u32,
        width: u16,
        height: u16,
        depth: u8,
    ) {
        if let Ok(mut shared) = self.shared_pixmaps.lock() {
            shared.insert(
                pixmap_id,
                super::super::types::SharedPixmapMeta {
                    width,
                    height,
                    depth,
                    owner_client_id: self.client_id.clone(),
                },
            );
        }
    }

    /// Unregister a pixmap from the shared registry.
    pub(crate) fn unregister_shared_pixmap(&self, pixmap_id: u32) {
        if let Ok(mut shared) = self.shared_pixmaps.lock() {
            shared.remove(&pixmap_id);
        }
        if let Ok(mut fbs) = self.shared_pixmap_fbs.lock() {
            fbs.remove(&pixmap_id);
        }
    }

    /// Register a GC in the shared registry for cross-connection access.
    pub(crate) fn register_shared_gc(&self, gc_id: u32) {
        if let Some(gc) = self.gcs.get(&gc_id) {
            if let Ok(mut shared) = self.shared_gcs.lock() {
                shared.insert(gc_id, gc.clone());
            }
        }
    }

    /// Unregister a GC from the shared registry.
    pub(crate) fn unregister_shared_gc(&self, gc_id: u32) {
        if let Ok(mut shared) = self.shared_gcs.lock() {
            shared.remove(&gc_id);
        }
    }

    /// Unregister all shared resources owned by this client.
    pub(crate) fn unregister_all_shared_resources(&self) {
        let client_id = &self.client_id;
        if let Ok(mut shared) = self.shared_pixmaps.lock() {
            shared.retain(|_, meta| meta.owner_client_id != *client_id);
        }
        if let Ok(mut fbs) = self.shared_pixmap_fbs.lock() {
            // Remove pixmap FBs owned by this client by checking shared_pixmaps
            let local_pix_ids: Vec<u32> = self.pixmaps.keys().copied().collect();
            for id in local_pix_ids {
                fbs.remove(&id);
            }
        }
        if let Ok(mut shared) = self.shared_gcs.lock() {
            let local_gc_ids: Vec<u32> = self.gcs.keys().copied().collect();
            for id in local_gc_ids {
                shared.remove(&id);
            }
        }
    }
}
