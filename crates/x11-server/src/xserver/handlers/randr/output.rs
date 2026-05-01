//! Output and provider RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use super::super::super::types::{OutputPropertyConfig, PropertyValue, RandrMode, RandrMonitor};
use super::super::parse_or_void;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::randr::{
    AddOutputModeRequest, ChangeOutputPropertyRequest, ConfigureOutputPropertyRequest,
    CreateModeRequest, DeleteMonitorRequest, DeleteOutputModeRequest, DeleteOutputPropertyRequest,
    DestroyModeRequest, GetOutputInfoRequest, GetOutputPropertyRequest, GetProviderInfoRequest,
    ListOutputPropertiesRequest, QueryOutputPropertyRequest, SetMonitorRequest,
    SetOutputPrimaryRequest, SetProviderOffloadSinkRequest, SetProviderOutputSourceRequest,
};

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
    build_list_output_properties_reply(state, seq, output_id)
}

/// RRQueryOutputProperty (11).
pub(crate) fn handle_query_output_property(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    if data.len() < 12 {
        return ReplyBuf::fixed(seq, state.msb_first).build();
    }
    let Ok(req) = QueryOutputPropertyRequest::try_parse_request(request_header(data), &data[4..])
    else {
        return ReplyBuf::fixed(seq, state.msb_first).build();
    };
    let output_id = req.output;
    let property_atom = req.property;

    // Check if the output has an explicit property config (set by ConfigureOutputProperty)
    let (pending, range, immutable, values) =
        if let Some(output) = state.randr_find_output(output_id) {
            if let Some(config) = output.property_configs.get(&property_atom) {
                (config.pending, config.range, false, config.values.clone())
            } else {
                // Fall back to well-known property defaults based on atom name
                let atom_name = state.get_atom_name(property_atom).unwrap_or_default();
                match atom_name.as_str() {
                    "Backlight" | "BACKLIGHT" => {
                        // Range constraint: min=0, max=100
                        (false, true, false, vec![0, 100])
                    }
                    _ => {
                        // Unknown property: no constraints
                        (false, false, false, Vec::new())
                    }
                }
            }
        } else {
            (false, false, false, Vec::new())
        };

    let num_values = values.len() as u32;
    let extra_bytes = (num_values as usize) * 4;
    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_u8(8, if pending { 1 } else { 0 })
        .set_u8(9, if range { 1 } else { 0 })
        .set_u8(10, if immutable { 1 } else { 0 });
    // values follow the 32-byte header
    for (i, &val) in values.iter().enumerate() {
        let off = 32 + i * 4;
        reply = reply.set_u32(off, val);
    }
    reply.build()
}

/// RRConfigureOutputProperty (12).
pub(crate) fn handle_configure_output_property(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 14 {
        let req = parse_or_void!(ConfigureOutputPropertyRequest, data);
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
        let req = parse_or_void!(ChangeOutputPropertyRequest, data);
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
        let req = parse_or_void!(DeleteOutputPropertyRequest, data);
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
    if data.len() < 40 {
        return ReplyBuf::fixed(seq, state.msb_first).build();
    }

    let Ok(req) = CreateModeRequest::try_parse_request(request_header(data), &data[4..]) else {
        return ReplyBuf::fixed(seq, state.msb_first).build();
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

    let mode_id = state.randr_next_mode_id;
    state.randr_next_mode_id += 1;

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
    state.randr_modes.push(mode);

    info!("RRCreateMode: {width}x{height} -> mode_id={mode_id}");

    // Reply: 32 bytes, mode_id at offset 8.
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u32(8, mode_id)
        .build()
}

/// RRDestroyMode (17).
pub(crate) fn handle_destroy_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let req = parse_or_void!(DestroyModeRequest, data);
        let mode_id = req.mode;
        state.randr_modes.retain(|m| m.id != mode_id);
        debug!("RRDestroyMode mode_id={mode_id}");
    }
    Vec::new()
}

/// RRAddOutputMode (18).
pub(crate) fn handle_add_output_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let req = parse_or_void!(AddOutputModeRequest, data);
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
        let req = parse_or_void!(DeleteOutputModeRequest, data);
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
        let req = parse_or_void!(SetOutputPrimaryRequest, data);
        let output_id = req.output;
        state.randr_primary_output = output_id;
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
    // Use the stored primary, or fall back to the first output if
    // no primary has been explicitly set yet.
    let primary_output = if state.randr_primary_output != 0 {
        state.randr_primary_output
    } else {
        state.randr_outputs.first().map(|o| o.id).unwrap_or(0)
    };
    ReplyBuf::fixed(seq, state.msb_first)
        .set_u32(8, primary_output)
        .build()
}

