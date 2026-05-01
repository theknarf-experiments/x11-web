//! SYNC extension handler — real counters, alarms, and fences.
//!
//! Implements the X Synchronization Extension per the spec:
//! - System counters (SERVERTIME)
//! - Client-created counters with get/set/change
//! - Alarms that trigger on counter value transitions
//! - Fences for synchronization primitives
use super::parse_minor;
use crate::xserver::reply::ReplyBuf;

mod alarm;
mod await_op;
mod counter;
mod fence;

use std::collections::HashMap;
use tracing::{debug, warn};

use super::super::client::ClientState;
use crate::xserver::event::serialize_event;
use x11rb_protocol::protocol::sync::{AlarmNotifyEvent, InitializeRequest, Int64, ALARMSTATE};

/// A SYNC counter (system or client-created).
#[derive(Clone, Debug)]
pub(crate) struct SyncCounter {
    /// Current counter value (64-bit, stored as hi/lo pair).
    pub(crate) value_hi: i32,
    pub(crate) value_lo: u32,
    /// True if this is a system counter (e.g., SERVERTIME).
    pub(crate) is_system: bool,
}

impl SyncCounter {
    pub(crate) fn value_i64(&self) -> i64 {
        ((self.value_hi as i64) << 32) | (self.value_lo as i64)
    }

    pub(crate) fn set_from_i64(&mut self, val: i64) {
        self.value_hi = (val >> 32) as i32;
        self.value_lo = val as u32;
    }
}

/// A SYNC alarm that monitors a counter.
#[derive(Clone, Debug)]
pub(crate) struct SyncAlarm {
    pub(crate) counter: u32,
    pub(crate) value_type: u8, // 0=Absolute, 1=Relative
    pub(crate) value_hi: i32,
    pub(crate) value_lo: u32,
    pub(crate) test_type: u8, // 0=PositiveTransition, 1=NegativeTransition, 2=PositiveComparison, 3=NegativeComparison
    pub(crate) delta_hi: i32,
    pub(crate) delta_lo: u32,
    pub(crate) events: bool,
    pub(crate) state: u8, // 0=Active, 1=Inactive, 2=Destroyed
}

/// A SYNC fence for synchronization.
#[derive(Clone, Debug)]
pub(crate) struct FenceState {
    /// The fence resource ID.
    pub(crate) id: u32,
    /// Whether the fence is currently triggered.
    pub(crate) triggered: bool,
    /// The initial triggered state from CreateFence.
    pub(crate) initially_triggered: bool,
    /// File descriptor backing this fence (DRI3 FenceFromFD). -1 if not fd-backed.
    pub(crate) fd: i32,
}

/// A single trigger condition for SYNC Await.
#[derive(Clone, Debug)]
pub(crate) struct AwaitTrigger {
    pub(crate) counter_id: u32,
    pub(crate) value_type: u32,
    pub(crate) wait_value: i64,
    pub(crate) test_type: u32,
    pub(crate) event_threshold: i64,
}

/// A pending SYNC Await request that hasn't been satisfied yet.
#[derive(Clone, Debug)]
pub(crate) struct PendingAwait {
    pub(crate) triggers: Vec<AwaitTrigger>,
    pub(crate) seq: u16,
}

/// A pending SYNC AwaitFence request that hasn't been satisfied yet.
#[derive(Clone, Debug)]
pub(crate) struct PendingFenceAwait {
    pub(crate) fence_ids: Vec<u32>,
    pub(crate) seq: u16,
}

/// State for the SYNC extension, stored in ClientState.
#[derive(Default)]
pub(crate) struct SyncState {
    pub(crate) counters: HashMap<u32, SyncCounter>,
    pub(crate) alarms: HashMap<u32, SyncAlarm>,
    pub(crate) fences: HashMap<u32, FenceState>,
    /// Per-resource priorities (resource_id -> priority i32).
    pub(crate) priorities: HashMap<u32, i32>,
    /// Pending Await requests: blocked until at least one trigger is satisfied.
    pub(crate) pending_awaits: Vec<PendingAwait>,
    /// Pending AwaitFence requests: blocked until at least one fence is triggered.
    pub(crate) pending_fence_awaits: Vec<PendingFenceAwait>,
    /// True when the connection is blocked on a pending Await or AwaitFence.
    /// The request processing loop should stop processing further requests.
    pub(crate) blocked: bool,
}

