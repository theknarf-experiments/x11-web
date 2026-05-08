//! Translation between the high-level `FrontendToBackend` /
//! `BackendToFrontend` Rust enums (in `x11-web-protocol`) and the
//! Cap'n Proto wire types in `ws_capnp`.
//!
//! Each side of the wire uses two of the four entry points:
//!   * Frontend writes call `encode_frontend_msg`.
//!   * Backend reads call `decode_frontend_msg`.
//!   * Backend writes call `encode_backend_msg`.
//!   * Frontend reads call `decode_backend_msg`.
//!
//! The body of the bridge currently stubs the variant translation
//! — Stage 1 of the refactor lands the schema + crate plumbing;
//! Stage 2 fills these in. Compile-only for now.

use capnp::message::{Builder, HeapAllocator};

use crate::ws_capnp;

#[derive(Debug)]
pub enum BridgeError {
    /// Wire-side variant the bridge doesn't translate yet (Stage 2).
    Unimplemented(&'static str),
    /// Underlying capnp parse failure (truncated frame, bad layout).
    Capnp(capnp::Error),
    /// Schema variant lookup hit an out-of-range discriminant.
    NotInSchema(capnp::NotInSchema),
}

impl From<capnp::Error> for BridgeError {
    fn from(e: capnp::Error) -> Self {
        Self::Capnp(e)
    }
}

impl From<capnp::NotInSchema> for BridgeError {
    fn from(e: capnp::NotInSchema) -> Self {
        Self::NotInSchema(e)
    }
}

/// Stub: serialise a `FrontendToBackend` to a capnp byte stream
/// suitable for `WebSocket::send` as a binary frame. Stage 2 fills
/// in the variant arms; for now produces an empty `noVariant`
/// envelope so the surrounding plumbing can be tested.
pub fn encode_frontend_msg(
    _msg: &x11_web_protocol::FrontendToBackend,
    traceparent: &str,
) -> Vec<u8> {
    let mut builder: Builder<HeapAllocator> = Builder::new_default();
    {
        let mut root = builder.init_root::<ws_capnp::frontend_msg::Builder>();
        root.set_traceparent(traceparent);
        let _payload = root.init_payload();
        // No variant set → reader sees `noVariant`.
    }
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &builder)
        .expect("writing to a Vec never fails");
    out
}

/// Stub: parse a capnp-encoded `FrontendMsg` into a typed enum
/// plus the `traceparent` header. Stage 2 fills this in.
pub fn decode_frontend_msg(
    _bytes: &[u8],
) -> Result<(x11_web_protocol::FrontendToBackend, String), BridgeError> {
    Err(BridgeError::Unimplemented("decode_frontend_msg"))
}

/// Stub: serialise a `BackendToFrontend`. See `encode_frontend_msg`
/// for the surrounding pattern.
pub fn encode_backend_msg(
    _msg: &x11_web_protocol::BackendToFrontend,
    traceparent: &str,
) -> Vec<u8> {
    let mut builder: Builder<HeapAllocator> = Builder::new_default();
    {
        let mut root = builder.init_root::<ws_capnp::backend_msg::Builder>();
        root.set_traceparent(traceparent);
        let _payload = root.init_payload();
    }
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &builder)
        .expect("writing to a Vec never fails");
    out
}

/// Stub: parse a capnp-encoded `BackendMsg` into a typed enum.
pub fn decode_backend_msg(
    _bytes: &[u8],
) -> Result<(x11_web_protocol::BackendToFrontend, String), BridgeError> {
    Err(BridgeError::Unimplemented("decode_backend_msg"))
}
