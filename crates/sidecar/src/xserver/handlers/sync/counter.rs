//! SYNC counter operations: ListSystemCounters, CreateCounter, SetCounter,
//! ChangeCounter, QueryCounter, DestroyCounter.

use tracing::debug;
use super::super::parse_minor;

use super::super::super::client::ClientState;
use super::super::super::core::VALUE_ERROR;
use super::{check_alarms, check_pending_awaits_ext, SyncCounter};
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::sync::{
    ChangeCounterRequest, CreateCounterRequest, DestroyCounterRequest, QueryCounterRequest,
    SetCounterRequest,
};

/// Minor opcode 1: ListSystemCounters
pub(crate) fn list_system_counters(state: &mut ClientState, seq: u16) -> Vec<u8> {
    debug!("SYNC ListSystemCounters");
    let counter_name = b"SERVERTIME";
    let name_len = counter_name.len();
    let name_pad = (4 - (name_len % 4)) % 4;
    let entry_size = 4 + 4 + 4 + 2 + name_len + name_pad;
    let extra = entry_size;
    let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
        .set_u32(8, 1u32); // num_counters = 1
    let off = 32;
    reply = reply.set_u32(off, 1u32) // counter ID = 1 (SERVERTIME)
        .set_u32(off + 4, 0u32) // resolution_hi
        .set_u32(off + 8, 1u32) // resolution_lo = 1ms
        .set_u16(off + 12, name_len as u16);
    reply.buf_mut()[off + 14..off + 14 + name_len].copy_from_slice(counter_name);
    reply.build()
}

/// Minor opcode 2: CreateCounter
pub(crate) fn create_counter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateCounterRequest, data, state, seq, 134, 2);
    let counter_id = req.id;
    let value_hi = req.initial_value.hi;
    let value_lo = req.initial_value.lo;
    debug!("SYNC CreateCounter: id={counter_id:#x} value={value_hi}:{value_lo}");
    state.sync_state.counters.insert(
        counter_id,
        SyncCounter {
            value_hi,
            value_lo,
            is_system: false,
        },
    );
    Vec::new()
}

/// Minor opcode 3: SetCounter
pub(crate) fn set_counter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SetCounterRequest, data, state, seq, 134, 3);
    let counter_id = req.counter;
    let value_hi = req.value.hi;
    let value_lo = req.value.lo;
    debug!("SYNC SetCounter: id={counter_id:#x} value={value_hi}:{value_lo}");

    let old_value = state
        .sync_state
        .counters
        .get(&counter_id)
        .map(|c| c.value_i64())
        .unwrap_or(0);

    if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
        counter.value_hi = value_hi;
        counter.value_lo = value_lo;
    }

    let new_value = ((value_hi as i64) << 32) | (value_lo as i64);
    check_alarms(
        &mut state.sync_state.alarms,
        counter_id,
        old_value,
        new_value,
        &mut state.pending_events,
        seq,
        state.msb_first,
    );
    // Check if any pending Await is now satisfied
    let ts = state.timestamp();
    check_pending_awaits_ext(&mut state.sync_state, || ts);
    Vec::new()
}

/// Minor opcode 4: ChangeCounter
pub(crate) fn change_counter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ChangeCounterRequest, data, state, seq, 134, 4);
    let counter_id = req.counter;
    let delta_hi = req.amount.hi;
    let delta_lo = req.amount.lo;
    let delta = ((delta_hi as i64) << 32) | (delta_lo as i64);
    debug!("SYNC ChangeCounter: id={counter_id:#x} delta={delta}");

    let old_value = state
        .sync_state
        .counters
        .get(&counter_id)
        .map(|c| c.value_i64())
        .unwrap_or(0);
    let new_value = old_value.wrapping_add(delta);

    if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
        counter.set_from_i64(new_value);
    }

    check_alarms(
        &mut state.sync_state.alarms,
        counter_id,
        old_value,
        new_value,
        &mut state.pending_events,
        seq,
        state.msb_first,
    );
    // Check if any pending Await is now satisfied
    let ts = state.timestamp();
    check_pending_awaits_ext(&mut state.sync_state, || ts);
    Vec::new()
}

/// Minor opcode 5: QueryCounter
pub(crate) fn query_counter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(QueryCounterRequest, data, state, seq, 134, 5);
    let counter_id = req.counter;
    debug!("SYNC QueryCounter: id={counter_id:#x}");

    let mut reply = ReplyBuf::fixed(seq, state.msb_first);

    if counter_id == 1 {
        // SERVERTIME: return current elapsed time in ms
        let ms = state.timestamp();
        reply = reply.set_u32(8, 0u32) // value_hi
            .set_u32(12, ms); // value_lo
    } else if let Some(counter) = state.sync_state.counters.get(&counter_id) {
        reply = reply.set_u32(8, counter.value_hi as u32)
            .set_u32(12, counter.value_lo);
    } else {
        // BadCounter
        return super::super::super::core::build_error_bo(
            VALUE_ERROR,
            seq,
            counter_id,
            134,
            5,
            state.msb_first,
        );
    }
    reply.build()
}

/// Minor opcode 6: DestroyCounter
pub(crate) fn destroy_counter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(DestroyCounterRequest, data, state, seq, 134, 6);
    let counter_id = req.counter;
    debug!("SYNC DestroyCounter: id={counter_id:#x}");
    state.sync_state.counters.remove(&counter_id);
    state.recycle_xid(counter_id);
    // Deactivate any alarms referencing this counter
    for alarm in state.sync_state.alarms.values_mut() {
        if alarm.counter == counter_id {
            alarm.state = 1; // Inactive
        }
    }
    // Cancel any pending Await requests that reference this counter.
    // Per X11 SYNC spec, destroying a counter while an Await references it
    // should unblock the client (the trigger can never be satisfied).
    let had_pending = !state.sync_state.pending_awaits.is_empty();
    state.sync_state.pending_awaits.retain(|pa| {
        // Remove awaits where ALL triggers reference only destroyed/missing counters,
        // or where at least one trigger references the destroyed counter.
        // Per spec: if the counter is destroyed, the trigger condition becomes
        // immediately True, so any await with a trigger on this counter is satisfied.
        let references_destroyed = pa.triggers.iter().any(|t| t.counter_id == counter_id);
        if references_destroyed {
            debug!(
                "SYNC DestroyCounter: cancelling pending Await (seq={})",
                pa.seq
            );
        }
        !references_destroyed
    });
    if had_pending
        && state.sync_state.pending_awaits.is_empty()
        && state.sync_state.pending_fence_awaits.is_empty()
    {
        state.sync_state.blocked = false;
    }
    Vec::new()
}
