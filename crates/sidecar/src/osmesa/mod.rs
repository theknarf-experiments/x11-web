//! OSMesa (Off-Screen Mesa) glue for software OpenGL rendering.
//!
//! OSMesa-specific FFI symbols (`OSMesaCreateContextExt` etc.) are resolved at
//! runtime via `dlopen` / `dlsym` so the build succeeds even when
//! `libOSMesa.so` is absent on the build machine. The plain GL surface is
//! handled by build-time-generated bindings in `gl_generated` (see
//! `build.rs`); we hand them an OSMesa-aware `load_with` callback after
//! dlopen succeeds.
//!
//! `gl_bindings` re-exports thin wrappers over the generated bindings so the
//! GLX handlers can keep their `crate::osmesa::gl_*(...)` call shape.

use std::ffi::{c_void, CStr, CString};
use std::ptr;
use std::sync::OnceLock;
use tracing::{debug, error, info, warn};

// Build-time-generated GL bindings (compatibility profile).
#[allow(
    non_upper_case_globals,
    non_snake_case,
    non_camel_case_types,
    dead_code
)]
pub mod gl_generated {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings_generated.rs"));
}

// --------------------------------------------------------------------------
// OSMesa-specific FFI.
// --------------------------------------------------------------------------

/// Opaque handle returned by `OSMesaCreateContextExt`.
pub type OSMesaContext = *mut c_void;

pub const OSMESA_RGBA: u32 = 0x1908;
pub const OSMESA_Y_UP: u32 = 0x11;

pub const GL_RGBA: u32 = 0x1908;
pub const GL_UNSIGNED_BYTE: u32 = 0x1401;

// Vertex array client states (used by glx/render/render_draw.rs).
pub const GL_VERTEX_ARRAY: u32 = 0x8074;
pub const GL_NORMAL_ARRAY: u32 = 0x8075;
pub const GL_COLOR_ARRAY: u32 = 0x8076;
pub const GL_TEXTURE_COORD_ARRAY: u32 = 0x8078;

type FnOSMesaCreateContextExt = unsafe extern "C" fn(
    format: u32,
    depth_bits: i32,
    stencil_bits: i32,
    accum_bits: i32,
    share_list: OSMesaContext,
) -> OSMesaContext;
type FnOSMesaDestroyContext = unsafe extern "C" fn(ctx: OSMesaContext);
type FnOSMesaMakeCurrent = unsafe extern "C" fn(
    ctx: OSMesaContext,
    buffer: *mut c_void,
    type_: u32,
    width: i32,
    height: i32,
) -> u8;
type FnOSMesaGetProcAddress =
    unsafe extern "C" fn(func_name: *const std::ffi::c_char) -> *const c_void;
type FnOSMesaPixelStore = unsafe extern "C" fn(pname: u32, value: i32);

struct OsMesaFns {
    create_context_ext: FnOSMesaCreateContextExt,
    destroy_context: FnOSMesaDestroyContext,
    make_current: FnOSMesaMakeCurrent,
    pixel_store: FnOSMesaPixelStore,
}

static FNS: OnceLock<OsMesaFns> = OnceLock::new();

/// Attempt to load libOSMesa at runtime. Returns `true` if successful.
pub fn init() -> bool {
    if FNS.get().is_some() {
        return true;
    }
    match try_load() {
        Ok(fns) => {
            let _ = FNS.set(fns);
            info!("OSMesa loaded successfully");
            true
        }
        Err(e) => {
            warn!("OSMesa not available: {e}");
            false
        }
    }
}

/// Returns true if OSMesa was successfully loaded.
pub fn is_available() -> bool {
    FNS.get().is_some()
}

fn fns() -> &'static OsMesaFns {
    FNS.get()
        .expect("OSMesa not initialized — call osmesa::init() first")
}

