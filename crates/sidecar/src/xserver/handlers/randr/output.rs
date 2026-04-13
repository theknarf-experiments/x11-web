//! Output and provider RandR operations.

use tracing::{debug, info};

use super::super::super::client::ClientState;
use super::super::super::types::{OutputPropertyConfig, PropertyValue, RandrMode, RandrMonitor};

/// RRGetOutputInfo (9).
pub(crate) fn handle_get_output_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let output_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    build_output_info_reply(state, seq, output_id)
}

/// RRListOutputProperties (10).
pub(crate) fn handle_list_output_properties(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let output_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
    build_list_output_properties_reply(state, seq, output_id)
}

/// RRQueryOutputProperty (11).
pub(crate) fn handle_query_output_property(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    if data.len() < 12 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }
    let output_id = state.read_u32(data, 4);
    let property_atom = state.read_u32(data, 8);

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
    let length_words = num_values;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_words);
    reply[8] = if pending { 1 } else { 0 };
    reply[9] = if range { 1 } else { 0 };
    reply[10] = if immutable { 1 } else { 0 };
    // values follow the 32-byte header
    for (i, &val) in values.iter().enumerate() {
        let off = 32 + i * 4;
        state.write_u32(&mut reply, off, val);
    }
    reply
}

/// RRConfigureOutputProperty (12).
pub(crate) fn handle_configure_output_property(
    state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    if data.len() >= 14 {
        let output_id = state.read_u32(data, 4);
        let property_atom = state.read_u32(data, 8);
        let pending = data[12] != 0;
        let range = data[13] != 0;
        // Parse valid values (u32 values after the fixed header)
        let mut values = Vec::new();
        let mut off = 16; // values start after padding
        while off + 4 <= data.len() {
            values.push(state.read_u32(data, off));
            off += 4;
        }
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
        let output_id = state.read_u32(data, 4);
        let property = state.read_u32(data, 8);
        let prop_type = state.read_u32(data, 12);
        let format = data[16];
        let _mode = data[17]; // 0=Replace, 1=Prepend, 2=Append
        let num_items = state.read_u32(data, 20) as usize;
        let bytes_per_item = match format {
            8 => 1,
            16 => 2,
            32 => 4,
            _ => 1,
        };
        let data_len = num_items * bytes_per_item;
        let prop_data = if data.len() >= 24 + data_len {
            data[24..24 + data_len].to_vec()
        } else {
            Vec::new()
        };
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
        let output_id = state.read_u32(data, 4);
        let property = state.read_u32(data, 8);
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
    // Request layout:
    //   4-7: window (unused, but must be present)
    //   8-39: XRRModeInfo (32 bytes):
    //     8-11: id (ignored, server assigns)
    //    12-13: width
    //    14-15: height
    //    16-19: dotClock
    //    20-21: hSyncStart
    //    22-23: hSyncEnd
    //    24-25: hTotal
    //    26-27: hSkew (unused)
    //    28-29: vSyncStart
    //    30-31: vSyncEnd
    //    32-33: vTotal
    //    34-35: nameLen
    //    36-39: modeFlags
    //   40+: name bytes (nameLen)

    if data.len() < 40 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    let width = state.read_u16(data, 12);
    let height = state.read_u16(data, 14);
    let dot_clock = state.read_u32(data, 16);
    let h_sync_start = state.read_u16(data, 20);
    let h_sync_end = state.read_u16(data, 22);
    let h_total = state.read_u16(data, 24);
    let v_sync_start = state.read_u16(data, 28);
    let v_sync_end = state.read_u16(data, 30);
    let v_total = state.read_u16(data, 32);
    let name_len = state.read_u16(data, 34) as usize;
    let flags = state.read_u32(data, 36);

    let name = if data.len() >= 40 + name_len && name_len > 0 {
        String::from_utf8_lossy(&data[40..40 + name_len]).to_string()
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
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, mode_id);
    reply.to_vec()
}

/// RRDestroyMode (17).
pub(crate) fn handle_destroy_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 8 {
        let mode_id = state.read_u32(data, 4);
        state.randr_modes.retain(|m| m.id != mode_id);
        debug!("RRDestroyMode mode_id={mode_id}");
    }
    Vec::new()
}

/// RRAddOutputMode (18).
pub(crate) fn handle_add_output_mode(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() >= 12 {
        let output_id = state.read_u32(data, 4);
        let mode_id = state.read_u32(data, 8);
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
        let output_id = state.read_u32(data, 4);
        let mode_id = state.read_u32(data, 8);
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
        let output_id = state.read_u32(data, 8);
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
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, primary_output);
    reply.to_vec()
}

