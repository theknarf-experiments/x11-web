//! X-Resource extension handler (XRes).
//!
//! Reports per-client resource usage. Used by tools like `xrestop` and
//! desktop environments for memory monitoring.

use tracing::debug;

use super::super::client::ClientState;
use super::super::core::*;

/// X-Resource major opcode (assigned in QueryExtension).
const XRES_MAJOR_OPCODE: u8 = 160;

pub(crate) fn handle_xresource_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let bo = state.msb_first;
    debug!("X-Resource minor opcode: {minor}");

    match minor {
        // 0: QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u16_bo(&mut reply, 8, 1, bo); // server_major = 1
            write_u16_bo(&mut reply, 10, 2, bo); // server_minor = 2
            reply.to_vec()
        }

        // 1: QueryClients — return list of connected client XIDs
        1 => {
            // Read all connected client resource bases from the shared registry.
            let client_bases = state.client_registry.lock().unwrap().clone();
            let num_clients = client_bases.len() as u32;
            let extra_words = num_clients * 2; // each client entry = 8 bytes = 2 words
            let mut reply = vec![0u8; 32 + (extra_words as usize) * 4];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, extra_words, bo); // reply length in 4-byte words
            write_u32_bo(&mut reply, 8, num_clients, bo); // num_clients

            // Client entries: resource_base (4 bytes) + resource_mask (4 bytes) each
            for (i, &resource_base) in client_bases.iter().enumerate() {
                let off = 32 + i * 8;
                write_u32_bo(&mut reply, off, resource_base, bo);
                write_u32_bo(&mut reply, off + 4, 0x003FFFFF, bo);
            }

            reply
        }

        // 2: QueryClientResources — return resource type counts for a client
        2 => {
            if data.len() < 8 {
                return build_error_bo(BAD_REQUEST, seq, 0, XRES_MAJOR_OPCODE, minor as u16, bo);
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
            let extra_words = num_types * 2;
            let mut reply = vec![0u8; 32 + (extra_words as usize) * 4];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, extra_words, bo);
            write_u32_bo(&mut reply, 8, num_types, bo);

            let mut off = 32;
            for t in active_types {
                let atom = {
                    let mut atoms = state.atoms.lock().unwrap();
                    atoms.intern(t.type_name, false)
                };
                write_u32_bo(&mut reply, off, atom, bo);
                write_u32_bo(&mut reply, off + 4, t.count, bo);
                off += 8;
            }

            reply
        }

        // 3: QueryClientPixmapBytes — total pixmap memory for a client
        3 => {
            if data.len() < 8 {
                return build_error_bo(BAD_REQUEST, seq, 0, XRES_MAJOR_OPCODE, minor as u16, bo);
            }

            let total_bytes: u64 = state
                .pixmaps
                .values()
                .map(|p| (p.width as u64) * (p.height as u64) * (p.depth as u64 / 8).max(1))
                .sum();

            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 8, total_bytes as u32, bo); // bytes (low 32)
            write_u32_bo(&mut reply, 12, (total_bytes >> 32) as u32, bo); // bytes_overflow (high 32)
            reply.to_vec()
        }

        // 4: QueryClientIds (XRes 1.2) — return client IDs with their types
        4 => {
            if data.len() < 8 {
                return build_error_bo(BAD_REQUEST, seq, 0, XRES_MAJOR_OPCODE, minor as u16, bo);
            }
            let num_specs = read_u32_bo(data, 4, bo) as usize;
            // Each spec is 8 bytes: client (4) + mask (4)
            if data.len() < 8 + num_specs * 8 {
                return build_error_bo(BAD_REQUEST, seq, 0, XRES_MAJOR_OPCODE, minor as u16, bo);
            }

            // Collect client IDs from the request specs
            let client_bases = state.client_registry.lock().unwrap().clone();
            let mut ids: Vec<(u32, u32)> = Vec::new(); // (resource_base, pid)

            for i in 0..num_specs {
                let off = 8 + i * 8;
                let client_xid = read_u32_bo(data, off, bo);
                let mask = read_u32_bo(data, off + 4, bo);

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
            let extra_words = data_bytes / 4;
            let mut reply = vec![0u8; 32 + data_bytes];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, extra_words as u32, bo);
            write_u32_bo(&mut reply, 8, num_ids, bo);

            let mut off = 32;
            for (base, value) in &ids {
                // ClientIdValue: client (4), mask (4), length (4), value (4)
                write_u32_bo(&mut reply, off, *base, bo);
                let id_mask = if *value == 0 { 1u32 } else { 2u32 };
                write_u32_bo(&mut reply, off + 4, id_mask, bo);
                write_u32_bo(&mut reply, off + 8, 4, bo); // length = 4 bytes
                write_u32_bo(&mut reply, off + 12, *value, bo);
                off += 16;
            }

            reply
        }

        // 5: QueryResourceBytes (XRes 1.2) — total bytes used by resource types
        5 => {
            if data.len() < 8 {
                return build_error_bo(BAD_REQUEST, seq, 0, XRES_MAJOR_OPCODE, minor as u16, bo);
            }
            let _client_xid = read_u32_bo(data, 4, bo);
            let num_specs = read_u32_bo(data, 8, bo) as usize;

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
            let extra_words = padded / 4;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, extra_words as u32, bo);
            write_u32_bo(&mut reply, 8, num_sizes, bo);

            let mut off = 32;
            for e in entries {
                let atom = {
                    let mut atoms = state.atoms.lock().unwrap();
                    atoms.intern(e.type_name, false)
                };
                write_u32_bo(&mut reply, off, atom, bo); // resource_type
                write_u32_bo(&mut reply, off + 4, e.count, bo); // count
                write_u32_bo(&mut reply, off + 8, e.bytes as u32, bo); // bytes (low)
                write_u32_bo(&mut reply, off + 12, (e.bytes >> 32) as u32, bo); // bytes (high)
                write_u32_bo(&mut reply, off + 16, 0, bo); // ref_count
                off += 20;
            }

            reply
        }

        _ => {
            debug!("Unhandled X-Resource minor opcode: {minor}");
            build_error_bo(
                BAD_REQUEST,
                seq,
                minor as u32,
                XRES_MAJOR_OPCODE,
                minor as u16,
                bo,
            )
        }
    }
}
