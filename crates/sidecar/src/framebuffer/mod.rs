use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

mod drawing;
mod shapes;
#[cfg(test)]
mod tests;

/// A server-side pixel buffer using pure Rust (no pixman).
/// Format: A8R8G8B8 (4 bytes per pixel, BGRA in memory on little-endian).
#[derive(Clone)]
pub struct Framebuffer {
    data: Vec<u8>,
    width: u32,
    height: u32,
    stride: usize,
    /// Dirty region (x, y, w, h) that needs to be sent to the frontend.
    dirty: Option<(i32, i32, u32, u32)>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let stride = width as usize * 4;
        let data = vec![0u8; stride * height as usize];
        Self {
            data,
            width,
            height,
            stride,
            dirty: None,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Resize the framebuffer, preserving existing content where possible.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width == self.width && new_height == self.height {
            return;
        }
        let new_stride = new_width as usize * 4;
        let mut new_data = vec![0u8; new_stride * new_height as usize];
        // Copy over existing pixels
        let copy_w = self.width.min(new_width) as usize * 4;
        let copy_h = self.height.min(new_height) as usize;
        for y in 0..copy_h {
            let src_off = y * self.stride;
            let dst_off = y * new_stride;
            new_data[dst_off..dst_off + copy_w]
                .copy_from_slice(&self.data[src_off..src_off + copy_w]);
        }
        self.data = new_data;
        self.width = new_width;
        self.height = new_height;
        self.stride = new_stride;
        self.mark_dirty(0, 0, new_width, new_height);
    }