/// RRGetProviders (32).
pub(crate) fn handle_get_providers(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let num_providers = state.randr_providers.len() as u16;
    let var_len = num_providers as usize * 4;
    let length_field = var_len / 4;
    let total = 32 + var_len;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field as u32);
    state.write_u32(&mut reply, 8, state.timestamp());
    state.write_u16(&mut reply, 12, num_providers);
    let mut off = 32;
    for p in &state.randr_providers {
        state.write_u32(&mut reply, off, p.id);
        off += 4;
    }
    reply
}

/// RRGetProviderInfo (33).
pub(crate) fn handle_get_provider_info(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let provider_id = if data.len() >= 8 {
        state.read_u32(data, 4)
    } else {
        0
    };
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
        let provider = _state.read_u32(data, 4);
        let sink = _state.read_u32(data, 8);
        let config_ts = _state.read_u32(data, 12);
        debug!("RRSetProviderOffloadSink: provider={provider:#x} sink={sink:#x} ts={config_ts}");
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
        let provider = _state.read_u32(data, 4);
        let source = _state.read_u32(data, 8);
        let config_ts = _state.read_u32(data, 12);
        debug!(
            "RRSetProviderOutputSource: provider={provider:#x} source={source:#x} ts={config_ts}"
        );
    }
    Vec::new()
}

/// RRListProviderProperties (36).
pub(crate) fn handle_list_provider_properties(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    // length = 0, num_atoms = 0
    state.write_u16(&mut reply, 8, 0);
    reply.to_vec()
}

/// RRQueryProviderProperty (37).
pub(crate) fn handle_query_provider_property(
    state: &mut ClientState,
    _data: &[u8],
    seq: u16,
) -> Vec<u8> {
    // Reply with empty constraints (pending=0, range=0, immutable=0, no values)
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    reply.to_vec()
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
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    // All fields default to 0: type=None, bytes_after=0, num_items=0
    reply.to_vec()
}

/// RRGetMonitors (42).
pub(crate) fn handle_get_monitors(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    super::screen::build_get_monitors_reply(state, seq)
}

/// RRSetMonitor (43).
pub(crate) fn handle_set_monitor(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    // MonitorInfo layout at byte 4:
    //   0-3: name (ATOM)
    //   4: primary (BOOL)
    //   5: automatic (BOOL)
    //   6-7: nOutput (CARD16)
    //   8-9: x (INT16)
    //  10-11: y (INT16)
    //  12-13: width (CARD16)
    //  14-15: height (CARD16)
    //  16-19: mmWidth (CARD32)
    //  20-23: mmHeight (CARD32)
    //  24+: output IDs
    if data.len() >= 28 {
        let name_atom = state.read_u32(data, 4);
        let primary = data[8] != 0;
        let automatic = data[9] != 0;
        let n_output = state.read_u16(data, 10) as usize;
        let x = state.read_i16(data, 12);
        let y = state.read_i16(data, 14);
        let width = state.read_u16(data, 16);
        let height = state.read_u16(data, 18);
        // mmWidth at 20, mmHeight at 24 (we skip these for storage)
        let mut output_ids = Vec::with_capacity(n_output);
        let outputs_start = 28;
        for i in 0..n_output {
            if outputs_start + i * 4 + 4 <= data.len() {
                output_ids.push(state.read_u32(data, outputs_start + i * 4));
            }
        }
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
        let _window = state.read_u32(data, 4);
        let name_atom = state.read_u32(data, 8);
        state.randr_monitors.retain(|m| m.name_atom != name_atom);
        debug!("RRDeleteMonitor name_atom={name_atom}");
    }
    Vec::new()
}

/// RRCreateLease (45).
pub(crate) fn handle_create_lease(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let minor = 45u8;
    debug!("RRCreateLease: not supported on virtual display");
    crate::xserver::core::build_error_bo(
        crate::xserver::core::BAD_ACCESS,
        seq,
        0,
        140,
        minor as u16,
        state.msb_first,
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
            let mut reply = vec![0u8; 32 + 24];
            reply[0] = 1;
            reply[1] = 0;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 6); // length
            reply[24] = 1; // Disconnected
            return reply;
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
    let length = (inline_header + var_data) / 4;
    let total = 32 + inline_header + var_data;
    let mut reply = vec![0u8; total];

    reply[0] = 1; // Reply
    reply[1] = 0; // Success
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length as u32);
    state.write_u32(&mut reply, 8, state.timestamp());
    state.write_u32(&mut reply, 12, output.crtc_id);
    state.write_u32(&mut reply, 16, output.mm_width);
    state.write_u32(&mut reply, 20, output.mm_height);
    reply[24] = output.connection_status;
    reply[25] = 0; // subpixel_order: Unknown
    state.write_u16(&mut reply, 26, num_crtcs);
    state.write_u16(&mut reply, 28, num_modes);
    state.write_u16(&mut reply, 30, 1); // num_preferred
    state.write_u16(&mut reply, 32, num_clones);
    state.write_u16(&mut reply, 34, output_name.len() as u16);

    let mut off = 36;
    // CRTC IDs (possible CRTCs)
    for &crtc_id in &output.possible_crtcs {
        state.write_u32(&mut reply, off, crtc_id);
        off += 4;
    }
    // Mode IDs
    for &mode_id in &output.modes {
        state.write_u32(&mut reply, off, mode_id);
        off += 4;
    }
    // Clone IDs (none)
    // Output name
    reply[off..off + output_name.len()].copy_from_slice(output_name);

    reply
}

