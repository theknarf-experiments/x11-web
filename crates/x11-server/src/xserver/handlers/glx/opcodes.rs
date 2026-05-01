//! GLX protocol opcode constants from Mesa's include/GL/glxproto.h.
//!
//! These match the X_GLsop_* and X_GLrop_* defines exactly.
//! Generated from Mesa 22.3.6 (Debian Bookworm).

#![allow(non_upper_case_globals)]

// --- GLX Single Operation opcodes (X_GLsop_*) ---
// Used for GL queries that require a reply (glGet*, glIs*, etc.)

pub const X_GLSOP_DELETE_LISTS: u32 = 103;
pub const X_GLSOP_GEN_LISTS: u32 = 104;
pub const X_GLSOP_RENDER_MODE: u32 = 107;
pub const X_GLSOP_FINISH: u32 = 108;
pub const X_GLSOP_PIXEL_STOREF: u32 = 109;
pub const X_GLSOP_PIXEL_STOREI: u32 = 110;
pub const X_GLSOP_GET_BOOLEANV: u32 = 112;
pub const X_GLSOP_GET_CLIP_PLANE: u32 = 113;
pub const X_GLSOP_GET_DOUBLEV: u32 = 114;
pub const X_GLSOP_GET_ERROR: u32 = 115;
pub const X_GLSOP_GET_FLOATV: u32 = 116;
pub const X_GLSOP_GET_INTEGERV: u32 = 117;
pub const X_GLSOP_GET_LIGHTFV: u32 = 118;
pub const X_GLSOP_GET_LIGHTIV: u32 = 119;
pub const X_GLSOP_GET_MATERIALFV: u32 = 123;
pub const X_GLSOP_GET_MATERIALIV: u32 = 124;
pub const X_GLSOP_GET_PIXEL_MAPFV: u32 = 125;
pub const X_GLSOP_GET_POLYGON_STIPPLE: u32 = 128;
pub const X_GLSOP_GET_STRING: u32 = 129;
pub const X_GLSOP_GET_TEX_ENVFV: u32 = 130;
pub const X_GLSOP_GET_TEX_ENVIV: u32 = 131;
pub const X_GLSOP_GET_TEX_GENDV: u32 = 132;
pub const X_GLSOP_GET_TEX_GENFV: u32 = 133;
pub const X_GLSOP_GET_TEX_GENIV: u32 = 134;
pub const X_GLSOP_GET_TEX_IMAGE: u32 = 135;
pub const X_GLSOP_GET_TEX_PARAMETERFV: u32 = 136;
pub const X_GLSOP_GET_TEX_PARAMETERIV: u32 = 137;
pub const X_GLSOP_GET_TEX_LEVEL_PARAMETERFV: u32 = 138;
pub const X_GLSOP_GET_TEX_LEVEL_PARAMETERIV: u32 = 139;
pub const X_GLSOP_IS_ENABLED: u32 = 140;
pub const X_GLSOP_IS_LIST: u32 = 141;
pub const X_GLSOP_ARE_TEXTURES_RESIDENT: u32 = 143;
pub const X_GLSOP_DELETE_TEXTURES: u32 = 144;
pub const X_GLSOP_GEN_TEXTURES: u32 = 145;
pub const X_GLSOP_IS_TEXTURE: u32 = 146;

// --- GLX Render Operation opcodes (X_GLrop_*) ---
// Used for GL commands batched in GLX Render requests.
