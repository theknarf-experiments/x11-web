use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use x11_web_protocol::{DisplayUpdate, InputEvent};
use crate::framebuffer::Framebuffer;

/// A display update tagged with the client_id that produced it.
pub type TaggedDisplayUpdate = (String, DisplayUpdate);

/// Shared window registry, keyed by window ID.
/// All connections share a single window namespace, as required by X11.
pub(crate) type SharedWindows = Arc<Mutex<HashMap<u32, WindowState>>>;

/// Message sent to a specific X11 connection via the window router.
pub(crate) enum WindowMessage {
    Input(InputEvent),
    Resize(u16, u16),
}

/// Routes messages from the frontend to the correct X11 connection.
/// Maps window UUID → (sender, x11_window_id).
#[derive(Clone)]
pub struct WindowRouter {
    routes: Arc<Mutex<HashMap<String, WindowRoute>>>,
}

struct WindowRoute {
    tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    x11_window_id: u32,
}

impl WindowRouter {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn register(&self, uuid: &str, x11_wid: u32, tx: &mpsc::UnboundedSender<(u32, WindowMessage)>) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(uuid.to_string(), WindowRoute {
                tx: tx.clone(),
                x11_window_id: x11_wid,
            });
        }
    }

    pub(crate) fn unregister_all(&self, uuids: &[String]) {
        if let Ok(mut routes) = self.routes.lock() {
            for uuid in uuids {
                routes.remove(uuid);
            }
        }
    }

    pub fn send_input(&self, window_uuid: &str, event: InputEvent) -> bool {
        if let Ok(routes) = self.routes.lock() {
            if let Some(route) = routes.get(window_uuid) {
                let _ = route.tx.send((route.x11_window_id, WindowMessage::Input(event)));
                return true;
            }
        }
        false
    }

    pub fn send_resize(&self, window_uuid: &str, width: u16, height: u16) -> bool {
        if let Ok(routes) = self.routes.lock() {
            if let Some(route) = routes.get(window_uuid) {
                let _ = route.tx.send((route.x11_window_id, WindowMessage::Resize(width, height)));
                return true;
            }
        }
        false
    }
}

/// Shared window-manager state.
pub(crate) struct WmState {
    pub(crate) client_id: Option<String>,
    pub(crate) event_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

pub(crate) type SharedWmState = Arc<Mutex<WmState>>;

/// RAII guard that clears the shared WM state when the WM client disconnects.
pub(crate) struct WmCleanupGuard {
    pub(crate) wm_state: SharedWmState,
    pub(crate) client_id: String,
}

impl Drop for WmCleanupGuard {
    fn drop(&mut self) {
        if let Ok(mut wm) = self.wm_state.lock() {
            if wm.client_id.as_deref() == Some(&self.client_id) {
                tracing::info!("WM client {} disconnected – clearing WM state", self.client_id);
                wm.client_id = None;
                wm.event_tx = None;
            }
        }
    }
}

/// Stored X11 property value.
#[derive(Clone)]
pub(crate) struct PropertyValue {
    pub(crate) prop_type: u32,
    pub(crate) format: u8,
    pub(crate) data: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct WindowState {
    pub(crate) id: u32,
    pub(crate) parent: u32,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) border_width: u16,
    pub(crate) visual: u32,
    pub(crate) class: u16,
    pub(crate) mapped: bool,
    pub(crate) event_mask: u32,
    pub(crate) background_pixel: u32,
    pub(crate) override_redirect: bool,
    pub(crate) redirected: bool,
    pub(crate) framebuffer: Framebuffer,
    pub(crate) properties: HashMap<u32, PropertyValue>,
    pub(crate) owner_client_id: String,
    pub(crate) cursor: Option<u32>,
}

pub(crate) struct PixmapState {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) depth: u8,
    pub(crate) framebuffer: Framebuffer,
    pub(crate) alias_window: Option<u32>,
    pub(crate) shm_backing: Option<ShmPixmapBacking>,
}

#[derive(Clone)]
pub(crate) struct ShmPixmapBacking {
    pub(crate) shmseg: u32,
    pub(crate) offset: usize,
}

/// Full X11 Graphics Context state per the spec.
#[derive(Clone)]
pub(crate) struct GcState {
    pub(crate) function: u8,
    pub(crate) plane_mask: u32,
    pub(crate) foreground: u32,
    pub(crate) background: u32,
    pub(crate) line_width: u16,
    pub(crate) line_style: u8,
    pub(crate) cap_style: u8,
    pub(crate) join_style: u8,
    pub(crate) fill_style: u8,
    pub(crate) fill_rule: u8,
    pub(crate) tile: u32,
    pub(crate) stipple: u32,
    pub(crate) ts_x: i16,
    pub(crate) ts_y: i16,
    pub(crate) font_id: u32,
    pub(crate) subwindow_mode: u8,
    pub(crate) graphics_exposures: bool,
    pub(crate) clip_x: i16,
    pub(crate) clip_y: i16,
    pub(crate) clip_mask: u32,
    pub(crate) dash_offset: u16,
    pub(crate) dashes: u8,
    pub(crate) arc_mode: u8,
    /// Clip rectangles set by SetClipRectangles (empty = no clipping).
    pub(crate) clip_rects: Vec<(i16, i16, u16, u16)>,
    /// Dash pattern set by SetDashes (empty = use `dashes` field as uniform length).
    pub(crate) dash_list: Vec<u8>,
}

impl Default for GcState {
    fn default() -> Self {
        Self {
            function: 3, // GXcopy
            plane_mask: 0xFFFFFFFF,
            foreground: 0x00_00_00,
            background: 0xFF_FF_FF,
            line_width: 0,
            line_style: 0, // Solid
            cap_style: 1,  // Butt
            join_style: 0, // Miter
            fill_style: 0, // Solid
            fill_rule: 0,  // EvenOdd
            tile: 0,
            stipple: 0,
            ts_x: 0,
            ts_y: 0,
            font_id: 0,
            subwindow_mode: 0, // ClipByChildren
            graphics_exposures: true,
            clip_x: 0,
            clip_y: 0,
            clip_mask: 0, // None
            dash_offset: 0,
            dashes: 4,
            arc_mode: 1, // PieSlice
            clip_rects: Vec::new(),
            dash_list: Vec::new(),
        }
    }
}

/// Damage subscription info for DAMAGE extension.
#[derive(Clone)]
pub(crate) struct DamageInfo {
    pub(crate) drawable: u32,
    pub(crate) level: u8,
}

/// Present extension event subscription.
#[derive(Clone)]
pub(crate) struct PresentSubscription {
    pub(crate) window: u32,
    pub(crate) event_mask: u32,
}

/// A shared memory segment attached via MIT-SHM.
pub(crate) struct ShmSegment {
    pub(crate) addr: *mut u8,
    pub(crate) size: usize,
}

unsafe impl Send for ShmSegment {}
