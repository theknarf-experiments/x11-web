//! Query and miscellaneous handlers (opcodes 97-99).

use super::*;

// ---------------------------------------------------------------------------
// Opcode 97: QueryBestSize
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_best_size(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return build_error(BAD_LENGTH, seq, 0, 97, 0);
    }
    let mut reply = [0u8; 32];
    reply[0] = 1;
    state.write_u16(&mut reply, 2, seq);
    let width = state.read_u16(data, 8);
    let height = state.read_u16(data, 10);
    state.write_u16(&mut reply, 8, width);
    state.write_u16(&mut reply, 10, height);
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// Opcode 98: QueryExtension
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_extension(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 { return build_error(BAD_LENGTH, seq, 0, 98, 0); }
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
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
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
            reply[10] = 100;
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
            reply[10] = 0;
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
        }
        "SECURITY" => {
            reply[8] = 1;
            reply[9] = 155;
        }
        "XVideo" => {
            reply[8] = 1;
            reply[9] = 156;
        }
        "DOUBLE-BUFFER" => {
            reply[8] = 1;
            reply[9] = 157;
        }
        "XINERAMA" => {
            reply[8] = 1;
            reply[9] = 158;
        }
        "GLX" => {
            reply[8] = 1;
            reply[9] = 159;
            reply[10] = 0;
            reply[11] = 0;
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
