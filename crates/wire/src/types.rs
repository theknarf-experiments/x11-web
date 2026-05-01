//! High-level Rust enums for sidecar↔backend messages.
//!
//! Wraps the data types from `x11_web_protocol` (`DisplayUpdate`,
//! `InputEvent`, `SpawnedProcessInfo`) with envelope variants. Each
//! consumer's `wire_bridge` translates between these and the Cap'n
//! Proto types in `wire_capnp`.
//!
//! No serde derives — the on-wire format is Cap'n Proto, so these
//! enums never see JSON. `Debug` is kept for tracing.

use x11_web_protocol::{DisplayUpdate, InputEvent};

/// On-host process listing returned by a sidecar in response to
/// `ListProcesses`. Wire-only; the frontend-facing protocol uses
/// `protocol::ProcessInfo` (which carries an X11 `client_id`).
#[derive(Debug, Clone)]
pub struct SpawnedProcessInfo {
    pub pid: u32,
    pub command: String,
}

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
    /// Resize a specific window.
    ResizeWindow {
        window_id: String,
        width: u16,
        height: u16,
    },
    /// Start live capture of a specific window. The macOS sidecar
    /// only runs SCStream for windows the backend has explicitly
    /// asked for; X11 streams unconditionally and ignores this.
    StartWindowCapture { window_id: String },
    /// Stop live capture — sent when no workspace has the window
    /// attached anymore.
    StopWindowCapture { window_id: String },
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
        processes: Vec<SpawnedProcessInfo>,
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
