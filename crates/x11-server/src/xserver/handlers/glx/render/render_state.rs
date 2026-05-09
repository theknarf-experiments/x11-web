//! GL state management opcodes (glEnable, glDisable, glBlendFunc, etc.).

use crate::osmesa;

/// Dispatch a GL state management render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glEnable
        69 => {
            if data.len() >= 4 {
                let cap = super::read_u32_le(data, 0);
                osmesa::gl_enable(cap);
            }
        }
        // glDisable
        68 => {
            if data.len() >= 4 {
                let cap = super::read_u32_le(data, 0);
                osmesa::gl_disable(cap);
            }
        }
        // glFinish
        108 => {
            osmesa::gl_finish();
        }
        // glFlush
        142 => {
            osmesa::gl_flush();
        }
        // glDepthFunc
        164 => {
            if data.len() >= 4 {
                let func = super::read_u32_le(data, 0);
                osmesa::gl_depth_func(func);
            }
        }
        // glDepthMask
        135 => {
            if data.len() >= 4 {
                let flag = super::read_u32_le(data, 0);
                osmesa::gl_depth_mask(if flag != 0 { 1 } else { 0 });
            }
        }
        // glClearColor
        130 => {
            if data.len() >= 16 {
                let r = super::read_f32_le(data, 0);
                let g = super::read_f32_le(data, 4);
                let b = super::read_f32_le(data, 8);
                let a = super::read_f32_le(data, 12);
                osmesa::gl_clear_color(r, g, b, a);
            }
        }
        // glClear
        127 => {
            if data.len() >= 4 {
                let mask = super::read_u32_le(data, 0);
                osmesa::gl_clear(mask);
            }
        }
        // glClearDepth
        132 => {
            if data.len() >= 8 {
                let depth = super::read_f64_le(data, 0);
                osmesa::gl_clear_depth(depth);
            }
        }
        // glClearStencil
        133 => {
            if data.len() >= 4 {
                let s = super::read_i32_le(data, 0);
                osmesa::gl_clear_stencil(s);
            }
        }
        // glColorMask
        134 => {
            if data.len() >= 4 {
                // Each is a GLboolean (4 bytes each in the wire protocol)
                if data.len() >= 16 {
                    let r = super::read_u32_le(data, 0);
                    let g = super::read_u32_le(data, 4);
                    let b = super::read_u32_le(data, 8);
                    let a = super::read_u32_le(data, 12);
                    osmesa::gl_color_mask(
                        if r != 0 { 1 } else { 0 },
                        if g != 0 { 1 } else { 0 },
                        if b != 0 { 1 } else { 0 },
                        if a != 0 { 1 } else { 0 },
                    );
                }
            }
        }
        // glBlendFunc
        160 => {
            if data.len() >= 8 {
                let sfactor = super::read_u32_le(data, 0);
                let dfactor = super::read_u32_le(data, 4);
                osmesa::gl_blend_func(sfactor, dfactor);
            }
        }
        // glStencilFunc
        162 => {
            if data.len() >= 12 {
                let func = super::read_u32_le(data, 0);
                let ref_ = super::read_i32_le(data, 4);
                let mask = super::read_u32_le(data, 8);
                osmesa::gl_stencil_func(func, ref_, mask);
            }
        }
        // glStencilMask (opcode 209)
        209 => {
            if data.len() >= 4 {
                let mask = super::read_u32_le(data, 0);
                osmesa::gl_stencil_mask(mask);
            }
        }
        // glStencilOp
        163 => {
            if data.len() >= 12 {
                let fail = super::read_u32_le(data, 0);
                let zfail = super::read_u32_le(data, 4);
                let zpass = super::read_u32_le(data, 8);
                osmesa::gl_stencil_op(fail, zfail, zpass);
            }
        }
        // glScissor
        103 => {
            if data.len() >= 16 {
                let x = super::read_i32_le(data, 0);
                let y = super::read_i32_le(data, 4);
                let w = super::read_i32_le(data, 8);
                let h = super::read_i32_le(data, 12);
                osmesa::gl_scissor(x, y, w, h);
            }
        }
        // glAlphaFunc
        240 => {
            if data.len() >= 8 {
                let func = super::read_u32_le(data, 0);
                let ref_ = super::read_f32_le(data, 4);
                osmesa::gl_alpha_func(func, ref_);
            }
        }
        // glHint
        85 => {
            if data.len() >= 8 {
                let target = super::read_u32_le(data, 0);
                let mode = super::read_u32_le(data, 4);
                osmesa::gl_hint(target, mode);
            }
        }
        // glLineWidth
        95 => {
            if data.len() >= 4 {
                let width = super::read_f32_le(data, 0);
                osmesa::gl_line_width(width);
            }
        }
        // glPointSize
        100 => {
            if data.len() >= 4 {
                let size = super::read_f32_le(data, 0);
                osmesa::gl_point_size(size);
            }
        }
        // glPolygonMode
        101 => {
            if data.len() >= 8 {
                let face = super::read_u32_le(data, 0);
                let mode = super::read_u32_le(data, 4);
                osmesa::gl_polygon_mode(face, mode);
            }
        }
        // glCullFace
        79 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_cull_face(mode);
            }
        }
        // glFrontFace
        84 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_front_face(mode);
            }
        }
        // glShadeModel
        104 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_shade_model(mode);
            }
        }
        // glPixelStorei
        110 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_i32_le(data, 4);
                osmesa::gl_pixel_storei(pname, param);
            }
        }
        // glPixelStoref (opcode 111)
        111 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_f32_le(data, 4);
                osmesa::gl_pixel_storef(pname, param);
            }
        }
        // glPixelTransferf (opcode 112)
        112 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_f32_le(data, 4);
                osmesa::gl_pixel_transferf(pname, param);
            }
        }
        // glPixelTransferi (opcode 113)
        113 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_i32_le(data, 4);
                osmesa::gl_pixel_transferi(pname, param);
            }
        }
        // glPixelZoom (opcode 114)
        114 => {
            if data.len() >= 8 {
                let xfactor = super::read_f32_le(data, 0);
                let yfactor = super::read_f32_le(data, 4);
                osmesa::gl_pixel_zoom(xfactor, yfactor);
            }
        }
        // glClipPlane (opcode 77)
        77 => {
            if data.len() >= 36 {
                let plane = super::read_u32_le(data, 0);
                let mut eq = [0f64; 4];
                for i in 0..4 {
                    eq[i] = f64::from_le_bytes(data[4 + i * 8..12 + i * 8].try_into().unwrap());
                }
                osmesa::gl_clip_plane(plane, &eq);
            }
        }
        // glColorMaterial (opcode 78)
        78 => {
            if data.len() >= 8 {
                let face = super::read_u32_le(data, 0);
                let mode = super::read_u32_le(data, 4);
                osmesa::gl_color_material(face, mode);
            }
        }
        // glFogf (opcode 80)
        80 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_f32_le(data, 4);
                osmesa::gl_fogf(pname, param);
            }
        }
        // glFogfv (opcode 81)
        81 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([
                        data[4 + i * 4],
                        data[5 + i * 4],
                        data[6 + i * 4],
                        data[7 + i * 4],
                    ]);
                }
                osmesa::gl_fogfv(pname, &params);
            }
        }
        // glFogi (opcode 82)
        82 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_i32_le(data, 4);
                osmesa::gl_fogi(pname, param);
            }
        }
        // glFogiv (opcode 83)
        83 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([
                        data[4 + i * 4],
                        data[5 + i * 4],
                        data[6 + i * 4],
                        data[7 + i * 4],
                    ]);
                }
                osmesa::gl_fogiv(pname, &params);
            }
        }
        // glLightf (opcode 86)
        86 => {
            if data.len() >= 12 {
                let light = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_lightf(light, pname, param);
            }
        }
        // glLightfv (opcode 87)
        87 => {
            if data.len() >= 12 {
                let light = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([
                        data[8 + i * 4],
                        data[9 + i * 4],
                        data[10 + i * 4],
                        data[11 + i * 4],
                    ]);
                }
                osmesa::gl_lightfv(light, pname, &params);
            }
        }
        // glLighti (opcode 88)
        88 => {
            if data.len() >= 12 {
                let light = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_lighti(light, pname, param);
            }
        }
        // glLightiv (opcode 89)
        89 => {
            if data.len() >= 12 {
                let light = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([
                        data[8 + i * 4],
                        data[9 + i * 4],
                        data[10 + i * 4],
                        data[11 + i * 4],
                    ]);
                }
                osmesa::gl_lightiv(light, pname, &params);
            }
        }
        // glLightModelf (opcode 90)
        90 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_f32_le(data, 4);
                osmesa::gl_light_modelf(pname, param);
            }
        }
        // glLightModelfv (opcode 91)
        91 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([
                        data[4 + i * 4],
                        data[5 + i * 4],
                        data[6 + i * 4],
                        data[7 + i * 4],
                    ]);
                }
                osmesa::gl_light_modelfv(pname, &params);
            }
        }
        // glLightModeli (opcode 92)
        92 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_i32_le(data, 4);
                osmesa::gl_light_modeli(pname, param);
            }
        }
        // glLightModeliv (opcode 93)
        93 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([
                        data[4 + i * 4],
                        data[5 + i * 4],
                        data[6 + i * 4],
                        data[7 + i * 4],
                    ]);
                }
                osmesa::gl_light_modeliv(pname, &params);
            }
        }
        // glLineStipple (opcode 94)
        94 => {
            if data.len() >= 8 {
                let factor = super::read_i32_le(data, 0);
                let pattern = super::read_u16_le(data, 4);
                osmesa::gl_line_stipple(factor, pattern);
            }
        }
        // glMaterialf (opcode 96)
        96 => {
            if data.len() >= 12 {
                let face = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_materialf(face, pname, param);
            }
        }
        // glMaterialfv (opcode 97)
        97 => {
            if data.len() >= 12 {
                let face = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([
                        data[8 + i * 4],
                        data[9 + i * 4],
                        data[10 + i * 4],
                        data[11 + i * 4],
                    ]);
                }
                osmesa::gl_materialfv(face, pname, &params);
            }
        }
        // glMateriali (opcode 98)
        98 => {
            if data.len() >= 12 {
                let face = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_materiali(face, pname, param);
            }
        }
        // glMaterialiv (opcode 99)
        99 => {
            if data.len() >= 12 {
                let face = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([
                        data[8 + i * 4],
                        data[9 + i * 4],
                        data[10 + i * 4],
                        data[11 + i * 4],
                    ]);
                }
                osmesa::gl_materialiv(face, pname, &params);
            }
        }
        // glPolygonStipple (opcode 102)
        102 => {
            if data.len() >= 128 {
                osmesa::gl_polygon_stipple(&data[0..128]);
            }
        }
        // glInitNames (opcode 125) -- selection name stack
        125 => {
            osmesa::gl_init_names();
        }
        // glLoadName (opcode 126)
        126 => {
            if data.len() >= 4 {
                let name = super::read_u32_le(data, 0);
                osmesa::gl_load_name(name);
            }
        }
        // glClearAccum (opcode 128)
        128 => {
            if data.len() >= 16 {
                let r = super::read_f32_le(data, 0);
                let g = super::read_f32_le(data, 4);
                let b = super::read_f32_le(data, 8);
                let a = super::read_f32_le(data, 12);
                osmesa::gl_clear_accum(r, g, b, a);
            }
        }
        // glClearIndex (opcode 129)
        129 => {
            if data.len() >= 4 {
                let c = super::read_f32_le(data, 0);
                osmesa::gl_clear_index(c);
            }
        }
        // glIndexMask (opcode 131)
        131 => {
            if data.len() >= 4 {
                let mask = super::read_u32_le(data, 0);
                osmesa::gl_index_mask(mask);
            }
        }
        // glPopAttrib (opcode 141)
        141 => {
            osmesa::gl_pop_attrib();
        }
        // glAccum (opcode 137)
        137 => {
            if data.len() >= 8 {
                let op = super::read_u32_le(data, 0);
                let value = super::read_f32_le(data, 4);
                osmesa::gl_accum(op, value);
            }
        }
        // glLogicOp (opcode 159)
        159 => {
            if data.len() >= 4 {
                let opcode_val = super::read_u32_le(data, 0);
                osmesa::gl_logic_op(opcode_val);
            }
        }
        // glDrawBuffer (opcode 136)
        136 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_draw_buffer(mode);
            }
        }
        // glReadBuffer (opcode 138)
        138 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_read_buffer(mode);
            }
        }
        // glPassThrough (opcode 139)
        139 => {
            if data.len() >= 4 {
                let token = super::read_f32_le(data, 0);
                osmesa::gl_pass_through(token);
            }
        }
        // glPopName (opcode 140)
        140 => {
            osmesa::gl_pop_name();
        }
        // glPixelMapfv (opcode 143)
        143 => {
            if data.len() >= 8 {
                let map = super::read_u32_le(data, 0);
                let map_size = super::read_i32_le(data, 4);
                let count = map_size as usize;
                if data.len() >= 8 + count * 4 {
                    let mut values = vec![0f32; count];
                    for i in 0..count {
                        values[i] = f32::from_le_bytes([
                            data[8 + i * 4],
                            data[9 + i * 4],
                            data[10 + i * 4],
                            data[11 + i * 4],
                        ]);
                    }
                    osmesa::gl_pixel_mapfv(map, map_size, &values);
                }
            }
        }
        // glPixelMapuiv (opcode 144)
        144 => {
            if data.len() >= 8 {
                let map = super::read_u32_le(data, 0);
                let map_size = super::read_i32_le(data, 4);
                let count = map_size as usize;
                if data.len() >= 8 + count * 4 {
                    let mut values = vec![0u32; count];
                    for i in 0..count {
                        values[i] = u32::from_le_bytes([
                            data[8 + i * 4],
                            data[9 + i * 4],
                            data[10 + i * 4],
                            data[11 + i * 4],
                        ]);
                    }
                    osmesa::gl_pixel_mapuiv(map, map_size, &values);
                }
            }
        }
        // glPixelMapusv (opcode 145)
        145 => {
            if data.len() >= 8 {
                let map = super::read_u32_le(data, 0);
                let map_size = super::read_i32_le(data, 4);
                let count = map_size as usize;
                if data.len() >= 8 + count * 2 {
                    let mut values = vec![0u16; count];
                    for i in 0..count {
                        values[i] = super::read_u16_le(data, 8 + i * 2);
                    }
                    osmesa::gl_pixel_mapusv(map, map_size, &values);
                }
            }
        }
        // glPushName (opcode 146)
        146 => {
            if data.len() >= 4 {
                let name = super::read_u32_le(data, 0);
                osmesa::gl_push_name(name);
            }
        }
        // glMapGrid1d (opcode 147)
        147 => {
            if data.len() >= 20 {
                let un = super::read_i32_le(data, 0);
                let u1 = super::read_f64_le(data, 4);
                let u2 = super::read_f64_le(data, 12);
                osmesa::gl_map_grid1d(un, u1, u2);
            }
        }
        // glMapGrid1f (opcode 148)
        148 => {
            if data.len() >= 12 {
                let un = super::read_i32_le(data, 0);
                let u1 = super::read_f32_le(data, 4);
                let u2 = super::read_f32_le(data, 8);
                osmesa::gl_map_grid1f(un, u1, u2);
            }
        }
        // glMapGrid2d (opcode 149)
        149 => {
            if data.len() >= 40 {
                let un = super::read_i32_le(data, 0);
                let u1 = super::read_f64_le(data, 4);
                let u2 = super::read_f64_le(data, 12);
                let vn = super::read_i32_le(data, 20);
                let v1 = super::read_f64_le(data, 24);
                let v2 = super::read_f64_le(data, 32);
                osmesa::gl_map_grid2d(un, u1, u2, vn, v1, v2);
            }
        }
        // glPushAttrib (opcode 150)
        150 => {
            if data.len() >= 4 {
                let mask = super::read_u32_le(data, 0);
                osmesa::gl_push_attrib(mask);
            }
        }
        // glPolygonOffset (opcode 161)
        161 => {
            if data.len() >= 8 {
                let factor = super::read_f32_le(data, 0);
                let units = super::read_f32_le(data, 4);
                osmesa::gl_polygon_offset(factor, units);
            }
        }
        // glDepthRange (opcode 174)
        174 => {
            if data.len() >= 16 {
                let near = super::read_f64_le(data, 0);
                let far = super::read_f64_le(data, 8);
                osmesa::gl_depth_range(near, far);
            }
        }
        // glPolygonOffset (alternate opcode 192)
        192 => {
            if data.len() >= 8 {
                let factor = super::read_f32_le(data, 0);
                let units = super::read_f32_le(data, 4);
                osmesa::gl_polygon_offset(factor, units);
            }
        }
        // glMultiTexCoord4dv (opcode 210): target(4) + s(8) + t(8) + r(8) + q(8) = 36 bytes
        210 => {
            if data.len() >= 36 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_f64_le(data, 4) as f32;
                let t = super::read_f64_le(data, 12) as f32;
                let r = super::read_f64_le(data, 20) as f32;
                let q = super::read_f64_le(data, 28) as f32;
                osmesa::gl_multi_tex_coord4f(target, s, t, r, q);
            }
        }
        // glMultiTexCoord4iv (opcode 211): target(4) + s(4) + t(4) + r(4) + q(4) = 20 bytes
        211 => {
            if data.len() >= 20 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_i32_le(data, 4) as f32;
                let t = super::read_i32_le(data, 8) as f32;
                let r = super::read_i32_le(data, 12) as f32;
                let q = super::read_i32_le(data, 16) as f32;
                osmesa::gl_multi_tex_coord4f(target, s, t, r, q);
            }
        }
        // glMultiTexCoord4sv (opcode 212): target(4) + s(2) + t(2) + r(2) + q(2) = 12 bytes
        212 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_i16_le(data, 4) as f32;
                let t = super::read_i16_le(data, 6) as f32;
                let r = super::read_i16_le(data, 8) as f32;
                let q = super::read_i16_le(data, 10) as f32;
                osmesa::gl_multi_tex_coord4f(target, s, t, r, q);
            }
        }
        // glCompressedTexImage1D (opcode 213): pixel header(20) + target(4) + level(4)
        //   + internalformat(4) + width(4) + border(4) + imageSize(4) + data...
        213 => {
            if data.len() >= 44 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internalformat = super::read_u32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let border = super::read_i32_le(data, 36);
                let image_size = super::read_i32_le(data, 40);
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                osmesa::gl_compressed_tex_image_1d(
                    target,
                    level,
                    internalformat,
                    width,
                    border,
                    image_size,
                    pixel_data,
                );
            }
        }
        // glCompressedTexImage2D (opcode 214): pixel header(20) + target(4) + level(4)
        //   + internalformat(4) + width(4) + height(4) + border(4) + imageSize(4) + data...
        214 => {
            if data.len() >= 48 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internalformat = super::read_u32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let height = super::read_i32_le(data, 36);
                let border = super::read_i32_le(data, 40);
                let image_size = super::read_i32_le(data, 44);
                let pixel_data = if data.len() > 48 { &data[48..] } else { &[] };
                osmesa::gl_compressed_tex_image_2d(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    border,
                    image_size,
                    pixel_data,
                );
            }
        }
        // glCompressedTexImage3D (opcode 215): pixel header(20) + target(4) + level(4)
        //   + internalformat(4) + width(4) + height(4) + depth(4) + border(4) + imageSize(4) + data...
        215 => {
            if data.len() >= 52 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internalformat = super::read_u32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let height = super::read_i32_le(data, 36);
                let depth = super::read_i32_le(data, 40);
                let border = super::read_i32_le(data, 44);
                let image_size = super::read_i32_le(data, 48);
                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                osmesa::gl_compressed_tex_image_3d(
                    target,
                    level,
                    internalformat,
                    width,
                    height,
                    depth,
                    border,
                    image_size,
                    pixel_data,
                );
            }
        }
        // ARB_vertex_blend opcodes 222-228: Weight functions
        // These are from a rarely-used ARB extension. Accept silently as
        // OSMesa's software renderer doesn't support vertex blending hardware.
        222..=228 => {
            // ARB_vertex_blend: glWeight[bsiu]v, glVertexBlend, glWeightfv, glWeightdv
            // These operations are effectively no-ops in software rendering.
        }
        // glSampleCoverage (opcode 229)
        229 => {
            if data.len() >= 8 {
                let value = super::read_f32_le(data, 0);
                let invert = data[4];
                osmesa::gl_sample_coverage(value, invert);
            }
        }
        // Opcodes 231-239: reserved / uncommon
        231..=239 => {
            // Reserved or rarely-used render opcodes -- silently accepted
        }
        // glBlendColor (opcode 4096)
        4096 => {
            if data.len() >= 16 {
                let r = super::read_f32_le(data, 0);
                let g = super::read_f32_le(data, 4);
                let b = super::read_f32_le(data, 8);
                let a = super::read_f32_le(data, 12);
                osmesa::gl_blend_color(r, g, b, a);
            }
        }
        // glBlendEquation (opcode 4097)
        4097 => {
            if data.len() >= 4 {
                let mode = super::read_u32_le(data, 0);
                osmesa::gl_blend_equation(mode);
            }
        }
        // glPointParameterf (opcode 4120)
        4120 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_f32_le(data, 4);
                osmesa::gl_point_parameterf(pname, param);
            }
        }
        // glPointParameterfv (opcode 4121)
        4121 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([
                        data[4 + i * 4],
                        data[5 + i * 4],
                        data[6 + i * 4],
                        data[7 + i * 4],
                    ]);
                }
                osmesa::gl_point_parameterfv(pname, &params);
            }
        }
        // glStencilFuncSeparate (opcode 4129)
        4129 => {
            if data.len() >= 16 {
                let face = super::read_u32_le(data, 0);
                let func = super::read_u32_le(data, 4);
                let ref_ = super::read_i32_le(data, 8);
                let mask = super::read_u32_le(data, 12);
                osmesa::gl_stencil_func_separate(face, func, ref_, mask);
            }
        }
        // glStencilOpSeparate (opcode 4130)
        4130 => {
            if data.len() >= 16 {
                let face = super::read_u32_le(data, 0);
                let sfail = super::read_u32_le(data, 4);
                let dpfail = super::read_u32_le(data, 8);
                let dppass = super::read_u32_le(data, 12);
                osmesa::gl_stencil_op_separate(face, sfail, dpfail, dppass);
            }
        }
        // glStencilMaskSeparate (opcode 4131)
        4131 => {
            if data.len() >= 8 {
                let face = super::read_u32_le(data, 0);
                let mask = super::read_u32_le(data, 4);
                osmesa::gl_stencil_mask_separate(face, mask);
            }
        }
        // glBlendFuncSeparate (opcode 4134)
        4134 => {
            if data.len() >= 16 {
                let srgb = super::read_u32_le(data, 0);
                let drgb = super::read_u32_le(data, 4);
                let salpha = super::read_u32_le(data, 8);
                let dalpha = super::read_u32_le(data, 12);
                osmesa::gl_blend_func_separate(srgb, drgb, salpha, dalpha);
            }
        }
        // glPointParameteri (opcode 4222)
        4222 => {
            if data.len() >= 8 {
                let pname = super::read_u32_le(data, 0);
                let param = super::read_i32_le(data, 4);
                osmesa::gl_point_parameteri(pname, param);
            }
        }
        _ => return None,
    }
    Some(true)
}
