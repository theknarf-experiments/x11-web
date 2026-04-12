//! SYNC extension handler — real counters, alarms, and fences.
//!
//! Implements the X Synchronization Extension per the spec:
//! - System counters (SERVERTIME)
//! - Client-created counters with get/set/change
//! - Alarms that trigger on counter value transitions
//! - Fences for synchronization primitives

use std::collections::HashMap;
use tracing::{debug, warn};

use super::super::client::ClientState;
use super::super::core::{write_u16_bo, write_u32_bo, BAD_LENGTH, BAD_MATCH, BAD_VALUE};

/// A SYNC counter (system or client-created).
#[derive(Clone, Debug)]
pub(crate) struct SyncCounter {
    /// Current counter value (64-bit, stored as hi/lo pair).
    pub(crate) value_hi: i32,
    pub(crate) value_lo: u32,
    /// True if this is a system counter (e.g., SERVERTIME).
    #[allow(dead_code)]
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
    pub(crate) value_type: u8,   // 0=Absolute, 1=Relative
    pub(crate) value_hi: i32,
    pub(crate) value_lo: u32,
    pub(crate) test_type: u8,    // 0=PositiveTransition, 1=NegativeTransition, 2=PositiveComparison, 3=NegativeComparison
    pub(crate) delta_hi: i32,
    pub(crate) delta_lo: u32,
    pub(crate) events: bool,
    pub(crate) state: u8,        // 0=Active, 1=Inactive, 2=Destroyed
}

/// A SYNC fence for synchronization.
#[derive(Clone, Debug)]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        s.counters.insert(1, SyncCounter {
            value_hi: 0,
            value_lo: 0,
            is_system: true,
        });
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
    check_alarms(alarms, counter_id, old_value, new_value, pending_events, seq, msb_first);
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
                0 => old_value < threshold && new_value >= threshold,  // PositiveTransition
                1 => old_value > threshold && new_value <= threshold,  // NegativeTransition
                2 => new_value >= threshold,                           // PositiveComparison
                3 => new_value <= threshold,                           // NegativeComparison
                _ => false,
            }
        })
        .map(|(&id, _)| id)
        .collect();

    for alarm_id in triggered {
        // Build AlarmNotify event (XSyncAlarmNotifyEvent)
        // Event code = SYNC base event (extension event, typically base + 0)
        // We use 83 as the SYNC alarm notify event code (134 base - 51 offset...
        // Actually the event is extension_base + 0. SYNC is opcode 134,
        // XSyncAlarmNotify is the only event = extension's first_event)
        // For our server, SYNC events are delivered via pending_events using the
        // event number registered in QueryExtension.
        // SYNC first_event = 83 (matches what we report in query.rs)
        let mut event = [0u8; 32];
        event[0] = 83; // SyncAlarmNotify event code
        event[1] = 0;  // sub-code
        write_u16_bo(&mut event, 2, seq, msb_first);
        write_u32_bo(&mut event, 4, alarm_id, msb_first);
        // counter value (bytes 8-15)
        let Some(alarm) = alarms.get(&alarm_id) else {
            return;
        };
        write_u32_bo(&mut event, 8, new_value as u32, msb_first);     // value_lo
        write_u32_bo(&mut event, 12, (new_value >> 32) as u32, msb_first); // value_hi
        // alarm value (bytes 16-23)
        write_u32_bo(&mut event, 16, alarm.value_lo, msb_first);
        write_u32_bo(&mut event, 20, alarm.value_hi as u32, msb_first);
        // timestamp
        write_u32_bo(&mut event, 24, 0, msb_first);
        // state
        event[28] = alarm.state;
        pending_events.push(event.to_vec());

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
        0 => current_value >= trigger.wait_value,  // PositiveTransition
        1 => current_value <= trigger.wait_value,  // NegativeTransition
        2 => current_value >= trigger.wait_value,  // PositiveComparison
        3 => current_value <= trigger.wait_value,  // NegativeComparison
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
                sync_state.counters.get(&trigger.counter_id)
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

    if unblocked && sync_state.pending_awaits.is_empty() && sync_state.pending_fence_awaits.is_empty() {
        sync_state.blocked = false;
    }

    unblocked
}

