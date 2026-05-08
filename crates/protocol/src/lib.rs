// Plain Rust types for the protocol surface. The on-wire format
// is Cap'n Proto on every boundary (see `crates/ws-wire`,
// `crates/wire`, `crates/rtc-wire`); these enums are just the
// typed shape that bridges convert to/from. No serde derives, no
// JSON-survival helpers.

/// Messages sent from the backend to a frontend client.
#[derive(Debug, Clone)]
pub enum BackendToFrontend {
    /// Current list of connected sidecars. Sent on initial frontend
    /// connect *and* whenever a sidecar joins or leaves — the frontend
    /// always reconciles against the full list rather than tracking
    /// incremental add/remove events.
    SidecarList { sidecars: Vec<SidecarInfo> },
    /// The active workspace for this frontend session. Sent in
    /// response to a `FrontendToBackend::OpenWorkspace` — exactly one
    /// workspace per frontend. The frontend should reflect the
    /// returned `id` in its URL hash, since the backend may have
    /// created a fresh workspace if the requested id wasn't found
    /// (e.g. after a backend restart).
    Workspace { workspace: Workspace },
    /// Response to a spawn/kill command.
    CommandResult {
        request_id: String,
        success: bool,
        message: String,
    },
    /// Authoritative list of X11-connected processes for one sidecar.
    /// Sent on initial frontend connect (one per sidecar), whenever a
    /// process connects or exits, and on sidecar disconnect (with an
    /// empty list). The frontend reconciles against the full list per
    /// sidecar rather than tracking incremental connect/exit events.
    ProcessList {
        sidecar_id: String,
        processes: Vec<ProcessInfo>,
    },
    /// Per-window update. Each variant identifies the affected window
    /// by its globally-unique `window_id`; no envelope-level routing
    /// metadata is needed.
    WindowUpdate { update: WindowUpdate },
    /// Authoritative list of all top-level / override-redirect windows
    /// that are currently mapped. Vec order = stacking order, last
    /// item on top. Each descriptor carries the position the frontend
    /// should render at (X11 authoritative for popups; cross-frontend
    /// tracked position for top-level windows when one exists). Sent
    /// on every change to the visible-window set including initial
    /// frontend connect.
    WindowList { windows: Vec<WindowDescriptor> },
    /// X11 bell event — frontend should play an audible/visual bell.
    /// Not per-window; lifted out of the per-window update stream.
    Bell { percent: u8 },

    /// SDP answer in response to a `FrontendToBackend::RtcOffer`.
    /// The WS carries WebRTC signaling; once the peer connection is
    /// up the DataChannel takes over high-frequency traffic.
    RtcAnswer { sdp: String },
    /// Trickled ICE candidate from the backend's `Rtc` driver.
    RtcIceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
}

/// Messages sent from a frontend client to the backend.
#[derive(Debug, Clone)]
pub enum FrontendToBackend {
    /// Bind this frontend to a workspace. `id = Some(uuid)` opens the
    /// existing workspace with that id; if the backend doesn't have
    /// one (e.g. after a restart) it falls back to creating a fresh
    /// workspace and returns the new id in `BackendToFrontend::Workspace`.
    /// `id = None` always creates a new workspace. The frontend must
    /// wait for the `Workspace` reply before driving the canvas, and
    /// should reflect the returned id in its URL hash.
    OpenWorkspace { id: Option<String> },
    /// Spawn a process on a specific sidecar. `workspace_id` is the
    /// frontend's active workspace; the backend remembers it so any
    /// X11 windows the spawned process opens auto-attach only to
    /// that workspace (rather than every open one).
    SpawnProcess {
        request_id: String,
        sidecar_id: String,
        workspace_id: String,
        command: String,
        args: Vec<String>,
    },
    /// Kill a process on a specific sidecar.
    KillProcess {
        request_id: String,
        sidecar_id: String,
        pid: u32,
    },
    /// Send input to a window on a sidecar.
    InputEvent {
        sidecar_id: String,
        window_id: String,
        event: InputEvent,
    },
    /// Resize a specific window on a sidecar.
    ResizeWindow {
        sidecar_id: String,
        window_id: String,
        width: u16,
        height: u16,
    },
    /// SDP offer initiating the WebRTC session — the frontend creates
    /// the peer connection and offer once the WS is up.
    RtcOffer { sdp: String },
    /// Trickled ICE candidate from the browser.
    RtcIceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
}

