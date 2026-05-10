//! Screen-level RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;

/// RandR `MODEINFO` wire structure: 32 bytes per mode in
/// GetScreenResources/GetScreenResourcesCurrent replies.
mod modeinfo_layout {
    /// Wire size of a single MODEINFO entry.
    pub(super) const SIZE: usize = 32;
    /// u32 mode ID.
    pub(super) const ID: usize = 0;
    /// u16 width.
    pub(super) const WIDTH: usize = 4;
    /// u16 height.
    pub(super) const HEIGHT: usize = 6;
    /// u32 dot clock.
    pub(super) const DOT_CLOCK: usize = 8;
    /// u16 horizontal sync start.
    pub(super) const H_SYNC_START: usize = 12;
    /// u16 horizontal sync end.
    pub(super) const H_SYNC_END: usize = 14;
    /// u16 horizontal total.
    pub(super) const H_TOTAL: usize = 16;
    // hSkew at offset 18 is always 0 for our backend.
    /// u16 vertical sync start.
    pub(super) const V_SYNC_START: usize = 20;
    /// u16 vertical sync end.
    pub(super) const V_SYNC_END: usize = 22;
    /// u16 vertical total.
    pub(super) const V_TOTAL: usize = 24;
    /// u16 mode-name length in bytes.
    pub(super) const NAME_LEN: usize = 26;
    /// u32 mode flags.
    pub(super) const FLAGS: usize = 28;
}

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
    let extra_data_len: usize = 8;
    let num_sizes: u16 = 1;
    ReplyBuf::with_extra(seq, extra_data_len, state.msb_first)
        .set_data_byte(1) // rotations = Rotate_0
        .set_u32(8, state.root_window)
        .set_u32(12, state.timestamp())
        .set_u32(16, state.randr_config_timestamp)
        .set_u16(20, num_sizes)
        .set_u16(22, 0) // sizeID
        .set_u16(24, 1) // rotation = Rotate_0
        .set_u16(26, 0) // nrateEnts
        .set_u16(32, state.screen_width)
        .set_u16(34, state.screen_height)
        .set_u16(36, 270)
        .set_u16(38, 203)
        .build()
}

/// RRGetScreenSizeRange (6).
pub(crate) fn handle_get_screen_size_range(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, 1) // min_width
        .set_u16(10, 1) // min_height
        .set_u16(12, 32767) // max_width
        .set_u16(14, 32767) // max_height
        .build()
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
    let num_crtcs = state.randr_crtcs.len();
    let num_outputs = state.randr_outputs.len();
    let num_modes = state.randr_modes.len();

    // Collect all mode name bytes (concatenated).
    let mut names_bytes = Vec::new();
    for mode in &state.randr_modes {
        names_bytes.extend_from_slice(mode.name.as_bytes());
    }
    let names_len = names_bytes.len();
    let names_pad = (4 - (names_len % 4)) % 4;

    // Variable data:
    //   crtc_ids: num_crtcs * 4
    //   output_ids: num_outputs * 4
    //   mode_infos: num_modes * 32
    //   mode_names: names_len + pad
    let var_len = num_crtcs * 4
        + num_outputs * 4
        + num_modes * modeinfo_layout::SIZE
        + names_len
        + names_pad;

    let mut reply = ReplyBuf::with_extra(seq, var_len, state.msb_first)
        .set_u32(8, state.timestamp())
        .set_u32(12, state.randr_config_timestamp)
        .set_u16(16, num_crtcs as u16)
        .set_u16(18, num_outputs as u16)
        .set_u16(20, num_modes as u16)
        .set_u16(22, names_len as u16);

    let mut off = 32;

    // CRTC IDs
    for crtc in &state.randr_crtcs {
        reply = reply.set_u32(off, crtc.id);
        off += 4;
    }

    // Output IDs
    for output in &state.randr_outputs {
        reply = reply.set_u32(off, output.id);
        off += 4;
    }

    // ModeInfo structs (32 bytes each)
    for mode in &state.randr_modes {
        reply = reply
            .set_u32(off + modeinfo_layout::ID, mode.id)
            .set_u16(off + modeinfo_layout::WIDTH, mode.width)
            .set_u16(off + modeinfo_layout::HEIGHT, mode.height)
            .set_u32(off + modeinfo_layout::DOT_CLOCK, mode.dot_clock)
            .set_u16(off + modeinfo_layout::H_SYNC_START, mode.h_sync_start)
            .set_u16(off + modeinfo_layout::H_SYNC_END, mode.h_sync_end)
            .set_u16(off + modeinfo_layout::H_TOTAL, mode.h_total)
            // hSkew at off+18 = 0
            .set_u16(off + modeinfo_layout::V_SYNC_START, mode.v_sync_start)
            .set_u16(off + modeinfo_layout::V_SYNC_END, mode.v_sync_end)
            .set_u16(off + modeinfo_layout::V_TOTAL, mode.v_total)
            .set_u16(off + modeinfo_layout::NAME_LEN, mode.name.len() as u16)
            .set_u32(off + modeinfo_layout::FLAGS, mode.flags);
        off += modeinfo_layout::SIZE;
    }

    // Mode names (concatenated)
    reply = reply.set_bytes(off, &names_bytes);

    reply.build()
}

