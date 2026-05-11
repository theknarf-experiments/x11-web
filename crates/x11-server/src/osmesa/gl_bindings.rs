//! Safe Rust wrappers around individual GL function calls.
//!
//! Each function delegates to the dynamically-resolved function pointer table
//! loaded by the parent `osmesa` module.

use std::ffi::{c_void, CStr};
use std::ptr;

use super::gl_generated;

/// Thin forwarder for trivial GL calls.
macro_rules! gl_thin {
    ($snake:ident -> $camel:ident($($p:ident: $t:ty),* $(,)?)) => {
        pub fn $snake($($p: $t),*) {
            unsafe { gl_generated::$camel($($p),*) }
        }
    };
}

// ===== Original GL 1.0-1.1 wrappers (unchanged API) =====

/// Execute a GL Clear call.
gl_thin!(gl_clear -> Clear(mask: u32));

/// Execute a GL ClearColor call.
gl_thin!(gl_clear_color -> ClearColor(r: f32, g: f32, b: f32, a: f32));

/// Execute a GL Viewport call.
gl_thin!(gl_viewport -> Viewport(x: i32, y: i32, w: i32, h: i32));

gl_thin!(gl_begin -> Begin(mode: u32));

gl_thin!(gl_end -> End());

gl_thin!(gl_vertex2f -> Vertex2f(x: f32, y: f32));

gl_thin!(gl_vertex3f -> Vertex3f(x: f32, y: f32, z: f32));

gl_thin!(gl_vertex4f -> Vertex4f(x: f32, y: f32, z: f32, w: f32));

gl_thin!(gl_vertex2i -> Vertex2i(x: i32, y: i32));

gl_thin!(gl_vertex3i -> Vertex3i(x: i32, y: i32, z: i32));

gl_thin!(gl_color3f -> Color3f(r: f32, g: f32, b: f32));

gl_thin!(gl_color4f -> Color4f(r: f32, g: f32, b: f32, a: f32));

gl_thin!(gl_color3ub -> Color3ub(r: u8, g: u8, b: u8));

gl_thin!(gl_color4ub -> Color4ub(r: u8, g: u8, b: u8, a: u8));

gl_thin!(gl_flush -> Flush());

gl_thin!(gl_finish -> Finish());

gl_thin!(gl_enable -> Enable(cap: u32));

gl_thin!(gl_disable -> Disable(cap: u32));

pub fn gl_gen_textures(n: i32, textures: &mut [u32]) {
    unsafe {
        gl_generated::GenTextures(n, textures.as_mut_ptr());
    }
}

pub fn gl_delete_textures(textures: &[u32]) {
    unsafe {
        gl_generated::DeleteTextures(textures.len() as i32, textures.as_ptr());
    }
}

gl_thin!(gl_bind_texture -> BindTexture(target: u32, texture: u32));

