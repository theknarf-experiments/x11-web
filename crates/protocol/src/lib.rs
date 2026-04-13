use serde::{Deserialize, Serialize};

/// Serde helper to encode Vec<u8> as base64 string in JSON.
mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Messages sent from the backend to a sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BackendToSidecar {
    /// Spawn a new process on the sidecar.
    SpawnProcess {
        request_id: String,
        command: String,
        args: Vec<String>,
    },
    /// Kill a running process.
    KillProcess { request_id: String, pid: u32 },
    /// Request list of running processes.
    ListProcesses { request_id: String },
    /// Forward input event from a frontend user.
    InputEvent {
        window_id: String,
        event: InputEvent,
    },
    /// Request a full redraw (sends Expose events to all windows).
    RequestRedraw { window_id: String },
    /// Resize a specific window.
    ResizeWindow {
        window_id: String,
        width: u16,
        height: u16,
    },
    /// Request clipboard data from X11 selection.
    RequestClipboard {
        selection: String,
        mime_type: String,
    },
    /// Set clipboard content from browser.
    SetClipboard {
        selection: String,
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Resize the virtual screen (RandR-driven).
    ResizeScreen { width: u16, height: u16 },
}

/// Messages sent from a sidecar to the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SidecarToBackend {
    /// Sidecar announces itself.
    Register { sidecar_name: String },
    /// Heartbeat to keep connection alive.
    Heartbeat,
    /// Response to a SpawnProcess command.
    ProcessSpawned { request_id: String, pid: u32 },
    /// Response to a KillProcess command.
    ProcessKilled { request_id: String, pid: u32 },
    /// A process exited on its own.
    ProcessExited { pid: u32, exit_code: Option<i32> },
    /// List of running processes.
    ProcessList {
        request_id: String,
        processes: Vec<ProcessInfo>,
    },
    /// An X11 client connected and was associated with a spawned process.
    ProcessConnected {
        pid: u32,
        client_id: String,
        command: String,
    },
    /// Display update from the X server.
    DisplayUpdate {
        client_id: String,
        update: DisplayUpdate,
    },
    /// Error response.
    Error {
        request_id: Option<String>,
        message: String,
    },
    /// An input event arrived for a window UUID that the sidecar's
    /// router has no entry for. The event is dropped on the floor;
    /// this notification lets the frontend surface that fact instead
    /// of leaving the user wondering why their app stopped responding.
    InputDropped { window_id: String, reason: String },
    /// Clipboard data in response to RequestClipboard.
    ClipboardData {
        selection: String,
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// X11 clipboard content changed (new selection owner).
    ClipboardOffer {
        selection: String,
        mime_types: Vec<String>,
    },
}

/// Messages sent from the backend to a frontend client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BackendToFrontend {
    /// Current list of connected sidecars.
    SidecarList { sidecars: Vec<SidecarInfo> },
    /// A sidecar connected.
    SidecarConnected { sidecar: SidecarInfo },
    /// A sidecar disconnected.
    SidecarDisconnected { sidecar_id: String },
    /// Response to a spawn/kill command.
    CommandResult {
        request_id: String,
        success: bool,
        message: String,
    },
    /// Process list for a sidecar.
    ProcessList {
        sidecar_id: String,
        processes: Vec<ProcessInfo>,
    },
    /// A process exited.
    ProcessExited {
        sidecar_id: String,
        pid: u32,
        exit_code: Option<i32>,
    },
    /// An X11 client connected and was associated with a spawned process.
    ProcessConnected {
        sidecar_id: String,
        pid: u32,
        client_id: String,
        command: String,
    },
    /// Display update forwarded from a sidecar.
    DisplayUpdate {
        sidecar_id: String,
        client_id: String,
        update: DisplayUpdate,
    },
    /// Initial list of all currently connected processes (sent on frontend connect).
    ConnectedProcessesList {
        processes: Vec<ConnectedProcessInfo>,
    },
    /// Initial window state for all windows (sent on frontend connect).
    WindowStateList { windows: Vec<WindowState> },
    /// A window's state changed (position/color, from another frontend).
    WindowStateChanged {
        client_id: String,
        x: f64,
        y: f64,
        color: String,
    },
    /// Forwarded from `SidecarToBackend::InputDropped`. Tells the
    /// frontend that an input event it sent was discarded by the
    /// sidecar's router (e.g. the window UUID is stale because the
    /// X11 client closed the window between send and route lookup).
    InputDropped {
        sidecar_id: String,
        window_id: String,
        reason: String,
    },
    /// Clipboard content from sidecar.
    ClipboardData {
        sidecar_id: String,
        selection: String,
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// X11 clipboard content changed.
    ClipboardOffer {
        sidecar_id: String,
        selection: String,
        mime_types: Vec<String>,
    },
}

/// Messages sent from a frontend client to the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FrontendToBackend {
    /// Spawn a process on a specific sidecar.
    SpawnProcess {
        request_id: String,
        sidecar_id: String,
        command: String,
        args: Vec<String>,
    },
    /// Kill a process on a specific sidecar.
    KillProcess {
        request_id: String,
        sidecar_id: String,
        pid: u32,
    },
    /// List processes on a sidecar.
    ListProcesses {
        request_id: String,
        sidecar_id: String,
    },
    /// Subscribe to display updates from a sidecar.
    SubscribeDisplay { sidecar_id: String },
    /// Request a full redraw of a window.
    RequestRedraw {
        sidecar_id: String,
        window_id: String,
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
    /// Update a window's position/color (synced across frontends).
    UpdateWindowState {
        client_id: String,
        sidecar_id: String,
        x: f64,
        y: f64,
        color: String,
    },
    /// Request clipboard content from X11.
    RequestClipboard {
        sidecar_id: String,
        selection: String,
        mime_type: String,
    },
    /// Set clipboard content in X11 from browser.
    SetClipboard {
        sidecar_id: String,
        selection: String,
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Resize the virtual screen on a sidecar (RandR-driven).
    ResizeScreen {
        sidecar_id: String,
        width: u16,
        height: u16,
    },
}

/// Information about a connected process (for initial sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedProcessInfo {
    pub sidecar_id: String,
    pub pid: u32,
    pub client_id: String,
    pub command: String,
}

/// Window state for position/color sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub client_id: String,
    pub sidecar_id: String,
    pub pid: u32,
    pub x: f64,
    pub y: f64,
    pub color: String,
}

