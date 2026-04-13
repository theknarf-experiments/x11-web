//! Query and miscellaneous handlers (opcodes 97-99).

use super::*;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// Opcode 97: QueryBestSize
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_best_size(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 97);
    let class = data[1]; // 0=Cursor, 1=Tile, 2=Stipple
    let width = state.read_u16(data, 8);
    let height = state.read_u16(data, 10);

    // Per X11 spec §8.5.2:
    // - Cursor: return the closest size that the display can support.
    //   Our software implementation supports any size, so return as-is.
    // - Tile: return the size snapped to a power-of-two or the closest
    //   size the server can tile efficiently.  In software, any size works.
    // - Stipple: similar to Tile.
    // Validate class is 0, 1, or 2.
    if class > 2 {
        return build_error(BAD_VALUE, seq, class as u32, 97, 0);
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

    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    state.write_u16(&mut reply, 8, best_w);
    state.write_u16(&mut reply, 10, best_h);
    reply.to_vec()
}

/// Round up to the nearest power of two, with a minimum of 1.
fn next_power_of_two(v: u16) -> u16 {
    if v == 0 { return 1; }
    let v32 = v as u32;
    (v32.next_power_of_two() as u16).max(1)
}

// ---------------------------------------------------------------------------
// Opcode 98: QueryExtension
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_extension(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 98);
    // Parse extension name from the request
    let name_len = _state.read_u16(data, 4) as usize;
    let name = if data.len() >= 8 + name_len {
        std::str::from_utf8(&data[8..8 + name_len]).unwrap_or("")
    } else {
        ""
    };

    debug!("QueryExtension: \"{}\"", name);

    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    _state.write_u16(&mut reply, 2, seq);

    match name {
        "RENDER" => {
            reply[8] = 1; // present = true
            reply[9] = 139; // major_opcode
            reply[10] = 0; // first_event (RENDER has no events)
            reply[11] = 142; // first_error: BadPictFormat=142, BadPicture=143, BadPictOp=144, BadGlyphSet=145, BadGlyph=146
        }
        "MIT-SHM" => {
            reply[8] = 1;
            reply[9] = 130;
            reply[10] = 65; // ShmCompletion
            reply[11] = 128;
        }
        "BIG-REQUESTS" => {
            reply[8] = 1;
            reply[9] = 133;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XFIXES" => {
            reply[8] = 1;
            reply[9] = 138;
            reply[10] = 87;
            reply[11] = 0;
        }
        "SHAPE" => {
            reply[8] = 1;
            reply[9] = 128;
            reply[10] = 64;
            reply[11] = 0;
        }
        "SYNC" => {
            reply[8] = 1;
            reply[9] = 134;
            reply[10] = 83; // first_event: AlarmNotify (must match handler event code)
            reply[11] = 0;
        }
        "Generic Event Extension" => {
            reply[8] = 1;
            reply[9] = 135;
            reply[10] = 0;
            reply[11] = 0;
        }
        "Composite" => {
            reply[8] = 1;
            reply[9] = 142;
            reply[10] = 0; // first_event (no events; Composite uses Damage events)
            reply[11] = 0; // first_error (no extension-specific errors)
        }
        "DAMAGE" => {
            reply[8] = 1;
            reply[9] = 143;
            reply[10] = 91;
            reply[11] = 152;
        }
        "RANDR" => {
            reply[8] = 1;
            reply[9] = 140;
            reply[10] = crate::xserver::types::RANDR_EVENT_BASE;
            reply[11] = 0;
        }
        "XKEYBOARD" => {
            reply[8] = 1;
            reply[9] = 136;
            reply[10] = 85; // first_event: XkbEventCode
            reply[11] = 0;
        }
        "XC-MISC" => {
            reply[8] = 1;
            reply[9] = 141;
            reply[10] = 0;
            reply[11] = 0;
        }
        "Present" => {
            reply[8] = 1;
            reply[9] = 148;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XInputExtension" => {
            reply[8] = 1;
            reply[9] = crate::xinput2::XI_MAJOR_OPCODE;
            reply[10] = crate::xinput2::XI_FIRST_EVENT;
            reply[11] = crate::xinput2::XI_FIRST_ERROR;
        }
        "XTEST" => {
            reply[8] = 1;
            reply[9] = 150;
            reply[10] = 0;
            reply[11] = 0;
        }
        "DPMS" => {
            reply[8] = 1;
            reply[9] = 151;
            reply[10] = 0;
            reply[11] = 0;
        }
        "MIT-SCREEN-SAVER" => {
            reply[8] = 1;
            reply[9] = 152;
            reply[10] = 0;
            reply[11] = 0;
        }
        "XFree86-VidModeExtension" => {
            reply[8] = 1;
            reply[9] = 153;
            reply[10] = 0;
            reply[11] = 0;
        }
        "RECORD" => {
            reply[8] = 1;
            reply[9] = 154;
            reply[10] = 0; // first_event (no events)
            reply[11] = 154; // first_error: BadContext
        }
        "SECURITY" => {
            reply[8] = 1;
            reply[9] = 155;
            reply[10] = 93; // first_event: SecurityAuthorizationRevoked
            reply[11] = 155; // first_error: BadAuthorization
        }
        "XVideo" => {
            reply[8] = 1;
            reply[9] = 156;
            reply[10] = 95; // first_event: XvVideoNotify=95, XvPortNotify=96
            reply[11] = 156; // first_error: XvBadPort, XvBadEncoding, XvBadControl
        }
        "DOUBLE-BUFFER" => {
            reply[8] = 1;
            reply[9] = 157;
            reply[10] = 0; // first_event (no events)
            reply[11] = 157; // first_error: BadBuffer
        }
        "XINERAMA" => {
            reply[8] = 1;
            reply[9] = 158;
            reply[10] = 0; // first_event (no events)
            reply[11] = 0; // first_error (no errors)
        }
        "GLX" => {
            reply[8] = 1;
            reply[9] = 159;
            reply[10] = 0; // first_event (GLX uses GenericEvent via GE)
            reply[11] = 159; // first_error: GLXBadContext=159, GLXBadContextState=160, etc.
        }
        "X-Resource" => {
            reply[8] = 1;
            reply[9] = 160;
            reply[10] = 0;
            reply[11] = 0;
        }
        "DRI3" => {
            reply[8] = 1;
            reply[9] = 149;
            reply[10] = 0;
            reply[11] = 0;
        }
        _ => {
            // present = false (byte 8 = 0) -- already zero
        }
    }

    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 99: ListExtensions
