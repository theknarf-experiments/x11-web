use tracing::debug;

use crate::xserver::ClientState;
use crate::xserver::core::read_u32_bo;
use super::read_fixed_bo;

/// SetPictureTransform (RENDER minor opcode 28).
///
/// Wire layout:
///
/// ```text
///   1   opcode (139)
///   1   minor (28)
///   2   length
///   4   PICTURE  picture
///   9*4 FIXED    transform (3x3 row-major matrix)
/// ```
///
/// The transform maps *destination* coordinates to *source*
/// coordinates: `(sx*sw, sy*sw, sw) = T · (dx, dy, 1)`. Used by
/// rendercheck (and Cairo) to project a small gradient over a much
/// larger destination region.
pub(crate) fn handle_set_picture_transform(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    if data.len() < 8 + 9 * 4 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, 0, 139, data[1] as u16, bo,
        );
    }
    let pid = read_u32_bo(data, 4, bo);
    let mut tx = [0f64; 9];
    for i in 0..9 {
        tx[i] = read_fixed_bo(data, 8 + i * 4, bo);
    }
    debug!(
        "SetPictureTransform: pid={pid:#x} m=[[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}],[{:.2},{:.2},{:.2}]]",
        tx[0], tx[1], tx[2], tx[3], tx[4], tx[5], tx[6], tx[7], tx[8]
    );
    // Identity matrix is the most common "reset" — drop the entry
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
