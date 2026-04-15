//! GLX context management (CreateContext, DestroyContext, MakeCurrent, IsDirect,
//! GLXSingle dispatch, VendorPrivateWithReply, SwapBuffers, WaitGL, WaitX).

use tracing::debug;

#[cfg(feature = "osmesa")]
use crate::osmesa;
#[cfg(feature = "osmesa")]
use crate::osmesa::MesaContext;

use super::super::super::client::ClientState;
use super::super::super::core::ROOT_VISUAL;
#[cfg(feature = "osmesa")]
use super::blit_osmesa_to_drawable;
use super::{get_drawable_size, GlxContext};

use super::single_ops;
use super::single_query;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// GLX_CREATE_CONTEXT (minor 3)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 24, seq, 159, 3, state.msb_first);
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let visual = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let screen = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let share_list = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

    let tag = state.glx.next_tag;
    state.glx.next_tag += 1;

    let context = GlxContext {
        id: ctx_id,
        visual,
        screen,
        tag,
        drawable: 0,
        share_list,
        #[cfg(feature = "osmesa")]
        mesa: None,
    };
    state.glx.contexts.insert(ctx_id, context);
    debug!("Created GLX context {ctx_id:#x} visual={visual:#x} share_list={share_list:#x}");

    Vec::new() // CreateContext is void
}

// ---------------------------------------------------------------------------
// GLX_CREATE_NEW_CONTEXT (minor 24)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_new_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Same layout but uses FBConfig ID instead of visual
    require_len!(data, 28, seq, 159, 24, state.msb_first);
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let fbconfig = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let screen = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let share_list = if data.len() >= 24 {
        u32::from_le_bytes([data[20], data[21], data[22], data[23]])
    } else {
        0
    };

    // Map fbconfig to visual
    let visual = if fbconfig == 2 { 0x40 } else { ROOT_VISUAL };

    let tag = state.glx.next_tag;
    state.glx.next_tag += 1;

    let context = GlxContext {
        id: ctx_id,
        visual,
        screen,
        tag,
        drawable: 0,
        share_list,
        #[cfg(feature = "osmesa")]
        mesa: None,
    };
    state.glx.contexts.insert(ctx_id, context);
    debug!("Created GLX new context {ctx_id:#x} fbconfig={fbconfig} share_list={share_list:#x}");

    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_CREATE_CONTEXT_ATTRIBS_ARB (minor 34)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_context_attribs(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 28, seq, 159, 34, state.msb_first);
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let fbconfig = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let screen = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let share_list = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

    let visual = if fbconfig == 2 { 0x40 } else { ROOT_VISUAL };

    let tag = state.glx.next_tag;
    state.glx.next_tag += 1;

    let context = GlxContext {
        id: ctx_id,
        visual,
        screen,
        tag,
        drawable: 0,
        share_list,
        #[cfg(feature = "osmesa")]
        mesa: None,
    };
    state.glx.contexts.insert(ctx_id, context);
    debug!("Created GLX context via attribs {ctx_id:#x} fbconfig={fbconfig} share_list={share_list:#x}");

    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_DESTROY_CONTEXT (minor 4)
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 4, state.msb_first);
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.glx.contexts.remove(&ctx_id);
    state.recycle_xid(ctx_id);
    if state.glx.current_context == ctx_id {
        state.glx.current_context = 0;
        state.glx.current_drawable = 0;
    }
    debug!("Destroyed GLX context {ctx_id:#x}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_MAKE_CURRENT (minor 5)
// ---------------------------------------------------------------------------

pub(crate) fn handle_make_current(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 16, seq, 159, 5, state.msb_first);
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let ctx_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    let tag = do_make_current(state, ctx_id, drawable);

    // Reply: context_tag
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&tag.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// GLX_MAKE_CONTEXT_CURRENT (minor 26)
// ---------------------------------------------------------------------------

pub(crate) fn handle_make_context_current(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 20, seq, 159, 26, state.msb_first);
    // draw drawable, read drawable, context
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _read_drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let ctx_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let tag = do_make_current(state, ctx_id, drawable);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&tag.to_le_bytes());
    reply.to_vec()
}

/// Common logic for MakeCurrent and MakeContextCurrent.
fn do_make_current(state: &mut ClientState, ctx_id: u32, drawable: u32) -> u32 {
    if ctx_id == 0 {
        // Unbind
        state.glx.current_context = 0;
        state.glx.current_drawable = 0;
        return 0;
    }

    // Look up the drawable dimensions
    let (width, height) = get_drawable_size(state, drawable);

    state.glx.current_context = ctx_id;
    state.glx.current_drawable = drawable;

    let tag = if let Some(ctx) = state.glx.contexts.get_mut(&ctx_id) {
        ctx.drawable = drawable;

        #[cfg(feature = "osmesa")]
        {
            if osmesa::is_available() {
                if ctx.mesa.is_none() {
                    ctx.mesa = MesaContext::new(width, height);
                    if ctx.mesa.is_some() {
                        debug!("Created OSMesa context for GLX ctx {ctx_id:#x} drawable {drawable:#x} ({width}x{height})");
                    }
                } else if let Some(ref mut mesa) = ctx.mesa {
                    if mesa.width() != width || mesa.height() != height {
                        mesa.resize(width, height);
                    } else {
                        mesa.make_current();
                    }
                }
            }
        }

        ctx.tag
    } else {
        0
    };

    debug!("GLX MakeCurrent ctx={ctx_id:#x} drawable={drawable:#x} tag={tag}");
    tag
}

