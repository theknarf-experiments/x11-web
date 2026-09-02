//! The Linux sidecar body.
//!
//! STAGE: Sidecar — everything in this module. It must mirror
//! `crates/sidecar/src/main.rs` piece for piece:
//!
//!   1. `crate::telemetry::init()`, then install rustls' ring crypto
//!      provider (`quinn` refuses to build a TLS config without one).
//!   2. Read the environment: `BACKEND_QUIC_ADDR`,
//!      `BACKEND_SERVER_NAME`,
//!      `X11WEB_SERVER_FINGERPRINT` | `X11WEB_FINGERPRINT_FILE`,
//!      `X11WEB_BEARER_TOKEN`, `SIDECAR_NAME`, plus
//!      `WAYLAND_SCREEN_SIZE` (default 1280x800). There is no
//!      `DISPLAY_NUMBER` equivalent — the socket name is assigned by
//!      the compositor and read back off `WaylandServer`.
//!   3. `start_dbus_session()` (see `process.rs`) — kept verbatim from
//!      the X11 sidecar because GTK apps still export their menus over
//!      the session bus under Wayland.
//!   4. Build the update / client-connected channels and the
//!      `WindowRouter`, construct `WaylandServer::new(...)`, and
//!      `tokio::spawn(async move { if let Err(e) = server.run().await { … } })`.
//!   5. The connect loop: re-read the fingerprint and re-resolve DNS
//!      on every attempt, `dial(..., SidecarKind::Wayland)`, then
//!      `run_session`, then sleep and retry.
//!   6. `run_session`: outbound `mpsc<SidecarToBackend>`, inbound
//!      `mpsc<(BackendToSidecar, traceparent)>`, a 30 s heartbeat, and
//!      a `select!` over the recv loop and the events loop.
//!   7. `encode_for_wire` copied verbatim (raw RGBA → WebP-lossless;
//!      the capnp `encoding` field stays `RawRgba` because the
//!      frontend hardcodes an `image/webp` Blob).
//!   8. `record_cmd_attrs` copied verbatim, so `mark_span_error`'s
//!      `error.kind` / `error.message` fields are declared on the
//!      span before anything tries to record them.
//!
//! `StartWindowCapture` / `StopWindowCapture` are ignored: like the
//! X11 sidecar, this one auto-streams every window.

/// Entry point called from `main`.
pub async fn run() {
    // STAGE: Sidecar — replace with the real body described above.
    // Deliberately loud rather than silent: a sidecar that exits
    // immediately with no output is indistinguishable from a crashed
    // container in the e2e harness.
    eprintln!("x11-web-sidecar-wayland: not yet implemented (STAGE: Sidecar)");
    std::process::exit(1);
}