    /// Resize the framebuffer with X11 bit-gravity pixel preservation.
    ///
    /// `gravity` values:
    ///   0 = Forget (discard content), 1 = NorthWest, 2 = North, 3 = NorthEast,
    ///   4 = West, 5 = Center, 6 = East, 7 = SouthWest, 8 = South,
    ///   9 = SouthEast, 10 = Static
    ///
    /// For Forget (0), existing pixels are discarded and the caller should
    /// generate Expose events. For all other values, old pixels are copied to
    /// the position dictated by the gravity so that the corresponding corner,
    /// edge-center, or center of the old content aligns with the same reference
    /// point in the new geometry.
    pub fn resize_with_gravity(&mut self, new_width: u32, new_height: u32, gravity: u8) {
        if new_width == 0 || new_height == 0 {
            return;
        }
        if new_width == self.width && new_height == self.height {
            return;
        }

        let old_w = self.width;
        let old_h = self.height;
        let dw = new_width as i32 - old_w as i32;
        let dh = new_height as i32 - old_h as i32;

        // For Forget gravity, just allocate a blank buffer — no pixel copy.
        if gravity == 0 {
            self.width = new_width;
            self.height = new_height;
            self.stride = new_width as usize * 4;
            self.data = vec![0u8; self.stride * new_height as usize];
            self.mark_dirty(0, 0, new_width, new_height);
            return;
        }

        let old_data = std::mem::take(&mut self.data);
        let old_stride = self.stride;

        self.width = new_width;
        self.height = new_height;
        self.stride = new_width as usize * 4;
        self.data = vec![0u8; self.stride * new_height as usize];

        // Destination offset: where in the new buffer to place old content.
        // A positive dx means old content shifts right; positive dy means down.
        let (dx, dy): (i32, i32) = match gravity {
            1 => (0, 0),           // NorthWest
            2 => (dw / 2, 0),      // North
            3 => (dw, 0),          // NorthEast
            4 => (0, dh / 2),      // West
            5 => (dw / 2, dh / 2), // Center
            6 => (dw, dh / 2),     // East
            7 => (0, dh),          // SouthWest
            8 => (dw / 2, dh),     // South
            9 => (dw, dh),         // SouthEast
            10 => (0, 0),          // Static
            _ => (0, 0),
        };

        // Row-by-row copy with bounds checking.
        // src_y is the row in the old buffer, dst_y = src_y + dy is in the new buffer.
        // Similarly for columns.
        let src_x_start = (-dx).max(0) as usize;
        let dst_x_start = dx.max(0) as usize;
        let copy_w = (old_w as usize)
            .min(new_width as usize - dst_x_start)
            .saturating_sub(src_x_start);

        for src_y in 0..old_h as usize {
            let dst_y = src_y as i32 + dy;
            if dst_y < 0 || dst_y >= new_height as i32 {
                continue;
            }
            if copy_w == 0 {
                continue;
            }
            let src_off = src_y * old_stride + src_x_start * 4;
            let dst_off = dst_y as usize * self.stride + dst_x_start * 4;
            let byte_len = copy_w * 4;
            if src_off + byte_len <= old_data.len() && dst_off + byte_len <= self.data.len() {
                self.data[dst_off..dst_off + byte_len]
                    .copy_from_slice(&old_data[src_off..src_off + byte_len]);
            }
        }

        self.mark_dirty(0, 0, new_width, new_height);
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Mark a rectangular region as dirty.
    pub fn mark_dirty(&mut self, x: i32, y: i32, w: u32, h: u32) {
        let x = x.max(0);
        let y = y.max(0);
        let w = w.min((self.width as i32 - x).max(0) as u32);
        let h = h.min((self.height as i32 - y).max(0) as u32);
        if w == 0 || h == 0 {
            return;
        }

        self.dirty = Some(match self.dirty {
            None => (x, y, w, h),
            Some((dx, dy, dw, dh)) => {
                let min_x = dx.min(x);
                let min_y = dy.min(y);
                let max_x = (dx + dw as i32).max(x + w as i32);
                let max_y = (dy + dh as i32).max(y + h as i32);
                (min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32)
            }
        });
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_some()
    }

    /// Clear the dirty flag without extracting pixels.
    pub fn clear_dirty(&mut self) {
        self.dirty = None;
    }

    /// Extract the dirty region and clear the dirty flag.
    pub fn take_dirty_pixels(&mut self) -> Option<(i16, i16, u16, u16, Vec<u8>)> {
        let (dx, dy, dw, dh) = self.dirty.take()?;
        if dw == 0 || dh == 0 {
            return None;
        }

        let dx = dx.max(0).min(self.width as i32 - 1);
        let dy = dy.max(0).min(self.height as i32 - 1);
        let dw = dw.min((self.width as i32 - dx) as u32);
        let dh = dh.min((self.height as i32 - dy) as u32);

        let mut pixels = Vec::with_capacity(dw as usize * dh as usize * 4);
        for row in 0..dh as usize {
            let y_off = (dy as usize + row) * self.stride;
            let x_off = dx as usize * 4;
            let end = y_off + x_off + dw as usize * 4;
            if end <= self.data.len() {
                pixels.extend_from_slice(&self.data[y_off + x_off..end]);
            }
        }

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        let _ = encoder.write_all(&pixels);
        let compressed = encoder.finish().unwrap_or_else(|e| {
            tracing::warn!("framebuffer compression failed, sending uncompressed: {e}");
            pixels
        });

        Some((dx as i16, dy as i16, dw as u16, dh as u16, compressed))
    }

    /// Fill a rectangle with a solid color (0x00RRGGBB format).
    pub fn fill_rect(&mut self, x: i16, y: i16, width: u16, height: u16, color: u32) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        let pixel = [b, g, r, 0xFF];

        // Pre-build a row of pixels
        let row_start = (x as i32).max(0) as usize;
        let row_end = ((x as i32 + width as i32).min(self.width as i32)).max(0) as usize;
        if row_start >= row_end {
            return;
        }
        let row_len = row_end - row_start;

        let mut row_buf = vec![0u8; row_len * 4];
        for i in 0..row_len {
            row_buf[i * 4..i * 4 + 4].copy_from_slice(&pixel);
        }

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let dst_off = dy as usize * self.stride + row_start * 4;
            if dst_off + row_len * 4 <= self.data.len() {
                self.data[dst_off..dst_off + row_len * 4].copy_from_slice(&row_buf);
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Fill a rectangle applying an X11 GC raster operation.
    ///
    /// For GXcopy (3) this behaves identically to [`fill_rect`]. For other
    /// operations the existing pixel value is read, combined with `color`
    /// through `apply_gc_function`, and written back. `plane_mask` selects
    /// which bit-planes are affected (0xFFFFFFFF means all).
    pub fn fill_rect_rop(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        color: u32,
        function: u8,
        plane_mask: u32,
    ) {
        // Fast-path: GXcopy with full plane mask is the common case.
        if function == 3 && plane_mask == 0xFFFFFFFF {
            self.fill_rect(x, y, width, height, color);
            return;
        }

        // GXnoop - nothing to do.
        if function == 5 {
            return;
        }

        let row_start = (x as i32).max(0) as usize;
        let row_end = ((x as i32 + width as i32).min(self.width as i32)).max(0) as usize;
        if row_start >= row_end {
            return;
        }
        let row_len = row_end - row_start;

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let base_off = dy as usize * self.stride + row_start * 4;
            if base_off + row_len * 4 > self.data.len() {
                continue;
            }
            for i in 0..row_len {
                let off = base_off + i * 4;
                // Read existing pixel as 0x00RRGGBB
                let dst = {
                    let b = self.data[off] as u32;
                    let g = self.data[off + 1] as u32;
                    let r = self.data[off + 2] as u32;
                    (r << 16) | (g << 8) | b
                };
                let result = apply_gc_function(function, color, dst);
                // Apply plane mask: affected planes come from result,
                // unaffected planes keep the dst value.
                let masked = (result & plane_mask) | (dst & !plane_mask);
                self.data[off] = (masked & 0xFF) as u8; // B
                self.data[off + 1] = ((masked >> 8) & 0xFF) as u8; // G
                self.data[off + 2] = ((masked >> 16) & 0xFF) as u8; // R
                self.data[off + 3] = 0xFF; // A
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Put raw pixel data into the framebuffer.
    pub fn put_image(&mut self, x: i16, y: i16, width: u16, height: u16, data: &[u8]) {
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }

        let src_stride = width as usize * 4;
        if data.len() < src_stride * height as usize {
            return;
        }

        for row in 0..height as usize {
            let dy = y as i32 + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let col_start = if x < 0 { (-x) as usize } else { 0 };
            let col_end = (width as usize)
                .min((self.width as i32 - x.max(0) as i32).max(0) as usize + col_start);
            if col_start >= col_end {
                continue;
            }

            let dx_start = (x as i32 + col_start as i32).max(0) as usize;
            let src_off = row * src_stride + col_start * 4;
            let dst_off = dy as usize * self.stride + dx_start * 4;
            let copy_len = (col_end - col_start) * 4;

            if dst_off + copy_len <= self.data.len() && src_off + copy_len <= data.len() {
                self.data[dst_off..dst_off + copy_len]
                    .copy_from_slice(&data[src_off..src_off + copy_len]);
            }
        }

        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Put raw pixel data using Over compositing (respects alpha channel).
    pub fn put_image_over(&mut self, x: i16, y: i16, width: u16, height: u16, data: &[u8]) {
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }

        let src_stride = width as usize * 4;
        if data.len() < src_stride * height as usize {
            return;
        }

        for row in 0..height as usize {
            let dy = y as i32 + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..width as usize {
                let dx = x as i32 + col as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let src_off = row * src_stride + col * 4;
                let src_a = data[src_off + 3];
                if src_a == 0 {
                    continue;
                }
                let dst_off = dy as usize * self.stride + dx as usize * 4;
                if dst_off + 3 >= self.data.len() {
                    continue;
                }
                if src_a == 0xFF {
                    self.data[dst_off..dst_off + 4].copy_from_slice(&data[src_off..src_off + 4]);
                } else {
                    let sa = src_a as u32;
                    let da = 255 - sa;
                    for c in 0..3 {
                        let s = data[src_off + c] as u32;
                        let d = self.data[dst_off + c] as u32;
                        self.data[dst_off + c] = ((s * sa + d * da) / 255) as u8;
                    }
                    self.data[dst_off + 3] = 0xFF;
                }
            }
        }

        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Composite ARGB source over destination, applying GC function and plane_mask.
    /// For PolyText8/16 which uses transparent background text rendering.
    pub fn put_image_over_gc(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        data: &[u8],
        function: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }
        // Fast path: GXcopy with no clipping and full plane_mask
        if function == 3 && plane_mask == 0xFFFFFFFF && clip_rects.is_empty() {
            self.put_image_over(x, y, width, height, data);
            return;
        }

        let src_stride = width as usize * 4;
        if data.len() < src_stride * height as usize {
            return;
        }

        for row in 0..height as usize {
            let dy = y as i32 + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..width as usize {
                let dx = x as i32 + col as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let src_off = row * src_stride + col * 4;
                let src_a = data[src_off + 3];
                if src_a == 0 {
                    continue;
                }

                // Check clip rects
                if !clip_rects.is_empty() && !point_in_clip_rects(dx, dy, clip_rects) {
                    continue;
                }

                let dst_off = dy as usize * self.stride + dx as usize * 4;
                if dst_off + 3 >= self.data.len() {
                    continue;
                }

                // Alpha-blend source over destination first
                let (sb, sg, sr) = if src_a == 0xFF {
                    (data[src_off], data[src_off + 1], data[src_off + 2])
                } else {
                    let sa = src_a as u32;
                    let da = 255 - sa;
                    let b =
                        ((data[src_off] as u32 * sa + self.data[dst_off] as u32 * da) / 255) as u8;
                    let g = ((data[src_off + 1] as u32 * sa + self.data[dst_off + 1] as u32 * da)
                        / 255) as u8;
                    let r = ((data[src_off + 2] as u32 * sa + self.data[dst_off + 2] as u32 * da)
                        / 255) as u8;
                    (b, g, r)
                };

                // Apply GC function and plane_mask
                let src_color = (sr as u32) << 16 | (sg as u32) << 8 | sb as u32;
                let dst_color = u32::from_le_bytes([
                    self.data[dst_off],
                    self.data[dst_off + 1],
                    self.data[dst_off + 2],
                    self.data[dst_off + 3],
                ]) & 0x00FFFFFF;
                let result = apply_gc_function(function, src_color, dst_color);
                let masked = (result & plane_mask) | (dst_color & !plane_mask);
                self.data[dst_off] = (masked & 0xFF) as u8;
                self.data[dst_off + 1] = ((masked >> 8) & 0xFF) as u8;
                self.data[dst_off + 2] = ((masked >> 16) & 0xFF) as u8;
                self.data[dst_off + 3] = 0xFF;
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Put an image applying GC function, plane_mask, and clip rectangles.
    /// For ImageText8/16 which uses opaque background text rendering.
    pub fn put_image_gc(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        data: &[u8],
        function: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }
        // Fast path
        if function == 3 && plane_mask == 0xFFFFFFFF && clip_rects.is_empty() {
            self.put_image(x, y, width, height, data);
            return;
        }

        let src_stride = width as usize * 4;
        if data.len() < src_stride * height as usize {
            return;
        }

        for row in 0..height as usize {
            let dy = y as i32 + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..width as usize {
                let dx = x as i32 + col as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                if !clip_rects.is_empty() && !point_in_clip_rects(dx, dy, clip_rects) {
                    continue;
                }
                let src_off = row * src_stride + col * 4;
                let src_color = (data[src_off + 2] as u32) << 16
                    | (data[src_off + 1] as u32) << 8
                    | data[src_off] as u32;
                let dst_off = dy as usize * self.stride + dx as usize * 4;
                if dst_off + 3 >= self.data.len() {
                    continue;
                }
                let dst_color = u32::from_le_bytes([
                    self.data[dst_off],
                    self.data[dst_off + 1],
                    self.data[dst_off + 2],
                    self.data[dst_off + 3],
                ]) & 0x00FFFFFF;
                let result = apply_gc_function(function, src_color, dst_color);
                let masked = (result & plane_mask) | (dst_color & !plane_mask);
                self.data[dst_off] = (masked & 0xFF) as u8;
                self.data[dst_off + 1] = ((masked >> 8) & 0xFF) as u8;
                self.data[dst_off + 2] = ((masked >> 16) & 0xFF) as u8;
                self.data[dst_off + 3] = 0xFF;
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Copy a region within the same framebuffer.
    pub fn copy_area_self(
        &mut self,
        src_x: i16,
        src_y: i16,
        dst_x: i16,
        dst_y: i16,
        width: u16,
        height: u16,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let pixels = self.extract_pixels(src_x, src_y, width, height);
        self.put_image(dst_x, dst_y, width, height, &pixels);
    }

    /// Extract raw pixel data from a region.
    pub fn extract_pixels(&self, x: i16, y: i16, width: u16, height: u16) -> Vec<u8> {
        if width == 0 || height == 0 {
            return Vec::new();
        }

        let w = width as usize;
        let h = height as usize;
        let mut pixels = vec![0u8; w * h * 4];

        for row in 0..h {
            let sy = y as i32 + row as i32;
            if sy < 0 || sy >= self.height as i32 {
                continue;
            }
            let sx = (x as i32).max(0);
            let avail = (self.width as i32 - sx).max(0) as usize;
            let copy_w = w.min(avail) * 4;
            if copy_w == 0 {
                continue;
            }
            let src_off = sy as usize * self.stride + sx as usize * 4;
            if src_off + copy_w > self.data.len() {
                continue;
            }
            let dst_start = row * w * 4;
            pixels[dst_start..dst_start + copy_w]
                .copy_from_slice(&self.data[src_off..src_off + copy_w]);
        }

        pixels
    }

    /// Extract the entire framebuffer as RGBA (from BGRA internal format).
    pub fn extract_rgba(&self) -> Vec<u8> {
        let pixel_count = (self.width as usize) * (self.height as usize);
        let mut rgba = vec![0u8; pixel_count * 4];
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let src = y * self.stride + x * 4;
                let dst = (y * self.width as usize + x) * 4;
                if src + 3 < self.data.len() && dst + 3 < rgba.len() {
                    rgba[dst] = self.data[src + 2]; // R (from BGRA B offset)
                    rgba[dst + 1] = self.data[src + 1]; // G
                    rgba[dst + 2] = self.data[src]; // B
                    rgba[dst + 3] = self.data[src + 3]; // A
                }
            }
        }
        rgba
    }

    /// Draw a single pixel at (x, y) with the given color and GC function.
    /// gc_func: 0=Clear, 1=And, 2=AndReverse, 3=Copy, 4=AndInverted,
    ///          5=Noop, 6=Xor, 7=Or, 8=Nor, 9=Equiv, 10=Invert,
    ///          11=OrReverse, 12=CopyInverted, 13=OrInverted, 14=Nand, 15=Set
    pub fn draw_point_with_func(&mut self, x: i32, y: i32, color: u32, gc_func: u8) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let off = y as usize * self.stride + x as usize * 4;
        if off + 3 >= self.data.len() {
            return;
        }

        let src = color;
        let dst = u32::from_le_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]);

        let result = apply_gc_function(gc_func, src, dst);

        let r = ((result >> 16) & 0xFF) as u8;
        let g = ((result >> 8) & 0xFF) as u8;
        let b = (result & 0xFF) as u8;
        self.data[off] = b;
        self.data[off + 1] = g;
        self.data[off + 2] = r;
        self.data[off + 3] = 0xFF;
        self.mark_dirty(x, y, 1, 1);
    }

    /// Draw a single pixel with GXcopy (the common case).
    pub fn draw_point(&mut self, x: i32, y: i32, color: u32) {
        self.draw_point_with_func(x, y, color, 3);
    }

    /// Draw a point applying both GC function and plane_mask.
    pub fn draw_point_with_func_masked(
        &mut self,
        x: i32,
        y: i32,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
    ) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let off = y as usize * self.stride + x as usize * 4;
        if off + 3 >= self.data.len() {
            return;
        }
        let src = color;
        let dst = u32::from_le_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]);
        let result = apply_gc_function(gc_func, src, dst);
        let masked = (result & plane_mask) | (dst & !plane_mask);
        self.data[off] = (masked & 0xFF) as u8;
        self.data[off + 1] = ((masked >> 8) & 0xFF) as u8;
        self.data[off + 2] = ((masked >> 16) & 0xFF) as u8;
        self.data[off + 3] = 0xFF;
        self.mark_dirty(x, y, 1, 1);
    }
}

