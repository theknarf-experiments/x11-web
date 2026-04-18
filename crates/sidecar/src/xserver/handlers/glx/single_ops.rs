//! GLX single GL operations (glDeleteLists, glDeleteTextures, glAreTexturesResident,
//! glRenderMode, glFinish, glPixelStoref, glPixelStorei, etc.).

#[cfg(feature = "osmesa")]
use crate::osmesa;

use super::reply::GlxReply;

// ---------------------------------------------------------------------------
// glDeleteLists (opcode 103)
// ---------------------------------------------------------------------------

pub(crate) fn handle_delete_lists(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() >= 8 {
        let list = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let range = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        #[cfg(feature = "osmesa")]
        {
            if osmesa::is_available() {
                osmesa::gl_delete_lists(list, range);
            }
        }
    }
    GlxReply::Empty.encode(seq)
}

// ---------------------------------------------------------------------------
// glRenderMode (opcode 107)
// ---------------------------------------------------------------------------

pub(crate) fn handle_render_mode(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() < 4 {
        return GlxReply::Empty.encode(seq);
    }
    let mode = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let result: i32 = {
        #[cfg(feature = "osmesa")]
        {
            if osmesa::is_available() {
                osmesa::gl_render_mode(mode)
            } else {
                0
            }
        }
        #[cfg(not(feature = "osmesa"))]
        {
            0
        }
    };
    GlxReply::Scalar(result as u32).encode(seq)
}

// ---------------------------------------------------------------------------
// glFinish (opcode 108)
// ---------------------------------------------------------------------------

pub(crate) fn handle_finish(seq: u16) -> Vec<u8> {
    #[cfg(feature = "osmesa")]
    {
        if osmesa::is_available() {
            osmesa::gl_finish();
        }
    }
    GlxReply::Empty.encode(seq)
}

// ---------------------------------------------------------------------------
// glPixelStoref (opcode 109)
// ---------------------------------------------------------------------------

pub(crate) fn handle_pixel_storef(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() < 8 {
        return GlxReply::Empty.encode(seq);
    }
    let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let param = f32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    #[cfg(feature = "osmesa")]
    {
        if osmesa::is_available() {
            osmesa::gl_pixel_storef(pname, param);
        }
    }
    GlxReply::Empty.encode(seq)
}

// ---------------------------------------------------------------------------
// glPixelStorei (opcode 110)
// ---------------------------------------------------------------------------

pub(crate) fn handle_pixel_storei(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() < 8 {
        return GlxReply::Empty.encode(seq);
    }
    let pname = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let param = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    #[cfg(feature = "osmesa")]
    {
        if osmesa::is_available() {
            osmesa::gl_pixel_storei(pname, param);
        }
    }
    GlxReply::Empty.encode(seq)
}

// ---------------------------------------------------------------------------
// glAreTexturesResident (opcode 143)
// ---------------------------------------------------------------------------

pub(crate) fn handle_are_textures_resident(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() < 4 {
        return GlxReply::Empty.encode(seq);
    }
    let n = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let n = n.max(0) as usize;
    if payload.len() < 4 + n * 4 {
        return GlxReply::Empty.encode(seq);
    }
    let textures: Vec<u32> = (0..n)
        .map(|i| {
            let off = 4 + i * 4;
            u32::from_le_bytes([
                payload[off],
                payload[off + 1],
                payload[off + 2],
                payload[off + 3],
            ])
        })
        .collect();
    let mut residences = vec![0u8; n];
    let all_resident: u8 = {
        #[cfg(feature = "osmesa")]
        {
            if osmesa::is_available() && n > 0 {
                if osmesa::gl_are_textures_resident(&textures, &mut residences) {
                    1
                } else {
                    0
                }
            } else {
                1
            }
        }
        #[cfg(not(feature = "osmesa"))]
        {
            1
        }
    };
    super::reply::are_textures_resident_reply(seq, all_resident != 0, &residences)
}

// ---------------------------------------------------------------------------
// glDeleteTextures (opcode 144)
// ---------------------------------------------------------------------------

pub(crate) fn handle_delete_textures(payload: &[u8], seq: u16) -> Vec<u8> {
    if payload.len() < 4 {
        return GlxReply::Empty.encode(seq);
    }
    let n = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let n = n.max(0) as usize;
    if payload.len() >= 4 + n * 4 {
        let textures: Vec<u32> = (0..n)
            .map(|i| {
                let off = 4 + i * 4;
                u32::from_le_bytes([
                    payload[off],
                    payload[off + 1],
                    payload[off + 2],
                    payload[off + 3],
                ])
            })
            .collect();
        #[cfg(feature = "osmesa")]
        {
            if osmesa::is_available() && n > 0 {
                osmesa::gl_delete_textures(&textures);
            }
        }
    }
    GlxReply::Empty.encode(seq)
}
