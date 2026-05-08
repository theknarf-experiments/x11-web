//! DBE (Double Buffer Extension) handler.

use super::parse_minor;
use tracing::debug;
use x11rb_protocol::protocol::dbe::{
    ALLOCATE_BACK_BUFFER_REQUEST, BEGIN_IDIOM_REQUEST, DEALLOCATE_BACK_BUFFER_REQUEST,
    END_IDIOM_REQUEST, GET_BACK_BUFFER_ATTRIBUTES_REQUEST, GET_VISUAL_INFO_REQUEST,
    GetVisualInfoReply, QUERY_VERSION_REQUEST, SWAP_BUFFERS_REQUEST, SwapAction, VisualInfo,
    VisualInfos,
};
use x11rb_protocol::x11_utils::Serialize;

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
            // GetVisualInfo: per the DBE spec the reply is one
            // `VisualInfos` entry per screen we advertise. We only
            // support one screen, with two DBE-capable visuals: the
            // root 24-bit visual and an ARGB 32-bit visual.
            let supported_visuals = vec![VisualInfos {
                infos: vec![
                    VisualInfo {
                        visual_id: 0x21, // ROOT_VISUAL
                        depth: 24,
                        perf_level: 0,
                    },
                    VisualInfo {
                        visual_id: 0x40, // ARGB visual
                        depth: 32,
                        perf_level: 0,
                    },
                ],
            }];
            let mut bytes = GetVisualInfoReply {
                sequence: seq,
                length: 0,
                supported_visuals,
            }
            .serialize();
            // Stamp length from the actual buffer size (overrides the
            // 0 we passed in).
            let length = ((bytes.len() - 32) / 4) as u32;
            bytes[4..8].copy_from_slice(&length.to_ne_bytes());
            if state.msb_first {
                byteswap_get_visual_info_reply(&mut bytes);
            }
            bytes
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

/// `GetVisualInfoReply`:
/// `[type:1, pad:1, sequence:u16, length:u32, n_supported_visuals:u32,
///   pad:20, supported_visuals:[VisualInfos]]`. Each `VisualInfos` is
/// `n_infos:u32` followed by `n_infos` × `VisualInfo (8 bytes:
/// visual_id:u32, depth:u8, perf_level:u8, pad:2)`.
///
/// Called before any byte-swaps; reads multi-byte fields in native
/// order to walk the layout, then swaps each field in place.
fn byteswap_get_visual_info_reply(buf: &mut [u8]) {
    use crate::xserver::byteswap::{swap_u16, swap_u32};
    let n_screens = u32::from_ne_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    swap_u16(buf, 2); // sequence
    swap_u32(buf, 4); // length
    swap_u32(buf, 8); // n_supported_visuals
    let mut off = 32;
    for _ in 0..n_screens {
        if off + 4 > buf.len() {
            return;
        }
        let n_infos =
            u32::from_ne_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as usize;
        swap_u32(buf, off); // n_infos
        off += 4;
        for _ in 0..n_infos {
            swap_u32(buf, off); // visual_id; depth/perf_level/pad are bytes
            off += 8;
        }
    }
}