/// Dash pattern state machine for dashed line drawing.
pub(crate) struct DashState {
    pub(crate) pattern: Vec<u8>,
    pub(crate) index: usize,
    pub(crate) remaining: u32,
    pub(crate) on: bool,
}

impl DashState {
    pub(crate) fn new(pattern: &[u8], offset: u16) -> Self {
        if pattern.is_empty() {
            return Self {
                pattern: vec![1],
                index: 0,
                remaining: 1,
                on: true,
            };
        }
        let pat: Vec<u8> = pattern.to_vec();
        // Advance past offset
        let mut idx = 0usize;
        let mut rem = pat[0] as u32;
        let mut on = true;
        let mut skip = offset as u32;
        while skip > 0 {
            if skip < rem {
                rem -= skip;
                break;
            }
            skip -= rem;
            idx = (idx + 1) % pat.len();
            rem = pat[idx] as u32;
            on = !on;
        }
        Self {
            pattern: pat,
            index: idx,
            remaining: rem,
            on,
        }
    }

    pub(crate) fn is_on(&self) -> bool {
        self.on
    }

    pub(crate) fn advance(&mut self) {
        self.remaining -= 1;
        if self.remaining == 0 {
            self.index = (self.index + 1) % self.pattern.len();
            self.remaining = self.pattern[self.index] as u32;
            if self.remaining == 0 {
                self.remaining = 1; // avoid infinite loop on 0-length dash
            }
            self.on = !self.on;
        }
    }
}

