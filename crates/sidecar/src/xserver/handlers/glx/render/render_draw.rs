//! Drawing opcodes (glBegin, glEnd, glVertex*, glColor*, glNormal*, etc.).

use crate::osmesa;

/// Dispatch a GL drawing render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glCallList
        1 => {
            if data.len() >= 4 {
                let list = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_call_list(list);
            }
        }
        // glCallLists
        2 => {
            if data.len() >= 8 {
                let n = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let list_type = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let list_data = if data.len() > 8 { &data[8..] } else { &[] };
                osmesa::gl_call_lists(n, list_type, list_data);
            }
        }
        // glListBase
        3 => {
            if data.len() >= 4 {
                let base = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_list_base(base);
            }
        }
        // glBegin
        4 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_begin(mode);
            }
        }
        // glBitmap (opcode 5)
        5 => {
            if data.len() >= 24 {
                let w = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let h = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let xo = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let yo = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let xm = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let ym = f32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let bitmap_data = if data.len() > 24 { &data[24..] } else { &[] };
                osmesa::gl_bitmap(w, h, xo, yo, xm, ym, bitmap_data);
            }
        }
        // glColor3fv
        6 => {
            if data.len() >= 12 {
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_color3f(r, g, b);
            }
        }
        // glColor3ubv
        7 => {
            if data.len() >= 3 {
                osmesa::gl_color3ub(data[0], data[1], data[2]);
            }
        }
        // glColor3bv (3 signed bytes)
        8 => {
            if data.len() >= 3 {
                osmesa::gl_color3b(data[0] as i8, data[1] as i8, data[2] as i8);
            }
        }
        // glColor4fv
        9 => {
            if data.len() >= 16 {
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_color4f(r, g, b, a);
            }
        }
        // glColor3sv (3 i16)
        10 => {
            if data.len() >= 6 {
                let r = i16::from_le_bytes([data[0], data[1]]);
                let g = i16::from_le_bytes([data[2], data[3]]);
                let b = i16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_color3s(r, g, b);
            }
        }
        // glColor4ubv
        11 => {
            if data.len() >= 4 {
                osmesa::gl_color4ub(data[0], data[1], data[2], data[3]);
            }
        }
        // glColor3iv (3 i32)
        12 => {
            if data.len() >= 12 {
                let r = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_color3i(r, g, b);
            }
        }
        // glColor3uiv (3 u32)
        13 => {
            if data.len() >= 12 {
                let r = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_color3ui(r, g, b);
            }
        }
        // glColor3usv (3 u16)
        14 => {
            if data.len() >= 6 {
                let r = u16::from_le_bytes([data[0], data[1]]);
                let g = u16::from_le_bytes([data[2], data[3]]);
                let b = u16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_color3us(r, g, b);
            }
        }
        // glColor3dv (3 f64)
        15 => {
            if data.len() >= 24 {
                let r = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let g = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let b = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_color3d(r, g, b);
            }
        }
        // glColor4bv (4 signed bytes)
        16 => {
            if data.len() >= 4 {
                osmesa::gl_color4b(data[0] as i8, data[1] as i8, data[2] as i8, data[3] as i8);
            }
        }
        // glColor4sv (4 i16)
        17 => {
            if data.len() >= 8 {
                let r = i16::from_le_bytes([data[0], data[1]]);
                let g = i16::from_le_bytes([data[2], data[3]]);
                let b = i16::from_le_bytes([data[4], data[5]]);
                let a = i16::from_le_bytes([data[6], data[7]]);
                osmesa::gl_color4s(r, g, b, a);
            }
        }
        // glColor4iv (4 i32)
        18 => {
            if data.len() >= 16 {
                let r = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_color4i(r, g, b, a);
            }
        }
        // glColor4uiv (4 u32)
        19 => {
            if data.len() >= 16 {
                let r = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let a = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_color4ui(r, g, b, a);
            }
        }
        // glColor4usv (4 u16)
        20 => {
            if data.len() >= 8 {
                let r = u16::from_le_bytes([data[0], data[1]]);
                let g = u16::from_le_bytes([data[2], data[3]]);
                let b = u16::from_le_bytes([data[4], data[5]]);
                let a = u16::from_le_bytes([data[6], data[7]]);
                osmesa::gl_color4us(r, g, b, a);
            }
        }
        // glColor4dv (4 f64)
        21 => {
            if data.len() >= 32 {
                let r = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let g = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let b = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let a = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_color4d(r, g, b, a);
            }
        }
        // glEdgeFlagv (1 byte boolean)
        22 => {
            if !data.is_empty() {
                osmesa::gl_edge_flag(data[0]);
            }
        }
        // glEnd
        23 => {
            osmesa::gl_end();
        }
        // glIndexdv
        24 => {
            if data.len() >= 8 {
                let c = f64::from_le_bytes(data[0..8].try_into().unwrap());
                osmesa::gl_indexd(c);
            }
        }
        // glIndexfv
        25 => {
            if data.len() >= 4 {
                let c = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_indexf(c);
            }
        }
        // glIndexiv
        26 => {
            if data.len() >= 4 {
                let c = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_indexi(c);
            }
        }
        // glIndexsv
        27 => {
            if data.len() >= 2 {
                let c = i16::from_le_bytes([data[0], data[1]]);
                osmesa::gl_indexs(c);
            }
        }
        // glNormal3bv (3 signed bytes)
        28 => {
            if data.len() >= 3 {
                osmesa::gl_normal3b(data[0] as i8, data[1] as i8, data[2] as i8);
            }
        }
        // glNormal3dv (3 f64)
        29 => {
            if data.len() >= 24 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_normal3d(x, y, z);
            }
        }
        // glNormal3fv
        30 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_normal3f(x, y, z);
            }
        }
        // glNormal3iv (3 i32)
        31 => {
            if data.len() >= 12 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_normal3i(x, y, z);
            }
        }
        // glNormal3sv (3 i16)
        32 => {
            if data.len() >= 6 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                let z = i16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_normal3s(x, y, z);
            }
        }
        // glRasterPos2dv
        33 => {
            if data.len() >= 16 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_raster_pos2d(x, y);
            }
        }
        // glRasterPos2fv
        34 => {
            if data.len() >= 8 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_raster_pos2f(x, y);
            }
        }
        // glRasterPos2iv
        35 => {
            if data.len() >= 8 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_raster_pos2i(x, y);
            }
        }
        // glRasterPos2sv
        36 => {
            if data.len() >= 4 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                osmesa::gl_raster_pos2s(x, y);
            }
        }
        // glRasterPos3dv
        37 => {
            if data.len() >= 24 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_raster_pos3d(x, y, z);
            }
        }
        // glRasterPos3fv
        38 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_raster_pos3f(x, y, z);
            }
        }
        // glRasterPos3iv
        39 => {
            if data.len() >= 12 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_raster_pos3i(x, y, z);
            }
        }
        // glRasterPos3sv
        40 => {
            if data.len() >= 6 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                let z = i16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_raster_pos3s(x, y, z);
            }
        }
        // glRasterPos4dv
        41 => {
            if data.len() >= 32 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let w = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_raster_pos4d(x, y, z, w);
            }
        }
        // glRasterPos4fv
        42 => {
            if data.len() >= 16 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let w = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_raster_pos4f(x, y, z, w);
            }
        }
        // glRasterPos4iv
        43 => {
            if data.len() >= 16 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let w = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_raster_pos4i(x, y, z, w);
            }
        }
        // glRectf
        44 => {
            if data.len() >= 16 {
                let x1 = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y1 = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let x2 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let y2 = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_rectf(x1, y1, x2, y2);
            }
        }
        // glRectdv
        45 => {
            if data.len() >= 32 {
                let x1 = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y1 = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let x2 = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let y2 = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_rectd(x1, y1, x2, y2);
            }
        }
        // glRecti
        46 => {
            if data.len() >= 16 {
                let x1 = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y1 = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let x2 = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let y2 = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_recti(x1, y1, x2, y2);
            }
        }
        // glRectsv
        47 => {
            if data.len() >= 8 {
                let x1 = i16::from_le_bytes([data[0], data[1]]);
                let y1 = i16::from_le_bytes([data[2], data[3]]);
                let x2 = i16::from_le_bytes([data[4], data[5]]);
                let y2 = i16::from_le_bytes([data[6], data[7]]);
                osmesa::gl_rects(x1, y1, x2, y2);
            }
        }
        // glTexCoord1dv
        50 => {
            if data.len() >= 8 {
                let s = f64::from_le_bytes(data[0..8].try_into().unwrap());
                osmesa::gl_tex_coord1d(s);
            }
        }
        // glTexCoord1fv
        51 => {
            if data.len() >= 4 {
                let s = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_tex_coord1f(s);
            }
        }
        // glTexCoord1iv
        52 => {
            if data.len() >= 4 {
                let s = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_tex_coord1i(s);
            }
        }
        // glTexCoord1sv
        53 => {
            if data.len() >= 2 {
                let s = i16::from_le_bytes([data[0], data[1]]);
                osmesa::gl_tex_coord1s(s);
            }
        }
        // glTexCoord2fv
        54 => {
            if data.len() >= 8 {
                let s = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_tex_coord2f(s, t);
            }
        }
        // glTexCoord2dv
        55 => {
            if data.len() >= 16 {
                let s = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let t = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_tex_coord2d(s, t);
            }
        }
        // glTexCoord4fv
        56 => {
            if data.len() >= 16 {
                let s = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let r = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let q = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_tex_coord4f(s, t, r, q);
            }
        }
        // glVertex2fv
        57 => {
            if data.len() >= 8 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_vertex2f(x, y);
            }
        }
        // glVertex2iv
        58 => {
            if data.len() >= 8 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_vertex2i(x, y);
            }
        }
        // glVertex3fv
        59 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_vertex3f(x, y, z);
            }
        }
        // glVertex3iv
        60 => {
            if data.len() >= 12 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_vertex3i(x, y, z);
            }
        }
        // glVertex4fv
        61 => {
            if data.len() >= 16 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let w = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_vertex4f(x, y, z, w);
            }
        }
        // glTexCoord2iv
        62 => {
            if data.len() >= 8 {
                let s = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_tex_coord2i(s, t);
            }
        }
        // glTexCoord2sv
        63 => {
            if data.len() >= 4 {
                let s = i16::from_le_bytes([data[0], data[1]]);
                let t = i16::from_le_bytes([data[2], data[3]]);
                osmesa::gl_tex_coord2s(s, t);
            }
        }
        // glTexCoord3dv
        64 => {
            if data.len() >= 24 {
                let s = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let t = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let r = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_tex_coord3d(s, t, r);
            }
        }
        // glTexCoord3fv
        65 => {
            if data.len() >= 12 {
                let s = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let r = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_coord3f(s, t, r);
            }
        }
        // glTexCoord3iv
        66 => {
            if data.len() >= 12 {
                let s = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let r = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_tex_coord3i(s, t, r);
            }
        }
        // glTexCoord3sv
        67 => {
            if data.len() >= 6 {
                let s = i16::from_le_bytes([data[0], data[1]]);
                let t = i16::from_le_bytes([data[2], data[3]]);
                let r = i16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_tex_coord3s(s, t, r);
            }
        }
        // glVertex2dv
        70 => {
            if data.len() >= 16 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_vertex2d(x, y);
            }
        }
        // glVertex2sv
        71 => {
            if data.len() >= 4 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                osmesa::gl_vertex2s(x, y);
            }
        }
        // glVertex3dv
        72 => {
            if data.len() >= 24 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_vertex3d(x, y, z);
            }
        }
        // glVertex3sv
        73 => {
            if data.len() >= 6 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                let z = i16::from_le_bytes([data[4], data[5]]);
                osmesa::gl_vertex3s(x, y, z);
            }
        }
        // glVertex4dv
        74 => {
            if data.len() >= 32 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let w = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_vertex4d(x, y, z, w);
            }
        }
        // glVertex4iv
        75 => {
            if data.len() >= 16 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let w = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_vertex4i(x, y, z, w);
            }
        }
        // glVertex4sv
        76 => {
            if data.len() >= 8 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                let z = i16::from_le_bytes([data[4], data[5]]);
                let w = i16::from_le_bytes([data[6], data[7]]);
                osmesa::gl_vertex4s(x, y, z, w);
            }
        }
        // glTexCoord4dv
        48 => {
            if data.len() >= 32 {
                let s = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let t = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let r = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let q = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_tex_coord4d(s, t, r, q);
            }
        }
        // glTexCoord4iv
        49 => {
            if data.len() >= 16 {
                let s = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let t = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let r = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let q = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_tex_coord4i(s, t, r, q);
            }
        }
        // glEvalCoord1dv (opcode 151)
        151 => {
            if data.len() >= 8 {
                let u = f64::from_le_bytes(data[0..8].try_into().unwrap());
                osmesa::gl_eval_coord1d(u);
            }
        }
        // glEvalCoord1fv (opcode 152)
        152 => {
            if data.len() >= 4 {
                let u = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_eval_coord1f(u);
            }
        }
        // glEvalCoord2dv (opcode 153)
        153 => {
            if data.len() >= 16 {
                let u = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let v = f64::from_le_bytes(data[8..16].try_into().unwrap());
                osmesa::gl_eval_coord2d(u, v);
            }
        }
        // glEvalCoord2fv (opcode 154)
        154 => {
            if data.len() >= 8 {
                let u = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let v = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_eval_coord2f(u, v);
            }
        }
        // glEvalMesh1 (opcode 155)
        155 => {
            if data.len() >= 12 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let i1 = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let i2 = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_eval_mesh1(mode, i1, i2);
            }
        }
        // glEvalPoint1 (opcode 156)
        156 => {
            if data.len() >= 4 {
                let i = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_eval_point1(i);
            }
        }
        // glEvalMesh2 (opcode 157)
        157 => {
            if data.len() >= 20 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let i1 = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let i2 = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let j1 = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let j2 = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                osmesa::gl_eval_mesh2(mode, i1, i2, j1, j2);
            }
        }
        // glEvalPoint2 (opcode 158)
        158 => {
            if data.len() >= 8 {
                let i = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let j = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_eval_point2(i, j);
            }
        }
        // glMap1f (opcode 165)
        165 => {
            if data.len() >= 20 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let u2 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let stride = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let order = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let count = (data.len() - 20) / 4;
                let mut points = vec![0f32; count];
                for i in 0..count {
                    points[i] = f32::from_le_bytes([data[20+i*4], data[21+i*4], data[22+i*4], data[23+i*4]]);
                }
                osmesa::gl_map1f(target, u1, u2, stride, order, &points);
            }
        }
        // glMap1d (opcode 166)
        166 => {
            if data.len() >= 32 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f64::from_le_bytes(data[4..12].try_into().unwrap());
                let u2 = f64::from_le_bytes(data[12..20].try_into().unwrap());
                let stride = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let order = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let count = (data.len() - 28) / 8;
                let mut points = vec![0f64; count];
                for i in 0..count {
                    points[i] = f64::from_le_bytes(data[28+i*8..36+i*8].try_into().unwrap());
                }
                osmesa::gl_map1d(target, u1, u2, stride, order, &points);
            }
        }
        // glMap2f (opcode 167)
        167 => {
            if data.len() >= 36 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let u2 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let ustride = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let uorder = i32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let v1 = f32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let v2 = f32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let vstride = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let vorder = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let count = (data.len() - 36) / 4;
                let mut points = vec![0f32; count];
                for i in 0..count {
                    points[i] = f32::from_le_bytes([data[36+i*4], data[37+i*4], data[38+i*4], data[39+i*4]]);
                }
                osmesa::gl_map2f(target, u1, u2, ustride, uorder, v1, v2, vstride, vorder, &points);
            }
        }
        // glMap2d (opcode 168)
        168 => {
            if data.len() >= 52 {
                let target = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f64::from_le_bytes(data[4..12].try_into().unwrap());
                let u2 = f64::from_le_bytes(data[12..20].try_into().unwrap());
                let ustride = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let uorder = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let v1 = f64::from_le_bytes(data[28..36].try_into().unwrap());
                let v2 = f64::from_le_bytes(data[36..44].try_into().unwrap());
                let vstride = i32::from_le_bytes([data[44], data[45], data[46], data[47]]);
                let vorder = i32::from_le_bytes([data[48], data[49], data[50], data[51]]);
                let count = (data.len() - 52) / 8;
                let mut points = vec![0f64; count];
                for i in 0..count {
                    points[i] = f64::from_le_bytes(data[52+i*8..60+i*8].try_into().unwrap());
                }
                osmesa::gl_map2d(target, u1, u2, ustride, uorder, v1, v2, vstride, vorder, &points);
            }
        }
        // glMapGrid1f (opcode 169)
        169 => {
            if data.len() >= 12 {
                let un = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let u2 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_map_grid1f(un, u1, u2);
            }
        }
        // glMapGrid1d (opcode 170)
        170 => {
            if data.len() >= 20 {
                let un = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f64::from_le_bytes(data[4..12].try_into().unwrap());
                let u2 = f64::from_le_bytes(data[12..20].try_into().unwrap());
                osmesa::gl_map_grid1d(un, u1, u2);
            }
        }
        // glMapGrid2f (opcode 171)
        171 => {
            if data.len() >= 24 {
                let un = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let u2 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let vn = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let v1 = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                let v2 = f32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                osmesa::gl_map_grid2f(un, u1, u2, vn, v1, v2);
            }
        }
        // glMapGrid2d (opcode 172)
        172 => {
            if data.len() >= 40 {
                let un = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let u1 = f64::from_le_bytes(data[4..12].try_into().unwrap());
                let u2 = f64::from_le_bytes(data[12..20].try_into().unwrap());
                let vn = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let v1 = f64::from_le_bytes(data[24..32].try_into().unwrap());
                let v2 = f64::from_le_bytes(data[32..40].try_into().unwrap());
                osmesa::gl_map_grid2d(un, u1, u2, vn, v1, v2);
            }
        }
        // glCopyPixels (opcode 173)
        173 => {
            if data.len() >= 20 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let w = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let h = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                let type_ = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                osmesa::gl_copy_pixels(x, y, w, h, type_);
            }
        }
        // glRasterPos4sv (opcode 193)
        193 => {
            if data.len() >= 8 {
                let x = i16::from_le_bytes([data[0], data[1]]);
                let y = i16::from_le_bytes([data[2], data[3]]);
                let z = i16::from_le_bytes([data[4], data[5]]);
                let w = i16::from_le_bytes([data[6], data[7]]);
                osmesa::gl_raster_pos4s(x, y, z, w);
            }
        }
        // glIndexubv (opcode 194)
        194 => {
            if !data.is_empty() {
                osmesa::gl_indexub(data[0]);
            }
        }
        // glNewList (opcode 195)
        195 => {
            if data.len() >= 8 {
                let list = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let mode = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_new_list(list, mode);
            }
        }
        // glEndList (opcode 196)
        196 => {
            osmesa::gl_end_list();
        }
        // glArrayElement (opcode 206)
        206 => {
            if data.len() >= 4 {
                let i = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_array_element(i);
            }
        }
        // Opcodes 202-205, 207-208: MultiTexCoord variants (double, int, short)
        202..=205 | 207..=208 => {
            // Multi-texture coordinate variants (non-float) -- silently accepted
        }
        // glDrawPixels (opcode 4107)
        4107 => {
            if data.len() >= 36 {
                let width = i32::from_le_bytes([data[20], data[21], data[22], data[23]]);
                let height = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
                let format = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
                let type_ = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);
                let pixel_data = if data.len() > 36 { &data[36..] } else { &[] };
                if !pixel_data.is_empty() {
                    osmesa::gl_draw_pixels(width, height, format, type_, pixel_data);
                }
            }
        }
        // glFogCoordf (opcode 4124)
        4124 => {
            if data.len() >= 4 {
                let coord = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_fog_coordf(coord);
            }
        }
        // glFogCoordd (opcode 4125)
        4125 => {
            if data.len() >= 8 {
                let coord = f64::from_le_bytes(data[0..8].try_into().unwrap());
                osmesa::gl_fog_coordd(coord);
            }
        }
        // glSecondaryColor3fv (opcode 4126)
        4126 => {
            if data.len() >= 12 {
                let r = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let g = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let b = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_secondary_color3f(r, g, b);
            }
        }
        // glSecondaryColor3ubv (opcode 4127)
        4127 => {
            if data.len() >= 3 {
                osmesa::gl_secondary_color3ub(data[0], data[1], data[2]);
            }
        }
        // glWindowPos2fv (opcode 4128)
        4128 => {
            if data.len() >= 8 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                osmesa::gl_window_pos2f(x, y);
            }
        }
        // glWindowPos3fv (opcode 230)
        230 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_window_pos3f(x, y, z);
            }
        }
        // glDrawArrays (opcode 4116)
        //
        // GLX wire format:
        //   mode(4) + first(4) + count(4) + num_components(4) = 16-byte header
        //   Then `num_components` header entries, each 12 bytes:
        //     data_type(4) + index(4) + num_bytes(4)
        //   data_type bitfield: bit 14 = vertex, bit 13 = normal,
        //                       bit 12 = color, bit 11 = texcoord
        //   Followed by interleaved vertex data.
        4116 => {
            if data.len() >= 16 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let first = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let count = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let num_components =
                    u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;

                let header_size = 16 + num_components * 12;
                if data.len() >= header_size {
                    let vertex_data = &data[header_size..];
                    let mut offset: usize = 0;
                    let mut enabled = Vec::new();

                    for i in 0..num_components {
                        let hdr_off = 16 + i * 12;
                        let data_type = u32::from_le_bytes([
                            data[hdr_off],
                            data[hdr_off + 1],
                            data[hdr_off + 2],
                            data[hdr_off + 3],
                        ]);
                        let _index = u32::from_le_bytes([
                            data[hdr_off + 4],
                            data[hdr_off + 5],
                            data[hdr_off + 6],
                            data[hdr_off + 7],
                        ]);
                        let num_bytes = u32::from_le_bytes([
                            data[hdr_off + 8],
                            data[hdr_off + 9],
                            data[hdr_off + 10],
                            data[hdr_off + 11],
                        ]) as usize;

                        // The low 4 bits encode the number of values per element;
                        // bits 4..13 encode the GL type; bits 14+ select the array.
                        let num_values = (data_type & 0x0F) as i32;
                        let gl_type = (data_type >> 4) & 0x3FF;
                        let array_bits = data_type >> 14;

                        if offset + num_bytes <= vertex_data.len() {
                            let ptr = vertex_data[offset..].as_ptr() as *const std::ffi::c_void;
                            let stride = if count > 0 {
                                (num_bytes as i32) / count
                            } else {
                                0
                            };

                            unsafe {
                                if array_bits & 0x01 != 0 {
                                    // vertex
                                    osmesa::gl_enable_client_state(osmesa::GL_VERTEX_ARRAY);
                                    osmesa::gl_vertex_pointer(num_values, gl_type, stride, ptr);
                                    enabled.push(osmesa::GL_VERTEX_ARRAY);
                                }
                                if array_bits & 0x02 != 0 {
                                    // normal
                                    osmesa::gl_enable_client_state(osmesa::GL_NORMAL_ARRAY);
                                    osmesa::gl_normal_pointer(gl_type, stride, ptr);
                                    enabled.push(osmesa::GL_NORMAL_ARRAY);
                                }
                                if array_bits & 0x04 != 0 {
                                    // color
                                    osmesa::gl_enable_client_state(osmesa::GL_COLOR_ARRAY);
                                    osmesa::gl_color_pointer(num_values, gl_type, stride, ptr);
                                    enabled.push(osmesa::GL_COLOR_ARRAY);
                                }
                                if array_bits & 0x08 != 0 {
                                    // texcoord
                                    osmesa::gl_enable_client_state(
                                        osmesa::GL_TEXTURE_COORD_ARRAY,
                                    );
                                    osmesa::gl_tex_coord_pointer(
                                        num_values, gl_type, stride, ptr,
                                    );
                                    enabled.push(osmesa::GL_TEXTURE_COORD_ARRAY);
                                }
                            }
                        }
                        offset += num_bytes;
                    }

                    osmesa::gl_draw_arrays(mode, first, count);

                    for arr in enabled {
                        osmesa::gl_disable_client_state(arr);
                    }
                }
            }
        }
        // glDrawElements (opcode 4117)
        //
        // Wire format: mode(4) + count(4) + type(4) + indices_data...
        // followed by vertex data that should already be set up via
        // prior DrawArrays header / vertex pointer calls.
        // In practice the GLX indirect protocol sends the index buffer inline.
        4117 => {
            if data.len() >= 12 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let count = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let index_type = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let indices = &data[12..];
                unsafe {
                    osmesa::gl_draw_elements(
                        mode,
                        count,
                        index_type,
                        indices.as_ptr() as *const std::ffi::c_void,
                    );
                }
            }
        }
        _ => return None,
    }
    Some(true)
}
