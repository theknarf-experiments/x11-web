//! Safe Rust wrappers around individual GL function calls.
//!
//! Each function delegates to the dynamically-resolved function pointer table
//! loaded by the parent `osmesa` module.

use std::ffi::{c_void, CStr};
use std::ptr;

use super::fns;

// ===== Original GL 1.0-1.1 wrappers (unchanged API) =====

/// Execute a GL Clear call.
pub fn gl_clear(mask: u32) {
    unsafe {
        (fns().clear)(mask);
    }
}

/// Execute a GL ClearColor call.
pub fn gl_clear_color(r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        (fns().clear_color)(r, g, b, a);
    }
}

/// Execute a GL Viewport call.
pub fn gl_viewport(x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        (fns().viewport)(x, y, w, h);
    }
}

pub fn gl_begin(mode: u32) {
    unsafe {
        (fns().begin)(mode);
    }
}

pub fn gl_end() {
    unsafe {
        (fns().end)();
    }
}

pub fn gl_vertex2f(x: f32, y: f32) {
    unsafe {
        (fns().vertex2f)(x, y);
    }
}

pub fn gl_vertex3f(x: f32, y: f32, z: f32) {
    unsafe {
        (fns().vertex3f)(x, y, z);
    }
}

pub fn gl_vertex4f(x: f32, y: f32, z: f32, w: f32) {
    unsafe {
        (fns().vertex4f)(x, y, z, w);
    }
}

pub fn gl_vertex2i(x: i32, y: i32) {
    unsafe {
        (fns().vertex2i)(x, y);
    }
}

pub fn gl_vertex3i(x: i32, y: i32, z: i32) {
    unsafe {
        (fns().vertex3i)(x, y, z);
    }
}

pub fn gl_color3f(r: f32, g: f32, b: f32) {
    unsafe {
        (fns().color3f)(r, g, b);
    }
}

pub fn gl_color4f(r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        (fns().color4f)(r, g, b, a);
    }
}

pub fn gl_color3ub(r: u8, g: u8, b: u8) {
    unsafe {
        (fns().color3ub)(r, g, b);
    }
}

pub fn gl_color4ub(r: u8, g: u8, b: u8, a: u8) {
    unsafe {
        (fns().color4ub)(r, g, b, a);
    }
}

pub fn gl_flush() {
    unsafe {
        (fns().flush)();
    }
}

pub fn gl_finish() {
    unsafe {
        (fns().finish)();
    }
}

pub fn gl_enable(cap: u32) {
    unsafe {
        (fns().enable)(cap);
    }
}

pub fn gl_disable(cap: u32) {
    unsafe {
        (fns().disable)(cap);
    }
}

pub fn gl_gen_textures(n: i32, textures: &mut [u32]) {
    unsafe {
        (fns().gen_textures)(n, textures.as_mut_ptr());
    }
}

pub fn gl_delete_textures(textures: &[u32]) {
    unsafe {
        (fns().delete_textures)(textures.len() as i32, textures.as_ptr());
    }
}

