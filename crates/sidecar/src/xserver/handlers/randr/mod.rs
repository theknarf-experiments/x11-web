//! RANDR extension handler — multi-monitor support.

mod crtc;
mod output;
mod screen;

use tracing::{debug, info};

use super::super::client::ClientState;
use crate::xserver::core::require_len;

pub(crate) fn handle_randr_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("RANDR minor={minor}");

    match minor {
        // ---------------------------------------------------------------
        // RRQueryVersion (0)
        // ---------------------------------------------------------------
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 1);  // major version
            state.write_u32(&mut reply, 12, 5); // minor version
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRSetScreenConfig (2) — legacy screen configuration
        // ---------------------------------------------------------------
        2 => {
            require_len!(data, 24, seq, 140, 2, state.msb_first);

            let _drawable = state.read_u32(data, 4);
            let _timestamp = state.read_u32(data, 8);
            let config_timestamp = state.read_u32(data, 12);
            let _size_index = state.read_u16(data, 16);
            let _rotation = state.read_u16(data, 18);

            // Check config timestamp — if it doesn't match, reply InvalidConfigTime
            let status = if config_timestamp != 0
                && config_timestamp != state.randr_config_timestamp
            {
                2 // InvalidConfigTime
            } else {
                0 // Success
            };

            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = status;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, state.timestamp());
            state.write_u32(&mut reply, 12, state.randr_config_timestamp);
            state.write_u32(&mut reply, 16, state.root_window);
            // subpixel_order at byte 20 (0 = Unknown, already zero)

            debug!("RRSetScreenConfig: status={status} config_ts={config_timestamp}");
            reply.to_vec()
        }

        // Screen operations
        4 => screen::handle_select_input(state, data, seq),
        5 => screen::handle_get_screen_info(state, data, seq),
        6 => screen::handle_get_screen_size_range(state, data, seq),
        7 => screen::handle_set_screen_size(state, data, seq),
        8 => screen::build_screen_resources_reply(state, seq),
        25 => screen::build_screen_resources_reply(state, seq),

        // Output operations
        9 => output::handle_get_output_info(state, data, seq),
        10 => output::handle_list_output_properties(state, data, seq),
        11 => output::handle_query_output_property(state, data, seq),
        12 => output::handle_configure_output_property(state, data, seq),
        13 => output::handle_change_output_property(state, data, seq),
        14 => output::handle_delete_output_property(state, data, seq),
        15 => output::handle_get_output_property(state, data, seq),
        16 => output::handle_create_mode(state, data, seq),
        17 => output::handle_destroy_mode(state, data, seq),
        18 => output::handle_add_output_mode(state, data, seq),
        19 => output::handle_delete_output_mode(state, data, seq),
        30 => output::handle_set_output_primary(state, data, seq),
        31 => output::handle_get_output_primary(state, data, seq),
        32 => output::handle_get_providers(state, data, seq),
        33 => output::handle_get_provider_info(state, data, seq),
        34 => output::handle_set_provider_offload_sink(state, data, seq),
        35 => output::handle_set_provider_output_source(state, data, seq),
        36 => output::handle_list_provider_properties(state, data, seq),
        37 => output::handle_query_provider_property(state, data, seq),
        38 => output::handle_configure_provider_property(state, data, seq),
        39 => output::handle_change_provider_property(state, data, seq),
        40 => output::handle_delete_provider_property(state, data, seq),
        41 => output::handle_get_provider_property(state, data, seq),
        42 => output::handle_get_monitors(state, data, seq),
        43 => output::handle_set_monitor(state, data, seq),
        44 => output::handle_delete_monitor(state, data, seq),
        45 => output::handle_create_lease(state, data, seq),
        46 => output::handle_free_lease(state, data, seq),

        // CRTC operations
        20 => crtc::handle_get_crtc_info(state, data, seq),
        21 => crtc::handle_set_crtc_config(state, data, seq),
        22 => crtc::handle_get_crtc_gamma_size(state, data, seq),
        23 => crtc::handle_get_crtc_gamma(state, data, seq),
        24 => crtc::handle_set_crtc_gamma(state, data, seq),
        26 => crtc::handle_set_crtc_transform(state, data, seq),
        27 => crtc::handle_get_panning(state, data, seq),
        28 => crtc::handle_set_panning(state, data, seq),
        29 => crtc::handle_get_crtc_transform(state, data, seq),

        _ => {
            info!("Unhandled RANDR minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                140, minor as u16, state.msb_first,
            )
        }
    }
}
