//! DBE (Double Buffer Extension) handler.

use tracing::debug;

use super::super::client::ClientState;
use crate::framebuffer::Framebuffer;
use crate::xserver::core::require_len;

/// DBE - Double Buffer Extension (opcode 157)
pub(crate) fn handle_dbe_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => { // GetVersion
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            reply[8] = 1; // major_version
            reply[9] = 0; // minor_version
            reply.to_vec()
        }
        1 => { // AllocateBackBufferName
            require_len!(data, 16, seq, 157, minor as u16, state.msb_first);
            let window_id = state.read_u32(data, 4);
            let back_buffer_id = state.read_u32(data, 8);
            let _swap_action = data[12]; // Undefined, Background, Untouched, Copied

            debug!("DBE AllocateBackBuffer: window={window_id:#x} buffer={back_buffer_id:#x}");

            // Create a pixmap-like back buffer backed by the window's dimensions
            if let Some(win) = state.windows.get(&window_id) {
                let w = win.width;
                let h = win.height;
                let depth = win.depth;
                state.pixmaps.insert(back_buffer_id, super::super::types::PixmapState {
                    width: w,
                    height: h,
                    depth,
                    framebuffer: Framebuffer::new(w as u32, h as u32),
                    alias_window: None,
                    shm_backing: None,
                });
                state.back_buffers.insert(back_buffer_id, window_id);
            } else {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_WINDOW, seq, window_id,
                    157, minor as u16, state.msb_first,
                );
            }
            Vec::new()
        }
        2 => { // DeallocateBackBufferName
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            let back_buffer_id = state.read_u32(data, 4);
            debug!("DBE DeallocateBackBuffer: buffer={back_buffer_id:#x}");
            state.pixmaps.remove(&back_buffer_id);
            state.back_buffers.remove(&back_buffer_id);
            state.recycle_xid(back_buffer_id);
            Vec::new()
        }
        3 => { // SwapBuffers
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            let n_windows = state.read_u32(data, 4) as usize;
            for i in 0..n_windows {
                let off = 8 + i * 8;
                if off + 8 > data.len() { break; }
                let window_id = state.read_u32(data, off);
                let swap_action = state.read_u32(data, off + 4) as u8;
                // swap_action: 0=Undefined, 1=Background, 2=Untouched, 3=Copied

                debug!("DBE SwapBuffers: window={window_id:#x} action={swap_action}");

                // Find the back buffer for this window
                let back_buffer_id = state.back_buffers.iter()
                    .find(|(_, &wid)| wid == window_id)
                    .map(|(&bbid, _)| bbid);

                if let Some(bbid) = back_buffer_id {
                    // Extract pixels from back buffer
                    let back_pixels = state.pixmaps.get(&bbid).map(|p| {
                        (p.width, p.height, p.framebuffer.extract_pixels(0, 0, p.width, p.height))
                    });

                    if let Some((bw, bh, pixels)) = back_pixels {
                        // For Copied swap action, save old front before overwriting
                        let old_front = if swap_action == 3 {
                            state.windows.get(&window_id).map(|w| {
                                w.framebuffer.extract_pixels(0, 0, w.width, w.height)
                            })
                        } else {
                            None
                        };

                        // Copy back buffer to front buffer
                        let (w, h, bg) = if let Some(win) = state.windows.get_mut(&window_id) {
                            let w = win.width;
                            let h = win.height;
                            win.framebuffer.put_image(0, 0, w.min(bw), h.min(bh), &pixels);
                            (w, h, win.background_pixel)
                        } else {
                            continue;
                        };

                        // Apply swap action to the back buffer
                        if let Some(bb) = state.pixmaps.get_mut(&bbid) {
                            match swap_action {
                                1 => {
                                    // Background: fill back buffer with window's background
                                    bb.framebuffer.fill_rect(0, 0, bb.width, bb.height, bg);
                                }
                                2 => {
                                    // Untouched: leave back buffer as-is
                                }
                                3 => {
                                    // Copied: swap — put old front into back buffer
                                    if let Some(old) = old_front {
                                        bb.framebuffer.put_image(0, 0, bb.width, bb.height, &old);
                                    }
                                }
                                _ => {
                                    // Undefined: content is undefined, no action needed
                                }
                            }
                        }

                        // Notify compositor of the damage
                        state.notify_damage(window_id, 0, 0, w, h);
                    }
                }
            }
            Vec::new()
        }
        4 => {
            // BeginIdiom: mark start of atomic swap group.
            // All SwapBuffers calls between Begin and EndIdiom should be treated as atomic.
            // We accept this silently since our SwapBuffers is already synchronous.
            state.dbe_idiom_depth += 1;
            Vec::new()
        }
        5 => {
            // EndIdiom: end of atomic swap group.
            if state.dbe_idiom_depth > 0 {
                state.dbe_idiom_depth -= 1;
            }
            Vec::new()
        }
        6 => { // GetVisualInfo
            // Return visual info for 1 screen with our 2 visuals supporting DBE
            let n_screens: u32 = 1;
            // PerflDepthInfo: depth(1) + pad(1) + n_visuals(2) + visual entries
            // Each visual entry: visual_id(4) + depth(1) + perflevel(1) + pad(2) = 8
            let n_visuals: u16 = 2; // 24-bit and 32-bit
            let per_depth_size = 4 + n_visuals as usize * 8; // header + visuals
            let screen_info_size = 4 + per_depth_size; // n_perfdepth(4) + depths
            let extra = 4 + screen_info_size; // n_screens already in header, then screen data
            let padded = (extra + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (padded / 4) as u32);
            state.write_u32(&mut reply, 8, n_screens);
            // Screen 0
            let off = 32;
            state.write_u32(&mut reply, off, 1); // n_perfdepth = 1

            // PerflDepthInfo
            let doff = off + 4;
            reply[doff] = 24; // depth
            state.write_u16(&mut reply, doff + 2, n_visuals);

            // Visual 0: ROOT_VISUAL (24-bit)
            let voff = doff + 4;
            state.write_u32(&mut reply, voff, 0x21); // ROOT_VISUAL
            reply[voff + 4] = 24; // depth
            reply[voff + 5] = 0; // performance level

            // Visual 1: ARGB visual (32-bit)
            let voff2 = voff + 8;
            state.write_u32(&mut reply, voff2, 0x40); // ARGB visual
            reply[voff2 + 4] = 32;
            reply[voff2 + 5] = 0;

            reply
        }
        7 => { // GetBackBufferAttributes
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            let back_buffer_id = state.read_u32(data, 4);
            let window_id = state.back_buffers.get(&back_buffer_id).copied().unwrap_or(0);
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, window_id);
            reply.to_vec()
        }
        _ => {
            debug!("DBE: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                157, minor as u16, state.msb_first,
            )
        }
    }
}
