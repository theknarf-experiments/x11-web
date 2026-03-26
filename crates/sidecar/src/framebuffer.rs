use pixman::{FormatCode, Image, Operation, Solid};

/// A server-side pixel buffer backed by a pixman Image.
pub struct Framebuffer {
    image: Image<'static, 'static>,
    width: u32,
    height: u32,
    /// Dirty region (x, y, w, h) that needs to be sent to the frontend.
    /// None = clean, Some = dirty rectangle.
    dirty: Option<(i32, i32, u32, u32)>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let image = Image::new(FormatCode::A8R8G8B8, width as usize, height as usize, true)
            .expect("Failed to create pixman image");
        Self {
            image,
            width,
            height,
            dirty: None,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn image(&self) -> &Image<'static, 'static> {
        &self.image
    }

    pub fn image_mut(&mut self) -> &mut Image<'static, 'static> {
        &mut self.image
    }

    /// Resize the framebuffer, preserving existing content where possible.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width == self.width && new_height == self.height {
            return;
        }

        let mut new_image = Image::new(
            FormatCode::A8R8G8B8,
            new_width as usize,
            new_height as usize,
            true,
        )
        .expect("Failed to create pixman image");

        // Copy old content
        let copy_w = new_width.min(self.width) as i32;
        let copy_h = new_height.min(self.height) as i32;
        if copy_w > 0 && copy_h > 0 {
            new_image.composite32(
                Operation::Src,
                &self.image,
                None,
                (0, 0),
                (0, 0),
                (0, 0),
                (copy_w, copy_h),
            );
        }

        self.image = new_image;
        self.width = new_width;
        self.height = new_height;
        self.mark_dirty(0, 0, new_width, new_height);
    }

    /// Mark a rectangular region as dirty.
    pub fn mark_dirty(&mut self, x: i32, y: i32, w: u32, h: u32) {
        // Clamp to image bounds
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

    /// Mark the entire framebuffer as dirty.
    pub fn mark_all_dirty(&mut self) {
        self.dirty = Some((0, 0, self.width, self.height));
    }

    /// Check if the framebuffer has dirty regions.
    pub fn is_dirty(&self) -> bool {
        self.dirty.is_some()
    }

    /// Extract the dirty region as BGRX pixel data and clear the dirty flag.
    /// Returns (x, y, width, height, pixel_data) or None if clean.
    pub fn take_dirty_pixels(&mut self) -> Option<(i16, i16, u16, u16, Vec<u8>)> {
        let (dx, dy, dw, dh) = self.dirty.take()?;

        if dw == 0 || dh == 0 {
            return None;
        }

        // Clamp to actual image dimensions
        let dx = dx.max(0).min(self.width as i32 - 1);
        let dy = dy.max(0).min(self.height as i32 - 1);
        let dw = dw.min((self.width as i32 - dx) as u32);
        let dh = dh.min((self.height as i32 - dy) as u32);

        let stride = self.image.stride();
        let data_ptr = unsafe { self.image.data() };
        if data_ptr.is_null() {
            return None;
        }

        // Extract the dirty rectangle pixels in BGRX format (4 bytes per pixel)
        let mut pixels = Vec::with_capacity(dw as usize * dh as usize * 4);
        let row_bytes = stride;

        for row in 0..dh as usize {
            let y_off = (dy as usize + row) * row_bytes;
            let x_off = dx as usize * 4; // 4 bytes per pixel (A8R8G8B8)
            let src = unsafe {
                std::slice::from_raw_parts(
                    (data_ptr as *const u8).add(y_off + x_off),
                    dw as usize * 4,
                )
            };
            // pixman A8R8G8B8 = BGRA in memory on little-endian
            // Our protocol expects BGRX (B, G, R, X) which matches
            pixels.extend_from_slice(src);
        }

        Some((dx as i16, dy as i16, dw as u16, dh as u16, pixels))
    }

    /// Fill a rectangle with a solid color (0x00RRGGBB format).
    pub fn fill_rect(&mut self, x: i16, y: i16, width: u16, height: u16, color: u32) {
        let r = ((color >> 16) & 0xFF) as u16;
        let g = ((color >> 8) & 0xFF) as u16;
        let b = (color & 0xFF) as u16;

        let color16: [u16; 4] = [r * 257, g * 257, b * 257, 0xFFFF];

        let rects = [pixman::ffi::pixman_rectangle16_t {
            x,
            y,
            width,
            height,
        }];

        let _ = self.image.fill_rectangles(Operation::Src, color16, &rects);
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Put raw pixel data (BGRX format) into the framebuffer.
    pub fn put_image(&mut self, x: i16, y: i16, width: u16, height: u16, data: &[u8]) {
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }

        // Create a temporary pixman image from the source data
        let src_stride = width as usize * 4;
        let expected_len = src_stride * height as usize;
        if data.len() < expected_len {
            return;
        }

        // We need a mutable u32 slice for pixman
        let mut pixels: Vec<u32> = Vec::with_capacity(width as usize * height as usize);
        for i in 0..(width as usize * height as usize) {
            let off = i * 4;
            if off + 3 < data.len() {
                pixels.push(u32::from_le_bytes([
                    data[off],
                    data[off + 1],
                    data[off + 2],
                    data[off + 3],
                ]));
            }
        }

        if let Ok(src_image) = Image::from_slice_mut(
            FormatCode::A8R8G8B8,
            width as usize,
            height as usize,
            &mut pixels,
            src_stride,
            false,
        ) {
            self.image.composite32(
                Operation::Src,
                &src_image,
                None,
                (0, 0),
                (0, 0),
                (x as i32, y as i32),
                (width as i32, height as i32),
            );
        }

        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
    }

    /// Copy a region from another framebuffer (or self) into this one.
    pub fn copy_area(
        &mut self,
        src: &Framebuffer,
        src_x: i16,
        src_y: i16,
        dst_x: i16,
        dst_y: i16,
        width: u16,
        height: u16,
    ) {
        self.image.composite32(
            Operation::Src,
            &src.image,
            None,
            (src_x as i32, src_y as i32),
            (0, 0),
            (dst_x as i32, dst_y as i32),
            (width as i32, height as i32),
        );
        self.mark_dirty(dst_x as i32, dst_y as i32, width as u32, height as u32);
    }

    /// Composite a source image onto this framebuffer with the given operation.
    pub fn composite(
        &mut self,
        op: Operation,
        src: &pixman::ImageRef,
        mask: Option<&pixman::ImageRef>,
        src_x: i32,
        src_y: i32,
        mask_x: i32,
        mask_y: i32,
        dst_x: i32,
        dst_y: i32,
        width: i32,
        height: i32,
    ) {
        self.image.composite32(
            op,
            src,
            mask,
            (src_x, src_y),
            (mask_x, mask_y),
            (dst_x, dst_y),
            (width, height),
        );
        self.mark_dirty(dst_x, dst_y, width as u32, height as u32);
    }
}
