//! XFIXES region types and operations.

/// XFIXES region: a list of rectangles forming a region.
#[derive(Clone, Debug)]
pub(crate) struct XFixesRegion {
    pub(crate) rects: Vec<RegionRect>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RegionRect {
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl XFixesRegion {
    pub(crate) fn new() -> Self {
        Self { rects: Vec::new() }
    }

    pub(crate) fn from_rects(rects: Vec<RegionRect>) -> Self {
        Self { rects }
    }

    /// Compute the bounding extents of the region.
    pub(crate) fn extents(&self) -> RegionRect {
        if self.rects.is_empty() {
            return RegionRect { x: 0, y: 0, width: 0, height: 0 };
        }
        let mut x1 = i16::MAX;
        let mut y1 = i16::MAX;
        let mut x2 = i16::MIN;
        let mut y2 = i16::MIN;
        for r in &self.rects {
            x1 = x1.min(r.x);
            y1 = y1.min(r.y);
            x2 = x2.max(r.x.saturating_add(r.width as i16));
            y2 = y2.max(r.y.saturating_add(r.height as i16));
        }
        RegionRect {
            x: x1,
            y: y1,
            width: (x2 - x1) as u16,
            height: (y2 - y1) as u16,
        }
    }

    /// Union of two regions (simple: concatenate rect lists).
    pub(crate) fn union(&self, other: &XFixesRegion) -> XFixesRegion {
        let mut rects = self.rects.clone();
        rects.extend_from_slice(&other.rects);
        XFixesRegion { rects }
    }

    /// Intersection of two regions (O(n*m) pairwise intersection).
    pub(crate) fn intersect(&self, other: &XFixesRegion) -> XFixesRegion {
        let mut rects = Vec::new();
        for a in &self.rects {
            for b in &other.rects {
                let x1 = a.x.max(b.x);
                let y1 = a.y.max(b.y);
                let x2 = (a.x + a.width as i16).min(b.x + b.width as i16);
                let y2 = (a.y + a.height as i16).min(b.y + b.height as i16);
                if x2 > x1 && y2 > y1 {
                    rects.push(RegionRect {
                        x: x1,
                        y: y1,
                        width: (x2 - x1) as u16,
                        height: (y2 - y1) as u16,
                    });
                }
            }
        }
        XFixesRegion { rects }
    }

    /// Subtract other from self.
    pub(crate) fn subtract(&self, other: &XFixesRegion) -> XFixesRegion {
        let mut result = self.rects.clone();
        for sub in &other.rects {
            let mut new_result = Vec::new();
            for r in &result {
                // Subtract sub from r, producing 0-4 rectangles
                subtract_rect(r, sub, &mut new_result);
            }
            result = new_result;
        }
        XFixesRegion { rects: result }
    }

    /// Translate region by (dx, dy).
    pub(crate) fn translate(&mut self, dx: i16, dy: i16) {
        for r in &mut self.rects {
            r.x = r.x.saturating_add(dx);
            r.y = r.y.saturating_add(dy);
        }
    }

    /// Invert: within a bounding rect, return the complement.
    pub(crate) fn invert(&self, bounds: &RegionRect) -> XFixesRegion {
        let bounding = XFixesRegion::from_rects(vec![*bounds]);
        bounding.subtract(self)
    }

    /// Expand the region by the given amounts on each side.
    #[allow(dead_code)]
    pub(crate) fn expand(&self, left: i16, right: i16, top: i16, bottom: i16) -> XFixesRegion {
        let mut rects = Vec::with_capacity(self.rects.len());
        for r in &self.rects {
            rects.push(RegionRect {
                x: r.x.saturating_sub(left),
                y: r.y.saturating_sub(top),
                width: r.width.saturating_add((left + right) as u16),
                height: r.height.saturating_add((top + bottom) as u16),
            });
        }
        XFixesRegion { rects }
    }
}

/// Subtract rectangle `sub` from rectangle `r`, appending result fragments.
fn subtract_rect(r: &RegionRect, sub: &RegionRect, out: &mut Vec<RegionRect>) {
    let rx2 = r.x + r.width as i16;
    let ry2 = r.y + r.height as i16;
    let sx2 = sub.x + sub.width as i16;
    let sy2 = sub.y + sub.height as i16;

    // No overlap - keep original
    if sub.x >= rx2 || sx2 <= r.x || sub.y >= ry2 || sy2 <= r.y {
        out.push(*r);
        return;
    }

    // Top strip
    if sub.y > r.y {
        out.push(RegionRect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: (sub.y - r.y) as u16,
        });
    }

    // Middle left
    let mid_y = r.y.max(sub.y);
    let mid_y2 = ry2.min(sy2);
    if mid_y < mid_y2 {
        if sub.x > r.x {
            out.push(RegionRect {
                x: r.x,
                y: mid_y,
                width: (sub.x - r.x) as u16,
                height: (mid_y2 - mid_y) as u16,
            });
        }
        // Middle right
        if sx2 < rx2 {
            out.push(RegionRect {
                x: sx2,
                y: mid_y,
                width: (rx2 - sx2) as u16,
                height: (mid_y2 - mid_y) as u16,
            });
        }
    }

    // Bottom strip
    if sy2 < ry2 {
        out.push(RegionRect {
            x: r.x,
            y: sy2,
            width: r.width,
            height: (ry2 - sy2) as u16,
        });
    }
}
