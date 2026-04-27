//! Color/colormap and cursor handlers (opcodes 78-96).

use super::*;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xproto::{
    AllocColorCellsRequest, AllocColorPlanesRequest, AllocColorRequest, AllocNamedColorRequest,
    CopyColormapAndFreeRequest, CreateColormapRequest, CreateCursorRequest,
    CreateGlyphCursorRequest, FreeColormapRequest, FreeColorsRequest, FreeCursorRequest,
    InstallColormapRequest, ListInstalledColormapsRequest, LookupColorRequest, QueryColorsRequest,
    RecolorCursorRequest, StoreColorsRequest, StoreNamedColorRequest, UninstallColormapRequest,
};

// ---------------------------------------------------------------------------
// Opcode 78: CreateColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_colormap(state: &mut ClientState, req: &CreateColormapRequest) -> Vec<u8> {
    let _alloc = u8::from(req.alloc);
    let mid = req.mid;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(mid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, mid, 78, 0);
    }

    // Enforce per-client colormap resource limit
    if !state.can_create_colormap() {
        return build_error(ALLOC_ERROR, state.sequence, mid, 78, 0);
    }

    let _window = req.window;
    let visual = req.visual;

    // Map visual ID to visual class and create appropriate colormap.
    // Visual IDs defined in our server setup:
    //   0x20 = TrueColor 24-bit (root)
    //   0x22 = DirectColor 24-bit
    //   0x23 = PseudoColor 8-bit (256 entries)
    //   0x24 = TrueColor 16-bit
    //   0x25 = StaticGray 4-bit (16 entries)
    //   0x26 = GrayScale 8-bit (256 entries, writable)
    //   0x27 = StaticColor 8-bit (256 entries, read-only)
    //   0x40 = TrueColor 32-bit (ARGB)
    let cmap = match visual {
        0x21 | 0x24 | 0x40 => ColormapState::new_truecolor(visual),
        0x22 => ColormapState::new_directcolor(visual, 256),
        0x23 => ColormapState::new_pseudocolor(visual, 256),
        0x25 => ColormapState::new_staticgray(visual, 16),
        0x26 => ColormapState::new_grayscale(visual, 256),
        0x27 => ColormapState::new_staticcolor(visual, 256),
        _ => {
            return build_error(MATCH_ERROR, state.sequence, visual, 78, 0);
        }
    };

    // If alloc=All and colormap is writable, pre-allocate all cells
    let mut cmap = cmap;
    if _alloc == 1 && cmap.is_writable() {
        for i in 0..cmap.allocated.len() {
            cmap.allocated[i] = true;
        }
    }

    state.colormaps.insert(mid, cmap);
    debug!("CreateColormap: id={mid:#x} visual={visual:#x} alloc={_alloc}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 79: FreeColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_free_colormap(state: &mut ClientState, req: &FreeColormapRequest) -> Vec<u8> {
    let mid = req.cmap;
    // Validate colormap exists (not the default, which cannot be freed)
    if mid != ROOT_COLORMAP && !state.colormaps.contains_key(&mid) {
        return build_error(COLORMAP_ERROR, state.sequence, mid, 79, 0);
    }
    state.colormaps.remove(&mid);
    state.installed_colormaps.remove(&mid);
    state.recycle_xid(mid);
    debug!("FreeColormap: id={mid:#x}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 80: CopyColormapAndFree
// ---------------------------------------------------------------------------

pub(crate) fn handle_copy_colormap_and_free(
    state: &mut ClientState,
    req: &CopyColormapAndFreeRequest,
) -> Vec<u8> {
    let _seq = state.sequence;
    let mid = req.mid;
    let src = req.src_cmap;
    // Validate source colormap exists
    if src != ROOT_COLORMAP && !state.colormaps.contains_key(&src) {
        return build_error(COLORMAP_ERROR, _seq, src, 80, 0);
    }
    let new_cmap = if let Some(src_cmap) = state.colormaps.get(&src) {
        src_cmap.clone()
    } else {
        ColormapState::new_truecolor(0x21)
    };
    state.colormaps.insert(mid, new_cmap);
    // Per X11 spec: free all allocated cells in the SOURCE colormap
    // (cells allocated by this client are released from the source)
    if let Some(src_cmap) = state.colormaps.get_mut(&src) {
        for a in src_cmap.allocated.iter_mut() {
            *a = false;
        }
    }
    debug!("CopyColormapAndFree: new_id={mid:#x}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 81: InstallColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_install_colormap(state: &mut ClientState, req: &InstallColormapRequest) -> Vec<u8> {
    let mid = req.cmap;
    // Validate colormap exists
    if mid != ROOT_COLORMAP && !state.colormaps.contains_key(&mid) {
        return build_error(COLORMAP_ERROR, state.sequence, mid, 81, 0);
    }
    debug!("InstallColormap: id={mid:#x}");

    // Track this colormap as installed
    state.installed_colormaps.insert(mid);

    // Per X11 spec, generate ColormapNotify for ALL windows that have
    // ColormapChangeMask (EventMask::COLOR_MAP_CHANGE) selected, notifying that this
    // colormap is now installed.
    let notify_windows: Vec<u32> = state
        .windows
        .iter()
        .filter(|(_, w)| w.event_mask & EventMask::COLOR_MAP_CHANGE != EventMask::NO_EVENT)
        .map(|(&id, _)| id)
        .collect();

    for wid in notify_windows {
        let mut event = [0u8; 32];
        event[0] = COLOURMAP_NOTIFY_EVENT;
        state.write_u32(&mut event, 4, wid);
        state.write_u32(&mut event, 8, mid);
        event[12] = 1; // new = true
        event[13] = 1; // state = Installed
        state.pending_events.push(event.to_vec());
        // Also broadcast to other connections selecting on this window
        state.broadcast_event(wid, u32::from(EventMask::COLOR_MAP_CHANGE), &event);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 82: UninstallColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_uninstall_colormap(state: &mut ClientState, req: &UninstallColormapRequest) -> Vec<u8> {
    let mid = req.cmap;
    // Validate colormap exists
    if mid != ROOT_COLORMAP && !state.colormaps.contains_key(&mid) {
        return build_error(COLORMAP_ERROR, state.sequence, mid, 82, 0);
    }
    debug!("UninstallColormap: id={mid:#x}");

    // Remove from installed set (but default colormap always stays installed)
    if mid != ROOT_COLORMAP {
        state.installed_colormaps.remove(&mid);
    }

    // Generate ColormapNotify with state=Uninstalled for all selecting windows
    let notify_windows: Vec<u32> = state
        .windows
        .iter()
        .filter(|(_, w)| w.event_mask & EventMask::COLOR_MAP_CHANGE != EventMask::NO_EVENT)
        .map(|(&id, _)| id)
        .collect();

    for wid in notify_windows {
        let mut event = [0u8; 32];
        event[0] = COLOURMAP_NOTIFY_EVENT;
        state.write_u32(&mut event, 4, wid);
        state.write_u32(&mut event, 8, mid);
        event[12] = 1; // new = true
        event[13] = 0; // state = Uninstalled
        state.pending_events.push(event.to_vec());
        state.broadcast_event(wid, u32::from(EventMask::COLOR_MAP_CHANGE), &event);
    }

    // Per spec, when a colormap is uninstalled, the default colormap
    // should be installed automatically.
    let default_cmap = ROOT_COLORMAP;
    if mid != default_cmap {
        let notify_windows2: Vec<u32> = state
            .windows
            .iter()
            .filter(|(_, w)| w.event_mask & EventMask::COLOR_MAP_CHANGE != EventMask::NO_EVENT)
            .map(|(&id, _)| id)
            .collect();
        for wid in notify_windows2 {
            let mut event = [0u8; 32];
            event[0] = COLOURMAP_NOTIFY_EVENT;
            state.write_u32(&mut event, 4, wid);
            state.write_u32(&mut event, 8, default_cmap);
            event[12] = 1; // new = true
            event[13] = 1; // state = Installed
            state.pending_events.push(event.to_vec());
            state.broadcast_event(wid, u32::from(EventMask::COLOR_MAP_CHANGE), &event);
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 83: ListInstalledColormaps
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_installed_colormaps(state: &ClientState, _req: &ListInstalledColormapsRequest) -> Vec<u8> {
    let seq = state.sequence;
    // _req.window is available but currently unused — we return all installed colormaps.
    // Return only colormaps that have been explicitly installed
    // (the default colormap ROOT_COLORMAP is always installed).
    let cmaps: Vec<u32> = state.installed_colormaps.iter().copied().collect();

    let n_cmaps = cmaps.len();
    let extra_bytes = n_cmaps * 4;
    let padded = (extra_bytes + 3) & !3;

    let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
        .set_u16(8, n_cmaps as u16);

    for (i, &cid) in cmaps.iter().enumerate() {
        let off = 32 + i * 4;
        reply = reply.set_u32(off, cid);
    }

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 84: AllocColor
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_color(state: &mut ClientState, req: &AllocColorRequest) -> Vec<u8> {
    let seq = state.sequence;
    let cmap_id = req.cmap;
    let red = req.red;
    let green = req.green;
    let blue = req.blue;

    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 84, 0);
    }

    // Try to allocate in the colormap (works for both TrueColor and PseudoColor)
    let pixel = if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
        cmap.alloc_color(red, green, blue)
    } else {
        // Default TrueColor allocation (ROOT_COLORMAP)
        let r8 = (red >> 8) as u32;
        let g8 = (green >> 8) as u32;
        let b8 = (blue >> 8) as u32;
        Some((r8 << 16) | (g8 << 8) | b8)
    };

    let pixel = match pixel {
        Some(p) => p,
        None => return build_error(ALLOC_ERROR, seq, 0, 84, 0),
    };

    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, red)
        .set_u16(10, green)
        .set_u16(12, blue)
        .set_u32(16, pixel)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 85: AllocNamedColor
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_named_color(state: &mut ClientState, req: &AllocNamedColorRequest) -> Vec<u8> {
    let seq = state.sequence;
    let cmap_id = req.cmap;
    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 85, 0);
    }

    let name = std::str::from_utf8(&req.name).unwrap_or("");

    let (r16, g16, b16) = match crate::colors::lookup_color(name) {
        Some(c) => c,
        None => {
            warn!("AllocNamedColor: unknown color {name:?}");
            return build_error(NAME_ERROR, seq, 0, 85, 0);
        }
    };
    let r8 = (r16 >> 8) as u32;
    let g8 = (g16 >> 8) as u32;
    let b8 = (b16 >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    info!("AllocNamedColor: name={name:?} -> pixel={pixel:#x}");

    ReplyBuf::fixed(seq, state.msb_first)
        .set_u32(8, pixel)
        .set_u16(12, r16) // exact red
        .set_u16(14, g16) // exact green
        .set_u16(16, b16) // exact blue
        .set_u16(18, r16) // visual red
        .set_u16(20, g16) // visual green
        .set_u16(22, b16) // visual blue
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 86: AllocColorCells
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_color_cells(state: &mut ClientState, req: &AllocColorCellsRequest) -> Vec<u8> {
    let seq = state.sequence;
    let contiguous = req.contiguous;
    let cmap_id = req.cmap;
    let n_colors = req.colors;
    let n_planes = req.planes;

    // Per X11 spec: n_colors must be non-zero
    if n_colors == 0 {
        return build_error(VALUE_ERROR, seq, 0, 86, 0);
    }

    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 86, 0);
    }

    // Only PseudoColor/GrayScale colormaps support writable cells
    let is_writable = state
        .colormaps
        .get(&cmap_id)
        .is_some_and(|c| c.is_writable());
    if !is_writable {
        return build_error(ALLOC_ERROR, seq, 0, 86, 0);
    }

    // Validate planes count is reasonable (max 24 bits depth)
    if n_planes > 24 {
        return build_error(VALUE_ERROR, seq, n_planes as u32, 86, 0);
    }

    let total_colors = if n_planes > 0 {
        n_colors as usize * (1usize << n_planes as u32)
    } else {
        n_colors as usize
    };

    let pixels = if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
        if contiguous {
            cmap.alloc_cells_contiguous(total_colors as u16)
        } else {
            cmap.alloc_cells(total_colors as u16)
        }
    } else {
        None
    };

    match pixels {
        Some(pix) => {
            // Build reply: pixels array + masks array
            let n_pix = n_colors as usize;
            let n_mask = if n_planes > 0 { n_planes as usize } else { 0 };
            let data_len = (n_pix + n_mask) * 4;
            let padded = (data_len + 3) & !3;
            let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
                .set_u16(8, n_pix as u16)
                .set_u16(10, n_mask as u16);
            for (i, &p) in pix.iter().take(n_pix).enumerate() {
                reply = reply.set_u32(32 + i * 4, p);
            }
            // Plane masks: for each plane bit, set that bit position
            for i in 0..n_mask {
                let mask = 1u32 << i;
                reply = reply.set_u32(32 + (n_pix + i) * 4, mask);
            }
            reply.build()
        }
        None => build_error(ALLOC_ERROR, seq, 0, 86, 0),
    }
}

// ---------------------------------------------------------------------------
// Opcode 87: AllocColorPlanes
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_color_planes(state: &mut ClientState, req: &AllocColorPlanesRequest) -> Vec<u8> {
    let seq = state.sequence;
    let contiguous = req.contiguous;
    let cmap_id = req.cmap;
    let n_colors = req.colors;
    let n_reds = req.reds;
    let n_greens = req.greens;
    let n_blues = req.blues;

    // Per X11 spec: n_colors must be non-zero
    if n_colors == 0 {
        return build_error(VALUE_ERROR, seq, 0, 87, 0);
    }

    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 87, 0);
    }

    let is_writable = state
        .colormaps
        .get(&cmap_id)
        .is_some_and(|c| c.is_writable());
    if !is_writable {
        return build_error(ALLOC_ERROR, seq, 0, 87, 0);
    }

    let total_planes = n_reds as usize + n_greens as usize + n_blues as usize;
    // Per X11 spec: total planes must fit within the visual's depth
    if total_planes > 24 {
        return build_error(VALUE_ERROR, seq, total_planes as u32, 87, 0);
    }
    let total_colors = n_colors as usize * (1usize << total_planes);

    let pixels = if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
        if contiguous {
            cmap.alloc_cells_contiguous(total_colors as u16)
        } else {
            cmap.alloc_cells(total_colors as u16)
        }
    } else {
        None
    };

    match pixels {
        Some(pix) => {
            let data_len = n_colors as usize * 4;
            let padded = (data_len + 3) & !3;
            // Red/green/blue masks at offsets 12, 16, 20
            let mut bit = 0u32;
            let mut red_mask = 0u32;
            for _ in 0..n_reds {
                red_mask |= 1 << bit;
                bit += 1;
            }
            let mut green_mask = 0u32;
            for _ in 0..n_greens {
                green_mask |= 1 << bit;
                bit += 1;
            }
            let mut blue_mask = 0u32;
            for _ in 0..n_blues {
                blue_mask |= 1 << bit;
                bit += 1;
            }
            let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
                .set_u16(8, n_colors)
                .set_u32(12, red_mask)
                .set_u32(16, green_mask)
                .set_u32(20, blue_mask);
            for (i, &p) in pix.iter().take(n_colors as usize).enumerate() {
                reply = reply.set_u32(32 + i * 4, p);
            }
            reply.build()
        }
        None => build_error(ALLOC_ERROR, seq, 0, 87, 0),
    }
}

