//! Browser ↔ Backend WebSocket binary wire format.
//!
//! Schema: `schema/ws.capnp`. Codegen runs in `build.rs`,
//! producing `OUT_DIR/ws_capnp.rs` which we `include!` here.
//!
//! Public surface:
//!   * `ws_capnp` — the generated readers/writers (low level).
//!   * `bridge::{encode_frontend_msg, decode_frontend_msg,
//!               encode_backend_msg, decode_backend_msg}` — the
//!     four conversion entry points each side of the wire calls.
//!     They translate to/from the high-level enums in
//!     `x11-web-protocol`.

#[allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused_imports,
    unused_qualifications
)]
pub mod ws_capnp {
    include!(concat!(env!("OUT_DIR"), "/ws_capnp.rs"));
}

pub mod bridge;

pub use bridge::{
    decode_backend_msg, decode_frontend_msg, encode_backend_msg, encode_frontend_msg, BridgeError,
};
pub use capnp;
