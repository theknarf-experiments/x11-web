use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

/// A server-side pixel buffer using pure Rust (no pixman).
/// Format: A8R8G8B8 (4 bytes per pixel, BGRA in memory on little-endian).
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

        let copy_w = new_width.min(self.width) as usize * 4;
        let copy_h = new_height.min(self.height) as usize;
        for row in 0..copy_h {
            let src_start = row * self.stride;
            let dst_start = row * new_stride;
            new_data[dst_start..dst_start + copy_w]
                .copy_from_slice(&self.data[src_start..src_start + copy_w]);
        }

        self.data = new_data;
        self.width = new_width;
        self.height = new_height;
        self.stride = new_stride;
        self.mark_dirty(0, 0, new_width, new_height);
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

    pub fn mark_all_dirty(&mut self) {
        self.dirty = Some((0, 0, self.width, self.height));
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_some()
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

    /// Fill a rectangle with a solid color (0x00RRGGBB format).
    pub fn fill_rect(&mut self, x: i16, y: i16, width: u16, height: u16, color: u32) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        let pixel = [b, g, r, 0xFF]; // BGRA with full alpha

        for row in 0..height as i32 {
            let dy = y as i32 + row;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..width as i32 {
                let dx = x as i32 + col;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let off = dy as usize * self.stride + dx as usize * 4;
                if off + 3 < self.data.len() {
                    self.data[off..off + 4].copy_from_slice(&pixel);
                }
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

    /// Draw a single pixel at (x, y) with the given color.
    pub fn draw_point(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        let off = y as usize * self.stride + x as usize * 4;
        if off + 3 < self.data.len() {
            self.data[off] = b;
            self.data[off + 1] = g;
            self.data[off + 2] = r;
            self.data[off + 3] = 0xFF;
        }
        self.mark_dirty(x, y, 1, 1);
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, line_width: u16) {
        if line_width <= 1 {
            self.bresenham_line(x0, y0, x1, y1, color);
        } else {
            let hw = (line_width / 2) as i32;
            if y0 == y1 {
                let min_x = x0.min(x1);
                let max_x = x0.max(x1);
                self.fill_rect(
                    min_x as i16,
                    (y0 - hw) as i16,
                    (max_x - min_x + 1) as u16,
                    line_width,
                    color,
                );
            } else if x0 == x1 {
                let min_y = y0.min(y1);
                let max_y = y0.max(y1);
                self.fill_rect(
                    (x0 - hw) as i16,
                    min_y as i16,
                    line_width,
                    (max_y - min_y + 1) as u16,
                    color,
                );
            } else {
                for d in -hw..=hw {
                    let dx = (x1 - x0).abs();
                    let dy = (y1 - y0).abs();
                    if dx >= dy {
                        self.bresenham_line(x0, y0 + d, x1, y1 + d, color);
                    } else {
                        self.bresenham_line(x0 + d, y0, x1 + d, y1, color);
                    }
                }
            }
        }
    }

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

    /// Draw an elliptical arc.
    pub fn draw_arc(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        filled: bool,
        color: u32,
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

            for py in min_y..=max_y {
                for px in min_x..=max_x {
                    let ddx = (px as f64 - cx) / rx;
                    let ddy = (py as f64 - cy) / ry;
                    if ddx * ddx + ddy * ddy <= 1.0 {
                        if angle2.abs() >= 360 * 64
                            || point_in_arc(ddx, ddy, start_rad, extent_rad)
                        {
                            self.draw_point(px, py, color);
                        }
                    }
                }
            }
        } else {
            let steps = ((rx + ry) * 2.0).max(64.0) as usize;
            let mut prev_x = None;
            let mut prev_y = None;

            for i in 0..=steps {
                let t = start_rad + extent_rad * (i as f64 / steps as f64);
                let px = (cx + rx * t.cos()) as i32;
                let py = (cy - ry * t.sin()) as i32;

                if let (Some(lx), Some(ly)) = (prev_x, prev_y) {
                    self.bresenham_line(lx, ly, px, py, color);
                }
                prev_x = Some(px);
                prev_y = Some(py);
            }
        }
    }

    /// Fill a convex polygon using scan-line fill.
    pub fn fill_polygon(&mut self, points: &[(i16, i16)], color: u32) {
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
            let mut intersections = Vec::new();
            let n = points.len();
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
                        self.fill_rect(start_x, y as i16, (end_x - start_x + 1) as u16, 1, color);
                    }
                }
            }
        }
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
