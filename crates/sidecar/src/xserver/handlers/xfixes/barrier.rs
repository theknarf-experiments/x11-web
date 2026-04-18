//! XFIXES barrier and misc operations.

use tracing::debug;

use super::super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;

/// 31: CreatePointerBarrier
pub(crate) fn handle_create_pointer_barrier(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 28 {
        let barrier_id = state.read_u32(data, 4);
        let window = state.read_u32(data, 8);
        // Per XFIXES spec: validate window exists
        if window != state.root_window && !state.windows.contains_key(&window) {
            return crate::xserver::core::build_error(
                crate::xserver::core::BAD_WINDOW,
                _seq,
                window,
                138,
                31, // XFIXES major opcode = 138
            );
        }
        let x1 = state.read_i16(data, 12);
        let y1 = state.read_i16(data, 14);
        let x2 = state.read_i16(data, 16);
        let y2 = state.read_i16(data, 18);
        let directions = state.read_u32(data, 20);
        // Per XFIXES spec: valid direction bits are 0-3 (PositiveX, PositiveY, NegativeX, NegativeY)
        // Silently mask invalid bits to prevent client errors while remaining compliant.
        let directions = directions & 0xF;
        let num_devices = state.read_u16(data, 24) as usize;
        let mut device_ids = Vec::with_capacity(num_devices);
        for i in 0..num_devices {
            let off = 28 + i * 2;
            if off + 2 <= data.len() {
                device_ids.push(state.read_u16(data, off));
            }
        }
        debug!("XFIXES CreatePointerBarrier: id={barrier_id:#x} window={window:#x} ({x1},{y1})-({x2},{y2}) dirs={directions:#x} devices={num_devices}");
        state.barriers.insert(
            barrier_id,
            super::super::super::types::PointerBarrier {
                barrier_id,
                window,
                x1,
                y1,
                x2,
                y2,
                directions,
                device_ids,
            },
        );
    }
    Vec::new()
}

/// 32: DeletePointerBarrier
pub(crate) fn handle_delete_pointer_barrier(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 8 {
        let barrier_id = state.read_u32(data, 4);
        debug!("XFIXES DeletePointerBarrier: id={barrier_id:#x}");
        state.barriers.remove(&barrier_id);
        state.recycle_xid(barrier_id);
    }
    Vec::new()
}

/// 33: SetClientDisconnectMode
///
/// Per XFIXES 6+ spec, valid modes: 0 = Default, 1 = ForceDisconnect.
/// Mask to valid bits for forward compatibility.
pub(crate) fn handle_set_client_disconnect_mode(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 8 {
        let mode = state.read_u32(data, 4) & 0x1; // Only bit 0 is defined
        debug!("XFIXES SetClientDisconnectMode: mode={mode:#x}");
        state.disconnect_mode = mode;
    }
    Vec::new()
}

/// 34: GetClientDisconnectMode
pub(crate) fn handle_get_client_disconnect_mode(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u32(8, state.disconnect_mode)
        .build()
}
