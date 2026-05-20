//! Output and provider RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use super::super::super::types::{OutputPropertyConfig, PropertyValue, RandrMode, RandrMonitor};
use super::super::parse_or_void;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::randr::{
    AddOutputModeRequest, ChangeOutputPropertyRequest, Connection,
    ConfigureOutputPropertyRequest, CreateModeReply, CreateModeRequest, DeleteMonitorRequest,
    DeleteOutputModeRequest, DeleteOutputPropertyRequest, DestroyModeRequest, GetOutputInfoReply,
    GetOutputInfoRequest, GetOutputPrimaryReply, GetOutputPropertyReply, GetOutputPropertyRequest,
    GetProviderInfoReply, GetProviderInfoRequest, GetProviderPropertyReply, GetProvidersReply,
    ListOutputPropertiesReply, ListOutputPropertiesRequest, ListProviderPropertiesReply,
    ProviderCapability, QueryOutputPropertyReply, QueryOutputPropertyRequest,
    QueryProviderPropertyReply, SetConfig, SetMonitorRequest, SetOutputPrimaryRequest,
    SetProviderOffloadSinkRequest, SetProviderOutputSourceRequest,
};
use x11rb_protocol::protocol::render::SubPixel;

/// RRGetOutputInfo (9).
pub(crate) fn handle_get_output_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let output_id = GetOutputInfoRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.output)
        .unwrap_or(0);
    build_output_info_reply(state, seq, output_id)
}

/// RRListOutputProperties (10).
pub(crate) fn handle_list_output_properties(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let output_id =
        ListOutputPropertiesRequest::try_parse_request(request_header(data), &data[4..])
            .map(|r| r.output)
            .unwrap_or(0);
    let atoms: Vec<u32> = state
        .randr_find_output(output_id)
        .map(|o| o.properties.keys().copied().collect())
        .unwrap_or_default();
    serialize_var_reply(
        &ListOutputPropertiesReply {
            sequence: seq,
            length: 0,
            atoms,
        },
        state.byte_order(),
    )
}

/// RRQueryOutputProperty (11).
pub(crate) fn handle_query_output_property(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let empty = QueryOutputPropertyReply {
        sequence: seq,
        pending: false,
        range: false,
        immutable: false,
        valid_values: Vec::new(),
    };
    if data.len() < 12 {
        return serialize_var_reply(&empty, state.byte_order());
    }
    let Ok(req) = QueryOutputPropertyRequest::try_parse_request(request_header(data), &data[4..])
    else {
        return serialize_var_reply(&empty, state.byte_order());
    };
    let output_id = req.output;
    let property_atom = req.property;

    let (pending, range, immutable, values) =
        if let Some(output) = state.randr_find_output(output_id) {
            if let Some(config) = output.property_configs.get(&property_atom) {
                (config.pending, config.range, false, config.values.clone())
            } else {
                let atom_name = state.get_atom_name(property_atom).unwrap_or_default();
                match atom_name.as_str() {
                    "Backlight" | "BACKLIGHT" => (false, true, false, vec![0, 100]),
                    _ => (false, false, false, Vec::new()),
                }
            }
        } else {
            (false, false, false, Vec::new())
        };

    serialize_var_reply(
        &QueryOutputPropertyReply {
            sequence: seq,
            pending,
            range,
            immutable,
            valid_values: values.into_iter().map(|v| v as i32).collect(),
        },
        state.byte_order(),
    )
}

/// RRConfigureOutputProperty (12).
pub(crate) fn handle_configure_output_property(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 14 {
        let req = parse_or_void!(ConfigureOutputPropertyRequest, data, state);
        let output_id = req.output;
        let property_atom = req.property;
        let pending = req.pending;
        let range = req.range;
        let values: Vec<u32> = req.values.iter().map(|&v| v as u32).collect();
        if let Some(output) = state.randr_find_output_mut(output_id) {
            output.property_configs.insert(
                property_atom,
                OutputPropertyConfig {
                    pending,
                    range,
                    values,
                },
            );
        }
        debug!("RRConfigureOutputProperty output={output_id} property={property_atom} pending={pending} range={range}");
    }
    Vec::new()
}

/// RRChangeOutputProperty (13).
pub(crate) fn handle_change_output_property(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 24 {
        let req = parse_or_void!(ChangeOutputPropertyRequest, data, state);
        let output_id = req.output;
        let property = req.property;
        let prop_type = req.type_;
        let format = req.format;
        let prop_data = req.data.into_owned();
        if let Some(output) = state.randr_find_output_mut(output_id) {
            output.properties.insert(
                property,
                PropertyValue {
                    prop_type,
                    format,
                    data: prop_data,
                },
            );
        }
    }
    Vec::new()
}

