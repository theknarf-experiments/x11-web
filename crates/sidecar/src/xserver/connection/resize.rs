//! Window and screen resize helpers for the per-client event loop.

use tracing::info;
use x11_web_protocol::DisplayUpdate;

use crate::framebuffer::Framebuffer;
use crate::xserver::client::ClientState;
use crate::xserver::core::*;
use crate::xserver::types::{RandrMode, generate_edid, PropertyValue};
use crate::xserver::is_descendant_of;

/// Resize a top-level window in response to a frontend canvas size change.
pub(crate) fn resize_window(state: &mut ClientState, window_uuid: &str, width: u16, height: u16) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;
    let bo = state.msb_first;

    let window_id = match state.x11_to_uuid.iter().find(|(_, uuid)| uuid.as_str() == window_uuid) {
        Some((&wid, _)) => wid,
        None => return events,
    };

    // Compute above_sibling before the mutable borrow
    let above_sib = state.windows.get(&window_id)
        .and_then(|w| {
            let parent = state.windows.get(&w.parent)?;
            let pos = parent.children_order.iter().position(|&id| id == window_id)?;
            if pos > 0 { Some(parent.children_order[pos - 1]) } else { None }
        })
        .unwrap_or(0);

    // _NET_WM_SYNC_REQUEST: if the window has a sync counter and supports the
    // protocol, increment the counter and send a ClientMessage before resizing.
    // This lets the client synchronize its repainting with the resize.
    {
        let sync_counter = state.windows.get(&window_id).and_then(|w| w.sync_request_counter);
        if let Some(counter_id) = sync_counter {
            let net_wm_sync_request_atom = state.intern_atom("_NET_WM_SYNC_REQUEST", false);
            let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
            let supports_sync = state.window_supports_protocol(window_id, net_wm_sync_request_atom);
            if supports_sync {
                // Increment the sync request value
                let new_value = state.windows.get(&window_id)
                    .map(|w| w.sync_request_value.wrapping_add(1))
                    .unwrap_or(1);
                if let Some(win) = state.windows.get_mut(&window_id) {
                    win.sync_request_value = new_value;
                }
                // Update the SYNC counter value
                let lo = new_value as u32;
                let hi = (new_value >> 32) as i32;
                if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
                    counter.value_lo = lo;
                    counter.value_hi = hi;
                }

                // Send _NET_WM_SYNC_REQUEST ClientMessage
                let timestamp = state.timestamp();
                let mut cm = [0u8; 32];
                cm[0] = CLIENT_MESSAGE_EVENT;
                cm[1] = 32; // format
                write_u16_bo(&mut cm, 2, seq, bo);
                write_u32_bo(&mut cm, 4, window_id, bo);
                write_u32_bo(&mut cm, 8, wm_protocols_atom, bo);
                write_u32_bo(&mut cm, 12, net_wm_sync_request_atom, bo);
                write_u32_bo(&mut cm, 16, timestamp, bo);
                write_u32_bo(&mut cm, 20, lo, bo); // counter value lo
                write_u32_bo(&mut cm, 24, hi as u32, bo); // counter value hi
                events.extend_from_slice(&cm);
            }
        }
    }

    if let Some(win) = state.windows.get_mut(&window_id) {
        win.width = width;
        win.height = height;
        win.framebuffer = Framebuffer::new(width as u32, height as u32);

        let mut event = [0u8; 32];
        event[0] = CONFIGURE_NOTIFY_EVENT;
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, window_id, bo);
        write_u32_bo(&mut event, 8, window_id, bo);
        write_u32_bo(&mut event, 12, above_sib, bo);
        write_i16_bo(&mut event, 16, win.x, bo);
        write_i16_bo(&mut event, 18, win.y, bo);
        write_u16_bo(&mut event, 20, width, bo);
        write_u16_bo(&mut event, 22, height, bo);
        write_u16_bo(&mut event, 24, win.border_width, bo);
        events.extend_from_slice(&event);
    }

    let exposed: Vec<(u32, u16, u16)> = std::iter::once(window_id)
        .chain(
            state
                .windows
                .values()
                .filter(|w| {
                    w.mapped && w.id != window_id && is_descendant_of(&state.windows, w.id, window_id)
                })
                .map(|w| w.id),
        )
        .filter_map(|wid| state.windows.get(&wid).map(|w| (wid, w.width, w.height)))
        .collect();
    let expose_total = exposed.len();
    for (i, (wid, w, h)) in exposed.iter().enumerate() {
        let mut expose = [0u8; 32];
        expose[0] = EXPOSE_EVENT;
        write_u16_bo(&mut expose, 2, seq, bo);
        write_u32_bo(&mut expose, 4, *wid, bo);
        write_u16_bo(&mut expose, 12, *w, bo);
        write_u16_bo(&mut expose, 14, *h, bo);
        let remaining = (expose_total - 1 - i) as u16;
        write_u16_bo(&mut expose, 16, remaining, bo); // count: remaining Expose events
        events.extend_from_slice(&expose);
    }

    if let Some(win) = state.windows.get(&window_id) {
        let owner = if win.owner_client_id.is_empty() {
            state.client_id.clone()
        } else {
            win.owner_client_id.clone()
        };
        let _ = state.update_tx.send((
            owner,
            DisplayUpdate::WindowConfigured {
                window_id: window_uuid.to_string(),
                x: win.x,
                y: win.y,
                width: win.width,
                height: win.height,
                border_width: win.border_width,
                border_pixel: win.border_pixel,
            },
        ));
    }

    events
}

