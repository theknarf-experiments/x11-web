//! Embeddable headless Wayland compositor.
//!
//! The Wayland twin of [`x11-web-x11-server`]: it hosts a smithay
//! compositor on a Unix socket, accepts Wayland client connections,
//! composites each toplevel's surface tree into an RGBA framebuffer,
//! and emits per-window updates via a tokio
//! `mpsc::UnboundedSender<TaggedDisplayUpdate>` that the embedder
//! supplies at construction time.
//!
//! The crate is consumed by `x11-web-sidecar-wayland`, which
//! translates the emitted `DisplayUpdate`s into wire frames (encoding
//! pixel data to WebP-lossless, packaging into Cap'n Proto, sending
//! over QUIC). Keeping wire / encoding concerns out here means the
//! compositor can be embedded by anything that wants a virtual
//! Wayland display with a typed event stream.
//!
//! ## Public surface
//!
//! - [`WaylandServer`] — owns the socket and the compositor thread.
//!   Construct via [`WaylandServer::new`] (which binds the socket
//!   eagerly, so the display name is known before any child process
//!   is spawned), read
//!   [`wayland_display_name`][WaylandServer::wayland_display_name]
//!   into the child's `WAYLAND_DISPLAY`, then `await`
//!   [`run`][WaylandServer::run] (it resolves only when the
//!   compositor thread exits).
//! - [`TaggedDisplayUpdate`] — the event type emitted on the
//!   embedder-supplied channel. Carries a `(client_id, DisplayUpdate)`
//!   pair; pixel `data` in [`DisplayUpdate::PutImage`] is **raw
//!   RGBA8888** — the embedder is responsible for compression before
//!   the wire.
//! - [`WindowRouter`] — handle the embedder uses to inject input,
//!   resize requests and screen-size changes after the server is
//!   running.
//!
//! ## Portability
//!
//! smithay only builds on Linux; this repo is developed on macOS.
//! Every smithay-touching module is `#[cfg(target_os = "linux")]`, so
//! on other platforms the crate compiles down to [`pixels`] and
//! [`translate`] — the two modules that are pure arithmetic and carry
//! the unit tests for the two bugs most likely to be silent
//! (red/blue-swapped framebuffers, and mis-mapped mouse buttons).
//!
//! ## Threading
//!
//! smithay's `EventLoop` and `Display` are `!Send`, and tokio owns the
//! process. They meet in exactly one place: [`WaylandServer::new`]
//! spawns a dedicated `"wayland-compositor"` OS thread and constructs
//! *all* smithay state inside it, so nothing `!Send` ever crosses a
//! thread boundary. Updates flow out over a tokio unbounded channel
//! (`Send + Sync`, non-async `send`); commands flow in over a
//! `calloop::channel`, whose sender [`WindowRouter`] holds behind a
//! `Mutex`.

pub mod pixels;
pub mod translate;

#[cfg(target_os = "linux")]
mod router;
#[cfg(target_os = "linux")]
mod server;

#[cfg(target_os = "linux")]
pub use router::WindowRouter;
#[cfg(target_os = "linux")]
pub use server::WaylandServer;

/// A display update tagged with the `client_id` that produced it.
///
/// Deliberately identical to `x11-web-x11-server`'s type of the same
/// name so the two sidecars' plumbing is interchangeable. The
/// `client_id` is a UUID minted when a Wayland client connects to the
/// socket; it is paired with the connecting process's pid (read off
/// the socket's peer credentials) in the `client_connected_tx`
/// channel, which is what the backend's `ProcessConnected` bookkeeping
/// consumes.
pub type TaggedDisplayUpdate = (String, x11_web_protocol::DisplayUpdate);