impl SyncState {
    pub(crate) fn new() -> Self {
        let mut s = Self::default();
        // Pre-populate SERVERTIME system counter (ID=1)
        s.counters.insert(
            1,
            SyncCounter {
                value_hi: 0,
                value_lo: 0,
                is_system: true,
            },
        );
        s
    }
}

/// Check all alarms for the given counter and generate AlarmNotify events.
/// Public wrapper for external callers (e.g., frame tick SERVERTIME updates).
pub(crate) fn check_alarms_ext(
    alarms: &mut HashMap<u32, SyncAlarm>,
    counter_id: u32,
    old_value: i64,
    new_value: i64,
    pending_events: &mut Vec<Vec<u8>>,
    seq: u16,
    msb_first: bool,
) {
    check_alarms(
        alarms,
        counter_id,
        old_value,
        new_value,
        pending_events,
        seq,
        msb_first,
    );
}

/// Check all alarms for the given counter and generate AlarmNotify events.
fn check_alarms(
    alarms: &mut HashMap<u32, SyncAlarm>,
    counter_id: u32,
    old_value: i64,
    new_value: i64,
    pending_events: &mut Vec<Vec<u8>>,
    seq: u16,
    msb_first: bool,
) {
    let triggered: Vec<u32> = alarms
        .iter()
        .filter(|(_, a)| a.counter == counter_id && a.state == 0 && a.events)
        .filter(|(_, a)| {
            let threshold = ((a.value_hi as i64) << 32) | (a.value_lo as i64);
            match a.test_type {
                0 => old_value < threshold && new_value >= threshold, // PositiveTransition
                1 => old_value > threshold && new_value <= threshold, // NegativeTransition
                2 => new_value >= threshold,                          // PositiveComparison
                3 => new_value <= threshold,                          // NegativeComparison
                _ => false,
            }
        })
        .map(|(&id, _)| id)
        .collect();

    for alarm_id in triggered {
        let Some(alarm) = alarms.get(&alarm_id) else {
            return;
        };
        let event = serialize_event(
            &AlarmNotifyEvent {
                response_type: 83,
                kind: 0,
                sequence: seq,
                alarm: alarm_id,
                counter_value: Int64 {
                    hi: (new_value >> 32) as i32,
                    lo: new_value as u32,
                },
                alarm_value: Int64 {
                    hi: alarm.value_hi,
                    lo: alarm.value_lo,
                },
                timestamp: 0,
                state: ALARMSTATE::from(alarm.state),
            },
            msb_first,
        );
        pending_events.push(event);

        // Update alarm: add delta to threshold for next trigger
        if let Some(alarm) = alarms.get_mut(&alarm_id) {
            let delta = ((alarm.delta_hi as i64) << 32) | (alarm.delta_lo as i64);
            if delta != 0 {
                let threshold = ((alarm.value_hi as i64) << 32) | (alarm.value_lo as i64);
                let new_threshold = threshold.wrapping_add(delta);
                alarm.value_hi = (new_threshold >> 32) as i32;
                alarm.value_lo = new_threshold as u32;
            } else {
                // No delta = one-shot alarm, deactivate
                alarm.state = 1; // Inactive
            }
        }
    }
}

/// Check if a single await trigger condition is satisfied given the current counter value.
fn is_trigger_satisfied(trigger: &AwaitTrigger, current_value: i64) -> bool {
    match trigger.test_type {
        0 => current_value >= trigger.wait_value, // PositiveTransition
        1 => current_value <= trigger.wait_value, // NegativeTransition
        2 => current_value >= trigger.wait_value, // PositiveComparison
        3 => current_value <= trigger.wait_value, // NegativeComparison
        _ => true,
    }
}