// ---------------------------------------------------------------------------
// Opcode 91: QueryColors
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_colors(state: &mut ClientState, req: &QueryColorsRequest) -> Vec<u8> {
    let seq = state.sequence;
    let cmap_id = req.cmap;

    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 91, 0);
    }

    let n_pixels = req.pixels.len();
    let mut colors = Vec::with_capacity(n_pixels);

    for &pixel in req.pixels.iter() {
        // Look up in the colormap (handles both PseudoColor and TrueColor)
        let (r, g, b) = if let Some(cmap) = state.colormaps.get(&cmap_id) {
            cmap.lookup(pixel)
        } else {
            // Default TrueColor decomposition (ROOT_COLORMAP)
            let r = ((pixel >> 16) & 0xFF) as u16;
            let g = ((pixel >> 8) & 0xFF) as u16;
            let b = (pixel & 0xFF) as u16;
            (r << 8 | r, g << 8 | g, b << 8 | b)
        };

        colors.push((r, g, b));
    }

    let data_len = n_pixels * 8; // Each RGB is 8 bytes (r2, g2, b2, pad2)
    let padded = (data_len + 3) & !3;

    let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
        .set_u16(8, n_pixels as u16);

    for (i, &(r, g, b)) in colors.iter().enumerate() {
        let off = 32 + i * 8;
        reply = reply.set_u16(off, r)
            .set_u16(off + 2, g)
            .set_u16(off + 4, b);
        // pad at off+6..off+8
    }

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 92: LookupColor
// ---------------------------------------------------------------------------

