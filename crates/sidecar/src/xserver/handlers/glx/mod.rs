//! GLX extension protocol handler.
//!
//! This module implements the GLX indirect rendering protocol by forwarding
//! OpenGL commands to a real OSMesa software context.  When OSMesa is not
//! available at runtime the extension still responds to QueryVersion and
//! friends but returns errors for rendering operations.
//!
//! ## Wire protocol overview
//!
//! GLX uses a single major opcode (assigned as 149 by our QueryExtension
//! handler).  The first byte of the request data is the GLX minor opcode.
//! GLX render requests (minor opcode 1) batch multiple GL commands inside
//! a single X11 request -- each sub-command has its own 4-byte header
//! (render-opcode + length).

mod context;
mod drawable;
mod query;
mod render;
mod single_ops;
mod single_query;

use std::collections::HashMap;
use tracing::{debug, warn};

#[cfg(feature = "osmesa")]
use crate::osmesa;
#[cfg(feature = "osmesa")]
use crate::osmesa::MesaContext;

use super::super::client::ClientState;
use crate::xserver::core::require_len;

// ---------------------------------------------------------------------------
// GLX extension constants
// ---------------------------------------------------------------------------

/// GLX major opcode (assigned in QueryExtension)
#[allow(dead_code)]
pub(crate) const GLX_MAJOR_OPCODE: u8 = 149;

// GLX minor opcodes
const GLX_RENDER: u8 = 1;
const GLX_RENDER_LARGE: u8 = 2;
const GLX_CREATE_CONTEXT: u8 = 3;
const GLX_DESTROY_CONTEXT: u8 = 4;
const GLX_MAKE_CURRENT: u8 = 5;
const GLX_IS_DIRECT: u8 = 6;
const GLX_QUERY_VERSION: u8 = 7;
const GLX_WAIT_GL: u8 = 8;
const GLX_WAIT_X: u8 = 9;
const GLX_COPY_CONTEXT: u8 = 10;
const GLX_SWAP_BUFFERS: u8 = 11;
const GLX_USE_X_FONT: u8 = 12;
const GLX_CREATE_GLX_PIXMAP: u8 = 13;
const GLX_DESTROY_GLX_PIXMAP: u8 = 15;
const GLX_VENDOR_PRIVATE: u8 = 16;
const GLX_VENDOR_PRIVATE_WITH_REPLY: u8 = 17;
const GLX_QUERY_EXTENSIONS_STRING: u8 = 18;
const GLX_QUERY_SERVER_STRING: u8 = 19;
const GLX_CLIENT_INFO: u8 = 20;
const GLX_GET_FB_CONFIGS: u8 = 21;
const GLX_CREATE_PIXMAP: u8 = 22;
const GLX_CREATE_NEW_CONTEXT: u8 = 24;
const GLX_MAKE_CONTEXT_CURRENT: u8 = 26;
const GLX_CREATE_PBUFFER: u8 = 27;
const GLX_DESTROY_PBUFFER: u8 = 28;
const GLX_GET_DRAWABLE_ATTRIBUTES: u8 = 29;
const GLX_CHANGE_DRAWABLE_ATTRIBUTES: u8 = 30;
const GLX_CREATE_WINDOW: u8 = 31;
const GLX_DELETE_WINDOW: u8 = 32;
const GLX_SET_CLIENT_INFO_ARB: u8 = 33;
const GLX_CREATE_CONTEXT_ATTRIBS_ARB: u8 = 34;
const GLX_SET_CLIENT_INFO_2ARB: u8 = 35;
const GLX_GET_VISUAL_CONFIGS: u8 = 14;
const GLX_QUERY_CONTEXT: u8 = 25;

