//! Color/colormap and cursor handlers (opcodes 78-96).

use super::*;
use crate::xserver::event::serialize_event;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xproto::{
    AllocColorCellsReply, AllocColorCellsRequest, AllocColorPlanesReply, AllocColorPlanesRequest,
    AllocColorReply, AllocColorRequest, AllocNamedColorReply, AllocNamedColorRequest,
    ColormapNotifyEvent, ColormapState as XColormapState, CopyColormapAndFreeRequest,
    CreateColormapRequest, CreateCursorRequest, CreateGlyphCursorRequest, FreeColormapRequest,
    FreeColorsRequest, FreeCursorRequest, InstallColormapRequest, ListInstalledColormapsReply,
    ListInstalledColormapsRequest, LookupColorReply, LookupColorRequest, QueryColorsReply,
    QueryColorsRequest, RecolorCursorRequest, Rgb, StoreColorsRequest, StoreNamedColorRequest,
    UninstallColormapRequest,
};

// ---------------------------------------------------------------------------
// Opcode 78: CreateColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_colormap(
    state: &mut ClientState,
    req: &CreateColormapRequest,
) -> Vec<u8> {
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
    let cmap = match visual {
        VISUAL_TRUE_COLOR_24 | VISUAL_TRUE_COLOR_16 | VISUAL_TRUE_COLOR_ARGB_32 => {
            ColormapState::new_truecolor(visual)
        }
        VISUAL_DIRECT_COLOR_24 => ColormapState::new_directcolor(visual, 256),
        VISUAL_PSEUDO_COLOR_8 => ColormapState::new_pseudocolor(visual, 256),
        VISUAL_STATIC_GRAY_4 => ColormapState::new_staticgray(visual, 16),
        VISUAL_GRAY_SCALE_8 => ColormapState::new_grayscale(visual, 256),
        VISUAL_STATIC_COLOR_8 => ColormapState::new_staticcolor(visual, 256),
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
        ColormapState::new_truecolor(VISUAL_TRUE_COLOR_24)
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

pub(crate) fn handle_install_colormap(
    state: &mut ClientState,
    req: &InstallColormapRequest,
) -> Vec<u8> {
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
        let event = serialize_event(
            &ColormapNotifyEvent {
                response_type: COLOURMAP_NOTIFY_EVENT,
                sequence: 0,
                window: wid,
                colormap: mid,
                new: true,
                state: XColormapState::INSTALLED,
            },
            state.msb_first,
        );
        state.pending_events.push(event.clone());
        // Also broadcast to other connections selecting on this window
        state.broadcast_event(wid, EventMask::COLOR_MAP_CHANGE, &event);
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 82: UninstallColormap
// ---------------------------------------------------------------------------

pub(crate) fn handle_uninstall_colormap(
    state: &mut ClientState,
    req: &UninstallColormapRequest,
) -> Vec<u8> {
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
        let event = serialize_event(
            &ColormapNotifyEvent {
                response_type: COLOURMAP_NOTIFY_EVENT,
                sequence: 0,
                window: wid,
                colormap: mid,
                new: true,
                state: XColormapState::UNINSTALLED,
            },
            state.msb_first,
        );
        state.pending_events.push(event.clone());
        state.broadcast_event(wid, EventMask::COLOR_MAP_CHANGE, &event);
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
            let event = serialize_event(
                &ColormapNotifyEvent {
                    response_type: COLOURMAP_NOTIFY_EVENT,
                    sequence: 0,
                    window: wid,
                    colormap: default_cmap,
                    new: true,
                    state: XColormapState::INSTALLED,
                },
                state.msb_first,
            );
            state.pending_events.push(event.clone());
            state.broadcast_event(wid, EventMask::COLOR_MAP_CHANGE, &event);
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 83: ListInstalledColormaps
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_installed_colormaps(
    state: &ClientState,
    _req: &ListInstalledColormapsRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    // _req.window is available but currently unused — we return all installed colormaps.
    let cmaps: Vec<u32> = state.installed_colormaps.iter().copied().collect();

    serialize_var_reply(
        &ListInstalledColormapsReply {
            sequence: seq,
            length: 0,
            cmaps,
        },
        state.byte_order(),
    )
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

    serialize_reply(
        &AllocColorReply {
            sequence: seq,
            length: 0,
            red,
            green,
            blue,
            pixel,
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 85: AllocNamedColor
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_named_color(
    state: &mut ClientState,
    req: &AllocNamedColorRequest,
) -> Vec<u8> {
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

    serialize_reply(
        &AllocNamedColorReply {
            sequence: seq,
            length: 0,
            pixel,
            exact_red: r16,
            exact_green: g16,
            exact_blue: b16,
            visual_red: r16,
            visual_green: g16,
            visual_blue: b16,
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 86: AllocColorCells
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_color_cells(
    state: &mut ClientState,
    req: &AllocColorCellsRequest,
) -> Vec<u8> {
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
            let n_pix = n_colors as usize;
            let n_mask = if n_planes > 0 { n_planes as usize } else { 0 };
            let pixels: Vec<u32> = pix.iter().take(n_pix).copied().collect();
            let masks: Vec<u32> = (0..n_mask).map(|i| 1u32 << i).collect();
            serialize_var_reply(
                &AllocColorCellsReply {
                    sequence: seq,
                    length: 0,
                    pixels,
                    masks,
                },
                state.byte_order(),
            )
        }
        None => build_error(ALLOC_ERROR, seq, 0, 86, 0),
    }
}

// ---------------------------------------------------------------------------
// Opcode 87: AllocColorPlanes
// ---------------------------------------------------------------------------

pub(crate) fn handle_alloc_color_planes(
    state: &mut ClientState,
    req: &AllocColorPlanesRequest,
) -> Vec<u8> {
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
            let pixels: Vec<u32> = pix.iter().take(n_colors as usize).copied().collect();
            serialize_var_reply(
                &AllocColorPlanesReply {
                    sequence: seq,
                    length: 0,
                    red_mask,
                    green_mask,
                    blue_mask,
                    pixels,
                },
                state.byte_order(),
            )
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

    let colors: Vec<Rgb> = req
        .pixels
        .iter()
        .map(|&pixel| {
            let (r, g, b) = if let Some(cmap) = state.colormaps.get(&cmap_id) {
                cmap.lookup(pixel)
            } else {
                // Default TrueColor decomposition (ROOT_COLORMAP)
                let (r8, g8, b8) = crate::framebuffer::unpack_rgb(pixel);
                let (r, g, b) = (r8 as u16, g8 as u16, b8 as u16);
                (r << 8 | r, g << 8 | g, b << 8 | b)
            };
            Rgb {
                red: r,
                green: g,
                blue: b,
            }
        })
        .collect();

    serialize_var_reply(
        &QueryColorsReply {
            sequence: seq,
            length: 0,
            colors,
        },
        state.byte_order(),
    )
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

    serialize_reply(
        &LookupColorReply {
            sequence: seq,
            length: 0,
            exact_red: r16,
            exact_green: g16,
            exact_blue: b16,
            visual_red: r16,
            visual_green: g16,
            visual_blue: b16,
        },
        state.byte_order(),
    )
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

pub(crate) fn handle_store_named_color(
    state: &mut ClientState,
    req: &StoreNamedColorRequest,
) -> Vec<u8> {
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
                // RGBA byte order — matches the wire format the frontend
                // (and tiny-skia) consume.
                argb[dst_off] = r;
                argb[dst_off + 1] = g;
                argb[dst_off + 2] = b;
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

pub(crate) fn handle_create_glyph_cursor(
    state: &mut ClientState,
    req: &CreateGlyphCursorRequest,
) -> Vec<u8> {
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

pub(crate) fn handle_recolor_cursor(
    state: &mut ClientState,
    req: &RecolorCursorRequest,
) -> Vec<u8> {
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
