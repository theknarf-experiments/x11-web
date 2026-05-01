#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("x11-web-sidecar-macos only builds on macOS.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::atomic::AtomicU8;
    use std::sync::Arc;
    use x11_web_sidecar_macos::tray;

    // The macOS tray UI (NSApplication, NSStatusItem) demands the
    // main thread, but the sidecar's tokio runtime owned it before.
    // Spin tokio off onto a dedicated worker thread and let the main
    // thread block on AppKit's run loop — when the user chooses Quit
    // the process exits and the tokio runtime is torn down with it.
    let conn_state = Arc::new(AtomicU8::new(tray::ConnState::Connecting as u8));
    let cs_for_runtime = conn_state.clone();
    std::thread::Builder::new()
        .name("sidecar-tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(macos::run(cs_for_runtime));
        })
        .expect("spawn sidecar tokio thread");

    tray::run_event_loop(conn_state);
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::atomic::AtomicU8;
    use std::sync::Arc;
    use std::time::Duration;

    use std::net::SocketAddr;

    use tokio::sync::mpsc;
    use tokio::time::interval;
    use tracing::{error, info, warn};
    use x11_web_sidecar_macos::input;
    use x11_web_sidecar_macos::router::WindowRouter;
    use x11_web_sidecar_macos::tray::{self, ConnState};
    use x11_web_wire::bridge as wire_bridge;
    use x11_web_wire::conn::{dial, DialedConnection};
    use x11_web_wire::tls::parse_fingerprint;
    use x11_web_wire::{wire_capnp, BackendToSidecar, SidecarKind, SidecarToBackend};

    pub async fn run(conn_state: Arc<AtomicU8>) {
        tracing_subscriber::fmt::init();

        // Probe SkyLight up front so the operator sees in the log
        // whether the private path is reachable on this system.
        let sky = x11_web_sidecar_macos::skylight::probe();
        info!(
            "SkyLight bridge: post_to_pid={} auth_message={} window_location={}",
            sky.post_to_pid, sky.auth_message, sky.window_location
        );

        // Trigger the Screen Recording TCC prompt if it hasn't been
        // granted yet. The first call surfaces the system dialog;
        // subsequent calls just return the cached grant state. The
        // sidecar can still register and enumerate windows without
        // it — we just won't be able to capture pixels.
        if objc2_core_graphics::CGRequestScreenCaptureAccess() {
            info!("Screen Recording permission: granted");
        } else {
            warn!(
                "Screen Recording permission: not granted. \
                 Open System Settings → Privacy & Security → Screen Recording \
                 and enable the entry for this binary, then restart the sidecar."
            );
        }

        // Accessibility: required by `CGEvent.postToPid` to inject
        // events into other processes. Without this, posts no-op
        // silently — there's no error from the API. Probe via
        // `AXIsProcessTrusted` and log the verdict so the operator
        // can see at a glance whether input will work.
        if unsafe { objc2_application_services::AXIsProcessTrusted() } {
            info!("Accessibility permission: granted");
        } else {
            warn!(
                "Accessibility permission: not granted. \
                 Open System Settings → Privacy & Security → Accessibility \
                 and enable X11WebSidecar.app. Without this, mouse and \
                 keyboard input will silently be dropped."
            );
        }

        // Install rustls's default crypto provider once per process
        // — quinn refuses to build a TLS config without it. `ring`
        // is what our wire crate is feature-gated to.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let backend_addr: SocketAddr = std::env::var("BACKEND_QUIC_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3002".into())
            .parse()
            .expect("BACKEND_QUIC_ADDR must be host:port");
        let server_name =
            std::env::var("BACKEND_SERVER_NAME").unwrap_or_else(|_| "localhost".into());
        // Fingerprint comes from one of:
        //   1. `X11WEB_SERVER_FINGERPRINT` env var (operator pasted).
        //   2. `X11WEB_FINGERPRINT_FILE` env var pointing to a file
        //      the backend wrote (default `target/x11web-fingerprint`).
        // We re-read on every connect attempt so a backend restart
        // (which generates a fresh cert) is picked up automatically
        // without restarting the sidecar.
        let fingerprint_source = match std::env::var("X11WEB_SERVER_FINGERPRINT") {
            Ok(s) => FingerprintSource::Inline(s),
            Err(_) => FingerprintSource::File(
                std::env::var("X11WEB_FINGERPRINT_FILE").unwrap_or_else(|_| {
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                    format!("{home}/.x11web-fingerprint")
                }),
            ),
        };
        let bearer_token = std::env::var("X11WEB_BEARER_TOKEN")
            .unwrap_or_else(|_| "dev-token".into())
            .into_bytes();

        let sidecar_name = std::env::var("SIDECAR_NAME")
            .unwrap_or_else(|_| hostname().unwrap_or_else(|| "macos-sidecar".into()));

        info!("Connecting to backend at {backend_addr} (server-name={server_name})");
        loop {
            tray::store(&conn_state, ConnState::Connecting);
            let fingerprint = match read_fingerprint(&fingerprint_source) {
                Ok(fp) => fp,
                Err(e) => {
                    warn!("Fingerprint not available yet: {e}. Retrying in 2s.");
                    tray::store(&conn_state, ConnState::Disconnected);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            match dial(
                backend_addr,
                &server_name,
                fingerprint,
                &bearer_token,
                &sidecar_name,
                SidecarKind::Macos,
            )
            .await
            {
                Ok(connection) => {
                    info!(
                        "Connected to backend; sidecar_id={} agreed_version={}",
                        connection.sidecar_id, connection.agreed_protocol_version
                    );
                    tray::store(&conn_state, ConnState::Connected);
                    run_session(connection).await;
                    tray::store(&conn_state, ConnState::Disconnected);
                    warn!("Disconnected from backend, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Failed to connect to backend: {e}");
                    tray::store(&conn_state, ConnState::Disconnected);
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn run_session(mut connection: DialedConnection) {
        let (tx, mut rx) = mpsc::unbounded_channel::<SidecarToBackend>();

        // Heartbeat task — pushes a `Heartbeat` into `tx` every 30 s.
        // The send_task is what actually serialises + writes to the
        // wire.
        let tx_heartbeat = tx.clone();
        let heartbeat_task = tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                if tx_heartbeat.send(SidecarToBackend::Heartbeat).is_err() {
                    break;
                }
            }
        });

        // Window enumeration → DisplayUpdate stream. Live SCStream
        // captures are off by default; backend asks for them via
        // `StartWindowCapture` / `StopWindowCapture`, which the
        // recv_loop forwards onto `capture_ctl_tx`.
        let router = WindowRouter::new();
        let (capture_ctl_tx, capture_ctl_rx) =
            mpsc::unbounded_channel::<x11_web_sidecar_macos::enumerator::CaptureControl>();
        x11_web_sidecar_macos::enumerator::spawn(tx.clone(), router.clone(), capture_ctl_rx);

        // Drive recv + send concurrently in the same task so capnp's
        // !Send readers don't fight tokio::spawn.
        let send_loop = async {
            while let Some(msg) = rx.recv().await {
                let Some(builder) = wire_bridge::build_from_sidecar(&msg) else {
                    continue;
                };
                if let Err(e) = connection.writer.write_message(&builder).await {
                    warn!("wire write failed: {e}");
                    return;
                }
            }
        };
        let recv_loop = async {
            loop {
                let msg = match connection
                    .reader
                    .read_message::<wire_capnp::to_sidecar::Owned>()
                    .await
                {
                    Ok(Some(m)) => m,
                    Ok(None) => return, // clean EOF
                    Err(e) => {
                        warn!("wire read failed: {e}");
                        return;
                    }
                };
                let to_sidecar: wire_capnp::to_sidecar::Reader = match msg.get_root() {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ToSidecar root: {e}");
                        continue;
                    }
                };
                match wire_bridge::read_to_sidecar(to_sidecar) {
                    Ok(cmd) => handle_backend_msg(cmd, &router, &capture_ctl_tx),
                    Err(e) => {
                        warn!("ToSidecar translate: {e:?}");
                    }
                }
            }
        };

        tokio::select! {
            _ = send_loop => {}
            _ = recv_loop => {}
        }

        heartbeat_task.abort();
    }

    fn handle_backend_msg(
        cmd: BackendToSidecar,
        router: &WindowRouter,
        capture_ctl_tx: &mpsc::UnboundedSender<x11_web_sidecar_macos::enumerator::CaptureControl>,
    ) {
        use x11_web_sidecar_macos::enumerator::CaptureControl;
        match cmd {
            BackendToSidecar::InputEvent { window_id, event } => {
                info!("InputEvent received: window={window_id} event={event:?}");
                match router.lookup(&window_id) {
                    Some(route) => {
                        info!(
                            "Routing to pid={} origin=({:.0},{:.0})",
                            route.pid, route.bounds.x, route.bounds.y
                        );
                        input::inject(route, event);
                    }
                    None => {
                        warn!("No route for window UUID {window_id}");
                    }
                }
            }
            BackendToSidecar::StartWindowCapture { window_id } => {
                let _ = capture_ctl_tx.send(CaptureControl::Start { window_id });
            }
            BackendToSidecar::StopWindowCapture { window_id } => {
                let _ = capture_ctl_tx.send(CaptureControl::Stop { window_id });
            }
            other => {
                info!("Backend msg (ignored): {other:?}");
            }
        }
    }

    fn hostname() -> Option<String> {
        std::env::var("HOSTNAME").ok().or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
    }

    /// Where the fingerprint comes from. We re-resolve on each
    /// dial attempt so a backend restart (which writes a new
    /// fingerprint to its file) is picked up automatically.
    pub enum FingerprintSource {
        Inline(String),
        File(String),
    }

    fn read_fingerprint(source: &FingerprintSource) -> Result<[u8; 32], String> {
        let raw = match source {
            FingerprintSource::Inline(s) => s.clone(),
            FingerprintSource::File(path) => {
                std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?
            }
        };
        parse_fingerprint(&raw).map_err(|e| format!("parse fingerprint: {e}"))
    }
}
