use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use x11rb_protocol::protocol::xproto::*;
use x11rb_protocol::x11_utils::{Serialize, TryParse};

use tokio::sync::broadcast;
use uuid::Uuid;
use x11_web_protocol::{DisplayUpdate, InputEvent};

use crate::fonts::FontManager;
use crate::framebuffer::Framebuffer;

/// A display update tagged with the client_id that produced it.
pub type TaggedDisplayUpdate = (String, DisplayUpdate);

/// Minimal X11 server that accepts client connections and translates
/// X11 drawing operations into DisplayUpdate messages.
pub struct X11Server {
    display_number: u32,
    socket_path: PathBuf,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    input_tx: broadcast::Sender<(String, InputEvent)>,
    resize_tx: broadcast::Sender<(String, u16, u16)>,
    client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    window_uuids: WindowUuidMap,
}

/// Shared window registry, keyed by window ID.
/// All connections share a single window namespace, as required by X11.
pub(crate) type SharedWindows = Arc<Mutex<HashMap<u32, WindowState>>>;

/// Maps window UUID (sent to frontend) → (client_id, x11_window_id).
/// Used to route input/resize from the frontend to the correct X11 connection.
pub(crate) type WindowUuidMap = Arc<Mutex<HashMap<String, WindowUuidEntry>>>;

#[derive(Clone)]
pub(crate) struct WindowUuidEntry {
    pub(crate) client_id: String,
    pub(crate) x11_window_id: u32,
}

/// Shared window-manager state.
///
/// When a client sets SubstructureRedirectMask on the root window it becomes
/// the window manager.  Other clients' MapWindow / ConfigureWindow calls on
/// top-level windows are then redirected as MapRequest / ConfigureRequest
/// events to the WM client via its event sender.
pub(crate) struct WmState {
    /// ID of the client that owns SubstructureRedirect on the root.
    pub(crate) client_id: Option<String>,
    /// Channel to send X11 events (MapRequest, ConfigureRequest, …) to the WM
    /// client's event loop.
    pub(crate) event_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

pub(crate) type SharedWmState = Arc<Mutex<WmState>>;

/// RAII guard that clears the shared WM state when the WM client disconnects.
struct WmCleanupGuard {
    wm_state: SharedWmState,
    client_id: String,
}

impl Drop for WmCleanupGuard {
    fn drop(&mut self) {
        if let Ok(mut wm) = self.wm_state.lock() {
            if wm.client_id.as_deref() == Some(&self.client_id) {
                info!("WM client {} disconnected – clearing WM state", self.client_id);
                wm.client_id = None;
                wm.event_tx = None;
            }
        }
    }
}

/// Per-connection state for an X11 client.
pub(crate) struct ClientState {
    pub(crate) client_id: String,
    pub(crate) sequence: u16,
    /// Local snapshot of windows. Before each request batch, sync from shared.
    /// After each request batch, sync back.
    pub(crate) windows: HashMap<u32, WindowState>,
    /// Shared window store across all connections.
    pub(crate) shared_windows: SharedWindows,
    pub(crate) pixmaps: HashMap<u32, PixmapState>,
    pub(crate) gcs: HashMap<u32, GcState>,
    pub(crate) atoms: Arc<Mutex<AtomManager>>,
    pub(crate) update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    pub(crate) root_window: u32,
    pub(crate) root_width: u16,
    pub(crate) root_height: u16,
    pub(crate) pointer_x: i16,
    pub(crate) pointer_y: i16,
    pub(crate) focus_window: u32,
    pub(crate) font_manager: FontManager,
    pub(crate) render: crate::render::RenderState,
    pub(crate) selections: HashMap<u32, u32>,
    pub(crate) shm_segments: HashMap<u32, ShmSegment>,
    /// Shared WM state – used to redirect MapWindow/ConfigureWindow to the WM.
    pub(crate) wm_state: SharedWmState,
    /// Sender half of this client's WM event channel.  Cloned into shared
    /// WmState when this client registers as the window manager.
    pub(crate) wm_events_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// DAMAGE extension: active damage subscriptions (damage_id -> DamageInfo).
    pub(crate) damage_regions: HashMap<u32, DamageInfo>,
    /// Present extension: event subscriptions (event_id -> PresentSubscription).
    pub(crate) present_subscriptions: HashMap<u32, PresentSubscription>,
    /// Pending X11 events to deliver to this client (e.g. DamageNotify).
    pub(crate) pending_events: Vec<Vec<u8>>,
    /// Shared UUID map for routing input from frontend to the right connection.
    pub(crate) window_uuids: WindowUuidMap,
    /// Local map: x11_window_id → uuid (for this client's top-level windows).
    pub(crate) x11_to_uuid: HashMap<u32, String>,
}

impl ClientState {
    /// Get or create a UUID for a top-level X11 window.
    pub(crate) fn get_or_create_window_uuid(&mut self, x11_wid: u32) -> String {
        if let Some(uuid) = self.x11_to_uuid.get(&x11_wid) {
            return uuid.clone();
        }
        let uuid = Uuid::new_v4().to_string();
        self.x11_to_uuid.insert(x11_wid, uuid.clone());
        if let Ok(mut map) = self.window_uuids.lock() {
            map.insert(uuid.clone(), WindowUuidEntry {
                client_id: self.client_id.clone(),
                x11_window_id: x11_wid,
            });
        }
        uuid
    }

    /// Get the UUID for a top-level X11 window (if it exists).
    pub(crate) fn get_window_uuid(&self, x11_wid: u32) -> Option<String> {
        self.x11_to_uuid.get(&x11_wid).cloned()
    }

    /// Get the window ID string to send in display updates.
    /// For top-level windows, returns the UUID. For others, returns the X11 ID as string.
    pub(crate) fn window_id_str(&self, x11_wid: u32) -> String {
        self.x11_to_uuid.get(&x11_wid).cloned().unwrap_or_else(|| x11_wid.to_string())
    }

    /// Intern an atom name, returning its global ID.
    fn intern_atom(&self, name: &str, only_if_exists: bool) -> u32 {
        self.atoms.lock().unwrap().intern(name, only_if_exists)
    }

    /// Get the name of an atom by its global ID.
    fn get_atom_name(&self, atom: u32) -> Option<String> {
        self.atoms.lock().unwrap().get_name(atom).map(|s| s.to_string())
    }

    /// Map a color value for the drawable's depth. In depth-1 pixmaps,
    /// color 0 = black and any non-zero = white (0xFFFFFF).
    pub(crate) fn map_color_for_drawable(&self, drawable: u32, color: u32) -> u32 {
        let depth = self.pixmaps.get(&drawable).map(|p| p._depth).unwrap_or(24);
        if depth <= 1 {
            if color != 0 { 0xFFFFFF } else { 0x000000 }
        } else {
            color
        }
    }

    /// Get a mutable reference to the framebuffer for a drawable (window or pixmap).
    /// For NameWindowPixmap aliases, this returns the window's framebuffer.
    pub(crate) fn get_framebuffer_mut(&mut self, drawable: u32) -> Option<&mut Framebuffer> {
        // First resolve the actual target: if it's a pixmap aliasing a window,
        // redirect to the window ID.
        let target = self.resolve_drawable(drawable);

        if let Some(win) = self.windows.get_mut(&target) {
            return Some(&mut win.framebuffer);
        }
        if let Some(pix) = self.pixmaps.get_mut(&target) {
            return Some(&mut pix.framebuffer);
        }
        None
    }

    /// Queue DamageNotify events for any DAMAGE subscriptions on the given drawable.
    /// This should be called after any drawing operation that modifies a pixmap,
    /// so that clients (like xeyes) using DAMAGE+Present know to present the updated content.
    pub(crate) fn notify_damage(&mut self, drawable: u32, x: i16, y: i16, width: u16, height: u16) {
        let resolved = self.resolve_drawable(drawable);
        let matches: Vec<(u32, u8)> = self
            .damage_regions
            .iter()
            .filter(|(_, info)| info.drawable == resolved)
            .map(|(&did, info)| (did, info.level))
            .collect();

        for (damage_id, level) in matches {
            let mut event = [0u8; 32];
            event[0] = 91; // DAMAGE first_event + DamageNotify
            event[1] = level;
            event[2..4].copy_from_slice(&self.sequence.to_le_bytes());
            event[4..8].copy_from_slice(&resolved.to_le_bytes()); // drawable
            event[8..12].copy_from_slice(&damage_id.to_le_bytes()); // damage
            // timestamp = 0
            event[14..16].copy_from_slice(&(x as u16).to_le_bytes()); // area.x
            event[16..18].copy_from_slice(&(y as u16).to_le_bytes()); // area.y
            event[18..20].copy_from_slice(&width.to_le_bytes()); // area.width
            event[20..22].copy_from_slice(&height.to_le_bytes()); // area.height
            // geometry = area (for raw level, area == geometry)
            event[22..24].copy_from_slice(&(x as u16).to_le_bytes());
            event[24..26].copy_from_slice(&(y as u16).to_le_bytes());
            event[26..28].copy_from_slice(&width.to_le_bytes());
            event[28..30].copy_from_slice(&height.to_le_bytes());
            self.pending_events.push(event.to_vec());
        }
    }

    /// Resolve a drawable ID to the actual drawable ID (following aliases).
    pub(crate) fn resolve_drawable(&self, drawable: u32) -> u32 {
        if let Some(pix) = self.pixmaps.get(&drawable) {
            if let Some(alias_wid) = pix.alias_window {
                return alias_wid;
            }
        }
        drawable
    }

    /// If `drawable` is an SHM-backed pixmap, copy the current SHM segment
    /// contents into the pixmap's framebuffer so subsequent reads see fresh data.
    /// This must be called before reading pixels from any drawable that might be
    /// SHM-backed (e.g. before CopyArea src, Render Composite src, GetImage).
    pub(crate) fn sync_shm_pixmap(&mut self, drawable: u32) {
        let target = self.resolve_drawable(drawable);
        let (shmseg, offset, width, height) = {
            let pix = match self.pixmaps.get(&target) {
                Some(p) => p,
                None => return,
            };
            let backing = match &pix.shm_backing {
                Some(b) => b,
                None => return,
            };
            (backing.shmseg, backing.offset, pix._width as usize, pix._height as usize)
        };

        let seg = match self.shm_segments.get(&shmseg) {
            Some(s) => s,
            None => {
                warn!("sync_shm_pixmap: SHM segment {shmseg} not found for pixmap {target:#x}");
                return;
            }
        };

        let bpp = 4usize;
        let stride = width * bpp;
        let total_bytes = stride * height;

        if offset + total_bytes > seg.size {
            warn!(
                "sync_shm_pixmap: out of bounds (offset={offset} + size={total_bytes} > seg.size={})",
                seg.size
            );
            return;
        }

        // Copy SHM data into the pixmap's framebuffer
        let pix = self.pixmaps.get_mut(&target).unwrap();
        let fb = &mut pix.framebuffer;
        let fb_data = fb.data_mut();
        unsafe {
            let src_ptr = seg.addr.add(offset);
            let copy_len = total_bytes.min(fb_data.len());
            std::ptr::copy_nonoverlapping(src_ptr, fb_data.as_mut_ptr(), copy_len);
        }
    }

    /// Sync local windows with the shared store.
    /// Pulls in windows created by other connections and pushes our changes.
    pub(crate) fn sync_windows(&mut self) {
        if let Ok(mut shared) = self.shared_windows.lock() {
            // Pull: add windows from shared that we don't have locally
            for (&wid, shared_win) in shared.iter() {
                if let Some(local_win) = self.windows.get_mut(&wid) {
                    // Merge: pull mapped flag and properties from shared
                    if shared_win.mapped && !local_win.mapped {
                        local_win.mapped = true;
                    }
                    if shared_win.redirected {
                        local_win.redirected = true;
                    }
                    // Sync properties from shared
                    for (&atom, val) in shared_win.properties.iter() {
                        if !local_win.properties.contains_key(&atom) {
                            local_win.properties.insert(atom, val.clone());
                        }
                    }
                } else {
                    self.windows.insert(wid, shared_win.clone());
                }
            }

            // Push: update shared with our local changes
            for (&wid, local_win) in self.windows.iter() {
                if let Some(shared_win) = shared.get_mut(&wid) {
                    if local_win.mapped {
                        shared_win.mapped = true;
                    }
                    if local_win.redirected {
                        shared_win.redirected = true;
                    }
                    for (&atom, val) in local_win.properties.iter() {
                        shared_win.properties.insert(atom, val.clone());
                    }
                    shared_win.x = local_win.x;
                    shared_win.y = local_win.y;
                    shared_win.width = local_win.width;
                    shared_win.height = local_win.height;
                } else {
                    shared.insert(wid, local_win.clone());
                }
            }

            // Remove from shared any windows that were destroyed locally
            let shared_ids: Vec<u32> = shared.keys().copied().collect();
            for wid in shared_ids {
                if !self.windows.contains_key(&wid) {
                    shared.remove(&wid);
                }
            }
        }
    }

    /// Send dirty framebuffer regions for all mapped windows as PutImage updates.
    fn flush_dirty_windows(&mut self) {
        // Step 1: Composite dirty child windows up into their top-level ancestor.
        // Walk up the parent chain so even deeply nested children reach the
        // top-level window.  After compositing, clear the child's dirty flag
        // so only top-level windows remain dirty.
        let children: Vec<(u32, u16, u16)> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped
                    && w.framebuffer.is_dirty()
                    && w.parent != self.root_window
                    && w.parent != 0
                    && w.class == 1 // InputOutput only
            })
            .map(|(_, w)| (w.id, w.width, w.height))
            .collect();

        for (child_id, cw, ch) in &children {
            // Walk up to find the top-level ancestor and accumulate offsets
            let mut target = *child_id;
            let mut off_x: i32 = 0;
            let mut off_y: i32 = 0;
            for _ in 0..10 {
                let (parent, wx, wy) = match self.windows.get(&target) {
                    Some(w) if w.parent != self.root_window && w.parent != 0 => {
                        (w.parent, w.x as i32, w.y as i32)
                    }
                    _ => break,
                };
                off_x += wx;
                off_y += wy;
                target = parent;
            }

            if target == *child_id {
                continue; // Already top-level, nothing to composite
            }

            // Extract child pixels and composite into ancestor
            let pixels = if let Some(child) = self.windows.get(child_id) {
                Some(child.framebuffer.extract_pixels(0, 0, *cw, *ch))
            } else {
                None
            };

            if let Some(pixels) = pixels {
                if let Some(ancestor) = self.windows.get_mut(&target) {
                    ancestor.framebuffer.put_image(off_x as i16, off_y as i16, *cw, *ch, &pixels);
                }
            }

            // Clear child dirty flag
            if let Some(child) = self.windows.get_mut(child_id) {
                child.framebuffer.clear_dirty();
            }
        }


