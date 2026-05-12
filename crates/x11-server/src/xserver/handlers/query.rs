//! Query and miscellaneous handlers (opcodes 97-99).

use super::*;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xproto::{
    ListExtensionsReply, ListExtensionsRequest, QueryBestSizeReply, QueryBestSizeRequest,
    QueryExtensionReply, QueryExtensionRequest, Str,
};

// ---------------------------------------------------------------------------
// Opcode 97: QueryBestSize
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_best_size(state: &ClientState, req: &QueryBestSizeRequest) -> Vec<u8> {
    let seq = state.sequence;
    let class = u8::from(req.class); // 0=Cursor, 1=Tile, 2=Stipple
    let width = req.width;
    let height = req.height;

    if class > 2 {
        return build_error(VALUE_ERROR, seq, class as u32, 97, 0);
    }

    let (best_w, best_h) = match class {
        0 => {
            // Cursor: most hardware has a max cursor size.  We support any
            // size in software; clamp to a reasonable 256×256 maximum.
            (width.min(256).max(1), height.min(256).max(1))
        }
        1 | 2 => {
            // Tile / Stipple: snap to nearest power-of-two for efficient
            // tiling when the hardware would benefit. Our software renderer
            // handles any size, but returning power-of-two is conventional.
            (next_power_of_two(width), next_power_of_two(height))
        }
        _ => (width, height),
    };

    serialize_reply(
        &QueryBestSizeReply {
            sequence: seq,
            length: 0,
            width: best_w,
            height: best_h,
        },
        state.byte_order(),
    )
}

/// Round up to the nearest power of two, with a minimum of 1.
fn next_power_of_two(v: u16) -> u16 {
    if v == 0 {
        return 1;
    }
    let v32 = v as u32;
    (v32.next_power_of_two() as u16).max(1)
}

// ---------------------------------------------------------------------------
// Opcode 98: QueryExtension
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_extension(
    state: &mut ClientState,
    req: &QueryExtensionRequest,
) -> Vec<u8> {
    let seq = state.sequence;
    let name = std::str::from_utf8(&req.name).unwrap_or("");

    debug!("QueryExtension: \"{}\"", name);

    let (present, major_opcode, first_event, first_error) =
        match state.extension_registry.by_name(name) {
            Some(info) if info.enabled => {
                (true, info.major_opcode, info.first_event, info.first_error)
            }
            _ => (false, 0, 0, 0),
        };

    serialize_reply(
        &QueryExtensionReply {
            sequence: seq,
            length: 0,
            present,
            major_opcode,
            first_event,
            first_error,
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 99: ListExtensions
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_extensions(state: &ClientState, _req: &ListExtensionsRequest) -> Vec<u8> {
    let seq = state.sequence;
    let names: Vec<Str> = state
        .extension_registry
        .enabled_extensions()
        .map(|info| Str {
            name: info.wire_name.as_bytes().to_vec(),
        })
        .collect();

    serialize_var_reply(
        &ListExtensionsReply {
            sequence: seq,
            length: 0,
            names,
        },
        state.byte_order(),
    )
}
