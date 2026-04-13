//! GLX single GL query/info operations (glGet*, glIs*, glGenTextures, glGenLists, etc.).

#[cfg(feature = "osmesa")]
use crate::osmesa;

use super::context::glx_single_empty_reply;

// ---------------------------------------------------------------------------
// glGetError (opcode 116)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_error(seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetIntegerv (opcode 117)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_integerv(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = data_bytes.div_ceil(4);
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

// ---------------------------------------------------------------------------
// glGetFloatv (opcode 112)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_floatv(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = data_bytes.div_ceil(4);
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

// ---------------------------------------------------------------------------
// glGetDoublev (opcode 113)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_doublev(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = data_bytes.div_ceil(4);
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

// ---------------------------------------------------------------------------
// glGetBooleanv (opcode 111)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_booleanv(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = n.div_ceil(4);
    let mut reply = vec![0u8; 32 + extra_words * 4];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&(n as u32).to_le_bytes());
    reply[32..32 + n].copy_from_slice(&params);
    reply
}

// ---------------------------------------------------------------------------
// glGetString (opcode 115)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_string(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glIsEnabled (opcode 118)
// ---------------------------------------------------------------------------

pub(crate) fn handle_is_enabled(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glIsTexture (opcode 119)
// ---------------------------------------------------------------------------

pub(crate) fn handle_is_texture(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGenTextures (opcode 125)
// ---------------------------------------------------------------------------

pub(crate) fn handle_gen_textures(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexParameteriv (opcode 136)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_parameteriv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexParameterfv (opcode 137)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_parameterfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexLevelParameteriv (opcode 138)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_level_parameteriv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexLevelParameterfv (opcode 139)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_level_parameterfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexImage (opcode 135)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_image(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = image_size.div_ceil(4);
    let mut reply = vec![0u8; 32 + extra_words * 4];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&(image_size as u32).to_le_bytes());
    reply[32..32 + image_size].copy_from_slice(&pixels);
    reply
}

// ---------------------------------------------------------------------------
// glGetLightfv (opcode 149)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_lightfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetLightiv (opcode 150)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_lightiv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetMaterialfv (opcode 123)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_materialfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetMaterialiv (opcode 124)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_materialiv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexEnvfv (opcode 130)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_envfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexEnviv (opcode 131)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_enviv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexGendv (opcode 132)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_gendv(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = data_bytes.div_ceil(4);
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

// ---------------------------------------------------------------------------
// glGetTexGenfv (opcode 133)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_genfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetTexGeniv (opcode 134)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_tex_geniv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetPixelMapfv (opcode 126)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_pixel_mapfv(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGetClipPlane (opcode 127)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_clip_plane(payload: &[u8], seq: u16) -> Vec<u8> {
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
    let extra_words = data_bytes.div_ceil(4);
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

// ---------------------------------------------------------------------------
// glGetPolygonStipple (opcode 128)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_polygon_stipple(seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glGenLists (opcode 104)
// ---------------------------------------------------------------------------

pub(crate) fn handle_gen_lists(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// glIsList (opcode 141)
// ---------------------------------------------------------------------------

pub(crate) fn handle_is_list(payload: &[u8], seq: u16) -> Vec<u8> {
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

// ---------------------------------------------------------------------------
// Helper functions for GL parameter counts
// ---------------------------------------------------------------------------

/// Return the expected number of values for a given GL state query.
fn gl_integer_count(pname: u32) -> usize {
    match pname {
        // Matrices (16 values)
        0x0BA6..=0x0BA8   // GL_TEXTURE_MATRIX
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
        0x1200..=0x1203   // GL_POSITION
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