        // Step 2: Only send PutImage for top-level windows (parent == root).
        let window_ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, w)| {
                w.mapped
                    && w.framebuffer.is_dirty()
                    && w.parent == self.root_window
                    && w.class == 1
            })
            .map(|(id, _)| *id)
            .collect();

        for wid in window_ids {
            let wid_str = self.window_id_str(wid);
            if let Some(win) = self.windows.get_mut(&wid) {
                if let Some((x, y, w, h, pixels)) = win.framebuffer.take_dirty_pixels() {
                    // Use the window's owner client_id for routing display updates
                    let owner = if win.owner_client_id.is_empty() {
                        self.client_id.clone()
                    } else {
                        win.owner_client_id.clone()
                    };
                    let _ = self.update_tx.send((
                        owner,
                        DisplayUpdate::PutImage {
                            window_id: wid_str.clone(),
                            x,
                            y,
                            width: w,
                            height: h,
                            data: pixels,
                        },
                    ));

                    // Send DamageNotify events for any damage subscriptions on this window
                    let win_width = win.width;
                    let win_height = win.height;
                    let damage_matches: Vec<(u32, u8)> = self
                        .damage_regions
                        .iter()
                        .filter(|(_, info)| info.drawable == wid)
                        .map(|(&did, info)| (did, info.level))
                        .collect();

                    for (damage_id, level) in damage_matches {
                        let mut event = [0u8; 32];
                        event[0] = 91; // DAMAGE first_event + 0 (DamageNotify)
                        event[1] = level;
                        event[2..4].copy_from_slice(&self.sequence.to_le_bytes());
                        event[4..8].copy_from_slice(&wid.to_le_bytes()); // drawable
                        event[8..12].copy_from_slice(&damage_id.to_le_bytes()); // damage
                        // timestamp = 0
                        event[14..16].copy_from_slice(&(x as u16).to_le_bytes()); // area.x
                        event[16..18].copy_from_slice(&(y as u16).to_le_bytes()); // area.y
                        event[18..20].copy_from_slice(&w.to_le_bytes()); // area.width
                        event[20..22].copy_from_slice(&h.to_le_bytes()); // area.height
                        // geometry = full window
                        // geometry.x and geometry.y = 0
                        event[26..28].copy_from_slice(&win_width.to_le_bytes()); // geometry.width
                        event[28..30].copy_from_slice(&win_height.to_le_bytes()); // geometry.height
                        self.pending_events.push(event.to_vec());
                    }
                }
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
    /// Properties set on this window (atom -> value).
    pub(crate) properties: HashMap<u32, PropertyValue>,
    /// The client_id that created this window (for display update routing).
    pub(crate) owner_client_id: String,
}

pub(crate) struct PixmapState {
    pub(crate) _id: u32,
    pub(crate) _width: u16,
    pub(crate) _height: u16,
    pub(crate) _depth: u8,
    pub(crate) framebuffer: Framebuffer,
    /// If this pixmap is a NameWindowPixmap alias, this holds the window ID.
    /// Drawing to this pixmap should actually draw to the window's framebuffer.
    pub(crate) alias_window: Option<u32>,
    /// If this pixmap is SHM-backed, the segment ID and offset into it.
    /// The client writes directly into shared memory; we must sync before reads.
    pub(crate) shm_backing: Option<ShmPixmapBacking>,
}

/// Tracks the SHM segment backing an SHM-created pixmap.
#[derive(Clone)]
pub(crate) struct ShmPixmapBacking {
    pub(crate) shmseg: u32,
    pub(crate) offset: usize,
}

#[derive(Clone)]
pub(crate) struct GcState {
    foreground: u32,
    background: u32,
    line_width: u16,
    function: u8,
    font_id: u32,
}

impl Default for GcState {
    fn default() -> Self {
        Self {
            foreground: 0x00_00_00, // black
            background: 0xFF_FF_FF, // white
            line_width: 0,
            function: 3, // GXcopy
            font_id: 0,
        }
    }
}

/// Damage subscription info for DAMAGE extension.
#[derive(Clone)]
pub(crate) struct DamageInfo {
    pub(crate) drawable: u32,  // the window being monitored
    pub(crate) level: u8,      // damage level (RawRectangles=0, DeltaRectangles=1, BoundingBox=2, NonEmpty=3)
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

// Safety: ShmSegment holds a raw pointer from shmat() which is valid for the
// lifetime of the attachment.  We only access it on the single client-handler
// task, so Send is fine.
unsafe impl Send for ShmSegment {}

pub(crate) struct AtomManager {
    atoms: HashMap<String, u32>,
    reverse: HashMap<u32, String>,
    next_atom: u32,
}

impl AtomManager {
    fn new() -> Self {
        let mut mgr = Self {
            atoms: HashMap::new(),
            reverse: HashMap::new(),
            next_atom: 1,
        };
        // Pre-register some standard atoms
        for (name, id) in PREDEFINED_ATOMS {
            mgr.atoms.insert(name.to_string(), *id);
            mgr.reverse.insert(*id, name.to_string());
            if *id >= mgr.next_atom {
                mgr.next_atom = *id + 1;
            }
        }
        mgr
    }

    fn intern(&mut self, name: &str, only_if_exists: bool) -> u32 {
        if let Some(&id) = self.atoms.get(name) {
            return id;
        }
        if only_if_exists {
            return 0;
        }
        let id = self.next_atom;
        self.next_atom += 1;
        self.atoms.insert(name.to_string(), id);
        self.reverse.insert(id, name.to_string());
        id
    }

    fn get_name(&self, atom: u32) -> Option<&str> {
        self.reverse.get(&atom).map(|s| s.as_str())
    }
}

const PREDEFINED_ATOMS: &[(&str, u32)] = &[
    ("PRIMARY", 1),
    ("SECONDARY", 2),
    ("ARC", 3),
    ("ATOM", 4),
    ("BITMAP", 5),
    ("CARDINAL", 6),
    ("COLORMAP", 7),
    ("CURSOR", 8),
    ("CUT_BUFFER0", 9),
    ("CUT_BUFFER1", 10),
    ("CUT_BUFFER2", 11),
    ("CUT_BUFFER3", 12),
    ("CUT_BUFFER4", 13),
    ("CUT_BUFFER5", 14),
    ("CUT_BUFFER6", 15),
    ("CUT_BUFFER7", 16),
    ("DRAWABLE", 17),
    ("FONT", 18),
    ("INTEGER", 19),
    ("PIXMAP", 20),
    ("POINT", 21),
    ("RECTANGLE", 22),
    ("RESOURCE_MANAGER", 23),
    ("RGB_COLOR_MAP", 24),
    ("RGB_BEST_MAP", 25),
    ("RGB_BLUE_MAP", 26),
    ("RGB_DEFAULT_MAP", 27),
    ("RGB_GRAY_MAP", 28),
    ("RGB_GREEN_MAP", 29),
    ("RGB_RED_MAP", 30),
    ("STRING", 31),
    ("VISUALID", 32),
    ("WINDOW", 33),
    ("WM_COMMAND", 34),
    ("WM_HINTS", 35),
    ("WM_CLIENT_MACHINE", 36),
    ("WM_ICON_NAME", 37),
    ("WM_ICON_SIZE", 38),
    ("WM_NAME", 39),
    ("WM_NORMAL_HINTS", 40),
    ("WM_SIZE_HINTS", 41),
    ("WM_ZOOM_HINTS", 42),
    ("MIN_SPACE", 43),
    ("NORM_SPACE", 44),
    ("MAX_SPACE", 45),
    ("END_SPACE", 46),
    ("SUPERSCRIPT_X", 47),
    ("SUPERSCRIPT_Y", 48),
    ("SUBSCRIPT_X", 49),
    ("SUBSCRIPT_Y", 50),
    ("UNDERLINE_POSITION", 51),
    ("UNDERLINE_THICKNESS", 52),
    ("STRIKEOUT_ASCENT", 53),
    ("STRIKEOUT_DESCENT", 54),
    ("ITALIC_ANGLE", 55),
    ("X_HEIGHT", 56),
    ("QUAD_WIDTH", 57),
    ("WEIGHT", 58),
    ("POINT_SIZE", 59),
    ("RESOLUTION", 60),
    ("COPYRIGHT", 61),
    ("NOTICE", 62),
    ("FONT_NAME", 63),
    ("FAMILY_NAME", 64),
    ("FULL_NAME", 65),
    ("CAP_HEIGHT", 66),
    ("WM_CLASS", 67),
    ("WM_TRANSIENT_FOR", 68),
];

impl X11Server {
    pub fn new(
        display_number: u32,
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        input_tx: broadcast::Sender<(String, InputEvent)>,
        resize_tx: broadcast::Sender<(String, u16, u16)>,
        client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    ) -> Self {
        let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));
        Self {
            display_number,
            socket_path,
            update_tx,
            input_tx,
            resize_tx,
            client_connected_tx,
            window_uuids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a reference to the window UUID map (for the main event loop to access).
    pub fn window_uuids(&self) -> WindowUuidMap {
        self.window_uuids.clone()
    }

    pub fn display_string(&self) -> String {
        format!(":{}", self.display_number)
    }

    pub async fn run(self) -> io::Result<()> {
        // Ensure socket directory exists
        let dir = self.socket_path.parent().unwrap();
        tokio::fs::create_dir_all(dir).await.ok();

        // Remove stale socket
        tokio::fs::remove_file(&self.socket_path).await.ok();

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(
            "X11 server listening on {} (DISPLAY={})",
            self.socket_path.display(),
            self.display_string()
        );

        static CONNECTION_COUNTER: AtomicU32 = AtomicU32::new(0);

        // Shared atom manager — atom IDs must be global across all X11 connections
        let shared_atoms: Arc<Mutex<AtomManager>> = Arc::new(Mutex::new(AtomManager::new()));

        // Shared window state across all connections
        let shared_windows: SharedWindows = Arc::new(Mutex::new(HashMap::new()));
        // Shared WM state – tracks which client is the window manager
        let shared_wm_state: SharedWmState = Arc::new(Mutex::new(WmState {
            client_id: None,
            event_tx: None,
        }));
        // Pre-populate with root window
        {
            let mut windows = shared_windows.lock().unwrap();
            windows.insert(
                ROOT_WINDOW,
                WindowState {
                    id: ROOT_WINDOW,
                    parent: 0,
                    x: 0,
                    y: 0,
                    width: SCREEN_WIDTH,
                    height: SCREEN_HEIGHT,
                    border_width: 0,
                    visual: ROOT_VISUAL,
                    class: 1,
                    mapped: true,
                    event_mask: 0,
                    background_pixel: 0x00000000,
                    override_redirect: false,
                    redirected: false,
                    framebuffer: Framebuffer::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
                    properties: HashMap::new(),
                    owner_client_id: String::new(), // root has no owner
                },
            );

            // Set EWMH properties on root at startup so they're available to ALL connections
            {
                let mut atoms = shared_atoms.lock().unwrap();
                let a_swc = atoms.intern("_NET_SUPPORTING_WM_CHECK", false);
                let a_name = atoms.intern("_NET_WM_NAME", false);
                let a_utf8 = atoms.intern("UTF8_STRING", false);
                let a_supported = atoms.intern("_NET_SUPPORTED", false);
                let a_active = atoms.intern("_NET_ACTIVE_WINDOW", false);
                let a_client_list = atoms.intern("_NET_CLIENT_LIST", false);
                let a_frame = atoms.intern("_NET_FRAME_EXTENTS", false);

                let supported = [
                    atoms.intern("_NET_WM_STATE", false),
                    atoms.intern("_NET_WM_STATE_FOCUSED", false),
                    a_active,
                    a_name,
                    atoms.intern("_NET_WM_PID", false),
                    atoms.intern("_NET_WM_WINDOW_TYPE", false),
                    atoms.intern("_NET_WM_WINDOW_TYPE_NORMAL", false),
                    a_frame,
                    a_swc,
                    a_supported,
                    a_client_list,
                ];
                let mut sup_data = Vec::new();
                for a in &supported { sup_data.extend_from_slice(&a.to_le_bytes()); }

                let root = windows.get_mut(&ROOT_WINDOW).unwrap();
                root.properties.insert(a_swc, PropertyValue { prop_type: 33, format: 32, data: ROOT_WINDOW.to_le_bytes().to_vec() });
                root.properties.insert(a_name, PropertyValue { prop_type: a_utf8, format: 8, data: b"x11-web".to_vec() });
                root.properties.insert(a_supported, PropertyValue { prop_type: 4, format: 32, data: sup_data });
                root.properties.insert(a_active, PropertyValue { prop_type: 33, format: 32, data: ROOT_WINDOW.to_le_bytes().to_vec() });
                root.properties.insert(a_client_list, PropertyValue { prop_type: 33, format: 32, data: Vec::new() });
                root.properties.insert(a_frame, PropertyValue { prop_type: 6, format: 32, data: vec![0; 16] });
            }
        }

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let conn_index = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
                    let client_id = Uuid::new_v4().to_string();
                    // Get the PID of the connecting process via SO_PEERCRED
                    let peer_pid = stream.peer_cred().ok().and_then(|c| c.pid()).unwrap_or(0) as u32;
                    let update_tx = self.update_tx.clone();
                    let input_rx = self.input_tx.subscribe();
                    let resize_rx = self.resize_tx.subscribe();
                    let _ = self.client_connected_tx.send((client_id.clone(), peer_pid));
                    let cid = client_id.clone();
                    let sw = shared_windows.clone();
                    let wm = shared_wm_state.clone();
                    let sa = shared_atoms.clone();
                    let wuuids = self.window_uuids.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_client(stream, client_id, update_tx, input_rx, resize_rx, conn_index, sw, wm, sa, wuuids).await
                        {
                            debug!("X11 client {cid} disconnected: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept X11 connection: {e}");
                }
            }
        }
    }
}

impl Drop for X11Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

const ROOT_WINDOW: u32 = 0x00000062; // arbitrary root window ID
const ROOT_VISUAL: u32 = 0x00000021;
const ROOT_COLORMAP: u32 = 0x00000020;
const SCREEN_WIDTH: u16 = 1024;
const SCREEN_HEIGHT: u16 = 768;

