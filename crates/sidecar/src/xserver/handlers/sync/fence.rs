//! SYNC fence operations: CreateFence, TriggerFence, ResetFence,
//! DestroyFence, QueryFence, AwaitFence.

use tracing::{debug, warn};
use super::super::parse_minor;

use super::super::super::client::ClientState;
use super::super::super::core::{MATCH_ERROR, VALUE_ERROR};
use super::{check_pending_fence_awaits_ext, FenceState, PendingFenceAwait};
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::sync::{
    AwaitFenceRequest, CreateFenceRequest, DestroyFenceRequest, QueryFenceRequest,
    ResetFenceRequest, TriggerFenceRequest,
};

/// Minor opcode 14: CreateFence
pub(crate) fn create_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateFenceRequest, data, state, seq, 134, 14);
    let fence_id = req.fence;
    let initially_triggered = req.initially_triggered;
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
    Vec::new()
}

/// Minor opcode 15: TriggerFence
pub(crate) fn trigger_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(TriggerFenceRequest, data, state, seq, 134, 15);
    let fence_id = req.fence;
    debug!("SYNC TriggerFence: id={fence_id:#x}");
    if let Some(fence) = state.sync_state.fences.get_mut(&fence_id) {
        fence.triggered = true;
    } else {
        warn!("SYNC TriggerFence: unknown fence {fence_id:#x}");
    }
    // Check if any pending AwaitFence is now satisfied
    check_pending_fence_awaits_ext(&mut state.sync_state);
    Vec::new()
}

/// Minor opcode 16: ResetFence
pub(crate) fn reset_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ResetFenceRequest, data, state, seq, 134, 16);
    let fence_id = req.fence;
    debug!("SYNC ResetFence: id={fence_id:#x}");
    // Per spec, fence must be triggered to reset; return BadMatch otherwise.
    if let Some(fence) = state.sync_state.fences.get_mut(&fence_id) {
        if !fence.triggered {
            return super::super::super::core::build_error_bo(
                MATCH_ERROR,
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
            VALUE_ERROR,
            seq,
            fence_id,
            134,
            16,
            state.msb_first,
        );
    }
    Vec::new()
}

/// Minor opcode 17: DestroyFence
pub(crate) fn destroy_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(DestroyFenceRequest, data, state, seq, 134, 17);
    let fence_id = req.fence;
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
    Vec::new()
}

/// Minor opcode 18: QueryFence
pub(crate) fn query_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(QueryFenceRequest, data, state, seq, 134, 18);
    let fence_id = req.fence;
    let triggered = state
        .sync_state
        .fences
        .get(&fence_id)
        .map(|f| f.triggered)
        .unwrap_or(true);
    debug!("SYNC QueryFence: id={fence_id:#x} triggered={triggered}");

    ReplyBuf::fixed(seq, state.msb_first)
        .set_u8(8, if triggered { 1 } else { 0 })
        .build()
}

/// Minor opcode 19: AwaitFence
pub(crate) fn await_fence(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(AwaitFenceRequest, data, state, seq, 134, 19);
    // Block until at least one fence is triggered.
    let fence_ids: Vec<u32> = req.fence_list.iter().copied().collect();
    let any_triggered = fence_ids.iter().any(|&fid| {
        state
            .sync_state
            .fences
            .get(&fid)
            .map(|f| f.triggered)
            .unwrap_or(false)
    });

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
