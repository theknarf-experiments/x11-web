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
//! ## Flow control
//!
//! The compositor produces at 60 Hz; the embedder's WebP encoder is an
//! order of magnitude slower, and `UnboundedSender::send` never blocks.
//! Left alone, that combination grows the update channel without bound
//! (a 1280x800 raw RGBA frame is ~4 MB). So the render tick asks
//! [`WindowRouter`] for per-window credit before it emits, and
//! *withholds `wl_surface.frame` callbacks* from a window that has none
//! — which is what actually pushes back on the client. The embedder
//! returns credit with
//! [`frame_shipped`][WindowRouter::frame_shipped], and brackets a
//! session with [`set_consuming`][WindowRouter::set_consuming]; while
//! nothing is consuming, frames are composited (clients keep animating)
//! but dropped rather than queued for a reader that does not exist.
//!
//! ## Provenance and licensing
//!
//! The compositor half of this crate is derived from **waylandcraft**
//! (<https://github.com/EVV1E/waylandcraft>, commit
//! `233d1431e6acbad1d0c47dfba44d971ce0cebfe8`), which is **GPLv3**.
//! `state.rs`, `surface.rs`, `seat.rs`, `output.rs` and `utils.rs` each
//! carry a header naming their upstream file and what changed;
//! `server.rs`, `router.rs`, `windows.rs`, `input.rs`, `pixels.rs`,
//! `translate.rs` and `decoration.rs` are original. The upstream
//! Minecraft/JNI half — the dmabuf/EGL import path, xwayland-satellite,
//! `.desktop` launching and `wl_data_device` — was not ported.
//!
//! Consequently this crate, and **any binary that links it**
//! (`x11-web-sidecar-wayland`, and the image
//! `Dockerfile.sidecar-wayland` builds), is conveyable only under
//! GPLv3. Crates outside that link graph are unaffected. See
//! `crates/wayland-server/NOTICE` for the file-by-file record and
//! `crates/wayland-server/COPYING` for the license text.
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
mod decoration;
#[cfg(target_os = "linux")]
mod input;
#[cfg(target_os = "linux")]
mod output;
#[cfg(target_os = "linux")]
mod router;
#[cfg(target_os = "linux")]
mod seat;
#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
mod state;
#[cfg(target_os = "linux")]
mod surface;
#[cfg(target_os = "linux")]
mod utils;
#[cfg(target_os = "linux")]
mod windows;

#[cfg(target_os = "linux")]
pub use router::WindowRouter;
#[cfg(target_os = "linux")]
pub use server::{ensure_xdg_runtime_dir, WaylandServer};

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