// ---------------------------------------------------------------------------
// GLX_IS_DIRECT (minor 6)
// ---------------------------------------------------------------------------

pub(crate) fn handle_is_direct(seq: u16) -> Vec<u8> {
    // Indirect rendering (is_direct = false)
    // GLX IsDirect reply: byte 0 = reply type, byte 1 = is_direct (BOOL)
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 0; // is_direct = false (byte 1 per GLX spec)
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// GLX_COPY_CONTEXT (minor 10)
// ---------------------------------------------------------------------------

/// Copies GL state groups from source to destination context.
/// Under OSMesa software rendering, context state is managed internally per
/// context and cross-context state copying is not supported (same limitation
/// as other indirect renderers).  We validate both contexts exist and
/// acknowledge the request.
pub(crate) fn handle_copy_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: src_context(4) dst_context(4) mask(4) src_context_tag(4)
    require_len!(data, 20, seq, 159, 10, state.msb_first);
    let src_ctx = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst_ctx = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    // Validate both contexts exist
    if !state.glx.contexts.contains_key(&src_ctx) {
        // GLXBadContext error (first error code for GLX extension = 160 by convention)
        return crate::xserver::core::build_error_bo(160, seq, src_ctx, 159, 10, state.msb_first);
    }
    if !state.glx.contexts.contains_key(&dst_ctx) {
        return crate::xserver::core::build_error_bo(160, seq, dst_ctx, 159, 10, state.msb_first);
    }

    debug!("GLX CopyContext: src={src_ctx:#x} dst={dst_ctx:#x}");
    Vec::new() // Void request
}

// ---------------------------------------------------------------------------
// GLX single GL commands (minor opcodes 101+) -- GL calls that require a reply
// ---------------------------------------------------------------------------

pub(crate) fn handle_glx_single(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // GLX single GL commands arrive as individual minor opcodes (101+).
    // Wire layout: major_opcode(1) minor=gl_opcode(1) req_len(2) context_tag(4) payload(...)
    require_len!(data, 8, seq, 159, data[1] as u16, state.msb_first);
    let _context_tag = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gl_opcode = data[1] as u32;
    let payload = if data.len() > 8 { &data[8..] } else { &[] };

    // Ensure the current OSMesa context is active before GL queries
    #[cfg(feature = "osmesa")]
    {
        if let Some(ctx) = state.glx.contexts.get_mut(&state.glx.current_context) {
            if let Some(ref mut mesa) = ctx.mesa {
                mesa.make_current();
            }
        }
    }

    match gl_opcode {
        // --- Query/info operations (delegated to single_query) ---
        111 => single_query::handle_get_booleanv(payload, seq),
        112 => single_query::handle_get_floatv(payload, seq),
        113 => single_query::handle_get_doublev(payload, seq),
        115 => single_query::handle_get_string(payload, seq),
        116 => single_query::handle_get_error(seq),
        117 => single_query::handle_get_integerv(payload, seq),
        118 => single_query::handle_is_enabled(payload, seq),
        119 => single_query::handle_is_texture(payload, seq),
        123 => single_query::handle_get_materialfv(payload, seq),
        124 => single_query::handle_get_materialiv(payload, seq),
        125 => single_query::handle_gen_textures(payload, seq),
        126 => single_query::handle_get_pixel_mapfv(payload, seq),
        127 => single_query::handle_get_clip_plane(payload, seq),
        128 => single_query::handle_get_polygon_stipple(seq),
        130 => single_query::handle_get_tex_envfv(payload, seq),
        131 => single_query::handle_get_tex_enviv(payload, seq),
        132 => single_query::handle_get_tex_gendv(payload, seq),
        133 => single_query::handle_get_tex_genfv(payload, seq),
        134 => single_query::handle_get_tex_geniv(payload, seq),
        135 => single_query::handle_get_tex_image(payload, seq),
        136 => single_query::handle_get_tex_parameteriv(payload, seq),
        137 => single_query::handle_get_tex_parameterfv(payload, seq),
        138 => single_query::handle_get_tex_level_parameteriv(payload, seq),
        139 => single_query::handle_get_tex_level_parameterfv(payload, seq),
        141 => single_query::handle_is_list(payload, seq),
        149 => single_query::handle_get_lightfv(payload, seq),
        150 => single_query::handle_get_lightiv(payload, seq),
        104 => single_query::handle_gen_lists(payload, seq),

        // --- Single-shot GL operations (delegated to single_ops) ---
        103 => single_ops::handle_delete_lists(payload, seq),
        107 => single_ops::handle_render_mode(payload, seq),
        108 => single_ops::handle_finish(seq),
        109 => single_ops::handle_pixel_storef(payload, seq),
        110 => single_ops::handle_pixel_storei(payload, seq),
        143 => single_ops::handle_are_textures_resident(payload, seq),
        144 => single_ops::handle_delete_textures(payload, seq),

        _ => {
            debug!("Unhandled GLX single opcode: {gl_opcode}");
            glx_single_empty_reply(seq)
        }
    }
}

