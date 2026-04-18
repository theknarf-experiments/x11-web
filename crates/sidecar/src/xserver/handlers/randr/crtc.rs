//! CRTC-level RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;

/// RRGetCrtcInfo (20).
pub(crate) fn handle_get_crtc_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    build_crtc_info_reply(state, seq, crtc_id)
}

/// RRSetCrtcConfig (21).
pub(crate) fn handle_set_crtc_config(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Request layout:
    //   4: crtc (CARD32)
    //   8: timestamp (CARD32)
    //  12: config_timestamp (CARD32)
    //  16: x (INT16)
    //  18: y (INT16)
    //  20: mode (CARD32)
    //  24: rotation (CARD16)
    //  26: pad
    //  28+: output list (CARD32 each)

    if data.len() < 28 {
        return ReplyBuf::fixed(seq, state.msb_first)
            .set_data_byte(1) // InvalidConfig
            .build();
    }

    let crtc_id = state.read_u32(data, 4);
    let _timestamp = state.read_u32(data, 8);
    let _config_timestamp = state.read_u32(data, 12);
    let x = state.read_i16(data, 16);
    let y = state.read_i16(data, 18);
    let mode_id = state.read_u32(data, 20);
    let rotation = state.read_u16(data, 24);

    // Parse output list
    let _num_outputs = if data.len() > 28 {
        (data.len() - 28) / 4
    } else {
        0
    };

    // Look up mode dimensions first to avoid borrow conflict.
    let mode_dims = if mode_id == 0 {
        Some((0u16, 0u16))
    } else {
        state.randr_find_mode(mode_id).map(|m| (m.width, m.height))
    };

    let found = if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
        info!(
            "RRSetCrtcConfig crtc={} mode={} pos=({},{}) rot={}",
            crtc_id, mode_id, x, y, rotation
        );

        crtc.x = x;
        crtc.y = y;
        crtc.mode_id = mode_id;
        crtc.rotation = rotation;

        if let Some((w, h)) = mode_dims {
            crtc.width = w;
            crtc.height = h;
        }
        true
    } else {
        false
    };

    if found {
        state.randr_config_timestamp += 1;
        state.randr_queue_crtc_change_notify(crtc_id);
        state.randr_queue_screen_change_notify();
    }

    let ts = state.timestamp();
    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(0) // Success
        .set_u32(8, ts)
        .build()
}

/// RRGetCrtcGammaSize (22).
pub(crate) fn handle_get_crtc_gamma_size(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let crtc_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    let size: u16 = if state.randr_find_crtc(crtc_id).is_some() {
        256
    } else {
        0
    };
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, size)
        .build()
}

/// RRGetCrtcGamma (23).
pub(crate) fn handle_get_crtc_gamma(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    build_get_crtc_gamma_reply(state, seq, crtc_id)
}

/// RRSetCrtcGamma (24).
pub(crate) fn handle_set_crtc_gamma(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    set_crtc_gamma(state, data);
    Vec::new()
}

/// RRSetCrtcTransform (26).
pub(crate) fn handle_set_crtc_transform(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 44 {
        let crtc_id = state.read_u32(data, 4);
        // Read the 3x3 fixed-point matrix.  state.read_u32 handles
        // byte-order; we reinterpret the bits as i32 for storage.
        let mut matrix = [0i32; 9];
        for (i, slot) in matrix.iter_mut().enumerate() {
            *slot = state.read_u32(data, 8 + i * 4) as i32;
        }
        if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
            crtc.transform = matrix;
            debug!("RRSetCrtcTransform crtc={crtc_id} transform stored");
        } else {
            debug!("RRSetCrtcTransform crtc={crtc_id} (unknown crtc, ignoring)");
        }
    } else {
        debug!(
            "RRSetCrtcTransform: request too short ({}B), ignoring",
            data.len()
        );
    }
    Vec::new()
}

/// RRGetPanning (27).
pub(crate) fn handle_get_panning(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(0) // Success
        .build()
}

/// RRSetPanning (28).
pub(crate) fn handle_set_panning(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    debug!("RRSetPanning crtc={crtc_id} -> Success");
    ReplyBuf::fixed(seq, state.msb_first)
        .set_data_byte(0) // Success
        .set_u32(8, state.timestamp())
        .build()
}

