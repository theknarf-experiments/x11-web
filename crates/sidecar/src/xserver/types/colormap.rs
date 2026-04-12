//! Colormap state for TrueColor, PseudoColor, DirectColor, GrayScale,
//! StaticGray, and StaticColor visuals.

/// Colormap state for both TrueColor (read-only) and PseudoColor (writable) visuals.
#[derive(Clone)]
pub(crate) struct ColormapState {
    /// Visual ID this colormap is associated with.
    #[allow(dead_code)]
    pub(crate) visual: u32,
    /// Visual class: 0=StaticGray, 1=GrayScale, 2=StaticColor, 3=PseudoColor, 4=TrueColor, 5=DirectColor
    pub(crate) visual_class: u8,
    /// Color table entries for PseudoColor/GrayScale (index → RGB). Up to 256 entries for depth 8.
    pub(crate) entries: Vec<(u16, u16, u16)>,
    /// Flags for whether each cell has been allocated (PseudoColor).
    pub(crate) allocated: Vec<bool>,
    /// Next free cell index for allocation.
    pub(crate) next_free: usize,
}

impl ColormapState {
    /// Create a new TrueColor colormap (read-only, no entries needed).
    pub(crate) fn new_truecolor(visual: u32) -> Self {
        Self {
            visual,
            visual_class: 4, // TrueColor
            entries: Vec::new(),
            allocated: Vec::new(),
            next_free: 0,
        }
    }