/// Information about a connected sidecar.
#[derive(Debug, Clone)]
pub struct SidecarInfo {
    pub id: String,
    pub name: String,
}

/// A workspace — a backend-resident container that a future revision
/// will use to hold the set of windows a user has pulled into their
/// canvas (vs. the larger set of windows their sidecars know about
/// but haven't been attached). For now it carries identity only.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,
    pub name: String,
}

/// One X11-connected process under a sidecar. Wrapped in a
/// [`BackendToFrontend::ProcessList`] keyed by `sidecar_id`.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub client_id: String,
    pub command: String,
}

/// One visible window in [`BackendToFrontend::WindowList`]. The
/// backend filters X11 windows down to "top-level *or*
/// override-redirect *and* mapped" before publishing — internal
/// child windows, unmapped windows, etc. never appear here.
///
/// `window_id` is a globally unique UUID — the only identifier the
/// frontend needs for any per-window operation. `sidecar_id`, `pid`,
/// and `command` are auxiliary metadata for UI labels and process
/// control (kill button targets `(sidecar_id, pid)`).
///
/// `override_redirect` distinguishes pop-ups (menus, tooltips) from
/// regular top-level windows. For popups, `(x, y)` is the X server
/// placement and is authoritative. For top-level windows, `(x, y)`
/// is meaningless to the frontend — canvas position lives on the
/// matching `OcifNode` in the workspace doc.
#[derive(Debug, Clone)]
pub struct WindowDescriptor {
    pub window_id: String,
    pub sidecar_id: String,
    pub pid: u32,
    pub command: String,
    pub x: f64,
    pub y: f64,
    pub width: u16,
    pub height: u16,
    pub border_width: u16,
    pub border_pixel: u32,
    pub override_redirect: bool,
    /// Whether the user can drag-resize the window. The frontend
    /// hides resize handles when this is false. Sidecar-determined:
    /// X11 windows are always reported as resizable; macOS sidecars
    /// probe AX (`AXSize` settable on the underlying NSWindow).
    pub resizable: bool,
}

/// Per-window incremental update sent to the frontend over the
/// WebSocket. Pixel rectangles travel over the WebRTC DataChannel as
/// Cap'n Proto frames (see `crates/rtc-wire`); the wire-level X11
/// lifecycle events (Created/Mapped/Unmapped/...) are absorbed by the
/// backend and re-emitted as `BackendToFrontend::WindowList`; what's
/// left here is the smaller set of per-window content/property
/// changes plus the cross-frontend drag delta.
#[derive(Debug, Clone)]
pub enum WindowUpdate {
    /// Window title (from WM_NAME).
    TitleChanged { window_id: String, title: String },
    /// X11 WM state — Normal / Minimized / Maximized / Fullscreen / Close.
    StateChanged {
        window_id: String,
        state: WindowWmState,
    },
    /// Input focus changed; `None` clears focus.
    Focused { window_id: Option<String> },
    /// Full GTK / dbusmenu menu tree mirror. Empty `menu` clears.
    MenuStructure {
        window_id: String,
        menu: Vec<MenuItem>,
    },
}

