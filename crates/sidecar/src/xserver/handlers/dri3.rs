//! DRI3 extension handler.
//!
//! DRI3 enables zero-copy buffer sharing between the X server and GPU clients
//! via DMA-BUF file descriptors. Our implementation provides version negotiation
//! and basic fd-backed pixmap import so Mesa's software fallback path works.

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

use tracing::{debug, warn};

use super::super::client::ClientState;
use super::super::core::*;
use super::super::types::PixmapState;
use crate::framebuffer::Framebuffer;

// DRM fourcc codes for YUV formats
const FOURCC_NV12: u32 = 0x3231564E; // 'NV12'
const FOURCC_YV12: u32 = 0x32315659; // 'YV12'
const FOURCC_YUY2: u32 = 0x32595559; // 'YUY2'

/// Convert a single YUV pixel to packed 0xAARRGGBB using BT.601 coefficients.
#[inline]
fn yuv_to_argb(y: u8, u: u8, v: u8) -> u32 {
    let y_f = 1.164 * (y as f64 - 16.0);
    let u_f = u as f64 - 128.0;
    let v_f = v as f64 - 128.0;

    let r = (y_f + 1.596 * v_f).round().clamp(0.0, 255.0) as u8;
    let g = (y_f - 0.813 * v_f - 0.391 * u_f).round().clamp(0.0, 255.0) as u8;
    let b = (y_f + 2.018 * u_f).round().clamp(0.0, 255.0) as u8;

    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | (b as u32)
}

/// Read buffer contents from an fd via `pread`.
/// Returns the bytes read, or an empty Vec on failure.
fn read_fd_buffer(fd: i32, size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    let n = unsafe { libc::pread(fd, buf.as_mut_ptr() as *mut libc::c_void, size, 0) };
    if n <= 0 {
        return Vec::new();
    }
    buf.truncate(n as usize);
    buf
}

/// Convert NV12 multi-plane data to ARGB framebuffer pixels.
///
/// NV12: plane 0 = Y at full resolution, plane 1 = interleaved UV at half
/// resolution in both dimensions.
fn convert_nv12_to_fb(
    fds: &[i32],
    strides: &[u32; 4],
    offsets: &[u32; 4],
    width: usize,
    height: usize,
    fb: &mut Framebuffer,
) {
    let y_stride = strides[0] as usize;
    let y_offset = offsets[0] as usize;
    let uv_stride = strides[1] as usize;
    let uv_offset = offsets[1] as usize;

    let y_size = y_offset + y_stride * height;
    let uv_height = (height + 1) / 2;
    let uv_size = uv_offset + uv_stride * uv_height;

    let y_fd = if !fds.is_empty() && fds[0] >= 0 { fds[0] } else { return; };
    let uv_fd = if fds.len() > 1 && fds[1] >= 0 { fds[1] } else { y_fd };

    let y_buf = read_fd_buffer(y_fd, y_size);
    let uv_buf = if uv_fd == y_fd { y_buf.clone() } else { read_fd_buffer(uv_fd, uv_size) };

    if y_buf.is_empty() {
        return;
    }

    let dst = fb.data_mut();
    for row in 0..height {
        for col in 0..width {
            let y_idx = y_offset + row * y_stride + col;
            let uv_row = row / 2;
            let uv_col = (col / 2) * 2; // each UV pair is 2 bytes
            let uv_idx = uv_offset + uv_row * uv_stride + uv_col;

            let y_val = y_buf.get(y_idx).copied().unwrap_or(0);
            let u_val = uv_buf.get(uv_idx).copied().unwrap_or(128);
            let v_val = uv_buf.get(uv_idx + 1).copied().unwrap_or(128);

            let argb = yuv_to_argb(y_val, u_val, v_val);
            let dst_off = (row * width + col) * 4;
            if dst_off + 4 <= dst.len() {
                dst[dst_off..dst_off + 4].copy_from_slice(&argb.to_ne_bytes());
            }
        }
    }
}