pub fn gl_bind_texture(target: u32, texture: u32) {
    unsafe {
        (fns().bind_texture)(target, texture);
    }
}

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
        (fns().tex_image_2d)(
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
        (fns().tex_image_2d)(
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

pub fn gl_tex_parameteri(target: u32, pname: u32, param: i32) {
    unsafe {
        (fns().tex_parameteri)(target, pname, param);
    }
}

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
        (fns().tex_sub_image_2d)(
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

pub fn gl_scissor(x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        (fns().scissor)(x, y, w, h);
    }
}

pub fn gl_blend_func(sfactor: u32, dfactor: u32) {
    unsafe {
        (fns().blend_func)(sfactor, dfactor);
    }
}

pub fn gl_depth_func(func: u32) {
    unsafe {
        (fns().depth_func)(func);
    }
}

pub fn gl_depth_mask(flag: u8) {
    unsafe {
        (fns().depth_mask)(flag);
    }
}

pub fn gl_color_mask(r: u8, g: u8, b: u8, a: u8) {
    unsafe {
        (fns().color_mask)(r, g, b, a);
    }
}

pub fn gl_stencil_func(func: u32, ref_: i32, mask: u32) {
    unsafe {
        (fns().stencil_func)(func, ref_, mask);
    }
}

pub fn gl_stencil_op(fail: u32, zfail: u32, zpass: u32) {
    unsafe {
        (fns().stencil_op)(fail, zfail, zpass);
    }
}

pub fn gl_stencil_mask(mask: u32) {
    unsafe {
        (fns().stencil_mask)(mask);
    }
}

pub fn gl_matrix_mode(mode: u32) {
    unsafe {
        (fns().matrix_mode)(mode);
    }
}

pub fn gl_load_identity() {
    unsafe {
        (fns().load_identity)();
    }
}

pub fn gl_load_matrixf(m: &[f32; 16]) {
    unsafe {
        (fns().load_matrixf)(m.as_ptr());
    }
}

pub fn gl_load_matrixd(m: &[f64; 16]) {
    unsafe {
        (fns().load_matrixd)(m.as_ptr());
    }
}

pub fn gl_mult_matrixf(m: &[f32; 16]) {
    unsafe {
        (fns().mult_matrixf)(m.as_ptr());
    }
}

pub fn gl_mult_matrixd(m: &[f64; 16]) {
    unsafe {
        (fns().mult_matrixd)(m.as_ptr());
    }
}

pub fn gl_push_matrix() {
    unsafe {
        (fns().push_matrix)();
    }
}

pub fn gl_pop_matrix() {
    unsafe {
        (fns().pop_matrix)();
    }
}

pub fn gl_ortho(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) {
    unsafe {
        (fns().ortho)(left, right, bottom, top, near, far);
    }
}

pub fn gl_frustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) {
    unsafe {
        (fns().frustum)(left, right, bottom, top, near, far);
    }
}

pub fn gl_rotatef(angle: f32, x: f32, y: f32, z: f32) {
    unsafe {
        (fns().rotatef)(angle, x, y, z);
    }
}

pub fn gl_scalef(x: f32, y: f32, z: f32) {
    unsafe {
        (fns().scalef)(x, y, z);
    }
}

pub fn gl_translatef(x: f32, y: f32, z: f32) {
    unsafe {
        (fns().translatef)(x, y, z);
    }
}

pub fn gl_normal3f(nx: f32, ny: f32, nz: f32) {
    unsafe {
        (fns().normal3f)(nx, ny, nz);
    }
}

pub fn gl_tex_coord2f(s: f32, t: f32) {
    unsafe {
        (fns().tex_coord2f)(s, t);
    }
}

pub fn gl_tex_coord4f(s: f32, t: f32, r: f32, q: f32) {
    unsafe {
        (fns().tex_coord4f)(s, t, r, q);
    }
}

pub fn gl_pixel_storei(pname: u32, param: i32) {
    unsafe {
        (fns().pixel_storei)(pname, param);
    }
}

pub fn gl_line_width(width: f32) {
    unsafe {
        (fns().line_width)(width);
    }
}

pub fn gl_point_size(size: f32) {
    unsafe {
        (fns().point_size)(size);
    }
}

pub fn gl_polygon_mode(face: u32, mode: u32) {
    unsafe {
        (fns().polygon_mode)(face, mode);
    }
}

pub fn gl_cull_face(mode: u32) {
    unsafe {
        (fns().cull_face)(mode);
    }
}

pub fn gl_front_face(mode: u32) {
    unsafe {
        (fns().front_face)(mode);
    }
}

pub fn gl_shade_model(mode: u32) {
    unsafe {
        (fns().shade_model)(mode);
    }
}

pub fn gl_clear_depth(depth: f64) {
    unsafe {
        (fns().clear_depth)(depth);
    }
}

pub fn gl_clear_stencil(s: i32) {
    unsafe {
        (fns().clear_stencil)(s);
    }
}

pub fn gl_alpha_func(func: u32, ref_: f32) {
    unsafe {
        (fns().alpha_func)(func, ref_);
    }
}

pub fn gl_hint(target: u32, mode: u32) {
    unsafe {
        (fns().hint)(target, mode);
    }
}

pub fn gl_get_integerv(pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_integerv)(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_floatv(pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_floatv)(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_error() -> u32 {
    unsafe { (fns().get_error)() }
}

pub fn gl_get_string(name: u32) -> String {
    let ptr = unsafe { (fns().get_string)(name) };
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        CStr::from_ptr(ptr as *const std::ffi::c_char)
            .to_string_lossy()
            .into_owned()
    }
}

pub fn gl_rectf(x1: f32, y1: f32, x2: f32, y2: f32) {
    unsafe {
        (fns().rectf)(x1, y1, x2, y2);
    }
}

pub fn gl_recti(x1: i32, y1: i32, x2: i32, y2: i32) {
    unsafe {
        (fns().recti)(x1, y1, x2, y2);
    }
}

pub fn gl_rectd(x1: f64, y1: f64, x2: f64, y2: f64) {
    unsafe {
        (fns().rectd)(x1, y1, x2, y2);
    }
}

pub fn gl_rects(x1: i16, y1: i16, x2: i16, y2: i16) {
    unsafe {
        (fns().rects)(x1, y1, x2, y2);
    }
}

// ===== Additional color variants =====

pub fn gl_color3b(r: i8, g: i8, b: i8) {
    unsafe {
        (fns().color3b)(r, g, b);
    }
}
pub fn gl_color3d(r: f64, g: f64, b: f64) {
    unsafe {
        (fns().color3d)(r, g, b);
    }
}
pub fn gl_color3i(r: i32, g: i32, b: i32) {
    unsafe {
        (fns().color3i)(r, g, b);
    }
}
pub fn gl_color3s(r: i16, g: i16, b: i16) {
    unsafe {
        (fns().color3s)(r, g, b);
    }
}
pub fn gl_color3ui(r: u32, g: u32, b: u32) {
    unsafe {
        (fns().color3ui)(r, g, b);
    }
}
pub fn gl_color3us(r: u16, g: u16, b: u16) {
    unsafe {
        (fns().color3us)(r, g, b);
    }
}
pub fn gl_color4b(r: i8, g: i8, b: i8, a: i8) {
    unsafe {
        (fns().color4b)(r, g, b, a);
    }
}
pub fn gl_color4d(r: f64, g: f64, b: f64, a: f64) {
    unsafe {
        (fns().color4d)(r, g, b, a);
    }
}
pub fn gl_color4i(r: i32, g: i32, b: i32, a: i32) {
    unsafe {
        (fns().color4i)(r, g, b, a);
    }
}
pub fn gl_color4s(r: i16, g: i16, b: i16, a: i16) {
    unsafe {
        (fns().color4s)(r, g, b, a);
    }
}
pub fn gl_color4ui(r: u32, g: u32, b: u32, a: u32) {
    unsafe {
        (fns().color4ui)(r, g, b, a);
    }
}
pub fn gl_color4us(r: u16, g: u16, b: u16, a: u16) {
    unsafe {
        (fns().color4us)(r, g, b, a);
    }
}

// ===== Edge flag / Index / ClearIndex =====

pub fn gl_edge_flag(flag: u8) {
    unsafe {
        (fns().edge_flag)(flag);
    }
}
pub fn gl_indexd(c: f64) {
    unsafe {
        (fns().indexd)(c);
    }
}
pub fn gl_indexf(c: f32) {
    unsafe {
        (fns().indexf)(c);
    }
}
pub fn gl_indexi(c: i32) {
    unsafe {
        (fns().indexi)(c);
    }
}
pub fn gl_indexs(c: i16) {
    unsafe {
        (fns().indexs)(c);
    }
}
pub fn gl_indexub(c: u8) {
    unsafe {
        (fns().indexub)(c);
    }
}
pub fn gl_index_mask(mask: u32) {
    unsafe {
        (fns().index_mask)(mask);
    }
}
pub fn gl_clear_index(c: f32) {
    unsafe {
        (fns().clear_index)(c);
    }
}

// ===== Additional normal variants =====

pub fn gl_normal3b(nx: i8, ny: i8, nz: i8) {
    unsafe {
        (fns().normal3b)(nx, ny, nz);
    }
}
pub fn gl_normal3d(nx: f64, ny: f64, nz: f64) {
    unsafe {
        (fns().normal3d)(nx, ny, nz);
    }
}
pub fn gl_normal3i(nx: i32, ny: i32, nz: i32) {
    unsafe {
        (fns().normal3i)(nx, ny, nz);
    }
}
pub fn gl_normal3s(nx: i16, ny: i16, nz: i16) {
    unsafe {
        (fns().normal3s)(nx, ny, nz);
    }
}

// ===== Additional vertex variants =====

pub fn gl_vertex2d(x: f64, y: f64) {
    unsafe {
        (fns().vertex2d)(x, y);
    }
}
pub fn gl_vertex2s(x: i16, y: i16) {
    unsafe {
        (fns().vertex2s)(x, y);
    }
}
pub fn gl_vertex3d(x: f64, y: f64, z: f64) {
    unsafe {
        (fns().vertex3d)(x, y, z);
    }
}
pub fn gl_vertex3s(x: i16, y: i16, z: i16) {
    unsafe {
        (fns().vertex3s)(x, y, z);
    }
}
pub fn gl_vertex4d(x: f64, y: f64, z: f64, w: f64) {
    unsafe {
        (fns().vertex4d)(x, y, z, w);
    }
}
pub fn gl_vertex4i(x: i32, y: i32, z: i32, w: i32) {
    unsafe {
        (fns().vertex4i)(x, y, z, w);
    }
}
pub fn gl_vertex4s(x: i16, y: i16, z: i16, w: i16) {
    unsafe {
        (fns().vertex4s)(x, y, z, w);
    }
}

// ===== Additional texcoord variants =====

pub fn gl_tex_coord1d(s: f64) {
    unsafe {
        (fns().tex_coord1d)(s);
    }
}
pub fn gl_tex_coord1f(s: f32) {
    unsafe {
        (fns().tex_coord1f)(s);
    }
}
pub fn gl_tex_coord1i(s: i32) {
    unsafe {
        (fns().tex_coord1i)(s);
    }
}
pub fn gl_tex_coord1s(s: i16) {
    unsafe {
        (fns().tex_coord1s)(s);
    }
}
pub fn gl_tex_coord2d(s: f64, t: f64) {
    unsafe {
        (fns().tex_coord2d)(s, t);
    }
}
pub fn gl_tex_coord2i(s: i32, t: i32) {
    unsafe {
        (fns().tex_coord2i)(s, t);
    }
}
pub fn gl_tex_coord2s(s: i16, t: i16) {
    unsafe {
        (fns().tex_coord2s)(s, t);
    }
}
pub fn gl_tex_coord3d(s: f64, t: f64, r: f64) {
    unsafe {
        (fns().tex_coord3d)(s, t, r);
    }
}
pub fn gl_tex_coord3f(s: f32, t: f32, r: f32) {
    unsafe {
        (fns().tex_coord3f)(s, t, r);
    }
}
pub fn gl_tex_coord3i(s: i32, t: i32, r: i32) {
    unsafe {
        (fns().tex_coord3i)(s, t, r);
    }
}
pub fn gl_tex_coord3s(s: i16, t: i16, r: i16) {
    unsafe {
        (fns().tex_coord3s)(s, t, r);
    }
}
pub fn gl_tex_coord4d(s: f64, t: f64, r: f64, q: f64) {
    unsafe {
        (fns().tex_coord4d)(s, t, r, q);
    }
}
pub fn gl_tex_coord4i(s: i32, t: i32, r: i32, q: i32) {
    unsafe {
        (fns().tex_coord4i)(s, t, r, q);
    }
}

// ===== Additional raster pos variants =====

pub fn gl_raster_pos2d(x: f64, y: f64) {
    unsafe {
        (fns().raster_pos2d)(x, y);
    }
}
pub fn gl_raster_pos2s(x: i16, y: i16) {
    unsafe {
        (fns().raster_pos2s)(x, y);
    }
}
pub fn gl_raster_pos3d(x: f64, y: f64, z: f64) {
    unsafe {
        (fns().raster_pos3d)(x, y, z);
    }
}
pub fn gl_raster_pos3s(x: i16, y: i16, z: i16) {
    unsafe {
        (fns().raster_pos3s)(x, y, z);
    }
}
pub fn gl_raster_pos4d(x: f64, y: f64, z: f64, w: f64) {
    unsafe {
        (fns().raster_pos4d)(x, y, z, w);
    }
}
pub fn gl_raster_pos4s(x: i16, y: i16, z: i16, w: i16) {
    unsafe {
        (fns().raster_pos4s)(x, y, z, w);
    }
}

// ===== Additional transform variants =====

pub fn gl_rotated(angle: f64, x: f64, y: f64, z: f64) {
    unsafe {
        (fns().rotated)(angle, x, y, z);
    }
}
pub fn gl_scaled(x: f64, y: f64, z: f64) {
    unsafe {
        (fns().scaled)(x, y, z);
    }
}
pub fn gl_translated(x: f64, y: f64, z: f64) {
    unsafe {
        (fns().translated)(x, y, z);
    }
}

// ===== Line stipple / draw+read buffer =====

pub fn gl_line_stipple(factor: i32, pattern: u16) {
    unsafe {
        (fns().line_stipple)(factor, pattern);
    }
}
pub fn gl_draw_buffer(mode: u32) {
    unsafe {
        (fns().draw_buffer)(mode);
    }
}
pub fn gl_read_buffer(mode: u32) {
    unsafe {
        (fns().read_buffer)(mode);
    }
}

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
        (fns().copy_tex_image_1d)(target, level, internal_format, x, y, width, border);
    }
}

