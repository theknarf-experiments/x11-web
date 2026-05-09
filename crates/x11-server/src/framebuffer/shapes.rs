use super::{
    build_clip_mask, build_dash, point_in_arc, read_pixel, skia_color, skia_eligible,
    stipple_to_tile, ArcChordData, DashState, Framebuffer,
};
use tiny_skia::{FillRule, Paint, PathBuilder, Transform};
use x11rb_protocol::protocol::xproto::{ArcMode, FillRule as XFillRule, LineStyle};

#[inline]
fn winding_fill_rule(fill_rule: u8) -> FillRule {
    if XFillRule::from(fill_rule) == XFillRule::WINDING {
        FillRule::Winding
    } else {
        FillRule::EvenOdd
    }
}

#[inline]
fn line_style_uses_dashes(line_style: u8) -> bool {
    let style = LineStyle::from(line_style);
    style == LineStyle::ON_OFF_DASH || style == LineStyle::DOUBLE_DASH
}

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
        // GXcopy: materialise the stipple as an RGBA tile and reuse
        // the tiled-fill fast path. Cleared bits become transparent
        // (Stippled) or `bg` (OpaqueStippled).
        if skia_eligible(function, plane_mask) {
            let tile = stipple_to_tile(
                stipple_data,
                stipple_w,
                stipple_h,
                foreground,
                background,
                opaque,
            );
            self.fill_rect_tiled(
                x, y, width, height, &tile, stipple_w, stipple_h, ts_x, ts_y, function, plane_mask,
            );
            return;
        }
        // Fallback (non-GXcopy): per-pixel raster-op blit.
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
        if tile_w == 0 || tile_h == 0 || tile_data.is_empty() || width == 0 || height == 0 {
            return;
        }
        let tile_stride = tile_w as usize * 4;
        // Fast path: GXcopy + full plane mask -> tiny-skia pattern paint.
        if skia_eligible(function, plane_mask) {
            if let Some(rect) =
                tiny_skia::Rect::from_xywh(x as f32, y as f32, width as f32, height as f32)
            {
                let mut pb = PathBuilder::new();
                pb.push_rect(rect);
                if let Some(path) = pb.finish() {
                    if self.fill_path_tiled(
                        &path,
                        tile_data,
                        tile_w,
                        tile_h,
                        ts_x,
                        ts_y,
                        FillRule::Winding,
                        &[],
                    ) {
                        self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
                        return;
                    }
                }
            }
        }
        // Fallback (non-GXcopy): per-pixel raster-op blit.
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
                    let color = read_pixel(tile_data, off);
                    self.draw_point_with_func_masked(px as i32, dy, color, function, plane_mask);
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
            // Fast path: solid GXcopy goes to tiny-skia for AA fill.
            if skia_eligible(gc_func, plane_mask) {
                if let Some(path) =
                    build_arc_path(cx, cy, rx, ry, start_rad, extent_rad, arc_mode, angle2)
                {
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
                    self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
                    return;
                }
            }

            // Fallback: per-pixel scan with raster op + plane_mask.
            let min_y = y.max(0) as i32;
            let max_y = ((y as i32 + height as i32).min(self.height as i32 - 1)).max(0);
            let min_x = x.max(0) as i32;
            let max_x = ((x as i32 + width as i32).min(self.width as i32 - 1)).max(0);
            let chord = ArcChordData::new_if_chord(arc_mode, angle2, start_rad, extent_rad);
            for py in min_y..=max_y {
                for px in min_x..=max_x {
                    if Self::pixel_in_filled_arc(
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
                        self.draw_point_gc(px, py, color, gc_func, plane_mask, clip_rects);
                    }
                }
            }
        } else {
            // Stroked arc outline.
            // Fast path: solid GXcopy goes to tiny-skia for AA stroke.
            if skia_eligible(gc_func, plane_mask) {
                if let Some(path) = build_arc_polyline(cx, cy, rx, ry, start_rad, extent_rad) {
                    self.stroke_path_skia(&path, color, line_width.max(1), 1, None, clip_rects);
                    self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
                    return;
                }
            }

            // Fallback (non-GXcopy): single-pixel Bresenham along
            // sampled arc points. Wide strokes under raster-op blends
            // are not supported.
            let steps = ((rx + ry) * 2.0).max(64.0) as usize;
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..=steps {
                let t = start_rad + extent_rad * (i as f64 / steps as f64);
                let px = (cx + rx * t.cos()) as i32;
                let py = (cy - ry * t.sin()) as i32;
                if let Some((lx, ly)) = prev {
                    self.bresenham_line_rop_clipped(
                        lx, ly, px, py, color, gc_func, plane_mask, clip_rects,
                    );
                }
                prev = Some((px, py));
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
        // Pre-computed chord data for ArcChord mode (only used for ArcMode::CHORD).
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
        if ArcMode::from(arc_mode) == ArcMode::CHORD {
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

        // Fast path: GXcopy + full plane mask -> tiny-skia path fill
        // with a tile pattern paint.
        if skia_eligible(gc_func, plane_mask) {
            if let Some(path) =
                build_arc_path(cx, cy, rx, ry, start_rad, extent_rad, arc_mode, angle2)
            {
                if self.fill_path_tiled(
                    &path,
                    tile_data,
                    tile_w,
                    tile_h,
                    ts_x,
                    ts_y,
                    FillRule::Winding,
                    clip_rects,
                ) {
                    self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
                    return;
                }
            }
        }

        // Fallback: per-pixel point-in-arc test with raster-op blit.
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
                    let color = read_pixel(tile_data, off);
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

        // GXcopy: bake the stipple to an RGBA tile and reuse the
        // tiled-arc fast path.
        if skia_eligible(gc_func, plane_mask) {
            let tile = stipple_to_tile(stipple_data, stipple_w, stipple_h, fg, bg, opaque);
            self.fill_arc_tiled(
                x, y, width, height, angle1, angle2, &tile, stipple_w, stipple_h, ts_x, ts_y,
                arc_mode, gc_func, plane_mask, clip_rects,
            );
            return;
        }

        // Fallback (non-GXcopy): per-pixel point-in-arc + stipple test.
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

        // Filled arcs delegate to the arc-mode-aware fill (already
        // tiny-skia-fast-pathed for GXcopy).
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

        // Fast path: solid GXcopy strokes (with optional dashes) go to
        // tiny-skia. DoubleDash is handled by stroking the background
        // colour with the inverse dash pattern first.
        let butt_cap = u32::from(x11rb_protocol::protocol::xproto::CapStyle::BUTT) as u8;
        if skia_eligible(function, plane_mask) {
            if let Some(path) = build_arc_polyline(cx, cy, rx, ry, start_rad, extent_rad) {
                let dash = build_dash(line_style, dash_list, dash_offset);
                if LineStyle::from(line_style) == LineStyle::DOUBLE_DASH {
                    if let Some(ref d) = dash {
                        self.stroke_path_skia(
                            &path,
                            background,
                            line_width,
                            butt_cap,
                            Some(d.inverted()),
                            clip_rects,
                        );
                    }
                }
                self.stroke_path_skia(
                    &path,
                    foreground,
                    line_width,
                    butt_cap,
                    dash,
                    clip_rects,
                );
                self.mark_dirty(x as i32, y as i32, width as u32, height as u32);
                return;
            }
        }

        // Fallback: per-pixel Bresenham along the arc with dash logic.
        let steps = ((rx + ry) * 2.0).max(64.0) as usize;
        let use_dashes = line_style_uses_dashes(line_style) && !dash_list.is_empty();
        let mut dash_state = use_dashes.then(|| DashState::new(dash_list, dash_offset));
        let mut prev: Option<(i32, i32)> = None;
        for i in 0..=steps {
            let t = start_rad + extent_rad * (i as f64 / steps as f64);
            let px = (cx + rx * t.cos()) as i32;
            let py = (cy - ry * t.sin()) as i32;
            if let Some((lx, ly)) = prev {
                let draw_fg = if let Some(ref mut ds) = dash_state {
                    let on = ds.is_on();
                    ds.advance();
                    on
                } else {
                    true
                };
                let color = if draw_fg {
                    foreground
                } else if LineStyle::from(line_style) == LineStyle::DOUBLE_DASH {
                    background
                } else {
                    prev = Some((px, py));
                    continue;
                };
                self.bresenham_line_rop_clipped(
                    lx, ly, px, py, color, function, plane_mask, clip_rects,
                );
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

        let bx0 = points.iter().map(|p| p.0).min().unwrap_or(0).max(0) as i32;
        let by0 = points.iter().map(|p| p.1).min().unwrap_or(0).max(0) as i32;
        let bx1 = (points.iter().map(|p| p.0).max().unwrap_or(0) as i32 + 1).min(self.width as i32);
        let by1 =
            (points.iter().map(|p| p.1).max().unwrap_or(0) as i32 + 1).min(self.height as i32);
        if bx0 >= bx1 || by0 >= by1 {
            return;
        }

        // tiny-skia handles the GXcopy fast path; for non-Porter-Duff
        // raster ops we fall back to a per-span scanline fill that
        // routes through `fill_rect_rop_clipped` so plane_mask + GC
        // function are honoured.
        if skia_eligible(gc_func, plane_mask) {
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].0 as f32, points[0].1 as f32);
            for &(px, py) in &points[1..] {
                pb.line_to(px as f32, py as f32);
            }
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(skia_color(color));
                paint.anti_alias = false;
                let rule = winding_fill_rule(fill_rule);
                let clip_mask = build_clip_mask(self.width, self.height, clip_rects);
                let _ = self.with_pixmap_mut(|pm| {
                    pm.fill_path(
                        &path,
                        &paint,
                        rule,
                        Transform::identity(),
                        clip_mask.as_ref(),
                    );
                });
                self.mark_dirty(bx0, by0, (bx1 - bx0) as u32, (by1 - by0) as u32);
                return;
            }
        }

        // Fallback (non-GXcopy raster op): scanline fill via existing
        // helper to ensure GC function + plane_mask are applied.
        let scanlines = self.compute_polygon_scanlines(points, fill_rule);
        for (y, spans) in scanlines {
            for (sx, ex) in spans {
                if ex >= sx {
                    self.fill_rect_rop_clipped(
                        sx,
                        y,
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
        // Fast path: GXcopy + full plane mask -> tiny-skia path fill with
        // a tile pattern paint.
        if skia_eligible(gc_func, plane_mask) {
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].0 as f32, points[0].1 as f32);
            for &(px, py) in &points[1..] {
                pb.line_to(px as f32, py as f32);
            }
            pb.close();
            let rule = winding_fill_rule(fill_rule);
            if let Some(path) = pb.finish() {
                if self.fill_path_tiled(
                    &path, tile_data, tile_w, tile_h, ts_x, ts_y, rule, clip_rects,
                ) {
                    let bx0 = points.iter().map(|p| p.0).min().unwrap_or(0).max(0) as i32;
                    let by0 = points.iter().map(|p| p.1).min().unwrap_or(0).max(0) as i32;
                    let bx1 = (points.iter().map(|p| p.0).max().unwrap_or(0) as i32 + 1)
                        .min(self.width as i32);
                    let by1 = (points.iter().map(|p| p.1).max().unwrap_or(0) as i32 + 1)
                        .min(self.height as i32);
                    if bx0 < bx1 && by0 < by1 {
                        self.mark_dirty(bx0, by0, (bx1 - bx0) as u32, (by1 - by0) as u32);
                    }
                    return;
                }
            }
        }
        // Fallback: per-pixel raster-op blit.
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
                        let color = read_pixel(tile_data, offset);
                        self.draw_point_with_func_masked(
                            px as i32, dy as i32, color, gc_func, plane_mask,
                        );
                    }
                }
            }
        }
    }

    /// Fill polygon with a stipple pattern.
    #[allow(clippy::too_many_arguments)]
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
        // GXcopy: bake the stipple to an RGBA tile and reuse the
        // tiled-polygon fast path.
        if skia_eligible(gc_func, plane_mask) {
            let tile = stipple_to_tile(stipple_data, stipple_w, stipple_h, fg, bg, opaque);
            self.fill_polygon_tiled(
                points, &tile, stipple_w, stipple_h, ts_x, ts_y, fill_rule, gc_func, plane_mask,
                clip_rects,
            );
            return;
        }
        // Fallback (non-GXcopy): per-pixel raster-op blit.
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
        let Some(min_y) = points.iter().map(|p| p.1).min() else {
            return Vec::new();
        };
        let min_y = min_y.max(0);
        let max_y = points
            .iter()
            .map(|p| p.1)
            .max()
            .unwrap_or(0)
            .min(self.height as i16 - 1);
        let n = points.len();
        let mut result = Vec::new();

        for y in min_y..=max_y {
            let y32 = y as i32;
            let mut spans = Vec::new();

            if XFillRule::from(fill_rule) == XFillRule::WINDING {
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
}

/// Build an *open* polyline path along the arc — no closing edge. For
/// stroke rendering of the outline.
fn build_arc_polyline(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    start_rad: f64,
    extent_rad: f64,
) -> Option<tiny_skia::Path> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let segments = ((rx + ry) * 2.0).clamp(32.0, 256.0) as usize;
    let mut pb = PathBuilder::new();
    let pt = |t: f64| ((cx + rx * t.cos()) as f32, (cy - ry * t.sin()) as f32);
    let (sx, sy) = pt(start_rad);
    pb.move_to(sx, sy);
    for i in 1..=segments {
        let t = start_rad + extent_rad * (i as f64 / segments as f64);
        let (px, py) = pt(t);
        pb.line_to(px, py);
    }
    pb.finish()
}

