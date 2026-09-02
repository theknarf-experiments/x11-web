//! Wayland sidecar: hosts a headless Wayland compositor
//! (`x11-web-wayland-server`), spawns clients against it, streams
//! every toplevel to the backend as `DisplayUpdate::PutImage`, and
//! injects the input events that come back.
//!
//! Structurally the same binary as `crates/sidecar` (X11) — same
//! connect/reconnect loop, same `run_session`, same
//! `encode_for_wire`. Only the display server underneath differs.
//!
//! Non-Linux hosts get the stub `main` below, exactly like
//! `crates/sidecar-macos` does on non-macOS: the crate stays a
//! workspace member so `cargo check --workspace` covers its portable
//! modules, without dragging smithay onto a platform that can't build
//! it.

// Compiled on every platform (opentelemetry is portable), but only
// referenced from the Linux binary — so silence the unused warnings
// on the stub path rather than cfg-ing the module out and having the
// two builds diverge.
#[allow(dead_code)]
mod telemetry;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("x11-web-sidecar-wayland only builds on Linux.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod process;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    linux::run().await;
}
