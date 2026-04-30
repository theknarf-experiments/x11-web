//! High-level Rust enums for sidecar↔backend messages.
//!
//! Wraps the data types from `x11_web_protocol` (`DisplayUpdate`,
//! `InputEvent`, `ProcessInfo`) with envelope variants. Each
//! consumer's `wire_bridge` translates between these and the Cap'n
//! Proto types in `wire_capnp`.
//!
//! No serde derives — the on-wire format is Cap'n Proto, so these
//! enums never see JSON. `Debug` is kept for tracing.

use x11_web_protocol::{DisplayUpdate, InputEvent, ProcessInfo};

/// Messages sent from the backend to a sidecar.
#[derive(Debug)]
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
        data: Vec<u8>,
    },
    /// Resize the virtual screen (RandR-driven).
    ResizeScreen { width: u16, height: u16 },
}

/// Messages sent from a sidecar to the backend.
///
/// Sidecar identity is established by the QUIC `Hello` handshake
/// (see `crate::conn`), not via a `Register` message; nothing here
/// carries the sidecar name.
#[derive(Debug)]
pub enum SidecarToBackend {
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
        data: Vec<u8>,
    },
    /// X11 clipboard content changed (new selection owner).
    ClipboardOffer {
        selection: String,
        mime_types: Vec<String>,
    },
}
