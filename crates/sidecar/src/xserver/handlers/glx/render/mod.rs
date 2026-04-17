//! GL render opcode dispatcher (called from GLX_RENDER and GLX_RENDER_LARGE).

#[cfg(feature = "osmesa")]
mod render_draw;
#[cfg(feature = "osmesa")]
mod render_matrix;
#[cfg(feature = "osmesa")]
mod render_state;
#[cfg(feature = "osmesa")]
mod render_texture;

use super::super::super::client::ClientState;
use crate::xserver::core::require_len;
use tracing::warn;

// ---------------------------------------------------------------------------
// GLX_RENDER (minor 1) -- batched GL commands
// ---------------------------------------------------------------------------

pub(crate) fn handle_render(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 1, state.msb_first);
    let _tag = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    #[cfg(feature = "osmesa")]
    {
        // Ensure the context is current
        if let Some(ctx) = state.glx.contexts.get_mut(&state.glx.current_context) {
            if let Some(ref mut mesa) = ctx.mesa {
                mesa.make_current();
            }
        }
    }

    // Parse batched render commands starting at offset 8
    let mut off = 8;
    while off + 4 <= data.len() {
        let render_opcode = u16::from_le_bytes([data[off], data[off + 1]]);
        let cmd_len = u16::from_le_bytes([data[off + 2], data[off + 3]]) as usize;

        if cmd_len < 4 || off + cmd_len > data.len() {
            break;
        }

        let cmd_data = &data[off + 4..off + cmd_len];

        #[cfg(feature = "osmesa")]
        {
            dispatch_render_opcode(render_opcode, cmd_data);
        }

        off += cmd_len;
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_RENDER_LARGE (minor 2) -- for commands larger than max request size
// ---------------------------------------------------------------------------

pub(crate) fn handle_render_large(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // RenderLarge has: tag(4), request_num(2), request_total(2), data_len(4), data(...)
    // For simplicity, treat same as Render with the payload starting at offset 16
    require_len!(data, 16, seq, 159, 2, state.msb_first);

    #[cfg(feature = "osmesa")]
    {
        if let Some(ctx) = state.glx.contexts.get_mut(&state.glx.current_context) {
            if let Some(ref mut mesa) = ctx.mesa {
                mesa.make_current();
            }
        }

        let payload_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        let payload_start = 16;
        let payload_end = (payload_start + payload_len).min(data.len());

        let mut off = payload_start;
        while off + 4 <= payload_end {
            let render_opcode = u16::from_le_bytes([data[off], data[off + 1]]);
            let cmd_len = u16::from_le_bytes([data[off + 2], data[off + 3]]) as usize;
            if cmd_len < 4 || off + cmd_len > payload_end {
                break;
            }
            let cmd_data = &data[off + 4..off + cmd_len];
            dispatch_render_opcode(render_opcode, cmd_data);
            off += cmd_len;
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// GL render opcode dispatcher
// ---------------------------------------------------------------------------

#[cfg(feature = "osmesa")]
fn dispatch_render_opcode(opcode: u16, data: &[u8]) {
    // Try each submodule in turn; the first to return Some signals it handled the opcode.
    if render_draw::dispatch(opcode, data).is_some() {
        return;
    }
    if render_state::dispatch(opcode, data).is_some() {
        return;
    }
    if render_texture::dispatch(opcode, data).is_some() {
        return;
    }
    if render_matrix::dispatch(opcode, data).is_some() {
        return;
    }

    // Unknown opcodes are silently skipped — returning an error would crash clients
    // that send vendor/extension render commands we don't implement.
    warn!(
        "Unhandled GLX render opcode: {opcode} (data len: {}), skipping",
        data.len()
    );
}