pub(crate) fn handle_lookup_color(state: &mut ClientState, req: &LookupColorRequest) -> Vec<u8> {
    let seq = state.sequence;
    let cmap_id = req.cmap;
    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, seq, cmap_id, 92, 0);
    }

    let name = std::str::from_utf8(&req.name).unwrap_or("");

    let (r16, g16, b16) = match crate::colors::lookup_color(name) {
        Some(c) => c,
        None => {
            warn!("LookupColor: unknown color {name:?}");
            return build_error(NAME_ERROR, seq, 0, 92, 0);
        }
    };

    // Reply: exact and visual colors
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, r16)
        .set_u16(10, g16)
        .set_u16(12, b16)
        .set_u16(14, r16)
        .set_u16(16, g16)
        .set_u16(18, b16)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 88: FreeColors
// ---------------------------------------------------------------------------

pub(crate) fn handle_free_colors(state: &mut ClientState, req: &FreeColorsRequest) -> Vec<u8> {
    let cmap_id = req.cmap;
    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, state.sequence, cmap_id, 88, 0);
    }
    // Per X11 spec, FreeColors on a read-only colormap is a BadAccess error
    if let Some(cmap) = state.colormaps.get(&cmap_id) {
        if !cmap.is_writable() {
            return build_error(ACCESS_ERROR, state.sequence, cmap_id, 88, 0);
        }
    }

    let _plane_mask = req.plane_mask;
    let pixels: Vec<u32> = req.pixels.to_vec();
    let n_pixels = pixels.len();

    if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
        cmap.free_cells(&pixels);
        debug!("FreeColors: cmap={cmap_id:#x} freed {} pixels", n_pixels);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 89: StoreColors