pub fn gl_tex_image_2d(
    target: u32,
    level: i32,
    internal_format: i32,
    width: i32,
    height: i32,
    border: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexImage2D(
            target,
            level,
            internal_format,
            width,
            height,
            border,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_tex_image_2d_null(
    target: u32,
    level: i32,
    internal_format: i32,
    width: i32,
    height: i32,
    border: i32,
    format: u32,
    type_: u32,
) {
    unsafe {
        gl_generated::TexImage2D(
            target,
            level,
            internal_format,
            width,
            height,
            border,
            format,
            type_,
            ptr::null(),
        );
    }
}

gl_thin!(gl_tex_parameteri -> TexParameteri(target: u32, pname: u32, param: i32));

pub fn gl_tex_sub_image_2d(
    target: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    width: i32,
    height: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexSubImage2D(
            target,
            level,
            xoffset,
            yoffset,
            width,
            height,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

gl_thin!(gl_scissor -> Scissor(x: i32, y: i32, w: i32, h: i32));

gl_thin!(gl_blend_func -> BlendFunc(sfactor: u32, dfactor: u32));

gl_thin!(gl_depth_func -> DepthFunc(func: u32));

gl_thin!(gl_depth_mask -> DepthMask(flag: u8));

gl_thin!(gl_color_mask -> ColorMask(r: u8, g: u8, b: u8, a: u8));

gl_thin!(gl_stencil_func -> StencilFunc(func: u32, ref_: i32, mask: u32));

gl_thin!(gl_stencil_op -> StencilOp(fail: u32, zfail: u32, zpass: u32));

gl_thin!(gl_stencil_mask -> StencilMask(mask: u32));

gl_thin!(gl_matrix_mode -> MatrixMode(mode: u32));

gl_thin!(gl_load_identity -> LoadIdentity());

pub fn gl_load_matrixf(m: &[f32; 16]) {
    unsafe {
        gl_generated::LoadMatrixf(m.as_ptr());
    }
}

pub fn gl_load_matrixd(m: &[f64; 16]) {
    unsafe {
        gl_generated::LoadMatrixd(m.as_ptr());
    }
}

pub fn gl_mult_matrixf(m: &[f32; 16]) {
    unsafe {
        gl_generated::MultMatrixf(m.as_ptr());
    }
}

pub fn gl_mult_matrixd(m: &[f64; 16]) {
    unsafe {
        gl_generated::MultMatrixd(m.as_ptr());
    }
}

gl_thin!(gl_push_matrix -> PushMatrix());

gl_thin!(gl_pop_matrix -> PopMatrix());

gl_thin!(gl_ortho -> Ortho(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64));

gl_thin!(gl_frustum -> Frustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64));

gl_thin!(gl_rotatef -> Rotatef(angle: f32, x: f32, y: f32, z: f32));

gl_thin!(gl_scalef -> Scalef(x: f32, y: f32, z: f32));

gl_thin!(gl_translatef -> Translatef(x: f32, y: f32, z: f32));

gl_thin!(gl_normal3f -> Normal3f(nx: f32, ny: f32, nz: f32));

gl_thin!(gl_tex_coord2f -> TexCoord2f(s: f32, t: f32));

gl_thin!(gl_tex_coord4f -> TexCoord4f(s: f32, t: f32, r: f32, q: f32));

gl_thin!(gl_pixel_storei -> PixelStorei(pname: u32, param: i32));

gl_thin!(gl_line_width -> LineWidth(width: f32));

gl_thin!(gl_point_size -> PointSize(size: f32));

gl_thin!(gl_polygon_mode -> PolygonMode(face: u32, mode: u32));

gl_thin!(gl_cull_face -> CullFace(mode: u32));

gl_thin!(gl_front_face -> FrontFace(mode: u32));

gl_thin!(gl_shade_model -> ShadeModel(mode: u32));

gl_thin!(gl_clear_depth -> ClearDepth(depth: f64));

gl_thin!(gl_clear_stencil -> ClearStencil(s: i32));

gl_thin!(gl_alpha_func -> AlphaFunc(func: u32, ref_: f32));

gl_thin!(gl_hint -> Hint(target: u32, mode: u32));

pub fn gl_get_integerv(pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetIntegerv(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_floatv(pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetFloatv(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_error() -> u32 {
    unsafe { gl_generated::GetError() }
}

pub fn gl_get_string(name: u32) -> String {
    let ptr = unsafe { gl_generated::GetString(name) };
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        CStr::from_ptr(ptr as *const std::ffi::c_char)
            .to_string_lossy()
            .into_owned()
    }
}

gl_thin!(gl_rectf -> Rectf(x1: f32, y1: f32, x2: f32, y2: f32));

gl_thin!(gl_recti -> Recti(x1: i32, y1: i32, x2: i32, y2: i32));

gl_thin!(gl_rectd -> Rectd(x1: f64, y1: f64, x2: f64, y2: f64));

gl_thin!(gl_rects -> Rects(x1: i16, y1: i16, x2: i16, y2: i16));

// ===== Additional color variants =====

pub fn gl_color3b(r: i8, g: i8, b: i8) {
    use gl_generated::types::GLbyte;
    unsafe {
        gl_generated::Color3b(r as GLbyte, g as GLbyte, b as GLbyte);
    }
}
gl_thin!(gl_color3d -> Color3d(r: f64, g: f64, b: f64));
gl_thin!(gl_color3i -> Color3i(r: i32, g: i32, b: i32));
gl_thin!(gl_color3s -> Color3s(r: i16, g: i16, b: i16));
gl_thin!(gl_color3ui -> Color3ui(r: u32, g: u32, b: u32));
gl_thin!(gl_color3us -> Color3us(r: u16, g: u16, b: u16));
pub fn gl_color4b(r: i8, g: i8, b: i8, a: i8) {
    use gl_generated::types::GLbyte;
    unsafe {
        gl_generated::Color4b(r as GLbyte, g as GLbyte, b as GLbyte, a as GLbyte);
    }
}
gl_thin!(gl_color4d -> Color4d(r: f64, g: f64, b: f64, a: f64));
gl_thin!(gl_color4i -> Color4i(r: i32, g: i32, b: i32, a: i32));
gl_thin!(gl_color4s -> Color4s(r: i16, g: i16, b: i16, a: i16));
gl_thin!(gl_color4ui -> Color4ui(r: u32, g: u32, b: u32, a: u32));
gl_thin!(gl_color4us -> Color4us(r: u16, g: u16, b: u16, a: u16));

// ===== Edge flag / Index / ClearIndex =====

gl_thin!(gl_edge_flag -> EdgeFlag(flag: u8));
gl_thin!(gl_indexd -> Indexd(c: f64));
gl_thin!(gl_indexf -> Indexf(c: f32));
gl_thin!(gl_indexi -> Indexi(c: i32));
gl_thin!(gl_indexs -> Indexs(c: i16));
gl_thin!(gl_indexub -> Indexub(c: u8));
gl_thin!(gl_index_mask -> IndexMask(mask: u32));
gl_thin!(gl_clear_index -> ClearIndex(c: f32));

// ===== Additional normal variants =====

pub fn gl_normal3b(nx: i8, ny: i8, nz: i8) {
    use gl_generated::types::GLbyte;
    unsafe {
        gl_generated::Normal3b(nx as GLbyte, ny as GLbyte, nz as GLbyte);
    }
}
gl_thin!(gl_normal3d -> Normal3d(nx: f64, ny: f64, nz: f64));
gl_thin!(gl_normal3i -> Normal3i(nx: i32, ny: i32, nz: i32));
gl_thin!(gl_normal3s -> Normal3s(nx: i16, ny: i16, nz: i16));

// ===== Additional vertex variants =====

gl_thin!(gl_vertex2d -> Vertex2d(x: f64, y: f64));
gl_thin!(gl_vertex2s -> Vertex2s(x: i16, y: i16));
gl_thin!(gl_vertex3d -> Vertex3d(x: f64, y: f64, z: f64));
gl_thin!(gl_vertex3s -> Vertex3s(x: i16, y: i16, z: i16));
gl_thin!(gl_vertex4d -> Vertex4d(x: f64, y: f64, z: f64, w: f64));
gl_thin!(gl_vertex4i -> Vertex4i(x: i32, y: i32, z: i32, w: i32));
gl_thin!(gl_vertex4s -> Vertex4s(x: i16, y: i16, z: i16, w: i16));

// ===== Additional texcoord variants =====

gl_thin!(gl_tex_coord1d -> TexCoord1d(s: f64));
gl_thin!(gl_tex_coord1f -> TexCoord1f(s: f32));
gl_thin!(gl_tex_coord1i -> TexCoord1i(s: i32));
gl_thin!(gl_tex_coord1s -> TexCoord1s(s: i16));
gl_thin!(gl_tex_coord2d -> TexCoord2d(s: f64, t: f64));
gl_thin!(gl_tex_coord2i -> TexCoord2i(s: i32, t: i32));
gl_thin!(gl_tex_coord2s -> TexCoord2s(s: i16, t: i16));
gl_thin!(gl_tex_coord3d -> TexCoord3d(s: f64, t: f64, r: f64));
gl_thin!(gl_tex_coord3f -> TexCoord3f(s: f32, t: f32, r: f32));
gl_thin!(gl_tex_coord3i -> TexCoord3i(s: i32, t: i32, r: i32));
gl_thin!(gl_tex_coord3s -> TexCoord3s(s: i16, t: i16, r: i16));
gl_thin!(gl_tex_coord4d -> TexCoord4d(s: f64, t: f64, r: f64, q: f64));
gl_thin!(gl_tex_coord4i -> TexCoord4i(s: i32, t: i32, r: i32, q: i32));

// ===== Additional raster pos variants =====

gl_thin!(gl_raster_pos2d -> RasterPos2d(x: f64, y: f64));
gl_thin!(gl_raster_pos2s -> RasterPos2s(x: i16, y: i16));
gl_thin!(gl_raster_pos3d -> RasterPos3d(x: f64, y: f64, z: f64));
gl_thin!(gl_raster_pos3s -> RasterPos3s(x: i16, y: i16, z: i16));
gl_thin!(gl_raster_pos4d -> RasterPos4d(x: f64, y: f64, z: f64, w: f64));
gl_thin!(gl_raster_pos4s -> RasterPos4s(x: i16, y: i16, z: i16, w: i16));

// ===== Additional transform variants =====

gl_thin!(gl_rotated -> Rotated(angle: f64, x: f64, y: f64, z: f64));
gl_thin!(gl_scaled -> Scaled(x: f64, y: f64, z: f64));
gl_thin!(gl_translated -> Translated(x: f64, y: f64, z: f64));

// ===== Line stipple / draw+read buffer =====

gl_thin!(gl_line_stipple -> LineStipple(factor: i32, pattern: u16));
gl_thin!(gl_draw_buffer -> DrawBuffer(mode: u32));
gl_thin!(gl_read_buffer -> ReadBuffer(mode: u32));

// ===== 1D texture copy/sub =====

pub fn gl_copy_tex_image_1d(
    target: u32,
    level: i32,
    internal_format: u32,
    x: i32,
    y: i32,
    width: i32,
    border: i32,
) {
    unsafe {
        gl_generated::CopyTexImage1D(target, level, internal_format, x, y, width, border);
    }
}

gl_thin!(gl_copy_tex_sub_image_1d -> CopyTexSubImage1D(target: u32, level: i32, xoffset: i32, x: i32, y: i32, width: i32));

pub fn gl_tex_sub_image_1d(
    target: u32,
    level: i32,
    xoffset: i32,
    width: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexSubImage1D(
            target,
            level,
            xoffset,
            width,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

// ===== Display Lists =====

gl_thin!(gl_new_list -> NewList(list: u32, mode: u32));

gl_thin!(gl_end_list -> EndList());

pub fn gl_gen_lists(range: i32) -> u32 {
    unsafe { gl_generated::GenLists(range) }
}

gl_thin!(gl_delete_lists -> DeleteLists(list: u32, range: i32));

pub fn gl_is_list(list: u32) -> bool {
    unsafe { gl_generated::IsList(list) != 0 }
}

gl_thin!(gl_call_list -> CallList(list: u32));

pub fn gl_call_lists(n: i32, list_type: u32, lists: &[u8]) {
    unsafe {
        gl_generated::CallLists(n, list_type, lists.as_ptr() as *const c_void);
    }
}

gl_thin!(gl_list_base -> ListBase(base: u32));

// ===== Lighting =====

gl_thin!(gl_lightf -> Lightf(light: u32, pname: u32, param: f32));

pub fn gl_lightfv(light: u32, pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::Lightfv(light, pname, params.as_ptr());
    }
}

gl_thin!(gl_lighti -> Lighti(light: u32, pname: u32, param: i32));

pub fn gl_lightiv(light: u32, pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::Lightiv(light, pname, params.as_ptr());
    }
}

gl_thin!(gl_light_modelf -> LightModelf(pname: u32, param: f32));

pub fn gl_light_modelfv(pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::LightModelfv(pname, params.as_ptr());
    }
}

gl_thin!(gl_light_modeli -> LightModeli(pname: u32, param: i32));

pub fn gl_light_modeliv(pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::LightModeliv(pname, params.as_ptr());
    }
}

gl_thin!(gl_materialf -> Materialf(face: u32, pname: u32, param: f32));

pub fn gl_materialfv(face: u32, pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::Materialfv(face, pname, params.as_ptr());
    }
}

gl_thin!(gl_materiali -> Materiali(face: u32, pname: u32, param: i32));

pub fn gl_materialiv(face: u32, pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::Materialiv(face, pname, params.as_ptr());
    }
}

gl_thin!(gl_color_material -> ColorMaterial(face: u32, mode: u32));

// ===== Fog =====

gl_thin!(gl_fogf -> Fogf(pname: u32, param: f32));

pub fn gl_fogfv(pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::Fogfv(pname, params.as_ptr());
    }
}

gl_thin!(gl_fogi -> Fogi(pname: u32, param: i32));

pub fn gl_fogiv(pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::Fogiv(pname, params.as_ptr());
    }
}

