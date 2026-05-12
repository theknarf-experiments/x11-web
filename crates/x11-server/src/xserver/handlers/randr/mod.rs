//! RANDR extension handler — multi-monitor support.

mod crtc;
mod output;
mod screen;

use tracing::{debug, info};
use x11rb_protocol::protocol::randr::{
    ADD_OUTPUT_MODE_REQUEST, CHANGE_OUTPUT_PROPERTY_REQUEST, CHANGE_PROVIDER_PROPERTY_REQUEST,
    CONFIGURE_OUTPUT_PROPERTY_REQUEST, CONFIGURE_PROVIDER_PROPERTY_REQUEST, CREATE_LEASE_REQUEST,
    CREATE_MODE_REQUEST, DELETE_MONITOR_REQUEST, DELETE_OUTPUT_MODE_REQUEST,
    DELETE_OUTPUT_PROPERTY_REQUEST, DELETE_PROVIDER_PROPERTY_REQUEST, DESTROY_MODE_REQUEST,
    FREE_LEASE_REQUEST, GET_CRTC_GAMMA_REQUEST, GET_CRTC_GAMMA_SIZE_REQUEST, GET_CRTC_INFO_REQUEST,
    GET_CRTC_TRANSFORM_REQUEST, GET_MONITORS_REQUEST, GET_OUTPUT_INFO_REQUEST,
    GET_OUTPUT_PRIMARY_REQUEST, GET_OUTPUT_PROPERTY_REQUEST, GET_PANNING_REQUEST,
    GET_PROVIDERS_REQUEST, GET_PROVIDER_INFO_REQUEST, GET_PROVIDER_PROPERTY_REQUEST,
    GET_SCREEN_INFO_REQUEST, GET_SCREEN_RESOURCES_CURRENT_REQUEST, GET_SCREEN_RESOURCES_REQUEST,
    GET_SCREEN_SIZE_RANGE_REQUEST, LIST_OUTPUT_PROPERTIES_REQUEST,
    LIST_PROVIDER_PROPERTIES_REQUEST, QUERY_OUTPUT_PROPERTY_REQUEST,
    QUERY_PROVIDER_PROPERTY_REQUEST, QUERY_VERSION_REQUEST, SELECT_INPUT_REQUEST,
    SET_CRTC_CONFIG_REQUEST, SET_CRTC_GAMMA_REQUEST, SET_CRTC_TRANSFORM_REQUEST,
    SET_MONITOR_REQUEST, SET_OUTPUT_PRIMARY_REQUEST, SET_PANNING_REQUEST,
    SET_PROVIDER_OFFLOAD_SINK_REQUEST, SET_PROVIDER_OUTPUT_SOURCE_REQUEST,
    SET_SCREEN_CONFIG_REQUEST, SET_SCREEN_SIZE_REQUEST,
};

use super::super::client::ClientState;
use super::parse_minor;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::randr::{
    QueryVersionReply as RandrQueryVersionReply, SetConfig, SetScreenConfigReply,
};
use x11rb_protocol::protocol::render::SubPixel;

