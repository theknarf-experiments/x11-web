//! Embeddable X11 server.
//!
//! Implements a minimal but spec-compliant X11 server that listens on
//! a Unix socket, accepts X11 client connections, drives the protocol
//! state machine, and emits per-window updates via a tokio
//! `mpsc::UnboundedSender<TaggedDisplayUpdate>` that the embedder
//! supplies at construction time.
//!
//! The crate is consumed by `x11-web-sidecar`, which translates the
//! emitted `DisplayUpdate`s into wire frames (encoding pixel data to
//! WebP-lossless, packaging into Cap'n Proto, sending over QUIC).
//! Keeping the wire / encoding concerns out here means the X11
//! server can be used from any host that wants a virtual X server
//! with a typed event stream.
//!
//! ## Public surface
//!
//! - [`X11Server`] — owns the socket and the entire server state.
//!   Construct via [`X11Server::new`], call
//!   [`write_xauthority`][X11Server::write_xauthority] to lay down
//!   the auth cookie, then `await` [`run`][X11Server::run] (it loops
//!   for the lifetime of the server).
//! - [`TaggedDisplayUpdate`] — the event type emitted on the
//!   embedder-supplied channel. Carries a `(client_id, DisplayUpdate)`
//!   pair; pixel `data` in [`DisplayUpdate::PutImage`] is **raw
//!   RGBA8888** — the embedder is responsible for compression before
//!   the wire.
//! - [`WindowRouter`] — handle the embedder uses to inject input and
//!   resize requests after `X11Server::run` has been spawned.
//! - [`MenuTracker`] — listens on the session DBus for GTK/Qt
//!   application menus and forwards them as `MenuStructure` updates.
//!   Constructed by the embedder so it can choose the DBus address
//!   to dial.

mod colors;
mod compose;
mod fonts;
mod framebuffer;
pub mod menus;
#[cfg(feature = "osmesa")]
mod osmesa;
mod xinput2;
mod xserver;

pub use menus::MenuTracker;
pub use xserver::{TaggedDisplayUpdate, WindowRouter, X11Server};

/// Attempt to load OSMesa for software OpenGL rendering. Returns `true` if
/// libOSMesa was found and the GL function pointers were resolved. Embedders
/// should call this once at startup before any client connects; absent this
/// call, GLX queries fall back to empty/stub replies.
#[cfg(feature = "osmesa")]
pub fn init_osmesa() -> bool {
    osmesa::init()
}

#[cfg(not(feature = "osmesa"))]
pub fn init_osmesa() -> bool {
    false
}
