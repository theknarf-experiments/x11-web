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

use std::collections::HashMap;
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

/// Per-connection state for an X11 client.
pub(crate) struct ClientState {
    pub(crate) client_id: String,
    pub(crate) resource_id_base: u32,
    /// Next XID to allocate for XC-MISC GetXIDRange/GetXIDList.
    /// Starts at resource_id_base and increments within the client's ID mask.
    pub(crate) next_xid: u32,
    pub(crate) sequence: u16,
    pub(crate) windows: HashMap<u32, WindowState>,
    pub(crate) shared_windows: SharedWindows,
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
    pub(crate) menu_tracker: crate::menus::MenuTracker,
    pub(crate) gtk_menu_paths: HashMap<u32, crate::menus::GtkMenuPaths>,
    /// Grab state: pointer grabs, keyboard grabs, passive grabs.
    pub(crate) grabs: GrabState,
    /// SaveSet: windows to reparent to root on client disconnect (used by WMs).
    pub(crate) save_set: Vec<u32>,
    /// Close-down mode for this client (0=Destroy, 1=RetainPermanent, 2=RetainTemporary).
    pub(crate) close_down_mode: u8,
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
    /// Screen saver settings.
    pub(crate) screen_saver: ScreenSaverSettings,
    /// MIT-SCREEN-SAVER: event mask for ScreenSaverNotify.
    pub(crate) screen_saver_event_mask: u32,
    /// MIT-SCREEN-SAVER: window ID for the screen saver window.
    pub(crate) screen_saver_window: u32,
    /// MIT-SCREEN-SAVER: stored attributes from SetAttributes.
    pub(crate) screen_saver_attrs: Option<super::handlers::misc_ext::ScreenSaverAttrs>,
    /// MIT-SCREEN-SAVER: reference-counted suspend level.
    pub(crate) screen_saver_suspend_count: u32,
    /// Client byte order: false = LSB-first (0x6c), true = MSB-first (0x42).
    pub(crate) msb_first: bool,
    /// Current screen dimensions (dynamic, updated on resize).
    pub(crate) screen_width: u16,
    pub(crate) screen_height: u16,
    /// RandR configuration timestamp (incremented on screen changes).
    pub(crate) randr_config_timestamp: u32,
    /// XFIXES regions.
    pub(crate) xfixes_regions: HashMap<u32, XFixesRegion>,
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
    /// Pointer button mapping (button 1-5 -> mapped button).
    pub(crate) pointer_mapping: [u8; 5],
    /// Modifier mapping: 8 modifiers x N keycodes.
    pub(crate) modifier_map: Vec<Vec<u8>>,
    /// Window gravity values (stored per window ID).
    pub(crate) win_gravity: HashMap<u32, u8>,
    /// Bit gravity values (stored per window ID).
    pub(crate) bit_gravity: HashMap<u32, u8>,
    /// Custom keycode→keysym mapping (ChangeKeyboardMapping).
    /// Key = keycode, value = list of keysyms for that keycode.
    pub(crate) custom_keymap: HashMap<u8, Vec<u32>>,
    /// XFIXES: clients subscribed to cursor change events (window_id → bool).
    pub(crate) cursor_event_subscribers: HashMap<u32, bool>,
    /// XFIXES: clients subscribed to selection events (selection_atom → event_mask).
    pub(crate) selection_event_subscribers: HashMap<u32, u32>,
    /// Current cursor serial (incremented on cursor change).
    pub(crate) cursor_serial: u32,
    /// Current cursor ID (for GetCursorImage).
    pub(crate) current_cursor: u32,
    /// XFIXES: cursor is hidden (HideCursor/ShowCursor nesting count).
    pub(crate) cursor_hidden: u32,
    /// DBE: back buffer allocations (back_buffer_id → window_id).
    pub(crate) back_buffers: HashMap<u32, u32>,
    /// DBE: idiom nesting depth (BeginIdiom/EndIdiom).
    pub(crate) dbe_idiom_depth: u32,
    /// RECORD extension: recording contexts (local, owned by this client).
    pub(crate) record_contexts: HashMap<u32, super::handlers::record::RecordContext>,
    /// Shared RECORD contexts for cross-connection interception.
    pub(crate) shared_record_contexts: SharedRecordContexts,
    /// GLX extension state.
    pub(crate) glx: super::handlers::glx::GlxState,
    /// SECURITY extension: authorization tokens (local to this client's session).
    pub(crate) security_authorizations: HashMap<u32, SecurityAuthorization>,
    /// Shared SECURITY tokens for cross-connection validation.
    pub(crate) shared_security_tokens: super::types::SharedSecurityTokens,
    /// Trust level for this client (0 = trusted, 1 = untrusted).
    /// Set during connection auth if a SECURITY-generated token was used.
    pub(crate) trust_level: u32,
    /// Font search path (SetFontPath/GetFontPath).
    pub(crate) font_path: Vec<String>,
    /// Access control list (ChangeHosts/ListHosts) — now backed by shared server-wide state.
    pub(crate) access_hosts: Vec<AccessHost>,
    /// Whether access control is enabled.
    pub(crate) access_control_enabled: bool,
    /// Shared server-wide access control state (for enforcement on new TCP connections).
    pub(crate) shared_access_control: super::types::SharedAccessControl,
    /// XTEST: impervious grab mode.
    pub(crate) xtest_grab_impervious: bool,
    /// DPMS: whether DPMS is enabled.
    pub(crate) dpms_enabled: bool,
    /// DPMS: current power level (0=On, 1=Standby, 2=Suspend, 3=Off).
    pub(crate) dpms_power_level: u16,
    /// DPMS: standby timeout in seconds.
    pub(crate) dpms_standby_timeout: u16,
    /// DPMS: suspend timeout in seconds.
    pub(crate) dpms_suspend_timeout: u16,
    /// DPMS: off timeout in seconds.
    pub(crate) dpms_off_timeout: u16,
    /// XKB modifier/group state and controls.
    pub(crate) xkb_state: XkbState,
    /// XKB extra keyboard groups/layouts (groups 1-3). Each entry is a
    /// HashMap<keycode, Vec<keysym>> for that group. Group 0 is the built-in
    /// US-QWERTY layout from `keycode_to_keysym`.
    pub(crate) xkb_extra_groups: Vec<HashMap<u8, Vec<u32>>>,
    /// XKB indicator state (32 bits, one per named indicator).
    pub(crate) xkb_indicators: u32,
    /// XKB indicator maps: indicator_index → (which_groups, groups, which_mods, mods).
    pub(crate) xkb_indicator_maps: Vec<XkbIndicatorMap>,
    /// XKB group-switch keys: (keycode, target_group_index).
    /// When multi-layout is active, pressing these keys switches the active group.
    pub(crate) xkb_group_switch_keys: Vec<(u8, u8)>,
    /// XKB symbolic names stored by SetNames (which_bit → atom).
    /// Bits: 0=Keycodes, 1=Geometry, 2=Symbols, 3=PhysSymbols, 4=Types, 5=Compat.
    pub(crate) xkb_names_atoms: HashMap<u8, u32>,
    /// XKB: per-type name atoms (overridden by SetNames).
    pub(crate) xkb_type_names: Vec<u32>,
    /// XKB: per-type per-level name atoms (overridden by SetNames).
    pub(crate) xkb_kt_level_names: Vec<Vec<u32>>,
    /// XKB: group name atoms (overridden by SetNames).
    pub(crate) xkb_group_names: Vec<u32>,
    /// XKB: indicator name atoms (overridden by SetNames).
    pub(crate) xkb_indicator_name_atoms: Vec<u32>,
    /// XKB: virtual modifier name atoms (overridden by SetNames).
    pub(crate) xkb_vmod_names: Vec<u32>,
    /// XKB: per-key name overrides (overridden by SetNames).
    pub(crate) xkb_key_names: HashMap<u8, [u8; 4]>,
    /// XKB: key alias pairs (overridden by SetNames).
    pub(crate) xkb_key_aliases: Vec<([u8; 4], [u8; 4])>,
    /// XKB: custom key types set by SetMap (keyed by type index).
    pub(crate) xkb_key_types: HashMap<u8, XkbKeyType>,
    /// XKB: per-key action lists set by SetMap (keyed by keycode).
    pub(crate) xkb_key_actions: HashMap<u8, Vec<XkbAction>>,
    /// XKB: per-key behaviors set by SetMap (keyed by keycode).
    pub(crate) xkb_key_behaviors: HashMap<u8, XkbKeyBehavior>,
    /// XKB: per-key explicit override flags set by SetMap (keyed by keycode).
    pub(crate) xkb_explicit: HashMap<u8, u8>,
    /// XKB: per-key modifier map set by SetMap (keyed by keycode).
    pub(crate) xkb_modmap: HashMap<u8, u8>,
    /// XKB: per-key virtual modifier map set by SetMap (keyed by keycode).
    pub(crate) xkb_vmodmap: HashMap<u8, u16>,
    /// XKB: virtual modifier bindings (16 entries for mod1-mod16).
    pub(crate) xkb_vmod_bindings: [u8; 16],
    /// XKB: per-button action mappings set by SetDeviceInfo (keyed by button index).
    pub(crate) xkb_button_actions: HashMap<u8, [u8; 8]>,
    /// XKB: LED feedback info blob from SetDeviceInfo (echoed by GetDeviceInfo).
    pub(crate) xkb_device_led_info: Vec<u8>,
    /// XKB: compatibility map — symbol interpretations (SI entries).
    /// Populated with defaults, overridable via SetCompatMap.
    pub(crate) xkb_compat_si: Vec<XkbSymInterpretation>,
    /// XKB: group compatibility entries (4 groups).
    pub(crate) xkb_group_compat: [XkbGroupCompat; 4],
    /// XKB: per-client event mask (SelectEvents). Bitmask of XKB event types this
    /// client wants to receive. Bit positions correspond to XkbEventType values
    /// (e.g., bit 0 = NewKeyboardNotify, bit 1 = MapNotify, bit 11 = StateNotify).
    pub(crate) xkb_event_mask: u32,
    /// XKB: named indicator settings set by SetNamedIndicator.
    /// Maps indicator name atom to (indicator_index, change_state, led_state,
    ///   affect_which, change_which, affect_map_mask, map).
    pub(crate) xkb_named_indicators: HashMap<u32, XkbNamedIndicator>,
    /// XVideo port state: per-port attributes and allocation tracking.
    pub(crate) xv_ports: HashMap<u32, super::handlers::xvideo::XvPortState>,
    /// Set of drawable IDs that have registered for XvVideoNotify events
    /// (via XvSelectVideoNotify). Used to deliver VideoNotify when video
    /// operations complete on those drawables.
    pub(crate) xv_video_notify_drawables: std::collections::HashSet<u32>,
    /// Set of port IDs that have registered for XvPortNotify events
    /// (via XvSelectPortNotify). Used to deliver PortNotify when port
    /// attributes change.
    pub(crate) xv_port_notify_ports: std::collections::HashSet<u32>,
    /// Current pointer button mask (bits 8-12 for buttons 1-5).
    pub(crate) pointer_button_mask: u16,
    /// POINTER_MOTION_HINT_MASK: when true, motion events are suppressed
    /// until QueryPointer/GetMotionEvents or button/crossing event occurs.
    pub(crate) motion_hint_suppressed: bool,
    /// XFIXES pointer barriers.
    pub(crate) barriers: HashMap<u32, PointerBarrier>,
    /// XFIXES client disconnect mode (0 = default).
    pub(crate) disconnect_mode: u32,
    /// Present extension: monotonically increasing media stream counter per-CRTC.
    pub(crate) present_msc: u64,
    /// Channel for clipboard events (selection ownership changes, data responses).
    pub(crate) clipboard_notify_tx: Option<mpsc::UnboundedSender<super::types::ClipboardEvent>>,
    /// Server-side clipboard data (set by backend for browser → X11 paste).
    #[allow(dead_code)]
    pub(crate) shared_clipboard: super::types::SharedClipboard,
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
    /// Cross-connection event broadcaster for per-window event subscriptions.
    pub(crate) event_broadcaster: super::types::EventBroadcaster,
    /// Shared server grab lock (GrabServer/UngrabServer across all clients).
    pub(crate) server_grab: super::types::ServerGrabLock,

