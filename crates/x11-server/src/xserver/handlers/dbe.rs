//! DBE (Double Buffer Extension) handler.

use super::parse_minor;
use tracing::debug;
use x11rb_protocol::protocol::dbe::{
    ALLOCATE_BACK_BUFFER_REQUEST, BEGIN_IDIOM_REQUEST, DEALLOCATE_BACK_BUFFER_REQUEST,
    END_IDIOM_REQUEST, GET_BACK_BUFFER_ATTRIBUTES_REQUEST, GET_VISUAL_INFO_REQUEST,
    QUERY_VERSION_REQUEST, SWAP_BUFFERS_REQUEST, SwapAction,
};

use super::super::client::ClientState;
use crate::framebuffer::Framebuffer;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

/// DBE - Double Buffer Extension (opcode 157)
pub(crate) fn handle_dbe_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let dbe_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 157, minor as u16)
    };
    match minor {
        QUERY_VERSION_REQUEST => {
            // GetVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u8(8, 1) // major_version
                .set_u8(9, 0) // minor_version
                .build()
        }
        ALLOCATE_BACK_BUFFER_REQUEST => {
            // AllocateBackBufferName
            require_len!(data, 16, seq, 157, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dbe::AllocateBackBufferRequest;
            let req = parse_minor!(
                AllocateBackBufferRequest,
                data,
                state,
                seq,
                157,
                minor as u16
            );
            let window_id = req.window;
            let back_buffer_id = req.buffer;
            let _swap_action = u8::from(req.swap_action); // Undefined, Background, Untouched, Copied

            debug!("DBE AllocateBackBuffer: window={window_id:#x} buffer={back_buffer_id:#x}");

            // Create a pixmap-like back buffer backed by the window's dimensions
            if let Some(win) = state.windows.get(&window_id) {
                let w = win.width;
                let h = win.height;
                let depth = win.depth;
                state.pixmaps.insert(
                    back_buffer_id,
                    super::super::types::PixmapState {
                        width: w,
                        height: h,
                        depth,
                        framebuffer: Framebuffer::new(w as u32, h as u32),
                        alias_window: None,
                        shm_backing: None,
                    },
                );
                state.back_buffers.insert(back_buffer_id, window_id);
            } else {
                return dbe_err(crate::xserver::core::WINDOW_ERROR, window_id);
            }
            Vec::new()
        }
        DEALLOCATE_BACK_BUFFER_REQUEST => {
            // DeallocateBackBufferName
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dbe::DeallocateBackBufferRequest;
            let req = parse_minor!(
                DeallocateBackBufferRequest,
                data,
                state,
                seq,
                157,
                minor as u16
            );
            let back_buffer_id = req.buffer;
            debug!("DBE DeallocateBackBuffer: buffer={back_buffer_id:#x}");
            state.pixmaps.remove(&back_buffer_id);
            state.back_buffers.remove(&back_buffer_id);
            state.recycle_xid(back_buffer_id);
            Vec::new()
        }
        SWAP_BUFFERS_REQUEST => {
            // SwapBuffers
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dbe::SwapBuffersRequest;
            let req = parse_minor!(SwapBuffersRequest, data, state, seq, 157, minor as u16);
            for action in req.actions.iter() {
                let window_id = action.window;
                let swap_action = action.swap_action;
                debug!("DBE SwapBuffers: window={window_id:#x} action={swap_action:?}");

                // Find the back buffer for this window
                let back_buffer_id = state
                    .back_buffers
                    .iter()
                    .find(|(_, &wid)| wid == window_id)
                    .map(|(&bbid, _)| bbid);

                if let Some(bbid) = back_buffer_id {
                    // Extract pixels from back buffer
                    let back_pixels = state.pixmaps.get(&bbid).map(|p| {
                        (
                            p.width,
                            p.height,
                            p.framebuffer.extract_pixels(0, 0, p.width, p.height),
                        )
                    });

                    if let Some((bw, bh, pixels)) = back_pixels {
                        // For Copied swap action, save old front before overwriting
                        let old_front = if swap_action == SwapAction::COPIED {
                            state
                                .windows
                                .get(&window_id)
                                .map(|w| w.framebuffer.extract_pixels(0, 0, w.width, w.height))
                        } else {
                            None
                        };

                        // Copy back buffer to front buffer
                        let (w, h, bg) = if let Some(win) = state.windows.get_mut(&window_id) {
                            let w = win.width;
                            let h = win.height;
                            win.framebuffer
                                .put_image(0, 0, w.min(bw), h.min(bh), &pixels);
                            (w, h, win.background_pixel)
                        } else {
                            continue;
                        };

                        // Apply swap action to the back buffer
                        if let Some(bb) = state.pixmaps.get_mut(&bbid) {
                            match swap_action {
                                SwapAction::BACKGROUND => {
                                    // Fill back buffer with window's background.
                                    bb.framebuffer.fill_rect(0, 0, bb.width, bb.height, bg);
                                }
                                SwapAction::UNTOUCHED => {
                                    // Leave back buffer as-is.
                                }
                                SwapAction::COPIED => {
                                    // Swap — put old front into back buffer.
                                    if let Some(old) = old_front {
                                        bb.framebuffer.put_image(0, 0, bb.width, bb.height, &old);
                                    }
                                }
                                // SwapAction::UNDEFINED and any unknown value:
                                // content is undefined, no action needed.
                                _ => {}
                            }
                        }

                        // Notify compositor of the damage
                        state.notify_damage(window_id, 0, 0, w, h);
                    }
                }
            }
            Vec::new()
        }
        BEGIN_IDIOM_REQUEST => {
            // BeginIdiom: mark start of atomic swap group.
            // All SwapBuffers calls between Begin and EndIdiom should be treated as atomic.
            // We accept this silently since our SwapBuffers is already synchronous.
            state.dbe_idiom_depth += 1;
            Vec::new()
        }
        END_IDIOM_REQUEST => {
            // EndIdiom: end of atomic swap group.
            if state.dbe_idiom_depth > 0 {
                state.dbe_idiom_depth -= 1;
            }
            Vec::new()
        }
        GET_VISUAL_INFO_REQUEST => {
            // GetVisualInfo
            // Return visual info for 1 screen with our 2 visuals supporting DBE
            let n_screens: u32 = 1;
            // PerflDepthInfo: depth(1) + pad(1) + n_visuals(2) + visual entries
            // Each visual entry: visual_id(4) + depth(1) + perflevel(1) + pad(2) = 8
            let n_visuals: u16 = 2; // 24-bit and 32-bit
            let per_depth_size = 4 + n_visuals as usize * 8; // header + visuals
            let screen_info_size = 4 + per_depth_size; // n_perfdepth(4) + depths
            let extra = 4 + screen_info_size; // n_screens already in header, then screen data
            let padded = (extra + 3) & !3;
            let mut reply =
                ReplyBuf::with_extra(seq, padded, state.msb_first).set_u32(8, n_screens);
            // Screen 0
            let off = 32;
            reply = reply.set_u32(off, 1); // n_perfdepth = 1

            // PerflDepthInfo
            let doff = off + 4;
            reply = reply
                .set_u8(doff, 24) // depth
                .set_u16(doff + 2, n_visuals);

            // Visual 0: ROOT_VISUAL (24-bit)
            let voff = doff + 4;
            reply = reply
                .set_u32(voff, 0x21) // ROOT_VISUAL
                .set_u8(voff + 4, 24) // depth
                .set_u8(voff + 5, 0); // performance level

            // Visual 1: ARGB visual (32-bit)
            let voff2 = voff + 8;
            reply = reply
                .set_u32(voff2, 0x40) // ARGB visual
                .set_u8(voff2 + 4, 32)
                .set_u8(voff2 + 5, 0);

            reply.build()
        }
        GET_BACK_BUFFER_ATTRIBUTES_REQUEST => {
            // GetBackBufferAttributes
            require_len!(data, 8, seq, 157, minor as u16, state.msb_first);
            use x11rb_protocol::protocol::dbe::GetBackBufferAttributesRequest;
            let req = parse_minor!(
                GetBackBufferAttributesRequest,
                data,
                state,
                seq,
                157,
                minor as u16
            );
            let back_buffer_id = req.buffer;
            let window_id = state
                .back_buffers
                .get(&back_buffer_id)
                .copied()
                .unwrap_or(0);
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, window_id)
                .build()
        }
        _ => {
            debug!("DBE: unhandled minor opcode {minor}");
            dbe_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