fn build_setup(conn_index: u32) -> Setup {
    let visual = Visualtype {
        visual_id: ROOT_VISUAL,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth24 = Depth {
        depth: 24,
        visuals: vec![visual],
    };

    // ARGB visual for depth 32 — needed by GTK3/wxWidgets for RGBA windows
    let visual_argb = Visualtype {
        visual_id: 0x40, // ARGB_VISUAL
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth32 = Depth {
        depth: 32,
        visuals: vec![visual_argb],
    };

    let screen = Screen {
        root: ROOT_WINDOW,
        default_colormap: ROOT_COLORMAP,
        white_pixel: 0x00FFFFFF,
        black_pixel: 0x00000000,
        current_input_masks: EventMask::from(0u32),
        width_in_pixels: SCREEN_WIDTH,
        height_in_pixels: SCREEN_HEIGHT,
        width_in_millimeters: 270,
        height_in_millimeters: 203,
        min_installed_maps: 1,
        max_installed_maps: 1,
        root_visual: ROOT_VISUAL,
        backing_stores: BackingStore::NOT_USEFUL,
        save_unders: false,
        root_depth: 24,
        allowed_depths: vec![depth24, depth32],
    };

    let format24 = Format {
        depth: 24,
        bits_per_pixel: 32,
        scanline_pad: 32,
    };
    let format32 = Format {
        depth: 32,
        bits_per_pixel: 32,
        scanline_pad: 32,
    };
    let format1 = Format {
        depth: 1,
        bits_per_pixel: 1,
        scanline_pad: 32,
    };

    // Build initial setup - length will be computed after serialization
    let mut setup = Setup {
        status: 1,
        protocol_major_version: 11,
        protocol_minor_version: 0,
        length: 0, // will fix below
        release_number: 0,
        // Each connection gets a unique resource ID range.
        // Base = (conn_index + 1) << 22, mask = 0x003FFFFF (4M IDs per connection)
        // This allows up to ~1000 connections without overlap.
        resource_id_base: ((conn_index + 1) as u32) << 22,
        resource_id_mask: 0x003FFFFF,
        motion_buffer_size: 256,
        maximum_request_length: 65535,
        image_byte_order: ImageOrder::LSB_FIRST,
        bitmap_format_bit_order: ImageOrder::LSB_FIRST,
        bitmap_format_scanline_unit: 32,
        bitmap_format_scanline_pad: 32,
        min_keycode: 8,
        max_keycode: 255,
        vendor: b"x11-web".to_vec(),
        pixmap_formats: vec![format1, format24, format32],
        roots: vec![screen],
    };

    // Compute length: serialize, subtract 8 bytes header, divide by 4
    let mut bytes = Vec::new();
    setup.serialize_into(&mut bytes);
    setup.length = ((bytes.len() - 8) / 4) as u16;

    setup
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    client_id: String,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    mut input_rx: broadcast::Receiver<(String, InputEvent)>,
    mut resize_rx: broadcast::Receiver<(String, u16, u16)>,
    conn_index: u32,
    shared_windows: SharedWindows,
    shared_wm_state: SharedWmState,
    shared_atoms: Arc<Mutex<AtomManager>>,
    window_uuids: WindowUuidMap,
) -> io::Result<()> {
    // Phase 1: Read client setup request
    // Read at least 12 bytes for the header
    let mut header_buf = [0u8; 12];
    stream.read_exact(&mut header_buf).await?;

    let byte_order = header_buf[0];
    if byte_order != 0x6c && byte_order != 0x42 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid byte order: 0x{:02x}", byte_order),
        ));
    }

    // Parse auth lengths from the header
    let (auth_name_len, auth_data_len) = if byte_order == 0x6c {
        (
            u16::from_le_bytes([header_buf[6], header_buf[7]]),
            u16::from_le_bytes([header_buf[8], header_buf[9]]),
        )
    } else {
        (
            u16::from_be_bytes([header_buf[6], header_buf[7]]),
            u16::from_be_bytes([header_buf[8], header_buf[9]]),
        )
    };

    // Calculate total setup request size
    fn pad4(n: u16) -> usize {
        let n = n as usize;
        (n + 3) & !3
    }
    let total_len = 12 + pad4(auth_name_len) + pad4(auth_data_len);
    let mut setup_buf = vec![0u8; total_len];
    setup_buf[..12].copy_from_slice(&header_buf);
    if total_len > 12 {
        stream.read_exact(&mut setup_buf[12..]).await?;
    }

    // We don't validate auth - just accept everything
    let _setup_request = SetupRequest::try_parse(&setup_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Bad setup: {e:?}")))?;

    // Phase 2: Send setup reply
    let setup = build_setup(conn_index);
    let mut reply_bytes = Vec::new();
    setup.serialize_into(&mut reply_bytes);
    stream.write_all(&reply_bytes).await?;

    info!("X11 client connected: {client_id}");

    // Phase 3: Handle requests
    // Initialize local windows from the shared store (includes root window).
    let local_windows = shared_windows.lock().unwrap().clone();

    // Create a channel for receiving WM events (MapRequest, ConfigureRequest)
    // directed at this client when it becomes the window manager.
    let (wm_events_tx, mut wm_events_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let mut state = ClientState {
        client_id: client_id.clone(),
        sequence: 0,
        windows: local_windows,
        shared_windows,
        pixmaps: HashMap::new(),
        gcs: HashMap::new(),
        atoms: shared_atoms,
        update_tx,
        root_window: ROOT_WINDOW,
        root_width: SCREEN_WIDTH,
        root_height: SCREEN_HEIGHT,
        pointer_x: 0,
        pointer_y: 0,
        focus_window: ROOT_WINDOW,
        font_manager: FontManager::new(),
        render: crate::render::RenderState::new(),
        selections: HashMap::new(),
        shm_segments: HashMap::new(),
        wm_state: shared_wm_state.clone(),
        wm_events_tx: wm_events_tx,
        damage_regions: HashMap::new(),
        present_subscriptions: HashMap::new(),
        pending_events: Vec::new(),
        window_uuids,
        x11_to_uuid: HashMap::new(),
    };

    // Set EWMH properties on the root window so GDK3/Firefox recognise an
    // EWMH-compliant WM.  We do this per-connection so that each client's
    // AtomManager has consistent atom IDs for the property keys it stores.
    {
        let atom_supporting_wm_check = state.intern_atom("_NET_SUPPORTING_WM_CHECK", false);
        let atom_window: u32 = 33; // WINDOW predefined atom
        let atom_net_wm_name = state.intern_atom("_NET_WM_NAME", false);
        let atom_utf8 = state.intern_atom("UTF8_STRING", false);
        let atom_net_supported = state.intern_atom("_NET_SUPPORTED", false);
        let atom_atom: u32 = 4; // ATOM predefined atom
        let atom_cardinal: u32 = 6; // CARDINAL predefined atom

        let supported_atoms = [
            state.intern_atom("_NET_WM_STATE", false),
            state.intern_atom("_NET_WM_STATE_FOCUSED", false),
            state.intern_atom("_NET_ACTIVE_WINDOW", false),
            atom_net_wm_name,
            state.intern_atom("_NET_WM_PID", false),
            state.intern_atom("_NET_WM_WINDOW_TYPE", false),
            state.intern_atom("_NET_WM_WINDOW_TYPE_NORMAL", false),
            state.intern_atom("_NET_FRAME_EXTENTS", false),
            atom_supporting_wm_check,
            atom_net_supported,
            state.intern_atom("_NET_CLIENT_LIST", false),
        ];

        // Pre-compute all atom values and root_window before borrowing windows mutably
        let atom_active = state.intern_atom("_NET_ACTIVE_WINDOW", false);
        let atom_client_list = state.intern_atom("_NET_CLIENT_LIST", false);
        let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
        let root_wid = state.root_window;

        let mut supported_data = Vec::new();
        for a in &supported_atoms {
            supported_data.extend_from_slice(&a.to_le_bytes());
        }

        if let Some(root) = state.windows.get_mut(&root_wid) {
            root.properties.insert(atom_supporting_wm_check, PropertyValue {
                prop_type: atom_window,
                format: 32,
                data: root_wid.to_le_bytes().to_vec(),
            });

            root.properties.insert(atom_net_wm_name, PropertyValue {
                prop_type: atom_utf8,
                format: 8,
                data: b"x11-web".to_vec(),
            });

            root.properties.insert(atom_net_supported, PropertyValue {
                prop_type: atom_atom,
                format: 32,
                data: supported_data,
            });

            root.properties.insert(atom_active, PropertyValue {
                prop_type: atom_window,
                format: 32,
                data: root_wid.to_le_bytes().to_vec(),
            });

            root.properties.insert(atom_client_list, PropertyValue {
                prop_type: atom_window,
                format: 32,
                data: Vec::new(),
            });

            root.properties.insert(atom_frame, PropertyValue {
                prop_type: atom_cardinal,
                format: 32,
                data: vec![0; 16], // left, right, top, bottom = 0
            });
        }

        // Push EWMH properties to shared store immediately
        state.sync_windows();
    }

    let mut buf = vec![0u8; 256 * 1024]; // 256KB read buffer
    let mut pending = Vec::new(); // Partial request data
    let mut frame_interval = tokio::time::interval(Duration::from_millis(100)); // ~10fps

    // Guard: clear WM state if this client was the WM when we exit.
    let _wm_guard = WmCleanupGuard {
        wm_state: shared_wm_state,
        client_id: client_id.clone(),
    };

    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    return Ok(()); // Client disconnected
                }

                pending.extend_from_slice(&buf[..n]);

                // Sync windows from shared store before processing
                state.sync_windows();

                // Process complete requests from the pending buffer
                while pending.len() >= 4 {
                    let req_len_units = u16::from_le_bytes([pending[2], pending[3]]) as usize;
                    let req_len_bytes = req_len_units * 4;

                    if req_len_bytes == 0 {
                        if pending.len() < 8 {
                            break;
                        }
                        let big_len =
                            u32::from_le_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize;
                        let big_bytes = big_len * 4;
                        if pending.len() < big_bytes {
                            break;
                        }
                        state.sequence = state.sequence.wrapping_add(1);
                        pending.drain(..big_bytes);
                        continue;
                    }

                    if pending.len() < req_len_bytes {
                        break;
                    }

                    let request_data: Vec<u8> = pending.drain(..req_len_bytes).collect();
                    state.sequence = state.sequence.wrapping_add(1);

                    let response = handle_request(&mut state, &request_data);
                    if !response.is_empty() {
                        stream.write_all(&response).await?;
                    }
                }

                // Sync windows back to shared store
                state.sync_windows();

                // Flush dirty windows after processing a batch of requests
                state.flush_dirty_windows();

                // Deliver any pending events (e.g. DamageNotify)
                for event in state.pending_events.drain(..) {
                    stream.write_all(&event).await?;
                }
            }
            _ = frame_interval.tick() => {
                // Sync with shared windows
                state.sync_windows();

                // Periodic frame flush for any remaining dirty regions
                state.flush_dirty_windows();

                // Deliver any pending events (e.g. DamageNotify)
                for event in state.pending_events.drain(..) {
                    stream.write_all(&event).await?;
                }

                // Act as a minimal WM: auto-map any unmapped top-level windows.
                // We do this on the tick rather than in CreateWindow so that the
                // client has time to finish its setup (ChangeProperty, SHAPE, etc.)
                // before we send the MapNotify/ConfigureNotify/Expose events.
                //
                // Skip auto-mapping when a real WM is connected – it will
                // handle MapRequest / ConfigureRequest itself.
                let has_wm = state.wm_state.lock().map_or(false, |wm| wm.client_id.is_some());
                if has_wm {
                    // Sync mapped state to shared so other connections see it
                    state.sync_windows();
                    continue;
                }

                let unmapped: Vec<u32> = state.windows.iter()
                    .filter(|(_, w)| {
                        !w.mapped
                        && w.parent == state.root_window
                        && w.class == 1 // InputOutput
                        && !w.override_redirect
                        && (w.width > 1 || w.height > 1)
                    })
                    .map(|(id, _)| *id)
                    .collect();

                // Pre-compute atoms before borrowing windows
                let wm_state_atom = state.intern_atom("WM_STATE", false);
                let net_wm_state_atom = state.intern_atom("_NET_WM_STATE", false);
                let focused_atom = state.intern_atom("_NET_WM_STATE_FOCUSED", false);

                for wid in unmapped {
                    let rw = state.root_width;
                    let rh = state.root_height;
                    let seq = state.sequence;
                    let wid_str = state.get_or_create_window_uuid(wid);

                    // Get the window's actual size for ConfigureNotify
                    let (win_x, win_y, win_w, win_h) = if let Some(win) = state.windows.get_mut(&wid) {
                        info!("WM auto-mapping top-level window {wid:#x} {}x{}", win.width, win.height);
                        win.mapped = true;

                        // Fill with background pixel
                        let w = win.width;
                        let h = win.height;
                        let bg = win.background_pixel;
                        win.framebuffer.fill_rect(0, 0, w, h, bg);

                        // Set WM_STATE = NormalState
                        let mut wm_state_data = vec![0u8; 8];
                        wm_state_data[0..4].copy_from_slice(&1u32.to_le_bytes());
                        win.properties.insert(wm_state_atom, PropertyValue {
                            prop_type: wm_state_atom,
                            format: 32,
                            data: wm_state_data,
                        });
                        win.properties.insert(net_wm_state_atom, PropertyValue {
                            prop_type: 4,
                            format: 32,
                            data: focused_atom.to_le_bytes().to_vec(),
                        });

                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            DisplayUpdate::WindowMapped { window_id: wid_str.clone(), is_top_level: true },
                        ));
                        let _ = state.update_tx.send((
                            state.client_id.clone(),
                            DisplayUpdate::WindowConfigured {
                                window_id: wid_str.clone(),
                                x: win.x,
                                y: win.y,
                                width: w,
                                height: h,
                            },
                        ));
                        (win.x, win.y, w, h)
                    } else {
                        continue;
                    };

                    // Send ConfigureNotify with the window's actual size
                    let mut config_event = [0u8; 32];
                    config_event[0] = CONFIGURE_NOTIFY_EVENT;
                    config_event[2..4].copy_from_slice(&seq.to_le_bytes());
                    config_event[4..8].copy_from_slice(&wid.to_le_bytes());
                    config_event[8..12].copy_from_slice(&wid.to_le_bytes());
                    config_event[16..18].copy_from_slice(&win_x.to_le_bytes());
                    config_event[18..20].copy_from_slice(&win_y.to_le_bytes());
                    config_event[20..22].copy_from_slice(&win_w.to_le_bytes());
                    config_event[22..24].copy_from_slice(&win_h.to_le_bytes());
                    stream.write_all(&config_event).await?;

                    let mut map_event = [0u8; 32];
                    map_event[0] = MAP_NOTIFY_EVENT;
                    map_event[2..4].copy_from_slice(&seq.to_le_bytes());
                    map_event[4..8].copy_from_slice(&wid.to_le_bytes());
                    map_event[8..12].copy_from_slice(&wid.to_le_bytes());
                    stream.write_all(&map_event).await?;

                    let mut expose_event = [0u8; 32];
                    expose_event[0] = EXPOSE_EVENT;
                    expose_event[2..4].copy_from_slice(&seq.to_le_bytes());
                    expose_event[4..8].copy_from_slice(&wid.to_le_bytes());
                    expose_event[12..14].copy_from_slice(&rw.to_le_bytes());
                    expose_event[14..16].copy_from_slice(&rh.to_le_bytes());
                    stream.write_all(&expose_event).await?;

                    // Note: PropertyNotify and FocusIn events removed here —
                    // they confused simple apps like xeyes that don't expect
                    // unsolicited events. The MapNotify + Expose are sufficient.
                }

                // Immediately sync mapped state to shared so other connections see it
                state.sync_windows();
            }
            result = input_rx.recv() => {
                if let Ok((window_uuid, input)) = result {
                    // Check if this client owns the target window
                    if state.x11_to_uuid.values().any(|u| u == &window_uuid) {
                        let event_bytes = build_x11_input_event(&mut state, &input);
                        if !event_bytes.is_empty() {
                            stream.write_all(&event_bytes).await?;
                        }
                    }
                }
            }
            result = resize_rx.recv() => {
                if let Ok((window_uuid, width, height)) = result {
                    // Check if this client owns the target window
                    if state.x11_to_uuid.values().any(|u| u == &window_uuid) {
                        let events = resize_window(&mut state, &window_uuid, width, height);
                        if !events.is_empty() {
                            stream.write_all(&events).await?;
                        }
                    }
                }
            }
            // Receive WM events (MapRequest, ConfigureRequest) directed at
            // this client when it is acting as the window manager.
            Some(event_data) = wm_events_rx.recv() => {
                stream.write_all(&event_data).await?;
            }
        }
    }
}

/// Resize a specific window and its children, send ConfigureNotify + Expose events.
fn resize_window(state: &mut ClientState, window_uuid: &str, width: u16, height: u16) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;

    // Look up X11 window ID from UUID
    let window_id = match state.x11_to_uuid.iter().find(|(_, uuid)| uuid.as_str() == window_uuid) {
        Some((&wid, _)) => wid,
        None => return events,
    };

    // Resize the target window and all its descendants
    let mut to_resize = vec![window_id];
    // Also find child windows to resize proportionally
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| is_descendant_of(&state.windows, w.id, window_id))
        .map(|w| w.id)
        .collect();
    to_resize.extend(children);

    for wid in &to_resize {
        if let Some(win) = state.windows.get_mut(wid) {
            if *wid == window_id {
                // Resize the target window to the exact requested size
                win.width = width;
                win.height = height;
            } else {
                // Child windows: resize to match parent (same size for simplicity)
                win.width = width;
                win.height = height;
            }
            win.framebuffer = Framebuffer::new(win.width as u32, win.height as u32);

            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes());
            event[8..12].copy_from_slice(&wid.to_le_bytes());
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&win.width.to_le_bytes());
            event[22..24].copy_from_slice(&win.height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            events.extend_from_slice(&event);

            if win.mapped {
                let mut expose = [0u8; 32];
                expose[0] = EXPOSE_EVENT;
                expose[2..4].copy_from_slice(&seq.to_le_bytes());
                expose[4..8].copy_from_slice(&wid.to_le_bytes());
                expose[12..14].copy_from_slice(&win.width.to_le_bytes());
                expose[14..16].copy_from_slice(&win.height.to_le_bytes());
                events.extend_from_slice(&expose);
            }
        }
    }

    // Send WindowConfigured display update for the top-level window
    if let Some(win) = state.windows.get(&window_id) {
        let owner = if win.owner_client_id.is_empty() {
            state.client_id.clone()
        } else {
            win.owner_client_id.clone()
        };
        let _ = state.update_tx.send((
            owner,
            DisplayUpdate::WindowConfigured {
                window_id: window_uuid.to_string(),
                x: win.x,
                y: win.y,
                width: win.width,
                height: win.height,
            },
        ));
    }

    events
}

/// Resize all mapped windows for this client and send ConfigureNotify + Expose events.
fn resize_all_windows(state: &mut ClientState, width: u16, height: u16) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;

    // Update root window dimensions
    state.root_width = width;
    state.root_height = height;

    // Collect window IDs to resize (avoid borrow issues)
    let window_ids: Vec<u32> = state.windows.keys().copied().collect();

    for wid in window_ids {
        let wid_str = state.window_id_str(wid);
        if let Some(win) = state.windows.get_mut(&wid) {
            win.width = width;
            win.height = height;
            win.framebuffer.resize(width as u32, height as u32);

            // Send ConfigureNotify
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes());
            event[8..12].copy_from_slice(&wid.to_le_bytes());
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&width.to_le_bytes());
            event[22..24].copy_from_slice(&height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            events.extend_from_slice(&event);

            // Send Expose to trigger redraw
            if win.mapped {
                let mut expose = [0u8; 32];
                expose[0] = EXPOSE_EVENT;
                expose[2..4].copy_from_slice(&seq.to_le_bytes());
                expose[4..8].copy_from_slice(&wid.to_le_bytes());
                expose[12..14].copy_from_slice(&width.to_le_bytes());
                expose[14..16].copy_from_slice(&height.to_le_bytes());
                events.extend_from_slice(&expose);

                // Send display update for the resize
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowConfigured {
                        window_id: wid_str.clone(),
                        x: win.x,
                        y: win.y,
                        width,
                        height,
                    },
                ));
            }
        }
    }

    events
}

