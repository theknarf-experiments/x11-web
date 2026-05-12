//! X-Resource extension handler (XRes).
//!
//! Reports per-client resource usage. Used by tools like `xrestop` and
//! desktop environments for memory monitoring.

use tracing::debug;

use super::super::client::ClientState;
use super::super::core::*;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use crate::xserver::request::request_header;
use x11rb_protocol::protocol::res::{
    Client, ClientIdMask, ClientIdSpec, ClientIdValue, QueryClientIdsReply,
    QueryClientIdsRequest, QueryClientPixmapBytesReply, QueryClientResourcesReply,
    QueryClientsReply, QueryResourceBytesReply, QueryResourceBytesRequest, QueryVersionReply,
    ResourceIdSpec, ResourceSizeSpec, ResourceSizeValue, Type,
};

/// X-Resource major opcode (assigned in QueryExtension).
const XRES_MAJOR_OPCODE: u8 = 160;

pub(crate) fn handle_xresource_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
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
        0 => serialize_reply(
            &QueryVersionReply {
                sequence: seq,
                length: 0,
                server_major: 1,
                server_minor: 2,
            },
            state.byte_order(),
        ),

        // 1: QueryClients — return list of connected client XIDs
        1 => {
            let client_bases = state.client_registry.lock().unwrap().clone();
            let clients: Vec<Client> = client_bases
                .into_iter()
                .map(|resource_base| Client {
                    resource_base,
                    resource_mask: RESOURCE_ID_MASK,
                })
                .collect();
            serialize_var_reply(
                &QueryClientsReply {
                    sequence: seq,
                    length: 0,
                    clients,
                },
                state.byte_order(),
            )
        }

        // 2: QueryClientResources — return resource type counts for a client
        2 => {
            if data.len() < 8 {
                return bad_request(0);
            }

            struct TypeCount {
                type_name: &'static str,
                count: u32,
            }
            let candidates = [
                TypeCount {
                    type_name: "WINDOW",
                    count: state.windows.len() as u32,
                },
                TypeCount {
                    type_name: "PIXMAP",
                    count: state.pixmaps.len() as u32,
                },
                TypeCount {
                    type_name: "GC",
                    count: state.gcs.len() as u32,
                },
                TypeCount {
                    type_name: "CURSOR",
                    count: state.cursors.len() as u32,
                },
                TypeCount {
                    type_name: "COLORMAP",
                    count: state.colormaps.len() as u32,
                },
                TypeCount {
                    type_name: "FONT",
                    count: 1,
                },
                TypeCount {
                    type_name: "PICTURE",
                    count: state.render.picture_count() as u32,
                },
                TypeCount {
                    type_name: "GLYPHSET",
                    count: state.render.glyphset_count() as u32,
                },
            ];

            let types: Vec<Type> = {
                let mut atoms = state.atoms.lock().unwrap();
                candidates
                    .iter()
                    .filter(|t| t.count > 0)
                    .map(|t| Type {
                        resource_type: atoms.intern(t.type_name, false),
                        count: t.count,
                    })
                    .collect()
            };

            serialize_var_reply(
                &QueryClientResourcesReply {
                    sequence: seq,
                    length: 0,
                    types,
                },
                state.byte_order(),
            )
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

            serialize_reply(
                &QueryClientPixmapBytesReply {
                    sequence: seq,
                    length: 0,
                    bytes: total_bytes as u32,
                    bytes_overflow: (total_bytes >> 32) as u32,
                },
                state.byte_order(),
            )
        }

        // 4: QueryClientIds (XRes 1.2) — return client IDs with their types
        4 => {
            let Ok(req) =
                QueryClientIdsRequest::try_parse_request(request_header(data), &data[4..])
            else {
                return bad_request(0);
            };

            let client_bases = state.client_registry.lock().unwrap().clone();
            let mut ids: Vec<ClientIdValue> = Vec::new();

            for spec in req.specs.iter() {
                let client_xid = spec.client;
                let mask = u32::from(spec.mask);

                if mask == 0 {
                    continue;
                }

                let targets: Vec<u32> = if client_xid == 0 {
                    client_bases.clone()
                } else {
                    let base = client_xid & !RESOURCE_ID_MASK;
                    if client_bases.contains(&base) {
                        vec![base]
                    } else {
                        vec![client_xid]
                    }
                };

                for &base in &targets {
                    if mask & u32::from(ClientIdMask::CLIENT_XID) != 0 {
                        ids.push(ClientIdValue {
                            spec: ClientIdSpec {
                                client: base,
                                mask: ClientIdMask::CLIENT_XID,
                            },
                            value: vec![base],
                        });
                    }
                    if mask & u32::from(ClientIdMask::LOCAL_CLIENT_PID) != 0 {
                        ids.push(ClientIdValue {
                            spec: ClientIdSpec {
                                client: base,
                                mask: ClientIdMask::LOCAL_CLIENT_PID,
                            },
                            value: vec![std::process::id()],
                        });
                    }
                }
            }

            serialize_var_reply(
                &QueryClientIdsReply {
                    sequence: seq,
                    length: 0,
                    ids,
                },
                state.byte_order(),
            )
        }

        // 5: QueryResourceBytes (XRes 1.2) — total bytes used by resource types
        5 => {
            let Ok(_req) =
                QueryResourceBytesRequest::try_parse_request(request_header(data), &data[4..])
            else {
                return bad_request(0);
            };

            let window_bytes: u64 = state.windows.len() as u64 * 256;
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
            let candidates = [
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

            let sizes: Vec<ResourceSizeValue> = {
                let mut atoms = state.atoms.lock().unwrap();
                candidates
                    .iter()
                    .filter(|e| e.count > 0)
                    .map(|e| ResourceSizeValue {
                        size: ResourceSizeSpec {
                            spec: ResourceIdSpec {
                                resource: 0,
                                type_: atoms.intern(e.type_name, false),
                            },
                            bytes: e.bytes as u32,
                            ref_count: e.count,
                            use_count: 0,
                        },
                        cross_references: Vec::new(),
                    })
                    .collect()
            };

            serialize_var_reply(
                &QueryResourceBytesReply {
                    sequence: seq,
                    length: 0,
                    sizes,
                },
                state.byte_order(),
            )
        }

        _ => {
            debug!("Unhandled X-Resource minor opcode: {minor}");
            bad_request(minor as u32)
        }
    }
}