/// A display update from the X server to be rendered in the browser.
#[derive(Debug, Clone)]
pub enum DisplayUpdate {
    /// A window was created.
    WindowCreated {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        /// True if this is a top-level, non-override-redirect, InputOutput window.
        /// Only these should be shown as WindowFrames in the frontend.
        is_top_level: bool,
        override_redirect: bool,
        /// X11 border width in pixels (drawn around the content area).
        border_width: u16,
        /// X11 border color (ARGB32).
        border_pixel: u32,
        /// Whether the user can drag-resize the window. The frontend
        /// hides resize handles when this is false. Sidecar-determined:
        /// X11 windows are always treated as resizable (window managers
        /// usually honour size hints anyway), macOS sidecars probe AX
        /// to see if `AXSize` is settable on the underlying NSWindow.
        resizable: bool,
    },
    /// A window was destroyed.
    WindowDestroyed {
        window_id: String,
    },
    /// A window was mapped (made visible).
    WindowMapped {
        window_id: String,
        is_top_level: bool,
        override_redirect: bool,
    },
    /// A window was unmapped (hidden).
    WindowUnmapped {
        window_id: String,
    },
    /// A window was moved/resized.
    WindowConfigured {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        /// X11 border width in pixels (drawn around the content area).
        border_width: u16,
        /// X11 border color (ARGB32).
        border_pixel: u32,
        /// See [`WindowCreated::resizable`]; re-emitted on every
        /// configure so apps that flip the constraint at runtime
        /// (rare) reach the frontend.
        resizable: bool,
    },
    /// Put an image — raw RGBA pixels straight on the wire.
    PutImage {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        data: Vec<u8>,
    },
    /// Window title changed (from WM_NAME property).
    TitleChanged {
        window_id: String,
        title: String,
    },
    /// Window WM state changed (minimize, maximize, fullscreen).
    WindowStateChanged {
        window_id: String,
        state: WindowWmState,
    },
    /// Window stacking order changed (raised to top).
    WindowRaised {
        window_id: String,
    },
    /// X11 Bell event — frontend should play an audible/visual bell.
    Bell {
        /// Percent volume (0-100).
        percent: u8,
    },
    /// The X11 input focus changed. `window_id` is the UUID of the
    /// focused top-level window, or `None` if focus was cleared (revert
    /// to root, no window focused). Used by the global menu bar to
    /// know which window's menu to display.
    WindowFocused {
        window_id: Option<String>,
    },
    /// Full menu tree for a top-level window. Sent when the sidecar
    /// first mirrors a GTK / Qt application menu, and again whenever
    /// the structure changes substantially. Empty `menu` clears the
    /// frontend's cache for that window (e.g. on app shutdown).
    MenuStructure {
        window_id: String,
        menu: Vec<MenuItem>,
    },
    /// Low-resolution window preview, WebP-encoded. Sidecar emits
    /// these at low rate (~1 Hz) for windows that aren't yet
    /// attached to the canvas. The backend filters them out of the
    /// WS path and routes the bytes via the WebRTC DataChannel
    /// (see `rtc_codec::encode_window_thumbnail`).
    WindowThumbnail {
        window_id: String,
        width: u16,
        height: u16,
        data: Vec<u8>,
    },
}

/// One node in a window's menu tree. Common shape across both
/// `org.gtk.Menus` and `com.canonical.dbusmenu`; the sidecar's
/// MenuTracker translates each protocol into this representation
/// before forwarding to the frontend.
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// Stable identifier inside this window's menu tree. The frontend
    /// uses this for React keys and `MenuStateChanged` lookups. Format
    /// is opaque — currently `"<group>.<position>"` for GTK menus and
    /// `"dbm:<id>"` for dbusmenu.
    pub id: String,
    /// Human-readable label. `None` indicates a separator.
    pub label: Option<String>,
    pub kind: MenuItemKind,
    pub enabled: bool,
    pub visible: bool,
    /// Current check / radio state, when applicable.
    pub checked: Option<bool>,
    /// Display form of the keyboard shortcut, e.g. `"Ctrl+Q"`. The
    /// underlying GTK / dbusmenu accelerator string is parsed by the
    /// sidecar before sending.
    pub accelerator: Option<String>,
    /// Named icon (typically a freedesktop icon name). Frontend
    /// resolves to its own asset, or skips if unknown.
    pub icon: Option<String>,
    /// Action to invoke when this item is activated. `None` for
    /// separators, submenu parents, and items waiting for lazy load.
    pub action: Option<MenuAction>,
    /// Children for submenus. Empty for leaves.
    pub children: Vec<MenuItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItemKind {
    Normal,
    Submenu,
    Separator,
    Checkbox,
    Radio,
}

