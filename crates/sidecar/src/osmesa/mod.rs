//! Safe Rust wrappers around OSMesa (Off-Screen Mesa) for software OpenGL rendering.
//!
//! OSMesa provides a software rasteriser that renders into a user-supplied pixel
//! buffer, which we then blit into the X11 window framebuffer.  All FFI symbols
//! are resolved at runtime via `dlopen` / `dlsym` so the build succeeds even
//! when `libOSMesa.so` is absent on the build machine.

use std::ffi::{c_void, CStr, CString};
use std::ptr;
use std::sync::OnceLock;
use tracing::{debug, error, info, warn};

// --------------------------------------------------------------------------
// Raw C types mirroring <GL/osmesa.h>
// --------------------------------------------------------------------------

/// Opaque handle returned by `OSMesaCreateContextExt`.
pub type OSMesaContext = *mut c_void;

// Mesa format constants (kept for completeness of the OSMesa/GL API surface)
#[allow(dead_code)] pub const OSMESA_BGRA: u32 = 0x0002; // GL_BGRA = 0x80E1 but OSMesa uses its own enum
pub const OSMESA_RGBA: u32 = 0x1908;
#[allow(dead_code)] pub const OSMESA_ROW_LENGTH: u32 = 0x10;
pub const OSMESA_Y_UP: u32 = 0x11;

// GL constants we need
#[allow(dead_code)] pub const GL_COLOR_BUFFER_BIT: u32 = 0x00004000;
#[allow(dead_code)] pub const GL_DEPTH_BUFFER_BIT: u32 = 0x00000100;
#[allow(dead_code)] pub const GL_STENCIL_BUFFER_BIT: u32 = 0x00000400;
#[allow(dead_code)] pub const GL_ACCUM_BUFFER_BIT: u32 = 0x00000200;
pub const GL_UNSIGNED_BYTE: u32 = 0x1401;
#[allow(dead_code)] pub const GL_BYTE: u32 = 0x1400;
#[allow(dead_code)] pub const GL_UNSIGNED_SHORT: u32 = 0x1403;
#[allow(dead_code)] pub const GL_SHORT: u32 = 0x1402;
#[allow(dead_code)] pub const GL_UNSIGNED_INT: u32 = 0x1405;
#[allow(dead_code)] pub const GL_INT: u32 = 0x1404;
#[allow(dead_code)] pub const GL_FLOAT: u32 = 0x1406;
#[allow(dead_code)] pub const GL_DOUBLE: u32 = 0x140A;
#[allow(dead_code)] pub const GL_TRUE: u8 = 1;
#[allow(dead_code)] pub const GL_FALSE: u8 = 0;

// GL primitive types
#[allow(dead_code)] pub const GL_POINTS: u32 = 0x0000;
#[allow(dead_code)] pub const GL_LINES: u32 = 0x0001;
#[allow(dead_code)] pub const GL_LINE_LOOP: u32 = 0x0002;
#[allow(dead_code)] pub const GL_LINE_STRIP: u32 = 0x0003;
#[allow(dead_code)] pub const GL_TRIANGLES: u32 = 0x0004;
#[allow(dead_code)] pub const GL_TRIANGLE_STRIP: u32 = 0x0005;
#[allow(dead_code)] pub const GL_TRIANGLE_FAN: u32 = 0x0006;
#[allow(dead_code)] pub const GL_QUADS: u32 = 0x0007;
#[allow(dead_code)] pub const GL_QUAD_STRIP: u32 = 0x0008;
#[allow(dead_code)] pub const GL_POLYGON: u32 = 0x0009;

// GL enable/disable caps
#[allow(dead_code)] pub const GL_TEXTURE_2D: u32 = 0x0DE1;
#[allow(dead_code)] pub const GL_TEXTURE_1D: u32 = 0x0DE0;
#[allow(dead_code)] pub const GL_DEPTH_TEST: u32 = 0x0B71;
#[allow(dead_code)] pub const GL_BLEND: u32 = 0x0BE2;
#[allow(dead_code)] pub const GL_ALPHA_TEST: u32 = 0x0BC0;
#[allow(dead_code)] pub const GL_SCISSOR_TEST: u32 = 0x0C11;
#[allow(dead_code)] pub const GL_STENCIL_TEST: u32 = 0x0B90;
#[allow(dead_code)] pub const GL_CULL_FACE: u32 = 0x0B44;
#[allow(dead_code)] pub const GL_LIGHTING: u32 = 0x0B50;
#[allow(dead_code)] pub const GL_FOG: u32 = 0x0B60;
#[allow(dead_code)] pub const GL_NORMALIZE: u32 = 0x0BA1;
#[allow(dead_code)] pub const GL_COLOR_MATERIAL: u32 = 0x0B57;
#[allow(dead_code)] pub const GL_LINE_SMOOTH: u32 = 0x0B20;
#[allow(dead_code)] pub const GL_POLYGON_SMOOTH: u32 = 0x0B41;
#[allow(dead_code)] pub const GL_MULTISAMPLE: u32 = 0x809D;
#[allow(dead_code)] pub const GL_DITHER: u32 = 0x0BD0;
#[allow(dead_code)] pub const GL_AUTO_NORMAL: u32 = 0x0D80;
#[allow(dead_code)] pub const GL_MAP1_VERTEX_3: u32 = 0x0D97;
#[allow(dead_code)] pub const GL_MAP1_VERTEX_4: u32 = 0x0D98;
#[allow(dead_code)] pub const GL_MAP2_VERTEX_3: u32 = 0x0DB7;
#[allow(dead_code)] pub const GL_MAP2_VERTEX_4: u32 = 0x0DB8;
#[allow(dead_code)] pub const GL_POLYGON_OFFSET_FILL: u32 = 0x8037;
#[allow(dead_code)] pub const GL_POLYGON_OFFSET_LINE: u32 = 0x2A02;
#[allow(dead_code)] pub const GL_POLYGON_OFFSET_POINT: u32 = 0x2A01;
#[allow(dead_code)] pub const GL_POLYGON_STIPPLE: u32 = 0x0B42;
#[allow(dead_code)] pub const GL_TEXTURE_GEN_S: u32 = 0x0C60;
#[allow(dead_code)] pub const GL_TEXTURE_GEN_T: u32 = 0x0C61;
#[allow(dead_code)] pub const GL_TEXTURE_GEN_R: u32 = 0x0C62;
#[allow(dead_code)] pub const GL_TEXTURE_GEN_Q: u32 = 0x0C63;
#[allow(dead_code)] pub const GL_CLIP_PLANE0: u32 = 0x3000;
#[allow(dead_code)] pub const GL_CLIP_PLANE1: u32 = 0x3001;
#[allow(dead_code)] pub const GL_CLIP_PLANE2: u32 = 0x3002;
#[allow(dead_code)] pub const GL_CLIP_PLANE3: u32 = 0x3003;
#[allow(dead_code)] pub const GL_CLIP_PLANE4: u32 = 0x3004;
#[allow(dead_code)] pub const GL_CLIP_PLANE5: u32 = 0x3005;

// Texture formats
#[allow(dead_code)] pub const GL_RGB: u32 = 0x1907;
pub const GL_RGBA: u32 = 0x1908;
#[allow(dead_code)] pub const GL_LUMINANCE: u32 = 0x1909;
#[allow(dead_code)] pub const GL_LUMINANCE_ALPHA: u32 = 0x190A;
#[allow(dead_code)] pub const GL_ALPHA: u32 = 0x1906;
#[allow(dead_code)] pub const GL_BGRA: u32 = 0x80E1;
#[allow(dead_code)] pub const GL_BGR: u32 = 0x80E0;
#[allow(dead_code)] pub const GL_RED: u32 = 0x1903;
#[allow(dead_code)] pub const GL_GREEN: u32 = 0x1904;
#[allow(dead_code)] pub const GL_BLUE: u32 = 0x1905;
#[allow(dead_code)] pub const GL_DEPTH_COMPONENT: u32 = 0x1902;
#[allow(dead_code)] pub const GL_STENCIL_INDEX: u32 = 0x1901;
#[allow(dead_code)] pub const GL_COLOR_INDEX: u32 = 0x1900;

// Texture parameters
#[allow(dead_code)] pub const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
#[allow(dead_code)] pub const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
#[allow(dead_code)] pub const GL_TEXTURE_WRAP_S: u32 = 0x2802;
#[allow(dead_code)] pub const GL_TEXTURE_WRAP_T: u32 = 0x2803;
#[allow(dead_code)] pub const GL_NEAREST: i32 = 0x2600;
#[allow(dead_code)] pub const GL_LINEAR: i32 = 0x2601;
#[allow(dead_code)] pub const GL_NEAREST_MIPMAP_NEAREST: i32 = 0x2700;
#[allow(dead_code)] pub const GL_LINEAR_MIPMAP_NEAREST: i32 = 0x2701;
#[allow(dead_code)] pub const GL_NEAREST_MIPMAP_LINEAR: i32 = 0x2702;
#[allow(dead_code)] pub const GL_LINEAR_MIPMAP_LINEAR: i32 = 0x2703;
#[allow(dead_code)] pub const GL_CLAMP: i32 = 0x2900;
#[allow(dead_code)] pub const GL_REPEAT: i32 = 0x2901;

// Matrix modes
#[allow(dead_code)] pub const GL_MODELVIEW: u32 = 0x1700;
#[allow(dead_code)] pub const GL_PROJECTION: u32 = 0x1701;
#[allow(dead_code)] pub const GL_TEXTURE: u32 = 0x1702;

// Lighting constants
#[allow(dead_code)] pub const GL_LIGHT0: u32 = 0x4000;
#[allow(dead_code)] pub const GL_LIGHT1: u32 = 0x4001;
#[allow(dead_code)] pub const GL_LIGHT2: u32 = 0x4002;
#[allow(dead_code)] pub const GL_LIGHT3: u32 = 0x4003;
#[allow(dead_code)] pub const GL_LIGHT4: u32 = 0x4004;
#[allow(dead_code)] pub const GL_LIGHT5: u32 = 0x4005;
#[allow(dead_code)] pub const GL_LIGHT6: u32 = 0x4006;
#[allow(dead_code)] pub const GL_LIGHT7: u32 = 0x4007;
#[allow(dead_code)] pub const GL_AMBIENT: u32 = 0x1200;
#[allow(dead_code)] pub const GL_DIFFUSE: u32 = 0x1201;
#[allow(dead_code)] pub const GL_SPECULAR: u32 = 0x1202;
#[allow(dead_code)] pub const GL_POSITION: u32 = 0x1203;
#[allow(dead_code)] pub const GL_SPOT_DIRECTION: u32 = 0x1204;
#[allow(dead_code)] pub const GL_SPOT_EXPONENT: u32 = 0x1205;
#[allow(dead_code)] pub const GL_SPOT_CUTOFF: u32 = 0x1206;
#[allow(dead_code)] pub const GL_CONSTANT_ATTENUATION: u32 = 0x1207;
#[allow(dead_code)] pub const GL_LINEAR_ATTENUATION: u32 = 0x1208;
#[allow(dead_code)] pub const GL_QUADRATIC_ATTENUATION: u32 = 0x1209;
#[allow(dead_code)] pub const GL_EMISSION: u32 = 0x1600;
#[allow(dead_code)] pub const GL_SHININESS: u32 = 0x1601;
#[allow(dead_code)] pub const GL_AMBIENT_AND_DIFFUSE: u32 = 0x1602;
#[allow(dead_code)] pub const GL_COLOR_INDEXES: u32 = 0x1603;
#[allow(dead_code)] pub const GL_LIGHT_MODEL_AMBIENT: u32 = 0x0B53;
#[allow(dead_code)] pub const GL_LIGHT_MODEL_LOCAL_VIEWER: u32 = 0x0B51;
#[allow(dead_code)] pub const GL_LIGHT_MODEL_TWO_SIDE: u32 = 0x0B52;
#[allow(dead_code)] pub const GL_FRONT: u32 = 0x0404;
#[allow(dead_code)] pub const GL_BACK: u32 = 0x0405;
#[allow(dead_code)] pub const GL_FRONT_AND_BACK: u32 = 0x0408;

// Display list modes
#[allow(dead_code)] pub const GL_COMPILE: u32 = 0x1300;
#[allow(dead_code)] pub const GL_COMPILE_AND_EXECUTE: u32 = 0x1301;

// Vertex array client state
#[allow(dead_code)] pub const GL_VERTEX_ARRAY: u32 = 0x8074;
#[allow(dead_code)] pub const GL_COLOR_ARRAY: u32 = 0x8076;
#[allow(dead_code)] pub const GL_NORMAL_ARRAY: u32 = 0x8075;
#[allow(dead_code)] pub const GL_TEXTURE_COORD_ARRAY: u32 = 0x8078;
#[allow(dead_code)] pub const GL_INDEX_ARRAY: u32 = 0x8077;
#[allow(dead_code)] pub const GL_EDGE_FLAG_ARRAY: u32 = 0x8079;

// Fog parameters
#[allow(dead_code)] pub const GL_FOG_MODE: u32 = 0x0B65;
#[allow(dead_code)] pub const GL_FOG_DENSITY: u32 = 0x0B62;
#[allow(dead_code)] pub const GL_FOG_START: u32 = 0x0B63;
#[allow(dead_code)] pub const GL_FOG_END: u32 = 0x0B64;
#[allow(dead_code)] pub const GL_FOG_INDEX: u32 = 0x0B61;
#[allow(dead_code)] pub const GL_FOG_COLOR: u32 = 0x0B66;
#[allow(dead_code)] pub const GL_EXP: u32 = 0x0800;
#[allow(dead_code)] pub const GL_EXP2: u32 = 0x0801;

// Texture environment
#[allow(dead_code)] pub const GL_TEXTURE_ENV: u32 = 0x2300;
#[allow(dead_code)] pub const GL_TEXTURE_ENV_MODE: u32 = 0x2200;
#[allow(dead_code)] pub const GL_TEXTURE_ENV_COLOR: u32 = 0x2201;
#[allow(dead_code)] pub const GL_MODULATE: u32 = 0x2100;
#[allow(dead_code)] pub const GL_DECAL: u32 = 0x2101;
#[allow(dead_code)] pub const GL_REPLACE: u32 = 0x1E01;

// Texture generation
#[allow(dead_code)] pub const GL_S: u32 = 0x2000;
#[allow(dead_code)] pub const GL_T: u32 = 0x2001;
#[allow(dead_code)] pub const GL_R: u32 = 0x2002;
#[allow(dead_code)] pub const GL_Q: u32 = 0x2003;
#[allow(dead_code)] pub const GL_TEXTURE_GEN_MODE: u32 = 0x2500;
#[allow(dead_code)] pub const GL_OBJECT_PLANE: u32 = 0x2501;
#[allow(dead_code)] pub const GL_EYE_PLANE: u32 = 0x2502;
#[allow(dead_code)] pub const GL_OBJECT_LINEAR: u32 = 0x2401;
#[allow(dead_code)] pub const GL_EYE_LINEAR: u32 = 0x2400;
#[allow(dead_code)] pub const GL_SPHERE_MAP: u32 = 0x2402;

