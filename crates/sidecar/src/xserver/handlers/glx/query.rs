//! GLX query operations (QueryVersion, GetVisualConfigs, GetFBConfigs,
//! QueryExtensionsString, QueryServerString).

use super::super::super::core::ROOT_VISUAL;
use super::{
    FBCONFIG_ATTRIB_COUNT, GLX_ACCUM_ALPHA_SIZE, GLX_ACCUM_BLUE_SIZE, GLX_ACCUM_GREEN_SIZE,
    GLX_ACCUM_RED_SIZE, GLX_ALPHA_SIZE, GLX_AUX_BUFFERS, GLX_BLUE_SIZE, GLX_BUFFER_SIZE,
    GLX_CONFIG_CAVEAT, GLX_DEPTH_SIZE, GLX_DOUBLEBUFFER, GLX_DRAWABLE_TYPE, GLX_FBCONFIG_ID,
    GLX_GREEN_SIZE, GLX_LEVEL, GLX_MAX_PBUFFER_HEIGHT, GLX_MAX_PBUFFER_PIXELS,
    GLX_MAX_PBUFFER_WIDTH, GLX_NONE, GLX_PBUFFER_BIT, GLX_PIXMAP_BIT, GLX_RED_SIZE,
    GLX_RENDER_TYPE, GLX_RGBA_BIT, GLX_SAMPLES, GLX_SAMPLE_BUFFERS, GLX_STENCIL_SIZE, GLX_STEREO,
    GLX_TRANSPARENT_TYPE, GLX_TRUE_COLOR, GLX_VISUAL_ID, GLX_WINDOW_BIT, GLX_X_RENDERABLE,
    GLX_X_VISUAL_TYPE,
};

// ---------------------------------------------------------------------------
// GLX_QUERY_VERSION (minor 7)
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_version(seq: u16) -> Vec<u8> {
    // Return GLX 1.4
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major
    reply[12..16].copy_from_slice(&4u32.to_le_bytes()); // minor
    reply.to_vec()
}