/// RRDeleteOutputProperty (14).
pub(crate) fn handle_delete_output_property(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(DeleteOutputPropertyRequest, data, state);
        let output_id = req.output;
        let property = req.property;
        if let Some(output) = state.randr_find_output_mut(output_id) {
            output.properties.remove(&property);
            output.property_configs.remove(&property);
        }
        debug!("RRDeleteOutputProperty output={output_id} property={property}");
    }
    Vec::new()
}

/// RRGetOutputProperty (15).
pub(crate) fn handle_get_output_property(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    build_get_output_property_reply(state, data, seq)
}

/// RRCreateMode (16).
pub(crate) fn handle_create_mode(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let empty = CreateModeReply {
        sequence: seq,
        length: 0,
        mode: 0,
    };
    if data.len() < 40 {
        return serialize_reply(&empty, state.byte_order());
    }

    let Ok(req) = CreateModeRequest::try_parse_request(request_header(data), &data[4..]) else {
        return serialize_reply(&empty, state.byte_order());
    };

    let mi = &req.mode_info;
    let width = mi.width;
    let height = mi.height;
    let dot_clock = mi.dot_clock;
    let h_sync_start = mi.hsync_start;
    let h_sync_end = mi.hsync_end;
    let h_total = mi.htotal;
    let v_sync_start = mi.vsync_start;
    let v_sync_end = mi.vsync_end;
    let v_total = mi.vtotal;
    let flags: u32 = mi.mode_flags.into();

    let name = if !req.name.is_empty() {
        String::from_utf8_lossy(&req.name).to_string()
    } else {
        format!("{}x{}", width, height)
    };

    let mode_id = state.randr.next_mode_id;
    state.randr.next_mode_id += 1;

    let mode = RandrMode {
        id: mode_id,
        name,
        width,
        height,
        dot_clock,
        h_sync_start,
        h_sync_end,
        h_total,
        v_sync_start,
        v_sync_end,
        v_total,
        flags,
    };
    state.randr.modes.push(mode);

    info!("RRCreateMode: {width}x{height} -> mode_id={mode_id}");

    serialize_reply(
        &CreateModeReply {
            sequence: seq,
            length: 0,
            mode: mode_id,
        },
        state.byte_order(),
    )
}

/// RRDestroyMode (17).
pub(crate) fn handle_destroy_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let req = parse_or_void!(DestroyModeRequest, data, state);
        let mode_id = req.mode;
        state.randr.modes.retain(|m| m.id != mode_id);
        debug!("RRDestroyMode mode_id={mode_id}");
    }
    Vec::new()
}

/// RRAddOutputMode (18).
pub(crate) fn handle_add_output_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(AddOutputModeRequest, data, state);
        let output_id = req.output;
        let mode_id = req.mode;
        if let Some(output) = state.randr_find_output_mut(output_id) {
            if !output.modes.contains(&mode_id) {
                output.modes.push(mode_id);
            }
        }
        debug!("RRAddOutputMode output={output_id} mode={mode_id}");
    }
    Vec::new()
}

/// RRDeleteOutputMode (19).
pub(crate) fn handle_delete_output_mode(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(DeleteOutputModeRequest, data, state);
        let output_id = req.output;
        let mode_id = req.mode;
        if let Some(output) = state.randr_find_output_mut(output_id) {
            output.modes.retain(|&m| m != mode_id);
        }
        debug!("RRDeleteOutputMode output={output_id} mode={mode_id}");
    }
    Vec::new()
}

/// RRSetOutputPrimary (30).
pub(crate) fn handle_set_output_primary(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(SetOutputPrimaryRequest, data, state);
        let output_id = req.output;
        state.randr.primary_output = output_id;
        debug!("RRSetOutputPrimary output={output_id}");
    }
    Vec::new()
}

/// RRGetOutputPrimary (31).
pub(crate) fn handle_get_output_primary(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let primary_output = if state.randr.primary_output != 0 {
        state.randr.primary_output
    } else {
        state.randr.outputs.first().map(|o| o.id).unwrap_or(0)
    };
    serialize_reply(
        &GetOutputPrimaryReply {
            sequence: seq,
            length: 0,
            output: primary_output,
        },
        state.byte_order(),
    )
}

/// RRGetProviders (32).
pub(crate) fn handle_get_providers(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let providers: Vec<u32> = state.randr.providers.iter().map(|p| p.id).collect();
    serialize_var_reply(
        &GetProvidersReply {
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            providers,
        },
        state.byte_order(),
    )
}

/// RRGetProviderInfo (33).
pub(crate) fn handle_get_provider_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let provider_id = GetProviderInfoRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.provider)
        .unwrap_or(0);
    build_provider_info_reply(state, seq, provider_id)
}

