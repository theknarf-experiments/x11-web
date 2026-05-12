//! CRTC-level RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::randr::{
    GetCrtcGammaReply, GetCrtcGammaRequest, GetCrtcGammaSizeReply, GetCrtcGammaSizeRequest,
    GetCrtcInfoReply, GetCrtcInfoRequest, GetCrtcTransformReply, GetCrtcTransformRequest,
    GetPanningReply, GetPanningRequest, Rotation, SetCrtcConfigReply, SetCrtcConfigRequest,
    SetCrtcGammaRequest, SetCrtcTransformRequest, SetPanningReply, SetPanningRequest, SetConfig,
};
use x11rb_protocol::protocol::render::Transform;

/// 16.16 fixed-point representation of `1.0` — used for identity-matrix entries.
const FIXED_16_16_ONE: i32 = 65536;

fn identity_transform() -> Transform {
    Transform {
        matrix11: FIXED_16_16_ONE,
        matrix12: 0,
        matrix13: 0,
        matrix21: 0,
        matrix22: FIXED_16_16_ONE,
        matrix23: 0,
        matrix31: 0,
        matrix32: 0,
        matrix33: FIXED_16_16_ONE,
    }
}

fn matrix_to_transform(matrix: [i32; 9]) -> Transform {
    Transform {
        matrix11: matrix[0],
        matrix12: matrix[1],
        matrix13: matrix[2],
        matrix21: matrix[3],
        matrix22: matrix[4],
        matrix23: matrix[5],
        matrix31: matrix[6],
        matrix32: matrix[7],
        matrix33: matrix[8],
    }
}

/// RRGetCrtcInfo (20).
pub(crate) fn handle_get_crtc_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = GetCrtcInfoRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.crtc)
        .unwrap_or(0);
    build_crtc_info_reply(state, seq, crtc_id)
}

/// RRSetCrtcConfig (21).
pub(crate) fn handle_set_crtc_config(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let Ok(req) = SetCrtcConfigRequest::try_parse_request(request_header(data), &data[4..]) else {
        return serialize_reply(
            &SetCrtcConfigReply {
                status: SetConfig::INVALID_CONFIG_TIME,
                sequence: seq,
                length: 0,
                timestamp: 0,
            },
            state.byte_order(),
        );
    };

    let crtc_id = req.crtc;
    let _timestamp = req.timestamp;
    let _config_timestamp = req.config_timestamp;
    let x = req.x;
    let y = req.y;
    let mode_id = req.mode;
    let rotation = u16::from(req.rotation);

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

    serialize_reply(
        &SetCrtcConfigReply {
            status: SetConfig::SUCCESS,
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
        },
        state.byte_order(),
    )
}

/// RRGetCrtcGammaSize (22).
pub(crate) fn handle_get_crtc_gamma_size(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let crtc_id = GetCrtcGammaSizeRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.crtc)
        .unwrap_or(0);
    let size: u16 = if state.randr_find_crtc(crtc_id).is_some() {
        256
    } else {
        0
    };
    serialize_reply(
        &GetCrtcGammaSizeReply {
            sequence: seq,
            length: 0,
            size,
        },
        state.byte_order(),
    )
}

/// RRGetCrtcGamma (23).
pub(crate) fn handle_get_crtc_gamma(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = GetCrtcGammaRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.crtc)
        .unwrap_or(0);
    build_get_crtc_gamma_reply(state, seq, crtc_id)
}

/// RRSetCrtcGamma (24).
pub(crate) fn handle_set_crtc_gamma(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if let Ok(req) = SetCrtcGammaRequest::try_parse_request(request_header(data), &data[4..]) {
        if let Some(crtc) = state.randr_find_crtc_mut(req.crtc) {
            crtc.gamma_red = req.red.to_vec();
            crtc.gamma_green = req.green.to_vec();
            crtc.gamma_blue = req.blue.to_vec();
            debug!("RRSetCrtcGamma crtc={} size={}", req.crtc, req.red.len());
        }
    }
    Vec::new()
}

