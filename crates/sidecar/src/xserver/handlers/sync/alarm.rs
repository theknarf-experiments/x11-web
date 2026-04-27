//! SYNC alarm operations: CreateAlarm, ChangeAlarm, QueryAlarm, DestroyAlarm.

use tracing::debug;
use super::super::parse_minor;

use super::super::super::client::ClientState;
use super::SyncAlarm;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::sync::{
    ChangeAlarmRequest, CreateAlarmRequest, DestroyAlarmRequest, QueryAlarmRequest,
};

/// Minor opcode 8: CreateAlarm
pub(crate) fn create_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(CreateAlarmRequest, data, state, seq, 134, 8);
    let alarm_id = req.id;
    let vl = &*req.value_list;

    let mut alarm = SyncAlarm {
        counter: vl.counter.unwrap_or(0),
        value_type: vl.value_type.map(|v| u32::from(v) as u8).unwrap_or(0), // Absolute
        value_hi: vl.value.map(|v| v.hi).unwrap_or(0),
        value_lo: vl.value.map(|v| v.lo).unwrap_or(0),
        test_type: vl.test_type.map(|v| u32::from(v) as u8).unwrap_or(0), // PositiveTransition
        delta_hi: vl.delta.map(|v| v.hi).unwrap_or(0),
        delta_lo: vl.delta.map(|v| v.lo).unwrap_or(1), // Default delta = 1
        events: vl.events.map(|v| v != 0).unwrap_or(true),
        state: 0, // Active
    };

    // If delta is present, both hi and lo come from it; if not, defaults are (0, 1).
    // But if only delta is set, the above handles it correctly already.
    // If delta is not in the value_list at all, keep defaults (0, 1).
    if vl.delta.is_some() {
        // delta was explicitly provided, values already set above
    } else {
        alarm.delta_hi = 0;
        alarm.delta_lo = 1;
    }

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
        if alarm.state == 1 {
            alarm.state = 0;
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

    let mut reply = ReplyBuf::with_extra(seq, 8, state.msb_first);

    if let Some(alarm) = state.sync_state.alarms.get(&alarm_id) {
        // trigger: counter(4) + value_type(4) + value(8) + test_type(4) + delta(8) + events(4) + state(4)
        reply = reply.set_u32(8, alarm.counter)
            .set_u32(12, alarm.value_type as u32)
            .set_u32(16, alarm.value_hi as u32);
        reply = reply.set_u32(20, alarm.value_lo)
            .set_u32(24, alarm.test_type as u32)
            .set_u32(28, alarm.delta_hi as u32)
            .set_u32(32, alarm.delta_lo);
        reply.buf_mut()[36] = if alarm.events { 1 } else { 0 };
        reply.buf_mut()[37] = alarm.state;
    }
    reply.build()
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