    /// Create a new PseudoColor colormap (writable, 256 entries for depth 8).
    pub(crate) fn new_pseudocolor(visual: u32, n_entries: usize) -> Self {
        // Initialize with a standard VGA-like default palette
        let mut entries = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            // Default: grayscale ramp
            let v = ((i * 65535) / n_entries.max(1)) as u16;
            entries.push((v, v, v));
        }
        Self {
            visual,
            visual_class: 3, // PseudoColor
            entries,
            allocated: vec![false; n_entries],
            next_free: 0,
        }
    }

    /// Create a new DirectColor colormap (writable RGB planes, separate R/G/B ramps).
    pub(crate) fn new_directcolor(visual: u32, n_entries: usize) -> Self {
        let mut entries = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let v = ((i * 65535) / n_entries.max(1)) as u16;
            entries.push((v, v, v));
        }
        Self {
            visual,
            visual_class: 5, // DirectColor
            entries,
            allocated: vec![false; n_entries],
            next_free: 0,
        }
    }

    /// Create a new GrayScale colormap (writable, grayscale ramp).
    pub(crate) fn new_grayscale(visual: u32, n_entries: usize) -> Self {
        let mut entries = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let v = ((i * 65535) / n_entries.max(1)) as u16;
            entries.push((v, v, v));
        }
        Self {
            visual,
            visual_class: 1, // GrayScale
            entries,
            allocated: vec![false; n_entries],
            next_free: 0,
        }
    }

    /// Create a new StaticGray colormap (read-only grayscale ramp).
    pub(crate) fn new_staticgray(visual: u32, n_entries: usize) -> Self {
        let mut entries = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let v = ((i * 65535) / n_entries.max(1)) as u16;
            entries.push((v, v, v));
        }
        Self {
            visual,
            visual_class: 0, // StaticGray
            entries,
            allocated: vec![true; n_entries], // read-only: all pre-allocated
            next_free: n_entries,
        }
    }

    /// Create a new StaticColor colormap (read-only indexed color, 3-3-2 RGB).
    pub(crate) fn new_staticcolor(visual: u32, n_entries: usize) -> Self {
        let mut entries = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            // 3-3-2 decomposition: RRRGGGBB
            let r = ((i >> 5) & 0x7) as u16;
            let g = ((i >> 2) & 0x7) as u16;
            let b = (i & 0x3) as u16;
            entries.push(
                ((r * 65535) / 7, (g * 65535) / 7, (b * 65535) / 3),
            );
        }
        Self {
            visual,
            visual_class: 2, // StaticColor
            entries,
            allocated: vec![true; n_entries], // read-only: all pre-allocated
            next_free: n_entries,
        }
    }

    /// Is this a writable colormap (PseudoColor, GrayScale, or DirectColor)?
    pub(crate) fn is_writable(&self) -> bool {
        matches!(self.visual_class, 1 | 3 | 5) // GrayScale, PseudoColor, DirectColor
    }

    /// Look up the RGB value for a pixel index.
    pub(crate) fn lookup(&self, pixel: u32) -> (u16, u16, u16) {
        match self.visual_class {
            5 => {
                // DirectColor: decompose pixel into per-channel indices and look up each
                let ri = ((pixel >> 16) & 0xFF) as usize;
                let gi = ((pixel >> 8) & 0xFF) as usize;
                let bi = (pixel & 0xFF) as usize;
                let n = self.entries.len();
                let r = if ri < n { self.entries[ri].0 } else { ((ri as u16) << 8) | ri as u16 };
                let g = if gi < n { self.entries[gi].1 } else { ((gi as u16) << 8) | gi as u16 };
                let b = if bi < n { self.entries[bi].2 } else { ((bi as u16) << 8) | bi as u16 };
                (r, g, b)
            }
            0 | 1 | 2 | 3 => {
                // StaticGray, GrayScale, StaticColor, PseudoColor: index into table
                if (pixel as usize) < self.entries.len() {
                    self.entries[pixel as usize]
                } else {
                    (0, 0, 0)
                }
            }
            _ => {
                // TrueColor: decompose pixel
                let r = ((pixel >> 16) & 0xFF) as u16;
                let g = ((pixel >> 8) & 0xFF) as u16;
                let b = (pixel & 0xFF) as u16;
                (r << 8 | r, g << 8 | g, b << 8 | b)
            }
        }
    }

    /// Allocate a color cell and return the pixel index.
    pub(crate) fn alloc_color(&mut self, r: u16, g: u16, b: u16) -> Option<u32> {
        match self.visual_class {
            4 => {
                // TrueColor: compute pixel directly
                let pixel = (((r >> 8) as u32) << 16) | (((g >> 8) as u32) << 8) | ((b >> 8) as u32);
                Some(pixel)
            }
            5 => {
                // DirectColor: compute pixel from per-channel lookup.
                let pixel = (((r >> 8) as u32) << 16) | (((g >> 8) as u32) << 8) | ((b >> 8) as u32);
                Some(pixel)
            }
            0 | 2 => {
                // StaticGray / StaticColor: read-only, find closest match
                let mut best_idx = 0u32;
                let mut best_dist = u64::MAX;
                for (i, &(er, eg, eb)) in self.entries.iter().enumerate() {
                    let dr = (r as i64 - er as i64).unsigned_abs();
                    let dg = (g as i64 - eg as i64).unsigned_abs();
                    let db = (b as i64 - eb as i64).unsigned_abs();
                    let dist = dr * dr + dg * dg + db * db;
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = i as u32;
                    }
                }
                Some(best_idx)
            }
            _ => {
                // PseudoColor (3), GrayScale (1): writable indexed
                // First check if this exact color already exists in an allocated cell
                for (i, &(er, eg, eb)) in self.entries.iter().enumerate() {
                    if er == r && eg == g && eb == b && self.allocated[i] {
                        return Some(i as u32);
                    }
                }

                // Find next free cell
                for i in 0..self.entries.len() {
                    let idx = (self.next_free + i) % self.entries.len();
                    if !self.allocated[idx] {
                        self.entries[idx] = (r, g, b);
                        self.allocated[idx] = true;
                        self.next_free = (idx + 1) % self.entries.len();
                        return Some(idx as u32);
                    }
                }
                None // All cells allocated
            }
        }
    }

    /// Allocate writable color cells (for AllocColorCells).
    pub(crate) fn alloc_cells(&mut self, n_colors: u16) -> Option<Vec<u32>> {
        if !self.is_writable() {
            return None;
        }
        let n = n_colors as usize;
        let mut pixels = Vec::with_capacity(n);
        for i in 0..self.entries.len() {
            if pixels.len() >= n { break; }
            if !self.allocated[i] {
                self.allocated[i] = true;
                pixels.push(i as u32);
            }
        }
        if pixels.len() < n {
            // Not enough free cells — free the ones we just allocated
            for &p in &pixels {
                self.allocated[p as usize] = false;
            }
            return None;
        }
        Some(pixels)
    }

    /// Free color cells.
    pub(crate) fn free_cells(&mut self, pixels: &[u32]) {
        for &p in pixels {
            if (p as usize) < self.allocated.len() {
                self.allocated[p as usize] = false;
            }
        }
    }

    /// Store colors into the colormap.
    pub(crate) fn store_colors(&mut self, items: &[(u32, u16, u16, u16, u8)]) {
        for &(pixel, r, g, b, flags) in items {
            if (pixel as usize) < self.entries.len() {
                let entry = &mut self.entries[pixel as usize];
                if flags & 0x01 != 0 { entry.0 = r; } // DoRed
                if flags & 0x02 != 0 { entry.1 = g; } // DoGreen
                if flags & 0x04 != 0 { entry.2 = b; } // DoBlue
                if flags == 0 { *entry = (r, g, b); }  // All channels
            }
        }
    }
}