pub fn gl_copy_tex_sub_image_1d(target: u32, level: i32, xoffset: i32, x: i32, y: i32, width: i32) {
    unsafe {
        (fns().copy_tex_sub_image_1d)(target, level, xoffset, x, y, width);
    }
}

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
        (fns().tex_sub_image_1d)(
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

pub fn gl_new_list(list: u32, mode: u32) {
    unsafe {
        (fns().new_list)(list, mode);
    }
}

pub fn gl_end_list() {
    unsafe {
        (fns().end_list)();
    }
}

pub fn gl_gen_lists(range: i32) -> u32 {
    unsafe { (fns().gen_lists)(range) }
}

pub fn gl_delete_lists(list: u32, range: i32) {
    unsafe {
        (fns().delete_lists)(list, range);
    }
}

pub fn gl_is_list(list: u32) -> bool {
    unsafe { (fns().is_list)(list) != 0 }
}

pub fn gl_call_list(list: u32) {
    unsafe {
        (fns().call_list)(list);
    }
}

pub fn gl_call_lists(n: i32, list_type: u32, lists: &[u8]) {
    unsafe {
        (fns().call_lists)(n, list_type, lists.as_ptr());
    }
}

pub fn gl_list_base(base: u32) {
    unsafe {
        (fns().list_base)(base);
    }
}

// ===== Lighting =====

pub fn gl_lightf(light: u32, pname: u32, param: f32) {
    unsafe {
        (fns().lightf)(light, pname, param);
    }
}

pub fn gl_lightfv(light: u32, pname: u32, params: &[f32]) {
    unsafe {
        (fns().lightfv)(light, pname, params.as_ptr());
    }
}

pub fn gl_lighti(light: u32, pname: u32, param: i32) {
    unsafe {
        (fns().lighti)(light, pname, param);
    }
}

pub fn gl_lightiv(light: u32, pname: u32, params: &[i32]) {
    unsafe {
        (fns().lightiv)(light, pname, params.as_ptr());
    }
}

pub fn gl_light_modelf(pname: u32, param: f32) {
    unsafe {
        (fns().light_modelf)(pname, param);
    }
}

pub fn gl_light_modelfv(pname: u32, params: &[f32]) {
    unsafe {
        (fns().light_modelfv)(pname, params.as_ptr());
    }
}

pub fn gl_light_modeli(pname: u32, param: i32) {
    unsafe {
        (fns().light_modeli)(pname, param);
    }
}

pub fn gl_light_modeliv(pname: u32, params: &[i32]) {
    unsafe {
        (fns().light_modeliv)(pname, params.as_ptr());
    }
}

pub fn gl_materialf(face: u32, pname: u32, param: f32) {
    unsafe {
        (fns().materialf)(face, pname, param);
    }
}

pub fn gl_materialfv(face: u32, pname: u32, params: &[f32]) {
    unsafe {
        (fns().materialfv)(face, pname, params.as_ptr());
    }
}

pub fn gl_materiali(face: u32, pname: u32, param: i32) {
    unsafe {
        (fns().materiali)(face, pname, param);
    }
}

pub fn gl_materialiv(face: u32, pname: u32, params: &[i32]) {
    unsafe {
        (fns().materialiv)(face, pname, params.as_ptr());
    }
}

pub fn gl_color_material(face: u32, mode: u32) {
    unsafe {
        (fns().color_material)(face, mode);
    }
}

// ===== Fog =====

pub fn gl_fogf(pname: u32, param: f32) {
    unsafe {
        (fns().fogf)(pname, param);
    }
}

pub fn gl_fogfv(pname: u32, params: &[f32]) {
    unsafe {
        (fns().fogfv)(pname, params.as_ptr());
    }
}

pub fn gl_fogi(pname: u32, param: i32) {
    unsafe {
        (fns().fogi)(pname, param);
    }
}

pub fn gl_fogiv(pname: u32, params: &[i32]) {
    unsafe {
        (fns().fogiv)(pname, params.as_ptr());
    }
}

// ===== Polygon/Drawing =====

pub fn gl_polygon_offset(factor: f32, units: f32) {
    unsafe {
        (fns().polygon_offset)(factor, units);
    }
}

pub fn gl_polygon_stipple(mask: &[u8]) {
    unsafe {
        (fns().polygon_stipple)(mask.as_ptr());
    }
}

pub fn gl_get_polygon_stipple(mask: &mut [u8]) {
    unsafe {
        (fns().get_polygon_stipple)(mask.as_mut_ptr());
    }
}

pub fn gl_logic_op(opcode: u32) {
    unsafe {
        (fns().logic_op)(opcode);
    }
}

pub fn gl_draw_pixels(width: i32, height: i32, format: u32, type_: u32, pixels: &[u8]) {
    unsafe {
        (fns().draw_pixels)(
            width,
            height,
            format,
            type_,
            pixels.as_ptr() as *const c_void,
        );
    }
}

pub fn gl_copy_pixels(x: i32, y: i32, width: i32, height: i32, type_: u32) {
    unsafe {
        (fns().copy_pixels)(x, y, width, height, type_);
    }
}

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
        (fns().bitmap)(width, height, xorig, yorig, xmove, ymove, bitmap.as_ptr());
    }
}

