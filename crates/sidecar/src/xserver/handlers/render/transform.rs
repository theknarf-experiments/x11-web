use tracing::debug;

use super::super::parse_minor;
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::{Fixed, SetPictureTransformRequest};

/// Convert a 16.16 fixed-point i32 (x11rb `Fixed`) to f64.
fn fixed_to_f64(f: Fixed) -> f64 {
    f as f64 / 65536.0
}

/// SetPictureTransform (RENDER minor opcode 28).
///
/// The transform maps *destination* coordinates to *source*
/// coordinates: `(sx*sw, sy*sw, sw) = T * (dx, dy, 1)`. Used by
/// rendercheck (and Cairo) to project a small gradient over a much
/// larger destination region.
pub(crate) fn handle_set_picture_transform(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(SetPictureTransformRequest, data, state, seq, 139, data[1] as u16);

    let pid = req.picture;
    let t = &req.transform;
    let tx = [
        fixed_to_f64(t.matrix11),
        fixed_to_f64(t.matrix12),
        fixed_to_f64(t.matrix13),
        fixed_to_f64(t.matrix21),
        fixed_to_f64(t.matrix22),
        fixed_to_f64(t.matrix23),
        fixed_to_f64(t.matrix31),
        fixed_to_f64(t.matrix32),
        fixed_to_f64(t.matrix33),
    ];

    debug!(
        "SetPictureTransform: pid={pid:#x} m=[[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}]]",
        tx[0], tx[1], tx[2], tx[3], tx[4], tx[5], tx[6], tx[7], tx[8]
    );

    // Identity matrix is the most common "reset" -- drop the entry
    // so the lookup short-circuits to the no-op fast path.
    let is_identity = (tx[0] - 1.0).abs() < 1e-9
        && tx[1].abs() < 1e-9
        && tx[2].abs() < 1e-9
        && tx[3].abs() < 1e-9
        && (tx[4] - 1.0).abs() < 1e-9
        && tx[5].abs() < 1e-9
        && tx[6].abs() < 1e-9
        && tx[7].abs() < 1e-9
        && (tx[8] - 1.0).abs() < 1e-9;
    if is_identity {
        state.render.transforms.remove(&pid);
    } else {
        state.render.transforms.insert(pid, tx);
    }
    Vec::new()
}

/// Apply a row-major 3x3 transform to a point. Returns
/// `(sx/sw, sy/sw)` per the X RENDER spec.
pub(crate) fn apply_transform(tx: &[f64; 9], px: f64, py: f64) -> (f64, f64) {
    let sx = tx[0] * px + tx[1] * py + tx[2];
    let sy = tx[3] * px + tx[4] * py + tx[5];
    let sw = tx[6] * px + tx[7] * py + tx[8];
    if sw.abs() < 1e-9 {
        (sx, sy)
    } else {
        (sx / sw, sy / sw)
    }
}