// ===== Polygon/Drawing =====

gl_thin!(gl_polygon_offset -> PolygonOffset(factor: f32, units: f32));

pub fn gl_polygon_stipple(mask: &[u8]) {
    unsafe {
        gl_generated::PolygonStipple(mask.as_ptr());
    }
}

pub fn gl_get_polygon_stipple(mask: &mut [u8]) {
    unsafe {
        gl_generated::GetPolygonStipple(mask.as_mut_ptr());
    }
}

gl_thin!(gl_logic_op -> LogicOp(opcode: u32));

pub fn gl_draw_pixels(width: i32, height: i32, format: u32, type_: u32, pixels: &[u8]) {
    unsafe {
        gl_generated::DrawPixels(
            width,
            height,
            format,
            type_,
            pixels.as_ptr() as *const c_void,
        );
    }
}

gl_thin!(gl_copy_pixels -> CopyPixels(x: i32, y: i32, width: i32, height: i32, type_: u32));

pub fn gl_bitmap(
    width: i32,
    height: i32,
    xorig: f32,
    yorig: f32,
    xmove: f32,
    ymove: f32,
    bitmap: &[u8],
) {
    unsafe {
        gl_generated::Bitmap(width, height, xorig, yorig, xmove, ymove, bitmap.as_ptr());
    }
}