/// Convert a frontend InputEvent into X11 wire-format event bytes (32 bytes).
fn build_x11_input_event(state: &mut ClientState, input: &InputEvent) -> Vec<u8> {
    // Deliver events to the focus window if set, otherwise find the
    // top-level mapped window owned by this client.
    let target_window = if state.focus_window != 0
        && state.focus_window != state.root_window
        && state.windows.contains_key(&state.focus_window)
    {
        state.focus_window
    } else {
        state
            .windows
            .values()
            .filter(|w| {
                w.mapped
                    && w.parent == state.root_window
                    && w.class == 1
                    && !w.override_redirect
            })
            .max_by_key(|w| w.id)
            .map(|w| w.id)
            .unwrap_or(state.root_window)
    };

    // Update tracked pointer position for QueryPointer
    match input {
        InputEvent::MotionNotify { x, y, .. }
        | InputEvent::ButtonPress { x, y, .. }
        | InputEvent::ButtonRelease { x, y, .. } => {
            state.pointer_x = *x;
            state.pointer_y = *y;
        }
        _ => {}
    }

    let seq = state.sequence;
    let mut event = [0u8; 32];

    // Timestamp in milliseconds since server start (X11 expects monotonic ms)
    static SERVER_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let timestamp = SERVER_START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u32;

    match input {
        InputEvent::MotionNotify { x, y, state: mask } => {
            event[0] = MOTION_NOTIFY_EVENT; // 6
            event[1] = 0; // detail: Normal
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
            event[12..16].copy_from_slice(&target_window.to_le_bytes()); // event window
            event[16..20].copy_from_slice(&target_window.to_le_bytes()); // child
            event[20..22].copy_from_slice(&x.to_le_bytes()); // root_x
            event[22..24].copy_from_slice(&y.to_le_bytes()); // root_y
            event[24..26].copy_from_slice(&x.to_le_bytes()); // event_x
            event[26..28].copy_from_slice(&y.to_le_bytes()); // event_y
            event[28..30].copy_from_slice(&mask.to_le_bytes()); // state
            event[30] = 1; // same_screen
        }
        InputEvent::ButtonPress {
            button,
            x,
            y,
            state: mask,
        } => {
            event[0] = BUTTON_PRESS_EVENT; // 4
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&x.to_le_bytes());
            event[26..28].copy_from_slice(&y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::ButtonRelease {
            button,
            x,
            y,
            state: mask,
        } => {
            event[0] = BUTTON_RELEASE_EVENT; // 5
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&x.to_le_bytes());
            event[26..28].copy_from_slice(&y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyPress {
            keycode,
            state: mask,
        } => {
            event[0] = KEY_PRESS_EVENT; // 2
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyRelease {
            keycode,
            state: mask,
        } => {
            event[0] = KEY_RELEASE_EVENT; // 3
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
    }

    event.to_vec()
}

fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;
    if major_opcode >= 128 {
        debug!("ext op={major_opcode} minor={_minor} seq={seq}");
    }


    match major_opcode {
        1 => handle_create_window(state, data, seq),
        2 => handle_change_window_attributes(state, data),
        3 => handle_get_window_attributes(state, data, seq),
        4 => handle_destroy_window(state, data),
        8 => handle_map_window(state, data, seq),
        9 => handle_map_subwindows(state, data, seq),
        10 => handle_unmap_window(state, data, seq),
        12 => handle_configure_window(state, data, seq),
        14 => handle_get_geometry(state, data, seq),
        15 => handle_query_tree(state, data, seq),
        16 => handle_intern_atom(state, data, seq),
        17 => handle_get_atom_name(state, data, seq),
        18 => handle_change_property(state, data),
        20 => handle_get_property(state, data, seq),
        23 => handle_get_selection_owner(state, data, seq),
        38 => handle_query_pointer(state, data, seq),
        42 => handle_set_input_focus(state, data),
        43 => handle_get_input_focus(state, data, seq),
        47 => handle_query_font(state, data, seq),
        49 => handle_list_fonts(state, data, seq),
        55 => handle_create_gc(state, data),
        56 => handle_change_gc(state, data),
        60 => handle_free_gc(state, data),
        53 => handle_create_pixmap(state, data),
        54 => handle_free_pixmap(state, data),
        61 => handle_clear_area(state, data, seq),
        62 => handle_copy_area(state, data),
        70 => handle_poly_fill_rectangle(state, data),
        65 => handle_poly_line(state, data),
        64 => handle_poly_point(state, data),
        66 => handle_poly_segment(state, data),
        67 => handle_poly_rectangle(state, data),
        68 => handle_poly_arc(state, data),
        69 => handle_fill_poly(state, data),
        71 => handle_poly_fill_arc(state, data),
        72 => handle_put_image(state, data),
        73 => handle_get_image(state, data, seq),
        84 => handle_alloc_color(state, data, seq),
        85 => handle_alloc_named_color(state, data, seq),
        92 => handle_lookup_color(state, data, seq),
        91 => handle_query_colors(state, data, seq),
        97 => {
            // QueryBestSize: reply with the requested width/height
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 12 {
                reply[8..10].copy_from_slice(&data[8..10]); // width
                reply[10..12].copy_from_slice(&data[10..12]); // height
            }
            reply.to_vec()
        }
        98 => handle_query_extension(state, data, seq),
        99 => handle_list_extensions(seq),
        101 => handle_get_keyboard_mapping(data, seq),
        // Requests that need replies - route to handle_misc_request
        26 | // GrabPointer
        31 | // GrabKeyboard
        40 | // TranslateCoordinates
        44 | // QueryKeymap
        48 | // QueryTextExtents
        50 | // ListFontsWithInfo
        52 | // GetFontPath
        103 | // GetKeyboardControl
        116 | // SetPointerMapping
        119   // GetModifierMapping
        => handle_misc_request(state, major_opcode, seq),
        // Font operations
        45 => handle_open_font(state, data),
        46 => handle_close_font(state, data),
        // Drawing operations
        74 => handle_poly_text8(state, data),
        76 => handle_image_text8(state, data),
        22 => handle_set_selection_owner(state, data),
        24 => handle_convert_selection(state, data, seq),
        19 => {
            // DeleteProperty
            if data.len() >= 12 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let property = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                if let Some(win) = state.windows.get_mut(&window) {
                    win.properties.remove(&property);
                }
            }
            Vec::new()
        }
        // Silently ignore these common requests (no reply needed)
        25 | // SendEvent
        27 | // UngrabPointer
        28 | // UngrabButton
        29 | // UngrabButton (alt)
        30 | // ChangeActivePointerGrab
        32 | // UngrabKeyboard
        33 | // GrabKey
        34 | // UngrabKey
        35 | // AllowEvents
        36 | // GrabServer
        37 | // UngrabServer
        41 | // WarpPointer
        51 | // SetFontPath
        57 | // CopyGC
        58 | // SetDashes
        59 | // SetClipRectangles
        75 | // PolyText16
        77 | // ImageText16
        78 | // CreateColormap
        79 | // FreeColormap
        88 | // FreeColors
        93 | // CreateCursor
        94 | // CreateGlyphCursor
        95 | // FreeCursor
        96 | // RecolorCursor
        100 | // ChangeKeyboardMapping
        102 | // ChangeKeyboardControl
        104 | // Bell
        115 | // ForceScreenSaver
        127 // NoOperation
        => { Vec::new() },
        133 => {
            // BIG-REQUESTS: Enable reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&(4194303u32).to_le_bytes()); // maximum-request-length
            reply.to_vec()
        }
        128 => handle_shape_request(state, data, seq),
        130 => handle_shm_request(state, data, seq),
        138 => handle_xfixes_request(state, data, seq),
        134 => handle_sync_request(state, data, seq),
        135 => handle_ge_request(data, seq),
        136 => handle_xkb_request(data, seq),
        139 => crate::render::handle_render_request(state, data, seq),
        140 => handle_randr_request(state, data, seq),
        141 => handle_xc_misc_request(data, seq),
        142 => handle_x_composite_request(state, data, seq),
        143 => handle_damage_request(state, data, seq),
        148 => handle_present_request(state, data, seq),
        _ => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {_minor}");
            Vec::new()
        }
    }
}

/// Build an X11 error reply (32 bytes)
fn build_error(error_code: u8, seq: u16, bad_value: u32, major_opcode: u8, minor_opcode: u16) -> Vec<u8> {
    let mut err = [0u8; 32];
    err[0] = 0; // Error indicator
    err[1] = error_code;
    err[2..4].copy_from_slice(&seq.to_le_bytes());
    err[4..8].copy_from_slice(&bad_value.to_le_bytes());
    err[8..10].copy_from_slice(&minor_opcode.to_le_bytes());
    err[10] = major_opcode;
    err.to_vec()
}

// Handles requests that need stub replies
fn handle_misc_request(state: &mut ClientState, opcode: u8, seq: u16) -> Vec<u8> {
    match opcode {
        26 => {
            // GrabPointer reply: Success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // GrabSuccess status
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        31 => {
            // GrabKeyboard reply: Success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // GrabSuccess status
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        40 => {
            // TranslateCoordinates reply
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 1; // same_screen
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // child
            reply.to_vec()
        }
        44 => {
            // QueryKeymap reply: all zeros (no keys pressed)
            let mut reply = [0u8; 40]; // 32 + 8 bytes of keymap
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&2u32.to_le_bytes()); // length = 2 (8 extra bytes)
            reply.to_vec()
        }
        48 => {
            // QueryTextExtents reply
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // font_ascent = 12, font_descent = 4, overall_width = 0
            reply[8..10].copy_from_slice(&12i16.to_le_bytes());
            reply[10..12].copy_from_slice(&4i16.to_le_bytes());
            reply.to_vec()
        }
        50 => {
            // ListFontsWithInfo reply: terminate with empty reply (name_length=0)
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // last-reply indicator (name_length = 0)
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply[4..8] = 7 (length of additional data = 7 * 4 = 28 bytes for min/max bounds)
            reply[4..8].copy_from_slice(&7u32.to_le_bytes());
            let mut full_reply = reply.to_vec();
            full_reply.resize(32 + 28, 0); // 28 bytes of padding for the min/max bounds
            full_reply
        }
        52 => {
            // GetFontPath reply: empty list
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        // 84 (AllocColor) and 91 (QueryColors) handled in handle_request
        // 101 (GetKeyboardMapping) handled in handle_request
        103 => {
            // GetKeyboardControl reply
            let mut reply = [0u8; 52]; // 32 + 20 extra
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&5u32.to_le_bytes()); // length = 5 (20 extra bytes)
            reply.to_vec()
        }
        116 => {
            // SetPointerMapping reply: Success
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 0; // MappingSuccess
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        119 => {
            // GetModifierMapping reply
            let keycodes_per_modifier: u8 = 2;
            let data_len = 8 * keycodes_per_modifier as u32; // 8 modifiers
            let reply_len = 32 + data_len as usize;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1;
            reply[1] = keycodes_per_modifier;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((data_len / 4).to_le_bytes()));
            reply
        }
        _ => Vec::new(),
    }
}

fn handle_create_window(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let parent = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let width = u16::from_le_bytes([data[16], data[17]]);
    let height = u16::from_le_bytes([data[18], data[19]]);
    let border_width = u16::from_le_bytes([data[20], data[21]]);
    let class = u16::from_le_bytes([data[22], data[23]]);
    let visual = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let value_mask = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

    let mut background_pixel = 0u32;
    let mut event_mask = 0u32;
    let mut override_redirect = false;

    // Parse value list
    let mut offset = 32;
    for bit in 0..15 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => {} // background-pixmap
                    1 => background_pixel = val,
                    2 => {} // border-pixmap
                    3 => {} // border-pixel
                    4 => {} // bit-gravity
                    5 => {} // win-gravity
                    6 => {} // backing-store
                    7 => {} // backing-planes
                    8 => {} // backing-pixel
                    9 => override_redirect = val != 0,
                    10 => {} // save-under
                    11 => event_mask = val,
                    12 => {} // do-not-propagate-mask
                    13 => {} // colormap
                    14 => {} // cursor
                    _ => {}
                }
                offset += 4;
            }
        }
    }

    let use_visual = if visual == 0 { ROOT_VISUAL } else { visual };

    info!("CreateWindow: id={wid:#x} parent={parent:#x} {x},{y} {width}x{height} depth={} class={class} visual={visual:#x} bg={background_pixel:#x}", data[1]);

    state.windows.insert(
        wid,
        WindowState {
            id: wid,
            parent,
            x,
            y,
            width,
            height,
            border_width,
            visual: use_visual,
            class,
            mapped: false,
            event_mask,
            background_pixel,
            override_redirect,
            redirected: false,
            framebuffer: Framebuffer::new(width as u32, height as u32),
            properties: HashMap::new(),
            owner_client_id: state.client_id.clone(),
        },
    );

    // Set _NET_FRAME_EXTENTS = (0,0,0,0) on new windows — GTK3 checks this.
    let atom_frame = state.intern_atom("_NET_FRAME_EXTENTS", false);
    if let Some(win) = state.windows.get_mut(&wid) {
        win.properties.insert(atom_frame, PropertyValue {
            prop_type: 6, // CARDINAL
            format: 32,
            data: vec![0; 16], // left, right, top, bottom = 0
        });
    }

    let is_top_level = parent == state.root_window && class == 1 && !override_redirect;
    let wid_str = if is_top_level {
        state.get_or_create_window_uuid(wid)
    } else {
        wid.to_string()
    };
    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowCreated {
            window_id: wid_str,
            x,
            y,
            width,
            height,
            is_top_level,
        },
    ));

    Vec::new() // No reply for CreateWindow
}

fn handle_change_window_attributes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    if let Some(win) = state.windows.get_mut(&wid) {
        let mut offset = 12;
        for bit in 0..15 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        1 => win.background_pixel = val,
                        11 => {
                            win.event_mask = val;
                            // SubstructureRedirectMask = bit 20 = 0x0010_0000
                            const SUBSTRUCTURE_REDIRECT_MASK: u32 = 0x0010_0000;
                            if wid == state.root_window && (val & SUBSTRUCTURE_REDIRECT_MASK) != 0 {
                                info!(
                                    "Client {} registering as window manager (SubstructureRedirectMask on root)",
                                    state.client_id
                                );
                                if let Ok(mut wm) = state.wm_state.lock() {
                                    wm.client_id = Some(state.client_id.clone());
                                    wm.event_tx = Some(state.wm_events_tx.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }
    }

    Vec::new()
}

fn handle_get_window_attributes(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let win = match state.windows.get(&wid) {
        Some(w) => w,
        None => return build_error(3, seq, wid, 3, 0), // BadWindow
    };

    let mut reply = vec![0u8; 44];
    reply[0] = 1; // Reply
    reply[1] = 0; // backing-store: NotUseful
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&3u32.to_le_bytes()); // length = 3 extra u32s
    reply[8..12].copy_from_slice(&win.visual.to_le_bytes());     // visual (4 bytes)
    reply[12..14].copy_from_slice(&win.class.to_le_bytes());     // class (2 bytes)
    reply[14] = 0; // bit_gravity
    reply[15] = 0; // win_gravity
    reply[16..20].copy_from_slice(&0u32.to_le_bytes()); // backing_planes
    reply[20..24].copy_from_slice(&0u32.to_le_bytes()); // backing_pixel
    reply[24] = 0; // save_under = false
    reply[25] = 1; // map_is_installed = true
    reply[26] = if win.mapped { 2 } else { 0 }; // map_state: Viewable or Unmapped
    reply[27] = if win.override_redirect { 1 } else { 0 };
    reply[28..32].copy_from_slice(&ROOT_COLORMAP.to_le_bytes()); // colormap
    reply[32..36].copy_from_slice(&win.event_mask.to_le_bytes()); // all_event_masks
    reply[36..40].copy_from_slice(&0u32.to_le_bytes()); // your_event_mask
    reply[40..42].copy_from_slice(&0u16.to_le_bytes()); // do_not_propagate_mask
    // bytes 42-43: unused padding

    reply
}

fn handle_destroy_window(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.windows.remove(&wid);
    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowDestroyed { window_id: state.window_id_str(wid) },
    ));
    Vec::new()
}

fn handle_map_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    info!("MapWindow called: wid={wid:#x} exists={}", state.windows.contains_key(&wid));

    let mut events = Vec::new();

    if !state.windows.contains_key(&wid) {
        warn!("MapWindow: id={wid:#x} NOT FOUND in client {}", state.client_id);
        return events;
    }

    // Check if this is a top-level window (parent == root) and a WM is active.
    // If so, redirect as a MapRequest event to the WM instead of mapping directly.
    // override_redirect windows bypass the WM redirect.
    let is_top_level = state.windows.get(&wid).map_or(false, |w| w.parent == state.root_window);
    let is_override_redirect = state.windows.get(&wid).map_or(false, |w| w.override_redirect);

    if is_top_level && !is_override_redirect {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                // Only redirect if the WM is a *different* client
                wm.client_id.as_ref().map_or(false, |id| id != &state.client_id)
            } else {
                false
            }
        };

        if should_redirect {
            info!(
                "MapWindow: redirecting wid={wid:#x} as MapRequest to WM"
            );
            // Build MapRequest event (code 20)
            let mut map_request = [0u8; 32];
            map_request[0] = MAP_REQUEST_EVENT;
            // map_request[1] = 0; // unused
            // sequence number will be the WM's – but we use 0 since the server
            // inserts events asynchronously.
            map_request[4..8].copy_from_slice(&state.root_window.to_le_bytes()); // parent
            map_request[8..12].copy_from_slice(&wid.to_le_bytes()); // window

            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(map_request.to_vec());
                }
            }
            // Don't map the window – the WM will do it.
            return events;
        }
    }

    let wid_str = state.window_id_str(wid);
    if let Some(win) = state.windows.get_mut(&wid) {
        info!("MapWindow: id={wid:#x} {}x{} mapped={}", win.width, win.height, win.mapped);
        let is_top_level = win.parent == state.root_window && win.class == 1 && !win.override_redirect;
        win.mapped = true;
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowMapped { window_id: wid_str.clone(), is_top_level },
        ));

        // Fill window with its background pixel (like a real X server does)
        let w = win.width;
        let h = win.height;
        let bg = win.background_pixel;
        win.framebuffer.fill_rect(0, 0, w, h, bg);

        // Send MapNotify event
        let mut map_event = [0u8; 32];
        map_event[0] = MAP_NOTIFY_EVENT;
        map_event[2..4].copy_from_slice(&seq.to_le_bytes());
        map_event[4..8].copy_from_slice(&wid.to_le_bytes()); // event window
        map_event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
        map_event[12] = if win.override_redirect { 1 } else { 0 };
        events.extend_from_slice(&map_event);

        // Send Expose event
        let width = win.width;
        let height = win.height;
        let mut expose_event = [0u8; 32];
        expose_event[0] = EXPOSE_EVENT;
        expose_event[2..4].copy_from_slice(&seq.to_le_bytes());
        expose_event[4..8].copy_from_slice(&wid.to_le_bytes());
        // x=0, y=0 already zero
        expose_event[12..14].copy_from_slice(&width.to_le_bytes());
        expose_event[14..16].copy_from_slice(&height.to_le_bytes());
        // count = 0
        events.extend_from_slice(&expose_event);

        // Also send Expose to all mapped descendant windows. In a real X
        // server, when a parent becomes visible, all its visible children
        // get Expose events so widgets can redraw (text, buttons, etc.).
        let descendants: Vec<(u32, u16, u16)> = state
            .windows
            .values()
            .filter(|w| w.mapped && w.id != wid && is_descendant_of(&state.windows, w.id, wid))
            .map(|w| (w.id, w.width, w.height))
            .collect();

        if !descendants.is_empty() {
        }

        for (desc_id, dw, dh) in descendants {
            let mut exp = [0u8; 32];
            exp[0] = EXPOSE_EVENT;
            exp[2..4].copy_from_slice(&seq.to_le_bytes());
            exp[4..8].copy_from_slice(&desc_id.to_le_bytes());
            exp[12..14].copy_from_slice(&dw.to_le_bytes());
            exp[14..16].copy_from_slice(&dh.to_le_bytes());
            events.extend_from_slice(&exp);
        }
    }

    events
}

/// Check if window `child` is a descendant of window `ancestor`.
fn is_descendant_of(windows: &HashMap<u32, WindowState>, child: u32, ancestor: u32) -> bool {
    let mut current = child;
    for _ in 0..20 {
        let parent = match windows.get(&current) {
            Some(w) => w.parent,
            None => return false,
        };
        if parent == ancestor {
            return true;
        }
        if parent == 0 {
            return false;
        }
        current = parent;
    }
    false
}

fn handle_map_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let parent = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Collect child window IDs first to avoid borrow issues
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && !w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        // Construct a fake MapWindow request for each child
        let mut fake_data = [0u8; 8];
        fake_data[0] = 8; // MapWindow opcode
        fake_data[2..4].copy_from_slice(&2u16.to_le_bytes()); // length = 2
        fake_data[4..8].copy_from_slice(&child_id.to_le_bytes());
        let events = handle_map_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}

fn handle_unmap_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut events = Vec::new();

    if let Some(win) = state.windows.get_mut(&wid) {
        win.mapped = false;
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowUnmapped { window_id: state.window_id_str(wid) },
        ));

        let mut event = [0u8; 32];
        event[0] = UNMAP_NOTIFY_EVENT;
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&wid.to_le_bytes());
        event[8..12].copy_from_slice(&wid.to_le_bytes());
        events.extend_from_slice(&event);
    }

    events
}

fn handle_configure_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u16::from_le_bytes([data[8], data[9]]);

    // Check if this is a top-level window that should be redirected to the WM.
    let is_top_level = state.windows.get(&wid).map_or(false, |w| w.parent == state.root_window);
    let is_override_redirect = state.windows.get(&wid).map_or(false, |w| w.override_redirect);

    if is_top_level && !is_override_redirect {
        let should_redirect = {
            if let Ok(wm) = state.wm_state.lock() {
                wm.client_id.as_ref().map_or(false, |id| id != &state.client_id)
            } else {
                false
            }
        };

        if should_redirect {
            info!("ConfigureWindow: redirecting wid={wid:#x} as ConfigureRequest to WM");

            // Parse the values from the request to populate the ConfigureRequest event.
            let mut x: i16 = 0;
            let mut y: i16 = 0;
            let mut width: u16 = 0;
            let mut height: u16 = 0;
            let mut border_width: u16 = 0;
            let mut sibling: u32 = 0;
            let mut stack_mode: u8 = 0;

            // Pre-fill with current values from the window
            if let Some(win) = state.windows.get(&wid) {
                x = win.x;
                y = win.y;
                width = win.width;
                height = win.height;
                border_width = win.border_width;
            }

            let mut offset = 12;
            for bit in 0..7u16 {
                if value_mask & (1 << bit) != 0 {
                    if offset + 4 <= data.len() {
                        let val = u32::from_le_bytes([
                            data[offset], data[offset + 1],
                            data[offset + 2], data[offset + 3],
                        ]);
                        match bit {
                            0 => x = val as i16,
                            1 => y = val as i16,
                            2 => width = val as u16,
                            3 => height = val as u16,
                            4 => border_width = val as u16,
                            5 => sibling = val,
                            6 => stack_mode = val as u8,
                            _ => {}
                        }
                        offset += 4;
                    }
                }
            }

            // Build ConfigureRequest event (code 23)
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_REQUEST_EVENT;
            event[1] = stack_mode; // detail = stack-mode
            // sequence = 0 (asynchronous server event)
            event[4..8].copy_from_slice(&state.root_window.to_le_bytes()); // parent
            event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
            event[12..16].copy_from_slice(&sibling.to_le_bytes()); // sibling
            event[16..18].copy_from_slice(&x.to_le_bytes());
            event[18..20].copy_from_slice(&y.to_le_bytes());
            event[20..22].copy_from_slice(&width.to_le_bytes());
            event[22..24].copy_from_slice(&height.to_le_bytes());
            event[24..26].copy_from_slice(&border_width.to_le_bytes());
            event[26..28].copy_from_slice(&value_mask.to_le_bytes());

            if let Ok(wm) = state.wm_state.lock() {
                if let Some(tx) = &wm.event_tx {
                    let _ = tx.send(event.to_vec());
                }
            }
            return Vec::new();
        }
    }

    let mut offset = 12;
    let mut changed = false;
    let wid_str = state.window_id_str(wid);

    if let Some(win) = state.windows.get_mut(&wid) {
        for bit in 0..7 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        0 => {
                            win.x = val as i16;
                            changed = true;
                        }
                        1 => {
                            win.y = val as i16;
                            changed = true;
                        }
                        2 => {
                            win.width = val as u16;
                            changed = true;
                        }
                        3 => {
                            win.height = val as u16;
                            changed = true;
                        }
                        4 => {
                            win.border_width = val as u16;
                        }
                        5 => {} // sibling
                        6 => {} // stack-mode
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }

        if changed {
            // Resize the framebuffer if the window dimensions changed
            let new_w = win.width as u32;
            let new_h = win.height as u32;
            if new_w != win.framebuffer.width() || new_h != win.framebuffer.height() {
                win.framebuffer = Framebuffer::new(new_w, new_h);
            }

            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowConfigured {
                    window_id: wid_str.clone(),
                    x: win.x,
                    y: win.y,
                    width: win.width,
                    height: win.height,
                },
            ));

            // Send ConfigureNotify
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes()); // event
            event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
                                                              // above_sibling = 0
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&win.width.to_le_bytes());
            event[22..24].copy_from_slice(&win.height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            event[26] = if win.override_redirect { 1 } else { 0 };
            return event.to_vec();
        }
    }

    Vec::new()
}