/// Apply a screen resize: update root window, screen dimensions, RandR state,
/// and generate the appropriate X11 events (ConfigureNotify, RRScreenChangeNotify,
/// RRCrtcChangeNotify, Expose) for the connection.
pub(super) fn apply_screen_resize(state: &mut ClientState, new_w: u16, new_h: u16) -> Vec<u8> {
    let old_w = state.screen_width;
    let old_h = state.screen_height;

    info!(
        "Screen resize: {}x{} -> {}x{} (client {})",
        old_w, old_h, new_w, new_h, state.client_id
    );

    // 1. Update screen dimensions
    state.screen_width = new_w;
    state.screen_height = new_h;
    state.randr_config_timestamp += 1;

    // 2. Update root window dimensions and framebuffer
    if let Some(root) = state.windows.get_mut(&state.root_window) {
        root.width = new_w;
        root.height = new_h;
        root.framebuffer.resize(new_w as u32, new_h as u32);
    }

    // Also update shared_windows so new connections start with the current size.
    if let Ok(mut shared) = state.shared_windows.lock() {
        if let Some(root) = shared.get_mut(&state.root_window) {
            root.width = new_w;
            root.height = new_h;
            root.framebuffer.resize(new_w as u32, new_h as u32);
        }
    }

    // 3. Update EWMH properties on root window (_NET_DESKTOP_GEOMETRY, _NET_WORKAREA)
    {
        let geom_atom = state.intern_atom("_NET_DESKTOP_GEOMETRY", false);
        let workarea_atom = state.intern_atom("_NET_WORKAREA", false);

        let mut geom_data = Vec::with_capacity(8);
        geom_data.extend_from_slice(&(new_w as u32).to_le_bytes());
        geom_data.extend_from_slice(&(new_h as u32).to_le_bytes());

        let mut workarea_data = Vec::with_capacity(16);
        workarea_data.extend_from_slice(&0u32.to_le_bytes());
        workarea_data.extend_from_slice(&0u32.to_le_bytes());
        workarea_data.extend_from_slice(&(new_w as u32).to_le_bytes());
        workarea_data.extend_from_slice(&(new_h as u32).to_le_bytes());

        if let Some(root) = state.windows.get_mut(&state.root_window) {
            root.properties.insert(geom_atom, PropertyValue {
                prop_type: 6, // CARDINAL
                format: 32,
                data: geom_data,
            });
            root.properties.insert(workarea_atom, PropertyValue {
                prop_type: 6,
                format: 32,
                data: workarea_data,
            });
        }
    }

    // 4. Update RandR CRTC, mode, and output to reflect new size
    let crtc_id: u32 = 100;
    let mode_id: u32 = 300;
    let output_id: u32 = 200;

    // Update mode
    let new_mode = RandrMode::new(mode_id, new_w, new_h);
    if let Some(mode) = state.randr_modes.iter_mut().find(|m| m.id == mode_id) {
        *mode = new_mode;
    }

    // Update CRTC
    if let Some(crtc) = state.randr_crtcs.iter_mut().find(|c| c.id == crtc_id) {
        crtc.width = new_w;
        crtc.height = new_h;
    }

    // Update output EDID and dimensions
    // Compute mm dimensions proportionally (96 DPI default)
    let mm_w = ((new_w as u32) * 254 + 480) / 960;
    let mm_h = ((new_h as u32) * 254 + 480) / 960;
    let edid_atom = state.intern_atom("EDID", false);
    let edid_data = generate_edid(mm_w as u16, mm_h as u16, new_w, new_h);
    if let Some(output) = state.randr_outputs.iter_mut().find(|o| o.id == output_id) {
        output.mm_width = mm_w;
        output.mm_height = mm_h;
        output.properties.insert(edid_atom, PropertyValue {
            prop_type: edid_atom,
            format: 8,
            data: edid_data,
        });
    }

    // 5. Generate X11 events
    let mut events = Vec::new();
    let bo = state.msb_first;
    let seq = state.sequence;

    // ConfigureNotify for root window
    {
        let mut event = [0u8; 32];
        event[0] = CONFIGURE_NOTIFY_EVENT;
        write_u16_bo(&mut event, 2, seq, bo);
        write_u32_bo(&mut event, 4, state.root_window, bo); // event window
        write_u32_bo(&mut event, 8, state.root_window, bo); // window
        // above_sibling = 0 (root window has no parent/siblings)
        write_i16_bo(&mut event, 16, 0, bo);  // x
        write_i16_bo(&mut event, 18, 0, bo);  // y
        write_u16_bo(&mut event, 20, new_w, bo);
        write_u16_bo(&mut event, 22, new_h, bo);
        write_u16_bo(&mut event, 24, 0, bo);  // border_width
        events.extend_from_slice(&event);
    }

    // RRScreenChangeNotify
    state.randr_queue_screen_change_notify();

    // RRCrtcChangeNotify
    state.randr_queue_crtc_change_notify(crtc_id);

    // Drain pending RandR events into output
    let pending: Vec<Vec<u8>> = state.pending_events.drain(..).collect();
    for ev in pending {
        events.extend_from_slice(&ev);
    }

    // Expose on root so clients can redraw
    {
        let mut expose = [0u8; 32];
        expose[0] = EXPOSE_EVENT;
        write_u16_bo(&mut expose, 2, seq, bo);
        write_u32_bo(&mut expose, 4, state.root_window, bo);
        write_u16_bo(&mut expose, 12, new_w, bo);
        write_u16_bo(&mut expose, 14, new_h, bo);
        write_u16_bo(&mut expose, 16, 0, bo); // count = 0 (last Expose)
        events.extend_from_slice(&expose);
    }

    events
}
