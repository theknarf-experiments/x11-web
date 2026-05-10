use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, PixmapMut, PixmapRef,
    SpreadMode, Stroke, StrokeDash, Transform,
};
use x11rb_protocol::protocol::xproto::{CapStyle, Gravity, LineStyle, GX};

/// Full plane mask: all 32 bit-planes are affected by GC operations.
pub(crate) const PLANE_MASK_ALL: u32 = u32::MAX;

/// Maximum 8-bit alpha value (fully opaque). Used as the divisor in
/// alpha-blending arithmetic and to mark output pixels opaque.
pub(crate) const ALPHA_MAX: u32 = 255;

mod drawing;
mod shapes;
#[cfg(test)]
mod tests;

/// Whether a drawing call can be served by a tiny-skia fast path.
///
/// tiny-skia handles Porter-Duff Source-Over compositing only, so the
/// X11 GC raster ops (XOR, AND, ...) and partial plane masks fall
/// through to the hand-rolled blitters. The common `(GXcopy, full
/// plane mask)` case is the only one we route to tiny-skia.
#[inline]
fn skia_eligible(gc_func: u8, plane_mask: u32) -> bool {
    GX::from(gc_func) == GX::COPY && plane_mask == PLANE_MASK_ALL
}

/// Unpack an `0x00RRGGBB` X11 colour into its 8-bit (R, G, B) channels.
/// The X11 wire format always passes pixel values packed in this order
/// regardless of visual class.
#[inline]
pub(crate) fn unpack_rgb(rgb: u32) -> (u8, u8, u8) {
    (
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    )
}

/// Inverse of [`unpack_rgb`]: pack 8-bit channels into a `0x00RRGGBB` u32.
#[inline]
pub(crate) fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Convert an `0x00RRGGBB` X11 color into a tiny-skia opaque [`Color`].
#[inline]
fn skia_color(rgb: u32) -> Color {
    let (r, g, b) = unpack_rgb(rgb);
    Color::from_rgba8(r, g, b, 0xFF)
}

/// X11 dash pattern translated into a tiny-skia stroke dash.
#[derive(Clone)]
pub(crate) struct DashSpec {
    pub(crate) array: Vec<f32>,
    pub(crate) offset: f32,
}

impl DashSpec {
    /// Return the inverted dash (swaps on/off runs by shifting the
    /// offset by the first run length). Used to draw the *background*
    /// colour in DoubleDash gaps via a second stroke pass.
    pub(crate) fn inverted(&self) -> Self {
        let first = self.array.first().copied().unwrap_or(0.0);
        Self {
            array: self.array.clone(),
            offset: self.offset + first,
        }
    }
}

/// Translate X11 line_style + dash_list + dash_offset into a tiny-skia
/// dash spec. Returns `None` for solid lines or empty patterns.
pub(crate) fn build_dash(line_style: u8, dash_list: &[u8], dash_offset: u16) -> Option<DashSpec> {
    let style = LineStyle::from(line_style);
    if (style != LineStyle::ON_OFF_DASH && style != LineStyle::DOUBLE_DASH) || dash_list.is_empty()
    {
        return None;
    }
    // X11 dash arrays may be odd-length; tiny-skia requires even-length
    // pairs (on, off, ...). Duplicate the pattern to make it even.
    let mut array: Vec<f32> = dash_list.iter().map(|&n| n as f32).collect();
    if array.len() % 2 != 0 {
        let dup = array.clone();
        array.extend(dup);
    }
    Some(DashSpec {
        array,
        offset: dash_offset as f32,
    })
}

/// Materialise a 1bpp X11 stipple as an RGBA tile suitable for use
/// with [`Framebuffer::fill_path_tiled`].
///
/// `opaque=false` (Stippled) leaves cleared bits transparent so the
/// destination shows through; `opaque=true` (OpaqueStippled) paints
/// them with `bg`.
pub(crate) fn stipple_to_tile(
    stipple_data: &[u8],
    stipple_w: u32,
    stipple_h: u32,
    fg: u32,
    bg: u32,
    opaque: bool,
) -> Vec<u8> {
    let stride = stipple_w.div_ceil(8) as usize;
    let mut out = vec![0u8; (stipple_w * stipple_h * 4) as usize];
    let (fg_r, fg_g, fg_b) = unpack_rgb(fg);
    let (bg_r, bg_g, bg_b) = unpack_rgb(bg);
    for sy in 0..stipple_h {
        for sx in 0..stipple_w {
            let byte_idx = (sy as usize) * stride + (sx as usize / 8);
            let bit_set = stipple_data
                .get(byte_idx)
                .is_some_and(|b| (b >> (sx % 8)) & 1 != 0);
            let off = ((sy * stipple_w + sx) * 4) as usize;
            if bit_set {
                out[off] = fg_r;
                out[off + 1] = fg_g;
                out[off + 2] = fg_b;
                out[off + 3] = 0xFF;
            } else if opaque {
                out[off] = bg_r;
                out[off + 1] = bg_g;
                out[off + 2] = bg_b;
                out[off + 3] = 0xFF;
            }
            // else: leave [0, 0, 0, 0] — transparent.
        }
    }
    out
}

