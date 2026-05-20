//! Per-connection client state for X11.

mod drawable;
mod focus;
mod helpers;
mod io;
mod randr_helpers;
mod record;
mod sync_flush;
pub(crate) mod types;
pub(crate) mod xkb_state;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::fonts::FontManager;

use super::atoms::AtomManager;
use super::grab::GrabState;
use super::handlers::sync::SyncState;
use super::types::*;

// Re-export submodule types so external code can use `client::XkbState` etc.
pub(crate) use types::*;
pub(crate) use xkb_state::*;

/// Per-client resource limits. These prevent a single client from exhausting
/// server memory by creating unbounded numbers of X11 resources.
pub(crate) struct ResourceLimits {
    pub(crate) max_windows: usize,
    pub(crate) max_pixmaps: usize,
    pub(crate) max_gcs: usize,
    pub(crate) max_colormaps: usize,
    pub(crate) max_cursors: usize,
    pub(crate) max_pending_events: usize,
    pub(crate) max_motion_history: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_windows: 4096,
            max_pixmaps: 4096,
            max_gcs: 4096,
            max_colormaps: 256,
            max_cursors: 1024,
            max_pending_events: 65536,
            max_motion_history: 256,
        }
    }
}

/// Per-connection state for an X11 client.
pub(crate) struct ClientState {
    pub(crate) client_id: String,
    /// PID of the connected peer process (from SO_PEERCRED), 0 if unknown.
    pub(crate) peer_pid: u32,
    pub(crate) resource_id_base: u32,
    /// Next XID to allocate for XC-MISC GetXIDRange/GetXIDList.
    /// Starts at resource_id_base and increments within the client's ID mask.
    pub(crate) next_xid: u32,
    pub(crate) sequence: u16,
    pub(crate) windows: HashMap<u32, WindowState>,
    pub(crate) shared_windows: SharedWindows,
    /// Window IDs whose local state has changed since the last `sync_windows`
    /// call and therefore need to be pushed back to `shared_windows`. Without
    /// this we'd re-iterate every window on every read tick, which becomes
    /// O(N²) under x11perf-style burst create/destroy workloads.
    pub(crate) shared_dirty_windows: HashSet<u32>,
    pub(crate) pixmaps: HashMap<u32, PixmapState>,
    pub(crate) gcs: HashMap<u32, GcState>,
    pub(crate) atoms: Arc<Mutex<AtomManager>>,
    pub(crate) update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    pub(crate) root_window: u32,
    pub(crate) pointer_x: i16,
    pub(crate) pointer_y: i16,
    pub(crate) focus_window: u32,
    /// Revert-to mode for focus (0=None, 1=PointerRoot, 2=Parent).
    pub(crate) focus_revert_to: u8,
    pub(crate) font_manager: FontManager,
    pub(crate) render: super::handlers::render::RenderState,
    pub(crate) selections: HashMap<u32, u32>,
    /// Timestamps when each selection was acquired (selection atom → timestamp).
    pub(crate) selection_timestamps: HashMap<u32, u32>,
    pub(crate) shm_segments: HashMap<u32, ShmSegment>,
    pub(crate) wm_state: SharedWmState,
    pub(crate) wm_events_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub(crate) event_router: EventRouter,
    pub(crate) shared_selections: SharedSelections,
    pub(crate) damage_regions: HashMap<u32, DamageInfo>,
    pub(crate) present_subscriptions: HashMap<u32, PresentSubscription>,
    pub(crate) pending_events: Vec<Vec<u8>>,
    pub(crate) window_router: WindowRouter,
    pub(crate) message_tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    pub(crate) x11_to_uuid: HashMap<u32, String>,
    pub(crate) cursors: HashMap<u32, String>,
    pub(crate) xi: crate::xinput2::XiState,
    pub(crate) menu: crate::menus::MenuState,
    /// Grab state: pointer grabs, keyboard grabs, passive grabs.
    pub(crate) grabs: GrabState,
    /// SaveSet: windows to reparent to root on client disconnect (used by WMs).
    pub(crate) save_set: Vec<u32>,
    /// Close-down mode for this client (0=Destroy, 1=RetainPermanent, 2=RetainTemporary).
    pub(crate) close_down_mode: u8,
    /// Atomic mirror of `close_down_mode`, shared with the disconnect cleanup
    /// guard so it can decide whether to tear down shared resources without
    /// touching ClientState (which is not Sync).
    pub(crate) close_down_mode_atomic: std::sync::Arc<std::sync::atomic::AtomicU8>,
    /// Set to true by the explicit `n == 0` cleanup path so the disconnect
    /// guard skips its fallback sweep.
    pub(crate) disconnect_cleanup_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Last window the pointer entered (for crossing events).
    pub(crate) last_entered_window: u32,
    /// Currently pressed keys (for QueryKeymap).
    pub(crate) pressed_keys: [u8; 32],
    /// Server start time for generating timestamps.
    pub(crate) server_start: std::time::Instant,
    /// Keyboard control settings.
    pub(crate) keyboard_control: KeyboardControl,
    /// Pointer control settings.
    pub(crate) pointer_control: PointerControl,
    /// Screen-saver state (core SetScreenSaver settings + MIT-SCREEN-SAVER fields).
    pub(crate) screen_saver: ScreenSaverState,
    /// Client byte order: false = LSB-first (0x6c), true = MSB-first (0x42).
    pub(crate) msb_first: bool,
    /// Current screen dimensions (dynamic, updated on resize).
    pub(crate) screen_width: u16,
    pub(crate) screen_height: u16,
    /// RANDR extension state (CRTCs, outputs, modes, monitors, event mask).
    pub(crate) randr: super::handlers::randr::RandRState,
    /// XFIXES extension state (regions, barriers, cursor subscribers, …).
    pub(crate) xfixes: super::handlers::xfixes::XFixesState,
    /// Pending INCR (incremental) selection transfers.
    pub(crate) incr_transfers: Vec<IncrTransfer>,
    /// Windows retained from clients that disconnected with close_down_mode = RetainTemporary.
    pub(crate) retained_temporary_windows: Vec<u32>,
    /// Colormaps: maps colormap ID to colormap state.
    pub(crate) colormaps: HashMap<u32, ColormapState>,
    /// Set of currently installed colormap IDs (for ListInstalledColormaps).
    pub(crate) installed_colormaps: std::collections::HashSet<u32>,
    /// Cursor metadata for RecolorCursor.
    pub(crate) cursor_info: HashMap<u32, CursorInfo>,
    /// SYNC extension state: counters, alarms, fences.
    pub(crate) sync_state: SyncState,
    /// File descriptors received via SCM_RIGHTS (for SHM AttachFd, etc.).
    pub(crate) pending_fds: Vec<i32>,
    /// File descriptors to send back to client via SCM_RIGHTS (for SHM CreateSegment).
    pub(crate) reply_fds: Vec<i32>,
    /// Motion history buffer (circular): (timestamp_ms, x, y).
    pub(crate) motion_history: Vec<(u32, i16, i16)>,
    /// Pointer button mapping (button 1-7 -> mapped button).
    pub(crate) pointer_mapping: [u8; 7],
    /// Modifier mapping: 8 modifiers x N keycodes.
    pub(crate) modifier_map: Vec<Vec<u8>>,
    /// Window gravity values (stored per window ID).
    pub(crate) win_gravity: HashMap<u32, u8>,
    /// Bit gravity values (stored per window ID).
    pub(crate) bit_gravity: HashMap<u32, u8>,
    /// Custom keycode→keysym mapping (ChangeKeyboardMapping).
    /// Key = keycode, value = list of keysyms for that keycode.
    /// Server-wide (shared across connections) per the X11 spec —
    /// `xmodmap` from one client must be observable from another.
    pub(crate) custom_keymap: super::types::SharedKeymap,
    /// Current cursor serial (incremented on cursor change).
    pub(crate) cursor_serial: u32,
    /// Current cursor ID (for GetCursorImage).
    pub(crate) current_cursor: u32,
    /// DBE extension state.
    pub(crate) dbe: super::handlers::dbe::DbeState,
    /// RECORD extension state.
    pub(crate) record: super::handlers::record::RecordState,
    /// GLX extension state.
    pub(crate) glx: super::handlers::glx::GlxState,
    /// SECURITY extension state.
    pub(crate) security: super::handlers::security::SecurityState,
    /// Font search path (SetFontPath/GetFontPath).
    pub(crate) font_path: Vec<String>,
    /// XTEST extension state.
    pub(crate) xtest: super::handlers::xtest::XTestState,
    /// DPMS extension state.
    pub(crate) dpms: super::handlers::dpms::DpmsState,
    /// XKB extension state (modifier/group tracking, controls, indicator
    /// maps, key overrides, compat map, …).
    pub(crate) xkb: XkbState,
    /// XVideo extension state.
    pub(crate) xvideo: super::handlers::xvideo::XVideoState,
    /// Current pointer button mask (bits 8-12 for buttons 1-5).
    pub(crate) pointer_button_mask: u16,
    /// POINTER_MOTION_HINT_MASK: when true, motion events are suppressed
    /// until QueryPointer/GetMotionEvents or button/crossing event occurs.
    pub(crate) motion_hint_suppressed: bool,
    /// Present extension: monotonically increasing media stream counter per-CRTC.
    pub(crate) present_msc: u64,
    /// Channel for clipboard events (selection ownership changes, data responses).
    pub(crate) clipboard_notify_tx: Option<mpsc::UnboundedSender<()>>,
    /// Persistent clipboard data saved when a clipboard owner disconnects.
    pub(crate) persistent_clipboard: super::types::PersistentClipboard,
    /// Shared pixmap registry for cross-connection drawable access.
    pub(crate) shared_pixmaps: super::types::SharedPixmaps,
    /// Shared pixmap framebuffers for cross-connection drawing.
    pub(crate) shared_pixmap_fbs: super::types::SharedPixmapFbs,
    /// Shared GC registry for cross-connection GC access.
    pub(crate) shared_gcs: super::types::SharedGcs,
    /// Shared registry of connected client resource bases (for X-Resource).
    pub(crate) client_registry: super::types::SharedClientRegistry,
    /// Globally-shared pointer `(x, y)` so a FakeInput from client A is
    /// visible to client B's next QueryPointer. Sync helpers live on
    /// `ClientState` (`set_pointer`, `refresh_pointer_from_shared`).
    pub(crate) shared_pointer: super::types::SharedPointer,
    /// Globally-shared focus window — xdotool's `windowfocus xterm`
    /// must be visible to xterm's own `GetInputFocus`. Sync helpers
    /// (`set_focus_shared`, `refresh_focus_from_shared`) live on
    /// `ClientState`.
    pub(crate) shared_focus: super::types::SharedFocus,
    /// Cross-connection event broadcaster for per-window event subscriptions.
    pub(crate) event_broadcaster: super::types::EventBroadcaster,
    /// Shared server grab lock (GrabServer/UngrabServer across all clients).
    pub(crate) server_grab: super::types::ServerGrabLock,

    /// XFree86-VidMode extension state.
    pub(crate) vidmode: super::handlers::vidmode::VidModeState,
    /// Whether the client has enabled BIG-REQUESTS via BigReqEnable.
    pub(crate) big_requests_enabled: bool,
    /// Freed resource IDs available for reuse via XC-MISC GetXIDRange/GetXIDList.
    pub(crate) freed_xids: Vec<u32>,
    /// Built-in XIM (X Input Method) server state.
    pub(crate) xim: super::handlers::xim::XimServer,
    /// Composite extension state.
    pub(crate) composite: super::handlers::composite::CompositeState,
    /// Shared extension registry — single source of truth for which extensions
    /// are available, their opcodes, event/error bases, and enabled state.
    pub(crate) extension_registry: std::sync::Arc<super::extensions::ExtensionRegistry>,
    /// Per-client resource limits to prevent memory exhaustion.
    pub(crate) resource_limits: ResourceLimits,
}