fn try_load() -> Result<OsMesaFns, String> {
    let lib_names = ["libOSMesa.so.8", "libOSMesa.so.6", "libOSMesa.so"];
    let lib = {
        let mut last_err = String::new();
        let mut loaded = None;
        for name in &lib_names {
            let handle = unsafe {
                libc::dlopen(
                    CString::new(*name).unwrap().as_ptr(),
                    libc::RTLD_NOW | libc::RTLD_GLOBAL,
                )
            };
            if !handle.is_null() {
                info!("Loaded {name}");
                loaded = Some(handle);
                break;
            }
            let err = unsafe { CStr::from_ptr(libc::dlerror()) };
            last_err = err.to_string_lossy().into_owned();
            debug!("Failed to load {name}: {last_err}");
        }
        loaded.ok_or_else(|| format!("Could not load libOSMesa: {last_err}"))?
    };

    macro_rules! sym {
        ($name:expr, $ty:ty) => {{
            let cname = CString::new($name).unwrap();
            let ptr = unsafe { libc::dlsym(lib, cname.as_ptr()) };
            if ptr.is_null() {
                return Err(format!("Symbol {} not found", $name));
            }
            unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
        }};
    }

    let create_context_ext = sym!("OSMesaCreateContextExt", FnOSMesaCreateContextExt);
    let destroy_context = sym!("OSMesaDestroyContext", FnOSMesaDestroyContext);
    let make_current = sym!("OSMesaMakeCurrent", FnOSMesaMakeCurrent);
    let get_proc_address = sym!("OSMesaGetProcAddress", FnOSMesaGetProcAddress);
    let pixel_store = sym!("OSMesaPixelStore", FnOSMesaPixelStore);

    // Hand the generated GL bindings an OSMesa-aware loader. They cache
    // per-symbol internally, so this closure runs once per GL function on
    // first use. Falls back to dlsym on the OSMesa library if
    // `OSMesaGetProcAddress` returns null (true for some legacy symbols).
    gl_generated::load_with(|name| {
        let cname = CString::new(name).unwrap();
        let mut p = unsafe { (get_proc_address)(cname.as_ptr()) };
        if p.is_null() {
            p = unsafe { libc::dlsym(lib, cname.as_ptr()) };
        }
        p as *const _
    });

    Ok(OsMesaFns {
        create_context_ext,
        destroy_context,
        make_current,
        pixel_store,
    })
}

// --------------------------------------------------------------------------
// MesaContext — owns an OSMesa context + its pixel buffer.
// --------------------------------------------------------------------------

pub struct MesaContext {
    ctx: OSMesaContext,
    /// Pixel buffer that OSMesa renders into (RGBA, 4 bytes/pixel).
    buffer: Vec<u8>,
    width: u32,
    height: u32,
}

// OSMesa contexts are thread-local in Mesa's implementation but we only ever
// use them from the per-client tokio task, so Send is fine.
unsafe impl Send for MesaContext {}

impl MesaContext {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if !is_available() {
            return None;
        }
        let f = fns();
        let ctx = unsafe { (f.create_context_ext)(OSMESA_RGBA, 24, 8, 0, ptr::null_mut()) };
        if ctx.is_null() {
            error!("OSMesaCreateContextExt returned NULL");
            return None;
        }
        let buf_size = (width * height * 4) as usize;
        let mut buffer = vec![0u8; buf_size];

        let ok = unsafe {
            (f.make_current)(
                ctx,
                buffer.as_mut_ptr() as *mut c_void,
                GL_UNSIGNED_BYTE,
                width as i32,
                height as i32,
            )
        };
        if ok == 0 {
            error!("OSMesaMakeCurrent failed for {}x{}", width, height);
            unsafe { (f.destroy_context)(ctx) };
            return None;
        }

        // Tell OSMesa that Y=0 is at the top (matches X11).
        unsafe { (f.pixel_store)(OSMESA_Y_UP, 0) };

        debug!("Created OSMesa context {}x{}", width, height);
        Some(Self {
            ctx,
            buffer,
            width,
            height,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let f = fns();
        let buf_size = (width * height * 4) as usize;
        self.buffer = vec![0u8; buf_size];
        self.width = width;
        self.height = height;
        let ok = unsafe {
            (f.make_current)(
                self.ctx,
                self.buffer.as_mut_ptr() as *mut c_void,
                GL_UNSIGNED_BYTE,
                width as i32,
                height as i32,
            )
        };
        if ok == 0 {
            error!("OSMesaMakeCurrent failed on resize to {}x{}", width, height);
            return false;
        }
        unsafe { (f.pixel_store)(OSMESA_Y_UP, 0) };
        true
    }

    pub fn make_current(&mut self) -> bool {
        let f = fns();
        let ok = unsafe {
            (f.make_current)(
                self.ctx,
                self.buffer.as_mut_ptr() as *mut c_void,
                GL_UNSIGNED_BYTE,
                self.width as i32,
                self.height as i32,
            )
        };
        ok != 0
    }

    /// RGBA pixel buffer, row-major, 4 bytes/pixel, Y=0 at top.
    pub fn pixels(&self) -> &[u8] {
        &self.buffer
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for MesaContext {
    fn drop(&mut self) {
        if is_available() && !self.ctx.is_null() {
            let f = fns();
            unsafe { (f.destroy_context)(self.ctx) };
        }
    }
}

// --------------------------------------------------------------------------
// GL command dispatch — thin wrappers over the generated bindings, called
// by the GLX handler. Wrappers exist purely to keep the existing call-site
// shape (slices instead of raw pointers, `gl_clear` rather than `Clear`).
// --------------------------------------------------------------------------

mod gl_bindings;
pub use gl_bindings::*;