// ---------------------------------------------------------------------------

pub(crate) fn handle_store_colors(state: &mut ClientState, req: &StoreColorsRequest) -> Vec<u8> {
    let cmap_id = req.cmap;
    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, state.sequence, cmap_id, 89, 0);
    }
    // Per X11 spec, StoreColors on a read-only colormap is a BadAccess error
    if let Some(cmap) = state.colormaps.get(&cmap_id) {
        if !cmap.is_writable() {
            return build_error(ACCESS_ERROR, state.sequence, cmap_id, 89, 0);
        }
    }

    let items: Vec<(u32, u16, u16, u16, u8)> = req
        .items
        .iter()
        .map(|ci| (ci.pixel, ci.red, ci.green, ci.blue, u8::from(ci.flags)))
        .collect();

    if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
        cmap.store_colors(&items);
        debug!(
            "StoreColors: cmap={cmap_id:#x} stored {} items",
            items.len()
        );
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 90: StoreNamedColor
// ---------------------------------------------------------------------------

pub(crate) fn handle_store_named_color(state: &mut ClientState, req: &StoreNamedColorRequest) -> Vec<u8> {
    let flags = u8::from(req.flags);
    let cmap_id = req.cmap;
    // Validate colormap exists
    if cmap_id != ROOT_COLORMAP && !state.colormaps.contains_key(&cmap_id) {
        return build_error(COLORMAP_ERROR, state.sequence, cmap_id, 90, 0);
    }
    // Per X11 spec, StoreNamedColor on a read-only colormap is a BadAccess error
    if let Some(cmap) = state.colormaps.get(&cmap_id) {
        if !cmap.is_writable() {
            return build_error(ACCESS_ERROR, state.sequence, cmap_id, 90, 0);
        }
    }

    let pixel = req.pixel;
    let name = std::str::from_utf8(&req.name).unwrap_or("");

    if let Some((r, g, b)) = crate::colors::lookup_color(name) {
        if let Some(cmap) = state.colormaps.get_mut(&cmap_id) {
            cmap.store_colors(&[(pixel, r, g, b, flags)]);
            debug!("StoreNamedColor: cmap={cmap_id:#x} pixel={pixel} color={name}");
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 93: CreateCursor
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_cursor(state: &mut ClientState, req: &CreateCursorRequest) -> Vec<u8> {
    let cid = req.cid;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(cid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, cid, 93, 0);
    }
    // Enforce per-client cursor resource limit
    if !state.can_create_cursor() {
        return build_error(ALLOC_ERROR, state.sequence, cid, 93, 0);
    }
    // Per X11 spec: reject duplicate cursor IDs
    if state.cursors.contains_key(&cid) || state.cursor_info.contains_key(&cid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, cid, 93, 0);
    }

    let source_pixmap = req.source;
    let mask_pixmap = req.mask;

    // Per X11 spec: source pixmap must exist and have depth 1
    if !state.pixmaps.contains_key(&source_pixmap) {
        return build_error(PIXMAP_ERROR, state.sequence, source_pixmap, 93, 0);
    }
    // Validate mask pixmap exists if non-zero
    if mask_pixmap != 0 && !state.pixmaps.contains_key(&mask_pixmap) {
        return build_error(PIXMAP_ERROR, state.sequence, mask_pixmap, 93, 0);
    }

    let fore_red = req.fore_red;
    let fore_green = req.fore_green;
    let fore_blue = req.fore_blue;
    let back_red = req.back_red;
    let back_green = req.back_green;
    let back_blue = req.back_blue;
    let hotspot_x = req.x;
    let hotspot_y = req.y;

    // Read the source pixmap dimensions and pixel data to build ARGB cursor bitmap.
    let (width, height, argb_data) = build_cursor_argb(
        state,
        source_pixmap,
        mask_pixmap,
        fore_red,
        fore_green,
        fore_blue,
        back_red,
        back_green,
        back_blue,
    );

    state.cursor_info.insert(
        cid,
        CursorInfo {
            css_name: "default".to_string(),
            source_pixmap,
            mask_pixmap,
            fore_red,
            fore_green,
            fore_blue,
            back_red,
            back_green,
            back_blue,
            hotspot_x,
            hotspot_y,
            argb_data,
            width,
            height,
            name: String::new(),
            anim_frames: Vec::new(),
        },
    );
    state.cursors.insert(cid, "default".to_string());

    info!("CreateCursor: id={cid:#x} (bitmap cursor {width}x{height})");
    Vec::new()
}

/// Build ARGB pixel data from a source bitmap pixmap and optional mask pixmap.
/// Source pixmap is depth-1: bit=1 means foreground, bit=0 means background.
/// Mask pixmap is depth-1: bit=1 means opaque, bit=0 means transparent.
/// If mask is 0 (None), all pixels are opaque.
/// Returns (width, height, argb_data).
fn build_cursor_argb(
    state: &ClientState,
    source_pixmap: u32,
    mask_pixmap: u32,
    fore_red: u16,
    fore_green: u16,
    fore_blue: u16,
    back_red: u16,
    back_green: u16,
    back_blue: u16,
) -> (u16, u16, Vec<u8>) {
    let src_pix = match state.pixmaps.get(&source_pixmap) {
        Some(p) => p,
        None => return (0, 0, Vec::new()),
    };

    let width = src_pix.width;
    let height = src_pix.height;
    let w = width as usize;
    let h = height as usize;

    if w == 0 || h == 0 {
        return (0, 0, Vec::new());
    }

    let src_data = src_pix.framebuffer.data();
    let mask_data = if mask_pixmap != 0 {
        state
            .pixmaps
            .get(&mask_pixmap)
            .map(|p| p.framebuffer.data())
    } else {
        None
    };

    // Foreground and background as 8-bit RGB
    let fg_r = (fore_red >> 8) as u8;
    let fg_g = (fore_green >> 8) as u8;
    let fg_b = (fore_blue >> 8) as u8;
    let bg_r = (back_red >> 8) as u8;
    let bg_g = (back_green >> 8) as u8;
    let bg_b = (back_blue >> 8) as u8;

    let mut argb = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let pixel_off = (y * w + x) * 4;
            if pixel_off + 2 >= src_data.len() {
                continue;
            }

            // Source pixmap stores depth-1 as 0x00 or 0xFF in each channel
            // (written by our PutImage depth-1 handler).
            let is_foreground = src_data[pixel_off] != 0; // check B channel

            // Mask: check if pixel is opaque
            let is_opaque = if let Some(md) = mask_data {
                if pixel_off + 2 < md.len() {
                    md[pixel_off] != 0
                } else {
                    false
                }
            } else {
                true // no mask means all opaque
            };

            let dst_off = (y * w + x) * 4;
            if is_opaque {
                let (r, g, b) = if is_foreground {
                    (fg_r, fg_g, fg_b)
                } else {
                    (bg_r, bg_g, bg_b)
                };
                // ARGB format: A, R, G, B
                argb[dst_off] = b;
                argb[dst_off + 1] = g;
                argb[dst_off + 2] = r;
                argb[dst_off + 3] = 0xFF; // fully opaque
            }
            // else: all zeros = fully transparent (already initialized)
        }
    }

    (width, height, argb)
}