gl_thin!(gl_pixel_zoom -> PixelZoom(xfactor: f32, yfactor: f32));

gl_thin!(gl_raster_pos2f -> RasterPos2f(x: f32, y: f32));

gl_thin!(gl_raster_pos3f -> RasterPos3f(x: f32, y: f32, z: f32));

gl_thin!(gl_raster_pos4f -> RasterPos4f(x: f32, y: f32, z: f32, w: f32));

gl_thin!(gl_raster_pos2i -> RasterPos2i(x: i32, y: i32));

gl_thin!(gl_raster_pos3i -> RasterPos3i(x: i32, y: i32, z: i32));

gl_thin!(gl_raster_pos4i -> RasterPos4i(x: i32, y: i32, z: i32, w: i32));

// ===== Depth =====

gl_thin!(gl_depth_range -> DepthRange(near: f64, far: f64));

// ===== Texture Environment/Generation =====

gl_thin!(gl_tex_envf -> TexEnvf(target: u32, pname: u32, param: f32));

pub fn gl_tex_envfv(target: u32, pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::TexEnvfv(target, pname, params.as_ptr());
    }
}

gl_thin!(gl_tex_envi -> TexEnvi(target: u32, pname: u32, param: i32));

pub fn gl_tex_enviv(target: u32, pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::TexEnviv(target, pname, params.as_ptr());
    }
}

