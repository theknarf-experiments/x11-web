//! SYNC Await and priority operations: Await (7), SetPriority (12), GetPriority (13).

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::super::core::BAD_LENGTH;
use super::{is_trigger_satisfied, AwaitTrigger, PendingAwait};

/// Minor opcode 7: Await
pub(crate) fn await_op(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Await: wait for one or more counter conditions.
    // Each wait condition is 28 bytes:
    //   counter(4) + value_type(4) + value_hi(4) + value_lo(4) +
    //   test_type(4) + event_threshold_hi(4) + event_threshold_lo(4)
    //
    // Per X11 SYNC spec, Await blocks the connection until at least one
    // trigger is satisfied. We check immediately; if none are satisfied,
    // we store the request and set the blocked flag so the request
    // processing loop stops until a counter update satisfies a trigger.

    // Parse the total request length to derive the number of triggers.
    // The request is: 1 byte opcode + 1 byte minor + 2 bytes length.
    // The triggers start at offset 4, each 28 bytes.
    let triggers_data_len = data.len().saturating_sub(4);
    let n_triggers = triggers_data_len / 28;

    let mut any_satisfied = false;
    let mut triggers = Vec::with_capacity(n_triggers);
    let mut offset = 4;
    for _ in 0..n_triggers {
        if offset + 28 > data.len() { break; }
        let counter_id = state.read_u32(data, offset);
        let value_type = state.read_u32(data, offset + 4);
        let wait_value = ((state.read_u32(data, offset + 8) as i64) << 32)
            | (state.read_u32(data, offset + 12) as i64);
        let test_type = state.read_u32(data, offset + 16);
        let event_threshold = ((state.read_u32(data, offset + 20) as i64) << 32)
            | (state.read_u32(data, offset + 24) as i64);

        // Get current counter value
        let current = if counter_id == 1 {
            state.timestamp() as i64
        } else {
            state.sync_state.counters.get(&counter_id)
                .map(|c| c.value_i64())
                .unwrap_or(0)
        };

        let trigger = AwaitTrigger {
            counter_id,
            value_type,
            wait_value,
            test_type,
            event_threshold,
        };

        if is_trigger_satisfied(&trigger, current) {
            any_satisfied = true;
        }
        triggers.push(trigger);
        offset += 28;
    }

    if any_satisfied || triggers.is_empty() {
        debug!("SYNC Await: satisfied immediately ({} triggers)", triggers.len());
    } else {
        debug!("SYNC Await: {} triggers not yet satisfied, blocking connection", triggers.len());
        state.sync_state.pending_awaits.push(PendingAwait {
            triggers,
            seq,
        });
        state.sync_state.blocked = true;
    }
    Vec::new()
}

/// Minor opcode 12: SetPriority
pub(crate) fn set_priority(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let resource_id = state.read_u32(data, 4);
        let priority = state.read_u32(data, 8) as i32;
        debug!("SYNC SetPriority: resource={resource_id:#x} priority={priority}");
        state.sync_state.priorities.insert(resource_id, priority);
    }
    Vec::new()
}

/// Minor opcode 13: GetPriority
pub(crate) fn get_priority(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, data[1] as u16, state.msb_first);
    }
    let resource_id = state.read_u32(data, 4);
    let priority = state.sync_state.priorities.get(&resource_id).copied().unwrap_or(0);
    debug!("SYNC GetPriority: resource={resource_id:#x} priority={priority}");
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, priority as u32);
    reply.to_vec()
}