/// Build an `Option<tiny_skia::Mask>` from X11 GC clip rectangles.
/// Returns `None` if `rects` is empty (no clipping needed).
fn build_clip_mask(
    width: u32,
    height: u32,
    rects: &[(i16, i16, u16, u16)],
) -> Option<tiny_skia::Mask> {
    if rects.is_empty() {
        return None;
    }
    let mut mask = tiny_skia::Mask::new(width, height)?;
    let mut pb = PathBuilder::new();
    for &(rx, ry, rw, rh) in rects {
        if rw == 0 || rh == 0 {
            continue;
        }
        if let Some(rect) = tiny_skia::Rect::from_xywh(rx as f32, ry as f32, rw as f32, rh as f32) {
            pb.push_rect(rect);
        }
    }
    let path = pb.finish()?;
    mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
    Some(mask)
}

/// A server-side pixel buffer.
///
/// Storage format: packed RGBA8888 — 4 bytes per pixel, byte order
/// `[R, G, B, A]`. This matches what the canvas/`putImageData` API on
/// the frontend expects, the format `tiny-skia` operates on directly,
/// and what the macOS sidecar emits. The X11 wire format on a
/// little-endian visual is BGRA, so PutImage/GetImage handlers swap
/// channel 0 and 2 at the wire boundary.
///
/// Logical pixel colors throughout this module use the X11 convention
/// `0x00RRGGBB` (R in bits 16-23). Use [`read_pixel`] / [`write_pixel`]
/// to convert between storage bytes and that u32.
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
    /// See `x11rb_protocol::protocol::xproto::Gravity` for the variant set.
    /// `BIT_FORGET` discards content; all other values translate old pixels
    /// so the corresponding edge / corner / centre stays aligned in the new
    /// geometry. The caller should generate Expose events for `BIT_FORGET`.
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
        let gravity = Gravity::from(gravity);

        // For Forget gravity, just allocate a blank buffer — no pixel copy.
        if gravity == Gravity::BIT_FORGET {
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
            Gravity::NORTH_WEST => (0, 0),
            Gravity::NORTH => (dw / 2, 0),
            Gravity::NORTH_EAST => (dw, 0),
            Gravity::WEST => (0, dh / 2),
            Gravity::CENTER => (dw / 2, dh / 2),
            Gravity::EAST => (dw, dh / 2),
            Gravity::SOUTH_WEST => (0, dh),
            Gravity::SOUTH => (dw / 2, dh),
            Gravity::SOUTH_EAST => (dw, dh),
            Gravity::STATIC => (0, 0),
            // Includes BIT_FORGET / WIN_UNMAP (both = 0) and unknown values.
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

    /// Extract the dirty region as raw RGBA bytes and clear the
    /// dirty flag. The caller is responsible for any post-processing
    /// (shape clipping, etc.) and for encoding to WebP before
    /// sending — encoding here would force every caller to operate
    /// on opaque compressed bytes, which silently corrupts when
    /// downstream code treats them as a pixel array.
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
        Some((dx as i16, dy as i16, dw as u16, dh as u16, pixels))
    }

    /// Fill a rectangle with a solid color (0x00RRGGBB format).
    pub fn fill_rect(&mut self, x: i16, y: i16, width: u16, height: u16, color: u32) {
        let (r, g, b) = unpack_rgb(color);
        let pixel = [r, g, b, 0xFF];

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
    /// For `GX::COPY` this behaves identically to [`fill_rect`]. For other
    /// operations the existing pixel value is read, combined with `color`
    /// through `apply_gc_function`, and written back. `plane_mask` selects
    /// which bit-planes are affected (`PLANE_MASK_ALL` means all).
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
        let func = GX::from(function);
        // Fast-path: GXcopy with full plane mask is the common case.
        if func == GX::COPY && plane_mask == PLANE_MASK_ALL {
            self.fill_rect(x, y, width, height, color);
            return;
        }

        // GXnoop - nothing to do.
        if func == GX::NOOP {
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
                let dst = read_pixel(&self.data, off);
                let result = apply_gc_function(function, color, dst);
                // Apply plane mask: affected planes come from result,
                // unaffected planes keep the dst value.
                let masked = (result & plane_mask) | (dst & !plane_mask);
                write_pixel(&mut self.data, off, masked, 0xFF);
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
                    let da = ALPHA_MAX - sa;
                    for c in 0..3 {
                        let s = data[src_off + c] as u32;
                        let d = self.data[dst_off + c] as u32;
                        self.data[dst_off + c] = ((s * sa + d * da) / ALPHA_MAX) as u8;
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
        if GX::from(function) == GX::COPY && plane_mask == PLANE_MASK_ALL && clip_rects.is_empty() {
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
                let (sr, sg, sb) = if src_a == 0xFF {
                    (data[src_off], data[src_off + 1], data[src_off + 2])
                } else {
                    let sa = src_a as u32;
                    let da = ALPHA_MAX - sa;
                    let r = ((data[src_off] as u32 * sa + self.data[dst_off] as u32 * da)
                        / ALPHA_MAX) as u8;
                    let g = ((data[src_off + 1] as u32 * sa + self.data[dst_off + 1] as u32 * da)
                        / ALPHA_MAX) as u8;
                    let b = ((data[src_off + 2] as u32 * sa + self.data[dst_off + 2] as u32 * da)
                        / ALPHA_MAX) as u8;
                    (r, g, b)
                };

                // Apply GC function and plane_mask
                let src_color = (sr as u32) << 16 | (sg as u32) << 8 | sb as u32;
                let dst_color = read_pixel(&self.data, dst_off);
                let result = apply_gc_function(function, src_color, dst_color);
                let masked = (result & plane_mask) | (dst_color & !plane_mask);
                write_pixel(&mut self.data, dst_off, masked, 0xFF);
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
        if GX::from(function) == GX::COPY && plane_mask == PLANE_MASK_ALL && clip_rects.is_empty() {
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
                let src_color = read_pixel(data, src_off);
                let dst_off = dy as usize * self.stride + dx as usize * 4;
                if dst_off + 3 >= self.data.len() {
                    continue;
                }
                let dst_color = read_pixel(&self.data, dst_off);
                let result = apply_gc_function(function, src_color, dst_color);
                let masked = (result & plane_mask) | (dst_color & !plane_mask);
                write_pixel(&mut self.data, dst_off, masked, 0xFF);
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
        let dst = read_pixel(&self.data, off);
        let result = apply_gc_function(gc_func, color, dst);
        write_pixel(&mut self.data, off, result, 0xFF);
        self.mark_dirty(x, y, 1, 1);
    }

    /// Draw a single pixel with GXcopy (the common case).
    pub fn draw_point(&mut self, x: i32, y: i32, color: u32) {
        self.draw_point_with_func(x, y, color, 3);
    }

    /// Wrap the framebuffer's storage as a tiny-skia [`PixmapMut`] for
    /// the duration of `f`. Returns `None` if tiny-skia rejects the
    /// (width, height) (e.g. zero size).
    fn with_pixmap_mut<R>(&mut self, f: impl FnOnce(&mut PixmapMut<'_>) -> R) -> Option<R> {
        let mut pm = PixmapMut::from_bytes(&mut self.data, self.width, self.height)?;
        Some(f(&mut pm))
    }

    /// Fill a tiny-skia [`Path`] with an X11 tile pattern.
    /// `tile_data` must be a valid RGBA pixel buffer of `tile_w × tile_h`.
    pub(crate) fn fill_path_tiled(
        &mut self,
        path: &Path,
        tile_data: &[u8],
        tile_w: u32,
        tile_h: u32,
        ts_x: i16,
        ts_y: i16,
        fill_rule: FillRule,
        clip_rects: &[(i16, i16, u16, u16)],
    ) -> bool {
        let Some(tile_pm) = PixmapRef::from_bytes(tile_data, tile_w, tile_h) else {
            return false;
        };
        let mut paint = Paint::default();
        paint.shader = Pattern::new(
            tile_pm,
            SpreadMode::Repeat,
            FilterQuality::Nearest,
            1.0,
            Transform::from_translate(ts_x as f32, ts_y as f32),
        );
        paint.anti_alias = false;
        let clip_mask = build_clip_mask(self.width, self.height, clip_rects);
        self.with_pixmap_mut(|pm| {
            pm.fill_path(
                path,
                &paint,
                fill_rule,
                Transform::identity(),
                clip_mask.as_ref(),
            );
        })
        .is_some()
    }

    /// Stroke a tiny-skia [`Path`] with optional X11 dashing.
    pub(crate) fn stroke_path_skia(
        &mut self,
        path: &Path,
        color: u32,
        line_width: u16,
        cap_style: u8,
        dash: Option<DashSpec>,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        self.stroke_path_skia_full(
            path,
            color,
            line_width,
            cap_style,
            tiny_skia::LineJoin::Miter,
            dash,
            clip_rects,
        );
    }

    /// As [`stroke_path_skia`], but also lets callers pick the line-join
    /// style. Used by polyline rendering.
    pub(crate) fn stroke_path_skia_full(
        &mut self,
        path: &Path,
        color: u32,
        line_width: u16,
        cap_style: u8,
        line_join: tiny_skia::LineJoin,
        dash: Option<DashSpec>,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        let mut paint = Paint::default();
        paint.set_color(skia_color(color));
        paint.anti_alias = true;
        let mut stroke = Stroke::default();
        stroke.width = line_width.max(1) as f32;
        stroke.line_cap = match CapStyle::from(cap_style) {
            CapStyle::ROUND => tiny_skia::LineCap::Round,
            CapStyle::PROJECTING => tiny_skia::LineCap::Square,
            _ => tiny_skia::LineCap::Butt,
        };
        stroke.line_join = line_join;
        if let Some(d) = dash {
            stroke.dash = StrokeDash::new(d.array, d.offset);
        }
        let clip_mask = build_clip_mask(self.width, self.height, clip_rects);
        let _ = self.with_pixmap_mut(|pm| {
            pm.stroke_path(
                path,
                &paint,
                &stroke,
                Transform::identity(),
                clip_mask.as_ref(),
            );
        });
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
        let dst = read_pixel(&self.data, off);
        let result = apply_gc_function(gc_func, color, dst);
        let masked = (result & plane_mask) | (dst & !plane_mask);
        write_pixel(&mut self.data, off, masked, 0xFF);
        self.mark_dirty(x, y, 1, 1);
    }
}

/// Read an RGBA-storage pixel at byte offset `off` as a `0x00RRGGBB` u32.
#[inline]
pub(crate) fn read_pixel(data: &[u8], off: usize) -> u32 {
    pack_rgb(data[off], data[off + 1], data[off + 2])
}

/// Write `0x00RRGGBB` u32 + alpha to RGBA-storage at byte offset `off`.
#[inline]
pub(crate) fn write_pixel(data: &mut [u8], off: usize, color: u32, alpha: u8) {
    let (r, g, b) = unpack_rgb(color);
    data[off] = r;
    data[off + 1] = g;
    data[off + 2] = b;
    data[off + 3] = alpha;
}

/// In-place swap of channels 0 and 2 (B↔R) over a packed pixel buffer.
/// Used at the X11 wire boundary, where PutImage/GetImage carry BGRA but
/// our framebuffer stores RGBA.
pub fn swap_br_in_place(data: &mut [u8]) {
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
}

/// Allocate a new buffer with channel 0 and 2 swapped (B↔R).
pub fn swap_br(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    swap_br_in_place(&mut out);
    out
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
    match GX::from(func) {
        GX::CLEAR => 0,
        GX::AND => src & dst,
        GX::AND_REVERSE => src & !dst,
        GX::COPY => src,
        GX::AND_INVERTED => !src & dst,
        GX::NOOP => dst,
        GX::XOR => src ^ dst,
        GX::OR => src | dst,
        GX::NOR => !(src | dst),
        GX::EQUIV => !(src ^ dst),
        GX::INVERT => !dst,
        GX::OR_REVERSE => src | !dst,
        GX::COPY_INVERTED => !src,
        GX::OR_INVERTED => !src | dst,
        GX::NAND => !(src & dst),
        GX::SET => 0xFFFFFFFF,
        // Unknown function: behave as GXcopy.
        _ => src,
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