/// RRSetCrtcTransform (26).
pub(crate) fn handle_set_crtc_transform(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if let Ok(req) = SetCrtcTransformRequest::try_parse_request(request_header(data), &data[4..]) {
        let crtc_id = req.crtc;
        let t = &req.transform;
        let matrix = [
            t.matrix11, t.matrix12, t.matrix13, t.matrix21, t.matrix22, t.matrix23, t.matrix31,
            t.matrix32, t.matrix33,
        ];
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
pub(crate) fn handle_get_panning(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let _req = GetPanningRequest::try_parse_request(request_header(data), &data[4..]);
    serialize_reply(
        &GetPanningReply {
            status: SetConfig::SUCCESS,
            sequence: seq,
            length: 0,
            timestamp: 0,
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            track_left: 0,
            track_top: 0,
            track_width: 0,
            track_height: 0,
            border_left: 0,
            border_top: 0,
            border_right: 0,
            border_bottom: 0,
        },
        state.byte_order(),
    )
}

/// RRSetPanning (28).
pub(crate) fn handle_set_panning(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = SetPanningRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.crtc)
        .unwrap_or(0);
    debug!("RRSetPanning crtc={crtc_id} -> Success");
    serialize_reply(
        &SetPanningReply {
            status: SetConfig::SUCCESS,
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
        },
        state.byte_order(),
    )
}

/// RRGetCrtcTransform (29).
pub(crate) fn handle_get_crtc_transform(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let crtc_id = GetCrtcTransformRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.crtc)
        .unwrap_or(0);
    let transform = state
        .randr_find_crtc(crtc_id)
        .map(|c| matrix_to_transform(c.transform))
        .unwrap_or_else(identity_transform);

    serialize_var_reply(
        &GetCrtcTransformReply {
            sequence: seq,
            length: 0,
            pending_transform: transform,
            has_transforms: false,
            current_transform: transform,
            pending_filter_name: Vec::new(),
            pending_params: Vec::new(),
            current_filter_name: Vec::new(),
            current_params: Vec::new(),
        },
        state.byte_order(),
    )
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Build the reply for RRGetCrtcInfo.
fn build_crtc_info_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let crtc = match state.randr_find_crtc(crtc_id) {
        Some(c) => c.clone(),
        None => {
            return serialize_var_reply(
                &GetCrtcInfoReply {
                    status: SetConfig::INVALID_CONFIG_TIME,
                    sequence: seq,
                    length: 0,
                    timestamp: 0,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    mode: 0,
                    rotation: Rotation::from(0u16),
                    rotations: Rotation::from(0u16),
                    outputs: Vec::new(),
                    possible: Vec::new(),
                },
                state.byte_order(),
            );
        }
    };

    // Possible outputs = all outputs (in our model every output can go to any CRTC)
    let possible: Vec<u32> = state.randr_outputs.iter().map(|o| o.id).collect();

    serialize_var_reply(
        &GetCrtcInfoReply {
            status: SetConfig::SUCCESS,
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            x: crtc.x,
            y: crtc.y,
            width: crtc.width,
            height: crtc.height,
            mode: crtc.mode_id,
            rotation: Rotation::from(crtc.rotation),
            rotations: Rotation::from(1u16), // Rotate_0
            outputs: crtc.outputs.clone(),
            possible,
        },
        state.byte_order(),
    )
}

/// Build the reply for RRGetCrtcGamma.
fn build_get_crtc_gamma_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let (red, green, blue) = match state.randr_find_crtc(crtc_id) {
        Some(c) => (c.gamma_red.clone(), c.gamma_green.clone(), c.gamma_blue.clone()),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };

    serialize_var_reply(
        &GetCrtcGammaReply {
            sequence: seq,
            length: 0,
            red,
            green,
            blue,
        },
        state.byte_order(),
    )
}
