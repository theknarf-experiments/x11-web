//! Query and miscellaneous handlers (opcodes 97-99).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;

// ---------------------------------------------------------------------------
// Opcode 97: QueryBestSize
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_best_size(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 97);
    use x11rb_protocol::protocol::xproto::QueryBestSizeRequest;
    let req = match QueryBestSizeRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 97, 0),
    };
    let class = u8::from(req.class); // 0=Cursor, 1=Tile, 2=Stipple
    let width = req.width;
    let height = req.height;

    // Per X11 spec §8.5.2:
    // - Cursor: return the closest size that the display can support.
    //   Our software implementation supports any size, so return as-is.
    // - Tile: return the size snapped to a power-of-two or the closest
    //   size the server can tile efficiently.  In software, any size works.
    // - Stipple: similar to Tile.
    // Validate class is 0, 1, or 2.
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

    ReplyBuf::fixed(seq, state.msb_first)
        .set_u16(8, best_w)
        .set_u16(10, best_h)
        .build()
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

pub(crate) fn handle_query_extension(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 98);
    use x11rb_protocol::protocol::xproto::QueryExtensionRequest;
    let req = match QueryExtensionRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 98, 0),
    };
    let name = std::str::from_utf8(&req.name).unwrap_or("");

    debug!("QueryExtension: \"{}\"", name);

    let mut reply = ReplyBuf::fixed(seq, _state.msb_first);

    // Look up the extension in the registry.
    if let Some(info) = _state.extension_registry.by_name(name) {
        if info.enabled {
            reply = reply
                .set_u8(8, 1) // present = true
                .set_u8(9, info.major_opcode)
                .set_u8(10, info.first_event)
                .set_u8(11, info.first_error);
        }
        // else: present = false (byte 8 = 0) — extension disabled
    }
    // else: present = false — extension unknown

    reply.build()
}

// ---------------------------------------------------------------------------
// Opcode 99: ListExtensions
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_extensions(state: &ClientState, seq: u16) -> Vec<u8> {
    // Collect enabled extension wire names from the registry.
    let extensions: Vec<&str> = state
        .extension_registry
        .enabled_extensions()
        .map(|info| info.wire_name)
        .collect();

    let mut names_data = Vec::new();
    for ext in &extensions {
        names_data.push(ext.len() as u8);
        names_data.extend_from_slice(ext.as_bytes());
    }
    while names_data.len() % 4 != 0 {
        names_data.push(0);
    }

    let extra_len = names_data.len();
    ReplyBuf::with_extra(seq, extra_len, state.msb_first)
        .set_data_byte(extensions.len() as u8)
        .set_bytes(32, &names_data)
        .build()
}