/// RRSetProviderOffloadSink (34): Set a provider as an offload sink.
pub(crate) fn handle_set_provider_offload_sink(
    _state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 16 {
        if let Ok(req) =
            SetProviderOffloadSinkRequest::try_parse_request(request_header(data), &data[4..])
        {
            let provider = req.provider;
            let sink = req.sink_provider;
            let config_ts = req.config_timestamp;
            debug!(
                "RRSetProviderOffloadSink: provider={provider:#x} sink={sink:#x} ts={config_ts}"
            );
        }
    }
    Vec::new()
}

/// RRSetProviderOutputSource (35).
pub(crate) fn handle_set_provider_output_source(
    _state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 16 {
        if let Ok(req) =
            SetProviderOutputSourceRequest::try_parse_request(request_header(data), &data[4..])
        {
            let provider = req.provider;
            let source = req.source_provider;
            let config_ts = req.config_timestamp;
            debug!(
                "RRSetProviderOutputSource: provider={provider:#x} source={source:#x} ts={config_ts}"
            );
        }
    }
    Vec::new()
}

/// RRListProviderProperties (36).
pub(crate) fn handle_list_provider_properties(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    serialize_var_reply(
        &ListProviderPropertiesReply {
            sequence: seq,
            length: 0,
            atoms: Vec::new(),
        },
        state.byte_order(),
    )
}

/// RRQueryProviderProperty (37).
pub(crate) fn handle_query_provider_property(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    serialize_var_reply(
        &QueryProviderPropertyReply {
            sequence: seq,
            pending: false,
            range: false,
            immutable: false,
            valid_values: Vec::new(),
        },
        state.byte_order(),
    )
}

/// RRConfigureProviderProperty (38).
pub(crate) fn handle_configure_provider_property(
    _state: &mut ClientState,
    _data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    debug!("RRConfigureProviderProperty (no-op)");
    Vec::new()
}

/// RRChangeProviderProperty (39).
pub(crate) fn handle_change_provider_property(
    _state: &mut ClientState,
    _data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    debug!("RRChangeProviderProperty (no-op)");
    Vec::new()
}

/// RRDeleteProviderProperty (40).
pub(crate) fn handle_delete_provider_property(
    _state: &mut ClientState,
    _data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    debug!("RRDeleteProviderProperty (no-op)");
    Vec::new()
}

/// RRGetProviderProperty (41).
pub(crate) fn handle_get_provider_property(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    serialize_var_reply(
        &GetProviderPropertyReply {
            format: 0,
            sequence: seq,
            length: 0,
            type_: 0,
            bytes_after: 0,
            num_items: 0,
            data: Vec::new(),
        },
        state.byte_order(),
    )
}

/// RRGetMonitors (42).
pub(crate) fn handle_get_monitors(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    super::screen::build_get_monitors_reply(state, seq)
}

/// RRSetMonitor (43).
pub(crate) fn handle_set_monitor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 28 {
        let req = parse_or_void!(SetMonitorRequest, data, state);
        let mi = &req.monitorinfo;
        let name_atom = mi.name;
        let primary = mi.primary;
        let automatic = mi.automatic;
        let x = mi.x;
        let y = mi.y;
        let width = mi.width;
        let height = mi.height;
        let output_ids: Vec<u32> = mi.outputs.clone();

        state.randr.monitors.retain(|m| m.name_atom != name_atom);
        state.randr.monitors.push(RandrMonitor {
            name_atom,
            primary,
            automatic,
            x,
            y,
            width,
            height,
            output_ids,
        });
        debug!("RRSetMonitor name_atom={name_atom} {width}x{height}+{x}+{y}");
    }
    Vec::new()
}

/// RRDeleteMonitor (44).
pub(crate) fn handle_delete_monitor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(DeleteMonitorRequest, data, state);
        let name_atom = req.name;
        state.randr.monitors.retain(|m| m.name_atom != name_atom);
        debug!("RRDeleteMonitor name_atom={name_atom}");
    }
    Vec::new()
}

/// RRCreateLease (45).
pub(crate) fn handle_create_lease(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let minor = 45u8;
    debug!("RRCreateLease: not supported on virtual display");
    crate::xserver::core::build_error(
        crate::xserver::core::ACCESS_ERROR,
        seq,
        0,
        140,
        minor as u16,
    )
}

