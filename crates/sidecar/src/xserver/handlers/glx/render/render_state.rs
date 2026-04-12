//! GL state management opcodes (glEnable, glDisable, glBlendFunc, etc.).

use crate::osmesa;

/// Dispatch a GL state management render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glEnable
        69 => {
            if data.len() >= 4 {
                let cap = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_enable(cap);
            }
        }
        // glDisable
        68 => {
            if data.len() >= 4 {
                let cap = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
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
                let func = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_depth_func(func);
            }
        }
        // glDepthMask
        135 => {
            if data.len() >= 4 {
                let flag = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_depth_mask(if flag != 0 { 1 } else { 0 });
            }
        }
        // glClearColor
        130 => {
            if data.len() >= 16 {
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_clear_color(r, g, b, a);
            }
        }
        // glClear
        127 => {
            if data.len() >= 4 {
                let mask = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_clear(mask);
            }
        }
        // glClearDepth
        132 => {
            if data.len() >= 8 {
                let depth = f64::from_le_bytes(data[0..8].try_into().unwrap());
                osmesa::gl_clear_depth(depth);
            }
        }
        // glClearStencil
        133 => {
            if data.len() >= 4 {
                let s = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_clear_stencil(s);
            }
        }
        // glColorMask
        134 => {
            if data.len() >= 4 {
                // Each is a GLboolean (4 bytes each in the wire protocol)
                if data.len() >= 16 {
                    let r = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    let g = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    let b = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                    let a = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
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
                let sfactor = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let dfactor = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_blend_func(sfactor, dfactor);
            }
        }
        // glStencilFunc
        162 => {
            if data.len() >= 12 {
                let func = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let ref_ = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_stencil_func(func, ref_, mask);
            }
        }
        // glStencilMask (opcode 209)
        209 => {
            if data.len() >= 4 {
                let mask = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_stencil_mask(mask);
            }
        }
        // glStencilOp
        163 => {
            if data.len() >= 12 {
                let fail = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let zfail = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let zpass = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_stencil_op(fail, zfail, zpass);
            }
        }
        // glScissor
        103 => {
            if data.len() >= 16 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let w = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let h = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_scissor(x, y, w, h);
            }
        }
        // glAlphaFunc
        240 => {
            if data.len() >= 8 {
                let func = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let ref_ = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_alpha_func(func, ref_);
            }
        }
        // glHint
        85 => {
            if data.len() >= 8 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mode = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_hint(target, mode);
            }
        }
        // glLineWidth
        95 => {
            if data.len() >= 4 {
                let width = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_line_width(width);
            }
        }
        // glPointSize
        100 => {
            if data.len() >= 4 {
                let size = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_point_size(size);
            }
        }
        // glPolygonMode
        101 => {
            if data.len() >= 8 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mode = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_polygon_mode(face, mode);
            }
        }
        // glCullFace
        79 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_cull_face(mode);
            }
        }
        // glFrontFace
        84 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_front_face(mode);
            }
        }
        // glShadeModel
        104 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_shade_model(mode);
            }
        }
        // glPixelStorei
        110 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_pixel_storei(pname, param);
            }
        }
        // glPixelStoref (opcode 111)
        111 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_pixel_storef(pname, param);
            }
        }
        // glPixelTransferf (opcode 112)
        112 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_pixel_transferf(pname, param);
            }
        }
        // glPixelTransferi (opcode 113)
        113 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_pixel_transferi(pname, param);
            }
        }
        // glPixelZoom (opcode 114)
        114 => {
            if data.len() >= 8 {
                let xfactor = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let yfactor = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_pixel_zoom(xfactor, yfactor);
            }
        }
        // glClipPlane (opcode 77)
        77 => {
            if data.len() >= 36 {
                let plane = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mut eq = [0f64; 4];
                for i in 0..4 {
                    eq[i] = f64::from_le_bytes(data[4+i*8..12+i*8].try_into().unwrap());
                }
                osmesa::gl_clip_plane(plane, &eq);
            }
        }
        // glColorMaterial (opcode 78)
        78 => {
            if data.len() >= 8 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mode = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_color_material(face, mode);
            }
        }
        // glFogf (opcode 80)
        80 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_fogf(pname, param);
            }
        }
        // glFogfv (opcode 81)
        81 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[4+i*4], data[5+i*4], data[6+i*4], data[7+i*4]]);
                }
                osmesa::gl_fogfv(pname, &params);
            }
        }
        // glFogi (opcode 82)
        82 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_fogi(pname, param);
            }
        }
        // glFogiv (opcode 83)
        83 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[4+i*4], data[5+i*4], data[6+i*4], data[7+i*4]]);
                }
                osmesa::gl_fogiv(pname, &params);
            }
        }
        // glLightf (opcode 86)
        86 => {
            if data.len() >= 12 {
                let light = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_lightf(light, pname, param);
            }
        }
        // glLightfv (opcode 87)
        87 => {
            if data.len() >= 12 {
                let light = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_lightfv(light, pname, &params);
            }
        }
        // glLighti (opcode 88)
        88 => {
            if data.len() >= 12 {
                let light = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_lighti(light, pname, param);
            }
        }
        // glLightiv (opcode 89)
        89 => {
            if data.len() >= 12 {
                let light = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_lightiv(light, pname, &params);
            }
        }
        // glLightModelf (opcode 90)
        90 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_light_modelf(pname, param);
            }
        }
        // glLightModelfv (opcode 91)
        91 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[4+i*4], data[5+i*4], data[6+i*4], data[7+i*4]]);
                }
                osmesa::gl_light_modelfv(pname, &params);
            }
        }
        // glLightModeli (opcode 92)
        92 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_light_modeli(pname, param);
            }
        }
        // glLightModeliv (opcode 93)
        93 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[4+i*4], data[5+i*4], data[6+i*4], data[7+i*4]]);
                }
                osmesa::gl_light_modeliv(pname, &params);
            }
        }
        // glLineStipple (opcode 94)
        94 => {
            if data.len() >= 8 {
                let factor = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pattern = u16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_line_stipple(factor, pattern);
            }
        }
        // glMaterialf (opcode 96)
        96 => {
            if data.len() >= 12 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_materialf(face, pname, param);
            }
        }
        // glMaterialfv (opcode 97)
        97 => {
            if data.len() >= 12 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_materialfv(face, pname, &params);
            }
        }
        // glMateriali (opcode 98)
        98 => {
            if data.len() >= 12 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_materiali(face, pname, param);
            }
        }
        // glMaterialiv (opcode 99)
        99 => {
            if data.len() >= 12 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
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
                let name = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_load_name(name);
            }
        }
        // glClearAccum (opcode 128)
        128 => {
            if data.len() >= 16 {
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_clear_accum(r, g, b, a);
            }
        }
        // glClearIndex (opcode 129)
        129 => {
            if data.len() >= 4 {
                let c = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_clear_index(c);
            }
        }
        // glIndexMask (opcode 131)
        131 => {
            if data.len() >= 4 {
                let mask = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
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
                let op = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let value = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_accum(op, value);
            }
        }
        // glLogicOp (opcode 159)
        159 => {
            if data.len() >= 4 {
                let opcode_val = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_logic_op(opcode_val);
            }
        }
        // glDrawBuffer (opcode 136)
        136 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_draw_buffer(mode);
            }
        }
        // glReadBuffer (opcode 138)
        138 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_read_buffer(mode);
            }
        }
        // glPassThrough (opcode 139)
        139 => {
            if data.len() >= 4 {
                let token = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
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
                let map = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let map_size = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = map_size as usize;
                if data.len() >= 8 + count * 4 {
                    let mut values = vec![0f32; count];
                    for i in 0..count {
                        values[i] = f32::from_le_bytes([
                            data[8 + i * 4], data[9 + i * 4],
                            data[10 + i * 4], data[11 + i * 4],
                        ]);
                    }
                    osmesa::gl_pixel_mapfv(map, map_size, &values);
                }
            }
        }
        // glPixelMapuiv (opcode 144)
        144 => {
            if data.len() >= 8 {
                let map = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let map_size = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = map_size as usize;
                if data.len() >= 8 + count * 4 {
                    let mut values = vec![0u32; count];
                    for i in 0..count {
                        values[i] = u32::from_le_bytes([
                            data[8 + i * 4], data[9 + i * 4],
                            data[10 + i * 4], data[11 + i * 4],
                        ]);
                    }
                    osmesa::gl_pixel_mapuiv(map, map_size, &values);
                }
            }
        }
        // glPixelMapusv (opcode 145)
        145 => {
            if data.len() >= 8 {
                let map = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let map_size = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = map_size as usize;
                if data.len() >= 8 + count * 2 {
                    let mut values = vec![0u16; count];
                    for i in 0..count {
                        values[i] = u16::from_le_bytes([
                            data[8 + i * 2], data[9 + i * 2],
                        ]);
                    }
                    osmesa::gl_pixel_mapusv(map, map_size, &values);
                }
            }
        }
        // glPushName (opcode 146)
        146 => {
            if data.len() >= 4 {
                let name = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_push_name(name);
            }
        }
        // Render opcodes 147-149: uncommon selection/feedback operations
        147..=149 => {}
        // glPushAttrib (opcode 150)
        150 => {
            if data.len() >= 4 {
                let mask = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_push_attrib(mask);
            }
        }
        // glPolygonOffset (opcode 161)
        161 => {
            if data.len() >= 8 {
                let factor = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let units = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_polygon_offset(factor, units);
            }
        }
        // glDepthRange (opcode 174)
        174 => {
            if data.len() >= 16 {
                let near = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let far = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_depth_range(near, far);
            }
        }
        // glPolygonOffset (alternate opcode 192)
        192 => {
            if data.len() >= 8 {
                let factor = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let units = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_polygon_offset(factor, units);
            }
        }
        // Opcodes 210-215, 222-228: remaining GL 1.3-1.5 render opcodes
        210..=215 | 222..=228 => {
            // Various uncommon render opcodes -- silently accepted for now
        }
        // glSampleCoverage (opcode 229)
        229 => {
            if data.len() >= 8 {
                let value = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
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
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_blend_color(r, g, b, a);
            }
        }
        // glBlendEquation (opcode 4097)
        4097 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_blend_equation(mode);
            }
        }
        // glPointParameterf (opcode 4120)
        4120 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_point_parameterf(pname, param);
            }
        }
        // glPointParameterfv (opcode 4121)
        4121 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = (data.len() - 4) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[4+i*4], data[5+i*4], data[6+i*4], data[7+i*4]]);
                }
                osmesa::gl_point_parameterfv(pname, &params);
            }
        }
        // glStencilFuncSeparate (opcode 4129)
        4129 => {
            if data.len() >= 16 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let func = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let ref_ = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_stencil_func_separate(face, func, ref_, mask);
            }
        }
        // glStencilOpSeparate (opcode 4130)
        4130 => {
            if data.len() >= 16 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let sfail = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let dpfail = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let dppass = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_stencil_op_separate(face, sfail, dpfail, dppass);
            }
        }
        // glStencilMaskSeparate (opcode 4131)
        4131 => {
            if data.len() >= 8 {
                let face = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mask = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_stencil_mask_separate(face, mask);
            }
        }
        // glBlendFuncSeparate (opcode 4134)
        4134 => {
            if data.len() >= 16 {
                let srgb = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let drgb = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let salpha = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let dalpha = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_blend_func_separate(srgb, drgb, salpha, dalpha);
            }
        }
        // glPointParameteri (opcode 4222)
        4222 => {
            if data.len() >= 8 {
                let pname = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let param = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_point_parameteri(pname, param);
            }
        }
        _ => return None,
    }
    Some(true)
}
