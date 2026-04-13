//! Matrix and transform opcodes (glMatrixMode, glLoadIdentity, glRotate*, etc.).

use crate::osmesa;

/// Dispatch a GL matrix/transform render opcode. Returns `true` if handled.
pub(crate) fn dispatch(opcode: u16, data: &[u8]) -> Option<bool> {
    match opcode {
        // glFrustum (opcode 175)
        175 => {
            if data.len() >= 48 {
                let left = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let right = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let bottom = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let top = f64::from_le_bytes(data[24..32].try_into().unwrap());
                let near = f64::from_le_bytes(data[32..40].try_into().unwrap());
                let far = f64::from_le_bytes(data[40..48].try_into().unwrap());
                osmesa::gl_frustum(left, right, bottom, top, near, far);
            }
        }
        // glLoadIdentity (opcode 176)
        176 => {
            osmesa::gl_load_identity();
        }
        // glLoadMatrixf (opcode 177)
        177 => {
            if data.len() >= 64 {
                let mut m = [0f32; 16];
                for i in 0..16 {
                    m[i] = f32::from_le_bytes([
                        data[i * 4],
                        data[i * 4 + 1],
                        data[i * 4 + 2],
                        data[i * 4 + 3],
                    ]);
                }
                osmesa::gl_load_matrixf(&m);
            }
        }
        // glLoadMatrixd (opcode 178)
        178 => {
            if data.len() >= 128 {
                let mut m = [0f64; 16];
                for i in 0..16 {
                    m[i] = f64::from_le_bytes([
                        data[i * 8],
                        data[i * 8 + 1],
                        data[i * 8 + 2],
                        data[i * 8 + 3],
                        data[i * 8 + 4],
                        data[i * 8 + 5],
                        data[i * 8 + 6],
                        data[i * 8 + 7],
                    ]);
                }
                osmesa::gl_load_matrixd(&m);
            }
        }
        // glMultMatrixf (opcode 179)
        179 => {
            if data.len() >= 64 {
                let mut m = [0f32; 16];
                for i in 0..16 {
                    m[i] = f32::from_le_bytes([
                        data[i * 4],
                        data[i * 4 + 1],
                        data[i * 4 + 2],
                        data[i * 4 + 3],
                    ]);
                }
                osmesa::gl_mult_matrixf(&m);
            }
        }
        // glMultMatrixd (opcode 180)
        180 => {
            if data.len() >= 128 {
                let mut m = [0f64; 16];
                for i in 0..16 {
                    m[i] = f64::from_le_bytes([
                        data[i * 8],
                        data[i * 8 + 1],
                        data[i * 8 + 2],
                        data[i * 8 + 3],
                        data[i * 8 + 4],
                        data[i * 8 + 5],
                        data[i * 8 + 6],
                        data[i * 8 + 7],
                    ]);
                }
                osmesa::gl_mult_matrixd(&m);
            }
        }
        // glMatrixMode (opcode 181)
        181 => {
            if data.len() >= 4 {
                let mode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                osmesa::gl_matrix_mode(mode);
            }
        }
        // glOrtho (opcode 182)
        182 => {
            if data.len() >= 48 {
                let left = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let right = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let bottom = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let top = f64::from_le_bytes(data[24..32].try_into().unwrap());
                let near = f64::from_le_bytes(data[32..40].try_into().unwrap());
                let far = f64::from_le_bytes(data[40..48].try_into().unwrap());
                osmesa::gl_ortho(left, right, bottom, top, near, far);
            }
        }
        // glPopMatrix (opcode 183)
        183 => {
            osmesa::gl_pop_matrix();
        }
        // glPushMatrix (opcode 184)
        184 => {
            osmesa::gl_push_matrix();
        }
        // glRotated (opcode 185)
        185 => {
            if data.len() >= 32 {
                let angle = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let x = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let y = f64::from_le_bytes(data[16..24].try_into().unwrap());
                let z = f64::from_le_bytes(data[24..32].try_into().unwrap());
                osmesa::gl_rotated(angle, x, y, z);
            }
        }
        // glRotatef (opcode 186)
        186 => {
            if data.len() >= 16 {
                let angle = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let x = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let y = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let z = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_rotatef(angle, x, y, z);
            }
        }
        // glScaled (opcode 187)
        187 => {
            if data.len() >= 24 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_scaled(x, y, z);
            }
        }
        // glScalef (opcode 188)
        188 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_scalef(x, y, z);
            }
        }
        // glTranslated (opcode 189)
        189 => {
            if data.len() >= 24 {
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                let z = f64::from_le_bytes(data[16..24].try_into().unwrap());
                osmesa::gl_translated(x, y, z);
            }
        }
        // glTranslatef (opcode 190)
        190 => {
            if data.len() >= 12 {
                let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                osmesa::gl_translatef(x, y, z);
            }
        }
        // glViewport (opcode 191)
        191 => {
            if data.len() >= 16 {
                let x = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let y = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let w = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let h = i32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                osmesa::gl_viewport(x, y, w, h);
            }
        }
        _ => return None,
    }
    Some(true)
}