// GLX FBConfig attribute tokens
const GLX_VISUAL_ID: u32 = 0x800B;
const GLX_FBCONFIG_ID: u32 = 0x8013;
const GLX_X_RENDERABLE: u32 = 0x8012;
const GLX_RENDER_TYPE: u32 = 0x8011;
const GLX_DRAWABLE_TYPE: u32 = 0x8010;
const GLX_X_VISUAL_TYPE: u32 = 0x22;
const GLX_CONFIG_CAVEAT: u32 = 0x20;
const GLX_RED_SIZE: u32 = 8;
const GLX_GREEN_SIZE: u32 = 9;
const GLX_BLUE_SIZE: u32 = 10;
const GLX_ALPHA_SIZE: u32 = 11;
const GLX_BUFFER_SIZE: u32 = 2;
const GLX_DOUBLEBUFFER: u32 = 5;
const GLX_DEPTH_SIZE: u32 = 12;
const GLX_STENCIL_SIZE: u32 = 13;
const GLX_LEVEL: u32 = 3;
const GLX_AUX_BUFFERS: u32 = 7;
const GLX_STEREO: u32 = 6;
const GLX_ACCUM_RED_SIZE: u32 = 14;
const GLX_ACCUM_GREEN_SIZE: u32 = 15;
const GLX_ACCUM_BLUE_SIZE: u32 = 16;
const GLX_ACCUM_ALPHA_SIZE: u32 = 17;
const GLX_SAMPLE_BUFFERS: u32 = 0x186A0;
const GLX_SAMPLES: u32 = 0x186A1;
const GLX_NONE: u32 = 0x8000;
const GLX_TRUE_COLOR: u32 = 0x8002;
const GLX_RGBA_BIT: u32 = 0x00000001;
const GLX_WINDOW_BIT: u32 = 0x00000001;
const GLX_PIXMAP_BIT: u32 = 0x00000002;
const GLX_PBUFFER_BIT: u32 = 0x00000004;
const GLX_MAX_PBUFFER_WIDTH: u32 = 0x8016;
const GLX_MAX_PBUFFER_HEIGHT: u32 = 0x8017;
const GLX_MAX_PBUFFER_PIXELS: u32 = 0x8018;
const GLX_TRANSPARENT_TYPE: u32 = 0x23;

// Number of attribute pairs in our FBConfig
const FBCONFIG_ATTRIB_COUNT: usize = 28;

/// The kind of GLX drawable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlxDrawableKind {
    Pixmap,
    Pbuffer,
    Window,
}

/// Metadata for a tracked GLX drawable.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct GlxDrawable {
    pub(crate) kind: GlxDrawableKind,
    /// The X11 drawable (pixmap or window) backing this GLX drawable.
    pub(crate) x_drawable: u32,
    /// FBConfig id used when creating this drawable (0 if unknown).
    pub(crate) fbconfig: u32,
    /// Attribute key/value pairs set via ChangeDrawableAttributes.
    pub(crate) attributes: HashMap<u32, u32>,
}

/// Per-client GLX state.
pub(crate) struct GlxState {
    /// GLX context id -> context state
    pub(crate) contexts: HashMap<u32, GlxContext>,
    /// GLX drawable id -> drawable metadata
    pub(crate) drawables: HashMap<u32, GlxDrawable>,
    /// Currently bound context (if any)
    pub(crate) current_context: u32,
    /// Currently bound drawable (if any)
    pub(crate) current_drawable: u32,
    /// Next context tag for MakeCurrent replies
    next_tag: u32,
}

impl Default for GlxState {
    fn default() -> Self {
        Self {
            contexts: HashMap::new(),
            drawables: HashMap::new(),
            current_context: 0,
            current_drawable: 0,
            next_tag: 1,
        }
    }
}

/// A single GLX context.
pub(crate) struct GlxContext {
    pub(crate) id: u32,
    pub(crate) visual: u32,
    pub(crate) screen: u32,
    pub(crate) tag: u32,
    pub(crate) drawable: u32,
    /// The context ID this context shares display lists / textures with (0 = none).
    /// OSMesa uses process-global GL name registries, so contexts in the same
    /// sidecar process already share resources — this field exists for protocol
    /// correctness (e.g. glXQueryContext(GLX_SHARE_CONTEXT_EXT)).
    pub(crate) share_list: u32,
    /// OSMesa software rendering context (if osmesa feature is enabled)
    #[cfg(feature = "osmesa")]
    pub(crate) mesa: Option<MesaContext>,
}

impl Drop for GlxContext {
    fn drop(&mut self) {
        debug!("Destroying GLX context {:#x}", self.id);
    }
}

// ---------------------------------------------------------------------------
// Main dispatcher
// ---------------------------------------------------------------------------