// Accumulation ops
#[allow(dead_code)] pub const GL_ACCUM: u32 = 0x0100;
#[allow(dead_code)] pub const GL_LOAD: u32 = 0x0101;
#[allow(dead_code)] pub const GL_RETURN: u32 = 0x0102;
#[allow(dead_code)] pub const GL_MULT: u32 = 0x0103;
#[allow(dead_code)] pub const GL_ADD: u32 = 0x0104;

// Selection/Feedback
#[allow(dead_code)] pub const GL_RENDER: u32 = 0x1C00;
#[allow(dead_code)] pub const GL_SELECT: u32 = 0x1C02;
#[allow(dead_code)] pub const GL_FEEDBACK: u32 = 0x1C01;
#[allow(dead_code)] pub const GL_2D: u32 = 0x0600;
#[allow(dead_code)] pub const GL_3D: u32 = 0x0601;
#[allow(dead_code)] pub const GL_3D_COLOR: u32 = 0x0602;
#[allow(dead_code)] pub const GL_3D_COLOR_TEXTURE: u32 = 0x0603;
#[allow(dead_code)] pub const GL_4D_COLOR_TEXTURE: u32 = 0x0604;
#[allow(dead_code)] pub const GL_PASS_THROUGH_TOKEN: f32 = 0x0700 as f32;

// Logic ops
#[allow(dead_code)] pub const GL_CLEAR: u32 = 0x1500;
#[allow(dead_code)] pub const GL_AND: u32 = 0x1501;
#[allow(dead_code)] pub const GL_AND_REVERSE: u32 = 0x1502;
#[allow(dead_code)] pub const GL_COPY: u32 = 0x1503;
#[allow(dead_code)] pub const GL_AND_INVERTED: u32 = 0x1504;
#[allow(dead_code)] pub const GL_NOOP: u32 = 0x1505;
#[allow(dead_code)] pub const GL_XOR: u32 = 0x1506;
#[allow(dead_code)] pub const GL_OR: u32 = 0x1507;
#[allow(dead_code)] pub const GL_NOR: u32 = 0x1508;
#[allow(dead_code)] pub const GL_EQUIV: u32 = 0x1509;
#[allow(dead_code)] pub const GL_INVERT: u32 = 0x150A;
#[allow(dead_code)] pub const GL_OR_REVERSE: u32 = 0x150B;
#[allow(dead_code)] pub const GL_COPY_INVERTED: u32 = 0x150C;
#[allow(dead_code)] pub const GL_OR_INVERTED: u32 = 0x150D;
#[allow(dead_code)] pub const GL_NAND: u32 = 0x150E;
#[allow(dead_code)] pub const GL_SET: u32 = 0x150F;

// Pixel copy types
#[allow(dead_code)] pub const GL_COLOR: u32 = 0x1800;
#[allow(dead_code)] pub const GL_DEPTH: u32 = 0x1801;
#[allow(dead_code)] pub const GL_STENCIL: u32 = 0x1802;

// Pixel storage
#[allow(dead_code)] pub const GL_PACK_ALIGNMENT: u32 = 0x0D05;
#[allow(dead_code)] pub const GL_UNPACK_ALIGNMENT: u32 = 0x0CF5;
#[allow(dead_code)] pub const GL_PACK_ROW_LENGTH: u32 = 0x0D02;
#[allow(dead_code)] pub const GL_UNPACK_ROW_LENGTH: u32 = 0x0CF2;
#[allow(dead_code)] pub const GL_PACK_SKIP_ROWS: u32 = 0x0D03;
#[allow(dead_code)] pub const GL_PACK_SKIP_PIXELS: u32 = 0x0D04;
#[allow(dead_code)] pub const GL_UNPACK_SKIP_ROWS: u32 = 0x0CF3;
#[allow(dead_code)] pub const GL_UNPACK_SKIP_PIXELS: u32 = 0x0CF4;
#[allow(dead_code)] pub const GL_PACK_LSB_FIRST: u32 = 0x0D01;
#[allow(dead_code)] pub const GL_UNPACK_LSB_FIRST: u32 = 0x0CF1;
#[allow(dead_code)] pub const GL_PACK_SWAP_BYTES: u32 = 0x0D00;
#[allow(dead_code)] pub const GL_UNPACK_SWAP_BYTES: u32 = 0x0CF0;

// GetString names
#[allow(dead_code)] pub const GL_VENDOR: u32 = 0x1F00;
#[allow(dead_code)] pub const GL_RENDERER: u32 = 0x1F01;
#[allow(dead_code)] pub const GL_VERSION: u32 = 0x1F02;
#[allow(dead_code)] pub const GL_EXTENSIONS: u32 = 0x1F03;

// GL 1.4 blend equation modes
#[allow(dead_code)] pub const GL_FUNC_ADD: u32 = 0x8006;
#[allow(dead_code)] pub const GL_FUNC_SUBTRACT: u32 = 0x800A;
#[allow(dead_code)] pub const GL_FUNC_REVERSE_SUBTRACT: u32 = 0x800B;
#[allow(dead_code)] pub const GL_MIN: u32 = 0x8007;
#[allow(dead_code)] pub const GL_MAX: u32 = 0x8008;

// GL 1.5 buffer targets
#[allow(dead_code)] pub const GL_ARRAY_BUFFER: u32 = 0x8892;
#[allow(dead_code)] pub const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
#[allow(dead_code)] pub const GL_STATIC_DRAW: u32 = 0x88E4;
#[allow(dead_code)] pub const GL_DYNAMIC_DRAW: u32 = 0x88E8;
#[allow(dead_code)] pub const GL_STREAM_DRAW: u32 = 0x88E0;
#[allow(dead_code)] pub const GL_READ_ONLY: u32 = 0x88B8;
#[allow(dead_code)] pub const GL_WRITE_ONLY: u32 = 0x88B9;
#[allow(dead_code)] pub const GL_READ_WRITE: u32 = 0x88BA;

// GL 2.0 shader types
#[allow(dead_code)] pub const GL_VERTEX_SHADER: u32 = 0x8B31;
#[allow(dead_code)] pub const GL_FRAGMENT_SHADER: u32 = 0x8B30;
#[allow(dead_code)] pub const GL_COMPILE_STATUS: u32 = 0x8B81;
#[allow(dead_code)] pub const GL_LINK_STATUS: u32 = 0x8B82;
#[allow(dead_code)] pub const GL_INFO_LOG_LENGTH: u32 = 0x8B84;

// GL 3.0 FBO constants
#[allow(dead_code)] pub const GL_FRAMEBUFFER: u32 = 0x8D40;
#[allow(dead_code)] pub const GL_RENDERBUFFER: u32 = 0x8D41;
#[allow(dead_code)] pub const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
#[allow(dead_code)] pub const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
#[allow(dead_code)] pub const GL_STENCIL_ATTACHMENT: u32 = 0x8D20;
#[allow(dead_code)] pub const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
#[allow(dead_code)] pub const GL_DEPTH_COMPONENT16: u32 = 0x81A5;
#[allow(dead_code)] pub const GL_DEPTH_COMPONENT24: u32 = 0x81A6;

// Evaluator map targets (some already above)
#[allow(dead_code)] pub const GL_MAP1_COLOR_4: u32 = 0x0D90;
#[allow(dead_code)] pub const GL_MAP1_INDEX: u32 = 0x0D91;
#[allow(dead_code)] pub const GL_MAP1_NORMAL: u32 = 0x0D92;
#[allow(dead_code)] pub const GL_MAP1_TEXTURE_COORD_1: u32 = 0x0D93;
#[allow(dead_code)] pub const GL_MAP1_TEXTURE_COORD_2: u32 = 0x0D94;
#[allow(dead_code)] pub const GL_MAP1_TEXTURE_COORD_3: u32 = 0x0D95;
#[allow(dead_code)] pub const GL_MAP1_TEXTURE_COORD_4: u32 = 0x0D96;
#[allow(dead_code)] pub const GL_MAP2_COLOR_4: u32 = 0x0DB0;
#[allow(dead_code)] pub const GL_MAP2_INDEX: u32 = 0x0DB1;
#[allow(dead_code)] pub const GL_MAP2_NORMAL: u32 = 0x0DB2;
#[allow(dead_code)] pub const GL_MAP2_TEXTURE_COORD_1: u32 = 0x0DB3;
#[allow(dead_code)] pub const GL_MAP2_TEXTURE_COORD_2: u32 = 0x0DB4;
#[allow(dead_code)] pub const GL_MAP2_TEXTURE_COORD_3: u32 = 0x0DB5;
#[allow(dead_code)] pub const GL_MAP2_TEXTURE_COORD_4: u32 = 0x0DB6;

// Hint targets
#[allow(dead_code)] pub const GL_PERSPECTIVE_CORRECTION_HINT: u32 = 0x0C50;
#[allow(dead_code)] pub const GL_POINT_SMOOTH_HINT: u32 = 0x0C51;
#[allow(dead_code)] pub const GL_LINE_SMOOTH_HINT: u32 = 0x0C52;
#[allow(dead_code)] pub const GL_POLYGON_SMOOTH_HINT: u32 = 0x0C53;
#[allow(dead_code)] pub const GL_FOG_HINT: u32 = 0x0C54;
#[allow(dead_code)] pub const GL_DONT_CARE: u32 = 0x1100;
#[allow(dead_code)] pub const GL_FASTEST: u32 = 0x1101;
#[allow(dead_code)] pub const GL_NICEST: u32 = 0x1102;

// GL 3.0 texture types
#[allow(dead_code)] pub const GL_TEXTURE_3D: u32 = 0x806F;

// --------------------------------------------------------------------------
// Dynamic function pointer table
// --------------------------------------------------------------------------

