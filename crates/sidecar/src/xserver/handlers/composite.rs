//! Composite and DAMAGE extension handlers.

use tracing::{debug, info, warn};

use super::super::client::ClientState;
use super::super::core::{OVERLAY_WINDOW, ROOT_COLORMAP};
use super::super::types::{DamageInfo, PixmapState, WindowState, WindowType};
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

pub(crate) fn handle_damage_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("DAMAGE minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.1
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, 1) // major version
                .set_u32(12, 1) // minor version
                .build()
        }
        1 => {
            // DamageCreate: data[4..8] = damage_id, data[8..12] = drawable, data[12] = level
            require_len!(data, 13, seq, 143, minor as u16, state.msb_first);
            let damage_id = state.read_u32(data, 4);
            let drawable = state.read_u32(data, 8);
            let level = data[12];
            info!("DAMAGE Create: id={damage_id:#x} drawable={drawable:#x} level={level}");
            state.damage_regions.insert(
                damage_id,
                DamageInfo {
                    drawable,
                    level,
                    accumulated: super::super::types::XFixesRegion::new(),
                },
            );
            Vec::new()
        }
        2 => {
            // DamageDestroy: data[4..8] = damage_id
            require_len!(data, 8, seq, 143, minor as u16, state.msb_first);
            let damage_id = state.read_u32(data, 4);
            debug!("DAMAGE Destroy: id={damage_id:#x}");
            state.damage_regions.remove(&damage_id);
            state.recycle_xid(damage_id);
            Vec::new()
        }
        3 => {
            // DamageSubtract: data[4..8] = damage_id, data[8..12] = repair_region, data[12..16] = parts_region
            // Per spec: subtract 'repair' from accumulated damage, store remainder in 'parts'.
            require_len!(data, 16, seq, 143, minor as u16, state.msb_first);
            let damage_id = state.read_u32(data, 4);
            let repair = state.read_u32(data, 8);
            let parts = state.read_u32(data, 12);
            debug!("DAMAGE Subtract: id={damage_id:#x} repair={repair:#x} parts={parts:#x}");

            // Get the accumulated damage for this damage object.
            let accumulated = state
                .damage_regions
                .get(&damage_id)
                .map(|d| d.accumulated.clone())
                .unwrap_or_else(super::super::types::XFixesRegion::new);

            let remainder = if repair == 0 {
                // repair=None: subtract everything (acknowledge all damage).
                super::super::types::XFixesRegion::new()
            } else if let Some(repair_region) = state.xfixes_regions.get(&repair) {
                // Subtract the repair region from accumulated damage.
                accumulated.subtract(repair_region)
            } else {
                // Repair region doesn't exist — treat as empty (acknowledge nothing).
                accumulated.clone()
            };

            // Store the remainder in the parts region (if not None).
            if parts != 0 {
                state.xfixes_regions.insert(parts, remainder.clone());
            }

            // Update the accumulated damage to the remainder.
            if let Some(dmg) = state.damage_regions.get_mut(&damage_id) {
                dmg.accumulated = remainder;
            }

            Vec::new()
        }
        4 => {
            // DamageAdd: manually add damage to a drawable
            require_len!(data, 12, seq, 143, minor as u16, state.msb_first);
            let drawable = state.read_u32(data, 4);
            let region = state.read_u32(data, 8);
            debug!("DAMAGE Add: drawable={drawable:#x} region={region:#x}");
            // Get region extents and notify damage
            if let Some(reg) = state.xfixes_regions.get(&region) {
                let ext = reg.extents();
                state.notify_damage(drawable, ext.x, ext.y, ext.width, ext.height);
            }
            Vec::new()
        }
        _ => {
            debug!("Unhandled DAMAGE minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                143,
                minor as u16,
                state.msb_first,
            )
        }
    }
}