/// Check all pending Await requests and unblock if any trigger is satisfied.
/// Called when a counter value changes (SetCounter, ChangeCounter, frame tick).
/// Returns true if the connection was unblocked.
pub(crate) fn check_pending_awaits_ext(
    sync_state: &mut SyncState,
    server_time_fn: impl Fn() -> u32,
) -> bool {
    if sync_state.pending_awaits.is_empty() {
        return false;
    }

    let mut unblocked = false;

    sync_state.pending_awaits.retain(|pa| {
        let any_satisfied = pa.triggers.iter().any(|trigger| {
            let current = if trigger.counter_id == 1 {
                server_time_fn() as i64
            } else {
                sync_state
                    .counters
                    .get(&trigger.counter_id)
                    .map(|c| c.value_i64())
                    .unwrap_or(0)
            };
            is_trigger_satisfied(trigger, current)
        });
        if any_satisfied {
            unblocked = true;
            debug!("SYNC Await satisfied (seq={})", pa.seq);
            false // remove from pending
        } else {
            true // keep waiting
        }
    });

    if unblocked
        && sync_state.pending_awaits.is_empty()
        && sync_state.pending_fence_awaits.is_empty()
    {
        sync_state.blocked = false;
    }

    unblocked
}

/// Check all pending AwaitFence requests and unblock if any fence is triggered.
/// Called when a fence is triggered (TriggerFence).
/// Returns true if the connection was unblocked.
pub(crate) fn check_pending_fence_awaits_ext(sync_state: &mut SyncState) -> bool {
    if sync_state.pending_fence_awaits.is_empty() {
        return false;
    }

    let mut unblocked = false;

    sync_state.pending_fence_awaits.retain(|pfa| {
        let any_triggered = pfa.fence_ids.iter().any(|&fid| {
            sync_state
                .fences
                .get(&fid)
                .map(|f| f.triggered)
                .unwrap_or(false)
        });
        if any_triggered {
            unblocked = true;
            debug!("SYNC AwaitFence satisfied (seq={})", pfa.seq);
            false // remove from pending
        } else {
            true // keep waiting
        }
    });

    if unblocked
        && sync_state.pending_fence_awaits.is_empty()
        && sync_state.pending_awaits.is_empty()
    {
        sync_state.blocked = false;
    }

    unblocked
}

pub(crate) fn handle_sync_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        0 => {
            // Initialize: reply with version 3.1
            let _req = parse_minor!(InitializeRequest, data, state, seq, 134, 0);
            debug!("SYNC Initialize");
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u8(8, 3) // major version
                .set_u8(9, 1) // minor version
                .build()
        }
        1 => counter::list_system_counters(state, seq),
        2 => counter::create_counter(state, data, seq),
        3 => counter::set_counter(state, data, seq),
        4 => counter::change_counter(state, data, seq),
        5 => counter::query_counter(state, data, seq),
        6 => counter::destroy_counter(state, data, seq),
        7 => await_op::await_op(state, data, seq),
        8 => alarm::create_alarm(state, data, seq),
        9 => alarm::change_alarm(state, data, seq),
        10 => alarm::query_alarm(state, data, seq),
        11 => alarm::destroy_alarm(state, data, seq),
        12 => await_op::set_priority(state, data, seq),
        13 => await_op::get_priority(state, data, seq),
        14 => fence::create_fence(state, data, seq),
        15 => fence::trigger_fence(state, data, seq),
        16 => fence::reset_fence(state, data, seq),
        17 => fence::destroy_fence(state, data, seq),
        18 => fence::query_fence(state, data, seq),
        19 => fence::await_fence(state, data, seq),
        _ => {
            warn!("Unhandled SYNC minor opcode: {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                134,
                minor as u16,
            )
        }
    }
}