/// RRFreeLease (46).
pub(crate) fn handle_free_lease(_state: &mut ClientState, _data: &[u8], _seq: u16) -> Vec<u8> {
    debug!("RRFreeLease (no-op)");
    Vec::new()
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Build the reply for RRGetOutputInfo.
fn build_output_info_reply(state: &ClientState, seq: u16, output_id: u32) -> Vec<u8> {
    let output = match state.randr_find_output(output_id) {
        Some(o) => o.clone(),
        None => {
            return serialize_var_reply(
                &GetOutputInfoReply {
                    status: SetConfig::SUCCESS,
                    sequence: seq,
                    length: 0,
                    timestamp: 0,
                    crtc: 0,
                    mm_width: 0,
                    mm_height: 0,
                    connection: Connection::DISCONNECTED,
                    subpixel_order: SubPixel::UNKNOWN,
                    num_preferred: 0,
                    crtcs: Vec::new(),
                    modes: Vec::new(),
                    clones: Vec::new(),
                    name: Vec::new(),
                },
                state.byte_order(),
            );
        }
    };

    serialize_var_reply(
        &GetOutputInfoReply {
            status: SetConfig::SUCCESS,
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            crtc: output.crtc_id,
            mm_width: output.mm_width,
            mm_height: output.mm_height,
            connection: Connection::from(output.connection_status),
            subpixel_order: SubPixel::UNKNOWN,
            num_preferred: 1,
            crtcs: output.possible_crtcs.clone(),
            modes: output.modes.clone(),
            clones: Vec::new(),
            name: output.name.as_bytes().to_vec(),
        },
        state.byte_order(),
    )
}

/// Build the reply for RRGetOutputProperty.
fn build_get_output_property_reply(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let empty_reply = |state: &ClientState| {
        serialize_var_reply(
            &GetOutputPropertyReply {
                format: 0,
                sequence: seq,
                length: 0,
                type_: 0,
                bytes_after: 0,
                num_items: 0,
                data: Vec::new(),
            },
            state.byte_order(),
        )
    };

    if data.len() < 24 {
        return empty_reply(state);
    }

    let Ok(req) = GetOutputPropertyRequest::try_parse_request(request_header(data), &data[4..])
    else {
        return empty_reply(state);
    };
    let output_id = req.output;
    let property = req.property;
    let req_type = req.type_;
    let long_offset = req.long_offset as usize;
    let long_length = req.long_length as usize;

    let output = match state.randr_find_output(output_id) {
        Some(o) => o,
        None => return empty_reply(state),
    };

    let prop = match output.properties.get(&property) {
        Some(p) => p,
        None => return empty_reply(state),
    };

    // Type mismatch: return type + bytes_after but no data.
    if req_type != 0 && req_type != prop.prop_type {
        return serialize_var_reply(
            &GetOutputPropertyReply {
                format: 0,
                sequence: seq,
                length: 0,
                type_: prop.prop_type,
                bytes_after: prop.data.len() as u32,
                num_items: 0,
                data: Vec::new(),
            },
            state.byte_order(),
        );
    }

    let bytes_per_item = match prop.format {
        8 => 1usize,
        16 => 2,
        32 => 4,
        _ => 1,
    };
    let byte_offset = long_offset * 4;
    let max_bytes = long_length * 4;

    let (slice_start, slice_end) = if byte_offset >= prop.data.len() {
        (0, 0)
    } else {
        let end = (byte_offset + max_bytes).min(prop.data.len());
        (byte_offset, end)
    };

    let returned_data = prop.data[slice_start..slice_end].to_vec();
    let bytes_after = if slice_end < prop.data.len() {
        prop.data.len() - slice_end
    } else {
        0
    };
    let num_items = returned_data.len() / bytes_per_item;

    serialize_var_reply(
        &GetOutputPropertyReply {
            format: prop.format,
            sequence: seq,
            length: 0,
            type_: prop.prop_type,
            bytes_after: bytes_after as u32,
            num_items: num_items as u32,
            data: returned_data,
        },
        state.byte_order(),
    )
}

/// Build the reply for RRGetProviderInfo.
fn build_provider_info_reply(state: &ClientState, seq: u16, provider_id: u32) -> Vec<u8> {
    let provider = match state.randr.providers.iter().find(|p| p.id == provider_id) {
        Some(p) => p.clone(),
        None => {
            return serialize_var_reply(
                &GetProviderInfoReply {
                    status: 0,
                    sequence: seq,
                    length: 0,
                    timestamp: 0,
                    capabilities: ProviderCapability::from(0u8),
                    crtcs: Vec::new(),
                    outputs: Vec::new(),
                    associated_providers: Vec::new(),
                    associated_capability: Vec::new(),
                    name: Vec::new(),
                },
                state.byte_order(),
            );
        }
    };

    serialize_var_reply(
        &GetProviderInfoReply {
            status: 0,
            sequence: seq,
            length: 0,
            timestamp: state.timestamp(),
            capabilities: ProviderCapability::from(provider.capabilities),
            crtcs: provider.crtcs.clone(),
            outputs: provider.outputs.clone(),
            associated_providers: Vec::new(),
            associated_capability: Vec::new(),
            name: provider.name.as_bytes().to_vec(),
        },
        state.byte_order(),
    )
}