fn handle_get_geometry(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Check windows first, then pixmaps
    if let Some(win) = state.windows.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = 24; // depth
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[12..14].copy_from_slice(&win.x.to_le_bytes());
        reply[14..16].copy_from_slice(&win.y.to_le_bytes());
        reply[16..18].copy_from_slice(&win.width.to_le_bytes());
        reply[18..20].copy_from_slice(&win.height.to_le_bytes());
        reply[20..22].copy_from_slice(&win.border_width.to_le_bytes());
        return reply.to_vec();
    }

    if let Some(pixmap) = state.pixmaps.get(&drawable) {
        let mut reply = [0u8; 32];
        reply[0] = 1; // Reply
        reply[1] = 24; // depth
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[16..18].copy_from_slice(&pixmap._width.to_le_bytes());
        reply[18..20].copy_from_slice(&pixmap._height.to_le_bytes());
        return reply.to_vec();
    }

    // Drawable not found - return BadDrawable error (error code 9)
    build_error(9, seq, drawable, 14, 0)
}

fn handle_query_tree(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    if !state.windows.contains_key(&wid) {
        return build_error(3, seq, wid, 15, 0); // BadWindow
    }

    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == wid)
        .map(|w| w.id)
        .collect();

    let n_children = children.len() as u16;
    let reply_len = 32 + children.len() * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(children.len() as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    reply[12..16].copy_from_slice(&parent.to_le_bytes());
    reply[16..18].copy_from_slice(&n_children.to_le_bytes());

    for (i, &child) in children.iter().enumerate() {
        let off = 32 + i * 4;
        reply[off..off + 4].copy_from_slice(&child.to_le_bytes());
    }

    reply
}

fn handle_intern_atom(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let only_if_exists = data[1] != 0;
    let name_len = u16::from_le_bytes([data[4], data[5]]) as usize;

    let name = if 8 + name_len <= data.len() {
        String::from_utf8_lossy(&data[8..8 + name_len]).to_string()
    } else {
        String::new()
    };

    let atom = state.intern_atom(&name, only_if_exists);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&atom.to_le_bytes());

    reply.to_vec()
}

fn handle_get_atom_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let atom = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let name = state.get_atom_name(atom).unwrap_or_default();
    let name_bytes = name.as_bytes();
    let padded_len = (name_bytes.len() + 3) & !3;

    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded_len / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    reply[32..32 + name_bytes.len()].copy_from_slice(name_bytes);

    reply
}

fn handle_change_property(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // ChangeProperty: [opcode(1), mode(1), length(2), window(4), property(4), type(4),
    //                   format(1), pad(3), data_len(4), data...]
    if data.len() < 24 {
        return Vec::new();
    }

    let _mode = data[1]; // 0=Replace, 1=Prepend, 2=Append
    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let property_atom = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let prop_type = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let format = data[16];
    let data_len = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    // Calculate actual byte length based on format
    let byte_len = match format {
        8 => data_len,
        16 => data_len * 2,
        32 => data_len * 4,
        _ => data_len,
    };

    // Store the property value
    if data.len() >= 24 + byte_len {
        let prop_data = data[24..24 + byte_len].to_vec();
        if let Some(win) = state.windows.get_mut(&window) {
            win.properties.insert(property_atom, PropertyValue {
                prop_type,
                format,
                data: prop_data,
            });
        }
    }

    // Check if this is WM_NAME (atom 39) or _NET_WM_NAME
    let is_wm_name = property_atom == 39
        || state
            .get_atom_name(property_atom)
            .map(|n| n == "_NET_WM_NAME" || n == "WM_NAME")
            .unwrap_or(false);

    if is_wm_name && format == 8 && data.len() >= 24 + byte_len {
        let title = String::from_utf8_lossy(&data[24..24 + byte_len]).to_string();
        if !title.is_empty() {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::TitleChanged {
                    window_id: state.window_id_str(window),
                    title,
                },
            ));
        }
    }

    Vec::new()
}

fn handle_get_property(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // GetProperty: [opcode(1), delete(1), length(2), window(4), property(4),
    //               type(4), long_offset(4), long_length(4)]
    if data.len() < 24 {
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        return reply.to_vec();
    }

    let delete = data[1] != 0;
    let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let property_atom = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _req_type = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let long_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let long_length = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    let prop = state.windows.get(&window).and_then(|w| w.properties.get(&property_atom)).cloned();

    if let Some(prop_val) = prop {
        let byte_offset = long_offset * 4;
        let max_bytes = long_length * 4;
        let total_bytes = prop_val.data.len();
        let available = if byte_offset >= total_bytes { 0 } else { total_bytes - byte_offset };
        let return_bytes = available.min(max_bytes);
        let bytes_after = if available > return_bytes { available - return_bytes } else { 0 };

        let return_data = if byte_offset < total_bytes {
            &prop_val.data[byte_offset..byte_offset + return_bytes]
        } else {
            &[]
        };

        // value_length is in units of format size
        let value_length = match prop_val.format {
            8 => return_data.len() as u32,
            16 => (return_data.len() / 2) as u32,
            32 => (return_data.len() / 4) as u32,
            _ => return_data.len() as u32,
        };

        let padded_len = (return_data.len() + 3) & !3;
        let extra_words = padded_len / 4;
        let total_reply = 32 + padded_len;

        let mut reply = vec![0u8; total_reply];
        reply[0] = 1; // Reply
        reply[1] = prop_val.format;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        reply[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes()); // length
        reply[8..12].copy_from_slice(&prop_val.prop_type.to_le_bytes()); // type
        reply[12..16].copy_from_slice(&(bytes_after as u32).to_le_bytes()); // bytes_after
        reply[16..20].copy_from_slice(&value_length.to_le_bytes()); // value_length
        reply[32..32 + return_data.len()].copy_from_slice(return_data);

        // Delete property if requested and we returned all of it
        if delete && bytes_after == 0 {
            if let Some(win) = state.windows.get_mut(&window) {
                win.properties.remove(&property_atom);
            }
        }

        reply
    } else {
        // Property not found
        let mut reply = [0u8; 32];
        reply[0] = 1;
        reply[2..4].copy_from_slice(&seq.to_le_bytes());
        // type = 0 (None), format = 0, bytes_after = 0, value_length = 0
        reply.to_vec()
    }
}

fn handle_get_selection_owner(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    if data.len() >= 8 {
        let selection = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        // Special-case _NET_WM_CM_S0: always return root window to indicate
        // that we are a compositing window manager.
        let owner = if state.get_atom_name(selection).as_deref() == Some("_NET_WM_CM_S0") {
            state.root_window
        } else {
            state.selections.get(&selection).copied().unwrap_or(0)
        };
        reply[8..12].copy_from_slice(&owner.to_le_bytes());
    }
    reply.to_vec()
}

fn handle_set_selection_owner(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // SetSelectionOwner: [opcode(1), pad(1), length(2), owner(4), selection(4), timestamp(4)]
    if data.len() >= 12 {
        let owner = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let selection = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if owner == 0 {
            state.selections.remove(&selection);
        } else {
            state.selections.insert(selection, owner);
        }
    }
    Vec::new()
}

fn handle_convert_selection(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // ConvertSelection: [opcode(1), pad(1), length(2), requestor(4), selection(4), target(4), property(4), timestamp(4)]
    if data.len() >= 24 {
        let requestor = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let selection = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let target = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        // Send SelectionNotify event back with property=None to indicate no data
        let mut event = [0u8; 32];
        event[0] = 31; // SelectionNotify
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&0u32.to_le_bytes()); // timestamp
        event[8..12].copy_from_slice(&requestor.to_le_bytes()); // requestor
        event[12..16].copy_from_slice(&selection.to_le_bytes()); // selection
        event[16..20].copy_from_slice(&target.to_le_bytes()); // target
        event[20..24].copy_from_slice(&0u32.to_le_bytes()); // property = None
        return event.to_vec();
    }
    Vec::new()
}

fn handle_query_pointer(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // same_screen
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
                                                                    // child = 0
    reply[16..18].copy_from_slice(&state.pointer_x.to_le_bytes()); // root_x
    reply[18..20].copy_from_slice(&state.pointer_y.to_le_bytes()); // root_y
    reply[20..22].copy_from_slice(&state.pointer_x.to_le_bytes()); // win_x
    reply[22..24].copy_from_slice(&state.pointer_y.to_le_bytes()); // win_y
                                                                   // mask = 0
    reply.to_vec()
}

fn handle_set_input_focus(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let focus = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        // 0 = None, 1 = PointerRoot — keep as-is; otherwise store the window
        state.focus_window = focus;
    }
    Vec::new()
}

fn handle_get_input_focus(state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // revert_to = Parent
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.focus_window.to_le_bytes());
    reply.to_vec()
}

fn handle_open_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // OpenFont: [opcode(1), unused(1), length(2), fid(4), name_len(2), pad(2), name...]
    if data.len() < 12 {
        return Vec::new();
    }
    let fid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        String::from_utf8_lossy(&data[12..12 + name_len]).to_string()
    } else {
        "fixed".to_string()
    };
    debug!("OpenFont: fid={fid:#x} name={name}");
    state.font_manager.open_font(fid, &name);
    Vec::new()
}

fn handle_close_font(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let fid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.font_manager.close_font(fid);
    Vec::new()
}

fn handle_query_font(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let fontable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // fontable can be a font ID or a GC ID (containing a font)
    let font = state
        .font_manager
        .get_font(fontable)
        .or_else(|| {
            let gc = state.gcs.get(&fontable)?;
            state.font_manager.get_font(gc.font_id)
        })
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => {
            // No font available — return minimal stub
            let mut reply = vec![0u8; 60];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&7u32.to_le_bytes());
            reply[40..42].copy_from_slice(&32u16.to_le_bytes());
            reply[42..44].copy_from_slice(&126u16.to_le_bytes());
            reply[44..46].copy_from_slice(&32u16.to_le_bytes());
            reply[48] = 0;
            reply[52..54].copy_from_slice(&10i16.to_le_bytes());
            reply[54..56].copy_from_slice(&3i16.to_le_bytes());
            return reply;
        }
    };

    let n_char_infos = (font.max_char - font.min_char + 1) as u32;
    let char_infos_bytes = n_char_infos as usize * 12;

    // QueryFont reply: 60-byte fixed header + n_char_infos * 12 bytes
    // The "60 bytes" includes 32 header + 28 bytes of font info
    // But actual X11 format: 32 bytes reply header, then inline data
    // Reply format: [1, unused, seq(2), length(4),
    //   min_bounds(12), pad(4), max_bounds(12), pad(4),
    //   min_char(2), max_char(2), default_char(2), n_properties(2),
    //   draw_direction(1), min_byte1(1), max_byte1(1), all_chars_exist(1),
    //   font_ascent(2), font_descent(2), n_char_infos(4),
    //   properties..., char_infos...]
    let reply_len = 60 + char_infos_bytes;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    let extra_words = ((reply_len - 32) / 4) as u32;
    reply[4..8].copy_from_slice(&extra_words.to_le_bytes());

    // min_bounds at offset 8 (12 bytes)
    {
        let ci = &font.min_bounds;
        reply[8..10].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
        reply[10..12].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
        reply[12..14].copy_from_slice(&ci.character_width.to_le_bytes());
        reply[14..16].copy_from_slice(&ci.ascent.to_le_bytes());
        reply[16..18].copy_from_slice(&ci.descent.to_le_bytes());
        reply[18..20].copy_from_slice(&ci.attributes.to_le_bytes());
    }
    // pad at 20..24

    // max_bounds at offset 24 (12 bytes)
    {
        let ci = &font.max_bounds;
        reply[24..26].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
        reply[26..28].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
        reply[28..30].copy_from_slice(&ci.character_width.to_le_bytes());
        reply[30..32].copy_from_slice(&ci.ascent.to_le_bytes());
        reply[32..34].copy_from_slice(&ci.descent.to_le_bytes());
        reply[34..36].copy_from_slice(&ci.attributes.to_le_bytes());
    }
    // pad at 36..40

    reply[40..42].copy_from_slice(&font.min_char.to_le_bytes());
    reply[42..44].copy_from_slice(&font.max_char.to_le_bytes());
    reply[44..46].copy_from_slice(&font.default_char.to_le_bytes());
    reply[46..48].copy_from_slice(&0u16.to_le_bytes()); // n_properties = 0
    reply[48] = 0; // draw_direction = LeftToRight
    reply[49] = 0; // min_byte1
    reply[50] = 0; // max_byte1
    reply[51] = if font.char_infos.len() == n_char_infos as usize {
        1
    } else {
        0
    }; // all_chars_exist
    reply[52..54].copy_from_slice(&font.font_ascent.to_le_bytes());
    reply[54..56].copy_from_slice(&font.font_descent.to_le_bytes());
    reply[56..60].copy_from_slice(&n_char_infos.to_le_bytes());

    // Char infos at offset 60
    let mut off = 60;
    for ci in &font.char_infos {
        if off + 12 <= reply.len() {
            reply[off..off + 2].copy_from_slice(&ci.left_side_bearing.to_le_bytes());
            reply[off + 2..off + 4].copy_from_slice(&ci.right_side_bearing.to_le_bytes());
            reply[off + 4..off + 6].copy_from_slice(&ci.character_width.to_le_bytes());
            reply[off + 6..off + 8].copy_from_slice(&ci.ascent.to_le_bytes());
            reply[off + 8..off + 10].copy_from_slice(&ci.descent.to_le_bytes());
            reply[off + 10..off + 12].copy_from_slice(&ci.attributes.to_le_bytes());
            off += 12;
        }
    }

    reply
}

fn handle_list_fonts(_state: &mut ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return a single font: "fixed"
    let font_name = b"fixed";
    let str_len = 1 + font_name.len(); // length byte + name
    let padded = (str_len + 3) & !3;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // number of names
    reply[32] = font_name.len() as u8;
    reply[33..33 + font_name.len()].copy_from_slice(font_name);

    reply
}

fn handle_create_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let mut gc = GcState::default();
    parse_gc_values(&mut gc, value_mask, &data[16..]);
    state.gcs.insert(gc_id, gc);

    Vec::new()
}

fn handle_change_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        parse_gc_values(gc, value_mask, &data[12..]);
    }

    Vec::new()
}

fn parse_gc_values(gc: &mut GcState, value_mask: u32, data: &[u8]) {
    let mut offset = 0;
    for bit in 0..23 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => gc.function = val as u8,
                    2 => gc.foreground = val,
                    3 => gc.background = val,
                    5 => gc.line_width = val as u16,
                    14 => gc.font_id = val, // font
                    _ => {}
                }
                offset += 4;
            }
        }
    }
}

fn handle_free_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.gcs.remove(&gc_id);
    Vec::new()
}

fn handle_create_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let depth = data[1];
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    info!("CreatePixmap: pid={pid:#x} {}x{} depth={depth}", width, height);

    state.pixmaps.insert(
        pid,
        PixmapState {
            _id: pid,
            _width: width,
            _height: height,
            _depth: depth,
            framebuffer: Framebuffer::new(width as u32, height as u32),
            alias_window: None,
            shm_backing: None,
        },
    );

    Vec::new()
}

fn handle_free_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.pixmaps.remove(&pid);
    Vec::new()
}

fn handle_clear_area(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let x = i16::from_le_bytes([data[8], data[9]]);
    let y = i16::from_le_bytes([data[10], data[11]]);
    let mut width = u16::from_le_bytes([data[12], data[13]]);
    let mut height = u16::from_le_bytes([data[14], data[15]]);

    // If width or height is 0, use the window's dimensions
    // ClearArea fills with the window's background pixel
    let bg = state.windows.get(&wid).map(|w| {
        if width == 0 {
            width = w.width;
        }
        if height == 0 {
            height = w.height;
        }
        w.background_pixel
    });

    let bg_pixel = bg.unwrap_or(0);
    if let Some(fb) = state.get_framebuffer_mut(wid) {
        fb.fill_rect(x, y, width, height, bg_pixel);
    }

    Vec::new()
}

