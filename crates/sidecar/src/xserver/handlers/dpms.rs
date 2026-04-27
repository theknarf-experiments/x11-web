//! DPMS (Display Power Management Signaling) extension handler (opcode 151).

use tracing::debug;
use super::parse_minor;

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// DPMS (opcode 151)
pub(crate) fn handle_dpms_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let bo = state.msb_first;
    let dpms_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error_bo(code, seq, bad_value, 151, minor as u16, bo)
    };
    match minor {
        0 => {
            // GetVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // major
                .set_u16(10, 2) // minor
                .build()
        }
        1 => {
            // Capable
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u8(8, 1) // capable = true
                .build()
        }
        2 => {
            // GetTimeouts
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, state.dpms_standby_timeout)
                .set_u16(10, state.dpms_suspend_timeout)
                .set_u16(12, state.dpms_off_timeout)
                .build()
        }
        3 => {
            // SetTimeouts
            require_len!(data, 10, seq, 151, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dpms::SetTimeoutsRequest;
            let req = parse_minor!(SetTimeoutsRequest, data, state, seq, 151, minor as u16);
            state.dpms_standby_timeout = req.standby_timeout;
            state.dpms_suspend_timeout = req.suspend_timeout;
            state.dpms_off_timeout = req.off_timeout;
            debug!(
                "DPMS SetTimeouts: standby={} suspend={} off={}",
                state.dpms_standby_timeout, state.dpms_suspend_timeout, state.dpms_off_timeout
            );
            Vec::new()
        }
        4 => {
            // Enable
            state.dpms_enabled = true;
            debug!("DPMS Enable");
            Vec::new()
        }
        5 => {
            // Disable
            state.dpms_enabled = false;
            state.dpms_power_level = 0; // reset to On when disabled
            debug!("DPMS Disable");
            Vec::new()
        }
        6 => {
            // ForceLevel
            require_len!(data, 6, seq, 151, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dpms::ForceLevelRequest;
            let req = parse_minor!(ForceLevelRequest, data, state, seq, 151, minor as u16);
            let level = u16::from(req.power_level);
            // Per DPMS spec: level must be 0-3 (On, Standby, Suspend, Off)
            if level > 3 {
                return dpms_err(crate::xserver::core::VALUE_ERROR, level as u32);
            }
            // Per DPMS spec: ForceLevel should fail if DPMS is disabled
            // and the requested level is not DPMSModeOn (0)
            if !state.dpms_enabled && level != 0 {
                return dpms_err(crate::xserver::core::VALUE_ERROR, level as u32);
            }
            state.dpms_power_level = level;
            debug!("DPMS ForceLevel: level={level}");
            Vec::new()
        }
        7 => {
            // Info
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, state.dpms_power_level)
                .set_u8(10, if state.dpms_enabled { 1 } else { 0 })
                .build()
        }
        _ => {
            debug!("DPMS: unhandled minor opcode {minor}");
            dpms_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
