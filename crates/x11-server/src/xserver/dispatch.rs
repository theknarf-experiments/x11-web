//! Top-level X11 request dispatch: routes opcodes to core or extension handlers.
//!
//! Extension dispatch uses the [`ExtensionRegistry`] to resolve major opcodes
//! and check whether an extension is enabled, then matches on [`ExtensionId`]
//! to call the concrete handler.

use tracing::warn;

use super::client::ClientState;
use super::core::*;
use super::extensions::ExtensionId;
use super::handlers;
use super::reply::serialize_reply;

/// Minimum length of an X11 request header: major opcode (1) + minor opcode
/// or data byte (1) + length-in-words (2).
const MIN_REQUEST_HEADER_LEN: usize = 4;

/// Dispatch an X11 request to the appropriate handler based on the major opcode.
pub(super) fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < MIN_REQUEST_HEADER_LEN {
        return Vec::new();
    }
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;

    // Core protocol requests (opcodes 1..=CORE_REQUEST_OPCODE_MAX).
    if major_opcode <= 127 {
        return handlers::handle_core_request(state, data);
    }

    // Extension protocol requests (opcodes 128+).
    let bad_request = || {
        build_error(
            REQUEST_ERROR,
            seq,
            major_opcode as u32,
            major_opcode,
            _minor as u16,
        )
    };
    match state.extension_registry.by_opcode(major_opcode) {
        Some(info) if !info.enabled => {
            warn!(
                "Request for disabled extension {:?} (opcode {major_opcode})",
                info.wire_name
            );
            bad_request()
        }
        Some(info) => dispatch_extension(state, data, seq, info.id),
        None => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {_minor}");
            bad_request()
        }
    }
}

/// Route an extension request to the correct handler based on its
/// [`ExtensionId`].  Each arm is feature-gated so that disabled extension
/// groups are compiled out entirely.
fn dispatch_extension(state: &mut ClientState, data: &[u8], seq: u16, id: ExtensionId) -> Vec<u8> {
    match id {
        // -- ext-core (always compiled in) ------------------------------------
        ExtensionId::BigRequests => {
            // BigReqEnable: mark BIG-REQUESTS as enabled and return max request length.
            state.big_requests_enabled = true;
            serialize_reply(
                &x11rb_protocol::protocol::bigreq::EnableReply {
                    sequence: seq,
                    length: 0,
                    maximum_request_length: crate::xserver::core::BIG_REQUESTS_MAX_LEN_WORDS,
                },
                state.byte_order(),
            )
        }
        ExtensionId::Shape => handlers::extensions::handle_shape_request(state, data, seq),
        ExtensionId::MitShm => handlers::extensions::handle_shm_request(state, data, seq),
        ExtensionId::Sync => handlers::extensions::handle_sync_request(state, data, seq),
        ExtensionId::GenericEvent => handlers::extensions::handle_ge_request(state, data, seq),
        ExtensionId::Xfixes => handlers::extensions::handle_xfixes_request(state, data, seq),
        ExtensionId::Randr => handlers::extensions::handle_randr_request(state, data, seq),
        ExtensionId::XcMisc => handlers::extensions::handle_xc_misc_request(state, data, seq),
        ExtensionId::XResource => handlers::extensions::handle_xresource_request(state, data, seq),

        // -- ext-input --------------------------------------------------------
        #[cfg(feature = "ext-input")]
        ExtensionId::XInput => {
            let custom_keymap = state.custom_keymap.lock().unwrap().clone();
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
                &custom_keymap,
            );
            if data.len() >= 2
                && data[1] == x11rb_protocol::protocol::xinput::XI_QUERY_POINTER_REQUEST
                && reply.len() >= 12
            {
                crate::xinput2::patch_query_pointer_root(
                    &mut reply,
                    state.root_window,
                    state.msb_first,
                );
            }
            reply
        }
        #[cfg(feature = "ext-input")]
        ExtensionId::Xtest => handlers::extensions::handle_xtest_request(state, data, seq),
        #[cfg(feature = "ext-input")]
        ExtensionId::Xkb => handlers::extensions::handle_xkb_request(state, data, seq),

        // -- ext-render -------------------------------------------------------
        #[cfg(feature = "ext-render")]
        ExtensionId::Render => handlers::render::handle_render_request(state, data, seq),
        #[cfg(feature = "ext-render")]
        ExtensionId::Composite => {
            handlers::extensions::handle_x_composite_request(state, data, seq)
        }
        #[cfg(feature = "ext-render")]
        ExtensionId::Damage => handlers::extensions::handle_damage_request(state, data, seq),
        #[cfg(feature = "ext-render")]
        ExtensionId::Present => handlers::extensions::handle_present_request(state, data, seq),

        // -- ext-glx ----------------------------------------------------------
        #[cfg(feature = "ext-glx")]
        ExtensionId::Glx => handlers::extensions::handle_glx_request(state, data, seq),

        // -- ext-media --------------------------------------------------------
        #[cfg(feature = "ext-media")]
        ExtensionId::XVideo => handlers::extensions::handle_xvideo_request(state, data, seq),
        #[cfg(feature = "ext-media")]
        ExtensionId::Dbe => handlers::extensions::handle_dbe_request(state, data, seq),

        // -- ext-compat -------------------------------------------------------
        #[cfg(feature = "ext-compat")]
        ExtensionId::Dpms => handlers::extensions::handle_dpms_request(state, data, seq),
        #[cfg(feature = "ext-compat")]
        ExtensionId::ScreenSaver => {
            handlers::extensions::handle_screen_saver_request(state, data, seq)
        }
        #[cfg(feature = "ext-compat")]
        ExtensionId::VidMode => handlers::extensions::handle_vidmode_request(state, data, seq),
        #[cfg(feature = "ext-compat")]
        ExtensionId::Record => handlers::record::handle_record_request(state, data, seq),
        #[cfg(feature = "ext-compat")]
        ExtensionId::Security => handlers::extensions::handle_security_request(state, data, seq),
        #[cfg(feature = "ext-compat")]
        ExtensionId::Xinerama => handlers::extensions::handle_xinerama_request(state, data, seq),

        // Unreachable when all features are enabled, but required for
        // exhaustiveness when some extension groups are compiled out.
        #[allow(unreachable_patterns)]
        _ => {
            warn!("Extension {:?} compiled out", id);
            build_error(REQUEST_ERROR, seq, data[0] as u32, data[0], data[1] as u16)
        }
    }
}