pub fn gl_pixel_zoom(xfactor: f32, yfactor: f32) {
    unsafe {
        (fns().pixel_zoom)(xfactor, yfactor);
    }
}

pub fn gl_raster_pos2f(x: f32, y: f32) {
    unsafe {
        (fns().raster_pos2f)(x, y);
    }
}

pub fn gl_raster_pos3f(x: f32, y: f32, z: f32) {
    unsafe {
        (fns().raster_pos3f)(x, y, z);
    }
}

pub fn gl_raster_pos4f(x: f32, y: f32, z: f32, w: f32) {
    unsafe {
        (fns().raster_pos4f)(x, y, z, w);
    }
}

pub fn gl_raster_pos2i(x: i32, y: i32) {
    unsafe {
        (fns().raster_pos2i)(x, y);
    }
}

pub fn gl_raster_pos3i(x: i32, y: i32, z: i32) {
    unsafe {
        (fns().raster_pos3i)(x, y, z);
    }
}

pub fn gl_raster_pos4i(x: i32, y: i32, z: i32, w: i32) {
    unsafe {
        (fns().raster_pos4i)(x, y, z, w);
    }
}

// ===== Depth =====

pub fn gl_depth_range(near: f64, far: f64) {
    unsafe {
        (fns().depth_range)(near, far);
    }
}