fn handle_copy_area(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 28 {
        return Vec::new();
    }

    let src = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let src_x = i16::from_le_bytes([data[16], data[17]]);
    let src_y = i16::from_le_bytes([data[18], data[19]]);
    let dst_x = i16::from_le_bytes([data[20], data[21]]);
    let dst_y = i16::from_le_bytes([data[22], data[23]]);
    let width = u16::from_le_bytes([data[24], data[25]]);
    let height = u16::from_le_bytes([data[26], data[27]]);

    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    // Sync SHM-backed pixmap data before reading from src
    state.sync_shm_pixmap(src);

    // Check if source is a 1-bit depth pixmap (used for clip masks)
    let src_depth = state.pixmaps.get(&src).map(|p| p._depth).unwrap_or(24);

    if src == dst {
        if let Some(fb) = state.get_framebuffer_mut(src) {
            fb.copy_area_self(src_x, src_y, dst_x, dst_y, width, height);
        }
    } else {
        let pixels = state
            .get_framebuffer_mut(src)
            .map(|fb| fb.extract_pixels(src_x, src_y, width, height));
        if let Some(pixels) = pixels {
            if src_depth <= 1 && gc.function != 3 {
                // 1-bit source with non-copy GC function: map pixel values
                // to foreground/background colors using the GC function.
                // Pixel != black → foreground, pixel == black → background
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    let fb_w = fb.width() as i32;
                    let fb_h = fb.height() as i32;
                    let src_stride = width as usize * 4;
                    for row in 0..height as usize {
                        let dy = dst_y as i32 + row as i32;
                        if dy < 0 || dy >= fb_h { continue; }
                        for col in 0..width as usize {
                            let dx = dst_x as i32 + col as i32;
                            if dx < 0 || dx >= fb_w { continue; }
                            let src_off = row * src_stride + col * 4;
                            if src_off + 3 >= pixels.len() { continue; }
                            let src_pixel = pixels[src_off] as u32
                                | (pixels[src_off + 1] as u32) << 8
                                | (pixels[src_off + 2] as u32) << 16;
                            // If any bit set → use foreground; else background
                            let color = if src_pixel != 0 {
                                gc.foreground
                            } else {
                                gc.background
                            };
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else if gc.function != 3 {
                // Non-copy GC function with regular depth source
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    let src_stride = width as usize * 4;
                    for row in 0..height as usize {
                        let dy = dst_y as i32 + row as i32;
                        for col in 0..width as usize {
                            let dx = dst_x as i32 + col as i32;
                            let src_off = row * src_stride + col * 4;
                            if src_off + 3 >= pixels.len() { continue; }
                            let color = (pixels[src_off + 2] as u32) << 16
                                | (pixels[src_off + 1] as u32) << 8
                                | pixels[src_off] as u32;
                            fb.draw_point_with_func(dx, dy, color, gc.function);
                        }
                    }
                }
            } else {
                // GXcopy — fast path
                if let Some(fb) = state.get_framebuffer_mut(dst) {
                    fb.put_image(dst_x, dst_y, width, height, &pixels);
                }
            }
        }
    }

    Vec::new()
}

fn handle_poly_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        rects.push((x, y, width, height));
        offset += 8;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height) in rects {
            let x2 = x as i32 + width as i32;
            let y2 = y as i32 + height as i32;
            fb.draw_line(x as i32, y as i32, x2, y as i32, gc.foreground, gc.line_width);
            fb.draw_line(x2, y as i32, x2, y2, gc.foreground, gc.line_width);
            fb.draw_line(x2, y2, x as i32, y2, gc.foreground, gc.line_width);
            fb.draw_line(x as i32, y2, x as i32, y as i32, gc.foreground, gc.line_width);
        }
    }

    Vec::new()
}

fn handle_fill_poly(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();
    let coord_mode = data[13]; // 0 = Origin, 1 = Previous


    let mut points = Vec::new();
    let mut offset = 16;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py): (i16, i16) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    if points.len() >= 3 {
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            fb.fill_polygon(&points, gc.foreground);
        }
    }

    Vec::new()
}

fn handle_poly_fill_rectangle(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let mut rects = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        rects.push((x, y, width, height));
        offset += 8;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    info!("PolyFillRect: draw={drawable:#x} fg={fg:#x} gc={gc_id:#x} rects={}", rects.len());
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for &(x, y, width, height) in &rects {
            fb.fill_rect(x, y, width, height, fg);
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height) in &rects {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}

fn handle_poly_line(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let coord_mode = data[1];
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let mut points: Vec<(i16, i16)> = Vec::new();
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 && !points.is_empty() {
            let (px, py) = points[points.len() - 1];
            points.push((px + x, py + y));
        } else {
            points.push((x, y));
        }
        offset += 4;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for w in points.windows(2) {
            fb.draw_line(
                w[0].0 as i32, w[0].1 as i32,
                w[1].0 as i32, w[1].1 as i32,
                gc.foreground, gc.line_width,
            );
        }
    }

    Vec::new()
}

fn handle_poly_point(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let coord_mode = data[1];
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let mut points = Vec::new();
    let mut last_x: i16 = 0;
    let mut last_y: i16 = 0;
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let mut x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let mut y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if coord_mode == 1 {
            x += last_x;
            y += last_y;
        }
        last_x = x;
        last_y = y;
        points.push((x, y));
        offset += 4;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y) in points {
            fb.draw_point(x as i32, y as i32, gc.foreground);
        }
    }

    Vec::new()
}

fn handle_poly_segment(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let mut segments = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x1 = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y1 = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let x2 = i16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let y2 = i16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        segments.push((x1, y1, x2, y2));
        offset += 8;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x1, y1, x2, y2) in segments {
            fb.draw_line(x1 as i32, y1 as i32, x2 as i32, y2 as i32, gc.foreground, gc.line_width);
        }
    }

    Vec::new()
}

fn handle_poly_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height, angle1, angle2) in arcs {
            fb.draw_arc(x, y, width, height, angle1, angle2, false, gc.foreground);
        }
    }

    Vec::new()
}

fn handle_poly_fill_arc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut arcs = Vec::new();
    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);
        arcs.push((x, y, width, height, angle1, angle2));
        offset += 12;
    }

    let fg = state.map_color_for_drawable(drawable, gc.foreground);
    info!("PolyFillArc: gc={gc_id:#x} func={} fg_raw={:#x} fg_mapped={fg:#x} draw={drawable:#x}", gc.function, gc.foreground);
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, width, height, angle1, angle2) in &arcs {
            fb.draw_arc(*x, *y, *width, *height, *angle1, *angle2, true, fg);
        }
    }

    // Notify DAMAGE subscribers
    for &(x, y, width, height, _, _) in &arcs {
        state.notify_damage(drawable, x, y, width, height);
    }

    Vec::new()
}

fn handle_poly_text8(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let mut cursor_x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();


    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    // Collect text items first to avoid borrow issues
    let mut items: Vec<(i16, i16, u16, u16, Vec<u8>)> = Vec::new();
    let mut offset = 16;
    let end = data.len();

    while offset < end {
        let item_len = data[offset] as usize;

        if item_len == 255 {
            offset += 5;
            continue;
        }
        if item_len == 0 {
            break;
        }
        if offset + 2 + item_len > end {
            break;
        }

        let delta = data[offset + 1] as i8;
        cursor_x += delta as i16;

        let text = &data[offset + 2..offset + 2 + item_len];
        let (img_w, img_h, pixels) = font.render_text_transparent(text, gc.foreground);

        if img_w > 0 && img_h > 0 {
            items.push((cursor_x, y - font.font_ascent, img_w, img_h, pixels));
        }

        let mut text_advance: i32 = 0;
        for &ch in text {
            text_advance += font.char_info(ch as u16).character_width as i32;
        }
        cursor_x += text_advance as i16;
        offset += 2 + item_len;
    }

    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        for (x, y, w, h, pixels) in items {
            // Use Over compositing to preserve background under transparent pixels
            fb.put_image_over(x, y, w, h, &pixels);
        }
    }

    Vec::new()
}

fn handle_image_text8(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let str_len = data[1] as usize;
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let text = if 16 + str_len <= data.len() {
        &data[16..16 + str_len]
    } else {
        return Vec::new();
    };

    let font = state
        .font_manager
        .get_font(gc.font_id)
        .or_else(|| state.font_manager.get_default_font());

    let font = match font {
        Some(f) => f,
        None => return Vec::new(),
    };

    let (img_w, img_h, pixels) = font.render_text(text, gc.foreground, gc.background);
    if img_w == 0 || img_h == 0 {
        return Vec::new();
    }

    let render_y = y - font.font_ascent;
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.put_image(x, render_y, img_w, img_h, &pixels);
    }

    Vec::new()
}

fn handle_put_image(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let format = data[1]; // 0=Bitmap, 1=XYPixmap, 2=ZPixmap
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);
    let dst_x = i16::from_le_bytes([data[16], data[17]]);
    let dst_y = i16::from_le_bytes([data[18], data[19]]);
    let _left_pad = data[20];
    let depth = data[21];

    let pixel_data = &data[24..];

    debug!("PutImage: fmt={format} depth={depth} drawable={drawable:#x} {width}x{height} at ({dst_x},{dst_y}) data={}", pixel_data.len());

    // Only handle ZPixmap format with 32bpp (our native format)
    if format == 2 && depth >= 24 {
        if let Some(fb) = state.get_framebuffer_mut(drawable) {
            fb.put_image(dst_x, dst_y, width, height, pixel_data);
        }
    } else if format == 2 && depth == 1 {
        // 1-bit depth ZPixmap: used for cursor bitmaps, skip
    } else {
        debug!(
            "PutImage: unsupported format={format} depth={depth} {}x{} data_len={}",
            width,
            height,
            pixel_data.len()
        );
    }

    Vec::new()
}

fn handle_get_image(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }

    let _format = data[1]; // 1=XYPixmap, 2=ZPixmap
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let x = i16::from_le_bytes([data[8], data[9]]);
    let y = i16::from_le_bytes([data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    // Sync SHM pixmaps before reading
    state.sync_shm_pixmap(drawable);

    // Read actual pixel data from the drawable's framebuffer
    let pixels = if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.extract_pixels(x, y, width, height)
    } else {
        vec![0u8; width as usize * height as usize * 4]
    };

    let row_bytes = width as usize * 4;
    let padded_row = (row_bytes + 3) & !3;
    let data_len = padded_row * height as usize;
    let length_field = (data_len / 4) as u32;

    let mut reply = vec![0u8; 32 + data_len];
    reply[0] = 1; // Reply
    reply[1] = 24; // depth
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..12].copy_from_slice(&ROOT_VISUAL.to_le_bytes());

    // Copy pixel data into reply (row by row with padding)
    for row in 0..height as usize {
        let src_off = row * row_bytes;
        let dst_off = 32 + row * padded_row;
        let copy_len = row_bytes.min(pixels.len() - src_off);
        if src_off + copy_len <= pixels.len() && dst_off + copy_len <= reply.len() {
            reply[dst_off..dst_off + copy_len].copy_from_slice(&pixels[src_off..src_off + copy_len]);
        }
    }

    reply
}

fn handle_alloc_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // AllocColor request: [opcode, pad, length, colormap(4), red(2), green(2), blue(2), pad(2)]
    if data.len() < 16 {
        return Vec::new();
    }

    let red = u16::from_le_bytes([data[8], data[9]]);
    let green = u16::from_le_bytes([data[10], data[11]]);
    let blue = u16::from_le_bytes([data[12], data[13]]);

    // For TrueColor visual with masks R=0xFF0000 G=0x00FF00 B=0x0000FF:
    // Convert 16-bit color components to 8-bit and pack into a pixel value
    let r8 = (red >> 8) as u32;
    let g8 = (green >> 8) as u32;
    let b8 = (blue >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    // AllocColor reply: [1, pad, seq(2), length(4)=0, red(2), green(2), blue(2), pad(2), pixel(4)]
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0 (no extra data beyond 32 bytes)
    reply[8..10].copy_from_slice(&red.to_le_bytes());
    reply[10..12].copy_from_slice(&green.to_le_bytes());
    reply[12..14].copy_from_slice(&blue.to_le_bytes());
    // pad at 14..16
    reply[16..20].copy_from_slice(&pixel.to_le_bytes());

    reply.to_vec()
}

/// Parse a named color to RGB. Returns (r16, g16, b16) in 16-bit values.
fn parse_color_name(name: &str) -> (u16, u16, u16) {
    match name.to_lowercase().as_str() {
        "white" => (0xFFFF, 0xFFFF, 0xFFFF),
        "black" => (0, 0, 0),
        "red" => (0xFFFF, 0, 0),
        "green" => (0, 0xFFFF, 0),
        "blue" => (0, 0, 0xFFFF),
        "yellow" => (0xFFFF, 0xFFFF, 0),
        "cyan" => (0, 0xFFFF, 0xFFFF),
        "magenta" => (0xFFFF, 0, 0xFFFF),
        "gray" | "grey" => (0xBEBE, 0xBEBE, 0xBEBE),
        "light gray" | "light grey" | "lightgray" | "lightgrey" => (0xD3D3, 0xD3D3, 0xD3D3),
        "dark gray" | "dark grey" | "darkgray" | "darkgrey" => (0xA9A9, 0xA9A9, 0xA9A9),
        "orange" => (0xFFFF, 0xA5A5, 0),
        "brown" => (0xA5A5, 0x2A2A, 0x2A2A),
        "pink" => (0xFFFF, 0xC0C0, 0xCBCB),
        "purple" => (0x8080, 0, 0x8080),
        "navy" => (0, 0, 0x8080),
        "olive" => (0x8080, 0x8080, 0),
        "teal" => (0, 0x8080, 0x8080),
        "maroon" => (0x8080, 0, 0),
        "silver" => (0xC0C0, 0xC0C0, 0xC0C0),
        "aqua" => (0, 0xFFFF, 0xFFFF),
        "lime" => (0, 0xFFFF, 0),
        "fuchsia" => (0xFFFF, 0, 0xFFFF),
        _ => {
            // Try to parse hex format: #RRGGBB or #RGB
            if name.starts_with('#') && name.len() == 7 {
                let r = u16::from_str_radix(&name[1..3], 16).unwrap_or(0);
                let g = u16::from_str_radix(&name[3..5], 16).unwrap_or(0);
                let b = u16::from_str_radix(&name[5..7], 16).unwrap_or(0);
                (r * 257, g * 257, b * 257)
            } else {
                (0, 0, 0) // default to black for unknown colors
            }
        }
    }
}

fn handle_alloc_named_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // AllocNamedColor: [opcode(1), pad(1), length(2), cmap(4), name_len(2), pad(2), name(...)]
    if data.len() < 12 {
        return Vec::new();
    }

    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        std::str::from_utf8(&data[12..12 + name_len]).unwrap_or("")
    } else {
        ""
    };

    let (r16, g16, b16) = parse_color_name(name);
    let r8 = (r16 >> 8) as u32;
    let g8 = (g16 >> 8) as u32;
    let b8 = (b16 >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    info!("AllocNamedColor: name={name:?} -> pixel={pixel:#x}");

    // Reply: [1, pad, seq(2), length(4)=0, pixel(4), exact_red(2), exact_green(2), exact_blue(2),
    //         visual_red(2), visual_green(2), visual_blue(2)]
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&pixel.to_le_bytes());
    reply[12..14].copy_from_slice(&r16.to_le_bytes()); // exact red
    reply[14..16].copy_from_slice(&g16.to_le_bytes()); // exact green
    reply[16..18].copy_from_slice(&b16.to_le_bytes()); // exact blue
    reply[18..20].copy_from_slice(&r16.to_le_bytes()); // visual red
    reply[20..22].copy_from_slice(&g16.to_le_bytes()); // visual green
    reply[22..24].copy_from_slice(&b16.to_le_bytes()); // visual blue

    reply.to_vec()
}

fn handle_lookup_color(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // LookupColor: [opcode(1), pad(1), length(2), cmap(4), name_len(2), pad(2), name(...)]
    if data.len() < 12 {
        return Vec::new();
    }

    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let name = if 12 + name_len <= data.len() {
        std::str::from_utf8(&data[12..12 + name_len]).unwrap_or("")
    } else {
        ""
    };

    let (r16, g16, b16) = parse_color_name(name);

    // Reply: exact and visual colors
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..10].copy_from_slice(&r16.to_le_bytes());
    reply[10..12].copy_from_slice(&g16.to_le_bytes());
    reply[12..14].copy_from_slice(&b16.to_le_bytes());
    reply[14..16].copy_from_slice(&r16.to_le_bytes());
    reply[16..18].copy_from_slice(&g16.to_le_bytes());
    reply[18..20].copy_from_slice(&b16.to_le_bytes());

    reply.to_vec()
}

fn handle_query_colors(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // QueryColors request: [opcode, pad, length, colormap(4), pixel0(4), pixel1(4), ...]
    if data.len() < 8 {
        return Vec::new();
    }

    let n_pixels = (data.len() - 8) / 4;
    let mut colors = Vec::with_capacity(n_pixels);

    for i in 0..n_pixels {
        let offset = 8 + i * 4;
        let pixel = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);

        // Decompose TrueColor pixel back to 16-bit RGB
        let r = ((pixel >> 16) & 0xFF) as u16;
        let g = ((pixel >> 8) & 0xFF) as u16;
        let b = (pixel & 0xFF) as u16;

        colors.push((r << 8 | r, g << 8 | g, b << 8 | b));
    }

    let data_len = n_pixels * 8; // Each RGB is 8 bytes (r2, g2, b2, pad2)
    let padded = (data_len + 3) & !3;
    let length_field = (padded / 4) as u32;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..10].copy_from_slice(&(n_pixels as u16).to_le_bytes());

    for (i, &(r, g, b)) in colors.iter().enumerate() {
        let off = 32 + i * 8;
        reply[off..off + 2].copy_from_slice(&r.to_le_bytes());
        reply[off + 2..off + 4].copy_from_slice(&g.to_le_bytes());
        reply[off + 4..off + 6].copy_from_slice(&b.to_le_bytes());
        // pad at off+6..off+8
    }

    reply
}

fn handle_get_keyboard_mapping(data: &[u8], seq: u16) -> Vec<u8> {
    // GetKeyboardMapping: [opcode(1), pad(1), length(2), first_keycode(1), count(1), pad(2)]
    let first_keycode = if data.len() >= 5 { data[4] } else { 8 };
    let count = if data.len() >= 6 { data[5] } else { 248 };

    // Return 4 keysyms per keycode (normal, shift, mode_switch, mode+shift)
    let keysyms_per_keycode: u8 = 4;
    let total_syms = count as u32 * keysyms_per_keycode as u32;
    let reply_len = 32 + total_syms as usize * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1; // Reply
    reply[1] = keysyms_per_keycode;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&total_syms.to_le_bytes());

    // Fill in keysyms for each keycode
    // This maps X11 keycodes to keysyms using a standard US keyboard layout.
    // X11 keycodes start at 8 (min_keycode in our setup).
    // Browser keyCode + 8 = X11 keycode (approximately).
    for i in 0..count as usize {
        let keycode = first_keycode as usize + i;
        let offset = 32 + i * keysyms_per_keycode as usize * 4;

        // Map keycode to (normal_keysym, shifted_keysym)
        let (normal, shifted) = keycode_to_keysym(keycode as u8);

        // Normal keysym
        reply[offset..offset + 4].copy_from_slice(&normal.to_le_bytes());
        // Shifted keysym
        reply[offset + 4..offset + 8].copy_from_slice(&shifted.to_le_bytes());
        // Mode switch and mode+shift left as 0 (NoSymbol)
    }

    reply
}

