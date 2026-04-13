//! Screen-level RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;

/// RRSelectInput (4) — select RandR events.
pub(crate) fn handle_select_input(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let _window = state.read_u32(data, 4);
        let enable = state.read_u16(data, 8);
        state.randr_event_mask = enable as u32;
        debug!("RRSelectInput mask=0x{:04x}", enable);
    }
    Vec::new()
}

/// RRGetScreenInfo (5) — legacy screen configuration.
pub(crate) fn handle_get_screen_info(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let num_sizes: u16 = 1;
    let extra_data_len: usize = 8;
    let reply_len = 32 + extra_data_len;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[1] = 1; // rotations = Rotate_0
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (extra_data_len / 4) as u32);
    state.write_u32(&mut reply, 8, state.root_window);
    state.write_u32(&mut reply, 12, state.timestamp());
    state.write_u32(&mut reply, 16, state.randr_config_timestamp);
    state.write_u16(&mut reply, 20, num_sizes);
    state.write_u16(&mut reply, 22, 0); // sizeID
    state.write_u16(&mut reply, 24, 1); // rotation = Rotate_0
    state.write_u16(&mut reply, 26, 0); // nrateEnts
    state.write_u16(&mut reply, 32, state.screen_width);
    state.write_u16(&mut reply, 34, state.screen_height);
    state.write_u16(&mut reply, 36, 270);
    state.write_u16(&mut reply, 38, 203);
    reply
}

/// RRGetScreenSizeRange (6).
pub(crate) fn handle_get_screen_size_range(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u16(&mut reply, 8, 1);     // min_width
    state.write_u16(&mut reply, 10, 1);    // min_height
    state.write_u16(&mut reply, 12, 32767); // max_width
    state.write_u16(&mut reply, 14, 32767); // max_height
    reply.to_vec()
}

