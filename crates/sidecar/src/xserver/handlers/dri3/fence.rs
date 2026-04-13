//! DRI3 fence operations: FenceFromFD, FDFromFence.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::DRI3_MAJOR_OPCODE;

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
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
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

    let _drawable = read_u32_bo(data, 4, bo);
    let fence_id = read_u32_bo(data, 8, bo);
    debug!("DRI3 FDFromFence: fence={fence_id:#x}");

    // Look up the fence in SYNC state
    if let Some(fence) = state.sync_state.fences.get(&fence_id) {
        if fence.fd >= 0 {
            // Duplicate the existing fd to return to the client
            let dup_fd = unsafe { libc::dup(fence.fd) };
            if dup_fd >= 0 {
                state.reply_fds.push(dup_fd);
                let mut reply = [0u8; 32];
                reply[0] = 1; // Reply
                reply[1] = 1; // nfd
                write_u16_bo(&mut reply, 2, seq, bo);
                write_u32_bo(&mut reply, 4, 0, bo); // length
                return reply.to_vec();
            }
        }
        // No fd backing — create an eventfd to represent the fence state
        let efd = unsafe {
            let initial: libc::c_uint = if fence.triggered { 1 } else { 0 };
            libc::eventfd(initial, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)
        };
        if efd >= 0 {
            state.reply_fds.push(efd);
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // nfd
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, 0, bo); // length
            reply.to_vec()
        } else {
            warn!("DRI3 FDFromFence: eventfd creation failed");
            build_error_bo(BAD_ALLOC, seq, fence_id, DRI3_MAJOR_OPCODE, 5, bo)
        }
    } else {
        warn!("DRI3 FDFromFence: unknown fence {fence_id:#x}");
        build_error_bo(BAD_VALUE, seq, fence_id, DRI3_MAJOR_OPCODE, 5, bo)
    }
}
