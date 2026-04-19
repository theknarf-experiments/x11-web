//! DRI3 extension handler.
//!
//! DRI3 enables zero-copy buffer sharing between the X server and GPU clients
//! via DMA-BUF file descriptors. Our implementation provides version negotiation
//! and basic fd-backed pixmap import so Mesa's software fallback path works.
use crate::xserver::reply::ReplyBuf;

// DRI3 minor opcodes:
// 0 = QueryVersion
// 1 = Open
// 2 = PixmapFromBuffer
// 3 = BufferFromPixmap
// 4 = FenceFromFD
// 5 = FDFromFence
// (DRI3 1.2+)
// 6 = GetSupportedModifiers
// 7 = PixmapFromBuffers
// 8 = BuffersFromPixmap
// (DRI3 1.4+)
// 9 = SetDRMDeviceInUse

mod device;
mod fence;
mod pixmap;

use tracing::{debug, warn};

use super::super::client::ClientState;
use super::super::core::*;
use crate::xserver::core::require_len;

/// DRI3 major opcode (assigned in QueryExtension).
#[allow(dead_code)]
pub(crate) const DRI3_MAJOR_OPCODE: u8 = 149;

// Supported DRI3 version
const DRI3_MAJOR_VERSION: u32 = 1;
const DRI3_MINOR_VERSION: u32 = 4;

pub(crate) fn handle_dri3_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 4, seq, DRI3_MAJOR_OPCODE, 0, state.msb_first);
    let minor = data[1];
    let bo = state.msb_first;

    match minor {
        // -----------------------------------------------------------------
        // 0: QueryVersion
        // -----------------------------------------------------------------
        0 => {
            require_len!(data, 12, seq, DRI3_MAJOR_OPCODE, minor as u16, bo);
            let client_major = read_u32_bo(data, 4, bo);
            let client_minor = read_u32_bo(data, 8, bo);

            let reply_major = client_major.min(DRI3_MAJOR_VERSION);
            let reply_minor = if client_major == DRI3_MAJOR_VERSION {
                client_minor.min(DRI3_MINOR_VERSION)
            } else if client_major > DRI3_MAJOR_VERSION {
                DRI3_MINOR_VERSION
            } else {
                client_minor
            };

            debug!("DRI3 QueryVersion: client={client_major}.{client_minor} -> reply={reply_major}.{reply_minor}");

            ReplyBuf::fixed(seq, bo)
                .set_u32(8, reply_major)
                .set_u32(12, reply_minor)
                .build()
        }

        // -----------------------------------------------------------------
        // 1: Open
        // -----------------------------------------------------------------
        1 => {
            // Open /dev/dri/renderD128 and return the fd via SCM_RIGHTS.
            // If no render node exists, return BadAlloc.
            debug!("DRI3 Open");

            let fd = unsafe {
                let path = b"/dev/dri/renderD128\0";
                libc::open(
                    path.as_ptr() as *const libc::c_char,
                    libc::O_RDWR | libc::O_CLOEXEC,
                )
            };

            if fd < 0 {
                // No GPU available — return BadAlloc
                warn!("DRI3 Open: failed to open /dev/dri/renderD128");
                return build_error_bo(ALLOC_ERROR, seq, 0, DRI3_MAJOR_OPCODE, 1, bo);
            }

            // Queue the fd for sending via SCM_RIGHTS
            state.reply_fds.push(fd);

            // Build the reply: 1 byte nfd (in unused/pad area), then 32-byte reply
            ReplyBuf::fixed(seq, bo)
                .set_data_byte(1) // nfd
                .build()
        }

        // Pixmap operations
        2 => pixmap::handle_pixmap_from_buffer(state, data, seq, minor, bo),
        3 => pixmap::handle_buffer_from_pixmap(state, data, seq, bo),
        7 => pixmap::handle_pixmap_from_buffers(state, data, seq, minor, bo),
        8 => pixmap::handle_buffers_from_pixmap(state, data, seq, bo),

        // Fence operations
        4 => fence::handle_fence_from_fd(state, data, seq, minor, bo),
        5 => fence::handle_fd_from_fence(state, data, seq, bo),

        // Device/modifier operations
        6 => device::handle_get_supported_modifiers(state, data, seq, minor, bo),
        9 => device::handle_set_drm_device_in_use(state, data, seq, minor, bo),

        _ => {
            warn!("Unhandled DRI3 minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                149,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