macro_rules! ffi_fn {
    ($name:ident, ($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        type $name = unsafe extern "C" fn($($arg: $ty),*) -> $ret;
    };
    ($name:ident, ($($arg:ident: $ty:ty),*)) => {
        type $name = unsafe extern "C" fn($($arg: $ty),*);
    };
}

// OSMesa functions
ffi_fn!(FnOSMesaCreateContextExt, (format: u32, depth_bits: i32, stencil_bits: i32, accum_bits: i32, share_list: OSMesaContext) -> OSMesaContext);
ffi_fn!(FnOSMesaDestroyContext, (ctx: OSMesaContext));
ffi_fn!(FnOSMesaMakeCurrent, (ctx: OSMesaContext, buffer: *mut c_void, type_: u32, width: i32, height: i32) -> u8);
ffi_fn!(FnOSMesaGetProcAddress, (func_name: *const i8) -> *const c_void);
ffi_fn!(FnOSMesaPixelStore, (pname: u32, value: i32));

// GL 1.0-1.1 functions (always available in OSMesa)
ffi_fn!(FnGlClear, (mask: u32));
ffi_fn!(FnGlClearColor, (r: f32, g: f32, b: f32, a: f32));
ffi_fn!(FnGlViewport, (x: i32, y: i32, width: i32, height: i32));
ffi_fn!(FnGlBegin, (mode: u32));
ffi_fn!(FnGlEnd, ());
ffi_fn!(FnGlVertex2f, (x: f32, y: f32));
ffi_fn!(FnGlVertex3f, (x: f32, y: f32, z: f32));
ffi_fn!(FnGlVertex4f, (x: f32, y: f32, z: f32, w: f32));
ffi_fn!(FnGlColor3f, (r: f32, g: f32, b: f32));
ffi_fn!(FnGlColor4f, (r: f32, g: f32, b: f32, a: f32));
ffi_fn!(FnGlColor3ub, (r: u8, g: u8, b: u8));
ffi_fn!(FnGlColor4ub, (r: u8, g: u8, b: u8, a: u8));
ffi_fn!(FnGlFlush, ());
ffi_fn!(FnGlFinish, ());
ffi_fn!(FnGlEnable, (cap: u32));
ffi_fn!(FnGlDisable, (cap: u32));
ffi_fn!(FnGlGenTextures, (n: i32, textures: *mut u32));
ffi_fn!(FnGlDeleteTextures, (n: i32, textures: *const u32));
ffi_fn!(FnGlBindTexture, (target: u32, texture: u32));
ffi_fn!(FnGlTexImage2D, (target: u32, level: i32, internal_format: i32, width: i32, height: i32, border: i32, format: u32, type_: u32, data: *const c_void));
ffi_fn!(FnGlTexParameteri, (target: u32, pname: u32, param: i32));
ffi_fn!(FnGlTexSubImage2D, (target: u32, level: i32, xoffset: i32, yoffset: i32, width: i32, height: i32, format: u32, type_: u32, data: *const c_void));
ffi_fn!(FnGlReadPixels, (x: i32, y: i32, width: i32, height: i32, format: u32, type_: u32, pixels: *mut c_void));
ffi_fn!(FnGlScissor, (x: i32, y: i32, width: i32, height: i32));
ffi_fn!(FnGlBlendFunc, (sfactor: u32, dfactor: u32));
ffi_fn!(FnGlDepthFunc, (func: u32));
ffi_fn!(FnGlDepthMask, (flag: u8));
ffi_fn!(FnGlColorMask, (r: u8, g: u8, b: u8, a: u8));
ffi_fn!(FnGlStencilFunc, (func: u32, ref_: i32, mask: u32));
ffi_fn!(FnGlStencilOp, (fail: u32, zfail: u32, zpass: u32));
ffi_fn!(FnGlStencilMask, (mask: u32));
ffi_fn!(FnGlMatrixMode, (mode: u32));
ffi_fn!(FnGlLoadIdentity, ());
ffi_fn!(FnGlLoadMatrixf, (m: *const f32));
ffi_fn!(FnGlLoadMatrixd, (m: *const f64));
ffi_fn!(FnGlMultMatrixf, (m: *const f32));
ffi_fn!(FnGlMultMatrixd, (m: *const f64));
ffi_fn!(FnGlPushMatrix, ());
ffi_fn!(FnGlPopMatrix, ());
ffi_fn!(FnGlOrtho, (left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64));
ffi_fn!(FnGlFrustum, (left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64));
ffi_fn!(FnGlRotatef, (angle: f32, x: f32, y: f32, z: f32));
ffi_fn!(FnGlScalef, (x: f32, y: f32, z: f32));
ffi_fn!(FnGlTranslatef, (x: f32, y: f32, z: f32));
ffi_fn!(FnGlNormal3f, (nx: f32, ny: f32, nz: f32));
ffi_fn!(FnGlTexCoord2f, (s: f32, t: f32));
ffi_fn!(FnGlTexCoord4f, (s: f32, t: f32, r: f32, q: f32));
ffi_fn!(FnGlPixelStorei, (pname: u32, param: i32));
ffi_fn!(FnGlLineWidth, (width: f32));
ffi_fn!(FnGlPointSize, (size: f32));
ffi_fn!(FnGlPolygonMode, (face: u32, mode: u32));
ffi_fn!(FnGlCullFace, (mode: u32));
ffi_fn!(FnGlFrontFace, (mode: u32));
ffi_fn!(FnGlShadeModel, (mode: u32));
ffi_fn!(FnGlClearDepth, (depth: f64));
ffi_fn!(FnGlClearStencil, (s: i32));
ffi_fn!(FnGlAlphaFunc, (func: u32, ref_: f32));
ffi_fn!(FnGlHint, (target: u32, mode: u32));
ffi_fn!(FnGlGetIntegerv, (pname: u32, params: *mut i32));
ffi_fn!(FnGlGetFloatv, (pname: u32, params: *mut f32));
ffi_fn!(FnGlGetError, () -> u32);
ffi_fn!(FnGlGetString, (name: u32) -> *const u8);
ffi_fn!(FnGlVertex2i, (x: i32, y: i32));
ffi_fn!(FnGlVertex3i, (x: i32, y: i32, z: i32));
ffi_fn!(FnGlRectf, (x1: f32, y1: f32, x2: f32, y2: f32));
ffi_fn!(FnGlRecti, (x1: i32, y1: i32, x2: i32, y2: i32));

// Display Lists
ffi_fn!(FnGlNewList, (list: u32, mode: u32));
ffi_fn!(FnGlEndList, ());
ffi_fn!(FnGlGenLists, (range: i32) -> u32);
ffi_fn!(FnGlDeleteLists, (list: u32, range: i32));
ffi_fn!(FnGlIsList, (list: u32) -> u8);
ffi_fn!(FnGlCallList, (list: u32));
ffi_fn!(FnGlCallLists, (n: i32, list_type: u32, lists: *const u8));
ffi_fn!(FnGlListBase, (base: u32));

// Lighting
ffi_fn!(FnGlLightf, (light: u32, pname: u32, param: f32));
ffi_fn!(FnGlLightfv, (light: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlLighti, (light: u32, pname: u32, param: i32));
ffi_fn!(FnGlLightiv, (light: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlLightModelf, (pname: u32, param: f32));
ffi_fn!(FnGlLightModelfv, (pname: u32, params: *const f32));
ffi_fn!(FnGlLightModeli, (pname: u32, param: i32));
ffi_fn!(FnGlLightModeliv, (pname: u32, params: *const i32));
ffi_fn!(FnGlMaterialf, (face: u32, pname: u32, param: f32));
ffi_fn!(FnGlMaterialfv, (face: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlMateriali, (face: u32, pname: u32, param: i32));
ffi_fn!(FnGlMaterialiv, (face: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlColorMaterial, (face: u32, mode: u32));

// Fog
ffi_fn!(FnGlFogf, (pname: u32, param: f32));
ffi_fn!(FnGlFogfv, (pname: u32, params: *const f32));
ffi_fn!(FnGlFogi, (pname: u32, param: i32));
ffi_fn!(FnGlFogiv, (pname: u32, params: *const i32));

// Polygon/Drawing
ffi_fn!(FnGlPolygonOffset, (factor: f32, units: f32));
ffi_fn!(FnGlPolygonStipple, (mask: *const u8));
ffi_fn!(FnGlGetPolygonStipple, (mask: *mut u8));
ffi_fn!(FnGlLogicOp, (opcode: u32));
ffi_fn!(FnGlDrawPixels, (width: i32, height: i32, format: u32, type_: u32, pixels: *const c_void));
ffi_fn!(FnGlCopyPixels, (x: i32, y: i32, width: i32, height: i32, type_: u32));
ffi_fn!(FnGlBitmap, (width: i32, height: i32, xorig: f32, yorig: f32, xmove: f32, ymove: f32, bitmap: *const u8));
ffi_fn!(FnGlPixelZoom, (xfactor: f32, yfactor: f32));
ffi_fn!(FnGlRasterPos2f, (x: f32, y: f32));
ffi_fn!(FnGlRasterPos3f, (x: f32, y: f32, z: f32));
ffi_fn!(FnGlRasterPos4f, (x: f32, y: f32, z: f32, w: f32));
ffi_fn!(FnGlRasterPos2i, (x: i32, y: i32));
ffi_fn!(FnGlRasterPos3i, (x: i32, y: i32, z: i32));
ffi_fn!(FnGlRasterPos4i, (x: i32, y: i32, z: i32, w: i32));

// Depth/Blending (core 1.0)
ffi_fn!(FnGlDepthRange, (near: f64, far: f64));

// Texture environment/generation
ffi_fn!(FnGlTexEnvf, (target: u32, pname: u32, param: f32));
ffi_fn!(FnGlTexEnvfv, (target: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlTexEnvi, (target: u32, pname: u32, param: i32));
ffi_fn!(FnGlTexEnviv, (target: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlTexGeni, (coord: u32, pname: u32, param: i32));
ffi_fn!(FnGlTexGenf, (coord: u32, pname: u32, param: f32));
ffi_fn!(FnGlTexGend, (coord: u32, pname: u32, param: f64));
ffi_fn!(FnGlTexGeniv, (coord: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlTexGenfv, (coord: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlTexGendv, (coord: u32, pname: u32, params: *const f64));
ffi_fn!(FnGlTexImage1D, (target: u32, level: i32, internal_format: i32, width: i32, border: i32, format: u32, type_: u32, data: *const c_void));
ffi_fn!(FnGlCopyTexImage2D, (target: u32, level: i32, internal_format: u32, x: i32, y: i32, width: i32, height: i32, border: i32));
ffi_fn!(FnGlCopyTexSubImage2D, (target: u32, level: i32, xoffset: i32, yoffset: i32, x: i32, y: i32, width: i32, height: i32));
ffi_fn!(FnGlTexParameterf, (target: u32, pname: u32, param: f32));
ffi_fn!(FnGlTexParameterfv, (target: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlTexParameteriv, (target: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlPixelStoref, (pname: u32, param: f32));
ffi_fn!(FnGlPixelTransferf, (pname: u32, param: f32));
ffi_fn!(FnGlPixelTransferi, (pname: u32, param: i32));

// Vertex Arrays (GL 1.1)
ffi_fn!(FnGlDrawArrays, (mode: u32, first: i32, count: i32));
ffi_fn!(FnGlDrawElements, (mode: u32, count: i32, type_: u32, indices: *const c_void));
ffi_fn!(FnGlVertexPointer, (size: i32, type_: u32, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlColorPointer, (size: i32, type_: u32, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlNormalPointer, (type_: u32, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlTexCoordPointer, (size: i32, type_: u32, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlEnableClientState, (array: u32));
ffi_fn!(FnGlDisableClientState, (array: u32));
ffi_fn!(FnGlInterleavedArrays, (format: u32, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlArrayElement, (i: i32));

// State queries
ffi_fn!(FnGlGetBooleanv, (pname: u32, params: *mut u8));
ffi_fn!(FnGlGetDoublev, (pname: u32, params: *mut f64));
ffi_fn!(FnGlIsEnabled, (cap: u32) -> u8);
ffi_fn!(FnGlGetTexParameteriv, (target: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetTexParameterfv, (target: u32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetTexLevelParameteriv, (target: u32, level: i32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetTexLevelParameterfv, (target: u32, level: i32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetTexImage, (target: u32, level: i32, format: u32, type_: u32, pixels: *mut c_void));
ffi_fn!(FnGlGetLightfv, (light: u32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetLightiv, (light: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetMaterialfv, (face: u32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetMaterialiv, (face: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlIsTexture, (texture: u32) -> u8);
ffi_fn!(FnGlAreTexturesResident, (n: i32, textures: *const u32, residences: *mut u8) -> u8);
ffi_fn!(FnGlGetTexEnvfv, (target: u32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetTexEnviv, (target: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetTexGendv, (coord: u32, pname: u32, params: *mut f64));
ffi_fn!(FnGlGetTexGenfv, (coord: u32, pname: u32, params: *mut f32));
ffi_fn!(FnGlGetTexGeniv, (coord: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetPixelMapfv, (map: u32, values: *mut f32));
ffi_fn!(FnGlGetPixelMapuiv, (map: u32, values: *mut u32));
ffi_fn!(FnGlGetPixelMapusv, (map: u32, values: *mut u16));
ffi_fn!(FnGlGetMapdv, (target: u32, query: u32, v: *mut f64));
ffi_fn!(FnGlGetMapfv, (target: u32, query: u32, v: *mut f32));
ffi_fn!(FnGlGetMapiv, (target: u32, query: u32, v: *mut i32));
ffi_fn!(FnGlGetClipPlane, (plane: u32, equation: *mut f64));
ffi_fn!(FnGlClipPlane, (plane: u32, equation: *const f64));

// Evaluators
ffi_fn!(FnGlMap1f, (target: u32, u1: f32, u2: f32, stride: i32, order: i32, points: *const f32));
ffi_fn!(FnGlMap1d, (target: u32, u1: f64, u2: f64, stride: i32, order: i32, points: *const f64));
ffi_fn!(FnGlMap2f, (target: u32, u1: f32, u2: f32, ustride: i32, uorder: i32, v1: f32, v2: f32, vstride: i32, vorder: i32, points: *const f32));
ffi_fn!(FnGlMap2d, (target: u32, u1: f64, u2: f64, ustride: i32, uorder: i32, v1: f64, v2: f64, vstride: i32, vorder: i32, points: *const f64));
ffi_fn!(FnGlEvalCoord1f, (u: f32));
ffi_fn!(FnGlEvalCoord1d, (u: f64));
ffi_fn!(FnGlEvalCoord2f, (u: f32, v: f32));
ffi_fn!(FnGlEvalCoord2d, (u: f64, v: f64));
ffi_fn!(FnGlMapGrid1f, (un: i32, u1: f32, u2: f32));
ffi_fn!(FnGlMapGrid1d, (un: i32, u1: f64, u2: f64));
ffi_fn!(FnGlMapGrid2f, (un: i32, u1: f32, u2: f32, vn: i32, v1: f32, v2: f32));
ffi_fn!(FnGlMapGrid2d, (un: i32, u1: f64, u2: f64, vn: i32, v1: f64, v2: f64));
ffi_fn!(FnGlEvalMesh1, (mode: u32, i1: i32, i2: i32));
ffi_fn!(FnGlEvalMesh2, (mode: u32, i1: i32, i2: i32, j1: i32, j2: i32));
ffi_fn!(FnGlEvalPoint1, (i: i32));
ffi_fn!(FnGlEvalPoint2, (i: i32, j: i32));

// Accumulation
ffi_fn!(FnGlAccum, (op: u32, value: f32));
ffi_fn!(FnGlClearAccum, (r: f32, g: f32, b: f32, a: f32));

// Selection/Feedback
ffi_fn!(FnGlRenderMode, (mode: u32) -> i32);
ffi_fn!(FnGlInitNames, ());
ffi_fn!(FnGlPushName, (name: u32));
ffi_fn!(FnGlPopName, ());
ffi_fn!(FnGlLoadName, (name: u32));
ffi_fn!(FnGlSelectBuffer, (size: i32, buffer: *mut u32));
ffi_fn!(FnGlFeedbackBuffer, (size: i32, type_: u32, buffer: *mut f32));
ffi_fn!(FnGlPassThrough, (token: f32));
ffi_fn!(FnGlPushAttrib, (mask: u32));
ffi_fn!(FnGlPopAttrib, ());
ffi_fn!(FnGlPixelMapfv, (map: u32, mapsize: i32, values: *const f32));
ffi_fn!(FnGlPixelMapuiv, (map: u32, mapsize: i32, values: *const u32));
ffi_fn!(FnGlPixelMapusv, (map: u32, mapsize: i32, values: *const u16));

// Additional GL 1.0-1.1 variants used by GLX handler
ffi_fn!(FnGlColor3b, (r: i8, g: i8, b: i8));
ffi_fn!(FnGlColor3d, (r: f64, g: f64, b: f64));
ffi_fn!(FnGlColor3i, (r: i32, g: i32, b: i32));
ffi_fn!(FnGlColor3s, (r: i16, g: i16, b: i16));
ffi_fn!(FnGlColor3ui, (r: u32, g: u32, b: u32));
ffi_fn!(FnGlColor3us, (r: u16, g: u16, b: u16));
ffi_fn!(FnGlColor4b, (r: i8, g: i8, b: i8, a: i8));
ffi_fn!(FnGlColor4d, (r: f64, g: f64, b: f64, a: f64));
ffi_fn!(FnGlColor4i, (r: i32, g: i32, b: i32, a: i32));
ffi_fn!(FnGlColor4s, (r: i16, g: i16, b: i16, a: i16));
ffi_fn!(FnGlColor4ui, (r: u32, g: u32, b: u32, a: u32));
ffi_fn!(FnGlColor4us, (r: u16, g: u16, b: u16, a: u16));
ffi_fn!(FnGlEdgeFlag, (flag: u8));
ffi_fn!(FnGlIndexd, (c: f64));
ffi_fn!(FnGlIndexf, (c: f32));
ffi_fn!(FnGlIndexi, (c: i32));
ffi_fn!(FnGlIndexs, (c: i16));
ffi_fn!(FnGlIndexub, (c: u8));
ffi_fn!(FnGlIndexMask, (mask: u32));
ffi_fn!(FnGlClearIndex, (c: f32));
ffi_fn!(FnGlNormal3b, (nx: i8, ny: i8, nz: i8));
ffi_fn!(FnGlNormal3d, (nx: f64, ny: f64, nz: f64));
ffi_fn!(FnGlNormal3i, (nx: i32, ny: i32, nz: i32));
ffi_fn!(FnGlNormal3s, (nx: i16, ny: i16, nz: i16));
ffi_fn!(FnGlVertex2d, (x: f64, y: f64));
ffi_fn!(FnGlVertex2s, (x: i16, y: i16));
ffi_fn!(FnGlVertex3d, (x: f64, y: f64, z: f64));
ffi_fn!(FnGlVertex3s, (x: i16, y: i16, z: i16));
ffi_fn!(FnGlVertex4d, (x: f64, y: f64, z: f64, w: f64));
ffi_fn!(FnGlVertex4i, (x: i32, y: i32, z: i32, w: i32));
ffi_fn!(FnGlVertex4s, (x: i16, y: i16, z: i16, w: i16));
ffi_fn!(FnGlTexCoord1d, (s: f64));
ffi_fn!(FnGlTexCoord1f, (s: f32));
ffi_fn!(FnGlTexCoord1i, (s: i32));
ffi_fn!(FnGlTexCoord1s, (s: i16));
ffi_fn!(FnGlTexCoord2d, (s: f64, t: f64));
ffi_fn!(FnGlTexCoord2i, (s: i32, t: i32));
ffi_fn!(FnGlTexCoord2s, (s: i16, t: i16));
ffi_fn!(FnGlTexCoord3d, (s: f64, t: f64, r: f64));
ffi_fn!(FnGlTexCoord3f, (s: f32, t: f32, r: f32));
ffi_fn!(FnGlTexCoord3i, (s: i32, t: i32, r: i32));
ffi_fn!(FnGlTexCoord3s, (s: i16, t: i16, r: i16));
ffi_fn!(FnGlTexCoord4d, (s: f64, t: f64, r: f64, q: f64));
ffi_fn!(FnGlTexCoord4i, (s: i32, t: i32, r: i32, q: i32));
ffi_fn!(FnGlRasterPos2d, (x: f64, y: f64));
ffi_fn!(FnGlRasterPos2s, (x: i16, y: i16));
ffi_fn!(FnGlRasterPos3d, (x: f64, y: f64, z: f64));
ffi_fn!(FnGlRasterPos3s, (x: i16, y: i16, z: i16));
ffi_fn!(FnGlRasterPos4d, (x: f64, y: f64, z: f64, w: f64));
ffi_fn!(FnGlRasterPos4s, (x: i16, y: i16, z: i16, w: i16));
ffi_fn!(FnGlRectd, (x1: f64, y1: f64, x2: f64, y2: f64));
ffi_fn!(FnGlRects, (x1: i16, y1: i16, x2: i16, y2: i16));
ffi_fn!(FnGlRotated, (angle: f64, x: f64, y: f64, z: f64));
ffi_fn!(FnGlScaled, (x: f64, y: f64, z: f64));
ffi_fn!(FnGlTranslated, (x: f64, y: f64, z: f64));
ffi_fn!(FnGlLineStipple, (factor: i32, pattern: u16));
ffi_fn!(FnGlDrawBuffer, (mode: u32));
ffi_fn!(FnGlReadBuffer, (mode: u32));
ffi_fn!(FnGlCopyTexImage1D, (target: u32, level: i32, internal_format: u32, x: i32, y: i32, width: i32, border: i32));
ffi_fn!(FnGlCopyTexSubImage1D, (target: u32, level: i32, xoffset: i32, x: i32, y: i32, width: i32));
ffi_fn!(FnGlTexSubImage1D, (target: u32, level: i32, xoffset: i32, width: i32, format: u32, type_: u32, data: *const c_void));

// GL 1.2 (optional)
ffi_fn!(FnGlTexImage3D, (target: u32, level: i32, internal_format: i32, width: i32, height: i32, depth: i32, border: i32, format: u32, type_: u32, data: *const c_void));
ffi_fn!(FnGlTexSubImage3D, (target: u32, level: i32, xoffset: i32, yoffset: i32, zoffset: i32, width: i32, height: i32, depth: i32, format: u32, type_: u32, data: *const c_void));

// GL 1.3 (optional)
ffi_fn!(FnGlActiveTexture, (texture: u32));
ffi_fn!(FnGlMultiTexCoord1f, (target: u32, s: f32));
ffi_fn!(FnGlMultiTexCoord2f, (target: u32, s: f32, t: f32));
ffi_fn!(FnGlMultiTexCoord3f, (target: u32, s: f32, t: f32, r: f32));
ffi_fn!(FnGlMultiTexCoord4f, (target: u32, s: f32, t: f32, r: f32, q: f32));
ffi_fn!(FnGlSampleCoverage, (value: f32, invert: u8));
ffi_fn!(FnGlCompressedTexImage1D, (target: u32, level: i32, internalformat: u32, width: i32, border: i32, image_size: i32, data: *const c_void));
ffi_fn!(FnGlCompressedTexImage2D, (target: u32, level: i32, internalformat: u32, width: i32, height: i32, border: i32, image_size: i32, data: *const c_void));
ffi_fn!(FnGlCompressedTexImage3D, (target: u32, level: i32, internalformat: u32, width: i32, height: i32, depth: i32, border: i32, image_size: i32, data: *const c_void));
ffi_fn!(FnGlCompressedTexSubImage1D, (target: u32, level: i32, xoffset: i32, width: i32, format: u32, image_size: i32, data: *const c_void));
ffi_fn!(FnGlCompressedTexSubImage2D, (target: u32, level: i32, xoffset: i32, yoffset: i32, width: i32, height: i32, format: u32, image_size: i32, data: *const c_void));
ffi_fn!(FnGlCompressedTexSubImage3D, (target: u32, level: i32, xoffset: i32, yoffset: i32, zoffset: i32, width: i32, height: i32, depth: i32, format: u32, image_size: i32, data: *const c_void));

// GL 1.4 (optional)
ffi_fn!(FnGlSecondaryColor3f, (r: f32, g: f32, b: f32));
ffi_fn!(FnGlSecondaryColor3ub, (r: u8, g: u8, b: u8));
ffi_fn!(FnGlWindowPos2f, (x: f32, y: f32));
ffi_fn!(FnGlWindowPos3f, (x: f32, y: f32, z: f32));
ffi_fn!(FnGlFogCoordf, (coord: f32));
ffi_fn!(FnGlFogCoordd, (coord: f64));
ffi_fn!(FnGlPointParameterf, (pname: u32, param: f32));
ffi_fn!(FnGlPointParameterfv, (pname: u32, params: *const f32));
ffi_fn!(FnGlPointParameteri, (pname: u32, param: i32));

// GL 2.0 stencil separate (optional)
ffi_fn!(FnGlStencilFuncSeparate, (face: u32, func: u32, ref_: i32, mask: u32));
ffi_fn!(FnGlStencilOpSeparate, (face: u32, sfail: u32, dpfail: u32, dppass: u32));
ffi_fn!(FnGlStencilMaskSeparate, (face: u32, mask: u32));

// Imaging subset (optional)
ffi_fn!(FnGlColorTable, (target: u32, internal_format: u32, width: i32, format: u32, type_: u32, data: *const c_void));
ffi_fn!(FnGlConvolutionParameterf, (target: u32, pname: u32, param: f32));
ffi_fn!(FnGlConvolutionParameterfv, (target: u32, pname: u32, params: *const f32));
ffi_fn!(FnGlConvolutionParameteri, (target: u32, pname: u32, param: i32));
ffi_fn!(FnGlConvolutionParameteriv, (target: u32, pname: u32, params: *const i32));
ffi_fn!(FnGlHistogram, (target: u32, width: i32, internal_format: u32, sink: u8));
ffi_fn!(FnGlMinmax, (target: u32, internal_format: u32, sink: u8));

// GL 1.4 (optional)
ffi_fn!(FnGlBlendEquation, (mode: u32));
ffi_fn!(FnGlBlendFuncSeparate, (src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32));
ffi_fn!(FnGlBlendColor, (r: f32, g: f32, b: f32, a: f32));

// GL 2.0 Shader functions (optional)
ffi_fn!(FnGlCreateShader, (type_: u32) -> u32);
ffi_fn!(FnGlDeleteShader, (shader: u32));
ffi_fn!(FnGlShaderSource, (shader: u32, count: i32, string: *const *const i8, length: *const i32));
ffi_fn!(FnGlCompileShader, (shader: u32));
ffi_fn!(FnGlGetShaderiv, (shader: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetShaderInfoLog, (shader: u32, max_length: i32, length: *mut i32, info_log: *mut i8));
ffi_fn!(FnGlCreateProgram, () -> u32);
ffi_fn!(FnGlDeleteProgram, (program: u32));
ffi_fn!(FnGlAttachShader, (program: u32, shader: u32));
ffi_fn!(FnGlDetachShader, (program: u32, shader: u32));
ffi_fn!(FnGlLinkProgram, (program: u32));
ffi_fn!(FnGlUseProgram, (program: u32));
ffi_fn!(FnGlGetProgramiv, (program: u32, pname: u32, params: *mut i32));
ffi_fn!(FnGlGetProgramInfoLog, (program: u32, max_length: i32, length: *mut i32, info_log: *mut i8));
ffi_fn!(FnGlGetUniformLocation, (program: u32, name: *const i8) -> i32);
ffi_fn!(FnGlUniform1f, (location: i32, v0: f32));
ffi_fn!(FnGlUniform2f, (location: i32, v0: f32, v1: f32));
ffi_fn!(FnGlUniform3f, (location: i32, v0: f32, v1: f32, v2: f32));
ffi_fn!(FnGlUniform4f, (location: i32, v0: f32, v1: f32, v2: f32, v3: f32));
ffi_fn!(FnGlUniform1i, (location: i32, v0: i32));
ffi_fn!(FnGlUniform2i, (location: i32, v0: i32, v1: i32));
ffi_fn!(FnGlUniform3i, (location: i32, v0: i32, v1: i32, v2: i32));
ffi_fn!(FnGlUniform4i, (location: i32, v0: i32, v1: i32, v2: i32, v3: i32));
ffi_fn!(FnGlUniformMatrix4fv, (location: i32, count: i32, transpose: u8, value: *const f32));
ffi_fn!(FnGlGetAttribLocation, (program: u32, name: *const i8) -> i32);
ffi_fn!(FnGlVertexAttribPointer, (index: u32, size: i32, type_: u32, normalized: u8, stride: i32, pointer: *const c_void));
ffi_fn!(FnGlEnableVertexAttribArray, (index: u32));
ffi_fn!(FnGlDisableVertexAttribArray, (index: u32));

// GL 1.5 Buffer Objects (optional)
ffi_fn!(FnGlGenBuffers, (n: i32, buffers: *mut u32));
ffi_fn!(FnGlDeleteBuffers, (n: i32, buffers: *const u32));
ffi_fn!(FnGlBindBuffer, (target: u32, buffer: u32));
ffi_fn!(FnGlBufferData, (target: u32, size: isize, data: *const c_void, usage: u32));
ffi_fn!(FnGlBufferSubData, (target: u32, offset: isize, size: isize, data: *const c_void));
ffi_fn!(FnGlMapBuffer, (target: u32, access: u32) -> *mut c_void);
ffi_fn!(FnGlUnmapBuffer, (target: u32) -> u8);

// GL 3.0 FBO/VAO (optional)
ffi_fn!(FnGlGenFramebuffers, (n: i32, ids: *mut u32));
ffi_fn!(FnGlDeleteFramebuffers, (n: i32, ids: *const u32));
ffi_fn!(FnGlBindFramebuffer, (target: u32, framebuffer: u32));
ffi_fn!(FnGlFramebufferTexture2D, (target: u32, attachment: u32, textarget: u32, texture: u32, level: i32));
ffi_fn!(FnGlGenRenderbuffers, (n: i32, ids: *mut u32));
ffi_fn!(FnGlDeleteRenderbuffers, (n: i32, ids: *const u32));
ffi_fn!(FnGlBindRenderbuffer, (target: u32, renderbuffer: u32));
ffi_fn!(FnGlRenderbufferStorage, (target: u32, internal_format: u32, width: i32, height: i32));
ffi_fn!(FnGlCheckFramebufferStatus, (target: u32) -> u32);
ffi_fn!(FnGlGenVertexArrays, (n: i32, arrays: *mut u32));
ffi_fn!(FnGlDeleteVertexArrays, (n: i32, arrays: *const u32));
ffi_fn!(FnGlBindVertexArray, (array: u32));
ffi_fn!(FnGlFramebufferRenderbuffer, (target: u32, attachment: u32, renderbuffertarget: u32, renderbuffer: u32));

/// Holds resolved function pointers to libOSMesa and GL.
#[allow(dead_code)]
struct OsMesaFns {
    // Library handle for additional symbol resolution
    lib_handle: *mut c_void,

    // OSMesa functions
    create_context_ext: FnOSMesaCreateContextExt,
    destroy_context: FnOSMesaDestroyContext,
    make_current: FnOSMesaMakeCurrent,
    get_proc_address: FnOSMesaGetProcAddress,
    pixel_store: FnOSMesaPixelStore,

    // GL 1.0-1.1 functions (always present)
    clear: FnGlClear,
    clear_color: FnGlClearColor,
    viewport: FnGlViewport,
    begin: FnGlBegin,
    end: FnGlEnd,
    vertex2f: FnGlVertex2f,
    vertex3f: FnGlVertex3f,
    vertex4f: FnGlVertex4f,
    vertex2i: FnGlVertex2i,
    vertex3i: FnGlVertex3i,
    color3f: FnGlColor3f,
    color4f: FnGlColor4f,
    color3ub: FnGlColor3ub,
    color4ub: FnGlColor4ub,
    flush: FnGlFlush,
    finish: FnGlFinish,
    enable: FnGlEnable,
    disable: FnGlDisable,
    gen_textures: FnGlGenTextures,
    delete_textures: FnGlDeleteTextures,
    bind_texture: FnGlBindTexture,
    tex_image_2d: FnGlTexImage2D,
    tex_parameteri: FnGlTexParameteri,
    tex_sub_image_2d: FnGlTexSubImage2D,
    read_pixels: FnGlReadPixels,
    scissor: FnGlScissor,
    blend_func: FnGlBlendFunc,
    depth_func: FnGlDepthFunc,
    depth_mask: FnGlDepthMask,
    color_mask: FnGlColorMask,
    stencil_func: FnGlStencilFunc,
    stencil_op: FnGlStencilOp,
    stencil_mask: FnGlStencilMask,
    matrix_mode: FnGlMatrixMode,
    load_identity: FnGlLoadIdentity,
    load_matrixf: FnGlLoadMatrixf,
    load_matrixd: FnGlLoadMatrixd,
    mult_matrixf: FnGlMultMatrixf,
    mult_matrixd: FnGlMultMatrixd,
    push_matrix: FnGlPushMatrix,
    pop_matrix: FnGlPopMatrix,
    ortho: FnGlOrtho,
    frustum: FnGlFrustum,
    rotatef: FnGlRotatef,
    scalef: FnGlScalef,
    translatef: FnGlTranslatef,
    normal3f: FnGlNormal3f,
    tex_coord2f: FnGlTexCoord2f,
    tex_coord4f: FnGlTexCoord4f,
    pixel_storei: FnGlPixelStorei,
    line_width: FnGlLineWidth,
    point_size: FnGlPointSize,
    polygon_mode: FnGlPolygonMode,
    cull_face: FnGlCullFace,
    front_face: FnGlFrontFace,
    shade_model: FnGlShadeModel,
    clear_depth: FnGlClearDepth,
    clear_stencil: FnGlClearStencil,
    alpha_func: FnGlAlphaFunc,
    hint: FnGlHint,
    get_integerv: FnGlGetIntegerv,
    get_floatv: FnGlGetFloatv,
    get_error: FnGlGetError,
    get_string: FnGlGetString,
    rectf: FnGlRectf,
    recti: FnGlRecti,
    rectd: FnGlRectd,
    rects: FnGlRects,

    // Additional color/vertex/normal/texcoord/index/rasterpos variants
    color3b: FnGlColor3b,
    color3d: FnGlColor3d,
    color3i: FnGlColor3i,
    color3s: FnGlColor3s,
    color3ui: FnGlColor3ui,
    color3us: FnGlColor3us,
    color4b: FnGlColor4b,
    color4d: FnGlColor4d,
    color4i: FnGlColor4i,
    color4s: FnGlColor4s,
    color4ui: FnGlColor4ui,
    color4us: FnGlColor4us,
    edge_flag: FnGlEdgeFlag,
    indexd: FnGlIndexd,
    indexf: FnGlIndexf,
    indexi: FnGlIndexi,
    indexs: FnGlIndexs,
    indexub: FnGlIndexub,
    index_mask: FnGlIndexMask,
    clear_index: FnGlClearIndex,
    normal3b: FnGlNormal3b,
    normal3d: FnGlNormal3d,
    normal3i: FnGlNormal3i,
    normal3s: FnGlNormal3s,
    vertex2d: FnGlVertex2d,
    vertex2s: FnGlVertex2s,
    vertex3d: FnGlVertex3d,
    vertex3s: FnGlVertex3s,
    vertex4d: FnGlVertex4d,
    vertex4i: FnGlVertex4i,
    vertex4s: FnGlVertex4s,
    tex_coord1d: FnGlTexCoord1d,
    tex_coord1f: FnGlTexCoord1f,
    tex_coord1i: FnGlTexCoord1i,
    tex_coord1s: FnGlTexCoord1s,
    tex_coord2d: FnGlTexCoord2d,
    tex_coord2i: FnGlTexCoord2i,
    tex_coord2s: FnGlTexCoord2s,
    tex_coord3d: FnGlTexCoord3d,
    tex_coord3f: FnGlTexCoord3f,
    tex_coord3i: FnGlTexCoord3i,
    tex_coord3s: FnGlTexCoord3s,
    tex_coord4d: FnGlTexCoord4d,
    tex_coord4i: FnGlTexCoord4i,
    raster_pos2d: FnGlRasterPos2d,
    raster_pos2s: FnGlRasterPos2s,
    raster_pos3d: FnGlRasterPos3d,
    raster_pos3s: FnGlRasterPos3s,
    raster_pos4d: FnGlRasterPos4d,
    raster_pos4s: FnGlRasterPos4s,
    rotated: FnGlRotated,
    scaled: FnGlScaled,
    translated: FnGlTranslated,
    line_stipple: FnGlLineStipple,
    draw_buffer: FnGlDrawBuffer,
    read_buffer: FnGlReadBuffer,
    copy_tex_image_1d: FnGlCopyTexImage1D,
    copy_tex_sub_image_1d: FnGlCopyTexSubImage1D,
    tex_sub_image_1d: FnGlTexSubImage1D,

    // Display Lists
    new_list: FnGlNewList,
    end_list: FnGlEndList,
    gen_lists: FnGlGenLists,
    delete_lists: FnGlDeleteLists,
    is_list: FnGlIsList,
    call_list: FnGlCallList,
    call_lists: FnGlCallLists,
    list_base: FnGlListBase,

    // Lighting
    lightf: FnGlLightf,
    lightfv: FnGlLightfv,
    lighti: FnGlLighti,
    lightiv: FnGlLightiv,
    light_modelf: FnGlLightModelf,
    light_modelfv: FnGlLightModelfv,
    light_modeli: FnGlLightModeli,
    light_modeliv: FnGlLightModeliv,
    materialf: FnGlMaterialf,
    materialfv: FnGlMaterialfv,
    materiali: FnGlMateriali,
    materialiv: FnGlMaterialiv,
    color_material: FnGlColorMaterial,

    // Fog
    fogf: FnGlFogf,
    fogfv: FnGlFogfv,
    fogi: FnGlFogi,
    fogiv: FnGlFogiv,

    // Polygon/Drawing
    polygon_offset: FnGlPolygonOffset,
    polygon_stipple: FnGlPolygonStipple,
    get_polygon_stipple: FnGlGetPolygonStipple,
    logic_op: FnGlLogicOp,
    draw_pixels: FnGlDrawPixels,
    copy_pixels: FnGlCopyPixels,
    bitmap: FnGlBitmap,
    pixel_zoom: FnGlPixelZoom,
    raster_pos2f: FnGlRasterPos2f,
    raster_pos3f: FnGlRasterPos3f,
    raster_pos4f: FnGlRasterPos4f,
    raster_pos2i: FnGlRasterPos2i,
    raster_pos3i: FnGlRasterPos3i,
    raster_pos4i: FnGlRasterPos4i,

    // Depth
    depth_range: FnGlDepthRange,

    // Texture environment/generation
    tex_envf: FnGlTexEnvf,
    tex_envfv: FnGlTexEnvfv,
    tex_envi: FnGlTexEnvi,
    tex_enviv: FnGlTexEnviv,
    tex_geni: FnGlTexGeni,
    tex_genf: FnGlTexGenf,
    tex_gend: FnGlTexGend,
    tex_geniv: FnGlTexGeniv,
    tex_genfv: FnGlTexGenfv,
    tex_gendv: FnGlTexGendv,
    tex_image_1d: FnGlTexImage1D,
    copy_tex_image_2d: FnGlCopyTexImage2D,
    copy_tex_sub_image_2d: FnGlCopyTexSubImage2D,
    tex_parameterf: FnGlTexParameterf,
    tex_parameterfv: FnGlTexParameterfv,
    tex_parameteriv: FnGlTexParameteriv,
    pixel_storef: FnGlPixelStoref,
    pixel_transferf: FnGlPixelTransferf,
    pixel_transferi: FnGlPixelTransferi,

    // Vertex Arrays (GL 1.1)
    draw_arrays: FnGlDrawArrays,
    draw_elements: FnGlDrawElements,
    vertex_pointer: FnGlVertexPointer,
    color_pointer: FnGlColorPointer,
    normal_pointer: FnGlNormalPointer,
    tex_coord_pointer: FnGlTexCoordPointer,
    enable_client_state: FnGlEnableClientState,
    disable_client_state: FnGlDisableClientState,
    interleaved_arrays: FnGlInterleavedArrays,
    array_element: FnGlArrayElement,

    // State queries
    get_booleanv: FnGlGetBooleanv,
    get_doublev: FnGlGetDoublev,
    is_enabled: FnGlIsEnabled,
    get_tex_parameteriv: FnGlGetTexParameteriv,
    get_tex_parameterfv: FnGlGetTexParameterfv,
    get_tex_level_parameteriv: FnGlGetTexLevelParameteriv,
    get_tex_level_parameterfv: FnGlGetTexLevelParameterfv,
    get_tex_image: FnGlGetTexImage,
    get_lightfv: FnGlGetLightfv,
    get_lightiv: FnGlGetLightiv,
    get_materialfv: FnGlGetMaterialfv,
    get_materialiv: FnGlGetMaterialiv,
    is_texture: FnGlIsTexture,
    are_textures_resident: FnGlAreTexturesResident,
    get_tex_envfv: FnGlGetTexEnvfv,
    get_tex_enviv: FnGlGetTexEnviv,
    get_tex_gendv: FnGlGetTexGendv,
    get_tex_genfv: FnGlGetTexGenfv,
    get_tex_geniv: FnGlGetTexGeniv,
    get_pixel_mapfv: FnGlGetPixelMapfv,
    get_pixel_mapuiv: FnGlGetPixelMapuiv,
    get_pixel_mapusv: FnGlGetPixelMapusv,
    get_mapdv: FnGlGetMapdv,
    get_mapfv: FnGlGetMapfv,
    get_mapiv: FnGlGetMapiv,
    get_clip_plane: FnGlGetClipPlane,
    clip_plane: FnGlClipPlane,

    // Evaluators
    map1f: FnGlMap1f,
    map1d: FnGlMap1d,
    map2f: FnGlMap2f,
    map2d: FnGlMap2d,
    eval_coord1f: FnGlEvalCoord1f,
    eval_coord1d: FnGlEvalCoord1d,
    eval_coord2f: FnGlEvalCoord2f,
    eval_coord2d: FnGlEvalCoord2d,
    map_grid1f: FnGlMapGrid1f,
    map_grid1d: FnGlMapGrid1d,
    map_grid2f: FnGlMapGrid2f,
    map_grid2d: FnGlMapGrid2d,
    eval_mesh1: FnGlEvalMesh1,
    eval_mesh2: FnGlEvalMesh2,
    eval_point1: FnGlEvalPoint1,
    eval_point2: FnGlEvalPoint2,

    // Accumulation
    accum: FnGlAccum,
    clear_accum: FnGlClearAccum,

    // Selection/Feedback
    render_mode: FnGlRenderMode,
    init_names: FnGlInitNames,
    push_name: FnGlPushName,
    pop_name: FnGlPopName,
    load_name: FnGlLoadName,
    select_buffer: FnGlSelectBuffer,
    feedback_buffer: FnGlFeedbackBuffer,
    pass_through: FnGlPassThrough,
    push_attrib: FnGlPushAttrib,
    pop_attrib: FnGlPopAttrib,
    pixel_mapfv: FnGlPixelMapfv,
    pixel_mapuiv: FnGlPixelMapuiv,
    pixel_mapusv: FnGlPixelMapusv,

    // --- Optional (GL 1.2+) ---

    // GL 1.2
    tex_image_3d: Option<FnGlTexImage3D>,
    tex_sub_image_3d: Option<FnGlTexSubImage3D>,

    // GL 1.3
    active_texture: Option<FnGlActiveTexture>,
    multi_tex_coord1f: Option<FnGlMultiTexCoord1f>,
    multi_tex_coord2f: Option<FnGlMultiTexCoord2f>,
    multi_tex_coord3f: Option<FnGlMultiTexCoord3f>,
    multi_tex_coord4f: Option<FnGlMultiTexCoord4f>,
    sample_coverage: Option<FnGlSampleCoverage>,
    compressed_tex_image_1d: Option<FnGlCompressedTexImage1D>,
    compressed_tex_image_2d: Option<FnGlCompressedTexImage2D>,
    compressed_tex_image_3d: Option<FnGlCompressedTexImage3D>,
    compressed_tex_sub_image_1d: Option<FnGlCompressedTexSubImage1D>,
    compressed_tex_sub_image_2d: Option<FnGlCompressedTexSubImage2D>,
    compressed_tex_sub_image_3d: Option<FnGlCompressedTexSubImage3D>,

    // GL 1.4
    secondary_color3f: Option<FnGlSecondaryColor3f>,
    secondary_color3ub: Option<FnGlSecondaryColor3ub>,
    window_pos2f: Option<FnGlWindowPos2f>,
    window_pos3f: Option<FnGlWindowPos3f>,
    fog_coordf: Option<FnGlFogCoordf>,
    fog_coordd: Option<FnGlFogCoordd>,
    point_parameterf: Option<FnGlPointParameterf>,
    point_parameterfv: Option<FnGlPointParameterfv>,
    point_parameteri: Option<FnGlPointParameteri>,
    blend_equation: Option<FnGlBlendEquation>,
    blend_func_separate: Option<FnGlBlendFuncSeparate>,
    blend_color: Option<FnGlBlendColor>,

    // GL 2.0 stencil separate
    stencil_func_separate: Option<FnGlStencilFuncSeparate>,
    stencil_op_separate: Option<FnGlStencilOpSeparate>,
    stencil_mask_separate: Option<FnGlStencilMaskSeparate>,

    // Imaging subset
    color_table: Option<FnGlColorTable>,
    convolution_parameterf: Option<FnGlConvolutionParameterf>,
    convolution_parameterfv: Option<FnGlConvolutionParameterfv>,
    convolution_parameteri: Option<FnGlConvolutionParameteri>,
    convolution_parameteriv: Option<FnGlConvolutionParameteriv>,
    histogram: Option<FnGlHistogram>,
    minmax: Option<FnGlMinmax>,

    // GL 2.0 Shaders
    create_shader: Option<FnGlCreateShader>,
    delete_shader: Option<FnGlDeleteShader>,
    shader_source: Option<FnGlShaderSource>,
    compile_shader: Option<FnGlCompileShader>,
    get_shaderiv: Option<FnGlGetShaderiv>,
    get_shader_info_log: Option<FnGlGetShaderInfoLog>,
    create_program: Option<FnGlCreateProgram>,
    delete_program: Option<FnGlDeleteProgram>,
    attach_shader: Option<FnGlAttachShader>,
    detach_shader: Option<FnGlDetachShader>,
    link_program: Option<FnGlLinkProgram>,
    use_program: Option<FnGlUseProgram>,
    get_programiv: Option<FnGlGetProgramiv>,
    get_program_info_log: Option<FnGlGetProgramInfoLog>,
    get_uniform_location: Option<FnGlGetUniformLocation>,
    uniform1f: Option<FnGlUniform1f>,
    uniform2f: Option<FnGlUniform2f>,
    uniform3f: Option<FnGlUniform3f>,
    uniform4f: Option<FnGlUniform4f>,
    uniform1i: Option<FnGlUniform1i>,
    uniform2i: Option<FnGlUniform2i>,
    uniform3i: Option<FnGlUniform3i>,
    uniform4i: Option<FnGlUniform4i>,
    uniform_matrix4fv: Option<FnGlUniformMatrix4fv>,
    get_attrib_location: Option<FnGlGetAttribLocation>,
    vertex_attrib_pointer: Option<FnGlVertexAttribPointer>,
    enable_vertex_attrib_array: Option<FnGlEnableVertexAttribArray>,
    disable_vertex_attrib_array: Option<FnGlDisableVertexAttribArray>,

    // GL 1.5 Buffer Objects
    gen_buffers: Option<FnGlGenBuffers>,
    delete_buffers: Option<FnGlDeleteBuffers>,
    bind_buffer: Option<FnGlBindBuffer>,
    buffer_data: Option<FnGlBufferData>,
    buffer_sub_data: Option<FnGlBufferSubData>,
    map_buffer: Option<FnGlMapBuffer>,
    unmap_buffer: Option<FnGlUnmapBuffer>,

    // GL 3.0 FBO/VAO
    gen_framebuffers: Option<FnGlGenFramebuffers>,
    delete_framebuffers: Option<FnGlDeleteFramebuffers>,
    bind_framebuffer: Option<FnGlBindFramebuffer>,
    framebuffer_texture_2d: Option<FnGlFramebufferTexture2D>,
    gen_renderbuffers: Option<FnGlGenRenderbuffers>,
    delete_renderbuffers: Option<FnGlDeleteRenderbuffers>,
    bind_renderbuffer: Option<FnGlBindRenderbuffer>,
    renderbuffer_storage: Option<FnGlRenderbufferStorage>,
    check_framebuffer_status: Option<FnGlCheckFramebufferStatus>,
    framebuffer_renderbuffer: Option<FnGlFramebufferRenderbuffer>,
    gen_vertex_arrays: Option<FnGlGenVertexArrays>,
    delete_vertex_arrays: Option<FnGlDeleteVertexArrays>,
    bind_vertex_array: Option<FnGlBindVertexArray>,
}

unsafe impl Send for OsMesaFns {}
unsafe impl Sync for OsMesaFns {}

static FNS: OnceLock<OsMesaFns> = OnceLock::new();

/// Attempt to load libOSMesa at runtime. Returns `true` if successful.
pub fn init() -> bool {
    if FNS.get().is_some() {
        return true;
    }
    match try_load() {
        Ok(fns) => {
            let _ = FNS.set(fns);
            info!("OSMesa loaded successfully");
            true
        }
        Err(e) => {
            warn!("OSMesa not available: {e}");
            false
        }
    }
}

/// Returns true if OSMesa was successfully loaded.
pub fn is_available() -> bool {
    FNS.get().is_some()
}

fn fns() -> &'static OsMesaFns {
    FNS.get().expect("OSMesa not initialized — call osmesa::init() first")
}

/// Resolve an arbitrary GL function by name at runtime using the stored
/// `OSMesaGetProcAddress` and library handle.  Returns `None` if the symbol
/// is not found.
pub fn resolve_proc(name: &str) -> Option<*const c_void> {
    let f = FNS.get()?;
    let cname = CString::new(name).ok()?;
    let mut ptr = unsafe { (f.get_proc_address)(cname.as_ptr()) };
    if ptr.is_null() {
        ptr = unsafe { libc::dlsym(f.lib_handle, cname.as_ptr()) } as *const c_void;
    }
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

fn try_load() -> Result<OsMesaFns, String> {
    // Try common library names
    let lib_names = [
        "libOSMesa.so.8",
        "libOSMesa.so.6",
        "libOSMesa.so",
    ];

    let lib = {
        let mut last_err = String::new();
        let mut loaded = None;
        for name in &lib_names {
            match unsafe { libc::dlopen(
                CString::new(*name).unwrap().as_ptr(),
                libc::RTLD_NOW | libc::RTLD_GLOBAL,
            ) } {
                handle if !handle.is_null() => {
                    info!("Loaded {name}");
                    loaded = Some(handle);
                    break;
                }
                _ => {
                    let err = unsafe { CStr::from_ptr(libc::dlerror()) };
                    last_err = err.to_string_lossy().into_owned();
                    debug!("Failed to load {name}: {last_err}");
                }
            }
        }
        loaded.ok_or_else(|| format!("Could not load libOSMesa: {last_err}"))?
    };

    macro_rules! sym {
        ($lib:expr, $name:expr, $ty:ty) => {{
            let cname = CString::new($name).unwrap();
            let ptr = unsafe { libc::dlsym($lib, cname.as_ptr()) };
            if ptr.is_null() {
                return Err(format!("Symbol {} not found", $name));
            }
            unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
        }};
    }

    let create_context_ext = sym!(lib, "OSMesaCreateContextExt", FnOSMesaCreateContextExt);
    let destroy_context = sym!(lib, "OSMesaDestroyContext", FnOSMesaDestroyContext);
    let make_current = sym!(lib, "OSMesaMakeCurrent", FnOSMesaMakeCurrent);
    let get_proc_address = sym!(lib, "OSMesaGetProcAddress", FnOSMesaGetProcAddress);
    let pixel_store = sym!(lib, "OSMesaPixelStore", FnOSMesaPixelStore);

    // Resolve GL functions — try OSMesaGetProcAddress first, then dlsym
    macro_rules! gl {
        ($lib:expr, $gpa:expr, $name:expr, $ty:ty) => {{
            let cname = CString::new($name).unwrap();
            let mut ptr = unsafe { ($gpa)(cname.as_ptr()) };
            if ptr.is_null() {
                ptr = unsafe { libc::dlsym($lib, cname.as_ptr()) };
            }
            if ptr.is_null() {
                return Err(format!("GL symbol {} not found", $name));
            }
            unsafe { std::mem::transmute::<*const c_void, $ty>(ptr) }
        }};
    }

    // Optional GL function resolution — returns None instead of Err on failure
    macro_rules! gl_opt {
        ($lib:expr, $gpa:expr, $name:expr, $ty:ty) => {{
            let cname = CString::new($name).unwrap();
            let mut ptr = unsafe { ($gpa)(cname.as_ptr()) };
            if ptr.is_null() {
                ptr = unsafe { libc::dlsym($lib, cname.as_ptr()) };
            }
            if ptr.is_null() {
                debug!("Optional GL symbol {} not found", $name);
                None
            } else {
                Some(unsafe { std::mem::transmute::<*const c_void, $ty>(ptr) })
            }
        }};
    }

    Ok(OsMesaFns {
        lib_handle: lib,

        create_context_ext,
        destroy_context,
        make_current,
        get_proc_address,
        pixel_store,

        // --- GL 1.0-1.1 (required) ---
        clear: gl!(lib, get_proc_address, "glClear", FnGlClear),
        clear_color: gl!(lib, get_proc_address, "glClearColor", FnGlClearColor),
        viewport: gl!(lib, get_proc_address, "glViewport", FnGlViewport),
        begin: gl!(lib, get_proc_address, "glBegin", FnGlBegin),
        end: gl!(lib, get_proc_address, "glEnd", FnGlEnd),
        vertex2f: gl!(lib, get_proc_address, "glVertex2f", FnGlVertex2f),
        vertex3f: gl!(lib, get_proc_address, "glVertex3f", FnGlVertex3f),
        vertex4f: gl!(lib, get_proc_address, "glVertex4f", FnGlVertex4f),
        vertex2i: gl!(lib, get_proc_address, "glVertex2i", FnGlVertex2i),
        vertex3i: gl!(lib, get_proc_address, "glVertex3i", FnGlVertex3i),
        color3f: gl!(lib, get_proc_address, "glColor3f", FnGlColor3f),
        color4f: gl!(lib, get_proc_address, "glColor4f", FnGlColor4f),
        color3ub: gl!(lib, get_proc_address, "glColor3ub", FnGlColor3ub),
        color4ub: gl!(lib, get_proc_address, "glColor4ub", FnGlColor4ub),
        flush: gl!(lib, get_proc_address, "glFlush", FnGlFlush),
        finish: gl!(lib, get_proc_address, "glFinish", FnGlFinish),
        enable: gl!(lib, get_proc_address, "glEnable", FnGlEnable),
        disable: gl!(lib, get_proc_address, "glDisable", FnGlDisable),
        gen_textures: gl!(lib, get_proc_address, "glGenTextures", FnGlGenTextures),
        delete_textures: gl!(lib, get_proc_address, "glDeleteTextures", FnGlDeleteTextures),
        bind_texture: gl!(lib, get_proc_address, "glBindTexture", FnGlBindTexture),
        tex_image_2d: gl!(lib, get_proc_address, "glTexImage2D", FnGlTexImage2D),
        tex_parameteri: gl!(lib, get_proc_address, "glTexParameteri", FnGlTexParameteri),
        tex_sub_image_2d: gl!(lib, get_proc_address, "glTexSubImage2D", FnGlTexSubImage2D),
        read_pixels: gl!(lib, get_proc_address, "glReadPixels", FnGlReadPixels),
        scissor: gl!(lib, get_proc_address, "glScissor", FnGlScissor),
        blend_func: gl!(lib, get_proc_address, "glBlendFunc", FnGlBlendFunc),
        depth_func: gl!(lib, get_proc_address, "glDepthFunc", FnGlDepthFunc),
        depth_mask: gl!(lib, get_proc_address, "glDepthMask", FnGlDepthMask),
        color_mask: gl!(lib, get_proc_address, "glColorMask", FnGlColorMask),
        stencil_func: gl!(lib, get_proc_address, "glStencilFunc", FnGlStencilFunc),
        stencil_op: gl!(lib, get_proc_address, "glStencilOp", FnGlStencilOp),
        stencil_mask: gl!(lib, get_proc_address, "glStencilMask", FnGlStencilMask),
        matrix_mode: gl!(lib, get_proc_address, "glMatrixMode", FnGlMatrixMode),
        load_identity: gl!(lib, get_proc_address, "glLoadIdentity", FnGlLoadIdentity),
        load_matrixf: gl!(lib, get_proc_address, "glLoadMatrixf", FnGlLoadMatrixf),
        load_matrixd: gl!(lib, get_proc_address, "glLoadMatrixd", FnGlLoadMatrixd),
        mult_matrixf: gl!(lib, get_proc_address, "glMultMatrixf", FnGlMultMatrixf),
        mult_matrixd: gl!(lib, get_proc_address, "glMultMatrixd", FnGlMultMatrixd),
        push_matrix: gl!(lib, get_proc_address, "glPushMatrix", FnGlPushMatrix),
        pop_matrix: gl!(lib, get_proc_address, "glPopMatrix", FnGlPopMatrix),
        ortho: gl!(lib, get_proc_address, "glOrtho", FnGlOrtho),
        frustum: gl!(lib, get_proc_address, "glFrustum", FnGlFrustum),
        rotatef: gl!(lib, get_proc_address, "glRotatef", FnGlRotatef),
        scalef: gl!(lib, get_proc_address, "glScalef", FnGlScalef),
        translatef: gl!(lib, get_proc_address, "glTranslatef", FnGlTranslatef),
        normal3f: gl!(lib, get_proc_address, "glNormal3f", FnGlNormal3f),
        tex_coord2f: gl!(lib, get_proc_address, "glTexCoord2f", FnGlTexCoord2f),
        tex_coord4f: gl!(lib, get_proc_address, "glTexCoord4f", FnGlTexCoord4f),
        pixel_storei: gl!(lib, get_proc_address, "glPixelStorei", FnGlPixelStorei),
        line_width: gl!(lib, get_proc_address, "glLineWidth", FnGlLineWidth),
        point_size: gl!(lib, get_proc_address, "glPointSize", FnGlPointSize),
        polygon_mode: gl!(lib, get_proc_address, "glPolygonMode", FnGlPolygonMode),
        cull_face: gl!(lib, get_proc_address, "glCullFace", FnGlCullFace),
        front_face: gl!(lib, get_proc_address, "glFrontFace", FnGlFrontFace),
        shade_model: gl!(lib, get_proc_address, "glShadeModel", FnGlShadeModel),
        clear_depth: gl!(lib, get_proc_address, "glClearDepth", FnGlClearDepth),
        clear_stencil: gl!(lib, get_proc_address, "glClearStencil", FnGlClearStencil),
        alpha_func: gl!(lib, get_proc_address, "glAlphaFunc", FnGlAlphaFunc),
        hint: gl!(lib, get_proc_address, "glHint", FnGlHint),
        get_integerv: gl!(lib, get_proc_address, "glGetIntegerv", FnGlGetIntegerv),
        get_floatv: gl!(lib, get_proc_address, "glGetFloatv", FnGlGetFloatv),
        get_error: gl!(lib, get_proc_address, "glGetError", FnGlGetError),
        get_string: gl!(lib, get_proc_address, "glGetString", FnGlGetString),
        rectf: gl!(lib, get_proc_address, "glRectf", FnGlRectf),
        recti: gl!(lib, get_proc_address, "glRecti", FnGlRecti),
        rectd: gl!(lib, get_proc_address, "glRectd", FnGlRectd),
        rects: gl!(lib, get_proc_address, "glRects", FnGlRects),

        // Additional variants
        color3b: gl!(lib, get_proc_address, "glColor3b", FnGlColor3b),
        color3d: gl!(lib, get_proc_address, "glColor3d", FnGlColor3d),
        color3i: gl!(lib, get_proc_address, "glColor3i", FnGlColor3i),
        color3s: gl!(lib, get_proc_address, "glColor3s", FnGlColor3s),
        color3ui: gl!(lib, get_proc_address, "glColor3ui", FnGlColor3ui),
        color3us: gl!(lib, get_proc_address, "glColor3us", FnGlColor3us),
        color4b: gl!(lib, get_proc_address, "glColor4b", FnGlColor4b),
        color4d: gl!(lib, get_proc_address, "glColor4d", FnGlColor4d),
        color4i: gl!(lib, get_proc_address, "glColor4i", FnGlColor4i),
        color4s: gl!(lib, get_proc_address, "glColor4s", FnGlColor4s),
        color4ui: gl!(lib, get_proc_address, "glColor4ui", FnGlColor4ui),
        color4us: gl!(lib, get_proc_address, "glColor4us", FnGlColor4us),
        edge_flag: gl!(lib, get_proc_address, "glEdgeFlag", FnGlEdgeFlag),
        indexd: gl!(lib, get_proc_address, "glIndexd", FnGlIndexd),
        indexf: gl!(lib, get_proc_address, "glIndexf", FnGlIndexf),
        indexi: gl!(lib, get_proc_address, "glIndexi", FnGlIndexi),
        indexs: gl!(lib, get_proc_address, "glIndexs", FnGlIndexs),
        indexub: gl!(lib, get_proc_address, "glIndexub", FnGlIndexub),
        index_mask: gl!(lib, get_proc_address, "glIndexMask", FnGlIndexMask),
        clear_index: gl!(lib, get_proc_address, "glClearIndex", FnGlClearIndex),
        normal3b: gl!(lib, get_proc_address, "glNormal3b", FnGlNormal3b),
        normal3d: gl!(lib, get_proc_address, "glNormal3d", FnGlNormal3d),
        normal3i: gl!(lib, get_proc_address, "glNormal3i", FnGlNormal3i),
        normal3s: gl!(lib, get_proc_address, "glNormal3s", FnGlNormal3s),
        vertex2d: gl!(lib, get_proc_address, "glVertex2d", FnGlVertex2d),
        vertex2s: gl!(lib, get_proc_address, "glVertex2s", FnGlVertex2s),
        vertex3d: gl!(lib, get_proc_address, "glVertex3d", FnGlVertex3d),
        vertex3s: gl!(lib, get_proc_address, "glVertex3s", FnGlVertex3s),
        vertex4d: gl!(lib, get_proc_address, "glVertex4d", FnGlVertex4d),
        vertex4i: gl!(lib, get_proc_address, "glVertex4i", FnGlVertex4i),
        vertex4s: gl!(lib, get_proc_address, "glVertex4s", FnGlVertex4s),
        tex_coord1d: gl!(lib, get_proc_address, "glTexCoord1d", FnGlTexCoord1d),
        tex_coord1f: gl!(lib, get_proc_address, "glTexCoord1f", FnGlTexCoord1f),
        tex_coord1i: gl!(lib, get_proc_address, "glTexCoord1i", FnGlTexCoord1i),
        tex_coord1s: gl!(lib, get_proc_address, "glTexCoord1s", FnGlTexCoord1s),
        tex_coord2d: gl!(lib, get_proc_address, "glTexCoord2d", FnGlTexCoord2d),
        tex_coord2i: gl!(lib, get_proc_address, "glTexCoord2i", FnGlTexCoord2i),
        tex_coord2s: gl!(lib, get_proc_address, "glTexCoord2s", FnGlTexCoord2s),
        tex_coord3d: gl!(lib, get_proc_address, "glTexCoord3d", FnGlTexCoord3d),
        tex_coord3f: gl!(lib, get_proc_address, "glTexCoord3f", FnGlTexCoord3f),
        tex_coord3i: gl!(lib, get_proc_address, "glTexCoord3i", FnGlTexCoord3i),
        tex_coord3s: gl!(lib, get_proc_address, "glTexCoord3s", FnGlTexCoord3s),
        tex_coord4d: gl!(lib, get_proc_address, "glTexCoord4d", FnGlTexCoord4d),
        tex_coord4i: gl!(lib, get_proc_address, "glTexCoord4i", FnGlTexCoord4i),
        raster_pos2d: gl!(lib, get_proc_address, "glRasterPos2d", FnGlRasterPos2d),
        raster_pos2s: gl!(lib, get_proc_address, "glRasterPos2s", FnGlRasterPos2s),
        raster_pos3d: gl!(lib, get_proc_address, "glRasterPos3d", FnGlRasterPos3d),
        raster_pos3s: gl!(lib, get_proc_address, "glRasterPos3s", FnGlRasterPos3s),
        raster_pos4d: gl!(lib, get_proc_address, "glRasterPos4d", FnGlRasterPos4d),
        raster_pos4s: gl!(lib, get_proc_address, "glRasterPos4s", FnGlRasterPos4s),
        rotated: gl!(lib, get_proc_address, "glRotated", FnGlRotated),
        scaled: gl!(lib, get_proc_address, "glScaled", FnGlScaled),
        translated: gl!(lib, get_proc_address, "glTranslated", FnGlTranslated),
        line_stipple: gl!(lib, get_proc_address, "glLineStipple", FnGlLineStipple),
        draw_buffer: gl!(lib, get_proc_address, "glDrawBuffer", FnGlDrawBuffer),
        read_buffer: gl!(lib, get_proc_address, "glReadBuffer", FnGlReadBuffer),
        copy_tex_image_1d: gl!(lib, get_proc_address, "glCopyTexImage1D", FnGlCopyTexImage1D),
        copy_tex_sub_image_1d: gl!(lib, get_proc_address, "glCopyTexSubImage1D", FnGlCopyTexSubImage1D),
        tex_sub_image_1d: gl!(lib, get_proc_address, "glTexSubImage1D", FnGlTexSubImage1D),

        // Display Lists
        new_list: gl!(lib, get_proc_address, "glNewList", FnGlNewList),
        end_list: gl!(lib, get_proc_address, "glEndList", FnGlEndList),
        gen_lists: gl!(lib, get_proc_address, "glGenLists", FnGlGenLists),
        delete_lists: gl!(lib, get_proc_address, "glDeleteLists", FnGlDeleteLists),
        is_list: gl!(lib, get_proc_address, "glIsList", FnGlIsList),
        call_list: gl!(lib, get_proc_address, "glCallList", FnGlCallList),
        call_lists: gl!(lib, get_proc_address, "glCallLists", FnGlCallLists),
        list_base: gl!(lib, get_proc_address, "glListBase", FnGlListBase),

        // Lighting
        lightf: gl!(lib, get_proc_address, "glLightf", FnGlLightf),
        lightfv: gl!(lib, get_proc_address, "glLightfv", FnGlLightfv),
        lighti: gl!(lib, get_proc_address, "glLighti", FnGlLighti),
        lightiv: gl!(lib, get_proc_address, "glLightiv", FnGlLightiv),
        light_modelf: gl!(lib, get_proc_address, "glLightModelf", FnGlLightModelf),
        light_modelfv: gl!(lib, get_proc_address, "glLightModelfv", FnGlLightModelfv),
        light_modeli: gl!(lib, get_proc_address, "glLightModeli", FnGlLightModeli),
        light_modeliv: gl!(lib, get_proc_address, "glLightModeliv", FnGlLightModeliv),
        materialf: gl!(lib, get_proc_address, "glMaterialf", FnGlMaterialf),
        materialfv: gl!(lib, get_proc_address, "glMaterialfv", FnGlMaterialfv),
        materiali: gl!(lib, get_proc_address, "glMateriali", FnGlMateriali),
        materialiv: gl!(lib, get_proc_address, "glMaterialiv", FnGlMaterialiv),
        color_material: gl!(lib, get_proc_address, "glColorMaterial", FnGlColorMaterial),

        // Fog
        fogf: gl!(lib, get_proc_address, "glFogf", FnGlFogf),
        fogfv: gl!(lib, get_proc_address, "glFogfv", FnGlFogfv),
        fogi: gl!(lib, get_proc_address, "glFogi", FnGlFogi),
        fogiv: gl!(lib, get_proc_address, "glFogiv", FnGlFogiv),

        // Polygon/Drawing
        polygon_offset: gl!(lib, get_proc_address, "glPolygonOffset", FnGlPolygonOffset),
        polygon_stipple: gl!(lib, get_proc_address, "glPolygonStipple", FnGlPolygonStipple),
        get_polygon_stipple: gl!(lib, get_proc_address, "glGetPolygonStipple", FnGlGetPolygonStipple),
        logic_op: gl!(lib, get_proc_address, "glLogicOp", FnGlLogicOp),
        draw_pixels: gl!(lib, get_proc_address, "glDrawPixels", FnGlDrawPixels),
        copy_pixels: gl!(lib, get_proc_address, "glCopyPixels", FnGlCopyPixels),
        bitmap: gl!(lib, get_proc_address, "glBitmap", FnGlBitmap),
        pixel_zoom: gl!(lib, get_proc_address, "glPixelZoom", FnGlPixelZoom),
        raster_pos2f: gl!(lib, get_proc_address, "glRasterPos2f", FnGlRasterPos2f),
        raster_pos3f: gl!(lib, get_proc_address, "glRasterPos3f", FnGlRasterPos3f),
        raster_pos4f: gl!(lib, get_proc_address, "glRasterPos4f", FnGlRasterPos4f),
        raster_pos2i: gl!(lib, get_proc_address, "glRasterPos2i", FnGlRasterPos2i),
        raster_pos3i: gl!(lib, get_proc_address, "glRasterPos3i", FnGlRasterPos3i),
        raster_pos4i: gl!(lib, get_proc_address, "glRasterPos4i", FnGlRasterPos4i),

        // Depth
        depth_range: gl!(lib, get_proc_address, "glDepthRange", FnGlDepthRange),

        // Texture environment/generation
        tex_envf: gl!(lib, get_proc_address, "glTexEnvf", FnGlTexEnvf),
        tex_envfv: gl!(lib, get_proc_address, "glTexEnvfv", FnGlTexEnvfv),
        tex_envi: gl!(lib, get_proc_address, "glTexEnvi", FnGlTexEnvi),
        tex_enviv: gl!(lib, get_proc_address, "glTexEnviv", FnGlTexEnviv),
        tex_geni: gl!(lib, get_proc_address, "glTexGeni", FnGlTexGeni),
        tex_genf: gl!(lib, get_proc_address, "glTexGenf", FnGlTexGenf),
        tex_gend: gl!(lib, get_proc_address, "glTexGend", FnGlTexGend),
        tex_geniv: gl!(lib, get_proc_address, "glTexGeniv", FnGlTexGeniv),
        tex_genfv: gl!(lib, get_proc_address, "glTexGenfv", FnGlTexGenfv),
        tex_gendv: gl!(lib, get_proc_address, "glTexGendv", FnGlTexGendv),
        tex_image_1d: gl!(lib, get_proc_address, "glTexImage1D", FnGlTexImage1D),
        copy_tex_image_2d: gl!(lib, get_proc_address, "glCopyTexImage2D", FnGlCopyTexImage2D),
        copy_tex_sub_image_2d: gl!(lib, get_proc_address, "glCopyTexSubImage2D", FnGlCopyTexSubImage2D),
        tex_parameterf: gl!(lib, get_proc_address, "glTexParameterf", FnGlTexParameterf),
        tex_parameterfv: gl!(lib, get_proc_address, "glTexParameterfv", FnGlTexParameterfv),
        tex_parameteriv: gl!(lib, get_proc_address, "glTexParameteriv", FnGlTexParameteriv),
        pixel_storef: gl!(lib, get_proc_address, "glPixelStoref", FnGlPixelStoref),
        pixel_transferf: gl!(lib, get_proc_address, "glPixelTransferf", FnGlPixelTransferf),
        pixel_transferi: gl!(lib, get_proc_address, "glPixelTransferi", FnGlPixelTransferi),

        // Vertex Arrays (GL 1.1)
        draw_arrays: gl!(lib, get_proc_address, "glDrawArrays", FnGlDrawArrays),
        draw_elements: gl!(lib, get_proc_address, "glDrawElements", FnGlDrawElements),
        vertex_pointer: gl!(lib, get_proc_address, "glVertexPointer", FnGlVertexPointer),
        color_pointer: gl!(lib, get_proc_address, "glColorPointer", FnGlColorPointer),
        normal_pointer: gl!(lib, get_proc_address, "glNormalPointer", FnGlNormalPointer),
        tex_coord_pointer: gl!(lib, get_proc_address, "glTexCoordPointer", FnGlTexCoordPointer),
        enable_client_state: gl!(lib, get_proc_address, "glEnableClientState", FnGlEnableClientState),
        disable_client_state: gl!(lib, get_proc_address, "glDisableClientState", FnGlDisableClientState),
        interleaved_arrays: gl!(lib, get_proc_address, "glInterleavedArrays", FnGlInterleavedArrays),
        array_element: gl!(lib, get_proc_address, "glArrayElement", FnGlArrayElement),

        // State queries
        get_booleanv: gl!(lib, get_proc_address, "glGetBooleanv", FnGlGetBooleanv),
        get_doublev: gl!(lib, get_proc_address, "glGetDoublev", FnGlGetDoublev),
        is_enabled: gl!(lib, get_proc_address, "glIsEnabled", FnGlIsEnabled),
        get_tex_parameteriv: gl!(lib, get_proc_address, "glGetTexParameteriv", FnGlGetTexParameteriv),
        get_tex_parameterfv: gl!(lib, get_proc_address, "glGetTexParameterfv", FnGlGetTexParameterfv),
        get_tex_level_parameteriv: gl!(lib, get_proc_address, "glGetTexLevelParameteriv", FnGlGetTexLevelParameteriv),
        get_tex_level_parameterfv: gl!(lib, get_proc_address, "glGetTexLevelParameterfv", FnGlGetTexLevelParameterfv),
        get_tex_image: gl!(lib, get_proc_address, "glGetTexImage", FnGlGetTexImage),
        get_lightfv: gl!(lib, get_proc_address, "glGetLightfv", FnGlGetLightfv),
        get_lightiv: gl!(lib, get_proc_address, "glGetLightiv", FnGlGetLightiv),
        get_materialfv: gl!(lib, get_proc_address, "glGetMaterialfv", FnGlGetMaterialfv),
        get_materialiv: gl!(lib, get_proc_address, "glGetMaterialiv", FnGlGetMaterialiv),
        is_texture: gl!(lib, get_proc_address, "glIsTexture", FnGlIsTexture),
        are_textures_resident: gl!(lib, get_proc_address, "glAreTexturesResident", FnGlAreTexturesResident),
        get_tex_envfv: gl!(lib, get_proc_address, "glGetTexEnvfv", FnGlGetTexEnvfv),
        get_tex_enviv: gl!(lib, get_proc_address, "glGetTexEnviv", FnGlGetTexEnviv),
        get_tex_gendv: gl!(lib, get_proc_address, "glGetTexGendv", FnGlGetTexGendv),
        get_tex_genfv: gl!(lib, get_proc_address, "glGetTexGenfv", FnGlGetTexGenfv),
        get_tex_geniv: gl!(lib, get_proc_address, "glGetTexGeniv", FnGlGetTexGeniv),
        get_pixel_mapfv: gl!(lib, get_proc_address, "glGetPixelMapfv", FnGlGetPixelMapfv),
        get_pixel_mapuiv: gl!(lib, get_proc_address, "glGetPixelMapuiv", FnGlGetPixelMapuiv),
        get_pixel_mapusv: gl!(lib, get_proc_address, "glGetPixelMapusv", FnGlGetPixelMapusv),
        get_mapdv: gl!(lib, get_proc_address, "glGetMapdv", FnGlGetMapdv),
        get_mapfv: gl!(lib, get_proc_address, "glGetMapfv", FnGlGetMapfv),
        get_mapiv: gl!(lib, get_proc_address, "glGetMapiv", FnGlGetMapiv),
        get_clip_plane: gl!(lib, get_proc_address, "glGetClipPlane", FnGlGetClipPlane),
        clip_plane: gl!(lib, get_proc_address, "glClipPlane", FnGlClipPlane),

        // Evaluators
        map1f: gl!(lib, get_proc_address, "glMap1f", FnGlMap1f),
        map1d: gl!(lib, get_proc_address, "glMap1d", FnGlMap1d),
        map2f: gl!(lib, get_proc_address, "glMap2f", FnGlMap2f),
        map2d: gl!(lib, get_proc_address, "glMap2d", FnGlMap2d),
        eval_coord1f: gl!(lib, get_proc_address, "glEvalCoord1f", FnGlEvalCoord1f),
        eval_coord1d: gl!(lib, get_proc_address, "glEvalCoord1d", FnGlEvalCoord1d),
        eval_coord2f: gl!(lib, get_proc_address, "glEvalCoord2f", FnGlEvalCoord2f),
        eval_coord2d: gl!(lib, get_proc_address, "glEvalCoord2d", FnGlEvalCoord2d),
        map_grid1f: gl!(lib, get_proc_address, "glMapGrid1f", FnGlMapGrid1f),
        map_grid1d: gl!(lib, get_proc_address, "glMapGrid1d", FnGlMapGrid1d),
        map_grid2f: gl!(lib, get_proc_address, "glMapGrid2f", FnGlMapGrid2f),
        map_grid2d: gl!(lib, get_proc_address, "glMapGrid2d", FnGlMapGrid2d),
        eval_mesh1: gl!(lib, get_proc_address, "glEvalMesh1", FnGlEvalMesh1),
        eval_mesh2: gl!(lib, get_proc_address, "glEvalMesh2", FnGlEvalMesh2),
        eval_point1: gl!(lib, get_proc_address, "glEvalPoint1", FnGlEvalPoint1),
        eval_point2: gl!(lib, get_proc_address, "glEvalPoint2", FnGlEvalPoint2),

        // Accumulation
        accum: gl!(lib, get_proc_address, "glAccum", FnGlAccum),
        clear_accum: gl!(lib, get_proc_address, "glClearAccum", FnGlClearAccum),

        // Selection/Feedback
        render_mode: gl!(lib, get_proc_address, "glRenderMode", FnGlRenderMode),
        init_names: gl!(lib, get_proc_address, "glInitNames", FnGlInitNames),
        push_name: gl!(lib, get_proc_address, "glPushName", FnGlPushName),
        pop_name: gl!(lib, get_proc_address, "glPopName", FnGlPopName),
        load_name: gl!(lib, get_proc_address, "glLoadName", FnGlLoadName),
        select_buffer: gl!(lib, get_proc_address, "glSelectBuffer", FnGlSelectBuffer),
        feedback_buffer: gl!(lib, get_proc_address, "glFeedbackBuffer", FnGlFeedbackBuffer),
        pass_through: gl!(lib, get_proc_address, "glPassThrough", FnGlPassThrough),
        push_attrib: gl!(lib, get_proc_address, "glPushAttrib", FnGlPushAttrib),
        pop_attrib: gl!(lib, get_proc_address, "glPopAttrib", FnGlPopAttrib),
        pixel_mapfv: gl!(lib, get_proc_address, "glPixelMapfv", FnGlPixelMapfv),
        pixel_mapuiv: gl!(lib, get_proc_address, "glPixelMapuiv", FnGlPixelMapuiv),
        pixel_mapusv: gl!(lib, get_proc_address, "glPixelMapusv", FnGlPixelMapusv),

        // --- Optional (GL 1.2+) ---

        // GL 1.2
        tex_image_3d: gl_opt!(lib, get_proc_address, "glTexImage3D", FnGlTexImage3D),
        tex_sub_image_3d: gl_opt!(lib, get_proc_address, "glTexSubImage3D", FnGlTexSubImage3D),

        // GL 1.3
        active_texture: gl_opt!(lib, get_proc_address, "glActiveTexture", FnGlActiveTexture),
        multi_tex_coord1f: gl_opt!(lib, get_proc_address, "glMultiTexCoord1f", FnGlMultiTexCoord1f),
        multi_tex_coord2f: gl_opt!(lib, get_proc_address, "glMultiTexCoord2f", FnGlMultiTexCoord2f),
        multi_tex_coord3f: gl_opt!(lib, get_proc_address, "glMultiTexCoord3f", FnGlMultiTexCoord3f),
        multi_tex_coord4f: gl_opt!(lib, get_proc_address, "glMultiTexCoord4f", FnGlMultiTexCoord4f),
        sample_coverage: gl_opt!(lib, get_proc_address, "glSampleCoverage", FnGlSampleCoverage),
        compressed_tex_image_1d: gl_opt!(lib, get_proc_address, "glCompressedTexImage1D", FnGlCompressedTexImage1D),
        compressed_tex_image_2d: gl_opt!(lib, get_proc_address, "glCompressedTexImage2D", FnGlCompressedTexImage2D),
        compressed_tex_image_3d: gl_opt!(lib, get_proc_address, "glCompressedTexImage3D", FnGlCompressedTexImage3D),
        compressed_tex_sub_image_1d: gl_opt!(lib, get_proc_address, "glCompressedTexSubImage1D", FnGlCompressedTexSubImage1D),
        compressed_tex_sub_image_2d: gl_opt!(lib, get_proc_address, "glCompressedTexSubImage2D", FnGlCompressedTexSubImage2D),
        compressed_tex_sub_image_3d: gl_opt!(lib, get_proc_address, "glCompressedTexSubImage3D", FnGlCompressedTexSubImage3D),

        // GL 1.4
        secondary_color3f: gl_opt!(lib, get_proc_address, "glSecondaryColor3f", FnGlSecondaryColor3f),
        secondary_color3ub: gl_opt!(lib, get_proc_address, "glSecondaryColor3ub", FnGlSecondaryColor3ub),
        window_pos2f: gl_opt!(lib, get_proc_address, "glWindowPos2f", FnGlWindowPos2f),
        window_pos3f: gl_opt!(lib, get_proc_address, "glWindowPos3f", FnGlWindowPos3f),
        fog_coordf: gl_opt!(lib, get_proc_address, "glFogCoordf", FnGlFogCoordf),
        fog_coordd: gl_opt!(lib, get_proc_address, "glFogCoordd", FnGlFogCoordd),
        point_parameterf: gl_opt!(lib, get_proc_address, "glPointParameterf", FnGlPointParameterf),
        point_parameterfv: gl_opt!(lib, get_proc_address, "glPointParameterfv", FnGlPointParameterfv),
        point_parameteri: gl_opt!(lib, get_proc_address, "glPointParameteri", FnGlPointParameteri),
        blend_equation: gl_opt!(lib, get_proc_address, "glBlendEquation", FnGlBlendEquation),
        blend_func_separate: gl_opt!(lib, get_proc_address, "glBlendFuncSeparate", FnGlBlendFuncSeparate),
        blend_color: gl_opt!(lib, get_proc_address, "glBlendColor", FnGlBlendColor),

        // GL 2.0 stencil separate
        stencil_func_separate: gl_opt!(lib, get_proc_address, "glStencilFuncSeparate", FnGlStencilFuncSeparate),
        stencil_op_separate: gl_opt!(lib, get_proc_address, "glStencilOpSeparate", FnGlStencilOpSeparate),
        stencil_mask_separate: gl_opt!(lib, get_proc_address, "glStencilMaskSeparate", FnGlStencilMaskSeparate),

        // Imaging subset
        color_table: gl_opt!(lib, get_proc_address, "glColorTable", FnGlColorTable),
        convolution_parameterf: gl_opt!(lib, get_proc_address, "glConvolutionParameterf", FnGlConvolutionParameterf),
        convolution_parameterfv: gl_opt!(lib, get_proc_address, "glConvolutionParameterfv", FnGlConvolutionParameterfv),
        convolution_parameteri: gl_opt!(lib, get_proc_address, "glConvolutionParameteri", FnGlConvolutionParameteri),
        convolution_parameteriv: gl_opt!(lib, get_proc_address, "glConvolutionParameteriv", FnGlConvolutionParameteriv),
        histogram: gl_opt!(lib, get_proc_address, "glHistogram", FnGlHistogram),
        minmax: gl_opt!(lib, get_proc_address, "glMinmax", FnGlMinmax),

        // GL 2.0 Shaders
        create_shader: gl_opt!(lib, get_proc_address, "glCreateShader", FnGlCreateShader),
        delete_shader: gl_opt!(lib, get_proc_address, "glDeleteShader", FnGlDeleteShader),
        shader_source: gl_opt!(lib, get_proc_address, "glShaderSource", FnGlShaderSource),
        compile_shader: gl_opt!(lib, get_proc_address, "glCompileShader", FnGlCompileShader),
        get_shaderiv: gl_opt!(lib, get_proc_address, "glGetShaderiv", FnGlGetShaderiv),
        get_shader_info_log: gl_opt!(lib, get_proc_address, "glGetShaderInfoLog", FnGlGetShaderInfoLog),
        create_program: gl_opt!(lib, get_proc_address, "glCreateProgram", FnGlCreateProgram),
        delete_program: gl_opt!(lib, get_proc_address, "glDeleteProgram", FnGlDeleteProgram),
        attach_shader: gl_opt!(lib, get_proc_address, "glAttachShader", FnGlAttachShader),
        detach_shader: gl_opt!(lib, get_proc_address, "glDetachShader", FnGlDetachShader),
        link_program: gl_opt!(lib, get_proc_address, "glLinkProgram", FnGlLinkProgram),
        use_program: gl_opt!(lib, get_proc_address, "glUseProgram", FnGlUseProgram),
        get_programiv: gl_opt!(lib, get_proc_address, "glGetProgramiv", FnGlGetProgramiv),
        get_program_info_log: gl_opt!(lib, get_proc_address, "glGetProgramInfoLog", FnGlGetProgramInfoLog),
        get_uniform_location: gl_opt!(lib, get_proc_address, "glGetUniformLocation", FnGlGetUniformLocation),
        uniform1f: gl_opt!(lib, get_proc_address, "glUniform1f", FnGlUniform1f),
        uniform2f: gl_opt!(lib, get_proc_address, "glUniform2f", FnGlUniform2f),
        uniform3f: gl_opt!(lib, get_proc_address, "glUniform3f", FnGlUniform3f),
        uniform4f: gl_opt!(lib, get_proc_address, "glUniform4f", FnGlUniform4f),
        uniform1i: gl_opt!(lib, get_proc_address, "glUniform1i", FnGlUniform1i),
        uniform2i: gl_opt!(lib, get_proc_address, "glUniform2i", FnGlUniform2i),
        uniform3i: gl_opt!(lib, get_proc_address, "glUniform3i", FnGlUniform3i),
        uniform4i: gl_opt!(lib, get_proc_address, "glUniform4i", FnGlUniform4i),
        uniform_matrix4fv: gl_opt!(lib, get_proc_address, "glUniformMatrix4fv", FnGlUniformMatrix4fv),
        get_attrib_location: gl_opt!(lib, get_proc_address, "glGetAttribLocation", FnGlGetAttribLocation),
        vertex_attrib_pointer: gl_opt!(lib, get_proc_address, "glVertexAttribPointer", FnGlVertexAttribPointer),
        enable_vertex_attrib_array: gl_opt!(lib, get_proc_address, "glEnableVertexAttribArray", FnGlEnableVertexAttribArray),
        disable_vertex_attrib_array: gl_opt!(lib, get_proc_address, "glDisableVertexAttribArray", FnGlDisableVertexAttribArray),

        // GL 1.5 Buffer Objects
        gen_buffers: gl_opt!(lib, get_proc_address, "glGenBuffers", FnGlGenBuffers),
        delete_buffers: gl_opt!(lib, get_proc_address, "glDeleteBuffers", FnGlDeleteBuffers),
        bind_buffer: gl_opt!(lib, get_proc_address, "glBindBuffer", FnGlBindBuffer),
        buffer_data: gl_opt!(lib, get_proc_address, "glBufferData", FnGlBufferData),
        buffer_sub_data: gl_opt!(lib, get_proc_address, "glBufferSubData", FnGlBufferSubData),
        map_buffer: gl_opt!(lib, get_proc_address, "glMapBuffer", FnGlMapBuffer),
        unmap_buffer: gl_opt!(lib, get_proc_address, "glUnmapBuffer", FnGlUnmapBuffer),

        // GL 3.0 FBO/VAO
        gen_framebuffers: gl_opt!(lib, get_proc_address, "glGenFramebuffers", FnGlGenFramebuffers),
        delete_framebuffers: gl_opt!(lib, get_proc_address, "glDeleteFramebuffers", FnGlDeleteFramebuffers),
        bind_framebuffer: gl_opt!(lib, get_proc_address, "glBindFramebuffer", FnGlBindFramebuffer),
        framebuffer_texture_2d: gl_opt!(lib, get_proc_address, "glFramebufferTexture2D", FnGlFramebufferTexture2D),
        gen_renderbuffers: gl_opt!(lib, get_proc_address, "glGenRenderbuffers", FnGlGenRenderbuffers),
        delete_renderbuffers: gl_opt!(lib, get_proc_address, "glDeleteRenderbuffers", FnGlDeleteRenderbuffers),
        bind_renderbuffer: gl_opt!(lib, get_proc_address, "glBindRenderbuffer", FnGlBindRenderbuffer),
        renderbuffer_storage: gl_opt!(lib, get_proc_address, "glRenderbufferStorage", FnGlRenderbufferStorage),
        check_framebuffer_status: gl_opt!(lib, get_proc_address, "glCheckFramebufferStatus", FnGlCheckFramebufferStatus),
        framebuffer_renderbuffer: gl_opt!(lib, get_proc_address, "glFramebufferRenderbuffer", FnGlFramebufferRenderbuffer),
        gen_vertex_arrays: gl_opt!(lib, get_proc_address, "glGenVertexArrays", FnGlGenVertexArrays),
        delete_vertex_arrays: gl_opt!(lib, get_proc_address, "glDeleteVertexArrays", FnGlDeleteVertexArrays),
        bind_vertex_array: gl_opt!(lib, get_proc_address, "glBindVertexArray", FnGlBindVertexArray),
    })
}

// --------------------------------------------------------------------------
// Safe wrappers
// --------------------------------------------------------------------------

/// An OSMesa rendering context with an associated pixel buffer.
pub struct MesaContext {
    ctx: OSMesaContext,
    /// Pixel buffer that OSMesa renders into (BGRA, 4 bytes/pixel).
    buffer: Vec<u8>,
    width: u32,
    height: u32,
}

// OSMesa contexts are thread-local in Mesa's implementation but we only ever
// use them from the per-client tokio task, so Send is fine.
unsafe impl Send for MesaContext {}

impl MesaContext {
    /// Create a new OSMesa context with the given size.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if !is_available() {
            return None;
        }
        let f = fns();
        let ctx = unsafe {
            (f.create_context_ext)(OSMESA_RGBA, 24, 8, 0, ptr::null_mut())
        };
        if ctx.is_null() {
            error!("OSMesaCreateContextExt returned NULL");
            return None;
        }
        let buf_size = (width * height * 4) as usize;
        let mut buffer = vec![0u8; buf_size];

        let ok = unsafe {
            (f.make_current)(ctx, buffer.as_mut_ptr() as *mut c_void, GL_UNSIGNED_BYTE, width as i32, height as i32)
        };
        if ok == 0 {
            error!("OSMesaMakeCurrent failed for {}x{}", width, height);
            unsafe { (f.destroy_context)(ctx); }
            return None;
        }

        // Tell OSMesa that Y=0 is at the top (matches X11 coordinate system)
        unsafe { (f.pixel_store)(OSMESA_Y_UP, 0); }

        debug!("Created OSMesa context {}x{}", width, height);
        Some(Self { ctx, buffer, width, height })
    }

    /// Resize the backing buffer and re-bind the context.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let f = fns();
        let buf_size = (width * height * 4) as usize;
        self.buffer = vec![0u8; buf_size];
        self.width = width;
        self.height = height;
        let ok = unsafe {
            (f.make_current)(self.ctx, self.buffer.as_mut_ptr() as *mut c_void, GL_UNSIGNED_BYTE, width as i32, height as i32)
        };
        if ok == 0 {
            error!("OSMesaMakeCurrent failed on resize to {}x{}", width, height);
            return false;
        }
        unsafe { (f.pixel_store)(OSMESA_Y_UP, 0); }
        true
    }

    /// Make this context current so GL calls target its buffer.
    pub fn make_current(&mut self) -> bool {
        let f = fns();
        let ok = unsafe {
            (f.make_current)(self.ctx, self.buffer.as_mut_ptr() as *mut c_void, GL_UNSIGNED_BYTE, self.width as i32, self.height as i32)
        };
        ok != 0
    }

    /// Get a reference to the RGBA pixel buffer. Pixels are stored row-major,
    /// 4 bytes per pixel (R, G, B, A) with Y=0 at top.
    pub fn pixels(&self) -> &[u8] {
        &self.buffer
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Copy the OSMesa RGBA buffer into an X11 framebuffer (BGRA/XRGB format).
    /// The framebuffer uses A8R8G8B8 (BGRA in memory on LE).
    #[allow(dead_code)]
    pub fn blit_to_framebuffer(&self, fb: &mut crate::framebuffer::Framebuffer) {
        let w = self.width.min(fb.width()) as usize;
        let h = self.height.min(fb.height()) as usize;
        let src_stride = self.width as usize * 4;
        let dst_stride = fb.stride();
        let fb_data = fb.data_mut();

        for y in 0..h {
            let src_row = &self.buffer[y * src_stride..y * src_stride + w * 4];
            let dst_row = &mut fb_data[y * dst_stride..y * dst_stride + w * 4];
            for x in 0..w {
                let si = x * 4;
                let di = x * 4;
                // OSMesa RGBA -> Framebuffer BGRA (A8R8G8B8 on LE)
                dst_row[di] = src_row[si + 2]; // B
                dst_row[di + 1] = src_row[si + 1]; // G
                dst_row[di + 2] = src_row[si]; // R
                dst_row[di + 3] = src_row[si + 3]; // A
            }
        }
        fb.mark_dirty(0, 0, w as u32, h as u32);
    }
}

impl Drop for MesaContext {
    fn drop(&mut self) {
        if is_available() && !self.ctx.is_null() {
            let f = fns();
            unsafe { (f.destroy_context)(self.ctx); }
        }
    }
}

// --------------------------------------------------------------------------
// GL command dispatch -- called from the GLX handler
// --------------------------------------------------------------------------

mod gl_bindings;
pub use gl_bindings::*;