/// Check if a point is inside any of the clip rectangles.
pub(crate) fn point_in_clip_rects(x: i32, y: i32, rects: &[(i16, i16, u16, u16)]) -> bool {
    for &(rx, ry, rw, rh) in rects {
        if x >= rx as i32
            && x < rx as i32 + rw as i32
            && y >= ry as i32
            && y < ry as i32 + rh as i32
        {
            return true;
        }
    }
    false
}

/// Apply X11 GC raster operation function to source and destination pixels.
pub fn apply_gc_function(func: u8, src: u32, dst: u32) -> u32 {
    match func {
        0 => 0,             // GXclear
        1 => src & dst,     // GXand
        2 => src & !dst,    // GXandReverse
        3 => src,           // GXcopy
        4 => !src & dst,    // GXandInverted
        5 => dst,           // GXnoop
        6 => src ^ dst,     // GXxor
        7 => src | dst,     // GXor
        8 => !(src | dst),  // GXnor
        9 => !(src ^ dst),  // GXequiv
        10 => !dst,         // GXinvert
        11 => src | !dst,   // GXorReverse
        12 => !src,         // GXcopyInverted
        13 => !src | dst,   // GXorInverted
        14 => !(src & dst), // GXnand
        15 => 0xFFFFFFFF,   // GXset
        _ => src,           // default to copy
    }
}

