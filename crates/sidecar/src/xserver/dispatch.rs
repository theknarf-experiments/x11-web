//! Top-level X11 request dispatch: routes opcodes to core or extension handlers.

use tracing::warn;

use super::client::ClientState;
use super::core::*;
use super::handlers;

/// Dispatch an X11 request to the appropriate handler based on the major opcode.
pub(super) fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;
    // Extension major opcodes (assigned by QueryExtension)
    const EXT_SHAPE: u8 = 128;
    const EXT_SHM: u8 = 130;
    const EXT_XINPUT: u8 = 131;
    const EXT_BIG_REQUESTS: u8 = 133;
    const EXT_SYNC: u8 = 134;
    const EXT_GE: u8 = 135;
    const EXT_XKB: u8 = 136;
    const EXT_XFIXES: u8 = 138;
    const EXT_RENDER: u8 = 139;
    const EXT_RANDR: u8 = 140;
    const EXT_XC_MISC: u8 = 141;
    const EXT_COMPOSITE: u8 = 142;
    const EXT_DAMAGE: u8 = 143;
    const EXT_PRESENT: u8 = 148;
    const EXT_DRI3: u8 = 149;
    const EXT_XTEST: u8 = 150;
    const EXT_DPMS: u8 = 151;
    const EXT_SCREEN_SAVER: u8 = 152;
    const EXT_VIDMODE: u8 = 153;
    const EXT_RECORD: u8 = 154;
    const EXT_SECURITY: u8 = 155;
    const EXT_XVIDEO: u8 = 156;
    const EXT_DBE: u8 = 157;
    const EXT_XINERAMA: u8 = 158;
    const EXT_GLX: u8 = 159;
    const EXT_XRESOURCE: u8 = 160;

    match major_opcode {
        // Core protocol requests (opcodes 1-127)
        1..=127 => handlers::handle_core_request(state, data),

        EXT_BIG_REQUESTS => {
            // BigReqEnable: mark BIG-REQUESTS as enabled and return max request length.
            state.big_requests_enabled = true;
            let bo = state.msb_first;
            let mut reply = [0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 8, 4194304u32, bo); // 16MB / 4 = 4194304 words
            reply.to_vec()
        }

        // Extension protocol requests
        EXT_SHAPE => handlers::extensions::handle_shape_request(state, data, seq),
        EXT_SHM => handlers::extensions::handle_shm_request(state, data, seq),
        EXT_XINPUT => {
            let mut reply = crate::xinput2::handle_request(
                data,
                seq,
                &mut state.xi.valuators,
                &mut state.xi.selections,
                &mut state.xi.pending,
                &mut state.xi.client_pointer,
                &mut state.xi.device_properties,
                &mut state.focus_window,
                &mut state.xi.active_grabs,
                &mut state.xi.passive_grabs,
                &mut state.xi.pointer_frozen,
                &mut state.xi.keyboard_frozen,
                &mut state.xi.frozen_pointer_events,
                &mut state.xi.frozen_keyboard_events,
                &mut state.xi.xi1_dont_propagate,
                SCREEN_WIDTH,
                SCREEN_HEIGHT,
                state.root_window,
                state.msb_first,
            );
            if data.len() >= 2 && data[1] == x11rb_protocol::protocol::xinput::XI_QUERY_POINTER_REQUEST
                && reply.len() >= 12
            {
                crate::xinput2::patch_query_pointer_root(&mut reply, state.root_window, state.msb_first);
            }
            reply
        }
        EXT_SYNC => handlers::extensions::handle_sync_request(state, data, seq),
        EXT_GE => handlers::extensions::handle_ge_request(state, data, seq),
        EXT_XKB => handlers::extensions::handle_xkb_request(state, data, seq),
        EXT_XFIXES => handlers::extensions::handle_xfixes_request(state, data, seq),
        EXT_RENDER => handlers::render::handle_render_request(state, data, seq),
        EXT_RANDR => handlers::extensions::handle_randr_request(state, data, seq),
        EXT_XC_MISC => handlers::extensions::handle_xc_misc_request(state, data, seq),
        EXT_COMPOSITE => handlers::extensions::handle_x_composite_request(state, data, seq),
        EXT_DAMAGE => handlers::extensions::handle_damage_request(state, data, seq),
        EXT_PRESENT => handlers::extensions::handle_present_request(state, data, seq),
        EXT_DRI3 => handlers::extensions::handle_dri3_request(state, data, seq),
        EXT_XTEST => handlers::extensions::handle_xtest_request(state, data, seq),
        EXT_DPMS => handlers::extensions::handle_dpms_request(state, data, seq),
        EXT_SCREEN_SAVER => handlers::extensions::handle_screen_saver_request(state, data, seq),
        EXT_VIDMODE => handlers::extensions::handle_vidmode_request(state, data, seq),
        EXT_RECORD => handlers::record::handle_record_request(state, data, seq),
        EXT_SECURITY => handlers::extensions::handle_security_request(state, data, seq),
        EXT_XVIDEO => handlers::extensions::handle_xvideo_request(state, data, seq),
        EXT_DBE => handlers::extensions::handle_dbe_request(state, data, seq),
        EXT_XINERAMA => handlers::extensions::handle_xinerama_request(state, data, seq),
        EXT_GLX => handlers::extensions::handle_glx_request(state, data, seq),
        EXT_XRESOURCE => handlers::extensions::handle_xresource_request(state, data, seq),
        _ => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {_minor}");
            // Return BadRequest error per spec for unrecognized opcodes
            super::core::build_error_bo(
                BAD_REQUEST, seq, major_opcode as u32,
                major_opcode, _minor as u16, state.msb_first,
            )
        }
    }
}