// ---------------------------------------------------------------------------
// Opcode 94: CreateGlyphCursor
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_glyph_cursor(state: &mut ClientState, req: &CreateGlyphCursorRequest) -> Vec<u8> {
    let cid = req.cid;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(cid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, cid, 94, 0);
    }
    // Enforce per-client cursor resource limit
    if !state.can_create_cursor() {
        return build_error(ALLOC_ERROR, state.sequence, cid, 94, 0);
    }
    // Per X11 spec: reject duplicate cursor IDs
    if state.cursors.contains_key(&cid) || state.cursor_info.contains_key(&cid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, cid, 94, 0);
    }

    let source_char = req.source_char;
    let fore_red = req.fore_red;
    let fore_green = req.fore_green;
    let fore_blue = req.fore_blue;
    let back_red = req.back_red;
    let back_green = req.back_green;
    let back_blue = req.back_blue;

    let css_name = glyph_to_css_cursor(source_char).to_string();
    info!("CreateGlyphCursor: id={cid:#x} glyph={source_char} -> \"{css_name}\"");

    state.cursor_info.insert(
        cid,
        CursorInfo {
            css_name: css_name.clone(),
            source_pixmap: 0,
            mask_pixmap: 0,
            fore_red,
            fore_green,
            fore_blue,
            back_red,
            back_green,
            back_blue,
            hotspot_x: 0,
            hotspot_y: 0,
            argb_data: Vec::new(),
            width: 0,
            height: 0,
            name: String::new(),
            anim_frames: Vec::new(),
        },
    );
    state.cursors.insert(cid, css_name);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 95: FreeCursor
