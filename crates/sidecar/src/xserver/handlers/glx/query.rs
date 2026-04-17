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
    let props_per_config: u32 = 28;
    let total_props = num_configs * props_per_config;

    let extra_bytes = total_props as usize * 4;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&total_props.to_le_bytes());
    reply[8..12].copy_from_slice(&num_configs.to_le_bytes());
    reply[12..16].copy_from_slice(&props_per_config.to_le_bytes());

    // Visual config properties: one RGBA config, depth=24, stencil=8
    // Property order matches Mesa's positional GetVisualConfigs parser:
    // [0]=visual_id, [1]=class, [2]=rgba, [3..6]=RGBA sizes, [7..10]=accum sizes,
    // [11]=doublebuf, [12]=stereo, [13]=bufsize, [14]=depth, [15]=stencil,
    // [16]=aux, [17]=level, [18..]=extended
    const X_VISUAL_CLASS_TRUE_COLOR: u32 = 4;
    let props: [u32; 28] = [
        ROOT_VISUAL,               // visual id
        X_VISUAL_CLASS_TRUE_COLOR, // class (TrueColor = 4)
        1,                         // rgba (True)
        8,           // red size
        8,           // green size
        8,           // blue size
        0,           // alpha size
        0,           // accum red
        0,           // accum green
        0,           // accum blue
        0,           // accum alpha
        1,           // double buffer
        0,           // stereo
        32,          // buffer size
        24,          // depth size
        8,           // stencil size
        0,           // aux buffers
        0,           // level
        0, // visual caveat (GLX_NONE)
        0, // transparent type (0 = none, 0x8008 = RGB, 0x8009 = Index)
        0, // transparent red
        0, // transparent green
        0, // transparent blue
        0, // transparent alpha
        0, // pad
        0, // pad
        0, // pad
        0, // pad
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

    // FBConfig 1: 24-bit XRGB
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

pub(crate) fn handle_query_extensions_string(_data: &[u8], seq: u16) -> Vec<u8> {
    // Do NOT advertise GLX_ARB_create_context — that extension requires a working
    // DRI stack (no /dev/dri in containers).  Without it, clients like Firefox fall
    // back to the older glXCreateContext() which supports indirect rendering through
    // our GLX protocol handlers.
    let ext_string = b"GLX_EXT_visual_info GLX_EXT_visual_rating GLX_MESA_copy_sub_buffer";
    let n = ext_string.len() as u32;
    let padded = ((n as usize) + 3) & !3;

    // GLX QueryExtensionsString reply (xGLXQueryExtensionsStringReply):
    //   [0]=Reply [2..4]=seq [4..8]=reply_length(padded/4)
    //   [8..12]=n (string byte count)  [32..32+n]=string data
    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&n.to_le_bytes());
    reply[32..32 + n as usize].copy_from_slice(ext_string);

    reply
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

    let n = string.len() as u32;
    let padded = ((n as usize) + 3) & !3;

    // xGLXQueryServerStringReply wire layout (DIFFERENT from QueryExtensionsString!):
    //   [0]=Reply [2..4]=seq [4..8]=reply_length(padded/4)
    //   [8..12]=pad2 (zero)  [12..16]=n (string byte count)  [32..32+padded]=string data
    // Note: xGLXQueryExtensionsStringReply has n at [8..12] (no pad2 field).
    //       xGLXQueryServerStringReply has an extra pad2 word, pushing n to [12..16].
    //       Mesa reads reply.n via the struct field at [12..16]; if we put n at [8..12]
    //       Mesa sees pad2=0, reads 0 bytes, and our padded data stays buffered causing
    //       the xcb_xlib_extra_reply_data_left assertion on the next _XReply call.
    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    // [8..12] = pad2, leave as zero
    reply[12..16].copy_from_slice(&n.to_le_bytes());  // n at correct offset
    if !string.is_empty() {
        reply[32..32 + n as usize].copy_from_slice(string);
    }

    reply
}