/// Identifier for the action a menu item triggers, plus any optional
/// target value (used by GTK actions that take a parameter — e.g.
/// `app.set-mode("scientific")`).
#[derive(Debug, Clone)]
pub struct MenuAction {
    /// Namespaced action name. Currently:
    /// - `"app.<name>"` and `"win.<name>"` for `org.gtk.Actions`
    /// - `"dbm:<int>"` for `com.canonical.dbusmenu`
    pub name: String,
    /// Action payload. Mirrors the GVariant types we actually see in
    /// real-world GTK / Qt menu items.
    pub target: Option<MenuActionTarget>,
}

/// Typed variant of a menu action's GVariant payload. Mirrors the
/// six concrete shapes `crates/sidecar/src/menus.rs` produces from
/// the D-Bus menu service — anything else is translated to `None`
/// upstream so this enum stays small and exhaustive.
#[derive(Debug, Clone)]
pub enum MenuActionTarget {
    String(String),
    Bool(bool),
    Int32(i32),
    UInt32(u32),
    Int64(i64),
    Float64(f64),
}

/// Window management state flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowWmState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
    /// Request graceful close via WM_DELETE_WINDOW (ICCCM).
    Close,
}

/// Phase of a gesture event (swipe or pinch).
#[derive(Debug, Clone)]
pub enum GesturePhase {
    Begin,
    Update,
    End,
}

/// Drag-and-drop event kinds mapped from XdndDrop protocol.
#[derive(Debug, Clone)]
pub enum DndEventKind {
    /// XdndEnter — a drag entered the window.
    Enter {
        /// Available MIME types.
        mime_types: Vec<String>,
    },
    /// XdndPosition — the drag is over the window at a given position.
    Position { x: i16, y: i16 },
    /// XdndDrop — the user dropped.
    Drop {
        /// MIME type of the dropped content.
        mime_type: String,
        /// Raw payload bytes — capnp `Data` on the wire.
        data: Vec<u8>,
    },
    /// XdndLeave — the drag left the window.
    Leave,
}

/// Input events sent from the frontend to X11 clients.
#[derive(Debug, Clone)]
pub enum InputEvent {
    KeyPress {
        keycode: u32,
        state: u16,
    },
    KeyRelease {
        keycode: u32,
        state: u16,
    },
    ButtonPress {
        button: u8,
        x: i16,
        y: i16,
        state: u16,
    },
    ButtonRelease {
        button: u8,
        x: i16,
        y: i16,
        state: u16,
    },
    MotionNotify {
        x: i16,
        y: i16,
        state: u16,
    },
    /// User clicked an item in the global menu bar. Routed by the
    /// existing per-window InputEvent channel; the sidecar's
    /// MenuTracker for that window dispatches the action over DBus.
    MenuActivate {
        action: MenuAction,
    },
    /// Window management action from frontend (minimize/maximize/fullscreen).
    WindowManage {
        action: WindowWmState,
    },
    /// Drag-and-drop event from browser to X11.
    DndBridge {
        event: DndEventKind,
    },
    /// Touch begin event (finger down).
    TouchBegin {
        touch_id: u32,
        x: i16,
        y: i16,
        state: u16,
    },
    /// Touch update event (finger move).
    TouchUpdate {
        touch_id: u32,
        x: i16,
        y: i16,
        state: u16,
    },
    /// Touch end event (finger up).
    TouchEnd {
        touch_id: u32,
        x: i16,
        y: i16,
        state: u16,
    },
    /// Gesture swipe event.
    GestureSwipe {
        phase: GesturePhase,
        fingers: u8,
        dx: f32,
        dy: f32,
    },
    /// Gesture pinch event.
    GesturePinch {
        phase: GesturePhase,
        fingers: u8,
        dx: f32,
        dy: f32,
        scale: f32,
        rotation: f32,
    },
    /// IME composition event from browser.
    CompositionEvent {
        phase: String,
        text: String,
    },
}
