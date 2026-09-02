//! Run the compositor on its own and print every `DisplayUpdate` it
//! emits.
//!
//! This is the smallest possible embedder: it does what
//! `x11-web-sidecar-wayland` does minus the QUIC wire, so the
//! compositor can be exercised against a real Wayland client with no
//! backend, no frontend and no browser in the loop:
//!
//! ```text
//! cargo run --example headless          # prints WAYLAND_DISPLAY=…
//! WAYLAND_DISPLAY=wayland-1 weston-simple-shm
//! ```
//!
//! It is also the harness the packaging stage's container smoke tests
//! drive, which is why it prints machine-greppable lines rather than
//! going through `tracing`.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the wayland compositor only builds on Linux; run this under tools/wayland-build.sh");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;
    use x11_web_protocol::DisplayUpdate;
    use x11_web_wayland_server::{WaylandServer, WindowRouter};

    let (update_tx, mut update_rx) = tokio::sync::mpsc::unbounded_channel();
    let (client_tx, mut client_rx) = tokio::sync::mpsc::unbounded_channel();
    let router = WindowRouter::new();

    let server = match WaylandServer::new(
        update_tx,
        client_tx,
        router,
        (1280, 800),
        Duration::from_millis(16),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: {e}");
            std::process::exit(1);
        }
    };

    println!("WAYLAND_DISPLAY={}", server.wayland_display_name());
    println!("XDG_RUNTIME_DIR={}", server.xdg_runtime_dir().display());
    println!("SOCKET={}", server.socket_path().display());

    // No tokio runtime on purpose: `try_recv` works off-runtime, and
    // keeping the example single-threaded and synchronous means what
    // it proves is the compositor, not the async plumbing around it.
    loop {
        while let Ok((client_id, pid)) = client_rx.try_recv() {
            println!("CLIENT client_id={client_id} pid={pid}");
        }
        while let Ok((client_id, update)) = update_rx.try_recv() {
            match update {
                DisplayUpdate::PutImage {
                    window_id,
                    x,
                    y,
                    width,
                    height,
                    data,
                } => {
                    let non_black = data
                        .chunks_exact(4)
                        .filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8)
                        .count();
                    println!(
                        "PutImage client={client_id} window={window_id} at={x},{y} \
                         size={width}x{height} bytes={} non_black_px={non_black} \
                         first_px={:?}",
                        data.len(),
                        &data[..data.len().min(4)]
                    );
                }
                other => println!("{other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
