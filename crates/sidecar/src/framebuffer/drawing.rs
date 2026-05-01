use super::{
    apply_gc_function, build_clip_mask, build_dash, point_in_clip_rects, read_pixel,
    skia_color, skia_eligible, write_pixel, DashState, Framebuffer,
};
use tiny_skia::{FillRule, Paint, PathBuilder, Transform};

impl Framebuffer {
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
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
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

        // Fast path: GXcopy lines (solid or dashed) go to tiny-skia
        // for AA stroking with proper caps and Stroke::dash.
        if skia_eligible(gc_func, plane_mask) {
            let mut pb = PathBuilder::new();
            pb.move_to(x0 as f32, y0 as f32);
            pb.line_to(x1 as f32, y1 as f32);
            if let Some(path) = pb.finish() {
                let dash = build_dash(line_style, dash_list, dash_offset);
                // DoubleDash: stroke background colour with the inverse
                // dash pattern in the gaps before drawing the foreground.
                if line_style == 2 {
                    if let Some(ref d) = dash {
                        self.stroke_path_skia(
                            &path,
                            background,
                            line_width,
                            cap_style,
                            Some(d.inverted()),
                            clip_rects,
                        );
                    }
                }
                self.stroke_path_skia(&path, color, line_width, cap_style, dash, clip_rects);
                let pad = (line_width as i32 / 2 + 1).max(1);
                let min_x = x0.min(x1) - pad;
                let min_y = y0.min(y1) - pad;
                let w = (x0.max(x1) - x0.min(x1) + pad * 2 + 1) as u32;
                let h = (y0.max(y1) - y0.min(y1) + pad * 2 + 1) as u32;
                self.mark_dirty(min_x, min_y, w, h);
                return;
            }
        }

