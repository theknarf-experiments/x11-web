//! DRI3 fence operations: FenceFromFD, FDFromFence.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::super::parse_minor;
use super::DRI3_MAJOR_OPCODE;
use crate::xserver::reply::ReplyBuf;

// -----------------------------------------------------------------
// 4: FenceFromFD — create a SYNC fence backed by an fd
// -----------------------------------------------------------------
pub(crate) fn handle_fence_from_fd(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
    bo: bool,
) -> Vec<u8> {
    // Request: drawable(4), fence(4), initially_triggered(1), pad(3)
    if data.len() < 16 {
        if let Some(fd) = state.pending_fds.pop() {
            unsafe {
                libc::close(fd);
            }
        }
        return super::dri3_err(LENGTH_ERROR, seq, 0, minor);
    }

    let _drawable = read_u32_bo(data, 4, bo);
    let fence_id = read_u32_bo(data, 8, bo);
    let initially_triggered = data[12] != 0;

    let fd = state.pending_fds.pop().unwrap_or(-1);
    debug!(
        "DRI3 FenceFromFD: fence={fence_id:#x} fd={fd} initially_triggered={initially_triggered}"
    );

    // Register with the SYNC extension's fence tracking
    use super::super::sync::FenceState;
    state.sync_state.fences.insert(
        fence_id,
        FenceState {
            id: fence_id,
            triggered: initially_triggered,
            initially_triggered,
            fd,
        },
    );

    Vec::new() // void request
}

// -----------------------------------------------------------------
// 5: FDFromFence — export a SYNC fence as a file descriptor
// -----------------------------------------------------------------
pub(crate) fn handle_fd_from_fence(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    bo: bool,
) -> Vec<u8> {
    require_len!(data, 12, seq, DRI3_MAJOR_OPCODE, 5, bo);

    use x11rb_protocol::protocol::dri3::FDFromFenceRequest;
    let req = parse_minor!(FDFromFenceRequest, data, state, seq, DRI3_MAJOR_OPCODE, 5u8);
    let fence_id = req.fence;
    debug!("DRI3 FDFromFence: fence={fence_id:#x}");

    // Look up the fence in SYNC state
    if let Some(fence) = state.sync_state.fences.get(&fence_id) {
        if fence.fd >= 0 {
            // Duplicate the existing fd to return to the client
            let dup_fd = unsafe { libc::dup(fence.fd) };
            if dup_fd >= 0 {
                state.reply_fds.push(dup_fd);
                return ReplyBuf::fixed(seq, bo)
                    .set_data_byte(1) // nfd
                    .build();
            }
        }
        // No fd backing — create an eventfd to represent the fence state
        let efd = unsafe {
            let initial: libc::c_uint = if fence.triggered { 1 } else { 0 };
            libc::eventfd(initial, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)
        };
        if efd >= 0 {
            state.reply_fds.push(efd);
            ReplyBuf::fixed(seq, bo)
                .set_data_byte(1) // nfd
                .build()
        } else {
            warn!("DRI3 FDFromFence: eventfd creation failed");
            super::dri3_err(ALLOC_ERROR, seq, fence_id, 5)
        }
    } else {
        warn!("DRI3 FDFromFence: unknown fence {fence_id:#x}");
        super::dri3_err(VALUE_ERROR, seq, fence_id, 5)
    }
}