/// RRGetCrtcTransform (29).
pub(crate) fn handle_get_crtc_transform(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    // Retrieve transform or fall back to identity.
    let identity = [65536i32, 0, 0, 0, 65536, 0, 0, 0, 65536];
    let transform = state
        .randr_find_crtc(crtc_id)
        .map(|c| c.transform)
        .unwrap_or(identity);

    // Reply: 32-byte header + 36 (pending matrix) + 2 (namelen) + 2 (pad)
    //        + 36 (current matrix) + 2 (namelen) + 2 (pad) = 32 + 80 = 112
    // But the length field counts words after the first 32 bytes: 80/4 = 20.
    let mut reply = ReplyBuf::with_extra(seq, 80, state.msb_first);

    // Write pending transform at offset 8
    for (i, &val) in transform.iter().enumerate() {
        reply = reply.set_u32(8 + i * 4, val as u32);
    }
    // pending filter name length = 0 at offset 44, padding at 46-47: already 0

    // Write current transform at offset 48
    for (i, &val) in transform.iter().enumerate() {
        reply = reply.set_u32(48 + i * 4, val as u32);
    }
    // current filter name length = 0 at offset 84, padding at 86-87: already 0

    reply.build()
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Build the reply for RRGetCrtcInfo.
fn build_crtc_info_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let crtc = match state.randr_find_crtc(crtc_id) {
        Some(c) => c.clone(),
        None => {
            return ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(1) // InvalidConfig
                .build();
        }
    };

    let num_outputs = crtc.outputs.len() as u16;
    // Possible outputs = all outputs (in our model every output can go to any CRTC)
    let num_possible = state.randr_outputs.len() as u16;
    let var_data = (num_outputs as usize + num_possible as usize) * 4;
    let inline_header = 24;
    let extra_bytes = inline_header + var_data;

    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_data_byte(0) // Success
        .set_u32(8, state.timestamp())
        .set_i16(12, crtc.x)
        .set_i16(14, crtc.y)
        .set_u16(16, crtc.width)
        .set_u16(18, crtc.height)
        .set_u32(20, crtc.mode_id)
        .set_u16(24, crtc.rotation)
        .set_u16(26, 1) // rotations supported: Rotate_0
        .set_u16(28, num_outputs)
        .set_u16(30, num_possible);

    let mut off = 32;
    // Current outputs
    for &oid in &crtc.outputs {
        reply = reply.set_u32(off, oid);
        off += 4;
    }
    // Possible outputs
    for output in &state.randr_outputs {
        reply = reply.set_u32(off, output.id);
        off += 4;
    }

    reply.build()
}

/// Build the reply for RRGetCrtcGamma.
fn build_get_crtc_gamma_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let crtc = match state.randr_find_crtc(crtc_id) {
        Some(c) => c,
        None => {
            // Empty gamma reply.
            return ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 0)
                .build();
        }
    };

    let size = crtc.gamma_red.len() as u16;
    // Each channel is `size` u16 values = size * 2 bytes.
    // Total gamma data = 3 * size * 2 bytes.
    let gamma_data_len = 3 * size as usize * 2;
    let pad = (4 - (gamma_data_len % 4)) % 4;
    let var_len = gamma_data_len + pad;

    let mut reply = ReplyBuf::with_extra(seq, var_len, state.msb_first)
        .set_u16(8, size);

    let mut off = 32;
    // Red
    for &v in &crtc.gamma_red {
        reply = reply.set_u16(off, v);
        off += 2;
    }
    // Green
    for &v in &crtc.gamma_green {
        reply = reply.set_u16(off, v);
        off += 2;
    }
    // Blue
    for &v in &crtc.gamma_blue {
        reply = reply.set_u16(off, v);
        off += 2;
    }

    reply.build()
}

/// Handle RRSetCrtcGamma: store the gamma LUT.
fn set_crtc_gamma(state: &mut ClientState, data: &[u8]) {
    if data.len() < 8 {
        return;
    }

    let crtc_id = state.read_u32(data, 4);
    let size = state.read_u16(data, 8) as usize;

    // Data starts at offset 12 (after crtc(4) + size(2) + pad(2)).
    let data_start = 12;
    let channel_bytes = size * 2;
    let needed = data_start + 3 * channel_bytes;

    if data.len() < needed || size == 0 {
        return;
    }

    // Parse gamma values before borrowing crtc mutably.
    let mut red = Vec::with_capacity(size);
    let mut green = Vec::with_capacity(size);
    let mut blue = Vec::with_capacity(size);

    for i in 0..size {
        red.push(state.read_u16(data, data_start + i * 2));
    }
    let g_off = data_start + channel_bytes;
    for i in 0..size {
        green.push(state.read_u16(data, g_off + i * 2));
    }
    let b_off = g_off + channel_bytes;
    for i in 0..size {
        blue.push(state.read_u16(data, b_off + i * 2));
    }

    if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
        crtc.gamma_red = red;
        crtc.gamma_green = green;
        crtc.gamma_blue = blue;
        debug!("RRSetCrtcGamma crtc={} size={}", crtc_id, size);
    }
}
