//! DPMS (Display Power Management Signaling) extension handler (opcode 151).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::core::require_len;

/// DPMS (opcode 151)
pub(crate) fn handle_dpms_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => { // GetVersion
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1); // major
            state.write_u16(&mut reply, 10, 2); // minor
            reply.to_vec()
        }
        1 => { // Capable
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            reply[8] = 1; // capable = true
            reply.to_vec()
        }
        2 => { // GetTimeouts
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, state.dpms_standby_timeout);
            state.write_u16(&mut reply, 10, state.dpms_suspend_timeout);
            state.write_u16(&mut reply, 12, state.dpms_off_timeout);
            reply.to_vec()
        }
        3 => { // SetTimeouts
            require_len!(data, 10, seq, 151, minor as u16, state.msb_first);
            state.dpms_standby_timeout = state.read_u16(data, 4);
            state.dpms_suspend_timeout = state.read_u16(data, 6);
            state.dpms_off_timeout = state.read_u16(data, 8);
            debug!(
                "DPMS SetTimeouts: standby={} suspend={} off={}",
                state.dpms_standby_timeout, state.dpms_suspend_timeout, state.dpms_off_timeout
            );
            Vec::new()
        }
        4 => { // Enable
            state.dpms_enabled = true;
            debug!("DPMS Enable");
            Vec::new()
        }
        5 => { // Disable
            state.dpms_enabled = false;
            state.dpms_power_level = 0; // reset to On when disabled
            debug!("DPMS Disable");
            Vec::new()
        }
        6 => { // ForceLevel
            require_len!(data, 6, seq, 151, minor as u16, state.msb_first);
            let level = state.read_u16(data, 4);
            // 0=On, 1=Standby, 2=Suspend, 3=Off
            if level <= 3 {
                state.dpms_power_level = level;
                debug!("DPMS ForceLevel: level={level}");
            }
            Vec::new()
        }
        7 => { // Info
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, state.dpms_power_level);
            reply[10] = if state.dpms_enabled { 1 } else { 0 };
            reply.to_vec()
        }
        _ => {
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                151, minor as u16, state.msb_first,
            )
        }
    }
}