// ---------------------------------------------------------------------------
// GLX_GET_VISUAL_CONFIGS (minor 14)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_visual_configs(_data: &[u8], seq: u16) -> Vec<u8> {
    // Return a single visual config matching ROOT_VISUAL
    let num_configs: u32 = 1;
    // Use exactly __GLX_MIN_CONFIG_PROPS (18) properties per config.
    // Mesa's createConfigsFromProperties reads exactly numProps * 4 bytes.
    let props_per_config: u32 = 18;
    let total_props = num_configs * props_per_config;

    let extra_bytes = total_props as usize * 4;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&total_props.to_le_bytes());
    reply[8..12].copy_from_slice(&num_configs.to_le_bytes());
    reply[12..16].copy_from_slice(&props_per_config.to_le_bytes());

    // Visual config properties — positional format per Mesa's GetVisualConfigs parser.
    // First 18 properties are the standard ones. Order must match Mesa exactly:
    // [0]=visual_id, [1]=class, [2]=rgba, [3]=red, [4]=green, [5]=blue, [6]=alpha,
    // [7..10]=accum RGBA, [11]=doublebuf, [12]=stereo, [13]=bufsize, [14]=depth,
    // [15]=stencil, [16]=aux, [17]=level
    const X_VISUAL_CLASS_TRUE_COLOR: u32 = 4;
    let props: [u32; 18] = [
        ROOT_VISUAL,               // [0] visual id
        X_VISUAL_CLASS_TRUE_COLOR, // [1] class (TrueColor = 4)
        1,                         // [2] rgba (True)
        8,           // [3] red size
        8,           // [4] green size
        8,           // [5] blue size
        0,           // [6] alpha size
        0,           // [7] accum red
        0,           // [8] accum green
        0,           // [9] accum blue
        0,           // [10] accum alpha
        1,           // [11] double buffer
        0,           // [12] stereo
        24,          // [13] buffer size (R+G+B+A = 8+8+8+0 = 24)
        24,          // [14] depth size
        8,           // [15] stencil size
        0,           // [16] aux buffers
        0,           // [17] level
    ];
    for (i, &v) in props.iter().enumerate() {
        let off = 32 + i * 4;
        reply[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    reply
}

// ---------------------------------------------------------------------------
// GLX_GET_FB_CONFIGS (minor 21)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_fb_configs(_data: &[u8], seq: u16) -> Vec<u8> {
    let num_configs: u32 = 2;
    // numAttribs in the GLX wire protocol is the number of attribute *pairs*
    // (key-value), NOT the number of u32 words.  Each attribute occupies
    // 2 u32s on the wire.  Mesa's __glXInitializeVisualConfigFromTags reads
    // exactly numAttribs key-value pairs, so getting this wrong makes the
    // client read past the end of the reply data.
    let num_attribs = FBCONFIG_ATTRIB_COUNT as u32;
    let u32s_per_config = num_attribs * 2;

    let total_u32s = num_configs * u32s_per_config;
    let extra_bytes = total_u32s as usize * 4;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&total_u32s.to_le_bytes());
    reply[8..12].copy_from_slice(&num_configs.to_le_bytes());
    reply[12..16].copy_from_slice(&num_attribs.to_le_bytes());

    // FBConfig 1: 24-bit XRGB (no alpha)
    // GLX_BUFFER_SIZE must equal R+G+B+A = 8+8+8+0 = 24.
    // Mesa's driConvertConfigs matches DRI configs against server configs by
    // comparing rgbBits; setting 32 here causes "No matching fbConfigs" because
    // swrast's 24-bit RGB DRI config has rgbBits=24, not 32.
    let config1: [(u32, u32); FBCONFIG_ATTRIB_COUNT] = [
        (GLX_FBCONFIG_ID, 1),
        (GLX_VISUAL_ID, ROOT_VISUAL),
        (GLX_X_RENDERABLE, 1),
        (GLX_RENDER_TYPE, GLX_RGBA_BIT),
        (
            GLX_DRAWABLE_TYPE,
            GLX_WINDOW_BIT | GLX_PIXMAP_BIT | GLX_PBUFFER_BIT,
        ),
        (GLX_X_VISUAL_TYPE, GLX_TRUE_COLOR),
        (GLX_CONFIG_CAVEAT, GLX_NONE),
        (GLX_RED_SIZE, 8),
        (GLX_GREEN_SIZE, 8),
        (GLX_BLUE_SIZE, 8),
        (GLX_ALPHA_SIZE, 0),
        (GLX_BUFFER_SIZE, 24),
        (GLX_DOUBLEBUFFER, 1),
        (GLX_DEPTH_SIZE, 24),
        (GLX_STENCIL_SIZE, 8),
        (GLX_LEVEL, 0),
        (GLX_AUX_BUFFERS, 0),
        (GLX_STEREO, 0),
        (GLX_ACCUM_RED_SIZE, 0),
        (GLX_ACCUM_GREEN_SIZE, 0),
        (GLX_ACCUM_BLUE_SIZE, 0),
        (GLX_ACCUM_ALPHA_SIZE, 0),
        (GLX_SAMPLE_BUFFERS, 0),
        (GLX_SAMPLES, 0),
        (GLX_TRANSPARENT_TYPE, GLX_NONE),
        (GLX_MAX_PBUFFER_WIDTH, 4096),
        (GLX_MAX_PBUFFER_HEIGHT, 4096),
        (GLX_MAX_PBUFFER_PIXELS, 4096 * 4096),
    ];

    // FBConfig 2: 32-bit ARGB
    let config2: [(u32, u32); FBCONFIG_ATTRIB_COUNT] = [
        (GLX_FBCONFIG_ID, 2),
        (GLX_VISUAL_ID, 0x40), // ARGB visual
        (GLX_X_RENDERABLE, 1),
        (GLX_RENDER_TYPE, GLX_RGBA_BIT),
        (
            GLX_DRAWABLE_TYPE,
            GLX_WINDOW_BIT | GLX_PIXMAP_BIT | GLX_PBUFFER_BIT,
        ),
        (GLX_X_VISUAL_TYPE, GLX_TRUE_COLOR),
        (GLX_CONFIG_CAVEAT, GLX_NONE),
        (GLX_RED_SIZE, 8),
        (GLX_GREEN_SIZE, 8),
        (GLX_BLUE_SIZE, 8),
        (GLX_ALPHA_SIZE, 8),
        (GLX_BUFFER_SIZE, 32),
        (GLX_DOUBLEBUFFER, 1),
        (GLX_DEPTH_SIZE, 24),
        (GLX_STENCIL_SIZE, 8),
        (GLX_LEVEL, 0),
        (GLX_AUX_BUFFERS, 0),
        (GLX_STEREO, 0),
        (GLX_ACCUM_RED_SIZE, 0),
        (GLX_ACCUM_GREEN_SIZE, 0),
        (GLX_ACCUM_BLUE_SIZE, 0),
        (GLX_ACCUM_ALPHA_SIZE, 0),
        (GLX_SAMPLE_BUFFERS, 0),
        (GLX_SAMPLES, 0),
        (GLX_TRANSPARENT_TYPE, GLX_NONE),
        (GLX_MAX_PBUFFER_WIDTH, 4096),
        (GLX_MAX_PBUFFER_HEIGHT, 4096),
        (GLX_MAX_PBUFFER_PIXELS, 4096 * 4096),
    ];

    let mut off = 32;
    for &(key, val) in config1.iter().chain(config2.iter()) {
        reply[off..off + 4].copy_from_slice(&key.to_le_bytes());
        off += 4;
        reply[off..off + 4].copy_from_slice(&val.to_le_bytes());
        off += 4;
    }

    reply
}