pub(crate) fn point_in_arc(dx: f64, dy: f64, start: f64, extent: f64) -> bool {
    let angle = (-dy).atan2(dx);
    let mut a = angle - start;
    if extent >= 0.0 {
        while a < 0.0 {
            a += 2.0 * std::f64::consts::PI;
        }
        a <= extent
    } else {
        while a > 0.0 {
            a -= 2.0 * std::f64::consts::PI;
        }
        a >= extent
    }
}

/// Pre-computed chord data for ArcChord fill mode.
pub(crate) struct ArcChordData {
    pub(crate) chord_x1: f64,
    pub(crate) chord_y1: f64,
    pub(crate) cdx: f64,
    pub(crate) cdy: f64,
    pub(crate) mid_cross: f64,
}

impl ArcChordData {
    /// Create chord data only if arc_mode is Chord (0) and extent < full circle.
    pub(crate) fn new_if_chord(
        arc_mode: u8,
        angle2: i16,
        start_rad: f64,
        extent_rad: f64,
    ) -> Option<Self> {
        if arc_mode != 0 || angle2.abs() >= 360 * 64 {
            return None;
        }
        let end_rad = start_rad + extent_rad;
        let chord_x1 = start_rad.cos();
        let chord_y1 = -start_rad.sin();
        let chord_x2 = end_rad.cos();
        let chord_y2 = -end_rad.sin();
        let cdx = chord_x2 - chord_x1;
        let cdy = chord_y2 - chord_y1;
        let mid_rad = start_rad + extent_rad / 2.0;
        let mid_x = mid_rad.cos();
        let mid_y = -mid_rad.sin();
        let mid_cross = cdx * (mid_y - chord_y1) - cdy * (mid_x - chord_x1);
        Some(Self {
            chord_x1,
            chord_y1,
            cdx,
            cdy,
            mid_cross,
        })
    }
}
