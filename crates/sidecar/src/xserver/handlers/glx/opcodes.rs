//! GLX protocol opcode constants from Mesa's include/GL/glxproto.h.
//!
//! These match the X_GLsop_* and X_GLrop_* defines exactly.
//! Generated from Mesa 22.3.6 (Debian Bookworm).

#![allow(non_upper_case_globals)]

// --- GLX Single Operation opcodes (X_GLsop_*) ---
// Used for GL queries that require a reply (glGet*, glIs*, etc.)

pub const X_GLsop_DeleteLists: u32 = 103;
pub const X_GLsop_GenLists: u32 = 104;
pub const X_GLsop_RenderMode: u32 = 107;
pub const X_GLsop_Finish: u32 = 108;
pub const X_GLsop_PixelStoref: u32 = 109;
pub const X_GLsop_PixelStorei: u32 = 110;
pub const X_GLsop_GetBooleanv: u32 = 112;
pub const X_GLsop_GetClipPlane: u32 = 113;
pub const X_GLsop_GetDoublev: u32 = 114;
pub const X_GLsop_GetError: u32 = 115;
pub const X_GLsop_GetFloatv: u32 = 116;
pub const X_GLsop_GetIntegerv: u32 = 117;
pub const X_GLsop_GetLightfv: u32 = 118;
pub const X_GLsop_GetLightiv: u32 = 119;
pub const X_GLsop_GetMaterialfv: u32 = 123;
pub const X_GLsop_GetMaterialiv: u32 = 124;
pub const X_GLsop_GetPixelMapfv: u32 = 125;
pub const X_GLsop_GetPolygonStipple: u32 = 128;
pub const X_GLsop_GetString: u32 = 129;
pub const X_GLsop_GetTexEnvfv: u32 = 130;
pub const X_GLsop_GetTexEnviv: u32 = 131;
pub const X_GLsop_GetTexGendv: u32 = 132;
pub const X_GLsop_GetTexGenfv: u32 = 133;
pub const X_GLsop_GetTexGeniv: u32 = 134;
pub const X_GLsop_GetTexImage: u32 = 135;
pub const X_GLsop_GetTexParameterfv: u32 = 136;
pub const X_GLsop_GetTexParameteriv: u32 = 137;
pub const X_GLsop_GetTexLevelParameterfv: u32 = 138;
pub const X_GLsop_GetTexLevelParameteriv: u32 = 139;
pub const X_GLsop_IsEnabled: u32 = 140;
pub const X_GLsop_IsList: u32 = 141;
pub const X_GLsop_AreTexturesResident: u32 = 143;
pub const X_GLsop_DeleteTextures: u32 = 144;
pub const X_GLsop_GenTextures: u32 = 145;
pub const X_GLsop_IsTexture: u32 = 146;

// --- GLX Render Operation opcodes (X_GLrop_*) ---
// Used for GL commands batched in GLX Render requests.