pub(crate) fn handle_randr_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("RANDR minor={minor}");

    match minor {
        // ---------------------------------------------------------------
        // RRQueryVersion (0)
        // ---------------------------------------------------------------
        QUERY_VERSION_REQUEST => serialize_reply(
            &RandrQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: 1,
                minor_version: 5,
            },
            state.byte_order(),
        ),

        // ---------------------------------------------------------------
        // RRSetScreenConfig (2) — legacy screen configuration
        // ---------------------------------------------------------------
        SET_SCREEN_CONFIG_REQUEST => {
            use x11rb_protocol::protocol::randr::SetScreenConfigRequest;
            let req = parse_minor!(SetScreenConfigRequest, data, state, seq, 140, 2);
            let config_timestamp = req.config_timestamp;

            // Check config timestamp — if it doesn't match, reply InvalidConfigTime
            let status = if config_timestamp != 0
                && config_timestamp != state.randr_config_timestamp
            {
                SetConfig::INVALID_CONFIG_TIME
            } else {
                SetConfig::SUCCESS
            };

            debug!(
                "RRSetScreenConfig: status={} config_ts={config_timestamp}",
                u8::from(status)
            );
            serialize_reply(
                &SetScreenConfigReply {
                    status,
                    sequence: seq,
                    length: 0,
                    new_timestamp: state.timestamp(),
                    config_timestamp: state.randr_config_timestamp,
                    root: state.root_window,
                    subpixel_order: SubPixel::UNKNOWN,
                },
                state.byte_order(),
            )
        }

        // Screen operations
        SELECT_INPUT_REQUEST => screen::handle_select_input(state, data, seq),
        GET_SCREEN_INFO_REQUEST => screen::handle_get_screen_info(state, data, seq),
        GET_SCREEN_SIZE_RANGE_REQUEST => screen::handle_get_screen_size_range(state, data, seq),
        SET_SCREEN_SIZE_REQUEST => screen::handle_set_screen_size(state, data, seq),
        GET_SCREEN_RESOURCES_REQUEST => screen::build_screen_resources_reply(state, seq),
        GET_SCREEN_RESOURCES_CURRENT_REQUEST => screen::build_screen_resources_reply(state, seq),

        // Output operations
        GET_OUTPUT_INFO_REQUEST => output::handle_get_output_info(state, data, seq),
        LIST_OUTPUT_PROPERTIES_REQUEST => output::handle_list_output_properties(state, data, seq),
        QUERY_OUTPUT_PROPERTY_REQUEST => output::handle_query_output_property(state, data, seq),
        CONFIGURE_OUTPUT_PROPERTY_REQUEST => {
            output::handle_configure_output_property(state, data, seq)
        }
        CHANGE_OUTPUT_PROPERTY_REQUEST => output::handle_change_output_property(state, data, seq),
        DELETE_OUTPUT_PROPERTY_REQUEST => output::handle_delete_output_property(state, data, seq),
        GET_OUTPUT_PROPERTY_REQUEST => output::handle_get_output_property(state, data, seq),
        CREATE_MODE_REQUEST => output::handle_create_mode(state, data, seq),
        DESTROY_MODE_REQUEST => output::handle_destroy_mode(state, data, seq),
        ADD_OUTPUT_MODE_REQUEST => output::handle_add_output_mode(state, data, seq),
        DELETE_OUTPUT_MODE_REQUEST => output::handle_delete_output_mode(state, data, seq),
        SET_OUTPUT_PRIMARY_REQUEST => output::handle_set_output_primary(state, data, seq),
        GET_OUTPUT_PRIMARY_REQUEST => output::handle_get_output_primary(state, data, seq),
        GET_PROVIDERS_REQUEST => output::handle_get_providers(state, data, seq),
        GET_PROVIDER_INFO_REQUEST => output::handle_get_provider_info(state, data, seq),
        SET_PROVIDER_OFFLOAD_SINK_REQUEST => {
            output::handle_set_provider_offload_sink(state, data, seq)
        }
        SET_PROVIDER_OUTPUT_SOURCE_REQUEST => {
            output::handle_set_provider_output_source(state, data, seq)
        }
        LIST_PROVIDER_PROPERTIES_REQUEST => {
            output::handle_list_provider_properties(state, data, seq)
        }
        QUERY_PROVIDER_PROPERTY_REQUEST => output::handle_query_provider_property(state, data, seq),
        CONFIGURE_PROVIDER_PROPERTY_REQUEST => {
            output::handle_configure_provider_property(state, data, seq)
        }
        CHANGE_PROVIDER_PROPERTY_REQUEST => {
            output::handle_change_provider_property(state, data, seq)
        }
        DELETE_PROVIDER_PROPERTY_REQUEST => {
            output::handle_delete_provider_property(state, data, seq)
        }
        GET_PROVIDER_PROPERTY_REQUEST => output::handle_get_provider_property(state, data, seq),
        GET_MONITORS_REQUEST => output::handle_get_monitors(state, data, seq),
        SET_MONITOR_REQUEST => output::handle_set_monitor(state, data, seq),
        DELETE_MONITOR_REQUEST => output::handle_delete_monitor(state, data, seq),
        CREATE_LEASE_REQUEST => output::handle_create_lease(state, data, seq),
        FREE_LEASE_REQUEST => output::handle_free_lease(state, data, seq),

        // CRTC operations
        GET_CRTC_INFO_REQUEST => crtc::handle_get_crtc_info(state, data, seq),
        SET_CRTC_CONFIG_REQUEST => crtc::handle_set_crtc_config(state, data, seq),
        GET_CRTC_GAMMA_SIZE_REQUEST => crtc::handle_get_crtc_gamma_size(state, data, seq),
        GET_CRTC_GAMMA_REQUEST => crtc::handle_get_crtc_gamma(state, data, seq),
        SET_CRTC_GAMMA_REQUEST => crtc::handle_set_crtc_gamma(state, data, seq),
        SET_CRTC_TRANSFORM_REQUEST => crtc::handle_set_crtc_transform(state, data, seq),
        GET_CRTC_TRANSFORM_REQUEST => crtc::handle_get_crtc_transform(state, data, seq),
        GET_PANNING_REQUEST => crtc::handle_get_panning(state, data, seq),
        SET_PANNING_REQUEST => crtc::handle_set_panning(state, data, seq),

        _ => {
            info!("Unhandled RANDR minor opcode: {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                140,
                minor as u16,
            )
        }
    }
}
