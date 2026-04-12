//! GLX context management (CreateContext, DestroyContext, MakeCurrent, IsDirect,
//! GLXSingle, VendorPrivateWithReply, SwapBuffers, WaitGL, WaitX).

use tracing::debug;

#[cfg(feature = "osmesa")]
use crate::osmesa;
#[cfg(feature = "osmesa")]
use crate::osmesa::MesaContext;

use super::super::super::client::ClientState;
use super::super::super::core::ROOT_VISUAL;
use super::{GlxContext, get_drawable_size};
#[cfg(feature = "osmesa")]
use super::blit_osmesa_to_drawable;

// ---------------------------------------------------------------------------
// GLX_CREATE_CONTEXT (minor 3)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 24 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 3, state.msb_first,
        );
    }
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
    if data.len() < 28 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 24, state.msb_first,
        );
    }
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let fbconfig = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let screen = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let share_list = if data.len() >= 24 {
        u32::from_le_bytes([data[20], data[21], data[22], data[23]])
    } else { 0 };

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

pub(crate) fn handle_create_context_attribs(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 28 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 34, state.msb_first,
        );
    }
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
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 4, state.msb_first,
        );
    }
    let ctx_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.glx.contexts.remove(&ctx_id);
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
    if data.len() < 16 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 5, state.msb_first,
        );
    }
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

