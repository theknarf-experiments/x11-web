//! SYNC Await and priority operations: Await (7), SetPriority (12), GetPriority (13).

use super::super::parse_minor;
use tracing::debug;

use super::super::super::client::ClientState;
use super::{is_trigger_satisfied, AwaitTrigger, PendingAwait};
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::sync::{
    AwaitRequest, GetPriorityReply, GetPriorityRequest, SetPriorityRequest,
};

/// Minor opcode 7: Await
pub(crate) fn await_op(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(AwaitRequest, data, state, seq, 134, 7);

    // Await: wait for one or more counter conditions.
    // Per X11 SYNC spec, Await blocks the connection until at least one
    // trigger is satisfied. We check immediately; if none are satisfied,
    // we store the request and set the blocked flag so the request
    // processing loop stops until a counter update satisfies a trigger.

    let mut any_satisfied = false;
    let mut triggers = Vec::with_capacity(req.wait_list.len());
    for wc in req.wait_list.iter() {
        let counter_id = wc.trigger.counter;
        let value_type = u32::from(wc.trigger.wait_type);
        let wait_value =
            ((wc.trigger.wait_value.hi as i64) << 32) | (wc.trigger.wait_value.lo as i64);
        let test_type = u32::from(wc.trigger.test_type);
        let event_threshold =
            ((wc.event_threshold.hi as i64) << 32) | (wc.event_threshold.lo as i64);

        // Get current counter value
        let current = if counter_id == 1 {
            state.timestamp() as i64
        } else {
            state
                .sync_state
                .counters
                .get(&counter_id)
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
    }

    if any_satisfied || triggers.is_empty() {
        debug!(
            "SYNC Await: satisfied immediately ({} triggers)",
            triggers.len()
        );
    } else {
        debug!(
            "SYNC Await: {} triggers not yet satisfied, blocking connection",
            triggers.len()
        );
        state
            .sync_state
            .pending_awaits
            .push(PendingAwait { triggers, seq });
        state.sync_state.blocked = true;
    }
    Vec::new()
}

/// Minor opcode 12: SetPriority
pub(crate) fn set_priority(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(SetPriorityRequest, data, state, seq, 134, 12);
    let resource_id = req.id;
    let priority = req.priority;
    debug!("SYNC SetPriority: resource={resource_id:#x} priority={priority}");
    state.sync_state.priorities.insert(resource_id, priority);
    Vec::new()
}

/// Minor opcode 13: GetPriority
pub(crate) fn get_priority(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(GetPriorityRequest, data, state, seq, 134, 13);
    let resource_id = req.id;
    let priority = state
        .sync_state
        .priorities
        .get(&resource_id)
        .copied()
        .unwrap_or(0);
    debug!("SYNC GetPriority: resource={resource_id:#x} priority={priority}");
    serialize_reply(
        &GetPriorityReply {
            sequence: seq,
            length: 0,
            priority,
        },
        state.byte_order(),
    )
}
