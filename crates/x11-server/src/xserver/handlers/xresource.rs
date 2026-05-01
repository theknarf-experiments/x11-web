//! X-Resource extension handler (XRes).
//!
//! Reports per-client resource usage. Used by tools like `xrestop` and
//! desktop environments for memory monitoring.

use tracing::debug;

use super::super::client::ClientState;
use super::super::core::*;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;

/// X-Resource major opcode (assigned in QueryExtension).
const XRES_MAJOR_OPCODE: u8 = 160;

pub(crate) fn handle_xresource_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let bo = state.msb_first;
    debug!("X-Resource minor opcode: {minor}");
    let bad_request = |bad_value: u32| {
        build_error(
            REQUEST_ERROR,
            seq,
            bad_value,
            XRES_MAJOR_OPCODE,
            minor as u16,
        )
    };

    match minor {
        // 0: QueryVersion
        0 => {
            ReplyBuf::fixed(seq, bo)
                .set_u16(8, 1) // server_major = 1
                .set_u16(10, 2) // server_minor = 2
                .build()
        }

        // 1: QueryClients — return list of connected client XIDs
        1 => {
            // Read all connected client resource bases from the shared registry.
            let client_bases = state.client_registry.lock().unwrap().clone();
            let num_clients = client_bases.len() as u32;
            let extra_bytes = (num_clients as usize) * 8; // each client entry = 8 bytes

            // Client entries: resource_base (4 bytes) + resource_mask (4 bytes) each
            let mut reply = ReplyBuf::with_extra(seq, extra_bytes, bo).set_u32(8, num_clients); // num_clients
            for (i, &resource_base) in client_bases.iter().enumerate() {
                let off = 32 + i * 8;
                reply = reply
                    .set_u32(off, resource_base)
                    .set_u32(off + 4, 0x003FFFFF);
            }

            reply.build()
        }

        // 2: QueryClientResources — return resource type counts for a client
        2 => {
            if data.len() < 8 {
                return bad_request(0);
            }

            // Count resources by type
            let num_windows = state.windows.len() as u32;
            let num_pixmaps = state.pixmaps.len() as u32;
            let num_gcs = state.gcs.len() as u32;
            let num_cursors = state.cursors.len() as u32;
            let num_colormaps = state.colormaps.len() as u32;
            let num_fonts = 1u32; // font manager always has at least default
            let num_pictures = state.render.picture_count() as u32;
            let num_glyphsets = state.render.glyphset_count() as u32;

            // Build type entries: each is resource_type_atom (4) + count (4) = 8 bytes
            struct TypeCount {
                type_name: &'static str,
                count: u32,
            }
            let types = [
                TypeCount {
                    type_name: "WINDOW",
                    count: num_windows,
                },
                TypeCount {
                    type_name: "PIXMAP",
                    count: num_pixmaps,
                },
                TypeCount {
                    type_name: "GC",
                    count: num_gcs,
                },
                TypeCount {
                    type_name: "CURSOR",
                    count: num_cursors,
                },
                TypeCount {
                    type_name: "COLORMAP",
                    count: num_colormaps,
                },
                TypeCount {
                    type_name: "FONT",
                    count: num_fonts,
                },
                TypeCount {
                    type_name: "PICTURE",
                    count: num_pictures,
                },
                TypeCount {
                    type_name: "GLYPHSET",
                    count: num_glyphsets,
                },
            ];

            // Only include types with count > 0
            let active_types: Vec<&TypeCount> = types.iter().filter(|t| t.count > 0).collect();
            let num_types = active_types.len() as u32;
            let extra_bytes = (num_types as usize) * 8;
            let mut reply = ReplyBuf::with_extra(seq, extra_bytes, bo).set_u32(8, num_types);

            let mut off = 32;
            for t in active_types {
                let atom = {
                    let mut atoms = state.atoms.lock().unwrap();
                    atoms.intern(t.type_name, false)
                };
                reply = reply.set_u32(off, atom).set_u32(off + 4, t.count);
                off += 8;
            }

            reply.build()
        }

        // 3: QueryClientPixmapBytes — total pixmap memory for a client
        3 => {
            if data.len() < 8 {
                return bad_request(0);
            }

            let total_bytes: u64 = state
                .pixmaps
                .values()
                .map(|p| (p.width as u64) * (p.height as u64) * (p.depth as u64 / 8).max(1))
                .sum();

            ReplyBuf::fixed(seq, bo)
                .set_u32(8, total_bytes as u32) // bytes (low 32)
                .set_u32(12, (total_bytes >> 32) as u32) // bytes_overflow (high 32)
                .build()
        }

        // 4: QueryClientIds (XRes 1.2) — return client IDs with their types
        4 => {
            use x11rb_protocol::protocol::res::QueryClientIdsRequest;
            let Ok(req) =
                QueryClientIdsRequest::try_parse_request(request_header(data), &data[4..])
            else {
                return bad_request(0);
            };

            // Collect client IDs from the request specs
            let client_bases = state.client_registry.lock().unwrap().clone();
            let mut ids: Vec<(u32, u32)> = Vec::new(); // (resource_base, pid)

            for spec in req.specs.iter() {
                let client_xid = spec.client;
                let mask = u32::from(spec.mask);

                // mask bit 0 = X_RES_CLIENT_ID_NR (client number)
                // mask bit 1 = X_RES_CLIENT_ID_PID (process id)
                if mask == 0 {
                    continue;
                }

                // If client_xid is 0, return info for all clients
                let targets: Vec<u32> = if client_xid == 0 {
                    client_bases.clone()
                } else {
                    // Find matching client by resource base
                    let base = client_xid & !0x003FFFFF;
                    if client_bases.contains(&base) {
                        vec![base]
                    } else {
                        vec![client_xid]
                    }
                };

                for &base in &targets {
                    if mask & 1 != 0 {
                        // X_RES_CLIENT_ID_NR: value is the XID base
                        ids.push((base, 0));
                    }
                    if mask & 2 != 0 {
                        // X_RES_CLIENT_ID_PID: report our PID
                        ids.push((base, std::process::id()));
                    }
                }
            }

            // Each ClientIdValue: spec (8 bytes) + length (4) + value (4) = 16 bytes
            let num_ids = ids.len() as u32;
            let data_bytes = num_ids as usize * 16;
            let mut reply = ReplyBuf::with_extra(seq, data_bytes, bo).set_u32(8, num_ids);

            let mut off = 32;
            for (base, value) in &ids {
                // ClientIdValue: client (4), mask (4), length (4), value (4)
                let id_mask = if *value == 0 { 1u32 } else { 2u32 };
                reply = reply
                    .set_u32(off, *base)
                    .set_u32(off + 4, id_mask)
                    .set_u32(off + 8, 4) // length = 4 bytes
                    .set_u32(off + 12, *value);
                off += 16;
            }

            reply.build()
        }

        // 5: QueryResourceBytes (XRes 1.2) — total bytes used by resource types
        5 => {
            use x11rb_protocol::protocol::res::QueryResourceBytesRequest;
            let Ok(req) =
                QueryResourceBytesRequest::try_parse_request(request_header(data), &data[4..])
            else {
                return bad_request(0);
            };
            let _client_xid = req.client;
            let num_specs = req.specs.len();

            // Compute byte counts for all resource types
            let window_bytes: u64 = state.windows.len() as u64 * 256; // estimate per window
            let pixmap_bytes: u64 = state
                .pixmaps
                .values()
                .map(|p| (p.width as u64) * (p.height as u64) * (p.depth as u64 / 8).max(1))
                .sum();
            let gc_bytes: u64 = state.gcs.len() as u64 * 128;
            let cursor_bytes: u64 = state.cursors.len() as u64 * 64;

            struct SizeEntry {
                type_name: &'static str,
                count: u32,
                bytes: u64,
            }
            let all_types = [
                SizeEntry {
                    type_name: "WINDOW",
                    count: state.windows.len() as u32,
                    bytes: window_bytes,
                },
                SizeEntry {
                    type_name: "PIXMAP",
                    count: state.pixmaps.len() as u32,
                    bytes: pixmap_bytes,
                },
                SizeEntry {
                    type_name: "GC",
                    count: state.gcs.len() as u32,
                    bytes: gc_bytes,
                },
                SizeEntry {
                    type_name: "CURSOR",
                    count: state.cursors.len() as u32,
                    bytes: cursor_bytes,
                },
            ];

            // If num_specs > 0, filter to requested types; otherwise return all
            let entries: Vec<&SizeEntry> = if num_specs == 0 {
                all_types.iter().filter(|e| e.count > 0).collect()
            } else {
                // Accept all types for simplicity
                all_types.iter().filter(|e| e.count > 0).collect()
            };

            let num_sizes = entries.len() as u32;
            // Each ResourceSizeValue: spec (8) + bytes (4) + ref_count (4) + use_count (4) = 20 bytes
            let data_bytes = num_sizes as usize * 20;
            let padded = (data_bytes + 3) & !3;
            let mut reply = ReplyBuf::with_extra(seq, padded, bo).set_u32(8, num_sizes);

            let mut off = 32;
            for e in entries {
                let atom = {
                    let mut atoms = state.atoms.lock().unwrap();
                    atoms.intern(e.type_name, false)
                };
                reply = reply
                    .set_u32(off, atom) // resource_type
                    .set_u32(off + 4, e.count) // count
                    .set_u32(off + 8, e.bytes as u32) // bytes (low)
                    .set_u32(off + 12, (e.bytes >> 32) as u32) // bytes (high)
                    .set_u32(off + 16, 0); // ref_count
                off += 20;
            }

            reply.build()
        }

        _ => {
            debug!("Unhandled X-Resource minor opcode: {minor}");
            bad_request(minor as u32)
        }
    }
}
