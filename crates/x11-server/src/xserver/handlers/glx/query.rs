//! GLX query operations (QueryVersion, GetVisualConfigs, GetFBConfigs,
//! QueryExtensionsString, QueryServerString).

use super::super::super::core::{ROOT_VISUAL, VISUAL_TRUE_COLOR_ARGB_32};
use super::{FBCONFIG_ARGB, FBCONFIG_RGB};
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
use crate::xserver::reply::serialize_var_reply;
use x11rb_protocol::protocol::glx::{GetFBConfigsReply, GetVisualConfigsReply};
use x11rb_protocol::x11_utils::ByteOrder;

// ---------------------------------------------------------------------------
// GLX_QUERY_VERSION (minor 7)
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_version(seq: u16) -> Vec<u8> {
    super::reply::query_version_reply(seq, 1, 4) // GLX 1.4
}

// ---------------------------------------------------------------------------
// GLX_GET_VISUAL_CONFIGS (minor 14)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_visual_configs(_data: &[u8], seq: u16) -> Vec<u8> {
    // Visual config properties — positional format per Mesa's GetVisualConfigs parser.
    // First 18 properties are the standard ones. Order must match Mesa exactly:
    // [0]=visual_id, [1]=class, [2]=rgba, [3]=red, [4]=green, [5]=blue, [6]=alpha,
    // [7..10]=accum RGBA, [11]=doublebuf, [12]=stereo, [13]=bufsize, [14]=depth,
    // [15]=stencil, [16]=aux, [17]=level
    const X_VISUAL_CLASS_TRUE_COLOR: u32 = 4;
    let property_list: Vec<u32> = vec![
        ROOT_VISUAL,
        X_VISUAL_CLASS_TRUE_COLOR,
        1,
        8,
        8,
        8,
        0,
        0,
        0,
        0,
        0,
        1,
        0,
        24,
        24,
        8,
        0,
        0,
    ];

    serialize_var_reply(
        &GetVisualConfigsReply {
            sequence: seq,
            num_visuals: 1,
            num_properties: 18,
            property_list,
        },
        ByteOrder::Lsb,
    )
}

// ---------------------------------------------------------------------------
// GLX_GET_FB_CONFIGS (minor 21)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_fb_configs(_data: &[u8], seq: u16) -> Vec<u8> {
    // numAttribs in the GLX wire protocol is the number of attribute *pairs*
    // (key-value), NOT the number of u32 words. Each attribute occupies
    // 2 u32s on the wire. Mesa's __glXInitializeVisualConfigFromTags reads
    // exactly numAttribs key-value pairs.
    let num_attribs = FBCONFIG_ATTRIB_COUNT as u32;

    // FBConfig 1: 24-bit XRGB (no alpha)
    // GLX_BUFFER_SIZE must equal R+G+B+A = 8+8+8+0 = 24.
    // Mesa's driConvertConfigs matches DRI configs against server configs by
    // comparing rgbBits; setting 32 here causes "No matching fbConfigs" because
    // swrast's 24-bit RGB DRI config has rgbBits=24, not 32.
    let config1: [(u32, u32); FBCONFIG_ATTRIB_COUNT] = [
        (GLX_FBCONFIG_ID, FBCONFIG_RGB),
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
        (GLX_FBCONFIG_ID, FBCONFIG_ARGB),
        (GLX_VISUAL_ID, VISUAL_TRUE_COLOR_ARGB_32),
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

    let property_list: Vec<u32> = config1
        .iter()
        .chain(config2.iter())
        .flat_map(|&(k, v)| [k, v])
        .collect();

    serialize_var_reply(
        &GetFBConfigsReply {
            sequence: seq,
            num_fb_configs: 2,
            num_properties: num_attribs,
            property_list,
        },
        ByteOrder::Lsb,
    )
}

// ---------------------------------------------------------------------------
// GLX_QUERY_EXTENSIONS_STRING (minor 18)
// ---------------------------------------------------------------------------

/// Build a GLX string reply (QueryExtensionsString or QueryServerString).
/// Both use the same wire layout: n at [12..16], string data at [32..].
/// Xorg includes the null terminator in the string data and count.
pub(crate) fn handle_query_extensions_string(_data: &[u8], seq: u16) -> Vec<u8> {
    let ext_string = b"GLX_EXT_visual_info GLX_EXT_visual_rating GLX_MESA_copy_sub_buffer";
    super::reply::build_glx_string_reply(seq, ext_string)
}

// ---------------------------------------------------------------------------
// GLX_QUERY_SERVER_STRING (minor 19)
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_server_string(data: &[u8], seq: u16) -> Vec<u8> {
    let name = if data.len() >= 12 {
        super::render::read_u32_le(data, 8)
    } else {
        0
    };

    let string = match name {
        1 => b"x11-web OSMesa" as &[u8], // GLX_VENDOR
        2 => b"1.4" as &[u8],            // GLX_VERSION
        3 => b"GLX_EXT_visual_info GLX_EXT_visual_rating GLX_MESA_copy_sub_buffer" as &[u8], // GLX_EXTENSIONS
        _ => b"" as &[u8],
    };

    super::reply::build_glx_string_reply(seq, string)
}