/// Map X11 keycode to (normal_keysym, shifted_keysym).
/// Based on standard US keyboard layout.
/// X11 keycodes = browser keyCode + 8 (approximately).
fn keycode_to_keysym(keycode: u8) -> (u32, u32) {
    // Evdev-based X11 keycodes (matching the frontend's e.code mapping)
    const XK_BACKSPACE: u32 = 0xff08;
    const XK_TAB: u32 = 0xff09;
    const XK_RETURN: u32 = 0xff0d;
    const XK_ESCAPE: u32 = 0xff1b;
    const XK_DELETE: u32 = 0xffff;
    const XK_HOME: u32 = 0xff50;
    const XK_LEFT: u32 = 0xff51;
    const XK_UP: u32 = 0xff52;
    const XK_RIGHT: u32 = 0xff53;
    const XK_DOWN: u32 = 0xff54;
    const XK_PAGE_UP: u32 = 0xff55;
    const XK_PAGE_DOWN: u32 = 0xff56;
    const XK_END: u32 = 0xff57;
    const XK_INSERT: u32 = 0xff63;
    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_SHIFT_R: u32 = 0xffe2;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_CONTROL_R: u32 = 0xffe4;
    const XK_CAPS_LOCK: u32 = 0xffe5;
    const XK_ALT_L: u32 = 0xffe9;
    const XK_ALT_R: u32 = 0xffea;
    const XK_SUPER_L: u32 = 0xffeb;
    const XK_SUPER_R: u32 = 0xffec;
    const XK_F1: u32 = 0xffbe;
    const XK_SPACE: u32 = 0x0020;

    match keycode {
        9 => (XK_ESCAPE, XK_ESCAPE),
        10 => (0x31, 0x21), // 1 !
        11 => (0x32, 0x40), // 2 @
        12 => (0x33, 0x23), // 3 #
        13 => (0x34, 0x24), // 4 $
        14 => (0x35, 0x25), // 5 %
        15 => (0x36, 0x5e), // 6 ^
        16 => (0x37, 0x26), // 7 &
        17 => (0x38, 0x2a), // 8 *
        18 => (0x39, 0x28), // 9 (
        19 => (0x30, 0x29), // 0 )
        20 => (0x2d, 0x5f), // - _
        21 => (0x3d, 0x2b), // = +
        22 => (XK_BACKSPACE, XK_BACKSPACE),
        23 => (XK_TAB, XK_TAB),
        24 => (0x71, 0x51), // q Q
        25 => (0x77, 0x57), // w W
        26 => (0x65, 0x45), // e E
        27 => (0x72, 0x52), // r R
        28 => (0x74, 0x54), // t T
        29 => (0x79, 0x59), // y Y
        30 => (0x75, 0x55), // u U
        31 => (0x69, 0x49), // i I
        32 => (0x6f, 0x4f), // o O
        33 => (0x70, 0x50), // p P
        34 => (0x5b, 0x7b), // [ {
        35 => (0x5d, 0x7d), // ] }
        36 => (XK_RETURN, XK_RETURN),
        37 => (XK_CONTROL_L, XK_CONTROL_L),
        38 => (0x61, 0x41), // a A
        39 => (0x73, 0x53), // s S
        40 => (0x64, 0x44), // d D
        41 => (0x66, 0x46), // f F
        42 => (0x67, 0x47), // g G
        43 => (0x68, 0x48), // h H
        44 => (0x6a, 0x4a), // j J
        45 => (0x6b, 0x4b), // k K
        46 => (0x6c, 0x4c), // l L
        47 => (0x3b, 0x3a), // ; :
        48 => (0x27, 0x22), // ' "
        49 => (0x60, 0x7e), // ` ~
        50 => (XK_SHIFT_L, XK_SHIFT_L),
        51 => (0x5c, 0x7c), // \ |
        52 => (0x7a, 0x5a), // z Z
        53 => (0x78, 0x58), // x X
        54 => (0x63, 0x43), // c C
        55 => (0x76, 0x56), // v V
        56 => (0x62, 0x42), // b B
        57 => (0x6e, 0x4e), // n N
        58 => (0x6d, 0x4d), // m M
        59 => (0x2c, 0x3c), // , <
        60 => (0x2e, 0x3e), // . >
        61 => (0x2f, 0x3f), // / ?
        62 => (XK_SHIFT_R, XK_SHIFT_R),
        64 => (XK_ALT_L, XK_ALT_L),
        65 => (XK_SPACE, XK_SPACE),
        66 => (XK_CAPS_LOCK, XK_CAPS_LOCK),
        k @ 67..=76 => (XK_F1 + (k - 67) as u32, XK_F1 + (k - 67) as u32),
        95 => (XK_F1 + 10, XK_F1 + 10),
        96 => (XK_F1 + 11, XK_F1 + 11),
        105 => (XK_CONTROL_R, XK_CONTROL_R),
        108 => (XK_ALT_R, XK_ALT_R),
        110 => (XK_HOME, XK_HOME),
        111 => (XK_UP, XK_UP),
        112 => (XK_PAGE_UP, XK_PAGE_UP),
        113 => (XK_LEFT, XK_LEFT),
        114 => (XK_RIGHT, XK_RIGHT),
        115 => (XK_END, XK_END),
        116 => (XK_DOWN, XK_DOWN),
        117 => (XK_PAGE_DOWN, XK_PAGE_DOWN),
        118 => (XK_INSERT, XK_INSERT),
        119 => (XK_DELETE, XK_DELETE),
        133 => (XK_SUPER_L, XK_SUPER_L),
        134 => (XK_SUPER_R, XK_SUPER_R),
        _ => (0, 0),
    }
}

fn handle_list_extensions(seq: u16) -> Vec<u8> {
    // Return the list of extensions we support
    let extensions: &[&str] = &["BIG-REQUESTS", "MIT-SHM", "RENDER", "XFIXES", "SHAPE", "SYNC", "Generic Event Extension", "XC-MISC", "Composite", "DAMAGE", "Present"];

    // Build the names data: each is a length-prefixed string (1 byte len + name)
    let mut names_data = Vec::new();
    for ext in extensions {
        names_data.push(ext.len() as u8);
        names_data.extend_from_slice(ext.as_bytes());
    }
    // Pad to 4-byte boundary
    while names_data.len() % 4 != 0 {
        names_data.push(0);
    }

    let extra_len = names_data.len();
    let mut reply = vec![0u8; 32 + extra_len];
    reply[0] = 1; // Reply
    reply[1] = extensions.len() as u8; // num_extensions
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((extra_len / 4) as u32).to_le_bytes());
    reply[32..].copy_from_slice(&names_data);

    reply
}

fn handle_query_extension(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // Parse extension name from the request
    let name_len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let name = if data.len() >= 8 + name_len {
        std::str::from_utf8(&data[8..8 + name_len]).unwrap_or("")
    } else {
        ""
    };

    debug!("QueryExtension: \"{}\"", name);

    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());

    match name {
        "RENDER" => {
            reply[8] = 1; // present = true
            reply[9] = 139; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "MIT-SHM" => {
            reply[8] = 1; // present = true
            reply[9] = 130; // major_opcode
            reply[10] = 65; // first_event (ShmCompletion)
            reply[11] = 128; // first_error
        }
        "BIG-REQUESTS" => {
            reply[8] = 1; // present = true
            reply[9] = 133; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "XFIXES" => {
            reply[8] = 1; // present = true
            reply[9] = 138; // major_opcode
            reply[10] = 87; // first_event
            reply[11] = 0; // first_error
        }
        "SHAPE" => {
            reply[8] = 1; // present = true
            reply[9] = 128; // major_opcode
            reply[10] = 64; // first_event
            reply[11] = 0; // first_error
        }
        "SYNC" => {
            reply[8] = 1; // present = true
            reply[9] = 134; // major_opcode
            reply[10] = 100; // first_event (use 100 to avoid conflict)
            reply[11] = 0; // first_error
        }
        "Generic Event Extension" => {
            reply[8] = 1; // present = true
            reply[9] = 135; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "Composite" => {
            reply[8] = 1;
            reply[9] = 142;
        }
        "DAMAGE" => {
            reply[8] = 1;
            reply[9] = 143;
            reply[10] = 91;
            reply[11] = 152;
        }
        // RANDR disabled — GTK renders incorrectly with our minimal RANDR replies.
        // The handler code is kept for future use when replies are fully correct.
        // XKEYBOARD disabled — our stub causes Firefox to take a worse init path
        "XKEYBOARD" => {
            // present = false
        }
        "XC-MISC" => {
            reply[8] = 1; // present = true
            reply[9] = 141; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "Present" => {
            reply[8] = 1; // present = true
            reply[9] = 148; // major_opcode
            reply[10] = 0; // first_event
            reply[11] = 0; // first_error
        }
        "XINERAMA" | "XInputExtension" => {
            // Not present — already zero
        }
        _ => {
            // present = false (byte 8 = 0) — already zero
        }
    }

    reply.to_vec()
}

fn handle_xfixes_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XFIXES minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: return version 5.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&5u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&0u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        4 => {
            // GetCursorImage: return an error (BadImplementation)
            let mut err = [0u8; 32];
            err[0] = 0; // Error
            err[1] = 17; // BadImplementation
            err[2..4].copy_from_slice(&seq.to_le_bytes());
            err.to_vec()
        }
        18 => {
            // FetchRegion: return empty region reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply length = 0 (no rectangles)
            // extents: x1=0, y1=0, x2=0, y2=0 (bytes 8-15, already zero)
            reply.to_vec()
        }
        31 => {
            // GetCursorName: return empty reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // atom = 0 (None), name length = 0
            reply.to_vec()
        }
        // All other minor opcodes: ignore
        _ => Vec::new(),
    }
}

fn handle_randr_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("RANDR minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: return version 1.5
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&5u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        2 => {
            // SetScreenConfig: reply with success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            // config_timestamp
            reply[12..16].copy_from_slice(&0u32.to_le_bytes());
            // root window
            reply[16..20].copy_from_slice(&state.root_window.to_le_bytes());
            reply.to_vec()
        }
        5 => {
            // GetScreenInfo: minimal screen configuration
            // Reply header (32) + 1 ScreenSize (8 bytes) + 0 rates
            let num_sizes: u16 = 1;
            let extra_data_len: usize = 8; // 1 screen size * 8 bytes
            let reply_len = 32 + extra_data_len;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1; // Reply
            reply[1] = 1; // rotations = Rotate_0
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((extra_data_len / 4) as u32).to_le_bytes()); // length
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
            // timestamp
            reply[12..16].copy_from_slice(&0u32.to_le_bytes());
            // config_timestamp
            reply[16..20].copy_from_slice(&0u32.to_le_bytes());
            reply[20..22].copy_from_slice(&num_sizes.to_le_bytes()); // nSizes
            reply[22..24].copy_from_slice(&0u16.to_le_bytes()); // sizeID (current)
            reply[24..26].copy_from_slice(&1u16.to_le_bytes()); // rotation = Rotate_0
            reply[26..28].copy_from_slice(&0u16.to_le_bytes()); // nrateEnts = 0
            // pad bytes 28-31 already zero
            // Screen size entry: width(2), height(2), mwidth(2), mheight(2)
            reply[32..34].copy_from_slice(&SCREEN_WIDTH.to_le_bytes());
            reply[34..36].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes());
            reply[36..38].copy_from_slice(&270u16.to_le_bytes()); // mm width
            reply[38..40].copy_from_slice(&203u16.to_le_bytes()); // mm height
            reply
        }
        6 => {
            // GetScreenSizeRange: min=1x1, max=32767x32767
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // min_width
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // min_height
            reply[12..14].copy_from_slice(&32767u16.to_le_bytes()); // max_width
            reply[14..16].copy_from_slice(&32767u16.to_le_bytes()); // max_height
            reply.to_vec()
        }
        7 => {
            // SetScreenSize: ignore (void)
            Vec::new()
        }
        8 | 19 => {
            // GetScreenResources / GetScreenResourcesCurrent
            // Minimal reply with 0 CRTCs, 0 outputs, 0 modes
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&0u32.to_le_bytes()); // length (no extra data)
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            // config_timestamp
            reply[12..16].copy_from_slice(&0u32.to_le_bytes());
            reply[16..18].copy_from_slice(&0u16.to_le_bytes()); // num_crtcs
            reply[18..20].copy_from_slice(&0u16.to_le_bytes()); // num_outputs
            reply[20..22].copy_from_slice(&0u16.to_le_bytes()); // num_modes
            reply[22..24].copy_from_slice(&0u16.to_le_bytes()); // names_len
            reply.to_vec()
        }
        9 => {
            // GetOutputInfo: return error (BadOutput)
            build_error(11, seq, 0, 140, 9) // BadAccess as placeholder
        }
        14 => {
            // GetCrtcInfo: return error
            build_error(11, seq, 0, 140, 14)
        }
        15 => {
            // SetCrtcConfig: reply with success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // Success
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            reply.to_vec()
        }
        20 => {
            // SelectInput: ignore
            Vec::new()
        }
        41 => {
            // GetOutputPrimary: reply with output=0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // output = 0
            reply.to_vec()
        }
        46 => {
            // GetProviders: 0 providers
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // timestamp
            reply[8..12].copy_from_slice(&0u32.to_le_bytes());
            reply[12..14].copy_from_slice(&0u16.to_le_bytes()); // num_providers
            reply.to_vec()
        }
        47 => {
            // GetProviderInfo: empty reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled RANDR minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_shape_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("SHAPE minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: return version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        _ => Vec::new(),
    }
}