/// Build a tiny-skia path approximating an X11 elliptical arc as a
/// polyline. Returns `None` for degenerate inputs.
///
/// `arc_mode`: 0 = ArcChord (close start→end with a chord),
///             1 = ArcPieSlice (close through the ellipse centre).
fn build_arc_path(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    start_rad: f64,
    extent_rad: f64,
    arc_mode: u8,
    angle2: i16,
) -> Option<tiny_skia::Path> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let full_circle = angle2.abs() >= 360 * 64;
    let segments = ((rx + ry) * 2.0).clamp(32.0, 256.0) as usize;
    let mut pb = PathBuilder::new();
    let pt = |t: f64| ((cx + rx * t.cos()) as f32, (cy - ry * t.sin()) as f32);
    let (sx, sy) = pt(start_rad);
    if full_circle || arc_mode != 0 {
        // PieSlice (or full ellipse): start at centre, line out to arc start.
        if !full_circle {
            pb.move_to(cx as f32, cy as f32);
            pb.line_to(sx, sy);
        } else {
            pb.move_to(sx, sy);
        }
    } else {
        // Chord: arc start is the path start; the closing edge will be
        // the implicit line from the arc end back to (sx, sy).
        pb.move_to(sx, sy);
    }
    for i in 1..=segments {
        let t = start_rad + extent_rad * (i as f64 / segments as f64);
        let (px, py) = pt(t);
        pb.line_to(px, py);
    }
    pb.close();
    pb.finish()
}