gl_thin!(gl_tex_geni -> TexGeni(coord: u32, pname: u32, param: i32));

gl_thin!(gl_tex_genf -> TexGenf(coord: u32, pname: u32, param: f32));

gl_thin!(gl_tex_gend -> TexGend(coord: u32, pname: u32, param: f64));

pub fn gl_tex_geniv(coord: u32, pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::TexGeniv(coord, pname, params.as_ptr());
    }
}

pub fn gl_tex_genfv(coord: u32, pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::TexGenfv(coord, pname, params.as_ptr());
    }
}

pub fn gl_tex_gendv(coord: u32, pname: u32, params: &[f64]) {
    unsafe {
        gl_generated::TexGendv(coord, pname, params.as_ptr());
    }
}

pub fn gl_tex_image_1d(
    target: u32,
    level: i32,
    internal_format: i32,
    width: i32,
    border: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexImage1D(
            target,
            level,
            internal_format,
            width,
            border,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_tex_image_1d_null(
    target: u32,
    level: i32,
    internal_format: i32,
    width: i32,
    border: i32,
    format: u32,
    type_: u32,
) {
    unsafe {
        gl_generated::TexImage1D(
            target,
            level,
            internal_format,
            width,
            border,
            format,
            type_,
            ptr::null(),
        );
    }
}

pub fn gl_copy_tex_image_2d(
    target: u32,
    level: i32,
    internal_format: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    border: i32,
) {
    unsafe {
        gl_generated::CopyTexImage2D(target, level, internal_format, x, y, width, height, border);
    }
}

pub fn gl_copy_tex_sub_image_2d(
    target: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    unsafe {
        gl_generated::CopyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height);
    }
}

gl_thin!(gl_tex_parameterf -> TexParameterf(target: u32, pname: u32, param: f32));

pub fn gl_tex_parameterfv(target: u32, pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::TexParameterfv(target, pname, params.as_ptr());
    }
}

pub fn gl_tex_parameteriv(target: u32, pname: u32, params: &[i32]) {
    unsafe {
        gl_generated::TexParameteriv(target, pname, params.as_ptr());
    }
}

gl_thin!(gl_pixel_storef -> PixelStoref(pname: u32, param: f32));

gl_thin!(gl_pixel_transferf -> PixelTransferf(pname: u32, param: f32));

gl_thin!(gl_pixel_transferi -> PixelTransferi(pname: u32, param: i32));

// ===== Vertex Arrays (GL 1.1) =====

gl_thin!(gl_draw_arrays -> DrawArrays(mode: u32, first: i32, count: i32));

/// # Safety
/// The `indices` pointer must be valid for the given `count` and `type_`.
pub unsafe fn gl_draw_elements(mode: u32, count: i32, type_: u32, indices: *const c_void) {
    gl_generated::DrawElements(mode, count, type_, indices);
}