// ===== Texture Environment/Generation =====

pub fn gl_tex_envf(target: u32, pname: u32, param: f32) {
    unsafe {
        (fns().tex_envf)(target, pname, param);
    }
}

pub fn gl_tex_envfv(target: u32, pname: u32, params: &[f32]) {
    unsafe {
        (fns().tex_envfv)(target, pname, params.as_ptr());
    }
}

pub fn gl_tex_envi(target: u32, pname: u32, param: i32) {
    unsafe {
        (fns().tex_envi)(target, pname, param);
    }
}

pub fn gl_tex_enviv(target: u32, pname: u32, params: &[i32]) {
    unsafe {
        (fns().tex_enviv)(target, pname, params.as_ptr());
    }
}

pub fn gl_tex_geni(coord: u32, pname: u32, param: i32) {
    unsafe {
        (fns().tex_geni)(coord, pname, param);
    }
}

pub fn gl_tex_genf(coord: u32, pname: u32, param: f32) {
    unsafe {
        (fns().tex_genf)(coord, pname, param);
    }
}

pub fn gl_tex_gend(coord: u32, pname: u32, param: f64) {
    unsafe {
        (fns().tex_gend)(coord, pname, param);
    }
}

pub fn gl_tex_geniv(coord: u32, pname: u32, params: &[i32]) {
    unsafe {
        (fns().tex_geniv)(coord, pname, params.as_ptr());
    }
}

pub fn gl_tex_genfv(coord: u32, pname: u32, params: &[f32]) {
    unsafe {
        (fns().tex_genfv)(coord, pname, params.as_ptr());
    }
}

