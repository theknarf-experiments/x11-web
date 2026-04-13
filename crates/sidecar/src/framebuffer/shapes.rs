use super::{point_in_arc, ArcChordData, DashState, Framebuffer};

impl Framebuffer {
    /// fill_rect_rop with clip rectangle support.
    pub fn fill_rect_rop_clipped(
        &mut self,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        color: u32,
        function: u8,
        plane_mask: u32,
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
                    ix0 as i16,
                    iy0 as i16,
                    (ix1 - ix0) as u16,
                    (iy1 - iy0) as u16,
                    color,
                    function,
                    plane_mask,
                );
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
            let stip_y =
                ((dy - ts_y as i32) % stipple_h as i32 + stipple_h as i32) as u32 % stipple_h;

            for px in row_start..row_end {
                let stip_x = ((px as i32 - ts_x as i32) % stipple_w as i32 + stipple_w as i32)
                    as u32
                    % stipple_w;
                let byte_idx = stip_y as usize * stipple_stride + (stip_x / 8) as usize;
                let bit = if byte_idx < stipple_data.len() {
                    (stipple_data[byte_idx] >> (stip_x % 8)) & 1
                } else {
                    0
                };

                if bit != 0 {
                    self.draw_point_with_func_masked(
                        px as i32, dy, foreground, function, plane_mask,
                    );
                } else if opaque {
                    self.draw_point_with_func_masked(
                        px as i32, dy, background, function, plane_mask,
                    );
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
                let tile_x =
                    ((px as i32 - ts_x as i32) % tile_w as i32 + tile_w as i32) as u32 % tile_w;
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
                        self.draw_point_with_func_masked(
                            px as i32, dy, color, function, plane_mask,
                        );
                    }
                }
            }
        }
        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
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
        px: i32,
        py: i32,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        _angle1: i16,
        angle2: i16,
        start_rad: f64,
        extent_rad: f64,
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
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        tile_data: &[u8],
        tile_w: u32,
        tile_h: u32,
        ts_x: i16,
        ts_y: i16,
        arc_mode: u8,
        gc_func: u8,
        plane_mask: u32,
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
                if !Self::pixel_in_filled_arc(
                    px,
                    py,
                    cx,
                    cy,
                    rx,
                    ry,
                    angle1,
                    angle2,
                    start_rad,
                    extent_rad,
                    arc_mode,
                    chord.as_ref(),
                ) {
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
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        fg: u32,
        bg: u32,
        stipple_data: &[u8],
        stipple_w: u32,
        stipple_h: u32,
        ts_x: i16,
        ts_y: i16,
        opaque: bool,
        arc_mode: u8,
        gc_func: u8,
        plane_mask: u32,
        clip_rects: &[(i16, i16, u16, u16)],
    ) {
        if width == 0 || height == 0 || stipple_w == 0 || stipple_h == 0 || stipple_data.is_empty()
        {
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
                if !Self::pixel_in_filled_arc(
                    px,
                    py,
                    cx,
                    cy,
                    rx,
                    ry,
                    angle1,
                    angle2,
                    start_rad,
                    extent_rad,
                    arc_mode,
                    chord.as_ref(),
                ) {
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
                x, y, width, height, angle1, angle2, true, foreground, arc_mode, function,
                plane_mask, clip_rects, line_width,
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
                                let t_proj = ((fx as f64 - lx as f64) * dx
                                    + (fy as f64 - ly as f64) * dy)
                                    / (len * len);
                                if (0.0..=1.0).contains(&t_proj) {
                                    let closest_x = lx as f64 + t_proj * dx;
                                    let closest_y = ly as f64 + t_proj * dy;
                                    let dist = ((fx as f64 - closest_x).powi(2)
                                        + (fy as f64 - closest_y).powi(2))
                                    .sqrt();
                                    if dist <= half_lw {
                                        if !clip_rects.is_empty()
                                            && !clip_rects.iter().any(|&(cx, cy, cw, ch)| {
                                                fx >= cx as i32
                                                    && fx < (cx as i32 + cw as i32)
                                                    && fy >= cy as i32
                                                    && fy < (cy as i32 + ch as i32)
                                            })
                                        {
                                            continue;
                                        }
                                        self.draw_point_with_func_masked(
                                            fx, fy, color, function, plane_mask,
                                        );
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
            let in_clip = clip_rects.is_empty()
                || clip_rects.iter().any(|&(rx, ry, rw, rh)| {
                    cx >= rx as i32
                        && cx < (rx as i32 + rw as i32)
                        && cy >= ry as i32
                        && cy < (ry as i32 + rh as i32)
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
                            let sx = sx_val.max(0).min(i16::MAX as i32) as i16;
                            let ex = (*cx).min(self.width as i32 - 1).min(i16::MAX as i32) as i16;
                            if ex >= sx {
                                self.fill_rect_rop_clipped(
                                    sx,
                                    y as i16,
                                    (ex - sx + 1) as u16,
                                    1,
                                    color,
                                    gc_func,
                                    plane_mask,
                                    clip_rects,
                                );
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
                        let start_x = pair[0].max(0).min(i16::MAX as i32) as i16;
                        let end_x = pair[1].min(self.width as i32 - 1).min(i16::MAX as i32) as i16;
                        if end_x >= start_x {
                            self.fill_rect_rop_clipped(
                                start_x,
                                y as i16,
                                (end_x - start_x + 1) as u16,
                                1,
                                color,
                                gc_func,
                                plane_mask,
                                clip_rects,
                            );
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
                    if !self.in_clip(px as i32, dy as i32, clip_rects) {
                        continue;
                    }
                    let tile_px = ((px as i32 - ts_x as i32).rem_euclid(tile_w as i32)) as usize;
                    let tile_py = ((dy as i32 - ts_y as i32).rem_euclid(tile_h as i32)) as usize;
                    let offset = (tile_py * tile_w as usize + tile_px) * 4;
                    if offset + 3 < tile_data.len() {
                        let b = tile_data[offset] as u32;
                        let g = tile_data[offset + 1] as u32;
                        let r = tile_data[offset + 2] as u32;
                        let a = tile_data[offset + 3] as u32;
                        let color = (a << 24) | (r << 16) | (g << 8) | b;
                        self.draw_point_with_func_masked(
                            px as i32, dy as i32, color, gc_func, plane_mask,
                        );
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
                    if !self.in_clip(px as i32, dy as i32, clip_rects) {
                        continue;
                    }
                    let stip_x = ((px as i32 - ts_x as i32).rem_euclid(stipple_w as i32)) as u32;
                    let stip_y = ((dy as i32 - ts_y as i32).rem_euclid(stipple_h as i32)) as u32;
                    let byte_idx = stip_y as usize * stipple_stride + (stip_x / 8) as usize;
                    let bit = if byte_idx < stipple_data.len() {
                        (stipple_data[byte_idx] >> (stip_x % 8)) & 1
                    } else {
                        0
                    };
                    if bit != 0 {
                        self.draw_point_with_func_masked(
                            px as i32, dy as i32, fg, gc_func, plane_mask,
                        );
                    } else if opaque {
                        self.draw_point_with_func_masked(
                            px as i32, dy as i32, bg, gc_func, plane_mask,
                        );
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
            x >= cx as i32
                && x < (cx as i32 + cw as i32)
                && y >= cy as i32
                && y < (cy as i32 + ch as i32)
        })
    }

    /// Compute polygon scanlines: returns sorted (y, [(start_x, end_x)]) pairs.
    fn compute_polygon_scanlines(
        &self,
        points: &[(i16, i16)],
        fill_rule: u8,
    ) -> Vec<(i16, Vec<(i16, i16)>)> {
        let min_y = points.iter().map(|p| p.1).min().unwrap().max(0);
        let max_y = points
            .iter()
            .map(|p| p.1)
            .max()
            .unwrap()
            .min(self.height as i16 - 1);
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
                            let sx = sx_val.max(0).min(i16::MAX as i32) as i16;
                            let ex = (*cx).min(self.width as i32 - 1).min(i16::MAX as i32) as i16;
                            if ex >= sx {
                                spans.push((sx, ex));
                            }
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
                        if ex >= sx {
                            spans.push((sx, ex));
                        }
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
