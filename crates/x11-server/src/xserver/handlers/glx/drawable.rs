//! GLX drawable management (CreateGLXPixmap, DestroyGLXPixmap, CreatePbuffer,
//! DestroyPbuffer, CreateWindow, DeleteWindow, GetDrawableAttributes,
//! ChangeDrawableAttributes, UseXFont, QueryContext).

use std::collections::HashMap;
use tracing::{debug, warn};

use super::super::super::client::ClientState;
use super::super::super::core::{ROOT_VISUAL, VISUAL_TRUE_COLOR_ARGB_32};
use super::{
    GlxDrawable, GlxDrawableKind, FBCONFIG_ARGB, FBCONFIG_RGB, GLX_FBCONFIG_ID, GLX_RENDER_TYPE,
    GLX_RGBA_BIT,
};
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// GLX_USE_X_FONT (minor 12)
// ---------------------------------------------------------------------------

pub(crate) fn handle_use_x_font(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    // UseXFont loads an X font into GL display lists containing glBitmap calls.
    // Wire: 4 context_tag | 4 font | 4 first | 4 count | 4 list_base
    if data.len() < 24 {
        return Vec::new();
    }
    let _context_tag = super::render::read_u32_le(data, 4);
    let font_id = super::render::read_u32_le(data, 8);
    let first = super::render::read_u32_le(data, 12);
    let count = super::render::read_u32_le(data, 16);
    let list_base = super::render::read_u32_le(data, 20);

    debug!("GLX UseXFont: font={font_id:#x} first={first} count={count} list_base={list_base}");

    let font = match state.font_manager.get_font(font_id) {
        Some(f) => f.clone(),
        None => {
            warn!("GLX UseXFont: font {font_id:#x} not found");
            return Vec::new();
        }
    };

    // GL enum tokens used by UseXFont. Values match the OpenGL spec.
    const GL_COMPILE: u32 = 0x1300;
    const GL_UNPACK_LSB_FIRST: u32 = 0x0CF1;
    const GL_UNPACK_ALIGNMENT: u32 = 0x0D05;

    // Set pixel storage for 1-bit bitmaps: byte-aligned, MSB first
    crate::osmesa::gl_pixel_storei(GL_UNPACK_ALIGNMENT, 1);
    crate::osmesa::gl_pixel_storei(GL_UNPACK_LSB_FIRST, 0);

    for i in 0..count {
        let char_code = (first + i) as u16;
        let list_id = list_base + i;

        crate::osmesa::gl_new_list(list_id, GL_COMPILE);

        if char_code >= font.min_char && char_code <= font.max_char {
            let idx = (char_code - font.min_char) as usize;
            if idx < font.glyphs.len() && idx < font.char_infos.len() {
                let glyph = &font.glyphs[idx];
                let ci = &font.char_infos[idx];

                if glyph.width > 0 && glyph.height > 0 && !glyph.bitmap.is_empty() {
                    crate::osmesa::gl_bitmap(
                        glyph.width as i32,
                        glyph.height as i32,
                        ci.left_side_bearing as f32, // x origin
                        ci.descent as f32,           // y origin
                        ci.character_width as f32,   // x advance
                        0.0,                         // y advance
                        &glyph.bitmap,
                    );
                } else {
                    // Empty glyph — just advance the raster position
                    crate::osmesa::gl_bitmap(0, 0, 0.0, 0.0, ci.character_width as f32, 0.0, &[]);
                }
            }
        }

        crate::osmesa::gl_end_list();
    }

    Vec::new() // void request
}

// ---------------------------------------------------------------------------
// GLX_CREATE_GLX_PIXMAP (minor 13)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_glx_pixmap(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: 4 screen | 4 visual | 4 pixmap (X) | 4 glx_pixmap (new id)
    require_len!(data, 20, seq, 159, 13, state.msb_first);
    let visual = super::render::read_u32_le(data, 8);
    let x_pixmap = super::render::read_u32_le(data, 12);
    let glx_pixmap = super::render::read_u32_le(data, 16);

    let fbconfig = if visual == VISUAL_TRUE_COLOR_ARGB_32 {
        FBCONFIG_ARGB
    } else {
        FBCONFIG_RGB
    };
    state.glx.drawables.insert(
        glx_pixmap,
        GlxDrawable {
            kind: GlxDrawableKind::Pixmap,
            x_drawable: x_pixmap,
            fbconfig,
            attributes: HashMap::new(),
        },
    );
    debug!("Created GLX pixmap {glx_pixmap:#x} backed by X pixmap {x_pixmap:#x}");
    Vec::new() // void request
}