pub(crate) fn handle_x_composite_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let minor = data[1];
    info!("Composite minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 0.4
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, 0) // major version
                .set_u32(12, 4) // minor version
                .build()
        }
        1 => {
            // RedirectWindow: data[4..8] = window, data[8] = update
            require_len!(data, 9, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            let update = data[8];
            info!("Composite RedirectWindow: window={window:#x} update={update}");
            if let Some(win) = state.windows.get_mut(&window) {
                win.redirected = true;
            } else {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            }
            Vec::new()
        }
        2 => {
            // RedirectSubwindows: data[4..8] = window, data[8] = update
            require_len!(data, 9, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            let update = data[8];
            info!("Composite RedirectSubwindows: window={window:#x} update={update}");
            if !state.windows.contains_key(&window) {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            }
            // Mark all children as redirected
            let children: Vec<u32> = state
                .windows
                .iter()
                .filter(|(_, w)| w.parent == window)
                .map(|(id, _)| *id)
                .collect();
            for child in children {
                if let Some(w) = state.windows.get_mut(&child) {
                    w.redirected = true;
                }
            }
            Vec::new()
        }
        3 => {
            // UnredirectWindow: data[4..8] = window
            require_len!(data, 8, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            debug!("Composite UnredirectWindow: window={window:#x}");
            if let Some(win) = state.windows.get_mut(&window) {
                win.redirected = false;
            } else {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            }
            Vec::new()
        }
        4 => {
            // UnredirectSubwindows: data[4..8] = window
            require_len!(data, 8, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            debug!("Composite UnredirectSubwindows: window={window:#x}");
            if !state.windows.contains_key(&window) {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            }
            let children: Vec<u32> = state
                .windows
                .iter()
                .filter(|(_, w)| w.parent == window)
                .map(|(id, _)| *id)
                .collect();
            for child in children {
                if let Some(w) = state.windows.get_mut(&child) {
                    w.redirected = false;
                }
            }
            Vec::new()
        }
        5 => {
            // CreateRegionFromBorderClip: data[4..8] = region, data[8..12] = window
            require_len!(data, 12, seq, 142, minor as u16, state.msb_first);
            let region_id = state.read_u32(data, 4);
            let window = state.read_u32(data, 8);
            debug!(
                "Composite CreateRegionFromBorderClip: region={region_id:#x} window={window:#x}"
            );
            let rect = if let Some(win) = state.windows.get(&window) {
                super::super::types::RegionRect {
                    x: 0,
                    y: 0,
                    width: win.width,
                    height: win.height,
                }
            } else {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            };
            state.xfixes_regions.insert(
                region_id,
                super::super::types::XFixesRegion::from_rects(vec![rect]),
            );
            Vec::new()
        }
        6 => {
            // NameWindowPixmap: create a pixmap that is a live alias of the
            // window's off-screen framebuffer.
            // data[4..8] = window, data[8..12] = pixmap
            //
            // Per the Composite spec the returned pixmap IS the window's
            // off-screen storage — reads (GetImage, CopyArea) always see the
            // most recent content and writes go directly to the window.
            // The alias_window link ensures resolve_drawable routes operations
            // back to the window for damage notification.
            //
            // For redirected windows this is the live off-screen surface.
            // For non-redirected windows we clone a snapshot (legacy compat).
            require_len!(data, 12, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            let pixmap = state.read_u32(data, 8);
            if let Some(win) = state.windows.get(&window) {
                let w = win.width;
                let h = win.height;
                let depth = win.depth;
                let redirected = win.redirected;
                // For redirected windows: the pixmap IS the window's framebuffer
                // (shared via alias_window). Drawing to the pixmap draws to the
                // window and vice versa. For non-redirected: clone a snapshot.
                let fb = if redirected {
                    // Create an empty framebuffer as placeholder — actual reads/writes
                    // will be redirected to the window via alias_window in
                    // get_framebuffer_mut / get_framebuffer.
                    crate::framebuffer::Framebuffer::new(w as u32, h as u32)
                } else {
                    win.framebuffer.clone()
                };
                state.pixmaps.insert(
                    pixmap,
                    PixmapState {
                        width: w,
                        height: h,
                        depth,
                        framebuffer: fb,
                        alias_window: Some(window),
                        shm_backing: None,
                    },
                );
                info!("NameWindowPixmap: window={window:#x} -> pixmap={pixmap:#x} {w}x{h} depth={depth} redirected={redirected}");
            } else {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::WINDOW_ERROR,
                    seq,
                    window,
                    142,
                    minor as u16,
                    state.msb_first,
                );
            }
            Vec::new()
        }
        7 => {
            // GetOverlayWindow: create (if needed) and return an InputOutput overlay
            // window positioned above all other windows at the root level.
            // The overlay window is transparent and covers the entire screen.
            if !state.windows.contains_key(&OVERLAY_WINDOW) {
                let w = state.screen_width;
                let h = state.screen_height;
                let overlay = WindowState {
                    id: OVERLAY_WINDOW,
                    parent: state.root_window,
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    border_width: 0,
                    visual: 0x40, // 32-bit ARGB visual for compositing
                    depth: 32,
                    class: 1, // InputOutput
                    mapped: true,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: true,
                    redirected: false,
                    framebuffer: crate::framebuffer::Framebuffer::new(w as u32, h as u32),
                    properties: std::collections::HashMap::new(),
                    owner_client_id: state.client_id.clone(),
                    cursor: None,
                    children_order: Vec::new(),
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: 0,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                };
                state.windows.insert(OVERLAY_WINDOW, overlay);
                // Push overlay to top of root's children stacking order
                if let Some(root) = state.windows.get_mut(&state.root_window) {
                    root.children_order.push(OVERLAY_WINDOW);
                }
                info!("Created overlay window {OVERLAY_WINDOW:#x} ({w}x{h})");
            }
            state.overlay_ref_count += 1;
            info!(
                "Overlay window ref count incremented to {}",
                state.overlay_ref_count
            );
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, OVERLAY_WINDOW)
                .build()
        }
        8 => {
            // ReleaseOverlayWindow: data[4..8] = window
            // Decrements the internal reference count on the overlay window.
            // When the count reaches zero the overlay is no longer in use.
            require_len!(data, 8, seq, 142, minor as u16, state.msb_first);
            let window = state.read_u32(data, 4);
            if state.overlay_ref_count > 0 {
                state.overlay_ref_count -= 1;
                info!("Composite ReleaseOverlayWindow: window={window:#x}, ref count decremented to {}", state.overlay_ref_count);
                if state.overlay_ref_count == 0 {
                    info!("Overlay window {window:#x} is no longer in use (ref count reached 0)");
                }
            } else {
                warn!("Composite ReleaseOverlayWindow: window={window:#x} called with ref count already at 0");
            }
            Vec::new()
        }
        _ => {
            debug!("Unhandled Composite minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                142,
                minor as u16,
                state.msb_first,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::types::{DamageInfo, RegionRect, XFixesRegion};

    fn r(x: i16, y: i16, w: u16, h: u16) -> RegionRect {
        RegionRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn damage_info_accumulates_regions() {
        let mut info = DamageInfo {
            drawable: 0x100,
            level: 0,
            accumulated: XFixesRegion::new(),
        };
        assert!(info.accumulated.rects.is_empty());

        // Accumulate first damage
        let r1 = XFixesRegion::from_rects(vec![r(0, 0, 10, 10)]);
        info.accumulated = info.accumulated.union(&r1);
        assert_eq!(info.accumulated.rects.len(), 1);

        // Accumulate second damage
        let r2 = XFixesRegion::from_rects(vec![r(20, 20, 5, 5)]);
        info.accumulated = info.accumulated.union(&r2);
        assert_eq!(info.accumulated.rects.len(), 2);

        // Extents should cover both
        let ext = info.accumulated.extents();
        assert_eq!(ext.x, 0);
        assert_eq!(ext.y, 0);
        assert_eq!(ext.width, 25);
        assert_eq!(ext.height, 25);
    }

    #[test]
    fn damage_subtract_removes_repair_region() {
        let mut info = DamageInfo {
            drawable: 0x100,
            level: 0,
            accumulated: XFixesRegion::from_rects(vec![r(0, 0, 100, 100)]),
        };

        // Subtract the top half
        let repair = XFixesRegion::from_rects(vec![r(0, 0, 100, 50)]);
        info.accumulated = info.accumulated.subtract(&repair);

        // Should have bottom half remaining
        let ext = info.accumulated.extents();
        assert_eq!(ext.y, 50);
        assert_eq!(ext.height, 50);
    }

    #[test]
    fn damage_subtract_all_clears_accumulated() {
        let mut info = DamageInfo {
            drawable: 0x100,
            level: 0,
            accumulated: XFixesRegion::from_rects(vec![r(0, 0, 10, 10), r(20, 20, 5, 5)]),
        };

        // Subtract everything (repair covers all)
        let repair = XFixesRegion::from_rects(vec![r(0, 0, 1000, 1000)]);
        info.accumulated = info.accumulated.subtract(&repair);

        assert!(info.accumulated.rects.is_empty());
    }

    #[test]
    fn damage_subtract_partial_leaves_remainder() {
        let mut info = DamageInfo {
            drawable: 0x100,
            level: 0,
            accumulated: XFixesRegion::from_rects(vec![r(0, 0, 50, 50), r(60, 60, 20, 20)]),
        };

        // Subtract only the first rect area
        let repair = XFixesRegion::from_rects(vec![r(0, 0, 50, 50)]);
        info.accumulated = info.accumulated.subtract(&repair);

        // Second rect should remain
        assert!(!info.accumulated.rects.is_empty());
        let ext = info.accumulated.extents();
        assert_eq!(ext.x, 60);
        assert_eq!(ext.y, 60);
    }
}