/// Convert YV12 multi-plane data to ARGB framebuffer pixels.
///
/// YV12: plane 0 = Y at full resolution, plane 1 = V at half resolution,
/// plane 2 = U at half resolution.
fn convert_yv12_to_fb(
    fds: &[i32],
    strides: &[u32; 4],
    offsets: &[u32; 4],
    width: usize,
    height: usize,
    fb: &mut Framebuffer,
) {
    let y_stride = strides[0] as usize;
    let y_offset = offsets[0] as usize;
    let v_stride = strides[1] as usize;
    let v_offset = offsets[1] as usize;
    let u_stride = strides[2] as usize;
    let u_offset = offsets[2] as usize;

    let half_h = (height + 1) / 2;
    let y_size = y_offset + y_stride * height;
    let v_size = v_offset + v_stride * half_h;
    let u_size = u_offset + u_stride * half_h;

    let y_fd = if !fds.is_empty() && fds[0] >= 0 { fds[0] } else { return; };
    let v_fd = if fds.len() > 1 && fds[1] >= 0 { fds[1] } else { y_fd };
    let u_fd = if fds.len() > 2 && fds[2] >= 0 { fds[2] } else { v_fd };

    let y_buf = read_fd_buffer(y_fd, y_size);
    let v_buf = if v_fd == y_fd { y_buf.clone() } else { read_fd_buffer(v_fd, v_size) };
    let u_buf = if u_fd == y_fd {
        y_buf.clone()
    } else if u_fd == v_fd {
        v_buf.clone()
    } else {
        read_fd_buffer(u_fd, u_size)
    };

    if y_buf.is_empty() {
        return;
    }

    let dst = fb.data_mut();
    for row in 0..height {
        for col in 0..width {
            let y_idx = y_offset + row * y_stride + col;
            let uv_row = row / 2;
            let uv_col = col / 2;
            let v_idx = v_offset + uv_row * v_stride + uv_col;
            let u_idx = u_offset + uv_row * u_stride + uv_col;

            let y_val = y_buf.get(y_idx).copied().unwrap_or(0);
            let v_val = v_buf.get(v_idx).copied().unwrap_or(128);
            let u_val = u_buf.get(u_idx).copied().unwrap_or(128);

            let argb = yuv_to_argb(y_val, u_val, v_val);
            let dst_off = (row * width + col) * 4;
            if dst_off + 4 <= dst.len() {
                dst[dst_off..dst_off + 4].copy_from_slice(&argb.to_ne_bytes());
            }
        }
    }
}

/// Convert YUY2 packed data to ARGB framebuffer pixels.
///
/// YUY2: packed YUYV — every 4 bytes encode 2 horizontally adjacent pixels
/// sharing the same U and V values.
fn convert_yuy2_to_fb(
    fds: &[i32],
    strides: &[u32; 4],
    offsets: &[u32; 4],
    width: usize,
    height: usize,
    fb: &mut Framebuffer,
) {
    let stride0 = strides[0] as usize;
    let offset0 = offsets[0] as usize;

    // YUY2 is packed: 2 bytes per pixel, so stride >= width * 2
    let read_size = offset0 + stride0 * height;

    let fd = if !fds.is_empty() && fds[0] >= 0 { fds[0] } else { return; };
    let buf = read_fd_buffer(fd, read_size);
    if buf.is_empty() {
        return;
    }

    let dst = fb.data_mut();
    for row in 0..height {
        // Process pixel pairs
        let mut col = 0usize;
        while col < width {
            let src_off = offset0 + row * stride0 + col * 2;
            let y0 = buf.get(src_off).copied().unwrap_or(0);
            let u_val = buf.get(src_off + 1).copied().unwrap_or(128);
            let y1 = buf.get(src_off + 2).copied().unwrap_or(0);
            let v_val = buf.get(src_off + 3).copied().unwrap_or(128);

            // First pixel
            let argb0 = yuv_to_argb(y0, u_val, v_val);
            let dst_off0 = (row * width + col) * 4;
            if dst_off0 + 4 <= dst.len() {
                dst[dst_off0..dst_off0 + 4].copy_from_slice(&argb0.to_ne_bytes());
            }

            // Second pixel (if within bounds)
            if col + 1 < width {
                let argb1 = yuv_to_argb(y1, u_val, v_val);
                let dst_off1 = (row * width + col + 1) * 4;
                if dst_off1 + 4 <= dst.len() {
                    dst[dst_off1..dst_off1 + 4].copy_from_slice(&argb1.to_ne_bytes());
                }
            }

            col += 2;
        }
    }
}

/// DRI3 major opcode (assigned in QueryExtension).
#[allow(dead_code)]
pub(crate) const DRI3_MAJOR_OPCODE: u8 = 149;

// Supported DRI3 version
const DRI3_MAJOR_VERSION: u32 = 1;
const DRI3_MINOR_VERSION: u32 = 4;

