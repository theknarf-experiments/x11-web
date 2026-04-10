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
    },
    /// A window was destroyed.
    WindowDestroyed { window_id: String },
    /// A window was mapped (made visible).
    WindowMapped {
        window_id: String,
        #[serde(default)]
        is_top_level: bool,
    },
    /// A window was unmapped (hidden).
    WindowUnmapped { window_id: String },
    /// A window was moved/resized.
    WindowConfigured {
        window_id: String,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
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
    TitleChanged { window_id: String, title: String },
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
    /// The X11 input focus changed. `window_id` is the UUID of the
    /// focused top-level window, or `None` if focus was cleared (revert
    /// to root, no window focused). Used by the global menu bar to
    /// know which window's menu to display.
    WindowFocused {
        window_id: Option<String>,
    },
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
}