// ---------------------------------------------------------------------------

pub(crate) fn handle_list_extensions(state: &ClientState, seq: u16) -> Vec<u8> {
    let extensions: &[&str] = &["BIG-REQUESTS", "MIT-SHM", "RENDER", "XFIXES", "SHAPE", "SYNC", "Generic Event Extension", "XC-MISC", "Composite", "DAMAGE", "Present", "RANDR", "XInputExtension", "XKEYBOARD", "XTEST", "DPMS", "MIT-SCREEN-SAVER", "XFree86-VidModeExtension", "RECORD", "SECURITY", "XVideo", "DOUBLE-BUFFER", "XINERAMA", "GLX", "DRI3", "X-Resource"];

    let mut names_data = Vec::new();
    for ext in extensions {
        names_data.push(ext.len() as u8);
        names_data.extend_from_slice(ext.as_bytes());
    }
    while names_data.len() % 4 != 0 {
        names_data.push(0);
    }

    let extra_len = names_data.len();
    let mut reply = vec![0u8; 32 + extra_len];
    reply[0] = 1; // Reply
    reply[1] = extensions.len() as u8;
    state.write_u16(&mut reply, 2, seq);
    state.write_u32(&mut reply, 4, (extra_len / 4) as u32);
    reply[32..].copy_from_slice(&names_data);

    reply
}