// ---------------------------------------------------------------------------
// GLX_CREATE_PIXMAP (minor 22)
// ---------------------------------------------------------------------------

/// Creates a GLX pixmap from an existing X pixmap and an FBConfig.
pub(crate) fn handle_create_pixmap(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: screen(4) fbconfig(4) pixmap(4) glx_pixmap(4) num_attribs(4) attribs...
    require_len!(data, 24, seq, 159, 22, state.msb_first);
    let _screen = super::render::read_u32_le(data, 4);
    let fbconfig = super::render::read_u32_le(data, 8);
    let x_pixmap = super::render::read_u32_le(data, 12);
    let glx_pixmap = super::render::read_u32_le(data, 16);
    let num_attribs = super::render::read_u32_le(data, 20) as usize;

    // Validate the X pixmap exists
    if !state.pixmaps.contains_key(&x_pixmap) {
        return crate::xserver::core::build_error(
            crate::xserver::core::PIXMAP_ERROR,
            seq,
            x_pixmap,
            159,
            22,
        );
    }

    // Parse attribute pairs (terminated by key=0 or end of data)
    let mut attributes = HashMap::new();
    for i in 0..num_attribs {
        let base = 24 + i * 8;
        if base + 8 > data.len() {
            break;
        }
        let key = super::render::read_u32_le(data, base);
        if key == 0 {
            break;
        }
        let val = u32::from_le_bytes([
            data[base + 4],
            data[base + 5],
            data[base + 6],
            data[base + 7],
        ]);
        attributes.insert(key, val);
    }

    state.glx.drawables.insert(
        glx_pixmap,
        GlxDrawable {
            kind: GlxDrawableKind::Pixmap,
            x_drawable: x_pixmap,
            fbconfig,
            attributes,
        },
    );
    debug!("Created GLX pixmap {glx_pixmap:#x} (opcode 22) backed by X pixmap {x_pixmap:#x} fbconfig={fbconfig}");
    Vec::new() // Void request
}

