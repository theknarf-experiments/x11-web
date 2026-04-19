//! Pixmap operations (opcodes 53-54).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::request::request_header;

// ---------------------------------------------------------------------------
// Opcode 53: CreatePixmap
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 16, state.sequence, 53);

    use x11rb_protocol::protocol::xproto::CreatePixmapRequest;
    let req = match CreatePixmapRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 53, 0),
    };
    let depth = req.depth;
    let pid = req.pid;

    // Validate resource ID is within this client's allocated range
    if !state.validate_resource_id(pid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, pid, 53, 0);
    }

    // Enforce per-client pixmap resource limit
    if !state.can_create_pixmap() {
        return build_error(ALLOC_ERROR, state.sequence, pid, 53, 0);
    }

    let _drawable = req.drawable;
    let width = req.width;
    let height = req.height;

    // Validate: width and height must be non-zero and within bounds
    if width == 0 || height == 0 {
        return build_error(VALUE_ERROR, state.sequence, 0, 53, 0);
    }
    if width > 32767 || height > 32767 {
        return build_error(VALUE_ERROR, state.sequence, width as u32, 53, 0);
    }
    // Validate: depth must match one of the supported pixmap formats.
    // Per X11 spec, the server advertises supported depths in the Setup reply.
    // We support: 1 (bitmap), 4, 8 (PseudoColor), 16 (HighColor), 24, 32 (TrueColor).
    if !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
        return build_error(VALUE_ERROR, state.sequence, depth as u32, 53, 0);
    }
    // Validate: ID must not already be in use
    if state.pixmaps.contains_key(&pid) || state.windows.contains_key(&pid) {
        return build_error(ID_CHOICE_ERROR, state.sequence, pid, 53, 0);
    }

    info!(
        "CreatePixmap: pid={pid:#x} {}x{} depth={depth}",
        width, height
    );

    state.pixmaps.insert(
        pid,
        PixmapState {
            width,
            height,
            depth,
            framebuffer: Framebuffer::new(width as u32, height as u32),
            alias_window: None,
            shm_backing: None,
        },
    );

    // Register in shared pixmap registry for cross-connection access
    state.register_shared_pixmap(pid, width, height, depth);

    Vec::new()
}

// ---------------------------------------------------------------------------
// Opcode 54: FreePixmap
// ---------------------------------------------------------------------------

pub(crate) fn handle_free_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    require_len!(data, 8, state.sequence, 54);
    use x11rb_protocol::protocol::xproto::FreePixmapRequest;
    let req = match FreePixmapRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, state.sequence, 0, 54, 0),
    };
    let pid = req.pixmap;
    if !state.pixmaps.contains_key(&pid) {
        return build_error(PIXMAP_ERROR, state.sequence, pid, 54, 0);
    }
    state.pixmaps.remove(&pid);
    // Unregister from shared registry
    state.unregister_shared_pixmap(pid);
    state.recycle_xid(pid);
    Vec::new()
}
