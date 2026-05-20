//! Top-level X11 request dispatch: routes opcodes to core or extension handlers.
//!
//! Extension dispatch consults the [`ExtensionRegistry`]: each
//! [`ExtensionInfo`] carries a handler function pointer that the
//! dispatcher calls directly, so adding a new extension is a one-file
//! change (register the entry, point at its `handle_*_request`).

use tracing::warn;

use super::client::ClientState;
use super::core::*;
use super::handlers;

/// Minimum length of an X11 request header: major opcode (1) + minor opcode
/// or data byte (1) + length-in-words (2).
const MIN_REQUEST_HEADER_LEN: usize = 4;

/// Dispatch an X11 request to the appropriate handler based on the major opcode.
pub(super) fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < MIN_REQUEST_HEADER_LEN {
        return Vec::new();
    }
    let major_opcode = data[0];
    let minor = data[1];
    let seq = state.sequence;

    // Core protocol requests (opcodes 1..=CORE_REQUEST_OPCODE_MAX).
    if major_opcode <= 127 {
        return handlers::handle_core_request(state, data);
    }

    // Extension protocol requests (opcodes 128+).
    let bad_request = || build_error(REQUEST_ERROR, seq, major_opcode as u32, major_opcode, minor as u16);
    let handler = match state.extension_registry.by_opcode(major_opcode) {
        Some(info) if !info.enabled => {
            warn!(
                "Request for disabled extension {:?} (opcode {major_opcode})",
                info.wire_name
            );
            return bad_request();
        }
        Some(info) => info.handler,
        None => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {minor}");
            return bad_request();
        }
    };
    handler(state, data, seq)
}
