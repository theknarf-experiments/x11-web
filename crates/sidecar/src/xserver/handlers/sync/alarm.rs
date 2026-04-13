//! SYNC alarm operations: CreateAlarm, ChangeAlarm, QueryAlarm, DestroyAlarm.

use tracing::debug;

use super::super::super::client::ClientState;
use super::SyncAlarm;
use crate::xserver::core::require_len;

/// Minor opcode 8: CreateAlarm
pub(crate) fn create_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 134, data[1] as u16, state.msb_first);
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

/// Minor opcode 9: ChangeAlarm
pub(crate) fn change_alarm(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 134, data[1] as u16, state.msb_first);
    let bo = state.msb_first;
    let alarm_id = state.read_u32(data, 4);
    let value_mask = if data.len() >= 12 { state.read_u32(data, 8) } else { 0 };

    if let Some(alarm) = state.sync_state.alarms.get_mut(&alarm_id) {
        let mut offset = 12;
        for bit in 0..4 {
            if value_mask & (1 << bit) != 0 && offset + 4 <= data.len() {
                let val = super::super::super::core::read_u32_bo(data, offset, bo);
                match bit {
                    0 => alarm.counter = val,
                    1 => alarm.value_type = val as u8,
                    2 => alarm.test_type = val as u8,
                    3 => {
                        alarm.value_hi = val as i32;
                        if offset + 8 <= data.len() {
                            alarm.value_lo = super::super::super::core::read_u32_bo(data, offset + 4, bo);
                            offset += 4;
                        }
                    }
                    _ => {}
                }
                offset += 4;
            }
        }
        if value_mask & 0x10 != 0 && offset + 8 <= data.len() {
            alarm.delta_hi = super::super::super::core::read_u32_bo(data, offset, bo) as i32;
            alarm.delta_lo = super::super::super::core::read_u32_bo(data, offset + 4, bo);
            offset += 8;
        }
        if value_mask & 0x20 != 0 && offset + 4 <= data.len() {
            alarm.events = super::super::super::core::read_u32_bo(data, offset, bo) != 0;
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
    require_len!(data, 8, seq, 134, data[1] as u16, state.msb_first);
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

/// Minor opcode 11: DestroyAlarm
pub(crate) fn destroy_alarm(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let alarm_id = state.read_u32(data, 4);
        debug!("SYNC DestroyAlarm: id={alarm_id:#x}");
        state.sync_state.alarms.remove(&alarm_id);
    }
    Vec::new()
}