/// Information about a connected sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarInfo {
    pub id: String,
    pub name: String,
}

/// Information about a running process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub command: String,
}

/// A display update from the X server to be rendered in the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
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
        #[serde(default)]
        is_top_level: bool,
        #[serde(default)]
        override_redirect: bool,
        /// X11 border width in pixels (drawn around the content area).
        #[serde(default)]
        border_width: u16,
        /// X11 border color (ARGB32).
        #[serde(default)]
        border_pixel: u32,
    },
    /// A window was destroyed.
    WindowDestroyed {
        window_id: String,
    },
    /// A window was mapped (made visible).
    WindowMapped {
        window_id: String,
        #[serde(default)]
        is_top_level: bool,
        #[serde(default)]
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
        #[serde(default)]
        border_width: u16,
        /// X11 border color (ARGB32).
        #[serde(default)]
        border_pixel: u32,
    },
    /// Fill a rectangle.
    FillRect {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        color: u32,
    },
    /// Draw lines.
    DrawLines {
        window_id: String,
        points: Vec<(i16, i16)>,
        color: u32,
        line_width: u16,
    },
    /// Put an image (raw RGBA pixels, base64 encoded for JSON transport).
    PutImage {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Copy an area within a window.
    CopyArea {
        src_window_id: String,
        dst_window_id: String,
        src_x: i16,
        src_y: i16,
        dst_x: i16,
        dst_y: i16,
        width: u16,
        height: u16,
    },
    /// Clear an area.
    ClearArea {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    /// Window title changed (from WM_NAME property).
    TitleChanged {
        window_id: String,
        title: String,
    },
    /// Draw an arc.
    DrawArc {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        angle1: i16,
        angle2: i16,
        filled: bool,
        color: u32,
    },
    CursorChanged {
        window_id: String,
        cursor: String,
    },
    /// Animated cursor with multiple frames (from CreateAnimCursor).
    CursorAnimated {
        window_id: String,
        frames: Vec<AnimCursorFrame>,
    },
    /// Custom cursor bitmap for a window.
    CursorBitmap {
        window_id: String,
        width: u16,
        height: u16,
        hotspot_x: u16,
        hotspot_y: u16,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Cursor confinement state changed (pointer grab with confine_to).
    CursorConfined {
        window_id: String,
        confined: bool,
    },
    /// Window WM state changed (minimize, maximize, fullscreen).
    WindowStateChanged {
        window_id: String,
        state: WindowWmState,
    },
    /// Transient-for relationship set (WM_TRANSIENT_FOR).
    TransientForSet {
        window_id: String,
        parent_window_id: Option<String>,
    },
    /// Drag-and-drop event from X11 (XdndDrop protocol).
    DndEvent {
        window_id: String,
        event: DndEventKind,
    },
    /// Window stacking order changed (raised to top).
    WindowRaised {
        window_id: String,
    },
    /// X11 clipboard content changed (selection owner set).
    ClipboardOffer {
        selection: String,
        mime_types: Vec<String>,
    },
    /// Clipboard data from X11 selection.
    ClipboardData {
        selection: String,
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// Window urgency hint changed (from WM_HINTS UrgencyHint flag).
    WindowUrgent {
        window_id: String,
        urgent: bool,
    },
    /// Window icon changed (from WM_HINTS icon_pixmap or _NET_WM_ICON).
    WindowIconChanged {
        window_id: String,
        width: u16,
        height: u16,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
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
    /// Incremental update for a single menu item — used for things
    /// like enabling / disabling actions or toggling check state
    /// without re-sending the whole tree.
    MenuStateChanged {
        window_id: String,
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enabled: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checked: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

/// One node in a window's menu tree. Common shape across both
/// `org.gtk.Menus` and `com.canonical.dbusmenu`; the sidecar's
/// MenuTracker translates each protocol into this representation
/// before forwarding to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItem {
    /// Stable identifier inside this window's menu tree. The frontend
    /// uses this for React keys and `MenuStateChanged` lookups. Format
    /// is opaque — currently `"<group>.<position>"` for GTK menus and
    /// `"dbm:<id>"` for dbusmenu.
    pub id: String,
    /// Human-readable label. `None` indicates a separator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub kind: MenuItemKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Current check / radio state, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Display form of the keyboard shortcut, e.g. `"Ctrl+Q"`. The
    /// underlying GTK / dbusmenu accelerator string is parsed by the
    /// sidecar before sending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accelerator: Option<String>,
    /// Named icon (typically a freedesktop icon name). Frontend
    /// resolves to its own asset, or skips if unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Action to invoke when this item is activated. `None` for
    /// separators, submenu parents, and items waiting for lazy load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<MenuAction>,
    /// Children for submenus. Empty for leaves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<MenuItem>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuAction {
    /// Namespaced action name. Currently:
    /// - `"app.<name>"` and `"win.<name>"` for `org.gtk.Actions`
    /// - `"dbm:<int>"` for `com.canonical.dbusmenu`
    pub name: String,
    /// GVariant rendered as JSON. The sidecar parses it back into a
    /// zvariant Value before invoking org.gtk.Actions.Activate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
}

/// A single frame in an animated cursor (from XRender CreateAnimCursor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimCursorFrame {
    /// ARGB pixel data, base64-encoded.
    #[serde(with = "base64_bytes")]
    pub pixels: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    /// Delay in milliseconds before advancing to the next frame.
    pub delay_ms: u32,
}

/// Window management state flags.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WindowWmState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
    /// Request graceful close via WM_DELETE_WINDOW (ICCCM).
    Close,
}

/// Phase of a gesture event (swipe or pinch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GesturePhase {
    Begin,
    Update,
    End,
}

/// Drag-and-drop event kinds mapped from XdndDrop protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
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
        /// Data payload (base64 encoded).
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    /// XdndLeave — the drag left the window.
    Leave,
}

/// Input events sent from the frontend to X11 clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
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