        // Fallback (non-GXcopy raster ops): single-pixel Bresenham
        // with optional dash support. Wide lines under raster-op
        // blends are rare in real apps; we accept the visual
        // simplification.
        let dashes = if (line_style == 1 || line_style == 2) && !dash_list.is_empty() {
            Some(DashState::new(dash_list, dash_offset))
        } else {
            None
        };
        self.bresenham_line_gc(
            x0, y0, x1, y1, color, gc_func, plane_mask, cap_style, dashes, line_style,
            background, clip_rects,
        );
    }

    /// Draw a polyline (sequence of connected line segments) with join style support.
    ///
    /// X11 join styles map directly onto tiny-skia's `LineJoin` for the
    /// GXcopy fast path; the fallback draws each segment independently
    /// via `draw_line_gc` and accepts that interior vertices look like
    /// adjacent butt-capped strokes.
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

        // Fast path: build a single multi-segment path and let tiny-skia
        // handle joins, caps, and dashes natively.
        if skia_eligible(gc_func, plane_mask) {
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].0 as f32, points[0].1 as f32);
            for &(x, y) in &points[1..] {
                pb.line_to(x as f32, y as f32);
            }
            if let Some(path) = pb.finish() {
                let dash = build_dash(line_style, dash_list, dash_offset);
                let join = match join_style {
                    1 => tiny_skia::LineJoin::Round,
                    2 => tiny_skia::LineJoin::Bevel,
                    _ => tiny_skia::LineJoin::Miter,
                };
                if line_style == 2 {
                    if let Some(ref d) = dash {
                        self.stroke_path_skia_full(
                            &path,
                            background,
                            line_width,
                            cap_style,
                            join,
                            Some(d.inverted()),
                            clip_rects,
                        );
                    }
                }
                self.stroke_path_skia_full(
                    &path,
                    color,
                    line_width,
                    cap_style,
                    join,
                    dash,
                    clip_rects,
                );
                let xs = points.iter().map(|p| p.0);
                let ys = points.iter().map(|p| p.1);
                let pad = (line_width as i32 / 2 + 1).max(1);
                let min_x = xs.clone().min().unwrap_or(0) - pad;
                let min_y = ys.clone().min().unwrap_or(0) - pad;
                let max_x = xs.max().unwrap_or(0) + pad;
                let max_y = ys.max().unwrap_or(0) + pad;
                self.mark_dirty(min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32);
                return;
            }
        }

        // Fallback: draw each segment independently. Joins under
        // non-GXcopy raster ops aren't supported; segments butt up
        // against each other.
        for w in points.windows(2) {
            self.draw_line_gc(
                w[0].0,
                w[0].1,
                w[1].0,
                w[1].1,
                color,
                line_width,
                gc_func,
                plane_mask,
                line_style,
                cap_style,
                join_style,
                dash_offset,
                dash_list,
                background,
                clip_rects,
            );
        }
    }

    /// Bresenham line with GC raster op, dashes, cap_style, and clip rects.
    fn bresenham_line_gc(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
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

    /// Draw a line using a tile pattern for per-pixel color lookup.
    /// Per X11 spec, when fill_style is Tiled, line pixels use the tile color at each (x,y).
    pub fn draw_line_tiled(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        tile_data: &[u8],
        tile_w: u32,
        tile_h: u32,
        ts_x: i16,
        ts_y: i16,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if tile_w == 0 || tile_h == 0 || tile_data.is_empty() {
            return;
        }
        let tile_stride = tile_w as usize * 4;
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
            let skip = cap_style == 0 && is_last && !is_first;

            if !skip {
                let tile_x = ((x - ts_x as i32) % tile_w as i32 + tile_w as i32) as u32 % tile_w;
                let tile_y = ((y - ts_y as i32) % tile_h as i32 + tile_h as i32) as u32 % tile_h;
                let off = tile_y as usize * tile_stride + tile_x as usize * 4;
                if off + 3 < tile_data.len() {
                    let color = read_pixel(tile_data, off);
                    self.draw_point_gc(x, y, color, gc_func, plane_mask, clip_rects);
                }
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

    /// Draw a line using a stipple pattern for per-pixel fill.
    /// Per X11 spec, when fill_style is Stippled, only foreground pixels where
    /// stipple bit is set are drawn. OpaqueStippled draws bg where bit is unset.
    pub fn draw_line_stippled(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        fg: u32,
        bg: u32,
        stipple_data: &[u8],
        stipple_w: u32,
        stipple_h: u32,
        ts_x: i16,
        ts_y: i16,
        opaque: bool,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if stipple_w == 0 || stipple_h == 0 || stipple_data.is_empty() {
            return;
        }
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;
        let mut is_first = true;

        // Stipple is a 1-bit-per-pixel bitmap; stride is ceil(stipple_w/8) bytes per row.
        // For full-depth pixmap stipples, framebuffer storage is 32bpp RGBA;
        // we treat any non-zero RGB triple as "set".
        // For 1-bit depth stipples, data is 1 bit per pixel row-major.
        // Format is detected by data size.
        let is_1bpp = stipple_data.len() < (stipple_w * stipple_h * 4) as usize;
        let bpp_stride = if is_1bpp {
            ((stipple_w + 7) / 8) as usize
        } else {
            stipple_w as usize * 4
        };

        loop {
            let is_last = x == x1 && y == y1;
            let skip = cap_style == 0 && is_last && !is_first;

            if !skip {
                let sx_pos =
                    ((x - ts_x as i32) % stipple_w as i32 + stipple_w as i32) as u32 % stipple_w;
                let sy_pos =
                    ((y - ts_y as i32) % stipple_h as i32 + stipple_h as i32) as u32 % stipple_h;

                let bit_set = if is_1bpp {
                    let byte_idx = sy_pos as usize * bpp_stride + (sx_pos / 8) as usize;
                    if byte_idx < stipple_data.len() {
                        stipple_data[byte_idx] & (1 << (sx_pos % 8)) != 0
                    } else {
                        false
                    }
                } else {
                    // 32bpp: check if pixel is non-zero (any channel)
                    let off = sy_pos as usize * bpp_stride + sx_pos as usize * 4;
                    if off + 3 < stipple_data.len() {
                        stipple_data[off] != 0
                            || stipple_data[off + 1] != 0
                            || stipple_data[off + 2] != 0
                    } else {
                        false
                    }
                };

                if bit_set {
                    self.draw_point_gc(x, y, fg, gc_func, plane_mask, clip_rects);
                } else if opaque {
                    self.draw_point_gc(x, y, bg, gc_func, plane_mask, clip_rects);
                }
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
        x: i32,
        y: i32,
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
        let dst = read_pixel(&self.data, off);
        let result = apply_gc_function(gc_func, color, dst);
        let masked = (result & plane_mask) | (dst & !plane_mask);
        write_pixel(&mut self.data, off, masked, 0xFF);
        self.mark_dirty(x, y, 1, 1);
    }

    /// Fill a small circle (for round line caps/joins).
    pub(crate) fn fill_circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if radius <= 0 {
            return;
        }
        if skia_eligible(gc_func, plane_mask) {
            if let Some(rect) = tiny_skia::Rect::from_xywh(
                (cx - radius) as f32,
                (cy - radius) as f32,
                (radius * 2) as f32,
                (radius * 2) as f32,
            ) {
                if let Some(path) = PathBuilder::from_oval(rect) {
                    let mut paint = Paint::default();
                    paint.set_color(skia_color(color));
                    paint.anti_alias = true;
                    let clip_mask = build_clip_mask(self.width, self.height, clip_rects);
                    let _ = self.with_pixmap_mut(|pm| {
                        pm.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            clip_mask.as_ref(),
                        );
                    });
                    self.mark_dirty(
                        cx - radius,
                        cy - radius,
                        (radius * 2 + 1) as u32,
                        (radius * 2 + 1) as u32,
                    );
                    return;
                }
            }
        }
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.draw_point_gc(cx + dx, cy + dy, color, gc_func, plane_mask, clip_rects);
                }
            }
        }
    }
}

