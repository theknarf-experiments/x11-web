use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

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
            new_data[dst_off..dst_off + copy_w].copy_from_slice(&self.data[src_off..src_off + copy_w]);
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
            1  => (0, 0),                   // NorthWest
            2  => (dw / 2, 0),              // North
            3  => (dw, 0),                  // NorthEast
            4  => (0, dh / 2),              // West
            5  => (dw / 2, dh / 2),         // Center
            6  => (dw, dh / 2),             // East
            7  => (0, dh),                  // SouthWest
            8  => (dw / 2, dh),             // South
            9  => (dw, dh),                 // SouthEast
            10 => (0, 0),                   // Static
            _  => (0, 0),
        };

        // Row-by-row copy with bounds checking.
        // src_y is the row in the old buffer, dst_y = src_y + dy is in the new buffer.
        // Similarly for columns.
        let src_x_start = (-dx).max(0) as usize;
        let dst_x_start = dx.max(0) as usize;
        let copy_w = (old_w as usize).min(new_width as usize - dst_x_start).saturating_sub(src_x_start);

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
                (
                    min_x,
                    min_y,
                    (max_x - min_x) as u32,
                    (max_y - min_y) as u32,
                )
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
        let compressed = encoder.finish().unwrap_or(pixels);

        Some((dx as i16, dy as i16, dw as u16, dh as u16, compressed))
    }

    /// Bitwise-invert every byte of a rectangle (the GXinvert raster
    /// op). Used by clients that flip bits via XSetFunction +
    /// XFillRectangle to compute the complement of an image — most
    /// commonly the rendercheck `libreoffice_xrgb` "invert" subtest.
    #[allow(dead_code)]
    pub fn invert_rect(&mut self, x: i16, y: i16, width: u16, height: u16) {
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
            let dst_off = dy as usize * self.stride + row_start * 4;
            if dst_off + row_len * 4 > self.data.len() {
                continue;
            }
            for byte in &mut self.data[dst_off..dst_off + row_len * 4] {
                *byte = !*byte;
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
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
        if row_start >= row_end { return; }
        let row_len = row_end - row_start;

        let mut row_buf = vec![0u8; row_len * 4];
        for i in 0..row_len {
            row_buf[i*4..i*4+4].copy_from_slice(&pixel);
        }

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 { continue; }
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
                self.data[off] = (masked & 0xFF) as u8;           // B
                self.data[off + 1] = ((masked >> 8) & 0xFF) as u8;  // G
                self.data[off + 2] = ((masked >> 16) & 0xFF) as u8; // R
                self.data[off + 3] = 0xFF;                          // A
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
            let col_end =
                (width as usize).min((self.width as i32 - x.max(0) as i32).max(0) as usize + col_start);
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
                    self.data[dst_off..dst_off + 4]
                        .copy_from_slice(&data[src_off..src_off + 4]);
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
                    let b = ((data[src_off] as u32 * sa + self.data[dst_off] as u32 * da) / 255) as u8;
                    let g = ((data[src_off + 1] as u32 * sa + self.data[dst_off + 1] as u32 * da) / 255) as u8;
                    let r = ((data[src_off + 2] as u32 * sa + self.data[dst_off + 2] as u32 * da) / 255) as u8;
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
                    rgba[dst] = self.data[src + 2];     // R (from BGRA B offset)
                    rgba[dst + 1] = self.data[src + 1]; // G
                    rgba[dst + 2] = self.data[src];     // B
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

    /// Draw a line using Bresenham's algorithm (simple version, GXcopy).
    #[allow(dead_code)]
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, line_width: u16) {
        self.draw_line_gc(x0, y0, x1, y1, color, line_width, 3, 0xFFFFFFFF,
                          0, 1, 0, 0, &[], 0, &[]);
    }

    /// Draw a line with full GC support: raster op, cap/join styles, dashes, clip rects.
    ///
    /// - `gc_func`: GC raster operation (0-15)
    /// - `plane_mask`: bit-plane mask
    /// - `line_style`: 0=Solid, 1=OnOffDash, 2=DoubleDash
    /// - `cap_style`: 0=NotLast, 1=Butt, 2=Round, 3=Projecting
    /// - `join_style`: 0=Miter, 1=Round, 2=Bevel (used by polyline callers)
    /// - `dash_offset`: offset into dash pattern
    /// - `dash_list`: dash pattern (empty = solid)
    /// - `background`: background color for DoubleDash
    /// - `clip_rects`: GC clip rectangles (empty = no clipping)
    #[allow(clippy::too_many_arguments)]
    pub fn draw_line_gc(
        &mut self,
        x0: i32, y0: i32, x1: i32, y1: i32,
        color: u32,
        line_width: u16,
        gc_func: u8,
        plane_mask: u32,
        line_style: u8,
        cap_style: u8,
        join_style: u8,
        dash_offset: u16,
        dash_list: &[u8],
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        // join_style is used by draw_polyline_gc; not needed for single segments.
        let _ = join_style;

        // Dashed line state
        let dashes = if (line_style == 1 || line_style == 2) && !dash_list.is_empty() {
            Some(DashState::new(dash_list, dash_offset))
        } else {
            None
        };

        if line_width <= 1 {
            self.bresenham_line_gc(x0, y0, x1, y1, color, gc_func, plane_mask,
                                   cap_style, dashes, line_style, background, clip_rects);
        } else {
            // Wide line: draw a filled rectangle along the line path
            let hw = (line_width / 2) as i32;
            if y0 == y1 {
                // Horizontal wide line
                let min_x = x0.min(x1);
                let max_x = x0.max(x1);
                let extra = match cap_style {
                    2 | 3 => hw, // Round or Projecting: extend by half-width
                    _ => 0,
                };
                self.fill_rect_rop_clipped(
                    (min_x - extra) as i16, (y0 - hw) as i16,
                    (max_x - min_x + 1 + extra * 2) as u16, line_width,
                    color, gc_func, plane_mask, clip_rects,
                );
            } else if x0 == x1 {
                // Vertical wide line
                let min_y = y0.min(y1);
                let max_y = y0.max(y1);
                let extra = match cap_style {
                    2 | 3 => hw,
                    _ => 0,
                };
                self.fill_rect_rop_clipped(
                    (x0 - hw) as i16, (min_y - extra) as i16,
                    line_width, (max_y - min_y + 1 + extra * 2) as u16,
                    color, gc_func, plane_mask, clip_rects,
                );
            } else {
                // Diagonal wide line: use perpendicular offset from the line direction.
                // Compute the unit direction and perpendicular vectors.
                let fdx = (x1 - x0) as f64;
                let fdy = (y1 - y0) as f64;
                let len = fdx.hypot(fdy);
                let ux = fdx / len;
                let uy = fdy / len;
                // Perpendicular direction (rotated 90 degrees)
                let px = -uy;
                let py = ux;

                // Extend endpoints for Projecting cap (cap_style 3)
                let (ex0, ey0, ex1, ey1) = if cap_style == 3 {
                    let ext = hw as f64;
                    (
                        (x0 as f64 - ux * ext).round() as i32,
                        (y0 as f64 - uy * ext).round() as i32,
                        (x1 as f64 + ux * ext).round() as i32,
                        (y1 as f64 + uy * ext).round() as i32,
                    )
                } else {
                    (x0, y0, x1, y1)
                };

                for d in -hw..=hw {
                    let ox = (px * d as f64).round() as i32;
                    let oy = (py * d as f64).round() as i32;
                    self.bresenham_line_gc(
                        ex0 + ox, ey0 + oy, ex1 + ox, ey1 + oy,
                        color, gc_func, plane_mask,
                        // Use Butt cap for the individual scan lines since we
                        // handle caps at the composite level
                        1, None, 0, background, clip_rects,
                    );
                }
            }

            // Round cap: draw filled circles at endpoints
            if cap_style == 2 && line_width > 2 {
                self.fill_circle(x0, y0, hw, color, gc_func, plane_mask, clip_rects);
                self.fill_circle(x1, y1, hw, color, gc_func, plane_mask, clip_rects);
            }

            // Projecting cap for diagonal lines: fill_circle would be wrong,
            // but the line extension above handles it via the extended endpoints.
        }
    }

    /// Draw a polyline (sequence of connected line segments) with join style support.
    ///
    /// This draws each segment via `draw_line_gc` and then renders the appropriate
    /// join decoration at each interior vertex where two segments meet.
    ///
    /// Join styles (X11 spec):
    /// - 0 (Miter): extend outer edges until they meet, with a miter limit
    ///   (angle < 10.43 degrees falls back to bevel)
    /// - 1 (Round): filled circle at join point with radius = line_width / 2
    /// - 2 (Bevel): fill the triangular notch between the outer edges
    #[allow(clippy::too_many_arguments)]
    pub fn draw_polyline_gc(
        &mut self,
        points: &[(i32, i32)],
        color: u32,
        line_width: u16,
        gc_func: u8,
        plane_mask: u32,
        line_style: u8,
        cap_style: u8,
        join_style: u8,
        dash_offset: u16,
        dash_list: &[u8],
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if points.len() < 2 {
            return;
        }

        // Draw each line segment
        for w in points.windows(2) {
            self.draw_line_gc(
                w[0].0, w[0].1, w[1].0, w[1].1,
                color, line_width, gc_func, plane_mask,
                line_style, cap_style, join_style,
                dash_offset, dash_list, background, clip_rects,
            );
        }

        // Apply join decorations at interior vertices (only for wide lines)
        if line_width <= 1 || points.len() < 3 {
            return;
        }

        let hw = (line_width as f64) / 2.0;

        for i in 1..points.len() - 1 {
            let (px, py) = (points[i - 1].0 as f64, points[i - 1].1 as f64);
            let (jx, jy) = (points[i].0 as f64, points[i].1 as f64);
            let (nx, ny) = (points[i + 1].0 as f64, points[i + 1].1 as f64);

            // Direction vectors for incoming and outgoing segments
            let d1x = jx - px;
            let d1y = jy - py;
            let d2x = nx - jx;
            let d2y = ny - jy;

            let len1 = (d1x * d1x + d1y * d1y).sqrt();
            let len2 = (d2x * d2x + d2y * d2y).sqrt();
            if len1 < 1e-9 || len2 < 1e-9 {
                continue;
            }

            // Unit direction vectors
            let u1x = d1x / len1;
            let u1y = d1y / len1;
            let u2x = d2x / len2;
            let u2y = d2y / len2;

            // Perpendicular normals (pointing left of direction)
            let n1x = -u1y;
            let n1y = u1x;
            let n2x = -u2y;
            let n2y = u2x;

            match join_style {
                1 => {
                    // JoinRound: filled circle at join point
                    self.fill_circle(
                        jx as i32, jy as i32, hw as i32,
                        color, gc_func, plane_mask, clip_rects,
                    );
                }
                2 => {
                    // JoinBevel: fill the triangular notch between the outer edges
                    // The three points of the bevel triangle:
                    // - join point offset along normal of segment 1
                    // - the join vertex itself (inner corner fills naturally)
                    // - join point offset along normal of segment 2
                    // We need to pick the outer side (where the gap is).
                    let cross = u1x * u2y - u1y * u2x;
                    let sign = if cross > 0.0 { 1.0 } else { -1.0 };

                    let p1x = jx + sign * n1x * hw;
                    let p1y = jy + sign * n1y * hw;
                    let p2x = jx + sign * n2x * hw;
                    let p2y = jy + sign * n2y * hw;

                    let tri = [
                        (jx as i16, jy as i16),
                        (p1x as i16, p1y as i16),
                        (p2x as i16, p2y as i16),
                    ];
                    self.fill_polygon_gc(&tri, color, 0, gc_func, plane_mask, clip_rects);
                }
                _ => {
                    // JoinMiter (0, default): extend outer edges until they meet.
                    // Miter limit: if the angle between segments is too acute,
                    // fall back to bevel. X11 spec miter limit is at 10.43 degrees,
                    // which corresponds to a miter length / line_width ratio of ~10.43
                    // (i.e. 1/sin(theta/2) where theta is the angle).
                    let cos_theta = u1x * u2x + u1y * u2y;
                    // theta is the angle between the two directions
                    // half_sin = sin(theta/2) where theta = pi - angle_between
                    // For miter: miter_length = hw / sin(alpha/2)
                    // where alpha = pi - theta (the join angle)
                    let alpha = (1.0 - cos_theta).clamp(0.0, 2.0);
                    // alpha = 2 * sin^2(angle/2), where angle is between the directions
                    // sin(join_half_angle) = sin((pi - angle)/2) = cos(angle/2)
                    // cos^2(angle/2) = (1 + cos_theta) / 2
                    let half_join_sin_sq = alpha / 2.0; // sin^2(half of the supplementary angle)
                    // Miter limit: miter_length <= miter_limit * line_width
                    // miter_length = hw / sin(half_join_angle)
                    // X11 miter limit ratio: 1/sin(10.43/2 degrees) ~ 11.0
                    // Simplified: if sin^2(half_join) < threshold, use bevel
                    // threshold = (1 / 11.0)^2 ≈ 0.00826
                    let miter_limit_sin_sq = 0.00826;

                    if half_join_sin_sq < miter_limit_sin_sq {
                        // Angle too acute - fall back to bevel
                        let cross = u1x * u2y - u1y * u2x;
                        let sign = if cross > 0.0 { 1.0 } else { -1.0 };
                        let p1x = jx + sign * n1x * hw;
                        let p1y = jy + sign * n1y * hw;
                        let p2x = jx + sign * n2x * hw;
                        let p2y = jy + sign * n2y * hw;
                        let tri = [
                            (jx as i16, jy as i16),
                            (p1x as i16, p1y as i16),
                            (p2x as i16, p2y as i16),
                        ];
                        self.fill_polygon_gc(&tri, color, 0, gc_func, plane_mask, clip_rects);
                    } else {
                        // Compute the miter point: intersection of the two offset edges
                        let cross = u1x * u2y - u1y * u2x;
                        let sign = if cross > 0.0 { 1.0 } else { -1.0 };

                        // Offset edge 1: point = join + sign*n1*hw, direction = u1
                        // Offset edge 2: point = join + sign*n2*hw, direction = u2
                        // Solve for intersection using parametric form
                        let p1x = jx + sign * n1x * hw;
                        let p1y = jy + sign * n1y * hw;
                        let p2x = jx + sign * n2x * hw;
                        let p2y = jy + sign * n2y * hw;

                        let denom = u1x * (-u2y) - u1y * (-u2x);
                        if denom.abs() > 1e-9 {
                            let t = ((p2x - p1x) * (-u2y) - (p2y - p1y) * (-u2x)) / denom;
                            let mx = p1x + t * u1x;
                            let my = p1y + t * u1y;

                            // Fill the miter triangle (join point + two outer edge points + miter point)
                            let quad = [
                                (jx as i16, jy as i16),
                                (p1x as i16, p1y as i16),
                                (mx as i16, my as i16),
                                (p2x as i16, p2y as i16),
                            ];
                            self.fill_polygon_gc(&quad, color, 0, gc_func, plane_mask, clip_rects);
                        }
                    }
                }
            }
        }
    }

    /// Bresenham line with GC raster op, dashes, cap_style, and clip rects.
    fn bresenham_line_gc(
        &mut self,
        x0: i32, y0: i32, x1: i32, y1: i32,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        mut dashes: Option<DashState>,
        line_style: u8,
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;
        let mut is_first = true;

        loop {
            let is_last = x == x1 && y == y1;

            // NotLast cap: skip the last pixel
            let skip = cap_style == 0 && is_last && !is_first;

            // Dash logic
            let draw_fg = if let Some(ref mut ds) = dashes {
                let on = ds.is_on();
                ds.advance();
                on
            } else {
                true
            };

            if !skip {
                if draw_fg {
                    self.draw_point_gc(x, y, color, gc_func, plane_mask, clip_rects);
                } else if line_style == 2 {
                    // DoubleDash: draw background in dash gaps
                    self.draw_point_gc(x, y, background, gc_func, plane_mask, clip_rects);
                }
                // OnOffDash with draw_fg=false: skip pixel entirely
            }

            is_first = false;
            if is_last {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draw a single pixel with GC function, plane mask, and clip rects.
    pub fn draw_point_gc(
        &mut self,
        x: i32, y: i32,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        if !clip_rects.is_empty() && !point_in_clip_rects(x, y, clip_rects) {
            return;
        }
        if gc_func == 3 && plane_mask == 0xFFFFFFFF {
            self.draw_point(x, y, color);
            return;
        }
        let off = y as usize * self.stride + x as usize * 4;
        if off + 3 >= self.data.len() {
            return;
        }
        let dst = {
            let b = self.data[off] as u32;
            let g = self.data[off + 1] as u32;
            let r = self.data[off + 2] as u32;
            (r << 16) | (g << 8) | b
        };
        let result = apply_gc_function(gc_func, color, dst);
        let masked = (result & plane_mask) | (dst & !plane_mask);
        self.data[off] = (masked & 0xFF) as u8;
        self.data[off + 1] = ((masked >> 8) & 0xFF) as u8;
        self.data[off + 2] = ((masked >> 16) & 0xFF) as u8;
        self.data[off + 3] = 0xFF;
        self.mark_dirty(x, y, 1, 1);
    }

    /// Fill a small circle (for round line caps/joins).
    fn fill_circle(
        &mut self,
        cx: i32, cy: i32, radius: i32,
        color: u32, gc_func: u8, plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.draw_point_gc(cx + dx, cy + dy, color, gc_func, plane_mask, clip_rects);
                }
            }
        }
    }

    /// fill_rect_rop with clip rectangle support.
    pub fn fill_rect_rop_clipped(
        &mut self,
        x: i16, y: i16, width: u16, height: u16,
        color: u32, function: u8, plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if clip_rects.is_empty() {
            self.fill_rect_rop(x, y, width, height, color, function, plane_mask);
            return;
        }
        // Intersect the fill rect with each clip rect
        for &(cx, cy, cw, ch) in clip_rects {
            let ix0 = (x as i32).max(cx as i32);
            let iy0 = (y as i32).max(cy as i32);
            let ix1 = (x as i32 + width as i32).min(cx as i32 + cw as i32);
            let iy1 = (y as i32 + height as i32).min(cy as i32 + ch as i32);
            if ix0 < ix1 && iy0 < iy1 {
                self.fill_rect_rop(
                    ix0 as i16, iy0 as i16,
                    (ix1 - ix0) as u16, (iy1 - iy0) as u16,
                    color, function, plane_mask,
                );
            }
        }
    }

    #[allow(dead_code)]
    fn bresenham_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;

        loop {
            self.draw_point(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draw an elliptical arc with full GC support.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_arc_with_mode_gc(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        filled: bool,
        color: u32,
        arc_mode: u8,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
        line_width: u16,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let cx = x as f64 + width as f64 / 2.0;
        let cy = y as f64 + height as f64 / 2.0;
        let rx = width as f64 / 2.0;
        let ry = height as f64 / 2.0;

        let start_rad = (angle1 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let extent_rad = (angle2 as f64) / 64.0 * std::f64::consts::PI / 180.0;

        if filled {
            let min_y = y.max(0) as i32;
            let max_y = ((y as i32 + height as i32).min(self.height as i32 - 1)).max(0);
            let min_x = x.max(0) as i32;
            let max_x = ((x as i32 + width as i32).min(self.width as i32 - 1)).max(0);

            if arc_mode == 0 && angle2.abs() < 360 * 64 {
                // ArcChord: fill area between the arc and a straight chord
                // connecting start and end points of the arc.
                let end_rad = start_rad + extent_rad;
                // Chord endpoints in normalized coords
                let chord_x1 = start_rad.cos();
                let chord_y1 = -start_rad.sin();
                let chord_x2 = end_rad.cos();
                let chord_y2 = -end_rad.sin();
                // Direction vector of chord: (dx, dy)
                let cdx = chord_x2 - chord_x1;
                let cdy = chord_y2 - chord_y1;
                // Mid-arc point to determine which side of chord to fill
                let mid_rad = start_rad + extent_rad / 2.0;
                let mid_x = mid_rad.cos();
                let mid_y = -mid_rad.sin();
                let mid_cross = cdx * (mid_y - chord_y1) - cdy * (mid_x - chord_x1);

                for py in min_y..=max_y {
                    for px in min_x..=max_x {
                        let ddx = (px as f64 - cx) / rx;
                        let ddy = (py as f64 - cy) / ry;
                        if ddx * ddx + ddy * ddy <= 1.0 {
                            // Check if point is on same side of chord as mid-arc
                            let cross = cdx * (ddy - chord_y1) - cdy * (ddx - chord_x1);
                            if (cross >= 0.0) == (mid_cross >= 0.0) {
                                self.draw_point_gc(px, py, color, gc_func, plane_mask, clip_rects);
                            }
                        }
                    }
                }
            } else {
                // ArcPieSlice (default): fill from center like a pie wedge
                for py in min_y..=max_y {
                    for px in min_x..=max_x {
                        let ddx = (px as f64 - cx) / rx;
                        let ddy = (py as f64 - cy) / ry;
                        if ddx * ddx + ddy * ddy <= 1.0
                            && (angle2.abs() >= 360 * 64
                                || point_in_arc(ddx, ddy, start_rad, extent_rad))
                            {
                                self.draw_point_gc(px, py, color, gc_func, plane_mask, clip_rects);
                            }
                    }
                }
            }
        } else {
            let lw = line_width.max(1) as f64;
            if lw <= 1.0 {
                // Thin line: use parametric Bresenham approach
                let steps = ((rx + ry) * 2.0).max(64.0) as usize;
                let mut prev_x: Option<i32> = None;
                let mut prev_y: Option<i32> = None;

                for i in 0..=steps {
                    let t = start_rad + extent_rad * (i as f64 / steps as f64);
                    let px = (cx + rx * t.cos()) as i32;
                    let py = (cy - ry * t.sin()) as i32;

                    if let (Some(lx), Some(ly)) = (prev_x, prev_y) {
                        // Use GC-aware point drawing via bresenham steps
                        let dx: i32 = (px - lx).abs();
                        let dy: i32 = -(py - ly).abs();
                        let sx: i32 = if lx < px { 1 } else { -1 };
                        let sy: i32 = if ly < py { 1 } else { -1 };
                        let mut err = dx + dy;
                        let mut bx = lx;
                        let mut by = ly;
                        loop {
                            self.draw_point_gc(bx, by, color, gc_func, plane_mask, clip_rects);
                            if bx == px && by == py {
                                break;
                            }
                            let e2 = 2 * err;
                            if e2 >= dy {
                                err += dy;
                                bx += sx;
                            }
                            if e2 <= dx {
                                err += dx;
                                by += sy;
                            }
                        }
                    }
                    prev_x = Some(px);
                    prev_y = Some(py);
                }
            } else {
                // Thick line: scan-convert the region between inner and outer
                // concentric ellipses and filter by angular range.
                let half_lw = lw / 2.0;
                let outer_rx = rx + half_lw;
                let outer_ry = ry + half_lw;
                let inner_rx = (rx - half_lw).max(0.0);
                let inner_ry = (ry - half_lw).max(0.0);

                let full_circle = angle2.abs() >= 360 * 64;

                // Bounding box of the outer ellipse
                let min_x = (cx - outer_rx).floor() as i32;
                let max_x = (cx + outer_rx).ceil() as i32;
                let min_y = (cy - outer_ry).floor() as i32;
                let max_y = (cy + outer_ry).ceil() as i32;

                for py in min_y..=max_y {
                    for px in min_x..=max_x {
                        let ddx = px as f64 - cx;
                        let ddy = py as f64 - cy;

                        // Check if pixel is within the outer ellipse
                        let outer_val = if outer_rx > 0.0 && outer_ry > 0.0 {
                            (ddx / outer_rx).powi(2) + (ddy / outer_ry).powi(2)
                        } else {
                            f64::MAX
                        };
                        if outer_val > 1.0 {
                            continue;
                        }

                        // Check if pixel is outside the inner ellipse
                        if inner_rx > 0.0 && inner_ry > 0.0 {
                            let inner_val = (ddx / inner_rx).powi(2) + (ddy / inner_ry).powi(2);
                            if inner_val < 1.0 {
                                continue;
                            }
                        }
                        // else inner ellipse has zero radius, so every point
                        // inside outer is part of the stroke

                        // Check angular range (use normalized coords against
                        // the original ellipse center)
                        if !full_circle {
                            // Normalize against the nominal ellipse radii for
                            // angle test so the angular boundaries match the
                            // X11 spec definition.
                            let norm_x = if rx > 0.0 { ddx / rx } else { ddx };
                            let norm_y = if ry > 0.0 { ddy / ry } else { ddy };
                            if !point_in_arc(norm_x, norm_y, start_rad, extent_rad) {
                                continue;
                            }
                        }

                        self.draw_point_gc(px, py, color, gc_func, plane_mask, clip_rects);
                    }
                }
            }
        }
    }

    /// Test whether a pixel is inside a filled arc region given arc parameters.
    /// Returns true if the point at (px, py) is inside the arc described by
    /// the bounding box (x, y, width, height) and angle parameters.
    #[inline]
    fn pixel_in_filled_arc(
        px: i32, py: i32,
        cx: f64, cy: f64, rx: f64, ry: f64,
        _angle1: i16, angle2: i16,
        start_rad: f64, extent_rad: f64,
        arc_mode: u8,
        // Pre-computed chord data for ArcChord mode (only used when arc_mode == 0)
        chord: Option<&ArcChordData>,
    ) -> bool {
        let ddx = (px as f64 - cx) / rx;
        let ddy = (py as f64 - cy) / ry;
        if ddx * ddx + ddy * ddy > 1.0 {
            return false;
        }
        if angle2.abs() >= 360 * 64 {
            return true;
        }
        if arc_mode == 0 {
            // ArcChord
            if let Some(ch) = chord {
                let cross = ch.cdx * (ddy - ch.chord_y1) - ch.cdy * (ddx - ch.chord_x1);
                (cross >= 0.0) == (ch.mid_cross >= 0.0)
            } else {
                point_in_arc(ddx, ddy, start_rad, extent_rad)
            }
        } else {
            // ArcPieSlice
            point_in_arc(ddx, ddy, start_rad, extent_rad)
        }
    }

    /// Fill an arc region with a tile pattern, respecting arc_mode (Chord vs PieSlice).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_arc_tiled(
        &mut self,
        x: i16, y: i16, width: u16, height: u16,
        angle1: i16, angle2: i16,
        tile_data: &[u8], tile_w: u32, tile_h: u32,
        ts_x: i16, ts_y: i16,
        arc_mode: u8,
        gc_func: u8, plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if width == 0 || height == 0 || tile_w == 0 || tile_h == 0 || tile_data.is_empty() {
            return;
        }

        let cx = x as f64 + width as f64 / 2.0;
        let cy = y as f64 + height as f64 / 2.0;
        let rx = width as f64 / 2.0;
        let ry = height as f64 / 2.0;
        let start_rad = (angle1 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let extent_rad = (angle2 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let chord = ArcChordData::new_if_chord(arc_mode, angle2, start_rad, extent_rad);
        let tile_stride = tile_w as usize * 4;

        let min_y = y.max(0) as i32;
        let max_y = ((y as i32 + height as i32).min(self.height as i32 - 1)).max(0);
        let min_x = x.max(0) as i32;
        let max_x = ((x as i32 + width as i32).min(self.width as i32 - 1)).max(0);

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                if !Self::pixel_in_filled_arc(px, py, cx, cy, rx, ry, angle1, angle2, start_rad, extent_rad, arc_mode, chord.as_ref()) {
                    continue;
                }
                if !self.in_clip(px, py, clip_rects) {
                    continue;
                }
                let tile_x = ((px - ts_x as i32).rem_euclid(tile_w as i32)) as usize;
                let tile_y = ((py - ts_y as i32).rem_euclid(tile_h as i32)) as usize;
                let off = tile_y * tile_stride + tile_x * 4;
                if off + 3 < tile_data.len() {
                    let color = (tile_data[off + 2] as u32) << 16
                        | (tile_data[off + 1] as u32) << 8
                        | tile_data[off] as u32;
                    self.draw_point_with_func_masked(px, py, color, gc_func, plane_mask);
                }
            }
        }
    }

    /// Fill an arc region with a stipple pattern, respecting arc_mode (Chord vs PieSlice).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_arc_stippled(
        &mut self,
        x: i16, y: i16, width: u16, height: u16,
        angle1: i16, angle2: i16,
        fg: u32, bg: u32,
        stipple_data: &[u8], stipple_w: u32, stipple_h: u32,
        ts_x: i16, ts_y: i16,
        opaque: bool,
        arc_mode: u8,
        gc_func: u8, plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if width == 0 || height == 0 || stipple_w == 0 || stipple_h == 0 || stipple_data.is_empty() {
            return;
        }

        let cx = x as f64 + width as f64 / 2.0;
        let cy = y as f64 + height as f64 / 2.0;
        let rx = width as f64 / 2.0;
        let ry = height as f64 / 2.0;
        let start_rad = (angle1 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let extent_rad = (angle2 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let chord = ArcChordData::new_if_chord(arc_mode, angle2, start_rad, extent_rad);
        let stipple_stride = stipple_w.div_ceil(8) as usize;

        let min_y = y.max(0) as i32;
        let max_y = ((y as i32 + height as i32).min(self.height as i32 - 1)).max(0);
        let min_x = x.max(0) as i32;
        let max_x = ((x as i32 + width as i32).min(self.width as i32 - 1)).max(0);

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                if !Self::pixel_in_filled_arc(px, py, cx, cy, rx, ry, angle1, angle2, start_rad, extent_rad, arc_mode, chord.as_ref()) {
                    continue;
                }
                if !self.in_clip(px, py, clip_rects) {
                    continue;
                }
                let stip_x = ((px - ts_x as i32).rem_euclid(stipple_w as i32)) as u32;
                let stip_y = ((py - ts_y as i32).rem_euclid(stipple_h as i32)) as u32;
                let byte_idx = stip_y as usize * stipple_stride + (stip_x / 8) as usize;
                let bit = if byte_idx < stipple_data.len() {
                    (stipple_data[byte_idx] >> (stip_x % 8)) & 1
                } else {
                    0
                };
                if bit != 0 {
                    self.draw_point_with_func_masked(px, py, fg, gc_func, plane_mask);
                } else if opaque {
                    self.draw_point_with_func_masked(px, py, bg, gc_func, plane_mask);
                }
            }
        }
    }

    /// Draw an arc with full GC attributes: line_width, raster op, plane_mask,
    /// line_style (dashes), and clip rectangles.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_arc_gc(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        filled: bool,
        foreground: u32,
        background: u32,
        line_width: u16,
        function: u8,
        plane_mask: u32,
        line_style: u8,
        dash_offset: u16,
        dash_list: &[u8],
        clip_rects: &[(i16, i16, u16, u16)],
        arc_mode: u8,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        // For filled arcs, use arc_mode-aware fill with full GC support.
        if filled {
            self.draw_arc_with_mode_gc(
                x, y, width, height, angle1, angle2, true, foreground, arc_mode,
                function, plane_mask, clip_rects, line_width,
            );
            return;
        }

        let cx = x as f64 + width as f64 / 2.0;
        let cy = y as f64 + height as f64 / 2.0;
        let rx = width as f64 / 2.0;
        let ry = height as f64 / 2.0;

        let start_rad = (angle1 as f64) / 64.0 * std::f64::consts::PI / 180.0;
        let extent_rad = (angle2 as f64) / 64.0 * std::f64::consts::PI / 180.0;

        let steps = ((rx + ry) * 2.0).max(64.0) as usize;
        let lw = line_width.max(1) as f64;
        let half_lw = lw / 2.0;

        // Set up dash state if dashed line style
        let use_dashes = (line_style == 1 || line_style == 2) && !dash_list.is_empty();
        let mut dash_state = if use_dashes {
            Some(DashState::new(dash_list, dash_offset))
        } else {
            None
        };

        let mut prev: Option<(i32, i32)> = None;

        for i in 0..=steps {
            let t = start_rad + extent_rad * (i as f64 / steps as f64);
            let px = (cx + rx * t.cos()) as i32;
            let py = (cy - ry * t.sin()) as i32;

            if let Some((lx, ly)) = prev {
                // Determine dash on/off state
                let draw_fg = if let Some(ref mut ds) = dash_state {
                    let on = ds.is_on();
                    ds.advance();
                    on
                } else {
                    true
                };

                let color = if draw_fg {
                    foreground
                } else if line_style == 2 {
                    // DoubleDash: draw background in gaps
                    background
                } else {
                    prev = Some((px, py));
                    continue; // OnOffDash: skip gaps
                };

                if lw <= 1.5 {
                    // Thin line: use single-pixel Bresenham
                    self.bresenham_line_rop_clipped(
                        lx, ly, px, py, color, function, plane_mask, clip_rects,
                    );
                } else {
                    // Thick line: draw perpendicular rectangles along the arc
                    let dx = (px - lx) as f64;
                    let dy = (py - ly) as f64;
                    let len = dx.hypot(dy);
                    if len > 0.0 {
                        let nx = -dy / len * half_lw;
                        let ny = dx / len * half_lw;
                        // Fill the thick segment as a small rectangle
                        let min_x = (lx as f64 - nx.abs()).floor() as i32;
                        let max_x = (px as f64 + nx.abs()).ceil() as i32;
                        let min_y = (ly as f64 - ny.abs()).floor() as i32;
                        let max_y = (py as f64 + ny.abs()).ceil() as i32;
                        for fy in min_y..=max_y {
                            for fx in min_x..=max_x {
                                // Check distance from line segment
                                let t_proj = ((fx as f64 - lx as f64) * dx + (fy as f64 - ly as f64) * dy) / (len * len);
                                if (0.0..=1.0).contains(&t_proj) {
                                    let closest_x = lx as f64 + t_proj * dx;
                                    let closest_y = ly as f64 + t_proj * dy;
                                    let dist = ((fx as f64 - closest_x).powi(2) + (fy as f64 - closest_y).powi(2)).sqrt();
                                    if dist <= half_lw {
                                        if !clip_rects.is_empty() && !clip_rects.iter().any(|&(cx, cy, cw, ch)| {
                                            fx >= cx as i32 && fx < (cx as i32 + cw as i32) &&
                                            fy >= cy as i32 && fy < (cy as i32 + ch as i32)
                                        }) {
                                            continue;
                                        }
                                        self.draw_point_with_func_masked(fx, fy, color, function, plane_mask);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            prev = Some((px, py));
        }
    }

    /// Draw a point applying both GC function and plane_mask.
    pub fn draw_point_with_func_masked(&mut self, x: i32, y: i32, color: u32, gc_func: u8, plane_mask: u32) {
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

    /// Bresenham line with raster op, plane_mask, and clip rectangles.
    fn bresenham_line_rop_clipped(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        color: u32,
        function: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut cx = x0;
        let mut cy = y0;
        loop {
            let in_clip = clip_rects.is_empty() || clip_rects.iter().any(|&(rx, ry, rw, rh)| {
                cx >= rx as i32 && cx < (rx as i32 + rw as i32) &&
                cy >= ry as i32 && cy < (ry as i32 + rh as i32)
            });
            if in_clip {
                self.draw_point_with_func_masked(cx, cy, color, function, plane_mask);
            }
            if cx == x1 && cy == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
    }

    /// Fill a rectangle using a stipple pattern.
    ///
    /// The stipple is a 1-bit-per-pixel bitmap where set bits draw the foreground
    /// and (for OpaqueStippled) cleared bits draw the background.
    /// `opaque`: if true, draw background for 0 bits (OpaqueStippled); if false,
    /// skip 0 bits (Stippled).
    pub fn fill_rect_stippled(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        foreground: u32,
        background: u32,
        stipple_data: &[u8],
        stipple_w: u32,
        stipple_h: u32,
        ts_x: i16,
        ts_y: i16,
        opaque: bool,
        function: u8,
        plane_mask: u32,
    ) {
        if stipple_w == 0 || stipple_h == 0 || stipple_data.is_empty() {
            self.fill_rect_rop(x, y, width, height, foreground, function, plane_mask);
            return;
        }
        let stipple_stride = stipple_w.div_ceil(8) as usize;
        let row_start = (x as i32).max(0) as usize;
        let row_end = ((x as i32 + width as i32).min(self.width as i32)).max(0) as usize;
        if row_start >= row_end {
            return;
        }

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            // Which row in the stipple pattern (tiled)
            let stip_y = ((dy - ts_y as i32) % stipple_h as i32 + stipple_h as i32) as u32 % stipple_h;

            for px in row_start..row_end {
                let stip_x = ((px as i32 - ts_x as i32) % stipple_w as i32 + stipple_w as i32) as u32 % stipple_w;
                let byte_idx = stip_y as usize * stipple_stride + (stip_x / 8) as usize;
                let bit = if byte_idx < stipple_data.len() {
                    (stipple_data[byte_idx] >> (stip_x % 8)) & 1
                } else {
                    0
                };

                if bit != 0 {
                    self.draw_point_with_func_masked(px as i32, dy, foreground, function, plane_mask);
                } else if opaque {
                    self.draw_point_with_func_masked(px as i32, dy, background, function, plane_mask);
                }
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Fill a rectangle using a tile pattern (pixmap).
    ///
    /// The tile is a full-color pixmap that is repeated across the drawable.
    pub fn fill_rect_tiled(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        tile_data: &[u8],
        tile_w: u32,
        tile_h: u32,
        ts_x: i16,
        ts_y: i16,
        function: u8,
        plane_mask: u32,
    ) {
        if tile_w == 0 || tile_h == 0 || tile_data.is_empty() {
            return;
        }
        let tile_stride = tile_w as usize * 4;
        let row_start = (x as i32).max(0) as usize;
        let row_end = ((x as i32 + width as i32).min(self.width as i32)).max(0) as usize;
        if row_start >= row_end {
            return;
        }

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let tile_y = ((dy - ts_y as i32) % tile_h as i32 + tile_h as i32) as u32 % tile_h;

            for px in row_start..row_end {
                let tile_x = ((px as i32 - ts_x as i32) % tile_w as i32 + tile_w as i32) as u32 % tile_w;
                let off = tile_y as usize * tile_stride + tile_x as usize * 4;
                if off + 3 < tile_data.len() {
                    let color = (tile_data[off + 2] as u32) << 16
                        | (tile_data[off + 1] as u32) << 8
                        | tile_data[off] as u32;
                    if function == 3 && plane_mask == 0xFFFFFFFF {
                        // Fast path: GXcopy
                        let dst_off = dy as usize * self.stride + px * 4;
                        if dst_off + 3 < self.data.len() {
                            self.data[dst_off..dst_off + 4]
                                .copy_from_slice(&tile_data[off..off + 4]);
                        }
                    } else {
                        self.draw_point_with_func_masked(px as i32, dy, color, function, plane_mask);
                    }
                }
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Fill a polygon using scan-line fill with GC support.
    ///
    /// `fill_rule`: 0 = EvenOdd, 1 = Winding (per X11 spec)
    /// `gc_func`: GC raster operation
    /// `plane_mask`: bit-plane mask
    /// `clip_rects`: GC clip rectangles
    pub fn fill_polygon_gc(
        &mut self,
        points: &[(i16, i16)],
        color: u32,
        fill_rule: u8,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if points.len() < 3 {
            return;
        }

        let min_y = points.iter().map(|p| p.1).min().unwrap().max(0) as i32;
        let max_y = points
            .iter()
            .map(|p| p.1)
            .max()
            .unwrap()
            .min(self.height as i16 - 1) as i32;

        for y in min_y..=max_y {
            let n = points.len();

            if fill_rule == 1 {
                // Winding rule: collect (x, direction) crossings, fill wherever winding != 0
                let mut crossings: Vec<(i32, i32)> = Vec::new();
                for i in 0..n {
                    let (x0, y0) = (points[i].0 as i32, points[i].1 as i32);
                    let (x1, y1) = (points[(i + 1) % n].0 as i32, points[(i + 1) % n].1 as i32);
                    if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                        let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                        let dir = if y0 < y1 { 1 } else { -1 };
                        crossings.push((x, dir));
                    }
                }
                crossings.sort_unstable_by_key(|c| c.0);

                let mut winding = 0i32;
                let mut span_start: Option<i32> = None;
                for (cx, dir) in &crossings {
                    let was_inside = winding != 0;
                    winding += dir;
                    let now_inside = winding != 0;
                    if !was_inside && now_inside {
                        span_start = Some(*cx);
                    } else if was_inside && !now_inside {
                        if let Some(sx_val) = span_start.take() {
                            let sx = sx_val.max(0) as i16;
                            let ex = (*cx).min(self.width as i32 - 1) as i16;
                            if ex >= sx {
                                self.fill_rect_rop_clipped(sx, y as i16,
                                    (ex - sx + 1) as u16, 1, color, gc_func, plane_mask, clip_rects);
                            }
                        }
                    }
                }
            } else {
                // EvenOdd rule (default)
                let mut intersections = Vec::new();
                for i in 0..n {
                    let (x0, y0) = (points[i].0 as i32, points[i].1 as i32);
                    let (x1, y1) = (points[(i + 1) % n].0 as i32, points[(i + 1) % n].1 as i32);

                    if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                        let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                        intersections.push(x);
                    }
                }
                intersections.sort_unstable();

                for pair in intersections.chunks(2) {
                    if pair.len() == 2 {
                        let start_x = pair[0].max(0) as i16;
                        let end_x = pair[1].min(self.width as i32 - 1) as i16;
                        if end_x >= start_x {
                            self.fill_rect_rop_clipped(start_x, y as i16,
                                (end_x - start_x + 1) as u16, 1, color, gc_func, plane_mask, clip_rects);
                        }
                    }
                }
            }
        }
    }

    /// Fill polygon with a tile pattern.
    pub fn fill_polygon_tiled(
        &mut self,
        points: &[(i16, i16)],
        tile_data: &[u8],
        tile_w: u32,
        tile_h: u32,
        ts_x: i16,
        ts_y: i16,
        fill_rule: u8,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if points.len() < 3 || tile_w == 0 || tile_h == 0 || tile_data.is_empty() {
            return;
        }
        // Rasterize polygon scanlines and apply tile pattern per-pixel
        let scanlines = self.compute_polygon_scanlines(points, fill_rule);
        for (y, spans) in &scanlines {
            let dy = *y;
            for &(sx, ex) in spans {
                for px in sx..=ex {
                    if !self.in_clip(px as i32, dy as i32, clip_rects) { continue; }
                    let tile_px = ((px as i32 - ts_x as i32).rem_euclid(tile_w as i32)) as usize;
                    let tile_py = ((dy as i32 - ts_y as i32).rem_euclid(tile_h as i32)) as usize;
                    let offset = (tile_py * tile_w as usize + tile_px) * 4;
                    if offset + 3 < tile_data.len() {
                        let b = tile_data[offset] as u32;
                        let g = tile_data[offset + 1] as u32;
                        let r = tile_data[offset + 2] as u32;
                        let a = tile_data[offset + 3] as u32;
                        let color = (a << 24) | (r << 16) | (g << 8) | b;
                        self.draw_point_with_func_masked(px as i32, dy as i32, color, gc_func, plane_mask);
                    }
                }
            }
        }
    }

    /// Fill polygon with a stipple pattern.
    pub fn fill_polygon_stippled(
        &mut self,
        points: &[(i16, i16)],
        fg: u32,
        bg: u32,
        stipple_data: &[u8],
        stipple_w: u32,
        stipple_h: u32,
        ts_x: i16,
        ts_y: i16,
        opaque: bool,
        fill_rule: u8,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if points.len() < 3 || stipple_w == 0 || stipple_h == 0 || stipple_data.is_empty() {
            return;
        }
        let stipple_stride = stipple_w.div_ceil(8) as usize;
        let scanlines = self.compute_polygon_scanlines(points, fill_rule);
        for (y, spans) in &scanlines {
            let dy = *y;
            for &(sx, ex) in spans {
                for px in sx..=ex {
                    if !self.in_clip(px as i32, dy as i32, clip_rects) { continue; }
                    let stip_x = ((px as i32 - ts_x as i32).rem_euclid(stipple_w as i32)) as u32;
                    let stip_y = ((dy as i32 - ts_y as i32).rem_euclid(stipple_h as i32)) as u32;
                    let byte_idx = stip_y as usize * stipple_stride + (stip_x / 8) as usize;
                    let bit = if byte_idx < stipple_data.len() {
                        (stipple_data[byte_idx] >> (stip_x % 8)) & 1
                    } else {
                        0
                    };
                    if bit != 0 {
                        self.draw_point_with_func_masked(px as i32, dy as i32, fg, gc_func, plane_mask);
                    } else if opaque {
                        self.draw_point_with_func_masked(px as i32, dy as i32, bg, gc_func, plane_mask);
                    }
                }
            }
        }
    }

    /// Check if a point is inside any clip rectangle (or no clipping if empty).
    fn in_clip(&self, x: i32, y: i32, clip_rects: &[(i16, i16, u16, u16)]) -> bool {
        if clip_rects.is_empty() {
            return true;
        }
        clip_rects.iter().any(|&(cx, cy, cw, ch)| {
            x >= cx as i32 && x < (cx as i32 + cw as i32) &&
            y >= cy as i32 && y < (cy as i32 + ch as i32)
        })
    }

    /// Compute polygon scanlines: returns sorted (y, [(start_x, end_x)]) pairs.
    fn compute_polygon_scanlines(&self, points: &[(i16, i16)], fill_rule: u8) -> Vec<(i16, Vec<(i16, i16)>)> {
        let min_y = points.iter().map(|p| p.1).min().unwrap().max(0);
        let max_y = points.iter().map(|p| p.1).max().unwrap().min(self.height as i16 - 1);
        let n = points.len();
        let mut result = Vec::new();

        for y in min_y..=max_y {
            let y32 = y as i32;
            let mut spans = Vec::new();

            if fill_rule == 1 {
                // Winding rule
                let mut crossings: Vec<(i32, i32)> = Vec::new();
                for i in 0..n {
                    let (x0, y0) = (points[i].0 as i32, points[i].1 as i32);
                    let (x1, y1) = (points[(i + 1) % n].0 as i32, points[(i + 1) % n].1 as i32);
                    if (y0 <= y32 && y1 > y32) || (y1 <= y32 && y0 > y32) {
                        let x = x0 + (y32 - y0) * (x1 - x0) / (y1 - y0);
                        let dir = if y0 < y1 { 1 } else { -1 };
                        crossings.push((x, dir));
                    }
                }
                crossings.sort_unstable_by_key(|c| c.0);
                let mut winding = 0i32;
                let mut span_start: Option<i32> = None;
                for (cx, dir) in &crossings {
                    let was_inside = winding != 0;
                    winding += dir;
                    let now_inside = winding != 0;
                    if !was_inside && now_inside {
                        span_start = Some(*cx);
                    } else if was_inside && !now_inside {
                        if let Some(sx_val) = span_start.take() {
                            let sx = sx_val.max(0) as i16;
                            let ex = (*cx).min(self.width as i32 - 1) as i16;
                            if ex >= sx { spans.push((sx, ex)); }
                        }
                    }
                }
            } else {
                // EvenOdd rule
                let mut intersections = Vec::new();
                for i in 0..n {
                    let (x0, y0) = (points[i].0 as i32, points[i].1 as i32);
                    let (x1, y1) = (points[(i + 1) % n].0 as i32, points[(i + 1) % n].1 as i32);
                    if (y0 <= y32 && y1 > y32) || (y1 <= y32 && y0 > y32) {
                        let x = x0 + (y32 - y0) * (x1 - x0) / (y1 - y0);
                        intersections.push(x);
                    }
                }
                intersections.sort_unstable();
                for pair in intersections.chunks(2) {
                    if pair.len() == 2 {
                        let sx = pair[0].max(0) as i16;
                        let ex = pair[1].min(self.width as i32 - 1) as i16;
                        if ex >= sx { spans.push((sx, ex)); }
                    }
                }
            }

            if !spans.is_empty() {
                result.push((y, spans));
            }
        }
        result
    }

    /// Simple fill_polygon (backward compat, EvenOdd, GXcopy).
    #[allow(dead_code)]
    pub fn fill_polygon(&mut self, points: &[(i16, i16)], color: u32) {
        self.fill_polygon_gc(points, color, 0, 3, 0xFFFFFFFF, &[]);
    }
}

/// Dash pattern state machine for dashed line drawing.
struct DashState {
    pattern: Vec<u8>,
    index: usize,
    remaining: u32,
    on: bool,
}

impl DashState {
    fn new(pattern: &[u8], offset: u16) -> Self {
        if pattern.is_empty() {
            return Self { pattern: vec![1], index: 0, remaining: 1, on: true };
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
        Self { pattern: pat, index: idx, remaining: rem, on }
    }

    fn is_on(&self) -> bool {
        self.on
    }

    fn advance(&mut self) {
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
fn point_in_clip_rects(x: i32, y: i32, rects: &[(i16, i16, u16, u16)]) -> bool {
    for &(rx, ry, rw, rh) in rects {
        if x >= rx as i32 && x < rx as i32 + rw as i32
            && y >= ry as i32 && y < ry as i32 + rh as i32
        {
            return true;
        }
    }
    false
}

/// Apply X11 GC raster operation function to source and destination pixels.
pub fn apply_gc_function(func: u8, src: u32, dst: u32) -> u32 {
    match func {
        0 => 0,                      // GXclear
        1 => src & dst,              // GXand
        2 => src & !dst,             // GXandReverse
        3 => src,                    // GXcopy
        4 => !src & dst,             // GXandInverted
        5 => dst,                    // GXnoop
        6 => src ^ dst,              // GXxor
        7 => src | dst,              // GXor
        8 => !(src | dst),           // GXnor
        9 => !(src ^ dst),           // GXequiv
        10 => !dst,                  // GXinvert
        11 => src | !dst,            // GXorReverse
        12 => !src,                  // GXcopyInverted
        13 => !src | dst,            // GXorInverted
        14 => !(src & dst),          // GXnand
        15 => 0xFFFFFFFF,            // GXset
        _ => src,                    // default to copy
    }
}

fn point_in_arc(dx: f64, dy: f64, start: f64, extent: f64) -> bool {
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
struct ArcChordData {
    chord_x1: f64,
    chord_y1: f64,
    cdx: f64,
    cdy: f64,
    mid_cross: f64,
}

impl ArcChordData {
    /// Create chord data only if arc_mode is Chord (0) and extent < full circle.
    fn new_if_chord(arc_mode: u8, angle2: i16, start_rad: f64, extent_rad: f64) -> Option<Self> {
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
        Some(Self { chord_x1, chord_y1, cdx, cdy, mid_cross })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Framebuffer basic operations
    // -----------------------------------------------------------------------

    #[test]
    fn new_framebuffer_is_zeroed() {
        let fb = Framebuffer::new(10, 10);
        assert_eq!(fb.width(), 10);
        assert_eq!(fb.height(), 10);
        assert!(fb.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn resize_preserves_content() {
        let mut fb = Framebuffer::new(4, 4);
        // Set pixel at (1,1) to red
        let off = 1 * fb.stride() + 1 * 4;
        fb.data_mut()[off] = 0;     // B
        fb.data_mut()[off + 1] = 0; // G
        fb.data_mut()[off + 2] = 255; // R
        fb.data_mut()[off + 3] = 255; // A
        fb.resize(8, 8);
        assert_eq!(fb.width(), 8);
        assert_eq!(fb.height(), 8);
        // Original pixel should still be at (1,1)
        let off2 = 1 * fb.stride() + 1 * 4;
        assert_eq!(fb.data()[off2 + 2], 255); // R preserved
    }

    #[test]
    fn resize_with_forget_gravity_clears() {
        let mut fb = Framebuffer::new(4, 4);
        let off = 1 * fb.stride() + 1 * 4;
        fb.data_mut()[off + 2] = 255;
        fb.resize_with_gravity(8, 8, 0); // Forget gravity
        // All pixels should be zeroed
        assert!(fb.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn resize_with_northwest_gravity_preserves_top_left() {
        let mut fb = Framebuffer::new(4, 4);
        let off = 0; // pixel (0,0)
        fb.data_mut()[off + 2] = 200;
        fb.resize_with_gravity(8, 8, 1); // NorthWest
        assert_eq!(fb.data()[off + 2], 200); // Top-left preserved
    }

    // -----------------------------------------------------------------------
    // fill_rect
    // -----------------------------------------------------------------------

    #[test]
    fn fill_rect_basic() {
        let mut fb = Framebuffer::new(10, 10);
        fb.fill_rect(2, 2, 3, 3, 0xFF0000FF); // Blue in ARGB
        // Check pixel at (3, 3) is set
        let off = 3 * fb.stride() + 3 * 4;
        assert_ne!(fb.data()[off], 0); // Should have some color
    }

    // -----------------------------------------------------------------------
    // apply_gc_function — all 16 GX operations
    // -----------------------------------------------------------------------

    #[test]
    fn gx_clear() {
        assert_eq!(apply_gc_function(0, 0xFFFFFFFF, 0xFFFFFFFF), 0);
    }

    #[test]
    fn gx_and() {
        assert_eq!(apply_gc_function(1, 0xFF00FF00, 0x00FF00FF), 0x00000000);
        assert_eq!(apply_gc_function(1, 0xFFFF0000, 0xFFFF0000), 0xFFFF0000);
    }

    #[test]
    fn gx_copy() {
        assert_eq!(apply_gc_function(3, 0xDEADBEEF, 0x12345678), 0xDEADBEEF);
    }

    #[test]
    fn gx_noop() {
        assert_eq!(apply_gc_function(5, 0xDEADBEEF, 0x12345678), 0x12345678);
    }

    #[test]
    fn gx_xor() {
        assert_eq!(apply_gc_function(6, 0xFF00FF00, 0x00FF00FF), 0xFFFFFFFF);
    }

    #[test]
    fn gx_or() {
        assert_eq!(apply_gc_function(7, 0xFF000000, 0x00FF0000), 0xFFFF0000);
    }

    #[test]
    fn gx_invert() {
        assert_eq!(apply_gc_function(10, 0x00000000, 0xFF00FF00), 0x00FF00FF);
    }

    #[test]
    fn gx_set() {
        assert_eq!(apply_gc_function(15, 0x00000000, 0x00000000), 0xFFFFFFFF);
    }

    #[test]
    fn gx_copy_inverted() {
        assert_eq!(apply_gc_function(12, 0xFF00FF00, 0x00000000), 0x00FF00FF);
    }

    // -----------------------------------------------------------------------
    // point_in_clip_rects
    // -----------------------------------------------------------------------

    #[test]
    fn clip_empty_allows_all() {
        // Empty clip = no clipping, but our function returns false for empty
        assert!(!point_in_clip_rects(5, 5, &[]));
    }

    #[test]
    fn clip_point_inside() {
        let rects = vec![(0i16, 0i16, 10u16, 10u16)];
        assert!(point_in_clip_rects(5, 5, &rects));
    }

    #[test]
    fn clip_point_outside() {
        let rects = vec![(0i16, 0i16, 10u16, 10u16)];
        assert!(!point_in_clip_rects(15, 15, &rects));
    }

    #[test]
    fn clip_point_on_boundary() {
        let rects = vec![(0i16, 0i16, 10u16, 10u16)];
        assert!(point_in_clip_rects(0, 0, &rects)); // top-left corner: inside
        assert!(!point_in_clip_rects(10, 10, &rects)); // bottom-right: outside (exclusive)
    }

    #[test]
    fn clip_multiple_rects() {
        let rects = vec![
            (0i16, 0i16, 5u16, 5u16),
            (10i16, 10i16, 5u16, 5u16),
        ];
        assert!(point_in_clip_rects(2, 2, &rects));
        assert!(point_in_clip_rects(12, 12, &rects));
        assert!(!point_in_clip_rects(7, 7, &rects)); // gap between rects
    }

    // -----------------------------------------------------------------------
    // DashState
    // -----------------------------------------------------------------------

    #[test]
    fn dash_state_alternates() {
        let mut ds = DashState::new(&[3, 2], 0);
        // First 3 steps: on
        assert!(ds.is_on());
        ds.advance();
        assert!(ds.is_on());
        ds.advance();
        assert!(ds.is_on());
        ds.advance();
        // Next 2 steps: off
        assert!(!ds.is_on());
        ds.advance();
        assert!(!ds.is_on());
        ds.advance();
        // Back to on
        assert!(ds.is_on());
    }

    #[test]
    fn dash_state_with_offset() {
        let ds = DashState::new(&[5, 3], 2);
        // Offset 2 into first dash (len=5), so 3 remaining in first on segment
        assert!(ds.is_on());
        assert_eq!(ds.remaining, 3);
    }

    // -----------------------------------------------------------------------
    // Dirty region tracking
    // -----------------------------------------------------------------------

    #[test]
    fn dirty_initially_none() {
        let mut fb = Framebuffer::new(10, 10);
        assert!(fb.take_dirty_pixels().is_none());
    }

    #[test]
    fn mark_dirty_creates_region() {
        let mut fb = Framebuffer::new(20, 20);
        fb.mark_dirty(5, 5, 10, 10);
        assert!(fb.dirty.is_some());
    }

    #[test]
    fn mark_dirty_merges_regions() {
        let mut fb = Framebuffer::new(100, 100);
        fb.mark_dirty(10, 10, 5, 5);
        fb.mark_dirty(20, 20, 5, 5);
        let dirty = fb.dirty.unwrap();
        // Should encompass both: (10,10) to (25,25) = (10, 10, 15, 15)
        assert_eq!(dirty.0, 10);
        assert_eq!(dirty.1, 10);
        assert!(dirty.2 >= 15);
        assert!(dirty.3 >= 15);
    }

    // -----------------------------------------------------------------------
    // CopyArea self-overlap safety
    // -----------------------------------------------------------------------

    #[test]
    fn copy_area_self_overlapping() {
        let mut fb = Framebuffer::new(10, 10);
        // Fill (0,0)-(4,4) with distinct value
        for y in 0..4usize {
            for x in 0..4usize {
                let off = y * fb.stride() + x * 4;
                fb.data_mut()[off] = 42;
                fb.data_mut()[off + 1] = 43;
                fb.data_mut()[off + 2] = 44;
                fb.data_mut()[off + 3] = 255;
            }
        }
        // Copy overlapping: src=(0,0) dst=(2,2) size=4x4
        fb.copy_area_self(0, 0, 2, 2, 4, 4);
        // (2,2) should now have the original (0,0) value
        let off = 2 * fb.stride() + 2 * 4;
        assert_eq!(fb.data()[off], 42);
    }
}
