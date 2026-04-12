//! RANDR extension handler — multi-monitor support.

use tracing::{debug, info};

use super::super::client::ClientState;
use super::super::types::{OutputPropertyConfig, PropertyValue, RandrMode, RandrMonitor};

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
        //
        // In a web-based X11 server, screen dimensions are managed by the
        // browser viewport, not by the client. We parse the request to
        // validate it, check the config timestamp for staleness, and
        // return success with the current configuration.
        // ---------------------------------------------------------------
        2 => {
            if data.len() < 24 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
                    140, 2, state.msb_first,
                );
            }

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

        // ---------------------------------------------------------------
        // RRSelectInput (4) — select RandR events
        // ---------------------------------------------------------------
        4 => {
            if data.len() >= 12 {
                let _window = state.read_u32(data, 4);
                let enable = state.read_u16(data, 8);
                state.randr_event_mask = enable as u32;
                debug!("RRSelectInput mask=0x{:04x}", enable);
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRGetScreenInfo (5) — legacy screen configuration
        // ---------------------------------------------------------------
        5 => {
            let num_sizes: u16 = 1;
            let extra_data_len: usize = 8;
            let reply_len = 32 + extra_data_len;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1;
            reply[1] = 1; // rotations = Rotate_0
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (extra_data_len / 4) as u32);
            state.write_u32(&mut reply, 8, state.root_window);
            state.write_u32(&mut reply, 12, state.timestamp());
            state.write_u32(&mut reply, 16, state.randr_config_timestamp);
            state.write_u16(&mut reply, 20, num_sizes);
            state.write_u16(&mut reply, 22, 0); // sizeID
            state.write_u16(&mut reply, 24, 1); // rotation = Rotate_0
            state.write_u16(&mut reply, 26, 0); // nrateEnts
            state.write_u16(&mut reply, 32, state.screen_width);
            state.write_u16(&mut reply, 34, state.screen_height);
            state.write_u16(&mut reply, 36, 270);
            state.write_u16(&mut reply, 38, 203);
            reply
        }

        // ---------------------------------------------------------------
        // RRGetScreenSizeRange (6)
        // ---------------------------------------------------------------
        6 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 1);     // min_width
            state.write_u16(&mut reply, 10, 1);    // min_height
            state.write_u16(&mut reply, 12, 32767); // max_width
            state.write_u16(&mut reply, 14, 32767); // max_height
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRSetScreenSize (7)
        // ---------------------------------------------------------------
        7 => {
            if data.len() >= 12 {
                let new_w = state.read_u16(data, 4);
                let new_h = state.read_u16(data, 6);
                if new_w > 0 && new_h > 0 {
                    info!("RandR SetScreenSize: {}x{} -> {}x{}", state.screen_width, state.screen_height, new_w, new_h);
                    state.screen_width = new_w;
                    state.screen_height = new_h;
                    state.randr_config_timestamp += 1;
                    if let Some(root) = state.windows.get_mut(&state.root_window) {
                        root.width = new_w;
                        root.height = new_h;
                        root.framebuffer.resize(new_w as u32, new_h as u32);
                    }
                    state.randr_queue_screen_change_notify();
                }
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRGetScreenResources (8)
        // ---------------------------------------------------------------
        8 => {
            build_screen_resources_reply(state, seq)
        }

        // ---------------------------------------------------------------
        // RRGetOutputInfo (9)
        // ---------------------------------------------------------------
        9 => {
            let output_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            build_output_info_reply(state, seq, output_id)
        }

        // ---------------------------------------------------------------
        // RRListOutputProperties (10)
        // ---------------------------------------------------------------
        10 => {
            let output_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            build_list_output_properties_reply(state, seq, output_id)
        }

        // ---------------------------------------------------------------
        // RRQueryOutputProperty (11) — return property constraints
        // ---------------------------------------------------------------
        11 => {
            if data.len() < 12 {
                let mut reply = [0u8; 32];
                reply[0] = 1;
                state.write_u16(&mut reply, 2, seq);
                return reply.to_vec();
            }
            let output_id = state.read_u32(data, 4);
            let property_atom = state.read_u32(data, 8);

            // Check if the output has an explicit property config (set by ConfigureOutputProperty)
            let (pending, range, immutable, values) = if let Some(output) = state.randr_find_output(output_id) {
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

        // ---------------------------------------------------------------
        // RRConfigureOutputProperty (12) — store property config
        // ---------------------------------------------------------------
        12 => {
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
                    output.property_configs.insert(property_atom, OutputPropertyConfig {
                        pending,
                        range,
                        values,
                    });
                }
                debug!("RRConfigureOutputProperty output={output_id} property={property_atom} pending={pending} range={range}");
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRChangeOutputProperty (13)
        // ---------------------------------------------------------------
        13 => {
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
                    output.properties.insert(property, PropertyValue {
                        prop_type,
                        format,
                        data: prop_data,
                    });
                }
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRDeleteOutputProperty (14)
        // ---------------------------------------------------------------
        14 => {
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

        // ---------------------------------------------------------------
        // RRGetOutputProperty (15)
        // ---------------------------------------------------------------
        15 => {
            build_get_output_property_reply(state, data, seq)
        }

        // ---------------------------------------------------------------
        // RRCreateMode (16) — create a new mode and return its ID
        // ---------------------------------------------------------------
        16 => {
            handle_create_mode(state, data, seq)
        }

        // ---------------------------------------------------------------
        // RRDestroyMode (17) — remove a mode by ID
        // ---------------------------------------------------------------
        17 => {
            if data.len() >= 8 {
                let mode_id = state.read_u32(data, 4);
                state.randr_modes.retain(|m| m.id != mode_id);
                debug!("RRDestroyMode mode_id={mode_id}");
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRAddOutputMode (18)
        // ---------------------------------------------------------------
        18 => {
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

        // ---------------------------------------------------------------
        // RRDeleteOutputMode (19)
        // ---------------------------------------------------------------
        19 => {
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

        // ---------------------------------------------------------------
        // RRGetCrtcInfo (20)
        // ---------------------------------------------------------------
        20 => {
            let crtc_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            build_crtc_info_reply(state, seq, crtc_id)
        }

        // ---------------------------------------------------------------
        // RRSetCrtcConfig (21)
        // ---------------------------------------------------------------
        21 => {
            handle_set_crtc_config(state, data, seq)
        }

        // ---------------------------------------------------------------
        // RRGetCrtcGammaSize (22)
        // ---------------------------------------------------------------
        22 => {
            let crtc_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            let size: u16 = if state.randr_find_crtc(crtc_id).is_some() { 256 } else { 0 };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, size);
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRGetCrtcGamma (23)
        // ---------------------------------------------------------------
        23 => {
            let crtc_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            build_get_crtc_gamma_reply(state, seq, crtc_id)
        }

        // ---------------------------------------------------------------
        // RRSetCrtcGamma (24)
        // ---------------------------------------------------------------
        24 => {
            handle_set_crtc_gamma(state, data);
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRSetCrtcTransform (26) — void request; store the transform.
        //
        // Wire layout (after the 4-byte request header at [0..4]):
        //   [4..8]   crtc (CRTC)
        //   [8..44]  transform (9 × Fixed = 36 bytes, row-major 3×3 matrix)
        //   [44..46] filter name length (CARD16)
        //   [46..48] padding
        //   [48..]   filter name bytes + filter params
        // ---------------------------------------------------------------
        26 => {
            if data.len() >= 44 {
                let crtc_id = state.read_u32(data, 4);
                // Read the 3×3 fixed-point matrix.  state.read_u32 handles
                // byte-order; we reinterpret the bits as i32 for storage.
                let mut matrix = [0i32; 9];
                for i in 0..9usize {
                    matrix[i] = state.read_u32(data, 8 + i * 4) as i32;
                }
                if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
                    crtc.transform = matrix;
                    debug!("RRSetCrtcTransform crtc={crtc_id} transform stored");
                } else {
                    debug!("RRSetCrtcTransform crtc={crtc_id} (unknown crtc, ignoring)");
                }
            } else {
                debug!("RRSetCrtcTransform: request too short ({}B), ignoring", data.len());
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRGetPanning (27) — no panning
        // ---------------------------------------------------------------
        27 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 0; // Success
            state.write_u16(&mut reply, 2, seq);
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRSetPanning (28) — accepts the request and returns Success.
        //
        // Per the RandR 1.3 spec the reply is:
        //   byte 0:   1 (reply)
        //   byte 1:   status (0 = Success, 1 = InvalidConfig, 2 = InvalidTime)
        //   bytes 2-3: sequence number
        //   bytes 4-7: reply length (0 extra words)
        //   bytes 8-11: timestamp
        //   bytes 12-31: unused
        // ---------------------------------------------------------------
        28 => {
            let crtc_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            debug!("RRSetPanning crtc={crtc_id} -> Success");
            let mut reply = [0u8; 32];
            reply[0] = 1;          // reply
            reply[1] = 0;          // Success
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 0);   // no extra data
            state.write_u32(&mut reply, 8, state.timestamp());
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRGetCrtcTransform (29)
        //
        // Reply layout (96 bytes = 32 header + 64 variable):
        //   byte 0:    1 (reply)
        //   bytes 2-3: sequence
        //   bytes 4-7: reply length in 4-byte words (16 = 64 extra bytes)
        //   bytes 8-43:  pending transform (9 × Fixed)
        //   bytes 44-46: pending filter name length (CARD16)
        //   bytes 47:    padding
        //   bytes 48:    pending filter name (empty)
        //   bytes 48-83: current transform (9 × Fixed)
        //   bytes 84-85: current filter name length (CARD16)
        //   bytes 86:    padding
        //   bytes 86+:   current filter name (empty)
        //
        // For a virtual display we return the stored transform for both
        // pending and current, with no filter name.
        // ---------------------------------------------------------------
        29 => {
            let crtc_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            // Retrieve transform or fall back to identity.
            let identity = [65536i32, 0, 0, 0, 65536, 0, 0, 0, 65536];
            let transform = state.randr_find_crtc(crtc_id)
                .map(|c| c.transform)
                .unwrap_or(identity);

            // Reply: 32-byte header + 36 (pending matrix) + 2 (namelen) + 2 (pad)
            //        + 36 (current matrix) + 2 (namelen) + 2 (pad) = 32 + 80 = 112
            // But the length field counts words after the first 32 bytes: 80/4 = 20.
            let mut reply = vec![0u8; 112];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 20); // 20 words of extra data

            // Write pending transform at offset 8 (state.write_u32 handles byte-order)
            for i in 0..9usize {
                state.write_u32(&mut reply, 8 + i * 4, transform[i] as u32);
            }
            // pending filter name length = 0 at offset 44, padding at 46-47: already 0

            // Write current transform at offset 48
            for i in 0..9usize {
                state.write_u32(&mut reply, 48 + i * 4, transform[i] as u32);
            }
            // current filter name length = 0 at offset 84, padding at 86-87: already 0

            reply
        }

        // ---------------------------------------------------------------
        // RRSetOutputPrimary (30) — store the primary output.
        //
        // Wire layout:
        //   [4..8]  window (WINDOW)  — root window
        //   [8..12] output (OUTPUT)  — 0 means "no primary"
        // ---------------------------------------------------------------
        30 => {
            if data.len() >= 12 {
                let output_id = state.read_u32(data, 8);
                state.randr_primary_output = output_id;
                debug!("RRSetOutputPrimary output={output_id}");
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRGetOutputPrimary (31)
        // ---------------------------------------------------------------
        31 => {
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

        // ---------------------------------------------------------------
        // RRGetProviders (32)
        // ---------------------------------------------------------------
        32 => {
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

        // ---------------------------------------------------------------
        // RRGetProviderInfo (33)
        // ---------------------------------------------------------------
        33 => {
            let provider_id = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            build_provider_info_reply(state, seq, provider_id)
        }

        // ---------------------------------------------------------------
        // RRSetProviderOffloadSink (34) — no-op for virtual display
        // ---------------------------------------------------------------
        34 => {
            debug!("RRSetProviderOffloadSink (no-op)");
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRSetProviderOutputSource (35) — no-op for virtual display
        // ---------------------------------------------------------------
        35 => {
            debug!("RRSetProviderOutputSource (no-op)");
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRListProviderProperties (36) — no provider properties
        // ---------------------------------------------------------------
        36 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            // length = 0, num_atoms = 0
            state.write_u16(&mut reply, 8, 0);
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRQueryProviderProperty (37) — no provider properties exist
        // ---------------------------------------------------------------
        37 => {
            // Reply with empty constraints (pending=0, range=0, immutable=0, no values)
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRConfigureProviderProperty (38) — no-op
        // ---------------------------------------------------------------
        38 => {
            debug!("RRConfigureProviderProperty (no-op)");
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRChangeProviderProperty (39) — no-op
        // ---------------------------------------------------------------
        39 => {
            debug!("RRChangeProviderProperty (no-op)");
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRDeleteProviderProperty (40) — no-op
        // ---------------------------------------------------------------
        40 => {
            debug!("RRDeleteProviderProperty (no-op)");
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRGetProviderProperty (41) — return empty / not found
        // ---------------------------------------------------------------
        41 => {
            // Reply with type=None, format=0, length=0, bytes_after=0
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            // All fields default to 0: type=None, bytes_after=0, num_items=0
            reply.to_vec()
        }

        // ---------------------------------------------------------------
        // RRGetScreenResourcesCurrent (25)
        // ---------------------------------------------------------------
        25 => {
            build_screen_resources_reply(state, seq)
        }

        // ---------------------------------------------------------------
        // RRGetMonitors (42)
        // ---------------------------------------------------------------
        42 => {
            build_get_monitors_reply(state, seq)
        }

        // ---------------------------------------------------------------
        // RRSetMonitor (43) — store a monitor definition
        // ---------------------------------------------------------------
        43 => {
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

        // ---------------------------------------------------------------
        // RRDeleteMonitor (44) — remove a named monitor
        // ---------------------------------------------------------------
        44 => {
            if data.len() >= 12 {
                let _window = state.read_u32(data, 4);
                let name_atom = state.read_u32(data, 8);
                state.randr_monitors.retain(|m| m.name_atom != name_atom);
                debug!("RRDeleteMonitor name_atom={name_atom}");
            }
            Vec::new()
        }

        // ---------------------------------------------------------------
        // RRCreateLease (45) — leases not supported on virtual display;
        // return a BadAccess error since we cannot hand out DRM fds.
        // ---------------------------------------------------------------
        45 => {
            debug!("RRCreateLease: not supported on virtual display");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_ACCESS, seq, 0,
                140, minor as u16, state.msb_first,
            )
        }

        // ---------------------------------------------------------------
        // RRFreeLease (46) — no-op since we never create leases
        // ---------------------------------------------------------------
        46 => {
            debug!("RRFreeLease (no-op)");
            Vec::new()
        }

        _ => {
            info!("Unhandled RANDR minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                140, minor as u16, state.msb_first,
            )
        }
    }
}

// ===========================================================================
// Reply builders
// ===========================================================================

/// Build the reply for RRGetScreenResources / RRGetScreenResourcesCurrent.
fn build_screen_resources_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    let num_crtcs = state.randr_crtcs.len();
    let num_outputs = state.randr_outputs.len();
    let num_modes = state.randr_modes.len();

    // Collect all mode name bytes (concatenated).
    let mut names_bytes = Vec::new();
    for mode in &state.randr_modes {
        names_bytes.extend_from_slice(mode.name.as_bytes());
    }
    let names_len = names_bytes.len();
    let names_pad = (4 - (names_len % 4)) % 4;

    // Variable data:
    //   crtc_ids: num_crtcs * 4
    //   output_ids: num_outputs * 4
    //   mode_infos: num_modes * 32
    //   mode_names: names_len + pad
    let var_len = num_crtcs * 4 + num_outputs * 4 + num_modes * 32 + names_len + names_pad;
    let length_field = var_len / 4;
    let total = 32 + var_len;

    let mut r = vec![0u8; total];
    r[0] = 1; // Reply
    state.write_u16(&mut r, 2, seq);
    state.write_u32(&mut r, 4, length_field as u32);
    state.write_u32(&mut r, 8, state.timestamp());
    state.write_u32(&mut r, 12, state.randr_config_timestamp);
    state.write_u16(&mut r, 16, num_crtcs as u16);
    state.write_u16(&mut r, 18, num_outputs as u16);
    state.write_u16(&mut r, 20, num_modes as u16);
    state.write_u16(&mut r, 22, names_len as u16);

    let mut off = 32;

    // CRTC IDs
    for crtc in &state.randr_crtcs {
        state.write_u32(&mut r, off, crtc.id);
        off += 4;
    }

    // Output IDs
    for output in &state.randr_outputs {
        state.write_u32(&mut r, off, output.id);
        off += 4;
    }

    // ModeInfo structs (32 bytes each)
    for mode in &state.randr_modes {
        state.write_u32(&mut r, off, mode.id);
        state.write_u16(&mut r, off + 4, mode.width);
        state.write_u16(&mut r, off + 6, mode.height);
        state.write_u32(&mut r, off + 8, mode.dot_clock);
        state.write_u16(&mut r, off + 12, mode.h_sync_start);
        state.write_u16(&mut r, off + 14, mode.h_sync_end);
        state.write_u16(&mut r, off + 16, mode.h_total);
        // hSkew at off+18 = 0
        state.write_u16(&mut r, off + 20, mode.v_sync_start);
        state.write_u16(&mut r, off + 22, mode.v_sync_end);
        state.write_u16(&mut r, off + 24, mode.v_total);
        state.write_u16(&mut r, off + 26, mode.name.len() as u16);
        state.write_u32(&mut r, off + 28, mode.flags);
        off += 32;
    }

    // Mode names (concatenated)
    r[off..off + names_len].copy_from_slice(&names_bytes);

    r
}

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
    let var_data = (num_crtcs as usize * 4) + (num_modes as usize * 4) + (num_clones as usize * 4) + output_name.len() + name_pad;
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

/// Build the reply for RRGetCrtcInfo.
fn build_crtc_info_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let crtc = match state.randr_find_crtc(crtc_id) {
        Some(c) => c.clone(),
        None => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 1; // InvalidConfig
            state.write_u16(&mut reply, 2, seq);
            return reply.to_vec();
        }
    };

    let num_outputs = crtc.outputs.len() as u16;
    // Possible outputs = all outputs (in our model every output can go to any CRTC)
    let num_possible = state.randr_outputs.len() as u16;
    let var_data = (num_outputs as usize + num_possible as usize) * 4;
    let inline_header = 24;
    let length = (inline_header + var_data) / 4;
    let total = 32 + inline_header + var_data;
    let mut reply = vec![0u8; total];

    reply[0] = 1;
    reply[1] = 0; // Success
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length as u32);
    state.write_u32(&mut reply, 8, state.timestamp());
    state.write_i16(&mut reply, 12, crtc.x);
    state.write_i16(&mut reply, 14, crtc.y);
    state.write_u16(&mut reply, 16, crtc.width);
    state.write_u16(&mut reply, 18, crtc.height);
    state.write_u32(&mut reply, 20, crtc.mode_id);
    state.write_u16(&mut reply, 24, crtc.rotation);
    state.write_u16(&mut reply, 26, 1); // rotations supported: Rotate_0
    state.write_u16(&mut reply, 28, num_outputs);
    state.write_u16(&mut reply, 30, num_possible);

    let mut off = 32;
    // Current outputs
    for &oid in &crtc.outputs {
        state.write_u32(&mut reply, off, oid);
        off += 4;
    }
    // Possible outputs
    for output in &state.randr_outputs {
        state.write_u32(&mut reply, off, output.id);
        off += 4;
    }

    reply
}

/// Handle RRSetCrtcConfig: update CRTC position/mode/rotation in the model.
fn handle_set_crtc_config(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Request layout:
    //   4: crtc (CARD32)
    //   8: timestamp (CARD32)
    //  12: config_timestamp (CARD32)
    //  16: x (INT16)
    //  18: y (INT16)
    //  20: mode (CARD32)
    //  24: rotation (CARD16)
    //  26: pad
    //  28+: output list (CARD32 each)

    if data.len() < 28 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[1] = 1; // InvalidConfig
        state.write_u16(&mut reply, 2, seq);
        return reply.to_vec();
    }

    let crtc_id = state.read_u32(data, 4);
    let _timestamp = state.read_u32(data, 8);
    let _config_timestamp = state.read_u32(data, 12);
    let x = state.read_i16(data, 16);
    let y = state.read_i16(data, 18);
    let mode_id = state.read_u32(data, 20);
    let rotation = state.read_u16(data, 24);

    // Parse output list
    let _num_outputs = if data.len() > 28 { (data.len() - 28) / 4 } else { 0 };

    // Look up mode dimensions first to avoid borrow conflict.
    let mode_dims = if mode_id == 0 {
        Some((0u16, 0u16))
    } else {
        state.randr_find_mode(mode_id).map(|m| (m.width, m.height))
    };

    let found = if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
        info!("RRSetCrtcConfig crtc={} mode={} pos=({},{}) rot={}", crtc_id, mode_id, x, y, rotation);

        crtc.x = x;
        crtc.y = y;
        crtc.mode_id = mode_id;
        crtc.rotation = rotation;

        if let Some((w, h)) = mode_dims {
            crtc.width = w;
            crtc.height = h;
        }
        true
    } else {
        false
    };

    if found {
        state.randr_config_timestamp += 1;
        state.randr_queue_crtc_change_notify(crtc_id);
        state.randr_queue_screen_change_notify();
    }

    let ts = state.timestamp();
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // Success
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 8, ts);
    reply.to_vec()
}

/// Build the reply for RRGetCrtcGamma.
fn build_get_crtc_gamma_reply(state: &ClientState, seq: u16, crtc_id: u32) -> Vec<u8> {
    let crtc = match state.randr_find_crtc(crtc_id) {
        Some(c) => c,
        None => {
            // Empty gamma reply.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 0);
            return reply.to_vec();
        }
    };

    let size = crtc.gamma_red.len() as u16;
    // Each channel is `size` u16 values = size * 2 bytes.
    // Total gamma data = 3 * size * 2 bytes.
    let gamma_data_len = 3 * size as usize * 2;
    let pad = (4 - (gamma_data_len % 4)) % 4;
    let var_len = gamma_data_len + pad;
    let length_field = var_len / 4;
    let total = 32 + var_len;

    let mut reply = vec![0u8; total];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, length_field as u32);
    state.write_u16(&mut reply, 8, size);

    let mut off = 32;
    // Red
    for &v in &crtc.gamma_red {
        state.write_u16(&mut reply, off, v);
        off += 2;
    }
    // Green
    for &v in &crtc.gamma_green {
        state.write_u16(&mut reply, off, v);
        off += 2;
    }
    // Blue
    for &v in &crtc.gamma_blue {
        state.write_u16(&mut reply, off, v);
        off += 2;
    }

    reply
}

/// Handle RRSetCrtcGamma: store the gamma LUT.
fn handle_set_crtc_gamma(state: &mut ClientState, data: &[u8]) {
    if data.len() < 8 {
        return;
    }

    let crtc_id = state.read_u32(data, 4);
    let size = state.read_u16(data, 8) as usize;

    // Data starts at offset 12 (after crtc(4) + size(2) + pad(2)).
    let data_start = 12;
    let channel_bytes = size * 2;
    let needed = data_start + 3 * channel_bytes;

    if data.len() < needed || size == 0 {
        return;
    }

    // Parse gamma values before borrowing crtc mutably.
    let mut red = Vec::with_capacity(size);
    let mut green = Vec::with_capacity(size);
    let mut blue = Vec::with_capacity(size);

    for i in 0..size {
        red.push(state.read_u16(data, data_start + i * 2));
    }
    let g_off = data_start + channel_bytes;
    for i in 0..size {
        green.push(state.read_u16(data, g_off + i * 2));
    }
    let b_off = g_off + channel_bytes;
    for i in 0..size {
        blue.push(state.read_u16(data, b_off + i * 2));
    }

    if let Some(crtc) = state.randr_find_crtc_mut(crtc_id) {
        crtc.gamma_red = red;
        crtc.gamma_green = green;
        crtc.gamma_blue = blue;
        debug!("RRSetCrtcGamma crtc={} size={}", crtc_id, size);
    }
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

/// Build the reply for RRGetMonitors.
/// Includes both automatic monitors derived from CRTCs and user-defined
/// monitors set via RRSetMonitor.
fn build_get_monitors_reply(state: &ClientState, seq: u16) -> Vec<u8> {
    // Collect automatic monitors from active CRTCs.
    let active_crtcs: Vec<_> = state.randr_crtcs.iter().filter(|c| c.mode_id != 0).collect();
    let n_auto = active_crtcs.len();
    let n_user = state.randr_monitors.len();
    let n_monitors = n_auto + n_user;
    let total_outputs: usize = active_crtcs.iter().map(|c| c.outputs.len()).sum::<usize>()
        + state.randr_monitors.iter().map(|m| m.output_ids.len()).sum::<usize>();

    // MonitorInfo = 24 bytes + nOutput * 4
    let mut monitor_data_len = 0usize;
    for c in &active_crtcs {
        monitor_data_len += 24 + c.outputs.len() * 4;
    }
    for m in &state.randr_monitors {
        monitor_data_len += 24 + m.output_ids.len() * 4;
    }
    let pad = (4 - (monitor_data_len % 4)) % 4;
    let length_field = (monitor_data_len + pad) / 4;
    let total = 32 + monitor_data_len + pad;

    let mut r = vec![0u8; total];
    r[0] = 1;
    state.write_u16(&mut r, 2, seq);
    state.write_u32(&mut r, 4, length_field as u32);
    state.write_u32(&mut r, 8, state.timestamp());
    state.write_u32(&mut r, 12, n_monitors as u32);
    state.write_u32(&mut r, 16, total_outputs as u32);

    let mut off = 32;

    // Emit automatic monitors from CRTCs.
    for (i, crtc) in active_crtcs.iter().enumerate() {
        let name_str = if i == 0 { "default".to_string() } else { format!("monitor-{}", i) };
        let monitor_name = state.intern_atom(&name_str, false);

        state.write_u32(&mut r, off, monitor_name);
        off += 4;
        r[off] = if i == 0 { 1 } else { 0 }; // primary
        off += 1;
        r[off] = 1; // automatic
        off += 1;
        state.write_u16(&mut r, off, crtc.outputs.len() as u16);
        off += 2;
        state.write_i16(&mut r, off, crtc.x);
        off += 2;
        state.write_i16(&mut r, off, crtc.y);
        off += 2;
        state.write_u16(&mut r, off, crtc.width);
        off += 2;
        state.write_u16(&mut r, off, crtc.height);
        off += 2;
        let (mm_w, mm_h) = crtc
            .outputs
            .first()
            .and_then(|&oid| state.randr_find_output(oid))
            .map(|o| (o.mm_width, o.mm_height))
            .unwrap_or((270, 203));
        state.write_u32(&mut r, off, mm_w);
        off += 4;
        state.write_u32(&mut r, off, mm_h);
        off += 4;
        for &oid in &crtc.outputs {
            state.write_u32(&mut r, off, oid);
            off += 4;
        }
    }

    // Emit user-defined monitors.
    for m in &state.randr_monitors {
        state.write_u32(&mut r, off, m.name_atom);
        off += 4;
        r[off] = m.primary as u8;
        off += 1;
        r[off] = m.automatic as u8;
        off += 1;
        state.write_u16(&mut r, off, m.output_ids.len() as u16);
        off += 2;
        state.write_i16(&mut r, off, m.x);
        off += 2;
        state.write_i16(&mut r, off, m.y);
        off += 2;
        state.write_u16(&mut r, off, m.width);
        off += 2;
        state.write_u16(&mut r, off, m.height);
        off += 2;
        // mm dimensions: approximate from pixel size (96 DPI).
        let mm_w = (m.width as u32 * 254 + 480) / 960;
        let mm_h = (m.height as u32 * 254 + 480) / 960;
        state.write_u32(&mut r, off, mm_w);
        off += 4;
        state.write_u32(&mut r, off, mm_h);
        off += 4;
        for &oid in &m.output_ids {
            state.write_u32(&mut r, off, oid);
            off += 4;
        }
    }

    r
}

/// Handle RRCreateMode: parse mode_info, allocate a new mode ID, store it,
/// and reply with the assigned mode ID.
fn handle_create_mode(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
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