pub(crate) fn handle_glx_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 4, seq, 159, 0, state.msb_first);
    let minor = data[1];
    debug!("GLX minor opcode: {minor}");

    match minor {
        GLX_QUERY_VERSION => query::handle_query_version(seq),
        GLX_GET_VISUAL_CONFIGS => query::handle_get_visual_configs(data, seq),
        GLX_GET_FB_CONFIGS => query::handle_get_fb_configs(data, seq),
        GLX_CREATE_CONTEXT => context::handle_create_context(state, data, seq),
        GLX_CREATE_NEW_CONTEXT => context::handle_create_new_context(state, data, seq),
        GLX_CREATE_CONTEXT_ATTRIBS_ARB => context::handle_create_context_attribs(state, data, seq),
        GLX_DESTROY_CONTEXT => context::handle_destroy_context(state, data, seq),
        GLX_MAKE_CURRENT => context::handle_make_current(state, data, seq),
        GLX_MAKE_CONTEXT_CURRENT => context::handle_make_context_current(state, data, seq),
        GLX_IS_DIRECT => context::handle_is_direct(seq),
        GLX_SWAP_BUFFERS => context::handle_swap_buffers(state, data, seq),
        GLX_WAIT_GL => context::handle_wait_gl(state),
        GLX_WAIT_X => context::handle_wait_x(state),
        GLX_COPY_CONTEXT => context::handle_copy_context(state, data, seq),
        GLX_VENDOR_PRIVATE_WITH_REPLY => context::handle_vendor_private_with_reply(data, seq),
        GLX_RENDER => render::handle_render(state, data, seq),
        GLX_RENDER_LARGE => render::handle_render_large(state, data, seq),
        GLX_QUERY_EXTENSIONS_STRING => query::handle_query_extensions_string(data, seq),
        GLX_QUERY_SERVER_STRING => query::handle_query_server_string(data, seq),
        GLX_CLIENT_INFO => {
            if data.len() >= 16 {
                let major = state.read_u32(data, 4);
                let minor = state.read_u32(data, 8);
                let str_len = state.read_u32(data, 12) as usize;
                let client_str = if data.len() >= 16 + str_len && str_len > 0 {
                    String::from_utf8_lossy(&data[16..16 + str_len]).trim_end_matches('\0').to_string()
                } else {
                    String::new()
                };
                debug!("GLX ClientInfo: version {major}.{minor}, extensions: {client_str}");
            }
            Vec::new()
        }
        GLX_SET_CLIENT_INFO_ARB | GLX_SET_CLIENT_INFO_2ARB => {
            if data.len() >= 20 {
                let major = state.read_u32(data, 4);
                let minor = state.read_u32(data, 8);
                let num_versions = state.read_u32(data, 12);
                let str_len = state.read_u32(data, 16) as usize;
                let versions_bytes = (num_versions as usize) * 8; // each version pair is 2 x u32
                let str_offset = 20 + versions_bytes;
                let client_str = if data.len() >= str_offset + str_len && str_len > 0 {
                    String::from_utf8_lossy(&data[str_offset..str_offset + str_len]).trim_end_matches('\0').to_string()
                } else {
                    String::new()
                };
                debug!(
                    "GLX SetClientInfo: version {major}.{minor}, num_versions: {num_versions}, extensions: {client_str}"
                );
            }
            Vec::new()
        }
        GLX_USE_X_FONT => drawable::handle_use_x_font(state, data, seq),
        GLX_CREATE_GLX_PIXMAP => drawable::handle_create_glx_pixmap(state, data, seq),
        GLX_CREATE_PIXMAP => drawable::handle_create_pixmap(state, data, seq),
        GLX_DESTROY_GLX_PIXMAP => drawable::handle_destroy_glx_pixmap(state, data, seq),
        GLX_CREATE_PBUFFER => drawable::handle_create_pbuffer(state, data, seq),
        GLX_DESTROY_PBUFFER => drawable::handle_destroy_pbuffer(state, data, seq),
        GLX_CREATE_WINDOW => drawable::handle_create_window(state, data, seq),
        GLX_DELETE_WINDOW => drawable::handle_delete_window(state, data, seq),
        GLX_GET_DRAWABLE_ATTRIBUTES => drawable::handle_get_drawable_attributes(state, data, seq),
        GLX_CHANGE_DRAWABLE_ATTRIBUTES => drawable::handle_change_drawable_attributes(state, data, seq),
        GLX_QUERY_CONTEXT => drawable::handle_query_context(state, data, seq),
        GLX_VENDOR_PRIVATE => {
            // Vendor private requests have no reply per the GLX spec.
            // Log the vendor code for diagnostics but otherwise succeed silently
            // since returning an error would break clients using common vendor ops.
            if data.len() >= 8 {
                let vendor_code = state.read_u32(data, 4);
                debug!("GLX VendorPrivate: vendor_code={vendor_code}");
            }
            Vec::new()
        }
        // GLX single GL commands use minor opcodes 101+ (one per GL query function).
        // These carry context_tag(4) + GL-specific payload, dispatched by gl_opcode.
        101..=255 => context::handle_glx_single(state, data, seq),
        _ => {
            warn!("Unhandled GLX minor opcode: {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                159, minor as u16, state.msb_first,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_drawable_size(state: &ClientState, drawable: u32) -> (u32, u32) {
    if let Some(win) = state.windows.get(&drawable) {
        return (win.width as u32, win.height as u32);
    }
    if let Some(pix) = state.pixmaps.get(&drawable) {
        return (pix.width as u32, pix.height as u32);
    }
    (state.screen_width as u32, state.screen_height as u32)
}

/// Blit the OSMesa framebuffer to the X11 drawable's framebuffer.
#[cfg(feature = "osmesa")]
fn blit_osmesa_to_drawable(state: &mut ClientState, drawable: u32) {
    let ctx_id = state.glx.current_context;
    if ctx_id == 0 {
        return;
    }

    // We need to split the borrow: extract the mesa context pixels first,
    // then write to the framebuffer separately.
    let pixels: Vec<u8>;
    let mesa_w: u32;
    let mesa_h: u32;
    {
        let ctx = match state.glx.contexts.get_mut(&ctx_id) {
            Some(c) => c,
            None => return,
        };
        match ctx.mesa {
            Some(ref mut mesa) => {
                mesa.make_current();
                osmesa::gl_flush();
                pixels = mesa.pixels().to_vec();
                mesa_w = mesa.width();
                mesa_h = mesa.height();
            }
            None => return,
        }
    }

    // Now blit to the drawable's framebuffer
    let target = state.resolve_drawable(drawable);
    if let Some(fb) = state.get_framebuffer_mut(target) {
        let w = mesa_w.min(fb.width()) as usize;
        let h = mesa_h.min(fb.height()) as usize;
        let src_stride = mesa_w as usize * 4;
        let dst_stride = fb.stride();
        let fb_data = fb.data_mut();

        for y in 0..h {
            let src_row = &pixels[y * src_stride..y * src_stride + w * 4];
            let dst_row = &mut fb_data[y * dst_stride..y * dst_stride + w * 4];
            // RGBA -> BGRA: process 4 pixels at a time when possible for better
            // throughput, then handle the remainder.
            let bulk = w & !3; // round down to multiple of 4
            for x in (0..bulk).step_by(4) {
                let si = x * 4;
                let di = x * 4;
                for k in 0..4 {
                    let s = si + k * 4;
                    let d = di + k * 4;
                    dst_row[d]     = src_row[s + 2]; // B
                    dst_row[d + 1] = src_row[s + 1]; // G
                    dst_row[d + 2] = src_row[s];     // R
                    dst_row[d + 3] = src_row[s + 3]; // A
                }
            }
            for x in bulk..w {
                let si = x * 4;
                let di = x * 4;
                dst_row[di]     = src_row[si + 2];
                dst_row[di + 1] = src_row[si + 1];
                dst_row[di + 2] = src_row[si];
                dst_row[di + 3] = src_row[si + 3];
            }
        }
        fb.mark_dirty(0, 0, w as u32, h as u32);
    }

    // Emit damage notification for the window so compositing sees the update
    if let Some(win) = state.windows.get(&target) {
        let width = win.width;
        let height = win.height;
        state.notify_damage(target, 0, 0, width, height);
    }
}
