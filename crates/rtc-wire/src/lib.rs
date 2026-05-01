//! Browser ↔ Backend binary wire format, carried over a WebRTC
//! DataChannel.
//!
//! The schema lives in `schema/wire.capnp` and the build script
//! regenerates Rust types into `OUT_DIR/wire_capnp.rs` on every build.

#[allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused_imports,
    unused_qualifications
)]
pub mod wire_capnp {
    include!(concat!(env!("OUT_DIR"), "/wire_capnp.rs"));
}

pub use capnp;