// ---------------------------------------------------------------------------
// GLX_DESTROY_GLX_PIXMAP (minor 15)
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_glx_pixmap(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 15, state.msb_first);
    let glx_pixmap = super::render::read_u32_le(data, 4);
    if state.glx.drawables.remove(&glx_pixmap).is_some() {
        debug!("Destroyed GLX pixmap {glx_pixmap:#x}");
    } else {
        warn!("DestroyGLXPixmap: unknown drawable {glx_pixmap:#x}");
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_CREATE_PBUFFER (minor 27)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_pbuffer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: 4 screen | 4 fbconfig | 4 pbuffer_id | 4 num_attribs | attribs...
    require_len!(data, 20, seq, 159, 27, state.msb_first);
    let fbconfig = super::render::read_u32_le(data, 8);
    let pbuffer_id = super::render::read_u32_le(data, 12);
    let num_attribs = super::render::read_u32_le(data, 16) as usize;

    let mut attributes = HashMap::new();
    for i in 0..num_attribs {
        let base = 20 + i * 8;
        if base + 8 > data.len() {
            break;
        }
        let key = super::render::read_u32_le(data, base);
        let val = u32::from_le_bytes([
            data[base + 4],
            data[base + 5],
            data[base + 6],
            data[base + 7],
        ]);
        attributes.insert(key, val);
    }

    state.glx.drawables.insert(
        pbuffer_id,
        GlxDrawable {
            kind: GlxDrawableKind::Pbuffer,
            x_drawable: 0, // pbuffers have no backing X drawable
            fbconfig,
            attributes,
        },
    );
    debug!("Created GLX pbuffer {pbuffer_id:#x} fbconfig={fbconfig}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_DESTROY_PBUFFER (minor 28)
// ---------------------------------------------------------------------------

pub(crate) fn handle_destroy_pbuffer(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 28, state.msb_first);
    let pbuffer_id = super::render::read_u32_le(data, 4);
    if state.glx.drawables.remove(&pbuffer_id).is_some() {
        state.recycle_xid(pbuffer_id);
        debug!("Destroyed GLX pbuffer {pbuffer_id:#x}");
    } else {
        warn!("DestroyPbuffer: unknown drawable {pbuffer_id:#x}");
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_CREATE_WINDOW (minor 31)
// ---------------------------------------------------------------------------

pub(crate) fn handle_create_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Wire: 4 screen | 4 fbconfig | 4 window (X) | 4 glx_window | 4 num_attribs | attribs...
    require_len!(data, 24, seq, 159, 31, state.msb_first);
    let fbconfig = super::render::read_u32_le(data, 8);
    let x_window = super::render::read_u32_le(data, 12);
    let glx_window = super::render::read_u32_le(data, 16);
    let num_attribs = super::render::read_u32_le(data, 20) as usize;

    let mut attributes = HashMap::new();
    for i in 0..num_attribs {
        let base = 24 + i * 8;
        if base + 8 > data.len() {
            break;
        }
        let key = super::render::read_u32_le(data, base);
        let val = u32::from_le_bytes([
            data[base + 4],
            data[base + 5],
            data[base + 6],
            data[base + 7],
        ]);
        attributes.insert(key, val);
    }

    state.glx.drawables.insert(
        glx_window,
        GlxDrawable {
            kind: GlxDrawableKind::Window,
            x_drawable: x_window,
            fbconfig,
            attributes,
        },
    );
    debug!("Created GLX window {glx_window:#x} backed by X window {x_window:#x}");
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_DELETE_WINDOW (minor 32)
// ---------------------------------------------------------------------------

pub(crate) fn handle_delete_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 32, state.msb_first);
    let glx_window = super::render::read_u32_le(data, 4);
    if state.glx.drawables.remove(&glx_window).is_some() {
        debug!("Deleted GLX window {glx_window:#x}");
    } else {
        warn!("DeleteWindow: unknown drawable {glx_window:#x}");
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_GET_DRAWABLE_ATTRIBUTES (minor 29)
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_drawable_attributes(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    if data.len() < 8 {
        return super::reply::attrib_pairs_reply(seq, &[]);
    }
    let drawable_id = super::render::read_u32_le(data, 4);

    if let Some(drawable) = state.glx.drawables.get(&drawable_id) {
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        pairs.push((GLX_FBCONFIG_ID, drawable.fbconfig));
        for (&k, &v) in &drawable.attributes {
            if k != GLX_FBCONFIG_ID {
                pairs.push((k, v));
            }
        }
        super::reply::attrib_pairs_reply(seq, &pairs)
    } else {
        super::reply::attrib_pairs_reply(seq, &[])
    }
}

// ---------------------------------------------------------------------------
// GLX_CHANGE_DRAWABLE_ATTRIBUTES (minor 30)
// ---------------------------------------------------------------------------

pub(crate) fn handle_change_drawable_attributes(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
) -> Vec<u8> {
    require_len!(data, 12, seq, 159, 30, state.msb_first);
    let drawable_id = super::render::read_u32_le(data, 4);
    let num_attribs = super::render::read_u32_le(data, 8) as usize;

    if let Some(drawable) = state.glx.drawables.get_mut(&drawable_id) {
        for i in 0..num_attribs {
            let base = 12 + i * 8;
            if base + 8 > data.len() {
                break;
            }
            let key = super::render::read_u32_le(data, base);
            let val = u32::from_le_bytes([
                data[base + 4],
                data[base + 5],
                data[base + 6],
                data[base + 7],
            ]);
            drawable.attributes.insert(key, val);
        }
        debug!("Changed {num_attribs} attributes on GLX drawable {drawable_id:#x}");
    } else {
        warn!("ChangeDrawableAttributes: unknown drawable {drawable_id:#x}");
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// GLX_QUERY_CONTEXT (minor 25)
// ---------------------------------------------------------------------------

pub(crate) fn handle_query_context(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 159, 25, state.msb_first);
    let ctx_id = super::render::read_u32_le(data, 4);

    let ctx = state.glx.contexts.get(&ctx_id);
    let screen = ctx.map(|c| c.screen).unwrap_or(0);
    let visual = ctx.map(|c| c.visual).unwrap_or(ROOT_VISUAL);
    let share_list = ctx.map(|c| c.share_list).unwrap_or(0);

    let pairs = [
        (
            GLX_FBCONFIG_ID,
            if visual == VISUAL_TRUE_COLOR_ARGB_32 {
                FBCONFIG_ARGB
            } else {
                FBCONFIG_RGB
            },
        ),
        (GLX_RENDER_TYPE, GLX_RGBA_BIT),
        (0x3, screen),        // GLX_SCREEN
        (0x800A, share_list), // GLX_SHARE_CONTEXT_EXT
    ];
    super::reply::attrib_pairs_reply(seq, &pairs)
}