pub(crate) fn handle_make_context_current(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 20 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 26, state.msb_first,
        );
    }
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
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8] = 0; // is_direct = false
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
    if data.len() < 20 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 10, state.msb_first,
        );
    }
    let src_ctx = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst_ctx = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    // Validate both contexts exist
    if !state.glx.contexts.contains_key(&src_ctx) {
        // GLXBadContext error (first error code for GLX extension = 160 by convention)
        return crate::xserver::core::build_error_bo(
            160, seq, src_ctx, 159, 10, state.msb_first,
        );
    }
    if !state.glx.contexts.contains_key(&dst_ctx) {
        return crate::xserver::core::build_error_bo(
            160, seq, dst_ctx, 159, 10, state.msb_first,
        );
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
    if data.len() < 8 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, data[1] as u16, state.msb_first,
        );
    }
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
        // glGetError (opcode 116)
        116 => {
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    let err = osmesa::gl_get_error();
                    let mut reply = [0u8; 32];
                    reply[0] = 1;
                    reply[2..4].copy_from_slice(&seq.to_le_bytes());
                    reply[8..12].copy_from_slice(&err.to_le_bytes());
                    return reply.to_vec();
                }
            }
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        // glGetIntegerv (opcode 117)
        117 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n: usize = gl_integer_count(pname);
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_integerv(pname, &mut params);
                }
            }
            let data_bytes = n * 4;
            let extra_words = (data_bytes + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetFloatv (opcode 112)
        112 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n: usize = gl_float_count(pname);
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_floatv(pname, &mut params);
                }
            }
            let data_bytes = n * 4;
            let extra_words = (data_bytes + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetDoublev (opcode 113)
        113 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n: usize = gl_float_count(pname);
            let mut params = vec![0f64; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_doublev(pname, &mut params);
                }
            }
            let data_bytes = n * 8;
            let extra_words = (data_bytes + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 8;
                reply[off..off + 8].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetBooleanv (opcode 111)
        111 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n: usize = gl_float_count(pname);
            let mut params = vec![0u8; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_booleanv(pname, &mut params);
                }
            }
            let extra_words = (n + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            reply[32..32 + n].copy_from_slice(&params);
            reply
        }
        // glGetString (opcode 115)
        115 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let name = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let s = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        osmesa::gl_get_string(name)
                    } else {
                        String::new()
                    }
                }
                #[cfg(not(feature = "osmesa"))]
                { String::new() }
            };
            let bytes = s.as_bytes();
            let n = bytes.len() as u32;
            let padded = (bytes.len() + 3) & !3;
            let extra_words = (padded / 4).max(1);
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&n.to_le_bytes());
            if !bytes.is_empty() {
                reply[32..32 + bytes.len()].copy_from_slice(bytes);
            }
            reply
        }
        // glIsEnabled (opcode 118)
        118 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let cap = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let enabled: u8 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        if osmesa::gl_is_enabled(cap) { 1 } else { 0 }
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = enabled;
            reply.to_vec()
        }
        // glGenTextures (opcode 125)
        125 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let n = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n = n.max(0) as usize;
            let mut textures = vec![0u32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() && n > 0 {
                    osmesa::gl_gen_textures(n as i32, &mut textures);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &t) in textures.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&t.to_le_bytes());
            }
            reply
        }
        // glGetTexParameteriv (opcode 136)
        136 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = 1usize;
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_parameteriv(target, pname, &mut params);
                }
            }
            let mut reply = vec![0u8; 32 + n * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(n as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexParameterfv (opcode 137)
        137 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = 1usize;
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_parameterfv(target, pname, &mut params);
                }
            }
            let mut reply = vec![0u8; 32 + n * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(n as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glDeleteLists (opcode 103)
        103 => {
            if payload.len() >= 8 {
                let list = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let range = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        osmesa::gl_delete_lists(list, range);
                    }
                }
            }
            glx_single_empty_reply(seq)
        }
        // glGenLists (opcode 104)
        104 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let range = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let result: u32 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() && range > 0 {
                        osmesa::gl_gen_lists(range)
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&result.to_le_bytes());
            reply.to_vec()
        }
        // glIsList (opcode 141)
        141 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let list = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let result: u8 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        if osmesa::gl_is_list(list) { 1 } else { 0 }
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = result;
            reply.to_vec()
        }
        // glRenderMode (opcode 107)
        107 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let mode = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let result: i32 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        osmesa::gl_render_mode(mode)
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&result.to_le_bytes());
            reply.to_vec()
        }
        // glFinish (opcode 108)
        108 => {
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_finish();
                }
            }
            glx_single_empty_reply(seq)
        }
        // glPixelStoref (opcode 109)
        109 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let param = f32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_pixel_storef(pname, param);
                }
            }
            glx_single_empty_reply(seq)
        }
        // glPixelStorei (opcode 110)
        110 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let param = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_pixel_storei(pname, param);
                }
            }
            glx_single_empty_reply(seq)
        }
        // glIsTexture (opcode 119)
        119 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let texture = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let result: u8 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        if osmesa::gl_is_texture(texture) { 1 } else { 0 }
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = result;
            reply.to_vec()
        }
        // glGetMaterialfv (opcode 123)
        123 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let face = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_material_param_count(pname);
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_materialfv(face, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetMaterialiv (opcode 124)
        124 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let face = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_material_param_count(pname);
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_materialiv(face, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetPixelMapfv (opcode 126)
        126 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let map = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            // Query the map size first via glGetIntegerv on the corresponding GL_PIXEL_MAP_*_SIZE
            let size_pname = gl_pixel_map_size_pname(map);
            let n: usize = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() && size_pname != 0 {
                        let mut sz = [0i32; 1];
                        osmesa::gl_get_integerv(size_pname, &mut sz);
                        sz[0].max(0) as usize
                    } else { 0 }
                }
                #[cfg(not(feature = "osmesa"))] { 0 }
            };
            let mut values = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() && n > 0 {
                    osmesa::gl_get_pixel_mapfv(map, &mut values);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in values.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetClipPlane (opcode 127)
        127 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let plane = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let mut equation = [0f64; 4];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_clip_plane(plane, &mut equation);
                }
            }
            let n = 4usize;
            let data_bytes = n * 8;
            let extra_words = (data_bytes + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in equation.iter().enumerate() {
                let off = 32 + i * 8;
                reply[off..off + 8].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetPolygonStipple (opcode 128)
        128 => {
            let mut mask = [0u8; 128];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_polygon_stipple(&mut mask);
                }
            }
            let extra_words = 128 / 4; // 32 words
            let mut reply = vec![0u8; 32 + 128];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&128u32.to_le_bytes());
            reply[32..32 + 128].copy_from_slice(&mask);
            reply
        }
        // glGetTexEnvfv (opcode 130)
        130 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_texenv_param_count(pname);
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_envfv(target, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexEnviv (opcode 131)
        131 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_texenv_param_count(pname);
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_enviv(target, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexGendv (opcode 132)
        132 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let coord = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_texgen_param_count(pname);
            let mut params = vec![0f64; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_gendv(coord, pname, &mut params);
                }
            }
            let data_bytes = n * 8;
            let extra_words = (data_bytes + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 8;
                reply[off..off + 8].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexGenfv (opcode 133)
        133 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let coord = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_texgen_param_count(pname);
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_genfv(coord, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexGeniv (opcode 134)
        134 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let coord = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_texgen_param_count(pname);
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_geniv(coord, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexImage (opcode 135)
        135 => {
            if payload.len() < 16 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let level = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let format = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
            let type_ = u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
            // Query texture dimensions via glGetTexLevelParameteriv
            let (width, height): (i32, i32) = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() {
                        let mut w = [0i32; 1];
                        let mut h = [0i32; 1];
                        osmesa::gl_get_tex_level_parameteriv(target, level, 0x1000, &mut w); // GL_TEXTURE_WIDTH
                        osmesa::gl_get_tex_level_parameteriv(target, level, 0x1001, &mut h); // GL_TEXTURE_HEIGHT
                        (w[0], h[0])
                    } else { (0, 0) }
                }
                #[cfg(not(feature = "osmesa"))] { (0, 0) }
            };
            let components = gl_format_components(format);
            let type_size = gl_type_size(type_);
            let image_size = (width.max(0) as usize) * (height.max(0) as usize) * components * type_size;
            if image_size == 0 {
                return glx_single_empty_reply(seq);
            }
            let mut pixels = vec![0u8; image_size];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_image(target, level, format, type_, &mut pixels);
                }
            }
            let extra_words = (image_size + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(image_size as u32).to_le_bytes());
            reply[32..32 + image_size].copy_from_slice(&pixels);
            reply
        }
        // glGetTexLevelParameteriv (opcode 138)
        138 => {
            if payload.len() < 12 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let level = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let pname = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
            let n = 1usize;
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_level_parameteriv(target, level, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetTexLevelParameterfv (opcode 139)
        139 => {
            if payload.len() < 12 {
                return glx_single_empty_reply(seq);
            }
            let target = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let level = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let pname = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
            let n = 1usize;
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_tex_level_parameterfv(target, level, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glAreTexturesResident (opcode 143)
        143 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let n = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n = n.max(0) as usize;
            if payload.len() < 4 + n * 4 {
                return glx_single_empty_reply(seq);
            }
            let mut textures = vec![0u32; n];
            for i in 0..n {
                let off = 4 + i * 4;
                textures[i] = u32::from_le_bytes([payload[off], payload[off + 1], payload[off + 2], payload[off + 3]]);
            }
            let mut residences = vec![0u8; n];
            let all_resident: u8 = {
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() && n > 0 {
                        if osmesa::gl_are_textures_resident(&textures, &mut residences) { 1 } else { 0 }
                    } else { 1 }
                }
                #[cfg(not(feature = "osmesa"))] { 1 }
            };
            let extra_words = (n + 3) / 4;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(all_resident as u32).to_le_bytes());
            for i in 0..n {
                reply[32 + i] = residences[i];
            }
            reply
        }
        // glDeleteTextures (opcode 144)
        144 => {
            if payload.len() < 4 {
                return glx_single_empty_reply(seq);
            }
            let n = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let n = n.max(0) as usize;
            if payload.len() >= 4 + n * 4 {
                let mut textures = vec![0u32; n];
                for i in 0..n {
                    let off = 4 + i * 4;
                    textures[i] = u32::from_le_bytes([payload[off], payload[off + 1], payload[off + 2], payload[off + 3]]);
                }
                #[cfg(feature = "osmesa")]
                {
                    if osmesa::is_available() && n > 0 {
                        osmesa::gl_delete_textures(&textures);
                    }
                }
            }
            glx_single_empty_reply(seq)
        }
        // glGetLightfv (opcode 149)
        149 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let light = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_light_param_count(pname);
            let mut params = vec![0f32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_lightfv(light, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
        // glGetLightiv (opcode 150)
        150 => {
            if payload.len() < 8 {
                return glx_single_empty_reply(seq);
            }
            let light = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let pname = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let n = gl_light_param_count(pname);
            let mut params = vec![0i32; n];
            #[cfg(feature = "osmesa")]
            {
                if osmesa::is_available() {
                    osmesa::gl_get_lightiv(light, pname, &mut params);
                }
            }
            let extra_words = n;
            let mut reply = vec![0u8; 32 + extra_words * 4];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            for (i, &v) in params.iter().enumerate() {
                let off = 32 + i * 4;
                reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            reply
        }
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

/// Return the expected number of values for a given GL state query.
fn gl_integer_count(pname: u32) -> usize {
    match pname {
        // Matrices (16 values)
        0x0BA6 | // GL_MODELVIEW_MATRIX
        0x0BA7 | // GL_PROJECTION_MATRIX
        0x0BA8   // GL_TEXTURE_MATRIX
        => 16,
        // 4-component vectors
        0x0B23 | // GL_COLOR_CLEAR_VALUE
        0x0C23 | // GL_COLOR_CLEAR_VALUE (alias)
        0x0B24 | // GL_ACCUM_CLEAR_VALUE
        0x0B25 | // GL_CURRENT_COLOR
        0x0C22 | // GL_INDEX_CLEAR_VALUE
        0x0B21 | // GL_SCISSOR_BOX (x,y,w,h)
        0x0BA2   // GL_VIEWPORT
        => 4,
        // 2-component
        0x0B70 | // GL_DEPTH_RANGE
        0x0C30 | // GL_POINT_SIZE_RANGE
        0x0B13   // GL_LINE_WIDTH_RANGE
        => 2,
        // Everything else: scalar
        _ => 1,
    }
}

/// Return the expected count for float/double queries.
fn gl_float_count(pname: u32) -> usize {
    gl_integer_count(pname)
}

/// Return the expected number of values for a glGetLight query.
fn gl_light_param_count(pname: u32) -> usize {
    match pname {
        0x1200 | // GL_AMBIENT
        0x1201 | // GL_DIFFUSE
        0x1202 | // GL_SPECULAR
        0x1203   // GL_POSITION
        => 4,
        0x1204   // GL_SPOT_DIRECTION
        => 3,
        // GL_SPOT_EXPONENT, GL_SPOT_CUTOFF, GL_CONSTANT_ATTENUATION,
        // GL_LINEAR_ATTENUATION, GL_QUADRATIC_ATTENUATION
        _ => 1,
    }
}

/// Return the expected number of values for a glGetMaterial query.
fn gl_material_param_count(pname: u32) -> usize {
    match pname {
        0x1200 | // GL_AMBIENT
        0x1201 | // GL_DIFFUSE
        0x1202 | // GL_SPECULAR
        0x1600   // GL_EMISSION
        => 4,
        0x1602   // GL_COLOR_INDEXES
        => 3,
        0x1601   // GL_SHININESS
        => 1,
        _ => 1,
    }
}

/// Return the expected number of values for a glGetTexEnv query.
fn gl_texenv_param_count(pname: u32) -> usize {
    match pname {
        0x2201 // GL_TEXTURE_ENV_COLOR
        => 4,
        // GL_TEXTURE_ENV_MODE, GL_COMBINE_RGB, GL_COMBINE_ALPHA, etc.
        _ => 1,
    }
}

/// Return the expected number of values for a glGetTexGen query.
fn gl_texgen_param_count(pname: u32) -> usize {
    match pname {
        0x2501 | // GL_OBJECT_PLANE
        0x2502   // GL_EYE_PLANE
        => 4,
        // GL_TEXTURE_GEN_MODE
        _ => 1,
    }
}

/// Map GL_PIXEL_MAP_* enum to its corresponding GL_PIXEL_MAP_*_SIZE enum.
fn gl_pixel_map_size_pname(map: u32) -> u32 {
    match map {
        0x0C70 => 0x0CB0, // GL_PIXEL_MAP_I_TO_I -> GL_PIXEL_MAP_I_TO_I_SIZE
        0x0C71 => 0x0CB1, // GL_PIXEL_MAP_S_TO_S -> GL_PIXEL_MAP_S_TO_S_SIZE
        0x0C72 => 0x0CB2, // GL_PIXEL_MAP_I_TO_R -> GL_PIXEL_MAP_I_TO_R_SIZE
        0x0C73 => 0x0CB3, // GL_PIXEL_MAP_I_TO_G -> GL_PIXEL_MAP_I_TO_G_SIZE
        0x0C74 => 0x0CB4, // GL_PIXEL_MAP_I_TO_B -> GL_PIXEL_MAP_I_TO_B_SIZE
        0x0C75 => 0x0CB5, // GL_PIXEL_MAP_I_TO_A -> GL_PIXEL_MAP_I_TO_A_SIZE
        0x0C76 => 0x0CB6, // GL_PIXEL_MAP_R_TO_R -> GL_PIXEL_MAP_R_TO_R_SIZE
        0x0C77 => 0x0CB7, // GL_PIXEL_MAP_G_TO_G -> GL_PIXEL_MAP_G_TO_G_SIZE
        0x0C78 => 0x0CB8, // GL_PIXEL_MAP_B_TO_B -> GL_PIXEL_MAP_B_TO_B_SIZE
        0x0C79 => 0x0CB9, // GL_PIXEL_MAP_A_TO_A -> GL_PIXEL_MAP_A_TO_A_SIZE
        _ => 0,
    }
}

/// Return the number of components for a GL pixel format.
fn gl_format_components(format: u32) -> usize {
    match format {
        0x1900 => 1, // GL_COLOR_INDEX
        0x1901 => 1, // GL_STENCIL_INDEX
        0x1902 => 1, // GL_DEPTH_COMPONENT
        0x1903 => 1, // GL_RED
        0x1904 => 1, // GL_GREEN
        0x1905 => 1, // GL_BLUE
        0x1906 => 1, // GL_ALPHA
        0x1907 => 3, // GL_RGB
        0x1908 => 4, // GL_RGBA
        0x190A => 2, // GL_LUMINANCE_ALPHA
        0x1909 => 1, // GL_LUMINANCE
        0x80E0 => 3, // GL_BGR
        0x80E1 => 4, // GL_BGRA
        _ => 4,
    }
}

/// Return the byte size of a GL type.
fn gl_type_size(type_: u32) -> usize {
    match type_ {
        0x1400 => 1, // GL_BYTE
        0x1401 => 1, // GL_UNSIGNED_BYTE
        0x1402 => 2, // GL_SHORT
        0x1403 => 2, // GL_UNSIGNED_SHORT
        0x1404 => 4, // GL_INT
        0x1405 => 4, // GL_UNSIGNED_INT
        0x1406 => 4, // GL_FLOAT
        _ => 1,
    }
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
    if data.len() < 12 {
        return crate::xserver::core::build_error_bo(
            crate::xserver::core::BAD_LENGTH, seq, data.len() as u32,
            159, 11, state.msb_first,
        );
    }
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
            let x_pixels: Option<(Vec<u8>, u32, u32)> = state.get_framebuffer(target)
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