/// Handle MIT-SHM extension requests (major opcode 130).
fn handle_shm_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];

    match minor {
        // QueryVersion
        0 => {
            info!("SHM QueryVersion");
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // shared_pixmaps = true
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply[4..8] = additional data length = 0
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&2u16.to_le_bytes()); // minor version
            reply[12..14].copy_from_slice(&0u16.to_le_bytes()); // uid
            reply[14..16].copy_from_slice(&0u16.to_le_bytes()); // gid
            reply[16] = 2; // pixmap_format = ZPixmap
            reply.to_vec()
        }

        // Attach
        1 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let shmid = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as i32;
            let read_only = data[12] != 0;

            info!("SHM Attach: shmseg={shmseg} shmid={shmid} read_only={read_only}");

            unsafe {
                // Get segment size via shmctl IPC_STAT
                let mut ds: libc::shmid_ds = std::mem::zeroed();
                let stat_ret = libc::shmctl(shmid, libc::IPC_STAT, &mut ds);
                if stat_ret < 0 {
                    warn!("SHM Attach: shmctl IPC_STAT failed for shmid={shmid}");
                    return Vec::new();
                }
                let size = ds.shm_segsz;

                let flags = if read_only { libc::SHM_RDONLY } else { 0 };
                let addr = libc::shmat(shmid, std::ptr::null(), flags);
                if addr == (-1isize) as *mut libc::c_void {
                    warn!("SHM Attach: shmat failed for shmid={shmid}");
                    return Vec::new();
                }

                state.shm_segments.insert(shmseg, ShmSegment {
                    addr: addr as *mut u8,
                    size,
                });
            }

            Vec::new() // No reply for Attach
        }

        // Detach
        2 => {
            if data.len() < 8 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            info!("SHM Detach: shmseg={shmseg}");

            if let Some(seg) = state.shm_segments.remove(&shmseg) {
                unsafe {
                    libc::shmdt(seg.addr as *const libc::c_void);
                }
            }

            Vec::new() // No reply for Detach
        }

        // PutImage
        3 => {
            if data.len() < 40 {
                return Vec::new();
            }

            let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let _gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            let total_width = u16::from_le_bytes([data[12], data[13]]) as usize;
            let _total_height = u16::from_le_bytes([data[14], data[15]]);
            let src_x = u16::from_le_bytes([data[16], data[17]]) as usize;
            let src_y = u16::from_le_bytes([data[18], data[19]]) as usize;
            let src_width = u16::from_le_bytes([data[20], data[21]]);
            let src_height = u16::from_le_bytes([data[22], data[23]]);
            let dst_x = i16::from_le_bytes([data[24], data[25]]);
            let dst_y = i16::from_le_bytes([data[26], data[27]]);
            let _depth = data[28];
            let _format = data[29];
            let send_event = data[30] != 0;
            let shmseg = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);
            let offset = u32::from_le_bytes([data[36], data[37], data[38], data[39]]) as usize;

            info!(
                "SHM PutImage: drawable={drawable:#x} shmseg={shmseg} offset={offset} \
                 total_width={total_width} src=({src_x},{src_y}) size=({src_width}x{src_height}) \
                 dst=({dst_x},{dst_y}) send_event={send_event}"
            );

            let seg = match state.shm_segments.get(&shmseg) {
                Some(s) => s,
                None => {
                    warn!("SHM PutImage: unknown shmseg={shmseg}");
                    return Vec::new();
                }
            };

            // Bytes per pixel (32bpp BGRA)
            let bpp = 4usize;
            let src_stride = total_width * bpp;
            let region_size = src_stride * (src_y + src_height as usize);

            // Bounds check
            if offset + region_size > seg.size {
                warn!(
                    "SHM PutImage: out of bounds (offset={offset} + region_size={region_size} > seg.size={})",
                    seg.size
                );
                return Vec::new();
            }

            // Build a contiguous pixel buffer for the source region
            let w = src_width as usize;
            let h = src_height as usize;
            let mut pixels = vec![0u8; w * h * bpp];

            unsafe {
                let base = seg.addr.add(offset);
                for row in 0..h {
                    let src_off = (src_y + row) * src_stride + src_x * bpp;
                    let dst_off = row * w * bpp;
                    let src_ptr = base.add(src_off);
                    std::ptr::copy_nonoverlapping(src_ptr, pixels.as_mut_ptr().add(dst_off), w * bpp);
                }
            }

            // Blit to the drawable's framebuffer
            if let Some(fb) = state.get_framebuffer_mut(drawable) {
                fb.put_image(dst_x, dst_y, src_width, src_height, &pixels);
            }

            // If send_event, return a ShmCompletion event
            if send_event {
                let mut event = [0u8; 32];
                event[0] = 65; // ShmCompletion event type (first_event + 0)
                event[2..4].copy_from_slice(&seq.to_le_bytes());
                event[4..8].copy_from_slice(&drawable.to_le_bytes());
                event[8..12].copy_from_slice(&shmseg.to_le_bytes());
                event[16..20].copy_from_slice(&(offset as u32).to_le_bytes());
                event.to_vec()
            } else {
                Vec::new()
            }
        }

        // GetImage
        4 => {
            if data.len() < 32 {
                return Vec::new();
            }
            let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let src_x = i16::from_le_bytes([data[8], data[9]]);
            let src_y = i16::from_le_bytes([data[10], data[11]]);
            let width = u16::from_le_bytes([data[12], data[13]]);
            let height = u16::from_le_bytes([data[14], data[15]]);
            let _plane_mask = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
            let _format = data[20];
            let shmseg = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
            let shm_offset = u32::from_le_bytes([data[28], data[29], data[30], data[31]]) as usize;

            info!("SHM GetImage: drawable={drawable:#x} ({src_x},{src_y}) {width}x{height} shmseg={shmseg} offset={shm_offset}");

            // Sync SHM-backed pixmap data before reading
            state.sync_shm_pixmap(drawable);

            // Copy pixels from drawable into SHM segment
            let resolved = state.resolve_drawable(drawable);
            let pixels = if let Some(fb) = state.get_framebuffer_mut(resolved) {
                fb.extract_pixels(src_x, src_y, width, height)
            } else {
                vec![0u8; width as usize * height as usize * 4]
            };

            if let Some(seg) = state.shm_segments.get(&shmseg) {
                let bpp = 4usize;
                let row_bytes = width as usize * bpp;
                let total_bytes = row_bytes * height as usize;
                if shm_offset + total_bytes <= seg.size {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            pixels.as_ptr(),
                            seg.addr.add(shm_offset),
                            total_bytes.min(pixels.len()),
                        );
                    }
                }
            }

            // Reply
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 24; // depth
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&ROOT_VISUAL.to_le_bytes());
            reply[12..16].copy_from_slice(&(width as u32 * height as u32).to_le_bytes()); // size
            reply.to_vec()
        }

        // CreatePixmap
        5 => {
            if data.len() < 28 {
                return Vec::new();
            }
            let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let width = u16::from_le_bytes([data[12], data[13]]);
            let height = u16::from_le_bytes([data[14], data[15]]);
            let depth = data[16];
            let shmseg = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
            let shm_offset = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as usize;

            info!("SHM CreatePixmap: pid={pid:#x} {width}x{height} depth={depth} shmseg={shmseg} offset={shm_offset}");

            // Create an SHM-backed pixmap. The client will write directly into
            // the SHM segment; we sync from it before reading.
            state.pixmaps.insert(
                pid,
                PixmapState {
                    _id: pid,
                    _width: width,
                    _height: height,
                    _depth: depth,
                    framebuffer: Framebuffer::new(width as u32, height as u32),
                    alias_window: None,
                    shm_backing: Some(ShmPixmapBacking {
                        shmseg,
                        offset: shm_offset,
                    }),
                },
            );
            Vec::new()
        }

        // AttachFd (minor 6) — used in MIT-SHM 1.2+ with fd passing
        6 => {
            if data.len() < 16 {
                return Vec::new();
            }
            let shmseg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            info!("SHM AttachFd: shmseg={shmseg} (stubbed — fd passing not supported)");
            Vec::new()
        }

        _ => {
            warn!("Unhandled SHM minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_sync_request(_state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    info!("SYNC minor opcode: {minor}");

    match minor {
        0 => {
            // Initialize: reply with version 3.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = 3; // major version
            reply[9] = 1; // minor version
            reply.to_vec()
        }
        1 => {
            // ListSystemCounters: reply with 0 counters
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // length = 0 (no extra data)
            // num_counters = 0
            reply.to_vec()
        }
        2 | 3 | 4 => {
            // CreateCounter, SetCounter, ChangeCounter: void
            Vec::new()
        }
        5 => {
            // QueryCounter: reply with value 0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // value_hi = 0, value_lo = 0 (already zero)
            reply.to_vec()
        }
        6 => {
            // DestroyCounter: void
            Vec::new()
        }
        7 => {
            // Await: return immediately (no blocking)
            Vec::new()
        }
        8 | 9 => {
            // CreateAlarm, ChangeAlarm: void
            Vec::new()
        }
        10 => {
            // QueryAlarm: reply with zeroed alarm state
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        11 => {
            // DestroyAlarm: void
            Vec::new()
        }
        12 => {
            // SetPriority: void
            Vec::new()
        }
        13 => {
            // GetPriority: reply with priority 0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // priority = 0 (already zero)
            reply.to_vec()
        }
        14 | 15 | 16 | 17 => {
            // CreateFence, TriggerFence, ResetFence, DestroyFence: void
            Vec::new()
        }
        18 => {
            // QueryFence: reply with triggered=true
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8] = 1; // triggered = true
            reply.to_vec()
        }
        19 => {
            // AwaitFence: return immediately
            Vec::new()
        }
        _ => {
            debug!("Unhandled SYNC minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_damage_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("DAMAGE minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&1u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // DamageCreate: data[4..8] = damage_id, data[8..12] = drawable, data[12] = level
            if data.len() >= 13 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let level = data[12];
                info!("DAMAGE Create: id={damage_id:#x} drawable={drawable:#x} level={level}");
                state.damage_regions.insert(damage_id, DamageInfo { drawable, level });
            }
            Vec::new()
        }
        2 => {
            // DamageDestroy: data[4..8] = damage_id
            if data.len() >= 8 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("DAMAGE Destroy: id={damage_id:#x}");
                state.damage_regions.remove(&damage_id);
            }
            Vec::new()
        }
        3 => {
            // DamageSubtract: data[4..8] = damage_id, data[8..12] = repair, data[12..16] = parts
            // This acknowledges the damage — we just accept it.
            if data.len() >= 8 {
                let damage_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("DAMAGE Subtract: id={damage_id:#x}");
            }
            Vec::new()
        }
        4 => {
            // DamageAdd: void
            Vec::new()
        }
        _ => {
            debug!("Unhandled DAMAGE minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_x_composite_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    info!("Composite minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 0.4
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&4u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // RedirectWindow: data[4..8] = window, data[8] = update
            if data.len() >= 9 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let update = data[8];
                info!("Composite RedirectWindow: window={window:#x} update={update}");
                if let Some(win) = state.windows.get_mut(&window) {
                    win.redirected = true;
                }
            }
            Vec::new()
        }
        2 => {
            // RedirectSubwindows: data[4..8] = window, data[8] = update
            if data.len() >= 9 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let update = data[8];
                info!("Composite RedirectSubwindows: window={window:#x} update={update}");
                // Mark all children as redirected
                let children: Vec<u32> = state.windows.iter()
                    .filter(|(_, w)| w.parent == window)
                    .map(|(id, _)| *id)
                    .collect();
                for child in children {
                    if let Some(w) = state.windows.get_mut(&child) {
                        w.redirected = true;
                    }
                }
            }
            Vec::new()
        }
        3 => {
            // UnredirectWindow: data[4..8] = window
            if data.len() >= 8 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                debug!("Composite UnredirectWindow: window={window:#x}");
                if let Some(win) = state.windows.get_mut(&window) {
                    win.redirected = false;
                }
            }
            Vec::new()
        }
        4 | 5 => {
            // UnredirectSubwindows, CreateRegionFromBorderClip: void
            Vec::new()
        }
        6 => {
            // NameWindowPixmap: create a pixmap aliased to a window's framebuffer
            // data[4..8] = window, data[8..12] = pixmap
            if data.len() >= 12 {
                let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let pixmap = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                // Create a pixmap entry that aliases the window's framebuffer.
                // The actual framebuffer here is a dummy - all accesses will be
                // redirected to the window via alias_window.
                if let Some(win) = state.windows.get(&window) {
                    let w = win.width;
                    let h = win.height;
                    state.pixmaps.insert(
                        pixmap,
                        PixmapState {
                            _id: pixmap,
                            _width: w,
                            _height: h,
                            _depth: 24,
                            framebuffer: crate::framebuffer::Framebuffer::new(0, 0),
                            alias_window: Some(window),
                            shm_backing: None,
                        },
                    );
                    info!("NameWindowPixmap: window={window:#x} -> pixmap={pixmap:#x} {w}x{h} (aliased)");
                }
            }
            Vec::new()
        }
        7 => {
            // GetOverlayWindow: reply with overlay window = root window
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled Composite minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_ge_request(data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Generic Event Extension minor opcode: {minor}");

    match minor {
        0 => {
            // QueryVersion: reply with version 1.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&0u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled GE minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_xkb_request(data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XKB minor opcode: {minor}");

    match minor {
        0 => {
            // UseExtension: reply with supported=true, version 1.0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 1; // supported = true
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // server major version
            reply[10..12].copy_from_slice(&0u16.to_le_bytes()); // server minor version
            reply.to_vec()
        }
        1 | 2 | 7 | 9 | 12 | 16 | 101 | 104 => {
            // SelectEvents, Bell, SetMap, SetCompatMap, SetIndicatorMap,
            // SetNames, LatchLockState, SetControls: void requests
            Vec::new()
        }
        4 => {
            // GetMap: return minimal empty map (present=0 means no data follows)
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply[4..8] = 0 (no extra data)
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id from request
            }
            reply[10..12].copy_from_slice(&8u16.to_le_bytes()); // min_key_code
            reply[12..14].copy_from_slice(&255u16.to_le_bytes()); // max_key_code
            // present = 0: no components present in reply
            reply.to_vec()
        }
        8 => {
            // GetCompatMap: return empty compat map
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            // all counts zero
            reply.to_vec()
        }
        10 => {
            // GetIndicatorState: reply with state=0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            // state = 0 (bytes 12..16 already zero)
            reply.to_vec()
        }
        11 => {
            // GetIndicatorMap: return empty indicators
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            reply.to_vec()
        }
        13 => {
            // GetNamedIndicator: reply with empty
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            reply.to_vec()
        }
        15 => {
            // GetNames: return with 0 counts for everything
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // reply[4..8] = 0 (no extra data)
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            reply[10..12].copy_from_slice(&8u16.to_le_bytes()); // min_key_code
            reply[12..14].copy_from_slice(&255u16.to_le_bytes()); // max_key_code
            // present = 0, everything else 0
            reply.to_vec()
        }
        17 => {
            // PerClientFlags: reply with value=0
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            // supported, value, autoCtrls, autoCtrlsValues all 0
            reply.to_vec()
        }
        100 => {
            // GetState: reply with device state
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            // all modifier/group state fields are 0
            reply.to_vec()
        }
        103 => {
            // GetControls: reply with minimal controls
            let mut reply = vec![0u8; 32 + 92]; // controls reply has extra data
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&(92u32 / 4).to_le_bytes()); // length
            if data.len() >= 6 {
                reply[8] = data[4]; // device_id
            }
            // num_groups = 1 (at least one group needed)
            // Offset 27 in the reply body is numGroups
            // The header is 32 bytes; controls data starts at byte 32
            // In the XKB GetControls reply: byte 10 = numGroups
            reply[10] = 1;
            // per_key_repeat at offset 32+20 = 52: set default repeat mask
            // (bytes 52..84 = 32 bytes of per-key repeat bitmap)
            for i in 0..32 {
                reply[32 + 20 + i] = 0xFF; // all keys repeat by default
            }
            reply
        }
        _ => {
            debug!("Unhandled XKB minor opcode: {minor}");
            Vec::new()
        }
    }
}

fn handle_xc_misc_request(data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("XC-MISC minor opcode: {minor}");

    match minor {
        0 => {
            // GetVersion: reply with version 1.1
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // major version
            reply[10..12].copy_from_slice(&1u16.to_le_bytes()); // minor version
            reply.to_vec()
        }
        1 => {
            // GetXIDRange: reply with a range of resource IDs
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0x08000000u32.to_le_bytes()); // start_id
            reply[12..16].copy_from_slice(&65536u32.to_le_bytes()); // count
            reply.to_vec()
        }
        2 => {
            // GetXIDList: return requested number of IDs
            let count = if data.len() >= 8 {
                u32::from_le_bytes([data[4], data[5], data[6], data[7]])
            } else {
                0
            };
            let ids_to_return = count.min(4096); // cap at reasonable limit
            let extra_bytes = (ids_to_return as usize) * 4;
            let padded = (extra_bytes + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
            reply[8..12].copy_from_slice(&ids_to_return.to_le_bytes()); // ids_count
            // Fill in sequential IDs starting from a high range
            let base: u32 = 0x09000000;
            for i in 0..ids_to_return {
                let offset = 32 + (i as usize) * 4;
                let id = base + i;
                reply[offset..offset + 4].copy_from_slice(&id.to_le_bytes());
            }
            reply
        }
        _ => {
            debug!("Unhandled XC-MISC minor opcode: {minor}");
            Vec::new()
        }
    }
}

/// Handle X Present extension requests (major opcode 148).
fn handle_present_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("Present minor opcode: {minor}");

    match minor {
        // QueryVersion
        0 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&1u32.to_le_bytes()); // major version
            reply[12..16].copy_from_slice(&2u32.to_le_bytes()); // minor version
            reply.to_vec()
        }
        // Pixmap (PresentPixmap) — the critical operation
        1 => {
            if data.len() < 72 {
                debug!("PresentPixmap: request too short ({} bytes)", data.len());
                return Vec::new();
            }
            let window = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let pixmap = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            let serial = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
            let x_off = i16::from_le_bytes([data[24], data[25]]);
            let y_off = i16::from_le_bytes([data[26], data[27]]);

            info!(
                "PresentPixmap: window={:#x} pixmap={:#x} serial={} x_off={} y_off={}",
                window, pixmap, serial, x_off, y_off
            );

            // Copy pixels from the source pixmap to the destination window.
            // We need to clone the pixel data first because we can't borrow
            // both the pixmap and window framebuffers simultaneously.
            // Sync SHM pixmaps before reading
            state.sync_shm_pixmap(pixmap);

            let src_info = {
                let resolved = state.resolve_drawable(pixmap);
                if let Some(win) = state.windows.get(&resolved) {
                    Some((
                        win.framebuffer.width() as u16,
                        win.framebuffer.height() as u16,
                        win.framebuffer.data().to_vec(),
                        24u8,
                    ))
                } else if let Some(pix) = state.pixmaps.get(&resolved) {
                    Some((
                        pix.framebuffer.width() as u16,
                        pix.framebuffer.height() as u16,
                        pix.framebuffer.data().to_vec(),
                        pix._depth,
                    ))
                } else {
                    debug!("PresentPixmap: source pixmap {:#x} not found", pixmap);
                    None
                }
            };

            if let Some((src_w, src_h, mut src_data, src_depth)) = src_info {
                // Debug: count non-black pixels in source

                // For depth-1 pixmaps, convert 1-bit values to proper RGB:
                // pixel != 0 → white (0xFFFFFF), pixel == 0 → black (0x000000)
                if src_depth <= 1 {
                    for i in (0..src_data.len()).step_by(4) {
                        if i + 3 < src_data.len() {
                            let is_set = src_data[i] != 0 || src_data[i + 1] != 0 || src_data[i + 2] != 0;
                            let val = if is_set { 0xFF } else { 0x00 };
                            src_data[i] = val;     // B
                            src_data[i + 1] = val; // G
                            src_data[i + 2] = val; // R
                            src_data[i + 3] = 0xFF;
                        }
                    }
                }
                // Determine the target window and offset for rendering.
                // If the target is a child window, propagate pixels up to the
                // parent (top-level) window so the frontend sees them.
                let (target_wid, total_x_off, total_y_off) = {
                    let mut wid = window;
                    let mut tx = x_off as i32;
                    let mut ty = y_off as i32;
                    // Walk up the parent chain to the top-level window
                    for _ in 0..10 {
                        let parent = state.windows.get(&wid).map(|w| w.parent);
                        match parent {
                            Some(p) if p != state.root_window && p != 0 => {
                                // Add this window's position relative to its parent
                                if let Some(w) = state.windows.get(&wid) {
                                    tx += w.x as i32;
                                    ty += w.y as i32;
                                }
                                wid = p;
                            }
                            _ => break,
                        }
                    }
                    (wid, tx as i16, ty as i16)
                };

                // Copy to the child window (keeps its framebuffer up-to-date)
                if let Some(win) = state.windows.get_mut(&window) {
                    win.framebuffer.put_image(x_off, y_off, src_w, src_h, &src_data);
                }

                // Also copy to the top-level parent so the frontend displays it
                if target_wid != window {
                    if let Some(parent_win) = state.windows.get_mut(&target_wid) {
                        parent_win.framebuffer.put_image(total_x_off, total_y_off, src_w, src_h, &src_data);
                        info!(
                            "PresentPixmap: propagated {}x{} from child {:#x} to parent {:#x} at ({},{})",
                            src_w, src_h, window, target_wid, total_x_off, total_y_off
                        );
                    }
                } else {
                    info!(
                        "PresentPixmap: copied {}x{} to window {:#x}",
                        src_w, src_h, window
                    );
                }

                if !state.windows.contains_key(&window) {
                    debug!("PresentPixmap: destination window {:#x} not found", window);
                }
            }

            // Send PresentCompleteNotify if the client subscribed via SelectInput
            let matching_subs: Vec<(u32, u32)> = state
                .present_subscriptions
                .iter()
                .filter(|(_, sub)| sub.window == window && (sub.event_mask & 1) != 0)
                .map(|(&eid, sub)| (eid, sub.window))
                .collect();

            for (event_id, _win) in matching_subs {
                // GenericEvent format for PresentCompleteNotify
                let mut event = [0u8; 32];
                event[0] = 35; // GenericEvent
                event[1] = 148; // Present extension major opcode
                event[2..4].copy_from_slice(&seq.to_le_bytes());
                // event[4..8] = 0 (no extra data beyond 32 bytes)
                event[8..10].copy_from_slice(&1u16.to_le_bytes()); // CompleteNotify event type
                // event[10..12] = pad
                event[12..16].copy_from_slice(&event_id.to_le_bytes()); // event_id
                event[16..20].copy_from_slice(&window.to_le_bytes()); // window
                event[20..24].copy_from_slice(&serial.to_le_bytes()); // serial
                event[24] = 0; // kind = Pixmap
                event[25] = 0; // mode = Copy
                state.pending_events.push(event.to_vec());
            }

            Vec::new() // PresentPixmap has no reply
        }
        // NotifyMSC
        2 => {
            // Stub: we don't track MSC, just ignore
            debug!("PresentNotifyMSC: stub");
            Vec::new()
        }
        // SelectInput
        3 => {
            if data.len() >= 16 {
                let event_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let window = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let event_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

                debug!(
                    "PresentSelectInput: event_id={:#x} window={:#x} event_mask={:#x}",
                    event_id, window, event_mask
                );

                if event_mask == 0 {
                    // Unsubscribe
                    state.present_subscriptions.remove(&event_id);
                } else {
                    state.present_subscriptions.insert(
                        event_id,
                        PresentSubscription { window, event_mask },
                    );
                }
            }
            Vec::new() // SelectInput has no reply
        }
        // QueryCapabilities
        4 => {
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&0u32.to_le_bytes()); // capabilities = none
            reply.to_vec()
        }
        _ => {
            debug!("Unhandled Present minor opcode: {minor}");
            Vec::new()
        }
    }
}
