//! DPMS (Display Power Management Signaling) extension handler (opcode 151).

use super::parse_minor;
use tracing::debug;
use x11rb_protocol::protocol::dpms::{
    CAPABLE_REQUEST, DISABLE_REQUEST, DPMSMode, ENABLE_REQUEST, FORCE_LEVEL_REQUEST,
    ForceLevelRequest, GET_TIMEOUTS_REQUEST, GET_VERSION_REQUEST, INFO_REQUEST,
    SET_TIMEOUTS_REQUEST, SetTimeoutsRequest,
};

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// DPMS (opcode 151)
pub(crate) fn handle_dpms_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let dpms_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 151, minor as u16)
    };
    match minor {
        GET_VERSION_REQUEST => ReplyBuf::fixed(seq, state.msb_first)
            .set_u16(8, 1) // major
            .set_u16(10, 2) // minor
            .build(),
        CAPABLE_REQUEST => ReplyBuf::fixed(seq, state.msb_first)
            .set_u8(8, 1) // capable = true
            .build(),
        GET_TIMEOUTS_REQUEST => ReplyBuf::fixed(seq, state.msb_first)
            .set_u16(8, state.dpms_standby_timeout)
            .set_u16(10, state.dpms_suspend_timeout)
            .set_u16(12, state.dpms_off_timeout)
            .build(),
        SET_TIMEOUTS_REQUEST => {
            require_len!(data, 10, seq, 151, minor as u16, state.msb_first);
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
        ENABLE_REQUEST => {
            state.dpms_enabled = true;
            debug!("DPMS Enable");
            Vec::new()
        }
        DISABLE_REQUEST => {
            state.dpms_enabled = false;
            state.dpms_power_level = u16::from(DPMSMode::ON); // reset when disabled
            debug!("DPMS Disable");
            Vec::new()
        }
        FORCE_LEVEL_REQUEST => {
            require_len!(data, 6, seq, 151, minor as u16, state.msb_first);
            let req = parse_minor!(ForceLevelRequest, data, state, seq, 151, minor as u16);
            let level = req.power_level;
            // DPMSMode is u16; only 0..=3 are valid (On/Standby/Suspend/Off).
            if !matches!(
                level,
                DPMSMode::ON | DPMSMode::STANDBY | DPMSMode::SUSPEND | DPMSMode::OFF
            ) {
                return dpms_err(crate::xserver::core::VALUE_ERROR, u32::from(level));
            }
            // ForceLevel fails if DPMS is disabled and the requested level
            // is not On (per the DPMS spec).
            if !state.dpms_enabled && level != DPMSMode::ON {
                return dpms_err(crate::xserver::core::VALUE_ERROR, u32::from(level));
            }
            state.dpms_power_level = u16::from(level);
            debug!("DPMS ForceLevel: level={level:?}");
            Vec::new()
        }
        INFO_REQUEST => ReplyBuf::fixed(seq, state.msb_first)
            .set_u16(8, state.dpms_power_level)
            .set_u8(10, if state.dpms_enabled { 1 } else { 0 })
            .build(),
        _ => {
            debug!("DPMS: unhandled minor opcode {minor}");
            dpms_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
