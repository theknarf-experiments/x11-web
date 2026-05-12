//! SYNC alarm operations: CreateAlarm, ChangeAlarm, QueryAlarm, DestroyAlarm.

use super::super::parse_minor;
use tracing::debug;

use super::super::super::client::ClientState;
use super::SyncAlarm;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::sync::{
    ChangeAlarmRequest, CreateAlarmRequest, DestroyAlarmRequest, Int64, QueryAlarmReply,
    QueryAlarmRequest, Trigger, ALARMSTATE, TESTTYPE, VALUETYPE,
};

/// Minor opcode 8: CreateAlarm
pub(crate) fn create_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateAlarmRequest, data, state, seq, 134, 8);
    let alarm_id = req.id;
    let vl = &*req.value_list;

    let alarm = SyncAlarm {
        counter: vl.counter.unwrap_or(0),
        value_type: vl
            .value_type
            .map(|v| u32::from(v) as u8)
            .unwrap_or_else(|| u32::from(VALUETYPE::ABSOLUTE) as u8),
        value_hi: vl.value.map(|v| v.hi).unwrap_or(0),
        value_lo: vl.value.map(|v| v.lo).unwrap_or(0),
        test_type: vl
            .test_type
            .map(|v| u32::from(v) as u8)
            .unwrap_or_else(|| u32::from(TESTTYPE::POSITIVE_TRANSITION) as u8),
        delta_hi: vl.delta.map(|v| v.hi).unwrap_or(0),
        delta_lo: vl.delta.map(|v| v.lo).unwrap_or(1), // Default delta = 1
        events: vl.events.map(|v| v != 0).unwrap_or(true),
        state: ALARMSTATE::ACTIVE.into(),
    };

    debug!(
        "SYNC CreateAlarm: id={alarm_id:#x} counter={:#x} test_type={} events={}",
        alarm.counter, alarm.test_type, alarm.events
    );

    state.sync_state.alarms.insert(alarm_id, alarm);
    Vec::new()
}

/// Minor opcode 9: ChangeAlarm
pub(crate) fn change_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(ChangeAlarmRequest, data, state, seq, 134, 9);
    let alarm_id = req.id;
    let vl = &*req.value_list;

    if let Some(alarm) = state.sync_state.alarms.get_mut(&alarm_id) {
        if let Some(counter) = vl.counter {
            alarm.counter = counter;
        }
        if let Some(vt) = vl.value_type {
            alarm.value_type = u32::from(vt) as u8;
        }
        if let Some(val) = vl.value {
            alarm.value_hi = val.hi;
            alarm.value_lo = val.lo;
        }
        if let Some(tt) = vl.test_type {
            alarm.test_type = u32::from(tt) as u8;
        }
        if let Some(d) = vl.delta {
            alarm.delta_hi = d.hi;
            alarm.delta_lo = d.lo;
        }
        if let Some(ev) = vl.events {
            alarm.events = ev != 0;
        }
        // Re-activate if it was inactive
        if ALARMSTATE::from(alarm.state) == ALARMSTATE::INACTIVE {
            alarm.state = ALARMSTATE::ACTIVE.into();
        }
        debug!("SYNC ChangeAlarm: id={alarm_id:#x}");
    }
    Vec::new()
}

/// Minor opcode 10: QueryAlarm
pub(crate) fn query_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(QueryAlarmRequest, data, state, seq, 134, 10);
    let alarm_id = req.alarm;
    debug!("SYNC QueryAlarm: id={alarm_id:#x}");

    let (trigger, delta, events, alarm_state) =
        if let Some(alarm) = state.sync_state.alarms.get(&alarm_id) {
            (
                Trigger {
                    counter: alarm.counter,
                    wait_type: VALUETYPE::from(alarm.value_type as u32),
                    wait_value: Int64 {
                        hi: alarm.value_hi,
                        lo: alarm.value_lo,
                    },
                    test_type: TESTTYPE::from(alarm.test_type as u32),
                },
                Int64 {
                    hi: alarm.delta_hi,
                    lo: alarm.delta_lo,
                },
                alarm.events,
                ALARMSTATE::from(alarm.state),
            )
        } else {
            (
                Trigger {
                    counter: 0,
                    wait_type: VALUETYPE::from(0u32),
                    wait_value: Int64 { hi: 0, lo: 0 },
                    test_type: TESTTYPE::from(0u32),
                },
                Int64 { hi: 0, lo: 0 },
                false,
                ALARMSTATE::INACTIVE,
            )
        };
    serialize_reply(
        &QueryAlarmReply {
            sequence: seq,
            length: 0,
            trigger,
            delta,
            events,
            state: alarm_state,
        },
        state.byte_order(),
    )
}

/// Minor opcode 11: DestroyAlarm
pub(crate) fn destroy_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(DestroyAlarmRequest, data, state, seq, 134, 11);
    let alarm_id = req.alarm;
    debug!("SYNC DestroyAlarm: id={alarm_id:#x}");
    state.sync_state.alarms.remove(&alarm_id);
    state.recycle_xid(alarm_id);
    Vec::new()
}
