//! XFIXES barrier and misc operations.

use super::super::parse_minor;
use tracing::debug;

use super::super::super::client::ClientState;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::xfixes::{
    ClientDisconnectFlags, CreatePointerBarrierRequest, DeletePointerBarrierRequest,
    GetClientDisconnectModeReply, GetClientDisconnectModeRequest, SetClientDisconnectModeRequest,
};

/// 31: CreatePointerBarrier
pub(crate) fn handle_create_pointer_barrier(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(CreatePointerBarrierRequest, data, state, seq, 138, 31);
    let barrier_id = req.barrier;
    let window = req.window;
    // Per XFIXES spec: validate window exists
    if window != state.root_window && !state.windows.contains_key(&window) {
        return crate::xserver::core::build_error(
            crate::xserver::core::WINDOW_ERROR,
            seq,
            window,
            138,
            31, // XFIXES major opcode = 138
        );
    }
    let x1 = req.x1 as i16;
    let y1 = req.y1 as i16;
    let x2 = req.x2 as i16;
    let y2 = req.y2 as i16;
    // Per XFIXES spec: valid direction bits are 0-3 (PositiveX, PositiveY, NegativeX, NegativeY)
    // Silently mask invalid bits to prevent client errors while remaining compliant.
    let directions = req.directions.bits() & 0xF;
    let device_ids: Vec<u16> = req.devices.iter().copied().collect();
    let num_devices = device_ids.len();
    debug!("XFIXES CreatePointerBarrier: id={barrier_id:#x} window={window:#x} ({x1},{y1})-({x2},{y2}) dirs={directions:#x} devices={num_devices}");
    state.xfixes.barriers.insert(
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
    Vec::new()
}

/// 32: DeletePointerBarrier
pub(crate) fn handle_delete_pointer_barrier(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(DeletePointerBarrierRequest, data, state, seq, 138, 32);
    let barrier_id = req.barrier;
    debug!("XFIXES DeletePointerBarrier: id={barrier_id:#x}");
    state.xfixes.barriers.remove(&barrier_id);
    state.recycle_xid(barrier_id);
    Vec::new()
}

/// 33: SetClientDisconnectMode
///
/// Per XFIXES 6+ spec, valid modes: 0 = Default, 1 = ForceDisconnect.
/// Mask to valid bits for forward compatibility.
pub(crate) fn handle_set_client_disconnect_mode(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let req = parse_minor!(SetClientDisconnectModeRequest, data, state, seq, 138, 33);
    let mode = req.disconnect_mode.bits() & 0x1; // Only bit 0 is defined
    debug!("XFIXES SetClientDisconnectMode: mode={mode:#x}");
    state.xfixes.disconnect_mode = mode;
    Vec::new()
}

/// 34: GetClientDisconnectMode
pub(crate) fn handle_get_client_disconnect_mode(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let _req = parse_minor!(GetClientDisconnectModeRequest, data, state, seq, 138, 34);
    serialize_reply(
        &GetClientDisconnectModeReply {
            sequence: seq,
            length: 0,
            disconnect_mode: ClientDisconnectFlags::from(state.xfixes.disconnect_mode),
        },
        state.byte_order(),
    )
}