pub(crate) fn glx_single_empty_reply(seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// GLX_VENDOR_PRIVATE_WITH_REPLY (minor 17) -- extensions that need a reply
// ---------------------------------------------------------------------------

pub(crate) fn handle_vendor_private_with_reply(data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: major(1) minor=17(1) req_len(2) vendor_code(4) context_tag(4) payload(...)
    if data.len() < 12 {
        return glx_single_empty_reply(seq);
    }
    let vendor_code = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    match vendor_code {
        // glXGetProcAddressARB (vendor code 1296)
        1296 => {
            // Client wants a function pointer. For indirect rendering we just
            // return a non-NULL stub -- the actual pointer lives in the client library.
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // Return a dummy non-zero proc address (1 = "supported but opaque")
            reply[8..12].copy_from_slice(&1u32.to_le_bytes());
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled GLX vendor private with reply: {vendor_code}");
            glx_single_empty_reply(seq)
        }
    }
}

// ---------------------------------------------------------------------------
// GLX_SWAP_BUFFERS (minor 11)
// ---------------------------------------------------------------------------

pub(crate) fn handle_swap_buffers(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 12, seq, 159, 11, state.msb_first);
    let _tag = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    #[cfg(feature = "osmesa")]
    {
        blit_osmesa_to_drawable(state, drawable);
    }

    debug!("GLX SwapBuffers drawable={drawable:#x}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_WAIT_GL (minor 8) -- wait for GL rendering to complete, then blit to X
// ---------------------------------------------------------------------------

pub(crate) fn handle_wait_gl(state: &mut ClientState) -> Vec<u8> {
    #[cfg(feature = "osmesa")]
    {
        // Flush and finish all pending GL operations
        let ctx_id = state.glx.current_context;
        if ctx_id != 0 {
            if let Some(ctx) = state.glx.contexts.get_mut(&ctx_id) {
                if let Some(ref mut mesa) = ctx.mesa {
                    mesa.make_current();
                    osmesa::gl_finish();
                }
            }
            // Blit the GL results to the drawable
            let drawable = state.glx.current_drawable;
            if drawable != 0 {
                blit_osmesa_to_drawable(state, drawable);
            }
        }
    }
    debug!("GLX WaitGL");
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_WAIT_X (minor 9) -- wait for X rendering to complete before GL reads
// ---------------------------------------------------------------------------

pub(crate) fn handle_wait_x(state: &mut ClientState) -> Vec<u8> {
    #[cfg(feature = "osmesa")]
    {
        // Ensure any pending X drawing operations are flushed to the framebuffer
        // so that subsequent GL operations (like glReadPixels) see the X content.
        // Since our X rendering is synchronous (no GPU pipeline), this is already
        // guaranteed. We just need to ensure the OSMesa context's backing buffer
        // is updated from the X framebuffer if needed.
        let ctx_id = state.glx.current_context;
        let drawable = state.glx.current_drawable;
        if ctx_id != 0 && drawable != 0 {
            // Copy the X framebuffer content into the OSMesa buffer
            let target = state.resolve_drawable(drawable);
            let x_pixels: Option<(Vec<u8>, u32, u32)> = state
                .get_framebuffer(target)
                .map(|fb| (fb.data().to_vec(), fb.width(), fb.height()));

            if let Some((pixels, w, h)) = x_pixels {
                if let Some(ctx) = state.glx.contexts.get_mut(&ctx_id) {
                    if let Some(ref mut mesa) = ctx.mesa {
                        mesa.make_current();
                        // Update the OSMesa buffer with X framebuffer contents
                        let mesa_w = mesa.width();
                        let mesa_h = mesa.height();
                        let copy_w = w.min(mesa_w) as usize;
                        let copy_h = h.min(mesa_h) as usize;
                        let src_stride = w as usize * 4;
                        let dst_stride = mesa_w as usize * 4;
                        let mesa_pixels = mesa.pixels_mut();
                        for y in 0..copy_h {
                            let src = &pixels[y * src_stride..y * src_stride + copy_w * 4];
                            let dst = &mut mesa_pixels[y * dst_stride..y * dst_stride + copy_w * 4];
                            dst.copy_from_slice(src);
                        }
                    }
                }
            }
        }
    }
    debug!("GLX WaitX");
    Vec::new()
}
