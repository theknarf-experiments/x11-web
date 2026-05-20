//! DBE (Double Buffer Extension) handler.

use std::collections::HashMap;

use super::parse_minor;
use tracing::debug;

/// Per-connection DBE extension state. Lives on `ClientState::dbe`;
/// reads and writes happen through `state.dbe.*`.
#[derive(Default)]
pub(crate) struct DbeState {
    /// Back buffer allocations (back_buffer_id → window_id).
    pub(crate) back_buffers: HashMap<u32, u32>,
    /// Idiom nesting depth (BeginIdiom/EndIdiom).
    pub(crate) idiom_depth: u32,
}
use x11rb_protocol::protocol::dbe::{
    GetVisualInfoReply, SwapAction, VisualInfo, VisualInfos, ALLOCATE_BACK_BUFFER_REQUEST,
    BEGIN_IDIOM_REQUEST, DEALLOCATE_BACK_BUFFER_REQUEST, END_IDIOM_REQUEST,
    GET_BACK_BUFFER_ATTRIBUTES_REQUEST, GET_VISUAL_INFO_REQUEST, QUERY_VERSION_REQUEST,
    SWAP_BUFFERS_REQUEST,
};
use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

use super::super::client::ClientState;
use crate::framebuffer::Framebuffer;
use crate::xserver::core::{
    depth_for_visual, require_len, VISUAL_TRUE_COLOR_24, VISUAL_TRUE_COLOR_ARGB_32,
};
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::dbe::{
    BufferAttributes, GetBackBufferAttributesReply, QueryVersionReply as DbeQueryVersionReply,
};

/// DBE - Double Buffer Extension (opcode 157)
pub(crate) fn handle_dbe_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let dbe_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 157, minor as u16)
    };
    match minor {
        QUERY_VERSION_REQUEST => serialize_reply(
            &DbeQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: 1,
                minor_version: 0,
            },
            state.byte_order(),
        ),
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
            let _swap_action = req.swap_action; // Undefined, Background, Untouched, Copied

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
                state.dbe.back_buffers.insert(back_buffer_id, window_id);
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
            state.dbe.back_buffers.remove(&back_buffer_id);
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
                    .dbe
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
            state.dbe.idiom_depth += 1;
            Vec::new()
        }
        END_IDIOM_REQUEST => {
            // EndIdiom: end of atomic swap group.
            if state.dbe.idiom_depth > 0 {
                state.dbe.idiom_depth -= 1;
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
                        visual_id: VISUAL_TRUE_COLOR_24,
                        depth: depth_for_visual(VISUAL_TRUE_COLOR_24),
                        perf_level: 0,
                    },
                    VisualInfo {
                        visual_id: VISUAL_TRUE_COLOR_ARGB_32,
                        depth: depth_for_visual(VISUAL_TRUE_COLOR_ARGB_32),
                        perf_level: 0,
                    },
                ],
            }];
            let reply = GetVisualInfoReply {
                sequence: seq,
                length: 0,
                supported_visuals,
            };
            let byte_order = state.byte_order();
            let mut bytes = Vec::new();
            reply.serialize_endian_into(&mut bytes, byte_order);
            // Stamp length from the actual buffer size (overrides the
            // 0 we passed in), endian-aware.
            let length = ((bytes.len() - 32) / 4) as u32;
            let length_bytes = match byte_order {
                ByteOrder::Lsb => length.to_le_bytes(),
                ByteOrder::Msb => length.to_be_bytes(),
            };
            bytes[4..8].copy_from_slice(&length_bytes);
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
                .dbe
                .back_buffers
                .get(&back_buffer_id)
                .copied()
                .unwrap_or(0);
            serialize_reply(
                &GetBackBufferAttributesReply {
                    sequence: seq,
                    length: 0,
                    attributes: BufferAttributes { window: window_id },
                },
                state.byte_order(),
            )
        }
        _ => {
            debug!("DBE: unhandled minor opcode {minor}");
            dbe_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