/// Build the reply for RRGetMonitors (42).
/// Includes both automatic monitors derived from CRTCs and user-defined
/// monitors set via RRSetMonitor.
pub(crate) fn build_get_monitors_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    // Collect automatic monitors from active CRTCs.
    let active_crtcs: Vec<_> = state
        .randr_crtcs
        .iter()
        .filter(|c| c.mode_id != 0)
        .collect();
    let n_auto = active_crtcs.len();
    let n_user = state.randr_monitors.len();
    let n_monitors = n_auto + n_user;
    let total_outputs: usize = active_crtcs.iter().map(|c| c.outputs.len()).sum::<usize>()
        + state
            .randr_monitors
            .iter()
            .map(|m| m.output_ids.len())
            .sum::<usize>();

    // MonitorInfo = 24 bytes + nOutput * 4
    let mut monitor_data_len = 0usize;
    for c in &active_crtcs {
        monitor_data_len += 24 + c.outputs.len() * 4;
    }
    for m in &state.randr_monitors {
        monitor_data_len += 24 + m.output_ids.len() * 4;
    }
    let pad = (4 - (monitor_data_len % 4)) % 4;
    let extra_bytes = monitor_data_len + pad;

    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_u32(8, state.timestamp())
        .set_u32(12, n_monitors as u32)
        .set_u32(16, total_outputs as u32);

    let mut off = 32;

    // Emit automatic monitors from CRTCs.
    for (i, crtc) in active_crtcs.iter().enumerate() {
        let name_str = if i == 0 {
            "default".to_string()
        } else {
            format!("monitor-{}", i)
        };
        let monitor_name = state.intern_atom(&name_str, false);

        reply = reply.set_u32(off, monitor_name);
        off += 4;
        {
            let buf = reply.buf_mut();
            buf[off] = if i == 0 { 1 } else { 0 }; // primary
            off += 1;
            buf[off] = 1; // automatic
            off += 1;
        }
        reply = reply.set_u16(off, crtc.outputs.len() as u16);
        off += 2;
        reply = reply.set_i16(off, crtc.x);
        off += 2;
        reply = reply.set_i16(off, crtc.y);
        off += 2;
        reply = reply.set_u16(off, crtc.width);
        off += 2;
        reply = reply.set_u16(off, crtc.height);
        off += 2;
        let (mm_w, mm_h) = crtc
            .outputs
            .first()
            .and_then(|&oid| state.randr_find_output(oid))
            .map(|o| (o.mm_width, o.mm_height))
            .unwrap_or((270, 203));
        reply = reply.set_u32(off, mm_w);
        off += 4;
        reply = reply.set_u32(off, mm_h);
        off += 4;
        for &oid in &crtc.outputs {
            reply = reply.set_u32(off, oid);
            off += 4;
        }
    }

    // Emit user-defined monitors.
    for m in &state.randr_monitors {
        reply = reply.set_u32(off, m.name_atom);
        off += 4;
        {
            let buf = reply.buf_mut();
            buf[off] = m.primary as u8;
            off += 1;
            buf[off] = m.automatic as u8;
            off += 1;
        }
        reply = reply.set_u16(off, m.output_ids.len() as u16);
        off += 2;
        reply = reply.set_i16(off, m.x);
        off += 2;
        reply = reply.set_i16(off, m.y);
        off += 2;
        reply = reply.set_u16(off, m.width);
        off += 2;
        reply = reply.set_u16(off, m.height);
        off += 2;
        // mm dimensions: approximate from pixel size (96 DPI).
        let mm_w = crate::xserver::types::pixels_to_mm_at_96dpi(m.width as u32);
        let mm_h = crate::xserver::types::pixels_to_mm_at_96dpi(m.height as u32);
        reply = reply.set_u32(off, mm_w);
        off += 4;
        reply = reply.set_u32(off, mm_h);
        off += 4;
        for &oid in &m.output_ids {
            reply = reply.set_u32(off, oid);
            off += 4;
        }
    }

    reply.build()
}
