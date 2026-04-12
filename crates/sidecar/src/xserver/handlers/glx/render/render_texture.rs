//! Texture opcodes (glBindTexture, glTexImage*, glTexParameter*, etc.).

use crate::osmesa;

/// Dispatch a GL texture render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glTexParameteri
        105 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_parameteri(target, pname, param);
            }
        }
        // glTexParameterf (opcode 106)
        106 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_parameterf(target, pname, param);
            }
        }
        // glTexParameterfv (opcode 107)
        107 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_parameterfv(target, pname, &params);
            }
        }
        // glTexParameteriv (opcode 109)
        109 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_parameteriv(target, pname, &params);
            }
        }
        // glTexEnvf (opcode 115)
        115 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_envf(target, pname, param);
            }
        }
        // glTexEnvfv (opcode 116)
        116 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_envfv(target, pname, &params);
            }
        }
        // glTexEnvi (opcode 117)
        117 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_envi(target, pname, param);
            }
        }
        // glTexEnviv (opcode 118)
        118 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_enviv(target, pname, &params);
            }
        }
        // glTexGend (opcode 119)
        119 => {
            if data.len() >= 16 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_tex_gend(coord, pname, param);
            }
        }
        // glTexGendv (opcode 120)
        120 => {
            if data.len() >= 12 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 8;
                let mut params = vec![0f64; count];
                for i in 0..count {
                    params[i] = f64::from_le_bytes(data[8+i*8..16+i*8].try_into().unwrap());
                }
                osmesa::gl_tex_gendv(coord, pname, &params);
            }
        }
        // glTexGenf (opcode 121)
        121 => {
            if data.len() >= 12 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_genf(coord, pname, param);
            }
        }
        // glTexGenfv (opcode 122)
        122 => {
            if data.len() >= 12 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_genfv(coord, pname, &params);
            }
        }
        // glTexGeni (opcode 123)
        123 => {
            if data.len() >= 12 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_geni(coord, pname, param);
            }
        }
        // glTexGeniv (opcode 124)
        124 => {
            if data.len() >= 12 {
                let coord = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_tex_geniv(coord, pname, &params);
            }
        }
        // glActiveTexture (opcode 197)
        197 => {
            if data.len() >= 4 {
                let texture = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_active_texture(texture);
            }
        }
        // glMultiTexCoord1fv (opcode 198)
        198 => {
            if data.len() >= 8 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let s = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_multi_tex_coord1f(target, s);
            }
        }
        // glMultiTexCoord2fv (opcode 199)
        199 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let s = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let t = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_multi_tex_coord2f(target, s, t);
            }
        }
        // glMultiTexCoord3fv (opcode 200)
        200 => {
            if data.len() >= 16 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let s = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let t = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let r = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_multi_tex_coord3f(target, s, t, r);
            }
        }
        // glMultiTexCoord4fv (opcode 201)
        201 => {
            if data.len() >= 20 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let s = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let t = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let r = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let q = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                osmesa::gl_multi_tex_coord4f(target, s, t, r, q);
            }
        }
        // glColorTable (opcode 4098 -- large render)
        4098 => {
            if data.len() >= 32 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let internalformat = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let width = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let format = if data.len() >= 36 {
                    u32::from_le_bytes([data[32], data[33], data[34], data[35]])
                } else { osmesa::GL_RGBA };
                let type_ = if data.len() >= 40 {
                    u32::from_le_bytes([data[36], data[37], data[38], data[39]])
                } else { osmesa::GL_UNSIGNED_BYTE };
                let pixel_data = if data.len() > 40 { &data[40..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_color_table(target, internalformat, width, format, type_, pixel_data);
                }
            }
        }
        // glTexSubImage1D (opcode 4099)
        4099 => {
            if data.len() >= 32 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = if data.len() >= 36 {
                    i32::from_le_bytes([data[32], data[33], data[34], data[35]])
                } else { 0 };
                let format = if data.len() >= 40 {
                    u32::from_le_bytes([data[36], data[37], data[38], data[39]])
                } else { osmesa::GL_RGBA };
                let type_ = if data.len() >= 44 {
                    u32::from_le_bytes([data[40], data[41], data[42], data[43]])
                } else { osmesa::GL_UNSIGNED_BYTE };
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_1d(target, level, xoffset, width, format, type_, pixel_data);
                }
            }
        }
        // glTexImage1D (opcode 4100)
        4100 => {
            if data.len() >= 36 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internal_format = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let border = if data.len() >= 40 {
                    i32::from_le_bytes([data[36], data[37], data[38], data[39]])
                } else { 0 };
                let format = if data.len() >= 44 {
                    u32::from_le_bytes([data[40], data[41], data[42], data[43]])
                } else { osmesa::GL_RGBA };
                let type_ = if data.len() >= 48 {
                    u32::from_le_bytes([data[44], data[45], data[46], data[47]])
                } else { osmesa::GL_UNSIGNED_BYTE };
                let pixel_data = if data.len() > 48 { &data[48..] } else { &[] };
                if pixel_data.is_empty() {
                    osmesa::gl_tex_image_1d_null(target, level, internal_format, width, border, format, type_);
                } else {
                    osmesa::gl_tex_image_1d(target, level, internal_format, width, border, format, type_, pixel_data);
                }
            }
        }
        // glTexImage2D (opcode 4101)
        4101 => {
            if data.len() >= 40 {
                let _swap_bytes = data[0];
                let _lsb_first = data[1];
                let _row_length = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let _skip_rows = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let _skip_pixels = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let _alignment = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internal_format = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let height = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let border = if data.len() >= 44 {
                    i32::from_le_bytes([data[40], data[41], data[42], data[43]])
                } else {
                    0
                };
                let format = if data.len() >= 48 {
                    u32::from_le_bytes([data[44], data[45], data[46], data[47]])
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 52 {
                    u32::from_le_bytes([data[48], data[49], data[50], data[51]])
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };

                let pixel_data = if data.len() > 52 {
                    &data[52..]
                } else {
                    &[]
                };

                if pixel_data.is_empty() {
                    osmesa::gl_tex_image_2d_null(target, level, internal_format, width, height, border, format, type_);
                } else {
                    osmesa::gl_tex_image_2d(target, level, internal_format, width, height, border, format, type_, pixel_data);
                }
            }
        }
        // glTexSubImage2D (opcode 4102)
        4102 => {
            if data.len() >= 44 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let yoffset = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let width = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let height = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let format = if data.len() >= 48 {
                    u32::from_le_bytes([data[44], data[45], data[46], data[47]])
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 52 {
                    u32::from_le_bytes([data[48], data[49], data[50], data[51]])
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };

                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_2d(target, level, xoffset, yoffset, width, height, format, type_, pixel_data);
                }
            }
        }
        // glCopyTexImage1D (opcode 4103)
        4103 => {
            if data.len() >= 28 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let level = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let internalformat = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let x = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let y = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let width = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let border = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                osmesa::gl_copy_tex_image_1d(target, level, internalformat, x, y, width, border);
            }
        }
        // glCopyTexImage2D (opcode 4104)
        4104 => {
            if data.len() >= 32 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let level = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let internalformat = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let x = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let y = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let width = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let height = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let border = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                osmesa::gl_copy_tex_image_2d(target, level, internalformat, x, y, width, height, border);
            }
        }
        // glCopyTexSubImage1D (opcode 4105)
        4105 => {
            if data.len() >= 24 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let level = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let xoffset = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let x = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let y = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let width = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                osmesa::gl_copy_tex_sub_image_1d(target, level, xoffset, x, y, width);
            }
        }
        // glCopyTexSubImage2D (opcode 4106)
        4106 => {
            if data.len() >= 32 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let level = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let xoffset = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let yoffset = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let x = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let y = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let width = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let height = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                osmesa::gl_copy_tex_sub_image_2d(target, level, xoffset, yoffset, x, y, width, height);
            }
        }
        // glConvolutionParameterf (opcode 4108)
        4108 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_convolution_parameterf(target, pname, param);
            }
        }
        // glConvolutionParameterfv (opcode 4109)
        4109 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0f32; count];
                for i in 0..count {
                    params[i] = f32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_convolution_parameterfv(target, pname, &params);
            }
        }
        // glConvolutionParameteri (opcode 4110)
        4110 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let param = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_convolution_parameteri(target, pname, param);
            }
        }
        // glConvolutionParameteriv (opcode 4111)
        4111 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let pname = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = (data.len() - 8) / 4;
                let mut params = vec![0i32; count];
                for i in 0..count {
                    params[i] = i32::from_le_bytes([data[8+i*4], data[9+i*4], data[10+i*4], data[11+i*4]]);
                }
                osmesa::gl_convolution_parameteriv(target, pname, &params);
            }
        }
        // glHistogram (opcode 4112)
        4112 => {
            if data.len() >= 16 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let width = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let internalformat = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let sink = data[12];
                osmesa::gl_histogram(target, width, internalformat, sink);
            }
        }
        // glMinmax (opcode 4113)
        4113 => {
            if data.len() >= 12 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let internalformat = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let sink = data[8];
                osmesa::gl_minmax(target, internalformat, sink);
            }
        }
        // glTexImage3D (opcode 4114)
        4114 => {
            if data.len() >= 56 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internal_format = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let height = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let depth = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let border = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let format = u32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let type_ = u32::from_le_bytes([data[52], data[53], data[54], data[55]]);
                let pixel_data = if data.len() > 56 { &data[56..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_image_3d(target, level, internal_format, width, height, depth, border, format, type_, pixel_data);
                }
            }
        }
        // glTexSubImage3D (opcode 4115)
        4115 => {
            if data.len() >= 60 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let yoffset = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let zoffset = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let width = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let height = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let depth = i32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let format = u32::from_le_bytes([data[52], data[53], data[54], data[55]]);
                let type_ = u32::from_le_bytes([data[56], data[57], data[58], data[59]]);
                let pixel_data = if data.len() > 60 { &data[60..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_3d(target, level, xoffset, yoffset, zoffset, width, height, depth, format, type_, pixel_data);
                }
            }
        }
        // glCompressedTexImage2D (opcode 4116 -- ARB version)
        4116 => {
            if data.len() >= 48 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internalformat = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let height = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let border = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let image_size = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);

                let pixel_data = if data.len() > 48 { &data[48..] } else { &[] };
                osmesa::gl_compressed_tex_image_2d(target, level, internalformat, width, height, border, image_size, pixel_data);
            }
        }
        // glBindTexture (opcode 4117)
        4117 => {
            if data.len() >= 8 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let texture = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_bind_texture(target, texture);
            }
        }
        // glGenTextures (opcode 4118)
        4118 => {
            if data.len() >= 4 {
                let n = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if n > 0 && n <= 256 {
                    let mut textures = vec![0u32; n as usize];
                    osmesa::gl_gen_textures(n, &mut textures);
                }
            }
        }
        // glDeleteTextures (opcode 4119)
        4119 => {
            if data.len() >= 4 {
                let n = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if n > 0 && data.len() >= 4 + n as usize * 4 {
                    let mut textures = vec![0u32; n as usize];
                    for i in 0..n as usize {
                        let off = 4 + i * 4;
                        textures[i] = u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
                    }
                    osmesa::gl_delete_textures(&textures);
                }
            }
        }
        // glCompressedTexImage1D (opcode 216)
        216 => {
            if data.len() >= 44 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internalformat = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let border = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let image_size = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                osmesa::gl_compressed_tex_image_1d(target, level, internalformat, width, border, image_size, pixel_data);
            }
        }
        // glCompressedTexImage2D (opcode 217 -- GL 1.3 non-ARB version)
        217 => {
            if data.len() >= 48 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internalformat = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let height = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let border = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let image_size = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let pixel_data = if data.len() > 48 { &data[48..] } else { &[] };
                osmesa::gl_compressed_tex_image_2d(target, level, internalformat, width, height, border, image_size, pixel_data);
            }
        }
        // glCompressedTexImage3D (opcode 218)
        218 => {
            if data.len() >= 52 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let internalformat = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let height = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let depth = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let border = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let image_size = i32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                osmesa::gl_compressed_tex_image_3d(target, level, internalformat, width, height, depth, border, image_size, pixel_data);
            }
        }
        // glCompressedTexSubImage1D (opcode 219)
        219 => {
            if data.len() >= 44 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let width = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let format = u32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let image_size = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_1d(target, level, xoffset, width, format, image_size, pixel_data);
            }
        }
        // glCompressedTexSubImage2D (opcode 220)
        220 => {
            if data.len() >= 52 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let yoffset = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let width = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let height = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let format = u32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let image_size = i32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_2d(target, level, xoffset, yoffset, width, height, format, image_size, pixel_data);
            }
        }
        // glCompressedTexSubImage3D (opcode 221)
        221 => {
            if data.len() >= 60 {
                let target = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let level = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let xoffset = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let yoffset = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let zoffset = i32::from_le_bytes([data[36], data[37], data[38], data[39]]);
                let width = i32::from_le_bytes([data[40], data[41], data[42], data[43]]);
                let height = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let depth = i32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let format = u32::from_le_bytes([data[52], data[53], data[54], data[55]]);
                let image_size = i32::from_le_bytes([data[56], data[57], data[58], data[59]]);
                let pixel_data = if data.len() > 60 { &data[60..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_3d(target, level, xoffset, yoffset, zoffset, width, height, depth, format, image_size, pixel_data);
            }
        }
        _ => return None,
    }
    Some(true)
}
