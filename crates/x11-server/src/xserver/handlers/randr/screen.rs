//! Screen-level RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::randr::{
    GetMonitorsReply, GetScreenInfoReply, GetScreenResourcesReply, GetScreenSizeRangeReply,
    ModeFlag, ModeInfo, MonitorInfo, RefreshRates, Rotation, ScreenSize,
};

/// RRSelectInput (4) — select RandR events.
pub(crate) fn handle_select_input(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    use x11rb_protocol::protocol::randr::SelectInputRequest;
    if let Ok(req) = SelectInputRequest::try_parse_request(request_header(data), &data[4..]) {
        let enable = u16::from(req.enable);
        state.randr_event_mask = enable as u32;
        debug!("RRSelectInput mask=0x{:04x}", enable);
    }
    Vec::new()
}

/// RRGetScreenInfo (5) — legacy screen configuration.
pub(crate) fn handle_get_screen_info(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    serialize_var_reply(
        &GetScreenInfoReply {
            rotations: Rotation::from(1u16),
            sequence: seq,
            length: 0,
            root: state.root_window,
            timestamp: state.timestamp(),
            config_timestamp: state.randr_config_timestamp,
            size_id: 0,
            rotation: Rotation::from(1u16),
            rate: 0,
            n_info: 1,
            sizes: vec![ScreenSize {
                width: state.screen_width,
                height: state.screen_height,
                mwidth: 270,
                mheight: 203,
            }],
            rates: vec![RefreshRates { rates: Vec::new() }],
        },
        state.byte_order(),
    )
}

/// RRGetScreenSizeRange (6).
pub(crate) fn handle_get_screen_size_range(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    serialize_reply(
        &GetScreenSizeRangeReply {
            sequence: seq,
            length: 0,
            min_width: 1,
            min_height: 1,
            max_width: 32767,
            max_height: 32767,
        },
        state.byte_order(),
    )
}

/// RRSetScreenSize (7).
pub(crate) fn handle_set_screen_size(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    use x11rb_protocol::protocol::randr::SetScreenSizeRequest;
    if let Ok(req) = SetScreenSizeRequest::try_parse_request(request_header(data), &data[4..]) {
        let new_w = req.width;
        let new_h = req.height;
        if new_w > 0 && new_h > 0 {
            info!(
                "RandR SetScreenSize: {}x{} -> {}x{}",
                state.screen_width, state.screen_height, new_w, new_h
            );
            state.screen_width = new_w;
            state.screen_height = new_h;
            state.randr_config_timestamp += 1;
            if let Some(root) = state.windows.get_mut(&state.root_window) {
                root.width = new_w;
                root.height = new_h;
                root.framebuffer.resize(new_w as u32, new_h as u32);
            }
            state.randr_queue_screen_change_notify();
        }
    }
    Vec::new()
}

/// RRGetScreenResources (8) / RRGetScreenResourcesCurrent (25).
pub(crate) fn build_screen_resources_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    let crtcs: Vec<u32> = state.randr_crtcs.iter().map(|c| c.id).collect();
    let outputs: Vec<u32> = state.randr_outputs.iter().map(|o| o.id).collect();
    let modes: Vec<ModeInfo> = state
        .randr_modes
        .iter()
        .map(|m| ModeInfo {
            id: m.id,
            width: m.width,
            height: m.height,
            dot_clock: m.dot_clock,
            hsync_start: m.h_sync_start,
            hsync_end: m.h_sync_end,
            htotal: m.h_total,
            hskew: 0,
            vsync_start: m.v_sync_start,
            vsync_end: m.v_sync_end,
            vtotal: m.v_total,
            name_len: m.name.len() as u16,
            mode_flags: ModeFlag::from(m.flags),
        })
        .collect();
    let mut names = Vec::new();
    for mode in &state.randr_modes {
        names.extend_from_slice(mode.name.as_bytes());
    }

    serialize_var_reply(
        &GetScreenResourcesReply {
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            config_timestamp: state.randr_config_timestamp,
            crtcs,
            outputs,
            modes,
            names,
        },
        state.byte_order(),
    )
}

/// Build the reply for RRGetMonitors (42).
pub(crate) fn build_get_monitors_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    let active_crtcs: Vec<_> = state
        .randr_crtcs
        .iter()
        .filter(|c| c.mode_id != 0)
        .collect();
    let total_outputs: u32 = (active_crtcs.iter().map(|c| c.outputs.len()).sum::<usize>()
        + state
            .randr_monitors
            .iter()
            .map(|m| m.output_ids.len())
            .sum::<usize>()) as u32;

    let mut monitors: Vec<MonitorInfo> = Vec::new();

    for (i, crtc) in active_crtcs.iter().enumerate() {
        let name_str = if i == 0 {
            "default".to_string()
        } else {
            format!("monitor-{}", i)
        };
        let monitor_name = state.intern_atom(&name_str, false);
        let (mm_w, mm_h) = crtc
            .outputs
            .first()
            .and_then(|&oid| state.randr_find_output(oid))
            .map(|o| (o.mm_width, o.mm_height))
            .unwrap_or((270, 203));
        monitors.push(MonitorInfo {
            name: monitor_name,
            primary: i == 0,
            automatic: true,
            x: crtc.x,
            y: crtc.y,
            width: crtc.width,
            height: crtc.height,
            width_in_millimeters: mm_w,
            height_in_millimeters: mm_h,
            outputs: crtc.outputs.clone(),
        });
    }

    for m in &state.randr_monitors {
        let mm_w = crate::xserver::types::pixels_to_mm_at_96dpi(m.width as u32);
        let mm_h = crate::xserver::types::pixels_to_mm_at_96dpi(m.height as u32);
        monitors.push(MonitorInfo {
            name: m.name_atom,
            primary: m.primary,
            automatic: m.automatic,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            width_in_millimeters: mm_w,
            height_in_millimeters: mm_h,
            outputs: m.output_ids.clone(),
        });
    }

    serialize_var_reply(
        &GetMonitorsReply {
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            n_outputs: total_outputs,
            monitors,
        },
        state.byte_order(),
    )
}