// ---------------------------------------------------------------------------
// GLX_QUERY_EXTENSIONS_STRING (minor 18)
// ---------------------------------------------------------------------------

/// Build a GLX string reply (QueryExtensionsString or QueryServerString).
/// Both use the same wire layout: n at [12..16], string data at [32..].
/// Xorg includes the null terminator in the string data and count.
fn build_glx_string_reply(seq: u16, string: &[u8]) -> Vec<u8> {
    // Include null terminator — Mesa's __glXQueryServerString allocates
    // exactly `n` bytes and does NOT null-terminate, so n must include '\0'.
    let n = (string.len() + 1) as u32; // +1 for null terminator
    let padded = ((n as usize) + 3) & !3;
    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    reply[12..16].copy_from_slice(&n.to_le_bytes());
    if !string.is_empty() {
        reply[32..32 + string.len()].copy_from_slice(string);
    }
    // Null terminator at reply[32 + string.len()] is already 0 from vec![0u8; ...]
    reply
}

pub(crate) fn handle_query_extensions_string(_data: &[u8], seq: u16) -> Vec<u8> {
    let ext_string = b"GLX_EXT_visual_info GLX_EXT_visual_rating GLX_MESA_copy_sub_buffer";
    build_glx_string_reply(seq, ext_string)
}

// ---------------------------------------------------------------------------
// GLX_QUERY_SERVER_STRING (minor 19)
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_server_string(data: &[u8], seq: u16) -> Vec<u8> {
    let name = if data.len() >= 12 {
        u32::from_le_bytes([data[8], data[9], data[10], data[11]])
    } else {
        0
    };

    let string = match name {
        1 => b"x11-web OSMesa" as &[u8],  // GLX_VENDOR
        2 => b"1.4" as &[u8],              // GLX_VERSION
        3 => b"GLX_EXT_visual_info GLX_EXT_visual_rating GLX_MESA_copy_sub_buffer" as &[u8],  // GLX_EXTENSIONS
        _ => b"" as &[u8],
    };

    build_glx_string_reply(seq, string)
}