/// RRGetProviders (32).
pub(crate) fn handle_get_providers(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let num_providers = state.randr_providers.len() as u16;
    let var_len = num_providers as usize * 4;
    let mut reply = ReplyBuf::with_extra(seq, var_len, state.msb_first)
        .set_u32(8, state.timestamp())
        .set_u16(12, num_providers);
    let mut off = 32;
    for p in &state.randr_providers {
        reply = reply.set_u32(off, p.id);
        off += 4;
    }
    reply.build()
}

/// RRGetProviderInfo (33).
pub(crate) fn handle_get_provider_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let provider_id = GetProviderInfoRequest::try_parse_request(request_header(data), &data[4..])
        .map(|r| r.provider)
        .unwrap_or(0);
    build_provider_info_reply(state, seq, provider_id)
}

/// RRSetProviderOffloadSink (34): Set a provider as an offload sink.
/// Virtual display has a single provider — accept and log for diagnostics.
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

/// RRSetProviderOutputSource (35): Set a provider as an output source.
/// Virtual display has a single provider — accept and log for diagnostics.
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
    ReplyBuf::fixed(seq, state.msb_first)
        // length = 0, num_atoms = 0
        .set_u16(8, 0)
        .build()
}

/// RRQueryProviderProperty (37).
pub(crate) fn handle_query_provider_property(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    // Reply with empty constraints (pending=0, range=0, immutable=0, no values)
    ReplyBuf::fixed(seq, state.msb_first).build()
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
    // Reply with type=None, format=0, length=0, bytes_after=0
    // All fields default to 0: type=None, bytes_after=0, num_items=0
    ReplyBuf::fixed(seq, state.msb_first).build()
}

/// RRGetMonitors (42).
pub(crate) fn handle_get_monitors(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    super::screen::build_get_monitors_reply(state, seq)
}

/// RRSetMonitor (43).
pub(crate) fn handle_set_monitor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 28 {
        let req = parse_or_void!(SetMonitorRequest, data);
        let mi = &req.monitorinfo;
        let name_atom = mi.name;
        let primary = mi.primary;
        let automatic = mi.automatic;
        let x = mi.x;
        let y = mi.y;
        let width = mi.width;
        let height = mi.height;
        let output_ids: Vec<u32> = mi.outputs.clone();

        // Remove any existing monitor with the same name
        state.randr_monitors.retain(|m| m.name_atom != name_atom);
        state.randr_monitors.push(RandrMonitor {
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
        let req = parse_or_void!(DeleteMonitorRequest, data);
        let name_atom = req.name;
        state.randr_monitors.retain(|m| m.name_atom != name_atom);
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
            // Return a minimal "disconnected" reply.
            return ReplyBuf::with_extra(seq, 24, state.msb_first)
                .set_data_byte(0)
                .set_u8(24, 1) // Disconnected
                .build();
        }
    };

    let output_name = output.name.as_bytes();
    let num_crtcs = output.possible_crtcs.len() as u16;
    let num_modes = output.modes.len() as u16;
    let num_clones: u16 = 0;

    let name_pad = (4 - (output_name.len() % 4)) % 4;
    let var_data = (num_crtcs as usize * 4)
        + (num_modes as usize * 4)
        + (num_clones as usize * 4)
        + output_name.len()
        + name_pad;
    let inline_header = 24; // bytes 8-31
    let extra_bytes = inline_header + var_data;

    let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
        .set_data_byte(0) // Success
        .set_u32(8, state.timestamp())
        .set_u32(12, output.crtc_id)
        .set_u32(16, output.mm_width)
        .set_u32(20, output.mm_height)
        .set_u8(24, output.connection_status)
        .set_u8(25, 0) // subpixel_order: Unknown
        .set_u16(26, num_crtcs)
        .set_u16(28, num_modes)
        .set_u16(30, 1) // num_preferred
        .set_u16(32, num_clones)
        .set_u16(34, output_name.len() as u16);

    let mut off = 36;
    // CRTC IDs (possible CRTCs)
    for &crtc_id in &output.possible_crtcs {
        reply = reply.set_u32(off, crtc_id);
        off += 4;
    }
    // Mode IDs
    for &mode_id in &output.modes {
        reply = reply.set_u32(off, mode_id);
        off += 4;
    }
    // Clone IDs (none)
    // Output name
    reply = reply.set_bytes(off, output_name);

    reply.build()
}

