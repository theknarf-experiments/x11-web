//! MIT-SHM (Shared Memory) extension handler.

use tracing::{info, warn};
use super::parse_minor;

use super::super::client::ClientState;
use super::super::core::ROOT_VISUAL;
use super::super::types::{PixmapState, ShmPixmapBacking, ShmSegment};
use crate::framebuffer::Framebuffer;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::shm::{
    AttachRequest, CreatePixmapRequest, CreateSegmentRequest, DetachRequest, GetImageRequest,
    PutImageRequest,
};

/// Handle MIT-SHM extension requests (major opcode 130).
pub(crate) fn handle_shm_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        // QueryVersion
        0 => {
            info!("SHM QueryVersion");
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(1) // shared_pixmaps = true
                .set_u16(8, 1u16) // major version
                .set_u16(10, 2u16) // minor version
                .set_u16(12, 0u16) // uid
                .set_u16(14, 0u16) // gid
                .set_u8(16, 2) // pixmap_format = ZPixmap
                .build()
        }

        // Attach
        1 => {
            let req = parse_minor!(AttachRequest, data, state, seq, 130, minor as u16);
            let shmseg = req.shmseg;
            let shmid = req.shmid as i32;
            let read_only = req.read_only;

            info!("SHM Attach: shmseg={shmseg} shmid={shmid} read_only={read_only}");

            unsafe {
                // Get segment size via shmctl IPC_STAT
                let mut ds: libc::shmid_ds = std::mem::zeroed();
                let stat_ret = libc::shmctl(shmid, libc::IPC_STAT, &mut ds);
                if stat_ret < 0 {
                    warn!("SHM Attach: shmctl IPC_STAT failed for shmid={shmid}");
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::VALUE_ERROR,
                        seq,
                        shmid as u32,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
                }
                let size = ds.shm_segsz;

                let flags = if read_only { libc::SHM_RDONLY } else { 0 };
                let addr = libc::shmat(shmid, std::ptr::null(), flags);
                if addr == (-1isize) as *mut libc::c_void {
                    warn!("SHM Attach: shmat failed for shmid={shmid}");
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::ACCESS_ERROR,
                        seq,
                        shmid as u32,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
                }

                state.shm_segments.insert(
                    shmseg,
                    ShmSegment {
                        addr: addr as *mut u8,
                        size,
                    },
                );
            }

            Vec::new() // No reply for Attach
        }

        // Detach
        2 => {
            let req = parse_minor!(DetachRequest, data, state, seq, 130, minor as u16);
            let shmseg = req.shmseg;
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

            let req = parse_minor!(PutImageRequest, data, state, seq, 130, minor as u16);
            let drawable = req.drawable;
            let _gc = req.gc;
            let total_width = req.total_width as usize;
            let _total_height = req.total_height;
            let src_x = req.src_x as usize;
            let src_y = req.src_y as usize;
            let src_width = req.src_width;
            let src_height = req.src_height;
            let dst_x = req.dst_x;
            let dst_y = req.dst_y;
            let _depth = req.depth;
            let _format = req.format;
            let send_event = req.send_event;
            let shmseg = req.shmseg;
            let offset = req.offset as usize;

            info!(
                "SHM PutImage: drawable={drawable:#x} shmseg={shmseg} offset={offset} \
                 total_width={total_width} src=({src_x},{src_y}) size=({src_width}x{src_height}) \
                 dst=({dst_x},{dst_y}) send_event={send_event}"
            );

            let seg = match state.shm_segments.get(&shmseg) {
                Some(s) => s,
                None => {
                    warn!("SHM PutImage: unknown shmseg={shmseg}");
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::VALUE_ERROR,
                        seq,
                        shmseg,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
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
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::VALUE_ERROR,
                    seq,
                    offset as u32,
                    130,
                    minor as u16,
                    state.msb_first,
                );
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
                    std::ptr::copy_nonoverlapping(
                        src_ptr,
                        pixels.as_mut_ptr().add(dst_off),
                        w * bpp,
                    );
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
            let req = parse_minor!(GetImageRequest, data, state, seq, 130, minor as u16);
            let drawable = req.drawable;
            let src_x = req.x;
            let src_y = req.y;
            let width = req.width;
            let height = req.height;
            let _plane_mask = req.plane_mask;
            let _format = req.format;
            let shmseg = req.shmseg;
            let shm_offset = req.offset as usize;

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
            ReplyBuf::fixed(seq, state.msb_first)
                .set_data_byte(24) // depth
                .set_u32(8, ROOT_VISUAL)
                .set_u32(12, width as u32 * height as u32) // size
                .build()
        }

        // CreatePixmap
        5 => {
            let req = parse_minor!(CreatePixmapRequest, data, state, seq, 130, minor as u16);
            let pid = req.pid;
            let width = req.width;
            let height = req.height;
            let depth = req.depth;
            let shmseg = req.shmseg;
            let shm_offset = req.offset as usize;

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
            require_len!(data, 12, seq, 130, minor as u16, state.msb_first);
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
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::ACCESS_ERROR,
                            seq,
                            shmseg,
                            130,
                            minor as u16,
                            state.msb_first,
                        );
                    }
                    let size = stat.st_size as usize;
                    if size == 0 {
                        warn!("SHM AttachFd: zero-size fd={fd}");
                        libc::close(fd);
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::VALUE_ERROR,
                            seq,
                            shmseg,
                            130,
                            minor as u16,
                            state.msb_first,
                        );
                    }

                    let prot = if read_only {
                        libc::PROT_READ
                    } else {
                        libc::PROT_READ | libc::PROT_WRITE
                    };
                    let addr =
                        libc::mmap(std::ptr::null_mut(), size, prot, libc::MAP_SHARED, fd, 0);
                    libc::close(fd); // fd can be closed after mmap

                    if addr == libc::MAP_FAILED {
                        warn!("SHM AttachFd: mmap failed for fd={fd}");
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::ACCESS_ERROR,
                            seq,
                            shmseg,
                            130,
                            minor as u16,
                            state.msb_first,
                        );
                    }

                    state.shm_segments.insert(
                        shmseg,
                        ShmSegment {
                            addr: addr as *mut u8,
                            size,
                        },
                    );
                }
            } else {
                warn!("SHM AttachFd: no pending fd for shmseg={shmseg}");
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::VALUE_ERROR,
                    seq,
                    shmseg,
                    130,
                    minor as u16,
                    state.msb_first,
                );
            }
            Vec::new()
        }

        // CreateSegment (minor 7) — MIT-SHM 1.2+
        // Server creates an SHM segment and returns the fd to the client.
        7 => {
            let req = parse_minor!(CreateSegmentRequest, data, state, seq, 130, minor as u16);
            let shmseg = req.shmseg;
            let size = req.size as usize;
            let read_only = req.read_only;

            info!("SHM CreateSegment: shmseg={shmseg:#x} size={size} read_only={read_only}");

            if size == 0 {
                return super::super::core::build_error(
                    super::super::core::VALUE_ERROR,
                    seq,
                    0,
                    130,
                    7,
                );
            }

            unsafe {
                // Create an anonymous shared memory segment via memfd_create
                let name = std::ffi::CString::new("x11-shm").unwrap_or_default();
                let fd = libc::syscall(libc::SYS_memfd_create, name.as_ptr(), 0i32) as i32;
                if fd < 0 {
                    warn!("SHM CreateSegment: memfd_create failed");
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::ALLOC_ERROR,
                        seq,
                        0,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
                }

                // Set the size
                if libc::ftruncate(fd, size as libc::off_t) < 0 {
                    warn!("SHM CreateSegment: ftruncate failed");
                    libc::close(fd);
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::ALLOC_ERROR,
                        seq,
                        size as u32,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
                }

                // mmap it for the server
                let prot = if read_only {
                    libc::PROT_READ
                } else {
                    libc::PROT_READ | libc::PROT_WRITE
                };
                let addr = libc::mmap(std::ptr::null_mut(), size, prot, libc::MAP_SHARED, fd, 0);
                if addr == libc::MAP_FAILED {
                    warn!("SHM CreateSegment: mmap failed");
                    libc::close(fd);
                    return crate::xserver::core::build_error_bo(
                        crate::xserver::core::ALLOC_ERROR,
                        seq,
                        size as u32,
                        130,
                        minor as u16,
                        state.msb_first,
                    );
                }

                state.shm_segments.insert(
                    shmseg,
                    ShmSegment {
                        addr: addr as *mut u8,
                        size,
                    },
                );

                // Build reply with fd
                // The fd needs to be sent via SCM_RIGHTS ancillary data
                // Queue the fd for sending via SCM_RIGHTS
                state.reply_fds.push(fd);

                ReplyBuf::fixed(seq, state.msb_first)
                    .set_data_byte(0) // nfd = 1 (but encoded differently in newer protocol)
                    .build()
            }
        }

        _ => {
            warn!("Unhandled SHM minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                130,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