// ---------------------------------------------------------------------------

pub(crate) fn handle_free_cursor(state: &mut ClientState, req: &FreeCursorRequest) -> Vec<u8> {
    let cid = req.cursor;
    // Validate cursor exists
    if !state.cursors.contains_key(&cid) {
        return build_error(CURSOR_ERROR, state.sequence, cid, 95, 0);
    }
    state.cursors.remove(&cid);
    state.cursor_info.remove(&cid);
    state.recycle_xid(cid);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 96: RecolorCursor
// ---------------------------------------------------------------------------

pub(crate) fn handle_recolor_cursor(state: &mut ClientState, req: &RecolorCursorRequest) -> Vec<u8> {
    let cid = req.cursor;
    // Validate cursor exists
    if !state.cursors.contains_key(&cid) {
        return build_error(CURSOR_ERROR, state.sequence, cid, 96, 0);
    }
    let fore_red = req.fore_red;
    let fore_green = req.fore_green;
    let fore_blue = req.fore_blue;
    let back_red = req.back_red;
    let back_green = req.back_green;
    let back_blue = req.back_blue;

    // First, rebuild ARGB data if this is a bitmap cursor
    let rebuilt = state.cursor_info.get(&cid).and_then(|info| {
        if info.source_pixmap != 0 {
            let (w, h, argb) = build_cursor_argb(
                state,
                info.source_pixmap,
                info.mask_pixmap,
                fore_red,
                fore_green,
                fore_blue,
                back_red,
                back_green,
                back_blue,
            );
            Some((w, h, argb))
        } else {
            None
        }
    });

    if let Some(info) = state.cursor_info.get_mut(&cid) {
        info.fore_red = fore_red;
        info.fore_green = fore_green;
        info.fore_blue = fore_blue;
        info.back_red = back_red;
        info.back_green = back_green;
        info.back_blue = back_blue;
        if let Some((w, h, argb)) = rebuilt {
            info.width = w;
            info.height = h;
            info.argb_data = argb;
        }
        debug!("RecolorCursor: id={cid:#x} fg=({fore_red},{fore_green},{fore_blue}) bg=({back_red},{back_green},{back_blue})");
    } else {
        debug!("RecolorCursor: cursor {cid:#x} not found");
    }

    Vec::new()
}