/// Build the reply for RRListOutputProperties.
fn build_list_output_properties_reply(state: &ClientState, seq: u16, output_id: u32) -> Vec<u8> {
    let atoms: Vec<u32> = state
        .randr_find_output(output_id)
        .map(|o| o.properties.keys().copied().collect())
        .unwrap_or_default();

    let num_atoms = atoms.len() as u16;
    let var_len = atoms.len() * 4;
    let mut reply = ReplyBuf::with_extra(seq, var_len, state.msb_first).set_u16(8, num_atoms);

    let mut off = 32;
    for atom in atoms {
        reply = reply.set_u32(off, atom);
        off += 4;
    }

    reply.build()
}

/// Build the reply for RRGetOutputProperty.
fn build_get_output_property_reply(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        return ReplyBuf::fixed(seq, state.msb_first).build();
    }

    let Ok(req) = GetOutputPropertyRequest::try_parse_request(request_header(data), &data[4..])
    else {
        return ReplyBuf::fixed(seq, state.msb_first).build();
    };
    let output_id = req.output;
    let property = req.property;
    let req_type = req.type_;
    let long_offset = req.long_offset as usize;
    let long_length = req.long_length as usize;

    let output = match state.randr_find_output(output_id) {
        Some(o) => o,
        None => {
            return ReplyBuf::fixed(seq, state.msb_first).build();
        }
    };

    let prop = match output.properties.get(&property) {
        Some(p) => p,
        None => {
            // Property not found.
            // type = None (0), format = 0, length = 0, bytes_after = 0
            return ReplyBuf::fixed(seq, state.msb_first).build();
        }
    };

    // Type mismatch check.
    if req_type != 0 && req_type != prop.prop_type {
        return ReplyBuf::fixed(seq, state.msb_first)
            .set_u32(8, prop.prop_type) // actual type
            // bytes_after = total data length
            .set_u32(12, prop.data.len() as u32)
            .build();
    }

    let bytes_per_item = match prop.format {
        8 => 1usize,
        16 => 2,
        32 => 4,
        _ => 1,
    };
    let _total_items = prop.data.len() / bytes_per_item;
    let byte_offset = long_offset * 4;
    let max_bytes = long_length * 4;

    let (slice_start, slice_end) = if byte_offset >= prop.data.len() {
        (0, 0)
    } else {
        let end = (byte_offset + max_bytes).min(prop.data.len());
        (byte_offset, end)
    };

    let returned_data = &prop.data[slice_start..slice_end];
    let bytes_after = if slice_end < prop.data.len() {
        prop.data.len() - slice_end
    } else {
        0
    };
    let num_items = returned_data.len() / bytes_per_item;

    let pad = (4 - (returned_data.len() % 4)) % 4;
    let var_len = returned_data.len() + pad;

    let reply = ReplyBuf::with_extra(seq, var_len, state.msb_first)
        .set_data_byte(prop.format)
        .set_u32(8, prop.prop_type)
        .set_u32(12, bytes_after as u32)
        .set_u32(16, num_items as u32)
        .set_bytes(32, returned_data);

    reply.build()
}

/// Build the reply for RRGetProviderInfo.
fn build_provider_info_reply(state: &ClientState, seq: u16, provider_id: u32) -> Vec<u8> {
    let provider = match state.randr_providers.iter().find(|p| p.id == provider_id) {
        Some(p) => p.clone(),
        None => {
            return ReplyBuf::fixed(seq, state.msb_first).build();
        }
    };

    let name_bytes = provider.name.as_bytes();
    let name_pad = (4 - (name_bytes.len() % 4)) % 4;
    let num_crtcs = provider.crtcs.len() as u16;
    let num_outputs = provider.outputs.len() as u16;
    let num_associated = 0u16;

    // GetProviderInfo reply layout:
    //   0-31: 32-byte fixed header (type, status, seq, length, timestamp,
    //         capabilities, num_crtcs, num_outputs, num_associated, name_len, pad)
    //   32+:  variable data (crtc IDs, output IDs, associated providers, name)
    let var_len = num_crtcs as usize * 4 + num_outputs as usize * 4 + name_bytes.len() + name_pad;

    let mut reply = ReplyBuf::with_extra(seq, var_len, state.msb_first)
        .set_u32(8, state.timestamp())
        .set_u32(12, provider.capabilities)
        .set_u16(16, num_crtcs)
        .set_u16(18, num_outputs)
        .set_u16(20, num_associated)
        .set_u16(22, name_bytes.len() as u16);

    let mut off = 32;
    for &cid in &provider.crtcs {
        reply = reply.set_u32(off, cid);
        off += 4;
    }
    for &oid in &provider.outputs {
        reply = reply.set_u32(off, oid);
        off += 4;
    }
    // No associated providers.
    reply = reply.set_bytes(off, name_bytes);

    reply.build()
}
