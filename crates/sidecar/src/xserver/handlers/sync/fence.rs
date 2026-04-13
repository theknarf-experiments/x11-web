//! SYNC fence operations: CreateFence, TriggerFence, ResetFence,
//! DestroyFence, QueryFence, AwaitFence.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::{BAD_MATCH, BAD_VALUE};
use super::{check_pending_fence_awaits_ext, FenceState, PendingFenceAwait};
use crate::xserver::core::require_len;

/// Minor opcode 14: CreateFence
pub(crate) fn create_fence(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    // bytes 4-7 = drawable, 8-11 = fence_id, 12 = initially_triggered
    if data.len() >= 13 {
        let _drawable = state.read_u32(data, 4);
        let fence_id = state.read_u32(data, 8);
        let initially_triggered = data[12] != 0;
        debug!("SYNC CreateFence: id={fence_id:#x} initially_triggered={initially_triggered}");
        state.sync_state.fences.insert(
            fence_id,
            FenceState {
                id: fence_id,
                triggered: initially_triggered,
                initially_triggered,
                fd: -1,
            },
        );
    }
    Vec::new()
}

/// Minor opcode 15: TriggerFence
pub(crate) fn trigger_fence(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    // bytes 4-7 = fence_id
    if data.len() >= 8 {
        let fence_id = state.read_u32(data, 4);
        debug!("SYNC TriggerFence: id={fence_id:#x}");
        if let Some(fence) = state.sync_state.fences.get_mut(&fence_id) {
            fence.triggered = true;
        } else {
            warn!("SYNC TriggerFence: unknown fence {fence_id:#x}");
        }
        // Check if any pending AwaitFence is now satisfied
        check_pending_fence_awaits_ext(&mut state.sync_state);
    }
    Vec::new()
}

/// Minor opcode 16: ResetFence
pub(crate) fn reset_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Per spec, fence must be triggered to reset; return BadMatch otherwise.
    if data.len() >= 8 {
        let fence_id = state.read_u32(data, 4);
        debug!("SYNC ResetFence: id={fence_id:#x}");
        if let Some(fence) = state.sync_state.fences.get_mut(&fence_id) {
            if !fence.triggered {
                return super::super::super::core::build_error_bo(
                    BAD_MATCH,
                    seq,
                    fence_id,
                    134,
                    16,
                    state.msb_first,
                );
            }
            fence.triggered = false;
        } else {
            return super::super::super::core::build_error_bo(
                BAD_VALUE,
                seq,
                fence_id,
                134,
                16,
                state.msb_first,
            );
        }
    }
    Vec::new()
}

/// Minor opcode 17: DestroyFence
pub(crate) fn destroy_fence(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let fence_id = state.read_u32(data, 4);
        debug!("SYNC DestroyFence: id={fence_id:#x}");
        state.recycle_xid(fence_id);
        if let Some(fence) = state.sync_state.fences.remove(&fence_id) {
            if fence.fd >= 0 {
                unsafe {
                    libc::close(fence.fd);
                }
            }
        }
        // Cancel any pending AwaitFence requests that reference this fence.
        // Per X11 SYNC spec, destroying a fence while an AwaitFence references it
        // should unblock the client.
        let had_pending = !state.sync_state.pending_fence_awaits.is_empty();
        state.sync_state.pending_fence_awaits.retain(|pfa| {
            let references_destroyed = pfa.fence_ids.contains(&fence_id);
            if references_destroyed {
                debug!(
                    "SYNC DestroyFence: cancelling pending AwaitFence (seq={})",
                    pfa.seq
                );
            }
            !references_destroyed
        });
        if had_pending
            && state.sync_state.pending_fence_awaits.is_empty()
            && state.sync_state.pending_awaits.is_empty()
        {
            state.sync_state.blocked = false;
        }
    }
    Vec::new()
}

/// Minor opcode 18: QueryFence
pub(crate) fn query_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 134, data[1] as u16, state.msb_first);
    let fence_id = state.read_u32(data, 4);
    let triggered = state
        .sync_state
        .fences
        .get(&fence_id)
        .map(|f| f.triggered)
        .unwrap_or(true);
    debug!("SYNC QueryFence: id={fence_id:#x} triggered={triggered}");

    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    reply[8] = if triggered { 1 } else { 0 };
    reply.to_vec()
}

/// Minor opcode 19: AwaitFence
pub(crate) fn await_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Block until at least one fence is triggered.
    // Parse fence IDs from the request body (each 4 bytes, starting at offset 4).
    let n_fences = data.len().saturating_sub(4) / 4;
    let mut fence_ids = Vec::with_capacity(n_fences);
    let mut any_triggered = false;
    let mut offset = 4;
    for _ in 0..n_fences {
        if offset + 4 > data.len() {
            break;
        }
        let fence_id = state.read_u32(data, offset);
        if state
            .sync_state
            .fences
            .get(&fence_id)
            .map(|f| f.triggered)
            .unwrap_or(false)
        {
            any_triggered = true;
        }
        fence_ids.push(fence_id);
        offset += 4;
    }

    if any_triggered || fence_ids.is_empty() {
        debug!(
            "SYNC AwaitFence: satisfied immediately ({} fences)",
            fence_ids.len()
        );
    } else {
        debug!(
            "SYNC AwaitFence: {} fences not yet triggered, blocking connection",
            fence_ids.len()
        );
        state
            .sync_state
            .pending_fence_awaits
            .push(PendingFenceAwait { fence_ids, seq });
        state.sync_state.blocked = true;
    }
    Vec::new()
}