pub(crate) fn handle_dri3_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 4 {
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, 0, state.msb_first);
    }
    let minor = data[1];
    let bo = state.msb_first;

    match minor {
        // -----------------------------------------------------------------
        // 0: QueryVersion
        // -----------------------------------------------------------------
        0 => {
            if data.len() < 12 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }
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

            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, 0, bo); // length
            write_u32_bo(&mut reply, 8, reply_major, bo);
            write_u32_bo(&mut reply, 12, reply_minor, bo);
            reply.to_vec()
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
                libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDWR | libc::O_CLOEXEC)
            };

            if fd < 0 {
                // No GPU available — return BadAlloc
                warn!("DRI3 Open: failed to open /dev/dri/renderD128");
                return build_error_bo(
                    BAD_ALLOC, seq, 0,
                    DRI3_MAJOR_OPCODE, 1, bo,
                );
            }

            // Queue the fd for sending via SCM_RIGHTS
            state.reply_fds.push(fd);

            // Build the reply: 1 byte nfd (in unused/pad area), then 32-byte reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // nfd
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, 0, bo); // length
            reply.to_vec()
        }

        // -----------------------------------------------------------------
        // 2: PixmapFromBuffer
        // -----------------------------------------------------------------
        2 => {
            // PixmapFromBuffer: create a pixmap from a DMA-BUF fd
            // Request: pixmap(4), drawable(4), size(4), width(2), height(2),
            //          stride(2), depth(1), bpp(1)
            if data.len() < 24 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let pixmap_id = read_u32_bo(data, 4, bo);
            let _drawable = read_u32_bo(data, 8, bo);
            let _size = read_u32_bo(data, 12, bo);
            let width = read_u16_bo(data, 16, bo);
            let height = read_u16_bo(data, 18, bo);
            let _stride = read_u16_bo(data, 20, bo);
            let depth = data[22];
            let _bpp = data[23];

            debug!("DRI3 PixmapFromBuffer: pid={pixmap_id:#x} {width}x{height} depth={depth}");

            // Consume the fd from pending_fds (received via SCM_RIGHTS)
            let fd = state.pending_fds.pop();
            if let Some(fd) = fd {
                // Read buffer data from the fd into the pixmap framebuffer
                let mut fb = Framebuffer::new(width as u32, height as u32);
                let buf_size = (width as usize) * (height as usize) * 4;
                let mut buf = vec![0u8; buf_size];

                unsafe {
                    let n = libc::pread(fd, buf.as_mut_ptr() as *mut libc::c_void, buf_size, 0);
                    if n > 0 {
                        fb.data_mut()[..n as usize].copy_from_slice(&buf[..n as usize]);
                    }
                    libc::close(fd);
                }

                state.pixmaps.insert(
                    pixmap_id,
                    PixmapState {
                        width,
                        height,
                        depth,
                        framebuffer: fb,
                        alias_window: None,
                        shm_backing: None,
                    },
                );
                state.register_shared_pixmap(pixmap_id, width, height, depth);
            } else {
                // No fd received — create an empty pixmap anyway
                warn!("DRI3 PixmapFromBuffer: no pending fd, creating empty pixmap");
                state.pixmaps.insert(
                    pixmap_id,
                    PixmapState {
                        width,
                        height,
                        depth,
                        framebuffer: Framebuffer::new(width as u32, height as u32),
                        alias_window: None,
                        shm_backing: None,
                    },
                );
                state.register_shared_pixmap(pixmap_id, width, height, depth);
            }

            Vec::new() // void request
        }

        // -----------------------------------------------------------------
        // 3: BufferFromPixmap
        // -----------------------------------------------------------------
        3 => {
            if data.len() < 8 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let pixmap_id = read_u32_bo(data, 4, bo);
            debug!("DRI3 BufferFromPixmap: pid={pixmap_id:#x}");

            let (width, height, depth, data_bytes) = if let Some(pix) = state.pixmaps.get(&pixmap_id) {
                (pix.width, pix.height, pix.depth, pix.framebuffer.data().to_vec())
            } else {
                warn!("DRI3 BufferFromPixmap: unknown pixmap {pixmap_id:#x}");
                return build_error_bo(
                    BAD_PIXMAP, seq, pixmap_id,
                    DRI3_MAJOR_OPCODE, 3, bo,
                );
            };

            // Create a memfd and write the pixmap data into it
            let fd = unsafe {
                let name = b"dri3-buffer\0";
                let fd = libc::memfd_create(name.as_ptr() as *const libc::c_char, libc::MFD_CLOEXEC);
                if fd >= 0 {
                    let _ = libc::ftruncate(fd, data_bytes.len() as libc::off_t);
                    let _ = libc::pwrite(
                        fd,
                        data_bytes.as_ptr() as *const libc::c_void,
                        data_bytes.len(),
                        0,
                    );
                }
                fd
            };

            if fd < 0 {
                warn!("DRI3 BufferFromPixmap: memfd_create failed");
                return build_error_bo(
                    BAD_ALLOC, seq, pixmap_id,
                    DRI3_MAJOR_OPCODE, 3, bo,
                );
            }

            state.reply_fds.push(fd);

            let stride = (width as u32) * 4;
            let size = data_bytes.len() as u32;
            let bpp = if depth == 1 { 1 } else { 32 };

            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // nfd
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, 0, bo); // length
            write_u32_bo(&mut reply, 8, size, bo);
            write_u16_bo(&mut reply, 12, width, bo);
            write_u16_bo(&mut reply, 14, height, bo);
            write_u16_bo(&mut reply, 16, stride as u16, bo);
            reply[18] = depth;
            reply[19] = bpp;
            reply.to_vec()
        }

        // -----------------------------------------------------------------
        // 4: FenceFromFD — create a SYNC fence backed by an fd
        // -----------------------------------------------------------------
        4 => {
            // Request: drawable(4), fence(4), initially_triggered(1), pad(3)
            if data.len() < 16 {
                if let Some(fd) = state.pending_fds.pop() {
                    unsafe { libc::close(fd); }
                }
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let _drawable = read_u32_bo(data, 4, bo);
            let fence_id = read_u32_bo(data, 8, bo);
            let initially_triggered = data[12] != 0;

            let fd = state.pending_fds.pop().unwrap_or(-1);
            debug!("DRI3 FenceFromFD: fence={fence_id:#x} fd={fd} initially_triggered={initially_triggered}");

            // Register with the SYNC extension's fence tracking
            use super::sync::FenceState;
            state.sync_state.fences.insert(fence_id, FenceState {
                id: fence_id,
                triggered: initially_triggered,
                initially_triggered,
                fd,
            });

            Vec::new() // void request
        }

        // -----------------------------------------------------------------
        // 5: FDFromFence — export a SYNC fence as a file descriptor
        // -----------------------------------------------------------------
        5 => {
            if data.len() < 12 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

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

        // -----------------------------------------------------------------
        // 6: GetSupportedModifiers (DRI3 1.2)
        // -----------------------------------------------------------------
        6 => {
            if data.len() < 12 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }
            debug!("DRI3 GetSupportedModifiers");

            // Return DRM_FORMAT_MOD_LINEAR (0) and DRM_FORMAT_MOD_INVALID
            // (0x00ffffffffffffff) as supported modifiers.
            // Window modifiers = what this "compositor" supports for scanout.
            // Screen modifiers = what the GPU/renderer supports for rendering.
            const DRM_FORMAT_MOD_LINEAR: u64 = 0;
            const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

            let num_window_modifiers: u32 = 1; // LINEAR only for window/scanout
            let num_screen_modifiers: u32 = 2; // LINEAR + INVALID for rendering

            // Extra data: window modifiers (1 * 8 bytes) + screen modifiers (2 * 8 bytes) = 24 bytes
            // 24 / 4 = 6 words
            let extra_bytes = ((num_window_modifiers + num_screen_modifiers) as usize) * 8;
            let extra_words = extra_bytes / 4;
            let mut reply = vec![0u8; 32 + extra_bytes];
            reply[0] = 1; // Reply
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, extra_words as u32, bo); // length
            write_u32_bo(&mut reply, 8, num_window_modifiers, bo);
            write_u32_bo(&mut reply, 12, num_screen_modifiers, bo);

            // Window modifiers (u64 each), starting at offset 32
            let mut off = 32;
            // LINEAR
            write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_LINEAR & 0xFFFF_FFFF) as u32, bo);
            write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_LINEAR >> 32) as u32, bo);
            off += 8;

            // Screen modifiers (u64 each)
            // LINEAR
            write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_LINEAR & 0xFFFF_FFFF) as u32, bo);
            write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_LINEAR >> 32) as u32, bo);
            off += 8;
            // INVALID (used by Mesa as a fallback/any-modifier sentinel)
            write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_INVALID & 0xFFFF_FFFF) as u32, bo);
            write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_INVALID >> 32) as u32, bo);

            reply
        }

        // -----------------------------------------------------------------
        // 7: PixmapFromBuffers (DRI3 1.2, multi-plane)
        // -----------------------------------------------------------------
        7 => {
            // Wire format:
            // bytes 4-7:   pixmap
            // bytes 8-11:  drawable
            // bytes 12-13: width
            // bytes 14-15: height
            // bytes 16-19: stride0
            // bytes 20-23: offset0
            // bytes 24-27: stride1
            // bytes 28-31: offset1
            // bytes 32-35: stride2
            // bytes 36-39: offset2
            // bytes 40-43: stride3
            // bytes 44-47: offset3
            // byte  48:    depth
            // byte  49:    bpp
            // bytes 50-53: fourcc (DRM format)
            // bytes 54-57: modifier (hi)
            // bytes 58-61: modifier (lo)
            // byte  62:    num_buffers
            if data.len() < 52 {
                // Drain any pending fds
                for fd in state.pending_fds.drain(..) {
                    unsafe { libc::close(fd); }
                }
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let pixmap_id = read_u32_bo(data, 4, bo);
            let _drawable = read_u32_bo(data, 8, bo);
            let width = read_u16_bo(data, 12, bo);
            let height = read_u16_bo(data, 14, bo);

            // Read per-plane strides and offsets (up to 4 planes)
            let strides = [
                read_u32_bo(data, 16, bo),
                read_u32_bo(data, 24, bo),
                read_u32_bo(data, 32, bo),
                read_u32_bo(data, 40, bo),
            ];
            let offsets = [
                read_u32_bo(data, 20, bo),
                read_u32_bo(data, 28, bo),
                read_u32_bo(data, 36, bo),
                read_u32_bo(data, 44, bo),
            ];

            let depth = data[48];
            let _bpp = data[49];
            let fourcc = if data.len() > 53 { read_u32_bo(data, 50, bo) } else { 0 };
            let num_buffers = if data.len() > 62 { data[62] } else { 1 };

            debug!(
                "DRI3 PixmapFromBuffers: pid={pixmap_id:#x} {width}x{height} depth={depth} \
                 fourcc={fourcc:#010x} planes={num_buffers}"
            );

            // Consume all pending fds
            let fds: Vec<i32> = state.pending_fds.drain(..).collect();

            let mut fb = Framebuffer::new(width as u32, height as u32);
            let w = width as usize;
            let h = height as usize;

            match fourcc {
                // ----------------------------------------------------------
                // NV12: Y plane + interleaved UV plane (4:2:0)
                // ----------------------------------------------------------
                FOURCC_NV12 => {
                    debug!("DRI3 PixmapFromBuffers: NV12 YUV→ARGB conversion");
                    convert_nv12_to_fb(&fds, &strides, &offsets, w, h, &mut fb);
                }

                // ----------------------------------------------------------
                // YV12: Y plane + V plane + U plane (4:2:0)
                // ----------------------------------------------------------
                FOURCC_YV12 => {
                    debug!("DRI3 PixmapFromBuffers: YV12 YUV→ARGB conversion");
                    convert_yv12_to_fb(&fds, &strides, &offsets, w, h, &mut fb);
                }

                // ----------------------------------------------------------
                // YUY2: Packed YUYV (4:2:2)
                // ----------------------------------------------------------
                FOURCC_YUY2 => {
                    debug!("DRI3 PixmapFromBuffers: YUY2 YUV→ARGB conversion");
                    convert_yuy2_to_fb(&fds, &strides, &offsets, w, h, &mut fb);
                }

                // ----------------------------------------------------------
                // Default: treat as ARGB/XRGB — copy plane 0 directly
                // ----------------------------------------------------------
                _ => {
                    if let Some(&first_fd) = fds.first() {
                        if first_fd >= 0 {
                            let stride0 = strides[0] as usize;
                            let offset0 = offsets[0] as usize;
                            let read_size = offset0 + stride0 * h;
                            let buf = read_fd_buffer(first_fd, read_size);
                            if !buf.is_empty() {
                                let dst = fb.data_mut();
                                let dst_stride = w * 4;
                                for row in 0..h {
                                    let src_start = offset0 + row * stride0;
                                    let dst_start = row * dst_stride;
                                    let copy_len = dst_stride.min(stride0).min(
                                        buf.len().saturating_sub(src_start)
                                    ).min(dst.len().saturating_sub(dst_start));
                                    if copy_len > 0 && src_start + copy_len <= buf.len() {
                                        dst[dst_start..dst_start + copy_len]
                                            .copy_from_slice(&buf[src_start..src_start + copy_len]);
                                    }
                                }
                            }
                        }
                    } else {
                        warn!("DRI3 PixmapFromBuffers: no pending fds, creating empty pixmap");
                    }
                }
            }

            // Close all fds
            for fd in &fds {
                if *fd >= 0 {
                    unsafe { libc::close(*fd); }
                }
            }

            state.pixmaps.insert(
                pixmap_id,
                PixmapState {
                    width,
                    height,
                    depth,
                    framebuffer: fb,
                    alias_window: None,
                    shm_backing: None,
                },
            );
            state.register_shared_pixmap(pixmap_id, width, height, depth);

            Vec::new() // void request
        }

        // -----------------------------------------------------------------
        // 8: BuffersFromPixmap (DRI3 1.2, multi-plane)
        // -----------------------------------------------------------------
        8 => {
            if data.len() < 8 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let pixmap_id = read_u32_bo(data, 4, bo);
            debug!("DRI3 BuffersFromPixmap: pid={pixmap_id:#x}");

            let (width, height, depth, data_bytes) = if let Some(pix) = state.pixmaps.get(&pixmap_id) {
                (pix.width, pix.height, pix.depth, pix.framebuffer.data().to_vec())
            } else {
                warn!("DRI3 BuffersFromPixmap: unknown pixmap {pixmap_id:#x}");
                return build_error_bo(
                    BAD_PIXMAP, seq, pixmap_id,
                    DRI3_MAJOR_OPCODE, 8, bo,
                );
            };

            // Create a memfd with the pixmap data (single plane)
            let fd = unsafe {
                let name = b"dri3-buffers\0";
                let fd = libc::memfd_create(name.as_ptr() as *const libc::c_char, libc::MFD_CLOEXEC);
                if fd >= 0 {
                    let _ = libc::ftruncate(fd, data_bytes.len() as libc::off_t);
                    let _ = libc::pwrite(
                        fd,
                        data_bytes.as_ptr() as *const libc::c_void,
                        data_bytes.len(),
                        0,
                    );
                }
                fd
            };

            if fd < 0 {
                warn!("DRI3 BuffersFromPixmap: memfd_create failed");
                return build_error_bo(
                    BAD_ALLOC, seq, pixmap_id,
                    DRI3_MAJOR_OPCODE, 8, bo,
                );
            }

            state.reply_fds.push(fd);

            let stride = (width as u32) * 4;
            let bpp = if depth == 1 { 1 } else { 32 };

            // Reply: nfd=1, num_buffers=1, then one stride(4) + one offset(4) = 8 bytes extra
            let mut reply = vec![0u8; 32 + 8];
            reply[0] = 1; // Reply
            reply[1] = 1; // nfd
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 4, 2, bo); // length = 8/4 = 2 words
            write_u16_bo(&mut reply, 8, width, bo);
            write_u16_bo(&mut reply, 10, height, bo);
            // reply[12..16] = pad/modifier (0 for LINEAR)
            write_u32_bo(&mut reply, 12, 0, bo);
            // reply[16..24] = modifier high bits
            write_u32_bo(&mut reply, 16, 0, bo);
            reply[20] = depth;
            reply[21] = bpp;
            reply[22] = 1; // num_buffers
            // Extra data: stride0 (4 bytes) + offset0 (4 bytes)
            write_u32_bo(&mut reply, 32, stride, bo);
            write_u32_bo(&mut reply, 36, 0, bo); // offset = 0
            reply
        }

        // -----------------------------------------------------------------
        // 9: SetDRMDeviceInUse (DRI3 1.4, void request)
        // -----------------------------------------------------------------
        9 => {
            // Request: window(4), drmMajor(4), drmMinor(4)
            if data.len() < 16 {
                return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
            }

            let _window = read_u32_bo(data, 4, bo);
            let drm_major = read_u32_bo(data, 8, bo);
            let drm_minor = read_u32_bo(data, 12, bo);

            debug!("DRI3 SetDRMDeviceInUse: window={_window:#x} drm_device={drm_major}:{drm_minor}");

            // Track the DRM device this client is using.
            state.dri3_drm_device = Some((drm_major, drm_minor));

            Vec::new()
        }

        _ => {
            warn!("Unhandled DRI3 minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                149, minor as u16, state.msb_first,
            )
        }
    }
}
