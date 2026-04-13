//! DRI3 pixmap operations: PixmapFromBuffer, BufferFromPixmap,
//! PixmapFromBuffers, BuffersFromPixmap.

use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::super::super::types::PixmapState;
use super::DRI3_MAJOR_OPCODE;
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
    let uv_height = height.div_ceil(2);
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

    let half_h = height.div_ceil(2);
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

// -----------------------------------------------------------------
// 2: PixmapFromBuffer
// -----------------------------------------------------------------
pub(crate) fn handle_pixmap_from_buffer(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
    bo: bool,
) -> Vec<u8> {
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
pub(crate) fn handle_buffer_from_pixmap(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    bo: bool,
) -> Vec<u8> {
    if data.len() < 8 {
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, 3, bo);
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
// 7: PixmapFromBuffers (DRI3 1.2, multi-plane)
// -----------------------------------------------------------------
pub(crate) fn handle_pixmap_from_buffers(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
    bo: bool,
) -> Vec<u8> {
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
            debug!("DRI3 PixmapFromBuffers: NV12 YUV->ARGB conversion");
            convert_nv12_to_fb(&fds, &strides, &offsets, w, h, &mut fb);
        }

        // ----------------------------------------------------------
        // YV12: Y plane + V plane + U plane (4:2:0)
        // ----------------------------------------------------------
        FOURCC_YV12 => {
            debug!("DRI3 PixmapFromBuffers: YV12 YUV->ARGB conversion");
            convert_yv12_to_fb(&fds, &strides, &offsets, w, h, &mut fb);
        }

        // ----------------------------------------------------------
        // YUY2: Packed YUYV (4:2:2)
        // ----------------------------------------------------------
        FOURCC_YUY2 => {
            debug!("DRI3 PixmapFromBuffers: YUY2 YUV->ARGB conversion");
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
pub(crate) fn handle_buffers_from_pixmap(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    bo: bool,
) -> Vec<u8> {
    if data.len() < 8 {
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, 8, bo);
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