pub fn gl_tex_gendv(coord: u32, pname: u32, params: &[f64]) {
    unsafe {
        (fns().tex_gendv)(coord, pname, params.as_ptr());
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
        (fns().tex_image_1d)(
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
        (fns().tex_image_1d)(
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
        (fns().copy_tex_image_2d)(target, level, internal_format, x, y, width, height, border);
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
        (fns().copy_tex_sub_image_2d)(target, level, xoffset, yoffset, x, y, width, height);
    }
}

pub fn gl_tex_parameterf(target: u32, pname: u32, param: f32) {
    unsafe {
        (fns().tex_parameterf)(target, pname, param);
    }
}

pub fn gl_tex_parameterfv(target: u32, pname: u32, params: &[f32]) {
    unsafe {
        (fns().tex_parameterfv)(target, pname, params.as_ptr());
    }
}

pub fn gl_tex_parameteriv(target: u32, pname: u32, params: &[i32]) {
    unsafe {
        (fns().tex_parameteriv)(target, pname, params.as_ptr());
    }
}

pub fn gl_pixel_storef(pname: u32, param: f32) {
    unsafe {
        (fns().pixel_storef)(pname, param);
    }
}

pub fn gl_pixel_transferf(pname: u32, param: f32) {
    unsafe {
        (fns().pixel_transferf)(pname, param);
    }
}

pub fn gl_pixel_transferi(pname: u32, param: i32) {
    unsafe {
        (fns().pixel_transferi)(pname, param);
    }
}

// ===== Vertex Arrays (GL 1.1) =====

pub fn gl_draw_arrays(mode: u32, first: i32, count: i32) {
    unsafe {
        (fns().draw_arrays)(mode, first, count);
    }
}

/// # Safety
/// The `indices` pointer must be valid for the given `count` and `type_`.
pub unsafe fn gl_draw_elements(mode: u32, count: i32, type_: u32, indices: *const c_void) {
    (fns().draw_elements)(mode, count, type_, indices);
}

/// # Safety
/// The `pointer` must remain valid while the vertex array is enabled.
pub unsafe fn gl_vertex_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    (fns().vertex_pointer)(size, type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the color array is enabled.
pub unsafe fn gl_color_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    (fns().color_pointer)(size, type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the normal array is enabled.
pub unsafe fn gl_normal_pointer(type_: u32, stride: i32, pointer: *const c_void) {
    (fns().normal_pointer)(type_, stride, pointer);
}

/// # Safety
/// The `pointer` must remain valid while the texcoord array is enabled.
pub unsafe fn gl_tex_coord_pointer(size: i32, type_: u32, stride: i32, pointer: *const c_void) {
    (fns().tex_coord_pointer)(size, type_, stride, pointer);
}

pub fn gl_enable_client_state(array: u32) {
    unsafe {
        (fns().enable_client_state)(array);
    }
}

pub fn gl_disable_client_state(array: u32) {
    unsafe {
        (fns().disable_client_state)(array);
    }
}

pub fn gl_array_element(i: i32) {
    unsafe {
        (fns().array_element)(i);
    }
}

// ===== State Queries =====

pub fn gl_get_booleanv(pname: u32, params: &mut [u8]) {
    unsafe {
        (fns().get_booleanv)(pname, params.as_mut_ptr());
    }
}

pub fn gl_get_doublev(pname: u32, params: &mut [f64]) {
    unsafe {
        (fns().get_doublev)(pname, params.as_mut_ptr());
    }
}

pub fn gl_is_enabled(cap: u32) -> bool {
    unsafe { (fns().is_enabled)(cap) != 0 }
}

pub fn gl_get_tex_parameteriv(target: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_tex_parameteriv)(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_parameterfv(target: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_tex_parameterfv)(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_level_parameteriv(target: u32, level: i32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_tex_level_parameteriv)(target, level, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_level_parameterfv(target: u32, level: i32, pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_tex_level_parameterfv)(target, level, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_image(target: u32, level: i32, format: u32, type_: u32, pixels: &mut [u8]) {
    unsafe {
        (fns().get_tex_image)(
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
        (fns().get_lightfv)(light, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_lightiv(light: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_lightiv)(light, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_materialfv(face: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_materialfv)(face, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_materialiv(face: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_materialiv)(face, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_clip_plane(plane: u32, equation: &mut [f64; 4]) {
    unsafe {
        (fns().get_clip_plane)(plane, equation.as_mut_ptr());
    }
}

pub fn gl_clip_plane(plane: u32, equation: &[f64; 4]) {
    unsafe {
        (fns().clip_plane)(plane, equation.as_ptr());
    }
}

pub fn gl_is_texture(texture: u32) -> bool {
    unsafe { (fns().is_texture)(texture) != 0 }
}

pub fn gl_are_textures_resident(textures: &[u32], residences: &mut [u8]) -> bool {
    unsafe {
        (fns().are_textures_resident)(
            textures.len() as i32,
            textures.as_ptr(),
            residences.as_mut_ptr(),
        ) != 0
    }
}

pub fn gl_get_tex_envfv(target: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_tex_envfv)(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_enviv(target: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_tex_enviv)(target, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_gendv(coord: u32, pname: u32, params: &mut [f64]) {
    unsafe {
        (fns().get_tex_gendv)(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_genfv(coord: u32, pname: u32, params: &mut [f32]) {
    unsafe {
        (fns().get_tex_genfv)(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_tex_geniv(coord: u32, pname: u32, params: &mut [i32]) {
    unsafe {
        (fns().get_tex_geniv)(coord, pname, params.as_mut_ptr());
    }
}

pub fn gl_get_pixel_mapfv(map: u32, values: &mut [f32]) {
    unsafe {
        (fns().get_pixel_mapfv)(map, values.as_mut_ptr());
    }
}

// ===== Evaluators =====

pub fn gl_map1f(target: u32, u1: f32, u2: f32, stride: i32, order: i32, points: &[f32]) {
    unsafe {
        (fns().map1f)(target, u1, u2, stride, order, points.as_ptr());
    }
}

pub fn gl_map1d(target: u32, u1: f64, u2: f64, stride: i32, order: i32, points: &[f64]) {
    unsafe {
        (fns().map1d)(target, u1, u2, stride, order, points.as_ptr());
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
        (fns().map2f)(
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
        (fns().map2d)(
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

pub fn gl_eval_coord1f(u: f32) {
    unsafe {
        (fns().eval_coord1f)(u);
    }
}

pub fn gl_eval_coord1d(u: f64) {
    unsafe {
        (fns().eval_coord1d)(u);
    }
}

pub fn gl_eval_coord2f(u: f32, v: f32) {
    unsafe {
        (fns().eval_coord2f)(u, v);
    }
}

pub fn gl_eval_coord2d(u: f64, v: f64) {
    unsafe {
        (fns().eval_coord2d)(u, v);
    }
}

pub fn gl_map_grid1f(un: i32, u1: f32, u2: f32) {
    unsafe {
        (fns().map_grid1f)(un, u1, u2);
    }
}

pub fn gl_map_grid1d(un: i32, u1: f64, u2: f64) {
    unsafe {
        (fns().map_grid1d)(un, u1, u2);
    }
}

pub fn gl_map_grid2f(un: i32, u1: f32, u2: f32, vn: i32, v1: f32, v2: f32) {
    unsafe {
        (fns().map_grid2f)(un, u1, u2, vn, v1, v2);
    }
}

pub fn gl_map_grid2d(un: i32, u1: f64, u2: f64, vn: i32, v1: f64, v2: f64) {
    unsafe {
        (fns().map_grid2d)(un, u1, u2, vn, v1, v2);
    }
}

pub fn gl_eval_mesh1(mode: u32, i1: i32, i2: i32) {
    unsafe {
        (fns().eval_mesh1)(mode, i1, i2);
    }
}

pub fn gl_eval_mesh2(mode: u32, i1: i32, i2: i32, j1: i32, j2: i32) {
    unsafe {
        (fns().eval_mesh2)(mode, i1, i2, j1, j2);
    }
}

pub fn gl_eval_point1(i: i32) {
    unsafe {
        (fns().eval_point1)(i);
    }
}

pub fn gl_eval_point2(i: i32, j: i32) {
    unsafe {
        (fns().eval_point2)(i, j);
    }
}

// ===== Accumulation =====

pub fn gl_accum(op: u32, value: f32) {
    unsafe {
        (fns().accum)(op, value);
    }
}

pub fn gl_clear_accum(r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        (fns().clear_accum)(r, g, b, a);
    }
}

// ===== Selection/Feedback =====

pub fn gl_render_mode(mode: u32) -> i32 {
    unsafe { (fns().render_mode)(mode) }
}

pub fn gl_init_names() {
    unsafe {
        (fns().init_names)();
    }
}

pub fn gl_push_name(name: u32) {
    unsafe {
        (fns().push_name)(name);
    }
}

pub fn gl_pop_name() {
    unsafe {
        (fns().pop_name)();
    }
}

pub fn gl_load_name(name: u32) {
    unsafe {
        (fns().load_name)(name);
    }
}

pub fn gl_pass_through(token: f32) {
    unsafe {
        (fns().pass_through)(token);
    }
}

pub fn gl_push_attrib(mask: u32) {
    unsafe {
        (fns().push_attrib)(mask);
    }
}

pub fn gl_pop_attrib() {
    unsafe {
        (fns().pop_attrib)();
    }
}

pub fn gl_pixel_mapfv(map: u32, map_size: i32, values: &[f32]) {
    unsafe {
        (fns().pixel_mapfv)(map, map_size, values.as_ptr());
    }
}

pub fn gl_pixel_mapuiv(map: u32, map_size: i32, values: &[u32]) {
    unsafe {
        (fns().pixel_mapuiv)(map, map_size, values.as_ptr());
    }
}

pub fn gl_pixel_mapusv(map: u32, map_size: i32, values: &[u16]) {
    unsafe {
        (fns().pixel_mapusv)(map, map_size, values.as_ptr());
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
    if let Some(f) = fns().tex_image_3d {
        unsafe {
            f(
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
    if let Some(f) = fns().tex_sub_image_3d {
        unsafe {
            f(
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
}

// ===== GL 1.3 Optional =====

pub fn gl_active_texture(texture: u32) {
    if let Some(f) = fns().active_texture {
        unsafe {
            f(texture);
        }
    }
}

pub fn gl_multi_tex_coord1f(target: u32, s: f32) {
    if let Some(f) = fns().multi_tex_coord1f {
        unsafe {
            f(target, s);
        }
    }
}

pub fn gl_multi_tex_coord2f(target: u32, s: f32, t: f32) {
    if let Some(f) = fns().multi_tex_coord2f {
        unsafe {
            f(target, s, t);
        }
    }
}

pub fn gl_multi_tex_coord3f(target: u32, s: f32, t: f32, r: f32) {
    if let Some(f) = fns().multi_tex_coord3f {
        unsafe {
            f(target, s, t, r);
        }
    }
}

pub fn gl_multi_tex_coord4f(target: u32, s: f32, t: f32, r: f32, q: f32) {
    if let Some(f) = fns().multi_tex_coord4f {
        unsafe {
            f(target, s, t, r, q);
        }
    }
}

pub fn gl_sample_coverage(value: f32, invert: u8) {
    if let Some(f) = fns().sample_coverage {
        unsafe {
            f(value, invert);
        }
    }
}

pub fn gl_compressed_tex_image_1d(
    target: u32,
    level: i32,
    internalformat: u32,
    width: i32,
    border: i32,
    image_size: i32,
    data: &[u8],
) {
    if let Some(f) = fns().compressed_tex_image_1d {
        unsafe {
            f(
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
    if let Some(f) = fns().compressed_tex_image_2d {
        unsafe {
            f(
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
    if let Some(f) = fns().compressed_tex_image_3d {
        unsafe {
            f(
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
    if let Some(f) = fns().compressed_tex_sub_image_1d {
        unsafe {
            f(
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
    if let Some(f) = fns().compressed_tex_sub_image_2d {
        unsafe {
            f(
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
    if let Some(f) = fns().compressed_tex_sub_image_3d {
        unsafe {
            f(
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
}

// ===== GL 1.4 Optional =====

pub fn gl_secondary_color3f(r: f32, g: f32, b: f32) {
    if let Some(f) = fns().secondary_color3f {
        unsafe {
            f(r, g, b);
        }
    }
}

pub fn gl_secondary_color3ub(r: u8, g: u8, b: u8) {
    if let Some(f) = fns().secondary_color3ub {
        unsafe {
            f(r, g, b);
        }
    }
}

pub fn gl_window_pos2f(x: f32, y: f32) {
    if let Some(f) = fns().window_pos2f {
        unsafe {
            f(x, y);
        }
    }
}

pub fn gl_window_pos3f(x: f32, y: f32, z: f32) {
    if let Some(f) = fns().window_pos3f {
        unsafe {
            f(x, y, z);
        }
    }
}

pub fn gl_fog_coordf(coord: f32) {
    if let Some(f) = fns().fog_coordf {
        unsafe {
            f(coord);
        }
    }
}

pub fn gl_fog_coordd(coord: f64) {
    if let Some(f) = fns().fog_coordd {
        unsafe {
            f(coord);
        }
    }
}

pub fn gl_point_parameterf(pname: u32, param: f32) {
    if let Some(f) = fns().point_parameterf {
        unsafe {
            f(pname, param);
        }
    }
}

pub fn gl_point_parameterfv(pname: u32, params: &[f32]) {
    if let Some(f) = fns().point_parameterfv {
        unsafe {
            f(pname, params.as_ptr());
        }
    }
}

pub fn gl_point_parameteri(pname: u32, param: i32) {
    if let Some(f) = fns().point_parameteri {
        unsafe {
            f(pname, param);
        }
    }
}

// ===== GL 1.4 Blend (Optional) =====

pub fn gl_blend_equation(mode: u32) {
    if let Some(f) = fns().blend_equation {
        unsafe {
            f(mode);
        }
    }
}

pub fn gl_blend_func_separate(src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32) {
    if let Some(f) = fns().blend_func_separate {
        unsafe {
            f(src_rgb, dst_rgb, src_alpha, dst_alpha);
        }
    }
}

pub fn gl_blend_color(r: f32, g: f32, b: f32, a: f32) {
    if let Some(f) = fns().blend_color {
        unsafe {
            f(r, g, b, a);
        }
    }
}

// ===== GL 2.0 Stencil Separate (Optional) =====

pub fn gl_stencil_func_separate(face: u32, func: u32, ref_: i32, mask: u32) {
    if let Some(f) = fns().stencil_func_separate {
        unsafe {
            f(face, func, ref_, mask);
        }
    }
}

pub fn gl_stencil_op_separate(face: u32, sfail: u32, dpfail: u32, dppass: u32) {
    if let Some(f) = fns().stencil_op_separate {
        unsafe {
            f(face, sfail, dpfail, dppass);
        }
    }
}

pub fn gl_stencil_mask_separate(face: u32, mask: u32) {
    if let Some(f) = fns().stencil_mask_separate {
        unsafe {
            f(face, mask);
        }
    }
}

// ===== Imaging Subset (Optional) =====

pub fn gl_color_table(
    target: u32,
    internal_format: u32,
    width: i32,
    format: u32,
    type_: u32,
    data: &[u8],
) {
    if let Some(f) = fns().color_table {
        unsafe {
            f(
                target,
                internal_format,
                width,
                format,
                type_,
                data.as_ptr() as *const c_void,
            );
        }
    }
}

pub fn gl_convolution_parameterf(target: u32, pname: u32, param: f32) {
    if let Some(f) = fns().convolution_parameterf {
        unsafe {
            f(target, pname, param);
        }
    }
}

pub fn gl_convolution_parameterfv(target: u32, pname: u32, params: &[f32]) {
    if let Some(f) = fns().convolution_parameterfv {
        unsafe {
            f(target, pname, params.as_ptr());
        }
    }
}

pub fn gl_convolution_parameteri(target: u32, pname: u32, param: i32) {
    if let Some(f) = fns().convolution_parameteri {
        unsafe {
            f(target, pname, param);
        }
    }
}

pub fn gl_convolution_parameteriv(target: u32, pname: u32, params: &[i32]) {
    if let Some(f) = fns().convolution_parameteriv {
        unsafe {
            f(target, pname, params.as_ptr());
        }
    }
}

pub fn gl_histogram(target: u32, width: i32, internal_format: u32, sink: u8) {
    if let Some(f) = fns().histogram {
        unsafe {
            f(target, width, internal_format, sink);
        }
    }
}

pub fn gl_minmax(target: u32, internal_format: u32, sink: u8) {
    if let Some(f) = fns().minmax {
        unsafe {
            f(target, internal_format, sink);
        }
    }
}

// ===== GL 2.0 Shader Functions (Optional) =====

// ===== GL 1.5 Buffer Objects (Optional) =====

// ===== GL 3.0 FBO/VAO (Optional) =====