/// Check all pending AwaitFence requests and unblock if any fence is triggered.
/// Called when a fence is triggered (TriggerFence).
/// Returns true if the connection was unblocked.
pub(crate) fn check_pending_fence_awaits_ext(
    sync_state: &mut SyncState,
) -> bool {
    if sync_state.pending_fence_awaits.is_empty() {
        return false;
    }

    let mut unblocked = false;

    sync_state.pending_fence_awaits.retain(|pfa| {
        let any_triggered = pfa.fence_ids.iter().any(|&fid| {
            sync_state.fences.get(&fid).map(|f| f.triggered).unwrap_or(false)
        });
        if any_triggered {
            unblocked = true;
            debug!("SYNC AwaitFence satisfied (seq={})", pfa.seq);
            false // remove from pending
        } else {
            true // keep waiting
        }
    });

    if unblocked && sync_state.pending_fence_awaits.is_empty() && sync_state.pending_awaits.is_empty() {
        sync_state.blocked = false;
    }

    unblocked
}

pub(crate) fn handle_sync_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        0 => {
            // Initialize: reply with version 3.1
            debug!("SYNC Initialize");
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            reply[8] = 3; // major version
            reply[9] = 1; // minor version
            reply.to_vec()
        }
        1 => {
            // ListSystemCounters: reply with SERVERTIME counter
            debug!("SYNC ListSystemCounters");
            let counter_name = b"SERVERTIME";
            let name_len = counter_name.len();
            let name_pad = (4 - (name_len % 4)) % 4;
            let entry_size = 4 + 4 + 4 + 2 + name_len + name_pad;
            let extra = entry_size;
            let total = 32 + extra;
            let mut reply = vec![0u8; total];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (extra / 4) as u32);
            state.write_u32(&mut reply, 8, 1u32); // num_counters = 1
            let off = 32;
            state.write_u32(&mut reply, off, 1u32); // counter ID = 1 (SERVERTIME)
            state.write_u32(&mut reply, off + 4, 0u32); // resolution_hi
            state.write_u32(&mut reply, off + 8, 1u32); // resolution_lo = 1ms
            state.write_u16(&mut reply, off + 12, name_len as u16);
            reply[off + 14..off + 14 + name_len].copy_from_slice(counter_name);
            reply
        }
        2 => {
            // CreateCounter
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let counter_id = state.read_u32(data, 4);
            let value_hi = state.read_u32(data, 8) as i32;
            let value_lo = state.read_u32(data, 12);
            debug!("SYNC CreateCounter: id={counter_id:#x} value={value_hi}:{value_lo}");
            state.sync_state.counters.insert(counter_id, SyncCounter {
                value_hi,
                value_lo,
                is_system: false,
            });
            Vec::new()
        }
        3 => {
            // SetCounter
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let counter_id = state.read_u32(data, 4);
            let value_hi = state.read_u32(data, 8) as i32;
            let value_lo = state.read_u32(data, 12);
            debug!("SYNC SetCounter: id={counter_id:#x} value={value_hi}:{value_lo}");

            let old_value = state.sync_state.counters.get(&counter_id)
                .map(|c| c.value_i64())
                .unwrap_or(0);

            if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
                counter.value_hi = value_hi;
                counter.value_lo = value_lo;
            }

            let new_value = ((value_hi as i64) << 32) | (value_lo as i64);
            check_alarms(
                &mut state.sync_state.alarms, counter_id,
                old_value, new_value,
                &mut state.pending_events, seq, state.msb_first,
            );
            // Check if any pending Await is now satisfied
            let ts = state.timestamp();
            check_pending_awaits_ext(&mut state.sync_state, || ts);
            Vec::new()
        }
        4 => {
            // ChangeCounter
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let counter_id = state.read_u32(data, 4);
            let delta_hi = state.read_u32(data, 8) as i32;
            let delta_lo = state.read_u32(data, 12);
            let delta = ((delta_hi as i64) << 32) | (delta_lo as i64);
            debug!("SYNC ChangeCounter: id={counter_id:#x} delta={delta}");

            let old_value = state.sync_state.counters.get(&counter_id)
                .map(|c| c.value_i64())
                .unwrap_or(0);
            let new_value = old_value.wrapping_add(delta);

            if let Some(counter) = state.sync_state.counters.get_mut(&counter_id) {
                counter.set_from_i64(new_value);
            }

            check_alarms(
                &mut state.sync_state.alarms, counter_id,
                old_value, new_value,
                &mut state.pending_events, seq, state.msb_first,
            );
            // Check if any pending Await is now satisfied
            let ts = state.timestamp();
            check_pending_awaits_ext(&mut state.sync_state, || ts);
            Vec::new()
        }
        5 => {
            // QueryCounter
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let counter_id = state.read_u32(data, 4);
            debug!("SYNC QueryCounter: id={counter_id:#x}");

            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);

            if counter_id == 1 {
                // SERVERTIME: return current elapsed time in ms
                let ms = state.timestamp();
                state.write_u32(&mut reply, 8, 0u32);  // value_hi
                state.write_u32(&mut reply, 12, ms);    // value_lo
            } else if let Some(counter) = state.sync_state.counters.get(&counter_id) {
                state.write_u32(&mut reply, 8, counter.value_hi as u32);
                state.write_u32(&mut reply, 12, counter.value_lo);
            } else {
                // BadCounter
                return super::super::core::build_error_bo(
                    BAD_VALUE, seq, counter_id, 134, 5, state.msb_first,
                );
            }
            reply.to_vec()
        }
        6 => {
            // DestroyCounter
            if data.len() >= 8 {
                let counter_id = state.read_u32(data, 4);
                debug!("SYNC DestroyCounter: id={counter_id:#x}");
                state.sync_state.counters.remove(&counter_id);
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
                        debug!("SYNC DestroyCounter: cancelling pending Await (seq={})", pa.seq);
                    }
                    !references_destroyed
                });
                if had_pending && state.sync_state.pending_awaits.is_empty()
                    && state.sync_state.pending_fence_awaits.is_empty()
                {
                    state.sync_state.blocked = false;
                }
            }
            Vec::new()
        }
        7 => {
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
        8 => {
            // CreateAlarm
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let alarm_id = state.read_u32(data, 4);
            let value_mask = if data.len() >= 12 { state.read_u32(data, 8) } else { 0 };

            let mut alarm = SyncAlarm {
                counter: 0,
                value_type: 0,   // Absolute
                value_hi: 0,
                value_lo: 0,
                test_type: 0,   // PositiveTransition
                delta_hi: 0,
                delta_lo: 1,    // Default delta = 1
                events: true,
                state: 0,       // Active
            };

            let mut offset = 12;
            for bit in 0..4 {
                if value_mask & (1 << bit) != 0 && offset + 4 <= data.len() {
                    let val = state.read_u32(data, offset);
                    match bit {
                        0 => alarm.counter = val,
                        1 => alarm.value_type = val as u8,
                        2 => alarm.test_type = val as u8,
                        3 => {
                            // Value is 64-bit (hi, lo)
                            alarm.value_hi = val as i32;
                            if offset + 8 <= data.len() {
                                alarm.value_lo = state.read_u32(data, offset + 4);
                                offset += 4; // extra 4 bytes for 64-bit value
                            }
                        }
                        _ => {}
                    }
                    offset += 4;
                }
            }
            // Parse delta if present (bits 4+)
            if value_mask & 0x10 != 0 && offset + 8 <= data.len() {
                alarm.delta_hi = state.read_u32(data, offset) as i32;
                alarm.delta_lo = state.read_u32(data, offset + 4);
                offset += 8;
            }
            if value_mask & 0x20 != 0 && offset + 4 <= data.len() {
                alarm.events = state.read_u32(data, offset) != 0;
            }

            debug!("SYNC CreateAlarm: id={alarm_id:#x} counter={:#x} test_type={} events={}",
                alarm.counter, alarm.test_type, alarm.events);

            state.sync_state.alarms.insert(alarm_id, alarm);
            Vec::new()
        }
        9 => {
            // ChangeAlarm
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let bo = state.msb_first;
            let alarm_id = state.read_u32(data, 4);
            let value_mask = if data.len() >= 12 { state.read_u32(data, 8) } else { 0 };

            if let Some(alarm) = state.sync_state.alarms.get_mut(&alarm_id) {
                let mut offset = 12;
                for bit in 0..4 {
                    if value_mask & (1 << bit) != 0 && offset + 4 <= data.len() {
                        let val = super::super::core::read_u32_bo(data, offset, bo);
                        match bit {
                            0 => alarm.counter = val,
                            1 => alarm.value_type = val as u8,
                            2 => alarm.test_type = val as u8,
                            3 => {
                                alarm.value_hi = val as i32;
                                if offset + 8 <= data.len() {
                                    alarm.value_lo = super::super::core::read_u32_bo(data, offset + 4, bo);
                                    offset += 4;
                                }
                            }
                            _ => {}
                        }
                        offset += 4;
                    }
                }
                if value_mask & 0x10 != 0 && offset + 8 <= data.len() {
                    alarm.delta_hi = super::super::core::read_u32_bo(data, offset, bo) as i32;
                    alarm.delta_lo = super::super::core::read_u32_bo(data, offset + 4, bo);
                    offset += 8;
                }
                if value_mask & 0x20 != 0 && offset + 4 <= data.len() {
                    alarm.events = super::super::core::read_u32_bo(data, offset, bo) != 0;
                }
                // Re-activate if it was inactive
                if alarm.state == 1 {
                    alarm.state = 0;
                }
                debug!("SYNC ChangeAlarm: id={alarm_id:#x}");
            }
            Vec::new()
        }
        10 => {
            // QueryAlarm
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let alarm_id = state.read_u32(data, 4);
            debug!("SYNC QueryAlarm: id={alarm_id:#x}");

            let mut reply = vec![0u8; 40];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 2u32); // length = 2 extra u32s

            if let Some(alarm) = state.sync_state.alarms.get(&alarm_id) {
                // trigger: counter(4) + value_type(4) + value(8) + test_type(4) + delta(8) + events(4) + state(4)
                state.write_u32(&mut reply, 8, alarm.counter);
                state.write_u32(&mut reply, 12, alarm.value_type as u32);
                state.write_u32(&mut reply, 16, alarm.value_hi as u32);
                state.write_u32(&mut reply, 20, alarm.value_lo);
                state.write_u32(&mut reply, 24, alarm.test_type as u32);
                state.write_u32(&mut reply, 28, alarm.delta_hi as u32);
                state.write_u32(&mut reply, 32, alarm.delta_lo);
                reply[36] = if alarm.events { 1 } else { 0 };
                reply[37] = alarm.state;
            }
            reply
        }
        11 => {
            // DestroyAlarm
            if data.len() >= 8 {
                let alarm_id = state.read_u32(data, 4);
                debug!("SYNC DestroyAlarm: id={alarm_id:#x}");
                state.sync_state.alarms.remove(&alarm_id);
            }
            Vec::new()
        }
        12 => {
            // SetPriority: store the priority for the given resource
            if data.len() >= 12 {
                let resource_id = state.read_u32(data, 4);
                let priority = state.read_u32(data, 8) as i32;
                debug!("SYNC SetPriority: resource={resource_id:#x} priority={priority}");
                state.sync_state.priorities.insert(resource_id, priority);
            }
            Vec::new()
        }
        13 => {
            // GetPriority: return stored priority or 0 (normal)
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
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
        14 => {
            // CreateFence: bytes 4-7 = drawable, 8-11 = fence_id, 12 = initially_triggered
            if data.len() >= 13 {
                let _drawable = state.read_u32(data, 4);
                let fence_id = state.read_u32(data, 8);
                let initially_triggered = data[12] != 0;
                debug!("SYNC CreateFence: id={fence_id:#x} initially_triggered={initially_triggered}");
                state.sync_state.fences.insert(fence_id, FenceState {
                    id: fence_id,
                    triggered: initially_triggered,
                    initially_triggered,
                    fd: -1,
                });
            }
            Vec::new()
        }
        15 => {
            // TriggerFence: bytes 4-7 = fence_id
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
        16 => {
            // ResetFence: bytes 4-7 = fence_id
            // Per spec, fence must be triggered to reset; return BadMatch otherwise.
            if data.len() >= 8 {
                let fence_id = state.read_u32(data, 4);
                debug!("SYNC ResetFence: id={fence_id:#x}");
                if let Some(fence) = state.sync_state.fences.get_mut(&fence_id) {
                    if !fence.triggered {
                        return super::super::core::build_error_bo(
                            BAD_MATCH, seq, fence_id, 134, 16, state.msb_first,
                        );
                    }
                    fence.triggered = false;
                } else {
                    return super::super::core::build_error_bo(
                        BAD_VALUE, seq, fence_id, 134, 16, state.msb_first,
                    );
                }
            }
            Vec::new()
        }
        17 => {
            // DestroyFence
            if data.len() >= 8 {
                let fence_id = state.read_u32(data, 4);
                debug!("SYNC DestroyFence: id={fence_id:#x}");
                if let Some(fence) = state.sync_state.fences.remove(&fence_id) {
                    if fence.fd >= 0 {
                        unsafe { libc::close(fence.fd); }
                    }
                }
                // Cancel any pending AwaitFence requests that reference this fence.
                // Per X11 SYNC spec, destroying a fence while an AwaitFence references it
                // should unblock the client.
                let had_pending = !state.sync_state.pending_fence_awaits.is_empty();
                state.sync_state.pending_fence_awaits.retain(|pfa| {
                    let references_destroyed = pfa.fence_ids.iter().any(|&fid| fid == fence_id);
                    if references_destroyed {
                        debug!("SYNC DestroyFence: cancelling pending AwaitFence (seq={})", pfa.seq);
                    }
                    !references_destroyed
                });
                if had_pending && state.sync_state.pending_fence_awaits.is_empty()
                    && state.sync_state.pending_awaits.is_empty()
                {
                    state.sync_state.blocked = false;
                }
            }
            Vec::new()
        }
        18 => {
            // QueryFence
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(BAD_LENGTH, seq, 0, 134, minor as u16, state.msb_first);
            }
            let fence_id = state.read_u32(data, 4);
            let triggered = state.sync_state.fences.get(&fence_id).map(|f| f.triggered).unwrap_or(true);
            debug!("SYNC QueryFence: id={fence_id:#x} triggered={triggered}");

            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            reply[8] = if triggered { 1 } else { 0 };
            reply.to_vec()
        }
        19 => {
            // AwaitFence: block until at least one fence is triggered.
            // Parse fence IDs from the request body (each 4 bytes, starting at offset 4).
            let n_fences = data.len().saturating_sub(4) / 4;
            let mut fence_ids = Vec::with_capacity(n_fences);
            let mut any_triggered = false;
            let mut offset = 4;
            for _ in 0..n_fences {
                if offset + 4 > data.len() { break; }
                let fence_id = state.read_u32(data, offset);
                if state.sync_state.fences.get(&fence_id).map(|f| f.triggered).unwrap_or(false) {
                    any_triggered = true;
                }
                fence_ids.push(fence_id);
                offset += 4;
            }

            if any_triggered || fence_ids.is_empty() {
                debug!("SYNC AwaitFence: satisfied immediately ({} fences)", fence_ids.len());
            } else {
                debug!("SYNC AwaitFence: {} fences not yet triggered, blocking connection", fence_ids.len());
                state.sync_state.pending_fence_awaits.push(PendingFenceAwait {
                    fence_ids,
                    seq,
                });
                state.sync_state.blocked = true;
            }
            Vec::new()
        }
        _ => {
            warn!("Unhandled SYNC minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                134, minor as u16, state.msb_first,
            )
        }
    }
}
