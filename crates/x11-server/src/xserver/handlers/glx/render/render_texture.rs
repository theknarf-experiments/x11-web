//! Texture opcodes (glBindTexture, glTexImage*, glTexParameter*, etc.).

use crate::osmesa;

/// Dispatch a GL texture render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glTexParameteri
        105 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_tex_parameteri(target, pname, param);
            }
        }
        // glTexParameterf (opcode 106)
        106 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_tex_parameterf(target, pname, param);
            }
        }
        // glTexParameterfv (opcode 107)
        107 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_parameterfv(target, pname, &params);
            }
        }
        // glTexParameteriv (opcode 109)
        109 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_parameteriv(target, pname, &params);
            }
        }
        // glTexEnvf (opcode 115)
        115 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_tex_envf(target, pname, param);
            }
        }
        // glTexEnvfv (opcode 116)
        116 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_envfv(target, pname, &params);
            }
        }
        // glTexEnvi (opcode 117)
        117 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_tex_envi(target, pname, param);
            }
        }
        // glTexEnviv (opcode 118)
        118 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_enviv(target, pname, &params);
            }
        }
        // glTexGend (opcode 119)
        119 => {
            if data.len() >= 16 {
                let coord = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f64_le(data, 8);
                osmesa::gl_tex_gend(coord, pname, param);
            }
        }
        // glTexGendv (opcode 120)
        120 => {
            if data.len() >= 12 {
                let coord = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let count = (data.len() - 8) / 8;
                let mut params = vec![0f64; count];
                for i in 0..count {
                    params[i] = f64::from_le_bytes(data[8 + i * 8..16 + i * 8].try_into().unwrap());
                }
                osmesa::gl_tex_gendv(coord, pname, &params);
            }
        }
        // glTexGenf (opcode 121)
        121 => {
            if data.len() >= 12 {
                let coord = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_tex_genf(coord, pname, param);
            }
        }
        // glTexGenfv (opcode 122)
        122 => {
            if data.len() >= 12 {
                let coord = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_genfv(coord, pname, &params);
            }
        }
        // glTexGeni (opcode 123)
        123 => {
            if data.len() >= 12 {
                let coord = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_tex_geni(coord, pname, param);
            }
        }
        // glTexGeniv (opcode 124)
        124 => {
            if data.len() >= 12 {
                let coord = super::read_u32_le(data, 0);
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
                osmesa::gl_tex_geniv(coord, pname, &params);
            }
        }
        // glActiveTexture (opcode 197)
        197 => {
            if data.len() >= 4 {
                let texture = super::read_u32_le(data, 0);
                osmesa::gl_active_texture(texture);
            }
        }
        // glMultiTexCoord1fv (opcode 198)
        198 => {
            if data.len() >= 8 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_f32_le(data, 4);
                osmesa::gl_multi_tex_coord1f(target, s);
            }
        }
        // glMultiTexCoord2fv (opcode 199)
        199 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_f32_le(data, 4);
                let t = super::read_f32_le(data, 8);
                osmesa::gl_multi_tex_coord2f(target, s, t);
            }
        }
        // glMultiTexCoord3fv (opcode 200)
        200 => {
            if data.len() >= 16 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_f32_le(data, 4);
                let t = super::read_f32_le(data, 8);
                let r = super::read_f32_le(data, 12);
                osmesa::gl_multi_tex_coord3f(target, s, t, r);
            }
        }
        // glMultiTexCoord4fv (opcode 201)
        201 => {
            if data.len() >= 20 {
                let target = super::read_u32_le(data, 0);
                let s = super::read_f32_le(data, 4);
                let t = super::read_f32_le(data, 8);
                let r = super::read_f32_le(data, 12);
                let q = super::read_f32_le(data, 16);
                osmesa::gl_multi_tex_coord4f(target, s, t, r, q);
            }
        }
        // glColorTable (opcode 4098 -- large render)
        4098 => {
            if data.len() >= 32 {
                let target = super::read_u32_le(data, 20);
                let internalformat = super::read_u32_le(data, 24);
                let width = super::read_i32_le(data, 28);
                let format = if data.len() >= 36 {
                    super::read_u32_le(data, 32)
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 40 {
                    super::read_u32_le(data, 36)
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };
                let pixel_data = if data.len() > 40 { &data[40..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_color_table(
                        target,
                        internalformat,
                        width,
                        format,
                        type_,
                        pixel_data,
                    );
                }
            }
        }
        // glTexSubImage1D (opcode 4099)
        4099 => {
            if data.len() >= 32 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let width = if data.len() >= 36 {
                    super::read_i32_le(data, 32)
                } else {
                    0
                };
                let format = if data.len() >= 40 {
                    super::read_u32_le(data, 36)
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 44 {
                    super::read_u32_le(data, 40)
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_1d(
                        target, level, xoffset, width, format, type_, pixel_data,
                    );
                }
            }
        }
        // glTexImage1D (opcode 4100)
        4100 => {
            if data.len() >= 36 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internal_format = super::read_i32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let border = if data.len() >= 40 {
                    super::read_i32_le(data, 36)
                } else {
                    0
                };
                let format = if data.len() >= 44 {
                    super::read_u32_le(data, 40)
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 48 {
                    super::read_u32_le(data, 44)
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };
                let pixel_data = if data.len() > 48 { &data[48..] } else { &[] };
                if pixel_data.is_empty() {
                    osmesa::gl_tex_image_1d_null(
                        target,
                        level,
                        internal_format,
                        width,
                        border,
                        format,
                        type_,
                    );
                } else {
                    osmesa::gl_tex_image_1d(
                        target,
                        level,
                        internal_format,
                        width,
                        border,
                        format,
                        type_,
                        pixel_data,
                    );
                }
            }
        }
        // glTexImage2D (opcode 4101)
        4101 => {
            if data.len() >= 40 {
                let _swap_bytes = data[0];
                let _lsb_first = data[1];
                let _row_length = super::read_i32_le(data, 4);
                let _skip_rows = super::read_i32_le(data, 8);
                let _skip_pixels = super::read_i32_le(data, 12);
                let _alignment = super::read_i32_le(data, 16);
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internal_format = super::read_i32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let height = super::read_i32_le(data, 36);
                let border = if data.len() >= 44 {
                    super::read_i32_le(data, 40)
                } else {
                    0
                };
                let format = if data.len() >= 48 {
                    super::read_u32_le(data, 44)
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 52 {
                    super::read_u32_le(data, 48)
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };

                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };

                if pixel_data.is_empty() {
                    osmesa::gl_tex_image_2d_null(
                        target,
                        level,
                        internal_format,
                        width,
                        height,
                        border,
                        format,
                        type_,
                    );
                } else {
                    osmesa::gl_tex_image_2d(
                        target,
                        level,
                        internal_format,
                        width,
                        height,
                        border,
                        format,
                        type_,
                        pixel_data,
                    );
                }
            }
        }
        // glTexSubImage2D (opcode 4102)
        4102 => {
            if data.len() >= 44 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let yoffset = super::read_i32_le(data, 32);
                let width = super::read_i32_le(data, 36);
                let height = super::read_i32_le(data, 40);
                let format = if data.len() >= 48 {
                    super::read_u32_le(data, 44)
                } else {
                    osmesa::GL_RGBA
                };
                let type_ = if data.len() >= 52 {
                    super::read_u32_le(data, 48)
                } else {
                    osmesa::GL_UNSIGNED_BYTE
                };

                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_2d(
                        target, level, xoffset, yoffset, width, height, format, type_, pixel_data,
                    );
                }
            }
        }
        // glCopyTexImage1D (opcode 4103)
        4103 => {
            if data.len() >= 28 {
                let target = super::read_u32_le(data, 0);
                let level = super::read_i32_le(data, 4);
                let internalformat = super::read_u32_le(data, 8);
                let x = super::read_i32_le(data, 12);
                let y = super::read_i32_le(data, 16);
                let width = super::read_i32_le(data, 20);
                let border = super::read_i32_le(data, 24);
                osmesa::gl_copy_tex_image_1d(target, level, internalformat, x, y, width, border);
            }
        }
        // glCopyTexImage2D (opcode 4104)
        4104 => {
            if data.len() >= 32 {
                let target = super::read_u32_le(data, 0);
                let level = super::read_i32_le(data, 4);
                let internalformat = super::read_u32_le(data, 8);
                let x = super::read_i32_le(data, 12);
                let y = super::read_i32_le(data, 16);
                let width = super::read_i32_le(data, 20);
                let height = super::read_i32_le(data, 24);
                let border = super::read_i32_le(data, 28);
                osmesa::gl_copy_tex_image_2d(
                    target,
                    level,
                    internalformat,
                    x,
                    y,
                    width,
                    height,
                    border,
                );
            }
        }
        // glCopyTexSubImage1D (opcode 4105)
        4105 => {
            if data.len() >= 24 {
                let target = super::read_u32_le(data, 0);
                let level = super::read_i32_le(data, 4);
                let xoffset = super::read_i32_le(data, 8);
                let x = super::read_i32_le(data, 12);
                let y = super::read_i32_le(data, 16);
                let width = super::read_i32_le(data, 20);
                osmesa::gl_copy_tex_sub_image_1d(target, level, xoffset, x, y, width);
            }
        }
        // glCopyTexSubImage2D (opcode 4106)
        4106 => {
            if data.len() >= 32 {
                let target = super::read_u32_le(data, 0);
                let level = super::read_i32_le(data, 4);
                let xoffset = super::read_i32_le(data, 8);
                let yoffset = super::read_i32_le(data, 12);
                let x = super::read_i32_le(data, 16);
                let y = super::read_i32_le(data, 20);
                let width = super::read_i32_le(data, 24);
                let height = super::read_i32_le(data, 28);
                osmesa::gl_copy_tex_sub_image_2d(
                    target, level, xoffset, yoffset, x, y, width, height,
                );
            }
        }
        // glConvolutionParameterf (opcode 4108)
        4108 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_f32_le(data, 8);
                osmesa::gl_convolution_parameterf(target, pname, param);
            }
        }
        // glConvolutionParameterfv (opcode 4109)
        4109 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_convolution_parameterfv(target, pname, &params);
            }
        }
        // glConvolutionParameteri (opcode 4110)
        4110 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let pname = super::read_u32_le(data, 4);
                let param = super::read_i32_le(data, 8);
                osmesa::gl_convolution_parameteri(target, pname, param);
            }
        }
        // glConvolutionParameteriv (opcode 4111)
        4111 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
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
                osmesa::gl_convolution_parameteriv(target, pname, &params);
            }
        }
        // glHistogram (opcode 4112)
        4112 => {
            if data.len() >= 16 {
                let target = super::read_u32_le(data, 0);
                let width = super::read_i32_le(data, 4);
                let internalformat = super::read_u32_le(data, 8);
                let sink = data[12];
                osmesa::gl_histogram(target, width, internalformat, sink);
            }
        }
        // glMinmax (opcode 4113)
        4113 => {
            if data.len() >= 12 {
                let target = super::read_u32_le(data, 0);
                let internalformat = super::read_u32_le(data, 4);
                let sink = data[8];
                osmesa::gl_minmax(target, internalformat, sink);
            }
        }
        // glTexImage3D (opcode 4114)
        4114 => {
            if data.len() >= 56 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let internal_format = super::read_i32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let height = super::read_i32_le(data, 36);
                let depth = super::read_i32_le(data, 40);
                let border = super::read_i32_le(data, 44);
                let format = super::read_u32_le(data, 48);
                let type_ = super::read_u32_le(data, 52);
                let pixel_data = if data.len() > 56 { &data[56..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_image_3d(
                        target,
                        level,
                        internal_format,
                        width,
                        height,
                        depth,
                        border,
                        format,
                        type_,
                        pixel_data,
                    );
                }
            }
        }
        // glTexSubImage3D (opcode 4115)
        4115 => {
            if data.len() >= 60 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let yoffset = super::read_i32_le(data, 32);
                let zoffset = super::read_i32_le(data, 36);
                let width = super::read_i32_le(data, 40);
                let height = super::read_i32_le(data, 44);
                let depth = super::read_i32_le(data, 48);
                let format = super::read_u32_le(data, 52);
                let type_ = super::read_u32_le(data, 56);
                let pixel_data = if data.len() > 60 { &data[60..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_tex_sub_image_3d(
                        target, level, xoffset, yoffset, zoffset, width, height, depth, format,
                        type_, pixel_data,
                    );
                }
            }
        }
        // glCompressedTexImage2D (opcode 4116 -- ARB version)
        4116 => {
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
        // glBindTexture (opcode 4117)
        4117 => {
            if data.len() >= 8 {
                let target = super::read_u32_le(data, 0);
                let texture = super::read_u32_le(data, 4);
                osmesa::gl_bind_texture(target, texture);
            }
        }
        // glGenTextures (opcode 4118)
        4118 => {
            if data.len() >= 4 {
                let n = super::read_i32_le(data, 0);
                if n > 0 && n <= 256 {
                    let mut textures = vec![0u32; n as usize];
                    osmesa::gl_gen_textures(n, &mut textures);
                }
            }
        }
        // glDeleteTextures (opcode 4119)
        4119 => {
            if data.len() >= 4 {
                let n = super::read_i32_le(data, 0);
                if n > 0 && data.len() >= 4 + n as usize * 4 {
                    let textures: Vec<u32> = (0..n as usize)
                        .map(|i| {
                            let off = 4 + i * 4;
                            u32::from_le_bytes([
                                data[off],
                                data[off + 1],
                                data[off + 2],
                                data[off + 3],
                            ])
                        })
                        .collect();
                    osmesa::gl_delete_textures(&textures);
                }
            }
        }
        // glCompressedTexImage1D (opcode 216)
        216 => {
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
        // glCompressedTexImage2D (opcode 217 -- GL 1.3 non-ARB version)
        217 => {
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
        // glCompressedTexImage3D (opcode 218)
        218 => {
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
        // glCompressedTexSubImage1D (opcode 219)
        219 => {
            if data.len() >= 44 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let width = super::read_i32_le(data, 32);
                let format = super::read_u32_le(data, 36);
                let image_size = super::read_i32_le(data, 40);
                let pixel_data = if data.len() > 44 { &data[44..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_1d(
                    target, level, xoffset, width, format, image_size, pixel_data,
                );
            }
        }
        // glCompressedTexSubImage2D (opcode 220)
        220 => {
            if data.len() >= 52 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let yoffset = super::read_i32_le(data, 32);
                let width = super::read_i32_le(data, 36);
                let height = super::read_i32_le(data, 40);
                let format = super::read_u32_le(data, 44);
                let image_size = super::read_i32_le(data, 48);
                let pixel_data = if data.len() > 52 { &data[52..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_2d(
                    target, level, xoffset, yoffset, width, height, format, image_size, pixel_data,
                );
            }
        }
        // glCompressedTexSubImage3D (opcode 221)
        221 => {
            if data.len() >= 60 {
                let target = super::read_u32_le(data, 20);
                let level = super::read_i32_le(data, 24);
                let xoffset = super::read_i32_le(data, 28);
                let yoffset = super::read_i32_le(data, 32);
                let zoffset = super::read_i32_le(data, 36);
                let width = super::read_i32_le(data, 40);
                let height = super::read_i32_le(data, 44);
                let depth = super::read_i32_le(data, 48);
                let format = super::read_u32_le(data, 52);
                let image_size = super::read_i32_le(data, 56);
                let pixel_data = if data.len() > 60 { &data[60..] } else { &[] };
                osmesa::gl_compressed_tex_sub_image_3d(
                    target, level, xoffset, yoffset, zoffset, width, height, depth, format,
                    image_size, pixel_data,
                );
            }
        }
        _ => return None,
    }
    Some(true)
}
