//! Sidecar ↔ Backend wire protocol: QUIC + Cap'n Proto.
//!
//! Everything flows over a single QUIC connection per sidecar with
//! one bidi stream carrying a sequence of Cap'n Proto messages. The
//! schema lives in `schema/wire.capnp` and the build script
//! regenerates Rust types on every build.
//!
//! Public modules:
//!   - `tls`    — self-signed cert generation + fingerprint pinning.
//!   - `conn`   — `dial` / `listen` plus the `Hello` handshake.
//!   - `types`  — high-level Rust enums (`BackendToSidecar`,
//!     `SidecarToBackend`) that wrap the Cap'n Proto wire types.
//!   - `bridge` — translation between `types` and `wire_capnp`.
//!
//! The generated Cap'n Proto module lives at the crate root as
//! `wire_capnp` so callers can write `wire::wire_capnp::Hello` etc.

pub mod bridge;
pub mod conn;
pub mod tls;
pub mod types;

pub use types::{BackendToSidecar, SidecarToBackend};

/// Generated Cap'n Proto types. The build script writes this module
/// to `$OUT_DIR/wire_capnp.rs`; we `include!` it so it appears as
/// `wire_capnp` in our crate.
#[allow(clippy::all)]
pub mod wire_capnp {
    include!(concat!(env!("OUT_DIR"), "/wire_capnp.rs"));
}

/// Protocol version sidecars send in `Hello.protocolVersion`. Bump
/// when removing or repurposing fields; new appended fields don't
/// require a bump (older peers ignore them).
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("handshake rejected: {0}")]
    HandshakeRejected(String),
    #[error("incompatible protocol version: peer={peer} ours={ours}")]
    IncompatibleVersion { peer: u32, ours: u32 },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("capnp error: {0}")]
    Capnp(#[from] capnp::Error),
    #[error("tls error: {0}")]
    Tls(String),
}