/// # Safety
/// The `pointer` must remain valid while the vertex array is enabled.
pub unsafe fn gl_vertex_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    gl_generated::VertexPointer(size, type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the color array is enabled.
pub unsafe fn gl_color_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    gl_generated::ColorPointer(size, type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the normal array is enabled.
pub unsafe fn gl_normal_pointer(type_: u32, stride: i32, pointer: *const c_void) {
    gl_generated::NormalPointer(type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the texcoord array is enabled.
pub unsafe fn gl_tex_coord_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    gl_generated::TexCoordPointer(size, type_, stride, pointer);
}

gl_thin!(gl_enable_client_state -> EnableClientState(array: u32));

gl_thin!(gl_disable_client_state -> DisableClientState(array: u32));

gl_thin!(gl_array_element -> ArrayElement(i: i32));

// ===== State Queries =====

pub fn gl_get_booleanv(pname: u32, params: &mut [u8]) {
    unsafe {
        gl_generated::GetBooleanv(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_doublev(pname: u32, params: &mut [f64]) {
    unsafe {
        gl_generated::GetDoublev(pname, params.as_mut_ptr());
    }
}

pub fn gl_is_enabled(cap: u32) -> bool {
    unsafe { gl_generated::IsEnabled(cap) != 0 }
}

pub fn gl_get_tex_parameteriv(target: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetTexParameteriv(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_parameterfv(target: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetTexParameterfv(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_level_parameteriv(target: u32, level: i32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetTexLevelParameteriv(target, level, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_level_parameterfv(target: u32, level: i32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetTexLevelParameterfv(target, level, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_image(target: u32, level: i32, format: u32, type_: u32, pixels: &mut [u8]) {
    unsafe {
        gl_generated::GetTexImage(
            target,
            level,
            format,
            type_,
            pixels.as_mut_ptr() as *mut c_void,
        );
    }
}

pub fn gl_get_lightfv(light: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetLightfv(light, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_lightiv(light: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetLightiv(light, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_materialfv(face: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetMaterialfv(face, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_materialiv(face: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetMaterialiv(face, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_clip_plane(plane: u32, equation: &mut [f64; 4]) {
    unsafe {
        gl_generated::GetClipPlane(plane, equation.as_mut_ptr());
    }
}

pub fn gl_clip_plane(plane: u32, equation: &[f64; 4]) {
    unsafe {
        gl_generated::ClipPlane(plane, equation.as_ptr());
    }
}

pub fn gl_is_texture(texture: u32) -> bool {
    unsafe { gl_generated::IsTexture(texture) != 0 }
}

pub fn gl_are_textures_resident(textures: &[u32], residences: &mut [u8]) -> bool {
    unsafe {
        gl_generated::AreTexturesResident(
            textures.len() as i32,
            textures.as_ptr(),
            residences.as_mut_ptr(),
        ) != 0
    }
}

pub fn gl_get_tex_envfv(target: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetTexEnvfv(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_enviv(target: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetTexEnviv(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_gendv(coord: u32, pname: u32, params: &mut [f64]) {
    unsafe {
        gl_generated::GetTexGendv(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_genfv(coord: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        gl_generated::GetTexGenfv(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_geniv(coord: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        gl_generated::GetTexGeniv(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_pixel_mapfv(map: u32, values: &mut [f32]) {
    unsafe {
        gl_generated::GetPixelMapfv(map, values.as_mut_ptr());
    }
}

// ===== Evaluators =====

pub fn gl_map1f(target: u32, u1: f32, u2: f32, stride: i32, order: i32, points: &[f32]) {
    unsafe {
        gl_generated::Map1f(target, u1, u2, stride, order, points.as_ptr());
    }
}

pub fn gl_map1d(target: u32, u1: f64, u2: f64, stride: i32, order: i32, points: &[f64]) {
    unsafe {
        gl_generated::Map1d(target, u1, u2, stride, order, points.as_ptr());
    }
}

pub fn gl_map2f(
    target: u32,
    u1: f32,
    u2: f32,
    ustride: i32,
    uorder: i32,
    v1: f32,
    v2: f32,
    vstride: i32,
    vorder: i32,
    points: &[f32],
) {
    unsafe {
        gl_generated::Map2f(
            target,
            u1,
            u2,
            ustride,
            uorder,
            v1,
            v2,
            vstride,
            vorder,
            points.as_ptr(),
        );
    }
}

pub fn gl_map2d(
    target: u32,
    u1: f64,
    u2: f64,
    ustride: i32,
    uorder: i32,
    v1: f64,
    v2: f64,
    vstride: i32,
    vorder: i32,
    points: &[f64],
) {
    unsafe {
        gl_generated::Map2d(
            target,
            u1,
            u2,
            ustride,
            uorder,
            v1,
            v2,
            vstride,
            vorder,
            points.as_ptr(),
        );
    }
}

gl_thin!(gl_eval_coord1f -> EvalCoord1f(u: f32));

gl_thin!(gl_eval_coord1d -> EvalCoord1d(u: f64));

gl_thin!(gl_eval_coord2f -> EvalCoord2f(u: f32, v: f32));

gl_thin!(gl_eval_coord2d -> EvalCoord2d(u: f64, v: f64));

gl_thin!(gl_map_grid1f -> MapGrid1f(un: i32, u1: f32, u2: f32));

gl_thin!(gl_map_grid1d -> MapGrid1d(un: i32, u1: f64, u2: f64));

gl_thin!(gl_map_grid2f -> MapGrid2f(un: i32, u1: f32, u2: f32, vn: i32, v1: f32, v2: f32));

gl_thin!(gl_map_grid2d -> MapGrid2d(un: i32, u1: f64, u2: f64, vn: i32, v1: f64, v2: f64));

gl_thin!(gl_eval_mesh1 -> EvalMesh1(mode: u32, i1: i32, i2: i32));

gl_thin!(gl_eval_mesh2 -> EvalMesh2(mode: u32, i1: i32, i2: i32, j1: i32, j2: i32));

gl_thin!(gl_eval_point1 -> EvalPoint1(i: i32));

gl_thin!(gl_eval_point2 -> EvalPoint2(i: i32, j: i32));

// ===== Accumulation =====

gl_thin!(gl_accum -> Accum(op: u32, value: f32));

gl_thin!(gl_clear_accum -> ClearAccum(r: f32, g: f32, b: f32, a: f32));

// ===== Selection/Feedback =====

pub fn gl_render_mode(mode: u32) -> i32 {
    unsafe { gl_generated::RenderMode(mode) }
}

gl_thin!(gl_init_names -> InitNames());

gl_thin!(gl_push_name -> PushName(name: u32));

gl_thin!(gl_pop_name -> PopName());

gl_thin!(gl_load_name -> LoadName(name: u32));

gl_thin!(gl_pass_through -> PassThrough(token: f32));

gl_thin!(gl_push_attrib -> PushAttrib(mask: u32));

gl_thin!(gl_pop_attrib -> PopAttrib());

pub fn gl_pixel_mapfv(map: u32, map_size: i32, values: &[f32]) {
    unsafe {
        gl_generated::PixelMapfv(map, map_size, values.as_ptr());
    }
}

pub fn gl_pixel_mapuiv(map: u32, map_size: i32, values: &[u32]) {
    unsafe {
        gl_generated::PixelMapuiv(map, map_size, values.as_ptr());
    }
}

pub fn gl_pixel_mapusv(map: u32, map_size: i32, values: &[u16]) {
    unsafe {
        gl_generated::PixelMapusv(map, map_size, values.as_ptr());
    }
}

// ===== GL 1.2 Optional =====

/// `glTexImage3D` -- available only if GL 1.2+ is supported.  No-op if missing.
pub fn gl_tex_image_3d(
    target: u32,
    level: i32,
    internal_format: i32,
    width: i32,
    height: i32,
    depth: i32,
    border: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexImage3D(
            target,
            level,
            internal_format,
            width,
            height,
            depth,
            border,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_tex_sub_image_3d(
    target: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    zoffset: i32,
    width: i32,
    height: i32,
    depth: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    unsafe {
        gl_generated::TexSubImage3D(
            target,
            level,
            xoffset,
            yoffset,
            zoffset,
            width,
            height,
            depth,
            format,
            type_,
            data.as_ptr() as *const c_void,
        );
    }
}

// ===== GL 1.3 Optional =====

gl_thin!(gl_active_texture -> ActiveTexture(texture: u32));

gl_thin!(gl_multi_tex_coord1f -> MultiTexCoord1f(target: u32, s: f32));

gl_thin!(gl_multi_tex_coord2f -> MultiTexCoord2f(target: u32, s: f32, t: f32));

gl_thin!(gl_multi_tex_coord3f -> MultiTexCoord3f(target: u32, s: f32, t: f32, r: f32));

gl_thin!(gl_multi_tex_coord4f -> MultiTexCoord4f(target: u32, s: f32, t: f32, r: f32, q: f32));

gl_thin!(gl_sample_coverage -> SampleCoverage(value: f32, invert: u8));

pub fn gl_compressed_tex_image_1d(
    target: u32,
    level: i32,
    internalformat: u32,
    width: i32,
    border: i32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexImage1D(
            target,
            level,
            internalformat,
            width,
            border,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_compressed_tex_image_2d(
    target: u32,
    level: i32,
    internalformat: u32,
    width: i32,
    height: i32,
    border: i32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_compressed_tex_image_3d(
    target: u32,
    level: i32,
    internalformat: u32,
    width: i32,
    height: i32,
    depth: i32,
    border: i32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexImage3D(
            target,
            level,
            internalformat,
            width,
            height,
            depth,
            border,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_compressed_tex_sub_image_1d(
    target: u32,
    level: i32,
    xoffset: i32,
    width: i32,
    format: u32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexSubImage1D(
            target,
            level,
            xoffset,
            width,
            format,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_compressed_tex_sub_image_2d(
    target: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    width: i32,
    height: i32,
    format: u32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexSubImage2D(
            target,
            level,
            xoffset,
            yoffset,
            width,
            height,
            format,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_compressed_tex_sub_image_3d(
    target: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    zoffset: i32,
    width: i32,
    height: i32,
    depth: i32,
    format: u32,
    image_size: i32,
    data: &[u8],
) {
    unsafe {
        gl_generated::CompressedTexSubImage3D(
            target,
            level,
            xoffset,
            yoffset,
            zoffset,
            width,
            height,
            depth,
            format,
            image_size,
            data.as_ptr() as *const c_void,
        );
    }
}

// ===== GL 1.4 Optional =====

gl_thin!(gl_secondary_color3f -> SecondaryColor3f(r: f32, g: f32, b: f32));

gl_thin!(gl_secondary_color3ub -> SecondaryColor3ub(r: u8, g: u8, b: u8));

gl_thin!(gl_window_pos2f -> WindowPos2f(x: f32, y: f32));

gl_thin!(gl_window_pos3f -> WindowPos3f(x: f32, y: f32, z: f32));

gl_thin!(gl_fog_coordf -> FogCoordf(coord: f32));

gl_thin!(gl_fog_coordd -> FogCoordd(coord: f64));

gl_thin!(gl_point_parameterf -> PointParameterf(pname: u32, param: f32));

pub fn gl_point_parameterfv(pname: u32, params: &[f32]) {
    unsafe {
        gl_generated::PointParameterfv(pname, params.as_ptr());
    }
}

gl_thin!(gl_point_parameteri -> PointParameteri(pname: u32, param: i32));

// ===== GL 1.4 Blend (Optional) =====

gl_thin!(gl_blend_equation -> BlendEquation(mode: u32));

gl_thin!(gl_blend_func_separate -> BlendFuncSeparate(src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32));

gl_thin!(gl_blend_color -> BlendColor(r: f32, g: f32, b: f32, a: f32));

// ===== GL 2.0 Stencil Separate (Optional) =====

gl_thin!(gl_stencil_func_separate -> StencilFuncSeparate(face: u32, func: u32, ref_: i32, mask: u32));

gl_thin!(gl_stencil_op_separate -> StencilOpSeparate(face: u32, sfail: u32, dpfail: u32, dppass: u32));

gl_thin!(gl_stencil_mask_separate -> StencilMaskSeparate(face: u32, mask: u32));

// ===== Imaging Subset (Optional) =====
//
// Most of the ARB_imaging legacy entry points (`ColorTable`,
// `ConvolutionParameter*`, `Histogram`, `Minmax`) were removed from
// the OpenGL spec in 3.x and are not present in modern drivers,
// including OSMesa. The GLX dispatcher still routes their opcodes
// here but the wrappers are no-ops — clients see "command issued
// successfully" with no visible effect.

pub fn gl_color_table(
    _target: u32,
    _internal_format: u32,
    _width: i32,
    _format: u32,
    _type_: u32,
    _data: &[u8],
) {
}

// Empty stubs for imaging-subset extensions we don't implement.
// Calling these is a no-op; GLX clients that hit them will draw nothing
// but won't crash the server.
pub fn gl_convolution_parameterf(_target: u32, _pname: u32, _param: f32) {}
pub fn gl_convolution_parameterfv(_target: u32, _pname: u32, _params: &[f32]) {}
pub fn gl_convolution_parameteri(_target: u32, _pname: u32, _param: i32) {}
pub fn gl_convolution_parameteriv(_target: u32, _pname: u32, _params: &[i32]) {}
pub fn gl_histogram(_target: u32, _width: i32, _internal_format: u32, _sink: u8) {}
pub fn gl_minmax(_target: u32, _internal_format: u32, _sink: u8) {}
