use super::{
    apply_gc_function, point_in_clip_rects, read_pixel, write_pixel, DashState, Framebuffer,
};

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

        // Dashed line state
        let dashes = if (line_style == 1 || line_style == 2) && !dash_list.is_empty() {
            Some(DashState::new(dash_list, dash_offset))
        } else {
            None
        };

        if line_width <= 1 {
            self.bresenham_line_gc(
                x0, y0, x1, y1, color, gc_func, plane_mask, cap_style, dashes, line_style,
                background, clip_rects,
            );
        } else {
            // Wide line with dash support
            let hw = (line_width / 2) as i32;
            let is_dashed = dashes.is_some();

            if y0 == y1 {
                // Horizontal wide line
                self.draw_wide_line_horiz(
                    x0, y0, x1, hw, line_width, color, gc_func, plane_mask, cap_style, line_style,
                    background, clip_rects, dashes,
                );
            } else if x0 == x1 {
                // Vertical wide line
                self.draw_wide_line_vert(
                    x0, y0, y1, hw, line_width, color, gc_func, plane_mask, cap_style, line_style,
                    background, clip_rects, dashes,
                );
            } else {
                // Diagonal wide line
                self.draw_wide_line_diagonal(
                    x0, y0, x1, y1, hw, color, gc_func, plane_mask, cap_style, line_style,
                    background, clip_rects, dashes,
                );
            }

            // Round cap: draw filled circles at endpoints (only for solid lines;
            // dashed lines handle caps per-segment)
            if cap_style == 2 && line_width > 2 && !is_dashed {
                self.fill_circle(x0, y0, hw, color, gc_func, plane_mask, clip_rects);
                self.fill_circle(x1, y1, hw, color, gc_func, plane_mask, clip_rects);
            }
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
                        jx as i32, jy as i32, hw as i32, color, gc_func, plane_mask, clip_rects,
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

    /// Draw a wide horizontal line with dash support.
    #[allow(clippy::too_many_arguments)]
    fn draw_wide_line_horiz(
        &mut self,
        x0: i32,
        y: i32,
        x1: i32,
        hw: i32,
        line_width: u16,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        line_style: u8,
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
        dashes: Option<DashState>,
    ) {
        let min_x = x0.min(x1);
        let max_x = x0.max(x1);
        let cap_extra = match cap_style {
            2 | 3 => hw,
            _ => 0,
        };

        match dashes {
            None => {
                // Solid wide horizontal line
                self.fill_rect_rop_clipped(
                    (min_x - cap_extra) as i16,
                    (y - hw) as i16,
                    (max_x - min_x + 1 + cap_extra * 2) as u16,
                    line_width,
                    color,
                    gc_func,
                    plane_mask,
                    clip_rects,
                );
            }
            Some(mut ds) => {
                // Dashed wide horizontal line: walk pixels left-to-right,
                // collect contiguous on/off runs, draw each as a rectangle.
                let dir = if x0 <= x1 { 1i32 } else { -1i32 };
                let count = (max_x - min_x + 1) as usize;
                let start_x = x0;
                let mut seg_start = start_x;
                let mut seg_on = ds.is_on();
                let mut cx = start_x;

                for i in 0..count {
                    let cur_on = ds.is_on();
                    if cur_on != seg_on || i == 0 {
                        if i > 0 {
                            // Flush previous segment
                            self.flush_wide_h_segment(
                                seg_start,
                                cx - dir,
                                y,
                                hw,
                                line_width,
                                seg_on,
                                color,
                                background,
                                gc_func,
                                plane_mask,
                                line_style,
                                cap_style,
                                i == 0,
                                false,
                                clip_rects,
                            );
                        }
                        seg_start = cx;
                        seg_on = cur_on;
                    }
                    ds.advance();
                    if i + 1 < count {
                        cx += dir;
                    }
                }
                // Flush last segment
                self.flush_wide_h_segment(
                    seg_start, cx, y, hw, line_width, seg_on, color, background, gc_func,
                    plane_mask, line_style, cap_style, false, true, clip_rects,
                );
            }
        }
    }

    /// Flush a single horizontal dash segment as a filled rectangle.
    #[allow(clippy::too_many_arguments)]
    fn flush_wide_h_segment(
        &mut self,
        x_start: i32,
        x_end: i32,
        y: i32,
        hw: i32,
        line_width: u16,
        is_on: bool,
        color: u32,
        background: u32,
        gc_func: u8,
        plane_mask: u32,
        line_style: u8,
        _cap_style: u8,
        _is_first: bool,
        _is_last: bool,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        let min_x = x_start.min(x_end);
        let max_x = x_start.max(x_end);
        let w = (max_x - min_x + 1) as u16;

        if is_on {
            self.fill_rect_rop_clipped(
                min_x as i16,
                (y - hw) as i16,
                w,
                line_width,
                color,
                gc_func,
                plane_mask,
                clip_rects,
            );
        } else if line_style == 2 {
            // DoubleDash: draw background in gaps
            self.fill_rect_rop_clipped(
                min_x as i16,
                (y - hw) as i16,
                w,
                line_width,
                background,
                gc_func,
                plane_mask,
                clip_rects,
            );
        }
    }

    /// Draw a wide vertical line with dash support.
    #[allow(clippy::too_many_arguments)]
    fn draw_wide_line_vert(
        &mut self,
        x: i32,
        y0: i32,
        y1: i32,
        hw: i32,
        line_width: u16,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        line_style: u8,
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
        dashes: Option<DashState>,
    ) {
        let min_y = y0.min(y1);
        let max_y = y0.max(y1);
        let cap_extra = match cap_style {
            2 | 3 => hw,
            _ => 0,
        };

        match dashes {
            None => {
                // Solid wide vertical line
                self.fill_rect_rop_clipped(
                    (x - hw) as i16,
                    (min_y - cap_extra) as i16,
                    line_width,
                    (max_y - min_y + 1 + cap_extra * 2) as u16,
                    color,
                    gc_func,
                    plane_mask,
                    clip_rects,
                );
            }
            Some(mut ds) => {
                // Dashed wide vertical line
                let dir = if y0 <= y1 { 1i32 } else { -1i32 };
                let count = (max_y - min_y + 1) as usize;
                let mut cy = y0;
                let mut seg_start = cy;
                let mut seg_on = ds.is_on();

                for i in 0..count {
                    let cur_on = ds.is_on();
                    if cur_on != seg_on || i == 0 {
                        if i > 0 {
                            let seg_min = seg_start.min(cy - dir);
                            let seg_max = seg_start.max(cy - dir);
                            let h = (seg_max - seg_min + 1) as u16;
                            if seg_on {
                                self.fill_rect_rop_clipped(
                                    (x - hw) as i16,
                                    seg_min as i16,
                                    line_width,
                                    h,
                                    color,
                                    gc_func,
                                    plane_mask,
                                    clip_rects,
                                );
                            } else if line_style == 2 {
                                self.fill_rect_rop_clipped(
                                    (x - hw) as i16,
                                    seg_min as i16,
                                    line_width,
                                    h,
                                    background,
                                    gc_func,
                                    plane_mask,
                                    clip_rects,
                                );
                            }
                        }
                        seg_start = cy;
                        seg_on = cur_on;
                    }
                    ds.advance();
                    if i + 1 < count {
                        cy += dir;
                    }
                }
                // Flush last segment
                let seg_min = seg_start.min(cy);
                let seg_max = seg_start.max(cy);
                let h = (seg_max - seg_min + 1) as u16;
                if seg_on {
                    self.fill_rect_rop_clipped(
                        (x - hw) as i16,
                        seg_min as i16,
                        line_width,
                        h,
                        color,
                        gc_func,
                        plane_mask,
                        clip_rects,
                    );
                } else if line_style == 2 {
                    self.fill_rect_rop_clipped(
                        (x - hw) as i16,
                        seg_min as i16,
                        line_width,
                        h,
                        background,
                        gc_func,
                        plane_mask,
                        clip_rects,
                    );
                }
            }
        }
    }

    /// Draw a wide diagonal line with dash support.
    /// Uses perpendicular strips from the center line, with dash state
    /// controlling which strips are drawn.
    #[allow(clippy::too_many_arguments)]
    fn draw_wide_line_diagonal(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        hw: i32,
        color: u32,
        gc_func: u8,
        plane_mask: u32,
        cap_style: u8,
        line_style: u8,
        background: u32,
        clip_rects: &[(i16, i16, u16, u16)],
        dashes: Option<DashState>,
    ) {
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

        match dashes {
            None => {
                // Solid wide diagonal line: draw parallel offset lines
                for d in -hw..=hw {
                    let ox = (px * d as f64).round() as i32;
                    let oy = (py * d as f64).round() as i32;
                    self.bresenham_line_gc(
                        ex0 + ox,
                        ey0 + oy,
                        ex1 + ox,
                        ey1 + oy,
                        color,
                        gc_func,
                        plane_mask,
                        1,
                        None,
                        0,
                        background,
                        clip_rects,
                    );
                }
            }
            Some(mut ds) => {
                // Dashed wide diagonal line: walk along center line with Bresenham,
                // drawing perpendicular strips at each pixel based on dash state.
                let dx = (ex1 - ex0).abs();
                let dy = -(ey1 - ey0).abs();
                let sx: i32 = if ex0 < ex1 { 1 } else { -1 };
                let sy: i32 = if ey0 < ey1 { 1 } else { -1 };
                let mut err = dx + dy;
                let mut cx = ex0;
                let mut cy = ey0;

                loop {
                    let is_on = ds.is_on();
                    let draw_color = if is_on {
                        Some(color)
                    } else if line_style == 2 {
                        Some(background)
                    } else {
                        None
                    };

                    if let Some(c) = draw_color {
                        // Draw perpendicular strip at (cx, cy)
                        for d in -hw..=hw {
                            let px_i = cx + (px * d as f64).round() as i32;
                            let py_i = cy + (py * d as f64).round() as i32;
                            self.draw_point_gc(px_i, py_i, c, gc_func, plane_mask, clip_rects);
                        }
                    }

                    ds.advance();

                    if cx == ex1 && cy == ey1 {
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
