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

/// Deliberately **not** `#[tokio::main]`.
///
/// `XDG_RUNTIME_DIR` has to be exported before anything else exists:
/// `setenv` is undefined behaviour in glibc once a second thread is
/// running, and `run()` starts a dbus-daemon child with stdout/stderr
/// drain tasks well before the compositor is built. The `#[tokio::main]`
/// macro builds the runtime *around* the whole function body, so there
/// is no way to get a statement in front of it — hence the hand-rolled
/// runtime. Everything after this line is identical to what the macro
/// would have generated.
#[cfg(target_os = "linux")]
fn main() {
    // A failure here is fatal by construction: without a runtime dir
    // there is nowhere to bind a Wayland socket, and every later error
    // would be a confusing downstream symptom of this one.
    if let Err(e) = x11_web_wayland_server::ensure_xdg_runtime_dir() {
        eprintln!("failed to prepare XDG_RUNTIME_DIR: {e}");
        std::process::exit(1);
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("failed to build the tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    runtime.block_on(linux::run());
}