    // -----------------------------------------------------------------------
    // RandR multi-monitor model
    // -----------------------------------------------------------------------
    /// RandR CRTCs (display controllers).
    pub(crate) randr_crtcs: Vec<RandrCrtc>,
    /// RandR outputs (connectors).
    pub(crate) randr_outputs: Vec<RandrOutput>,
    /// RandR modes (resolutions/timings).
    pub(crate) randr_modes: Vec<RandrMode>,
    /// RandR providers.
    pub(crate) randr_providers: Vec<RandrProvider>,
    /// RandR event selection mask for this client.
    pub(crate) randr_event_mask: u32,
    /// RandR 1.5 monitor definitions (set by RRSetMonitor).
    pub(crate) randr_monitors: Vec<RandrMonitor>,
    /// Primary output ID (0 = none / use first). Set by RRSetOutputPrimary.
    pub(crate) randr_primary_output: u32,
    /// Next RandR mode ID for RRCreateMode.
    pub(crate) randr_next_mode_id: u32,
    /// XFree86-VidMode viewport X offset (set by XF86VidModeSetViewPort).
    /// For our single virtual display this is always clamped to 0.
    pub(crate) vidmode_viewport_x: u32,
    /// XFree86-VidMode viewport Y offset (set by XF86VidModeSetViewPort).
    /// For our single virtual display this is always clamped to 0.
    pub(crate) vidmode_viewport_y: u32,
    /// XFree86-VidMode: list of available video modes.
    pub(crate) vidmode_modes: Vec<super::handlers::misc_ext::VidModeInfo>,
    /// XFree86-VidMode: whether mode switching is locked.
    pub(crate) vidmode_locked: bool,
    /// XFree86-VidMode: index of the current mode in `vidmode_modes`.
    pub(crate) vidmode_current_mode: usize,
    /// Whether the client has enabled BIG-REQUESTS via BigReqEnable.
    pub(crate) big_requests_enabled: bool,
    /// Built-in XIM (X Input Method) server state.
    pub(crate) xim: super::handlers::xim::XimServer,
    /// DRI3: DRM device in use by this client (major, minor device numbers).
    pub(crate) dri3_drm_device: Option<(u32, u32)>,
    /// Composite: overlay window reference count (GetOverlayWindow increments,
    /// ReleaseOverlayWindow decrements).
    pub(crate) overlay_ref_count: u32,
}
