//! MIT-SHM (Shared Memory) extension handler.

use tracing::{info, warn};

use super::super::client::ClientState;
use super::super::core::ROOT_VISUAL;
use super::super::types::{PixmapState, ShmPixmapBacking, ShmSegment};
use crate::framebuffer::Framebuffer;

/// Handle MIT-SHM extension requests (major opcode 130).
pub(crate) fn handle_shm_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        // QueryVersion
        0 => {
            info!("SHM QueryVersion");
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // shared_pixmaps = true
            state.write_u16(&mut reply, 2, seq);
            // reply[4..8] = additional data length = 0
            state.write_u16(&mut reply, 8, 1u16); // major version
            state.write_u16(&mut reply, 10, 2u16); // minor version
            state.write_u16(&mut reply, 12, 0u16); // uid
            state.write_u16(&mut reply, 14, 0u16); // gid
            reply[16] = 2; // pixmap_format = ZPixmap
            reply.to_vec()
        }

        // Attach
        1 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = state.read_u32(data, 4);
            let shmid = state.read_u32(data, 8) as i32;
            let read_only = data[12] != 0;

            info!("SHM Attach: shmseg={shmseg} shmid={shmid} read_only={read_only}");

            unsafe {
                // Get segment size via shmctl IPC_STAT
                let mut ds: libc::shmid_ds = std::mem::zeroed();
                let stat_ret = libc::shmctl(shmid, libc::IPC_STAT, &mut ds);
                if stat_ret < 0 {
                    warn!("SHM Attach: shmctl IPC_STAT failed for shmid={shmid}");
                    return Vec::new();
                }
                let size = ds.shm_segsz;

                let flags = if read_only { libc::SHM_RDONLY } else { 0 };
                let addr = libc::shmat(shmid, std::ptr::null(), flags);
                if addr == (-1isize) as *mut libc::c_void {
                    warn!("SHM Attach: shmat failed for shmid={shmid}");
                    return Vec::new();
                }

                state.shm_segments.insert(shmseg, ShmSegment {
                    addr: addr as *mut u8,
                    size,
                });
            }

            Vec::new() // No reply for Attach
        }

        // Detach
        2 => {
            if data.len() < 8 {
                return Vec::new();
            }
            let shmseg = state.read_u32(data, 4);
            info!("SHM Detach: shmseg={shmseg}");

            if let Some(seg) = state.shm_segments.remove(&shmseg) {
                unsafe {
                    libc::shmdt(seg.addr as *const libc::c_void);
                }
            }

            Vec::new() // No reply for Detach
        }

        // PutImage
        3 => {
            if data.len() < 40 {
                return Vec::new();
            }

            let drawable = state.read_u32(data, 4);
            let _gc = state.read_u32(data, 8);
            let total_width = state.read_u16(data, 12) as usize;
            let _total_height = state.read_u16(data, 14);
            let src_x = state.read_u16(data, 16) as usize;
            let src_y = state.read_u16(data, 18) as usize;
            let src_width = state.read_u16(data, 20);
            let src_height = state.read_u16(data, 22);
            let dst_x = state.read_i16(data, 24);
            let dst_y = state.read_i16(data, 26);
            let _depth = data[28];
            let _format = data[29];
            let send_event = data[30] != 0;
            let shmseg = state.read_u32(data, 32);
            let offset = state.read_u32(data, 36) as usize;

            info!(
                "SHM PutImage: drawable={drawable:#x} shmseg={shmseg} offset={offset} \
                 total_width={total_width} src=({src_x},{src_y}) size=({src_width}x{src_height}) \
                 dst=({dst_x},{dst_y}) send_event={send_event}"
            );

            let seg = match state.shm_segments.get(&shmseg) {
                Some(s) => s,
                None => {
                    warn!("SHM PutImage: unknown shmseg={shmseg}");
                    return Vec::new();
                }
            };

            // Bytes per pixel (32bpp BGRA)
            let bpp = 4usize;
            let src_stride = total_width * bpp;
            let region_size = src_stride * (src_y + src_height as usize);

            // Bounds check
            if offset + region_size > seg.size {
                warn!(
                    "SHM PutImage: out of bounds (offset={offset} + region_size={region_size} > seg.size={})",
                    seg.size
                );
                return Vec::new();
            }

            // Build a contiguous pixel buffer for the source region
            let w = src_width as usize;
            let h = src_height as usize;
            let mut pixels = vec![0u8; w * h * bpp];

            unsafe {
                let base = seg.addr.add(offset);
                for row in 0..h {
                    let src_off = (src_y + row) * src_stride + src_x * bpp;
                    let dst_off = row * w * bpp;
                    let src_ptr = base.add(src_off);
                    std::ptr::copy_nonoverlapping(src_ptr, pixels.as_mut_ptr().add(dst_off), w * bpp);
                }
            }

            // Blit to the drawable's framebuffer
            if let Some(fb) = state.get_framebuffer_mut(drawable) {
                fb.put_image(dst_x, dst_y, src_width, src_height, &pixels);
            }

            // If send_event, return a ShmCompletion event
            if send_event {
                let mut event = [0u8; 32];
                event[0] = 65; // ShmCompletion event type (first_event + 0)
                state.write_u16(&mut event, 2, seq);
                state.write_u32(&mut event, 4, drawable);
                state.write_u32(&mut event, 8, shmseg);
                state.write_u32(&mut event, 16, offset as u32);
                event.to_vec()
            } else {
                Vec::new()
            }
        }

        // GetImage
        4 => {
            if data.len() < 32 {
                return Vec::new();
            }
            let drawable = state.read_u32(data, 4);
            let src_x = state.read_i16(data, 8);
            let src_y = state.read_i16(data, 10);
            let width = state.read_u16(data, 12);
            let height = state.read_u16(data, 14);
            let _plane_mask = state.read_u32(data, 16);
            let _format = data[20];
            let shmseg = state.read_u32(data, 24);
            let shm_offset = state.read_u32(data, 28) as usize;

            info!("SHM GetImage: drawable={drawable:#x} ({src_x},{src_y}) {width}x{height} shmseg={shmseg} offset={shm_offset}");

            // Sync SHM-backed pixmap data before reading
            state.sync_shm_pixmap(drawable);

            // Copy pixels from drawable into SHM segment
            let resolved = state.resolve_drawable(drawable);
            let pixels = if let Some(fb) = state.get_framebuffer_mut(resolved) {
                fb.extract_pixels(src_x, src_y, width, height)
            } else {
                vec![0u8; width as usize * height as usize * 4]
            };

            if let Some(seg) = state.shm_segments.get(&shmseg) {
                let bpp = 4usize;
                let row_bytes = width as usize * bpp;
                let total_bytes = row_bytes * height as usize;
                if shm_offset + total_bytes <= seg.size {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            pixels.as_ptr(),
                            seg.addr.add(shm_offset),
                            total_bytes.min(pixels.len()),
                        );
                    }
                }
            }

            // Reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 24; // depth
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, ROOT_VISUAL);
            state.write_u32(&mut reply, 12, width as u32 * height as u32); // size
            reply.to_vec()
        }

        // CreatePixmap
        5 => {
            if data.len() < 28 {
                return Vec::new();
            }
            let pid = state.read_u32(data, 4);
            let width = state.read_u16(data, 12);
            let height = state.read_u16(data, 14);
            let depth = data[16];
            let shmseg = state.read_u32(data, 20);
            let shm_offset = state.read_u32(data, 24) as usize;

            info!("SHM CreatePixmap: pid={pid:#x} {width}x{height} depth={depth} shmseg={shmseg} offset={shm_offset}");

            // Create an SHM-backed pixmap. The client will write directly into
            // the SHM segment; we sync from it before reading.
            state.pixmaps.insert(
                pid,
                PixmapState {
                    width,
                    height,
                    depth,
                    framebuffer: Framebuffer::new(width as u32, height as u32),
                    alias_window: None,
                    shm_backing: Some(ShmPixmapBacking {
                        shmseg,
                        offset: shm_offset,
                    }),
                },
            );
            Vec::new()
        }

        // AttachFd (minor 6) — MIT-SHM 1.2+ with fd passing
        6 => {
            if data.len() < 12 {
                return Vec::new();
            }
            let shmseg = state.read_u32(data, 4);
            let read_only = data[8] != 0;

            // The fd should have been received via SCM_RIGHTS ancillary data
            // and stored in state.pending_fds by the connection handler.
            if let Some(fd) = state.pending_fds.pop() {
                info!("SHM AttachFd: shmseg={shmseg} fd={fd} read_only={read_only}");
                unsafe {
                    // Get the file size via fstat
                    let mut stat: libc::stat = std::mem::zeroed();
                    if libc::fstat(fd, &mut stat) < 0 {
                        warn!("SHM AttachFd: fstat failed for fd={fd}");
                        libc::close(fd);
                        return Vec::new();
                    }
                    let size = stat.st_size as usize;
                    if size == 0 {
                        warn!("SHM AttachFd: zero-size fd={fd}");
                        libc::close(fd);
                        return Vec::new();
                    }

                    let prot = if read_only {
                        libc::PROT_READ
                    } else {
                        libc::PROT_READ | libc::PROT_WRITE
                    };
                    let addr = libc::mmap(
                        std::ptr::null_mut(),
                        size,
                        prot,
                        libc::MAP_SHARED,
                        fd,
                        0,
                    );
                    libc::close(fd); // fd can be closed after mmap

                    if addr == libc::MAP_FAILED {
                        warn!("SHM AttachFd: mmap failed for fd={fd}");
                        return Vec::new();
                    }

                    state.shm_segments.insert(shmseg, ShmSegment {
                        addr: addr as *mut u8,
                        size,
                    });
                }
            } else {
                warn!("SHM AttachFd: no pending fd for shmseg={shmseg}");
            }
            Vec::new()
        }

        // CreateSegment (minor 7) — MIT-SHM 1.2+
        // Server creates an SHM segment and returns the fd to the client.
        7 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = state.read_u32(data, 4);
            let size = state.read_u32(data, 8) as usize;
            let read_only = data[12] != 0;

            info!("SHM CreateSegment: shmseg={shmseg:#x} size={size} read_only={read_only}");

            if size == 0 {
                return super::super::core::build_error(
                    super::super::core::BAD_VALUE, seq, 0, 130, 7,
                );
            }

            unsafe {
                // Create an anonymous shared memory segment via memfd_create
                let name = std::ffi::CString::new("x11-shm").unwrap_or_default();
                let fd = libc::syscall(libc::SYS_memfd_create, name.as_ptr(), 0i32) as i32;
                if fd < 0 {
                    warn!("SHM CreateSegment: memfd_create failed");
                    return Vec::new();
                }

                // Set the size
                if libc::ftruncate(fd, size as libc::off_t) < 0 {
                    warn!("SHM CreateSegment: ftruncate failed");
                    libc::close(fd);
                    return Vec::new();
                }

                // mmap it for the server
                let prot = if read_only {
                    libc::PROT_READ
                } else {
                    libc::PROT_READ | libc::PROT_WRITE
                };
                let addr = libc::mmap(
                    std::ptr::null_mut(),
                    size,
                    prot,
                    libc::MAP_SHARED,
                    fd,
                    0,
                );
                if addr == libc::MAP_FAILED {
                    warn!("SHM CreateSegment: mmap failed");
                    libc::close(fd);
                    return Vec::new();
                }

                state.shm_segments.insert(shmseg, ShmSegment {
                    addr: addr as *mut u8,
                    size,
                });

                // Build reply with fd
                // The fd needs to be sent via SCM_RIGHTS ancillary data
                let mut reply = [0u8; 32];
                reply[0] = 1; // Reply
                reply[1] = 0; // nfd = 1 (but encoded differently in newer protocol)
                state.write_u16(&mut reply, 2, seq);
                state.write_u32(&mut reply, 4, 0); // length

                // Queue the fd for sending via SCM_RIGHTS
                state.reply_fds.push(fd);

                reply.to_vec()
            }
        }

        _ => {
            warn!("Unhandled SHM minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                130, minor as u16, state.msb_first,
            )
        }
    }
}