/// Build the reply for RRListOutputProperties.
fn build_list_output_properties_reply(state: &ClientState, seq: u16, output_id: u32) -> Vec<u8> {
    let atoms: Vec<u32> = state
        .randr_find_output(output_id)
        .map(|o| o.properties.keys().copied().collect())
        .unwrap_or_default();

    let num_atoms = atoms.len() as u16;
    let var_len = atoms.len() * 4;
    let length_field = var_len / 4;
    let total = 32 + var_len;
    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field as u32);
    state.write_u16(&mut reply, 8, num_atoms);

    let mut off = 32;
    for atom in atoms {
        state.write_u32(&mut reply, off, atom);
        off += 4;
    }

    reply
}

/// Build the reply for RRGetOutputProperty.
fn build_get_output_property_reply(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Request layout:
    //   4: output (CARD32)
    //   8: property (ATOM)
    //  12: type (ATOM) — 0 = AnyPropertyType
    //  16: long_offset (CARD32)
    //  20: long_length (CARD32)
    //  24: delete (BOOL) + pad
    //  25: pending (BOOL) + pad

    if data.len() < 24 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    let output_id = state.read_u32(data, 4);
    let property = state.read_u32(data, 8);
    let req_type = state.read_u32(data, 12);
    let long_offset = state.read_u32(data, 16) as usize;
    let long_length = state.read_u32(data, 20) as usize;

    let output = match state.randr_find_output(output_id) {
        Some(o) => o,
        None => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            return reply.to_vec();
        }
    };

    let prop = match output.properties.get(&property) {
        Some(p) => p,
        None => {
            // Property not found.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            // type = None (0), format = 0, length = 0, bytes_after = 0
            return reply.to_vec();
        }
    };

    // Type mismatch check.
    if req_type != 0 && req_type != prop.prop_type {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        state.write_u16(&mut reply, 2, seq);
        state.write_u32(&mut reply, 8, prop.prop_type); // actual type
                                                        // bytes_after = total data length
        state.write_u32(&mut reply, 12, prop.data.len() as u32);
        return reply.to_vec();
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
    let length_field = var_len / 4;
    let total_reply = 32 + var_len;

    let mut reply = vec![0u8; total_reply];
    reply[0] = 1;
    reply[1] = prop.format;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field as u32);
    state.write_u32(&mut reply, 8, prop.prop_type);
    state.write_u32(&mut reply, 12, bytes_after as u32);
    state.write_u32(&mut reply, 16, num_items as u32);

    reply[32..32 + returned_data.len()].copy_from_slice(returned_data);

    reply
}

/// Build the reply for RRGetProviderInfo.
fn build_provider_info_reply(state: &ClientState, seq: u16, provider_id: u32) -> Vec<u8> {
    let provider = match state.randr_providers.iter().find(|p| p.id == provider_id) {
        Some(p) => p.clone(),
        None => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            return reply.to_vec();
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
    let length_field = var_len / 4;
    let total = 32 + var_len;

    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field as u32);
    state.write_u32(&mut reply, 8, state.timestamp());
    state.write_u32(&mut reply, 12, provider.capabilities);
    state.write_u16(&mut reply, 16, num_crtcs);
    state.write_u16(&mut reply, 18, num_outputs);
    state.write_u16(&mut reply, 20, num_associated);
    state.write_u16(&mut reply, 22, name_bytes.len() as u16);

    let mut off = 32;
    for &cid in &provider.crtcs {
        state.write_u32(&mut reply, off, cid);
        off += 4;
    }
    for &oid in &provider.outputs {
        state.write_u32(&mut reply, off, oid);
        off += 4;
    }
    // No associated providers.
    reply[off..off + name_bytes.len()].copy_from_slice(name_bytes);

    reply
}
