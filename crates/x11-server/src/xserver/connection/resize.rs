//! Window and screen resize helpers for the per-client event loop.

use tracing::info;
use x11_web_protocol::DisplayUpdate;
use x11rb_protocol::protocol::xproto::{
    ClientMessageData, ClientMessageEvent, ConfigureNotifyEvent, ExposeEvent,
};

use crate::framebuffer::Framebuffer;
use crate::xserver::client::ClientState;
use crate::xserver::core::*;
use crate::xserver::event::serialize_event;
use crate::xserver::is_descendant_of;
use crate::xserver::types::{
    generate_edid, PropertyValue, RandrMode, DEFAULT_RANDR_CRTC_ID, DEFAULT_RANDR_MODE_ID,
    DEFAULT_RANDR_OUTPUT_ID,
};

/// Resize a top-level window in response to a frontend canvas size change.
pub(crate) fn resize_window(
    state: &mut ClientState,
    window_uuid: &str,
    width: u16,
    height: u16,
) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;
    let bo = state.msb_first;

    let window_id = match state
        .x11_to_uuid
        .iter()
        .find(|(_, uuid)| uuid.as_str() == window_uuid)
    {
        Some((&wid, _)) => wid,
        None => return events,
    };

    // Compute above_sibling before the mutable borrow
    let above_sib = state
        .windows
        .get(&window_id)
        .and_then(|w| {
            let parent = state.windows.get(&w.parent)?;
            let pos = parent
                .children_order
                .iter()
                .position(|&id| id == window_id)?;
            if pos > 0 {
                Some(parent.children_order[pos - 1])
            } else {
                None
            }
        })
        .unwrap_or(0);

    // _NET_WM_SYNC_REQUEST: if the window has a sync counter and supports the
    // protocol, increment the counter and send a ClientMessage before resizing.
    // This lets the client synchronize its repainting with the resize.
    {
        let sync_counter = state
            .windows
            .get(&window_id)
            .and_then(|w| w.sync_request_counter);
        if let Some(counter_id) = sync_counter {
            let net_wm_sync_request_atom = state.intern_atom("_NET_WM_SYNC_REQUEST", false);
            let wm_protocols_atom = state.intern_atom("WM_PROTOCOLS", false);
            let supports_sync = state.window_supports_protocol(window_id, net_wm_sync_request_atom);
            if supports_sync {
                // Increment the sync request value
                let new_value = state
                    .windows
                    .get(&window_id)
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
                let cm = serialize_event(
                    &ClientMessageEvent {
                        response_type: CLIENT_MESSAGE_EVENT,
                        format: 32,
                        sequence: seq,
                        window: window_id,
                        type_: wm_protocols_atom,
                        data: ClientMessageData::from([
                            net_wm_sync_request_atom,
                            timestamp,
                            lo,
                            hi as u32,
                            0,
                        ]),
                    },
                    bo,
                );
                events.extend_from_slice(&cm);
            }
        }
    }

    if let Some(win) = state.windows.get_mut(&window_id) {
        win.width = width;
        win.height = height;
        win.framebuffer = Framebuffer::new(width as u32, height as u32);

        let event = serialize_event(
            &ConfigureNotifyEvent {
                response_type: CONFIGURE_NOTIFY_EVENT,
                sequence: seq,
                event: window_id,
                window: window_id,
                above_sibling: above_sib,
                x: win.x,
                y: win.y,
                width,
                height,
                border_width: win.border_width,
                override_redirect: false,
            },
            bo,
        );
        events.extend_from_slice(&event);
    }

    let exposed: Vec<(u32, u16, u16)> = std::iter::once(window_id)
        .chain(
            state
                .windows
                .values()
                .filter(|w| {
                    w.mapped
                        && w.id != window_id
                        && is_descendant_of(&state.windows, w.id, window_id)
                })
                .map(|w| w.id),
        )
        .filter_map(|wid| state.windows.get(&wid).map(|w| (wid, w.width, w.height)))
        .collect();
    let expose_total = exposed.len();
    for (i, (wid, w, h)) in exposed.iter().enumerate() {
        let remaining = (expose_total - 1 - i) as u16;
        let expose = serialize_event(
            &ExposeEvent {
                response_type: EXPOSE_EVENT,
                sequence: seq,
                window: *wid,
                x: 0,
                y: 0,
                width: *w,
                height: *h,
                count: remaining,
            },
            bo,
        );
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
                resizable: true,
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
            root.properties.insert(
                geom_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: geom_data,
                },
            );
            root.properties.insert(
                workarea_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: workarea_data,
                },
            );
        }
    }

    // 4. Update RandR CRTC, mode, and output to reflect new size
    let crtc_id = DEFAULT_RANDR_CRTC_ID;
    let mode_id = DEFAULT_RANDR_MODE_ID;
    let output_id = DEFAULT_RANDR_OUTPUT_ID;

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
    let mm_w = crate::xserver::types::pixels_to_mm_at_96dpi(new_w as u32);
    let mm_h = crate::xserver::types::pixels_to_mm_at_96dpi(new_h as u32);
    let edid_atom = state.intern_atom("EDID", false);
    let edid_data = generate_edid(mm_w as u16, mm_h as u16, new_w, new_h);
    if let Some(output) = state.randr_outputs.iter_mut().find(|o| o.id == output_id) {
        output.mm_width = mm_w;
        output.mm_height = mm_h;
        output.properties.insert(
            edid_atom,
            PropertyValue {
                prop_type: edid_atom,
                format: 8,
                data: edid_data,
            },
        );
    }

    // 5. Generate X11 events
    let mut events = Vec::new();
    let bo = state.msb_first;
    let seq = state.sequence;

    // ConfigureNotify for root window
    {
        let event = serialize_event(
            &ConfigureNotifyEvent {
                response_type: CONFIGURE_NOTIFY_EVENT,
                sequence: seq,
                event: state.root_window,
                window: state.root_window,
                above_sibling: 0,
                x: 0,
                y: 0,
                width: new_w,
                height: new_h,
                border_width: 0,
                override_redirect: false,
            },
            bo,
        );
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
        let expose = serialize_event(
            &ExposeEvent {
                response_type: EXPOSE_EVENT,
                sequence: seq,
                window: state.root_window,
                x: 0,
                y: 0,
                width: new_w,
                height: new_h,
                count: 0,
            },
            bo,
        );
        events.extend_from_slice(&expose);
    }

    events
}