/// RRSetScreenSize (7).
pub(crate) fn handle_set_screen_size(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let new_w = state.read_u16(data, 4);
        let new_h = state.read_u16(data, 6);
        if new_w > 0 && new_h > 0 {
            info!("RandR SetScreenSize: {}x{} -> {}x{}", state.screen_width, state.screen_height, new_w, new_h);
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
    let var_len = num_crtcs * 4 + num_outputs * 4 + num_modes * 32 + names_len + names_pad;
    let length_field = var_len / 4;
    let total = 32 + var_len;

    let mut r = vec![0u8; total];
    r[0] = 1; // Reply
    state.write_u16(&mut r, 2, seq);
    state.write_u32(&mut r, 4, length_field as u32);
    state.write_u32(&mut r, 8, state.timestamp());
    state.write_u32(&mut r, 12, state.randr_config_timestamp);
    state.write_u16(&mut r, 16, num_crtcs as u16);
    state.write_u16(&mut r, 18, num_outputs as u16);
    state.write_u16(&mut r, 20, num_modes as u16);
    state.write_u16(&mut r, 22, names_len as u16);

    let mut off = 32;

    // CRTC IDs
    for crtc in &state.randr_crtcs {
        state.write_u32(&mut r, off, crtc.id);
        off += 4;
    }

    // Output IDs
    for output in &state.randr_outputs {
        state.write_u32(&mut r, off, output.id);
        off += 4;
    }

    // ModeInfo structs (32 bytes each)
    for mode in &state.randr_modes {
        state.write_u32(&mut r, off, mode.id);
        state.write_u16(&mut r, off + 4, mode.width);
        state.write_u16(&mut r, off + 6, mode.height);
        state.write_u32(&mut r, off + 8, mode.dot_clock);
        state.write_u16(&mut r, off + 12, mode.h_sync_start);
        state.write_u16(&mut r, off + 14, mode.h_sync_end);
        state.write_u16(&mut r, off + 16, mode.h_total);
        // hSkew at off+18 = 0
        state.write_u16(&mut r, off + 20, mode.v_sync_start);
        state.write_u16(&mut r, off + 22, mode.v_sync_end);
        state.write_u16(&mut r, off + 24, mode.v_total);
        state.write_u16(&mut r, off + 26, mode.name.len() as u16);
        state.write_u32(&mut r, off + 28, mode.flags);
        off += 32;
    }

    // Mode names (concatenated)
    r[off..off + names_len].copy_from_slice(&names_bytes);

    r
}

/// Build the reply for RRGetMonitors (42).
/// Includes both automatic monitors derived from CRTCs and user-defined
/// monitors set via RRSetMonitor.
pub(crate) fn build_get_monitors_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    // Collect automatic monitors from active CRTCs.
    let active_crtcs: Vec<_> = state.randr_crtcs.iter().filter(|c| c.mode_id != 0).collect();
    let n_auto = active_crtcs.len();
    let n_user = state.randr_monitors.len();
    let n_monitors = n_auto + n_user;
    let total_outputs: usize = active_crtcs.iter().map(|c| c.outputs.len()).sum::<usize>()
        + state.randr_monitors.iter().map(|m| m.output_ids.len()).sum::<usize>();

    // MonitorInfo = 24 bytes + nOutput * 4
    let mut monitor_data_len = 0usize;
    for c in &active_crtcs {
        monitor_data_len += 24 + c.outputs.len() * 4;
    }
    for m in &state.randr_monitors {
        monitor_data_len += 24 + m.output_ids.len() * 4;
    }
    let pad = (4 - (monitor_data_len % 4)) % 4;
    let length_field = (monitor_data_len + pad) / 4;
    let total = 32 + monitor_data_len + pad;

    let mut r = vec![0u8; total];
    r[0] = 1;
    state.write_u16(&mut r, 2, seq);
    state.write_u32(&mut r, 4, length_field as u32);
    state.write_u32(&mut r, 8, state.timestamp());
    state.write_u32(&mut r, 12, n_monitors as u32);
    state.write_u32(&mut r, 16, total_outputs as u32);

    let mut off = 32;

    // Emit automatic monitors from CRTCs.
    for (i, crtc) in active_crtcs.iter().enumerate() {
        let name_str = if i == 0 { "default".to_string() } else { format!("monitor-{}", i) };
        let monitor_name = state.intern_atom(&name_str, false);

        state.write_u32(&mut r, off, monitor_name);
        off += 4;
        r[off] = if i == 0 { 1 } else { 0 }; // primary
        off += 1;
        r[off] = 1; // automatic
        off += 1;
        state.write_u16(&mut r, off, crtc.outputs.len() as u16);
        off += 2;
        state.write_i16(&mut r, off, crtc.x);
        off += 2;
        state.write_i16(&mut r, off, crtc.y);
        off += 2;
        state.write_u16(&mut r, off, crtc.width);
        off += 2;
        state.write_u16(&mut r, off, crtc.height);
        off += 2;
        let (mm_w, mm_h) = crtc
            .outputs
            .first()
            .and_then(|&oid| state.randr_find_output(oid))
            .map(|o| (o.mm_width, o.mm_height))
            .unwrap_or((270, 203));
        state.write_u32(&mut r, off, mm_w);
        off += 4;
        state.write_u32(&mut r, off, mm_h);
        off += 4;
        for &oid in &crtc.outputs {
            state.write_u32(&mut r, off, oid);
            off += 4;
        }
    }

    // Emit user-defined monitors.
    for m in &state.randr_monitors {
        state.write_u32(&mut r, off, m.name_atom);
        off += 4;
        r[off] = m.primary as u8;
        off += 1;
        r[off] = m.automatic as u8;
        off += 1;
        state.write_u16(&mut r, off, m.output_ids.len() as u16);
        off += 2;
        state.write_i16(&mut r, off, m.x);
        off += 2;
        state.write_i16(&mut r, off, m.y);
        off += 2;
        state.write_u16(&mut r, off, m.width);
        off += 2;
        state.write_u16(&mut r, off, m.height);
        off += 2;
        // mm dimensions: approximate from pixel size (96 DPI).
        let mm_w = (m.width as u32 * 254 + 480) / 960;
        let mm_h = (m.height as u32 * 254 + 480) / 960;
        state.write_u32(&mut r, off, mm_w);
        off += 4;
        state.write_u32(&mut r, off, mm_h);
        off += 4;
        for &oid in &m.output_ids {
            state.write_u32(&mut r, off, oid);
            off += 4;
        }
    }

    r
}
