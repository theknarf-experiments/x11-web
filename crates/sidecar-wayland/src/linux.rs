//! The Linux sidecar body.
//!
//! Structurally a mirror of `crates/sidecar/src/main.rs`: same
//! environment contract, same fingerprint sourcing, same DNS-resolving
//! dial loop with the same reconnect delay, same `run_session` split
//! into a recv loop and an events loop, same heartbeat, same
//! `encode_for_wire`, same OTel span plumbing. A reviewer should be
//! able to diff the two mentally; where they differ, there is a comment
//! saying why.
//!
//! The differences, in full:
//!
//!   * `WaylandServer` replaces `X11Server`. Its `new()` is fallible
//!     (it binds the socket eagerly so `WAYLAND_DISPLAY` is known
//!     before any child is spawned) and takes a plain `(w, h)` screen
//!     size rather than a `watch::Receiver`.
//!   * No OSMesa: this sidecar has no GLX to software-render. Wayland
//!     clients that want GL negotiate it with the client-side Mesa in
//!     their own image; the slice is shm-only, so a client that insists
//!     on a GPU buffer simply doesn't map.
//!   * No `MenuTracker` and no clipboard channel — both are X11/DBus
//!     concerns that `x11-web-wayland-server` deliberately doesn't have.
//!     The session bus itself is still started, for the children.
//!   * `XAUTHORITY` has no analogue: a Wayland socket's access control
//!     *is* filesystem permissions on `$XDG_RUNTIME_DIR`.
//!
//! `StartWindowCapture` / `StopWindowCapture` are ignored: like the X11
//! sidecar, this one auto-streams every window.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info, warn};
use x11_web_protocol::DisplayUpdate;
use x11_web_wayland_server::{TaggedDisplayUpdate, WaylandServer, WindowRouter};
use x11_web_wire::bridge as wire_bridge;
use x11_web_wire::conn::{dial, DialedConnection};
use x11_web_wire::tls::parse_fingerprint;
use x11_web_wire::{wire_capnp, BackendToSidecar, SidecarKind, SidecarToBackend};

use crate::process::{find_ancestor_pid, start_dbus_session, ProcessManager};
use crate::telemetry;

/// How often the sidecar pings the backend to keep the connection alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Polling cadence for reaping exited child processes.
const PROCESS_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// Reconnect delay when the backend QUIC link drops.
const BACKEND_RECONNECT_DELAY: Duration = Duration::from_secs(5);
/// Sleep between dial retries when the backend isn't reachable yet.
const DIAL_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Render tick period for the compositor. Also the cadence at which
/// `wl_surface.frame` callbacks are released, which is why it is a
/// fixed schedule and not "whenever a client commits" — see
/// `x11-web-wayland-server`'s `windows::tick`.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// wl_output mode advertised to clients when `WAYLAND_SCREEN_SIZE` is
/// unset. Larger than the X11 sidecar's 1024x768 because Wayland
/// clients routinely size their initial window as a fraction of the
/// output, and a small output makes every toolkit open a small window.
const DEFAULT_SCREEN_SIZE: (u16, u16) = (1280, 800);

/// Entry point called from `main`.
pub async fn run() {
    let telemetry = telemetry::init();

    // Install rustls's default crypto provider — quinn's TLS config
    // rejects building without one. `ring` is what the wire crate is
    // feature-gated to.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // `BACKEND_QUIC_ADDR` may be either a literal `IP:PORT` or
    // `hostname:PORT` (this sidecar in Docker reaching `backend:3002`
    // over the network alias). Resolve via DNS at dial time rather than
    // parse here so we re-resolve on each reconnect — handy when the
    // backend container restarts and gets a new IP.
    let backend_addr_str =
        std::env::var("BACKEND_QUIC_ADDR").unwrap_or_else(|_| "127.0.0.1:3002".into());
    let server_name = std::env::var("BACKEND_SERVER_NAME").unwrap_or_else(|_| "localhost".into());
    let fingerprint_source = match std::env::var("X11WEB_SERVER_FINGERPRINT") {
        Ok(s) => FingerprintSource::Inline(s),
        Err(_) => FingerprintSource::File(std::env::var("X11WEB_FINGERPRINT_FILE").unwrap_or_else(
            |_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                format!("{home}/.x11web-fingerprint")
            },
        )),
    };
    let bearer_token = std::env::var("X11WEB_BEARER_TOKEN")
        .unwrap_or_else(|_| "dev-token".into())
        .into_bytes();
    let sidecar_name =
        std::env::var("SIDECAR_NAME").unwrap_or_else(|_| hostname().unwrap_or("sidecar".into()));
    // The X11 sidecar's `DISPLAY_NUMBER` has no counterpart: the
    // Wayland socket name is picked by `ListeningSocketSource::new_auto`
    // (wayland-1..wayland-32) and read back off the server below.
    let screen_size = parse_screen_size(std::env::var("WAYLAND_SCREEN_SIZE").ok().as_deref());

    // Start the per-sidecar DBus session bus before anything else so
    // every spawned app inherits DBUS_SESSION_BUS_ADDRESS. The returned
    // handle is held for the lifetime of `run` to keep the daemon
    // alive.
    let dbus_session = start_dbus_session().await;
    let dbus_address = dbus_session.as_ref().map(|s| s.address.clone());

    // Start the Wayland compositor.
    let (display_tx, mut display_rx) = mpsc::unbounded_channel::<TaggedDisplayUpdate>();
    let (client_connected_tx, mut client_connected_rx) = mpsc::unbounded_channel::<(String, u32)>();
    let window_router = WindowRouter::new();
    // Unlike `X11Server::new`, this is fallible and binds the socket
    // synchronously. There is nothing useful to do without a display,
    // and exiting loudly beats sitting in a reconnect loop serving a
    // compositor that isn't there.
    let wayland_server = match WaylandServer::new(
        display_tx,
        client_connected_tx,
        window_router.clone(),
        screen_size,
        FRAME_INTERVAL,
    ) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to start Wayland compositor: {e}");
            telemetry.shutdown();
            std::process::exit(1);
        }
    };
    let wayland_display = wayland_server.wayland_display_name().to_string();
    let xdg_runtime_dir = wayland_server.xdg_runtime_dir().display().to_string();

    info!(
        "Starting Wayland compositor on WAYLAND_DISPLAY={} (XDG_RUNTIME_DIR={}, output {}x{})",
        wayland_display, xdg_runtime_dir, screen_size.0, screen_size.1
    );

    tokio::spawn(async move {
        if let Err(e) = wayland_server.run().await {
            error!("Wayland compositor error: {e}");
        }
    });

    info!("Connecting to backend at {backend_addr_str} (server-name={server_name})");

    // Race the connect-loop against SIGINT/SIGTERM. Whichever resolves
    // first ends the select; we then drain the telemetry pipelines so
    // the last batch makes it to OpenObserve before the process exits.
    let connect_loop = async {
        loop {
            let fingerprint = match read_fingerprint(&fingerprint_source) {
                Ok(fp) => fp,
                Err(e) => {
                    warn!("Fingerprint not available yet: {e}. Retrying in 2s.");
                    tokio::time::sleep(DIAL_RETRY_DELAY).await;
                    continue;
                }
            };
            let backend_addr: SocketAddr = match resolve_backend_addr(&backend_addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    warn!("Failed to resolve {backend_addr_str}: {e}. Retrying in 2s.");
                    tokio::time::sleep(DIAL_RETRY_DELAY).await;
                    continue;
                }
            };
            match dial(
                backend_addr,
                &server_name,
                fingerprint,
                &bearer_token,
                &sidecar_name,
                SidecarKind::Wayland,
            )
            .await
            {
                Ok(connection) => {
                    info!(
                        "Connected to backend; sidecar_id={} agreed_version={}",
                        connection.sidecar_id, connection.agreed_protocol_version
                    );
                    run_session(
                        connection,
                        &wayland_display,
                        &xdg_runtime_dir,
                        dbus_address.clone(),
                        &mut display_rx,
                        &window_router,
                        &mut client_connected_rx,
                    )
                    .await;
                    warn!("Disconnected from backend, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Failed to connect to backend: {e}");
                }
            }
            tokio::time::sleep(BACKEND_RECONNECT_DELAY).await;
        }
    };
    tokio::select! {
        _ = connect_loop => {}
        _ = x11_web_telemetry::shutdown_signal() => {
            info!("Shutdown signal received; flushing telemetry...");
        }
    }
    telemetry.shutdown();
}

/// Where the fingerprint comes from. Mirror of the X11 sidecar's
/// equivalent — re-resolved on every dial attempt so a backend restart
/// picks up the new fingerprint without restarting the sidecar.
enum FingerprintSource {
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

/// Parse `WAYLAND_SCREEN_SIZE` ("1280x800"). Anything unparseable falls
/// back to the default with a warning rather than failing startup: the
/// output size is a hint to clients, not a correctness property, and a
/// typo in compose.yml should not cost you the whole sidecar.
fn parse_screen_size(spec: Option<&str>) -> (u16, u16) {
    let Some(spec) = spec else {
        return DEFAULT_SCREEN_SIZE;
    };
    let parsed = spec
        .split_once(['x', 'X'])
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .filter(|&(w, h): &(u16, u16)| w > 0 && h > 0);
    match parsed {
        Some(size) => size,
        None => {
            warn!("Unparseable WAYLAND_SCREEN_SIZE={spec:?}; using default");
            DEFAULT_SCREEN_SIZE
        }
    }
}

/// Resolve `host:port` (or `ip:port`) to a single `SocketAddr`,
/// preferring IPv4 because quinn's default endpoint binds 0.0.0.0
/// and refuses to dial v6 from a v4 socket. Re-runs on every
/// reconnect — a restarted backend container changes IPs.
async fn resolve_backend_addr(s: &str) -> Result<SocketAddr, String> {
    let mut addrs = tokio::net::lookup_host(s)
        .await
        .map_err(|e| format!("lookup_host: {e}"))?;
    let mut first_v6 = None;
    for a in addrs.by_ref() {
        if a.is_ipv4() {
            return Ok(a);
        }
        if first_v6.is_none() {
            first_v6 = Some(a);
        }
    }
    first_v6.ok_or_else(|| "no addresses resolved".into())
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    connection: DialedConnection,
    wayland_display: &str,
    xdg_runtime_dir: &str,
    dbus_address: Option<String>,
    display_rx: &mut mpsc::UnboundedReceiver<TaggedDisplayUpdate>,
    window_router: &WindowRouter,
    client_connected_rx: &mut mpsc::UnboundedReceiver<(String, u32)>,
) {
    let DialedConnection {
        mut reader,
        mut writer,
        ..
    } = connection;
    let mut process_manager = ProcessManager::new(
        wayland_display.to_string(),
        xdg_runtime_dir.to_string(),
        dbus_address,
    );

    // Outgoing messages: every event source pushes SidecarToBackend
    // here, the events loop drains it through the wire writer.
    let (tx, mut rx) = mpsc::unbounded_channel::<SidecarToBackend>();

    // Incoming messages: the recv loop owns the wire reader, decodes
    // Cap'n Proto, forwards BackendToSidecar over this channel so the
    // events loop can keep `process_manager` borrowed exclusively.
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<(BackendToSidecar, String)>();

    // Heartbeat — pushes Heartbeat into tx every 30s.
    let tx_heartbeat = tx.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut tick = interval(HEARTBEAT_INTERVAL);
        loop {
            tick.tick().await;
            if tx_heartbeat.send(SidecarToBackend::Heartbeat).is_err() {
                break;
            }
        }
    });

    // Recv loop owns the wire reader. Translates messages to
    // BackendToSidecar and forwards over `in_tx`.
    let recv_loop = async {
        loop {
            let msg = match reader.read_message::<wire_capnp::to_sidecar::Owned>().await {
                Ok(Some(m)) => m,
                Ok(None) => return,
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
                Ok((cmd, traceparent)) => {
                    if in_tx.send((cmd, traceparent)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    warn!("ToSidecar translate: {e:?}");
                }
            }
        }
    };

    // Events + send loop owns the wire writer plus every other event
    // source.
    let events_loop = async {
        let mut check_interval = interval(PROCESS_CHECK_INTERVAL);
        loop {
            tokio::select! {
                Some((cmd, traceparent)) = in_rx.recv() => {
                    // Continue the backend's span for the duration of
                    // this command — any tracing macros inside
                    // `handle_command` thread up into one trace.
                    use x11_web_telemetry::{OpenTelemetrySpanExt, TraceContextExt};
                    let parent_ctx = x11_web_telemetry::extract_traceparent(&traceparent);
                    let span = tracing::info_span!(
                        "sidecar.handle_command",
                        traceparent = %traceparent,
                        cmd.kind = tracing::field::Empty,
                        request.id = tracing::field::Empty,
                        window.id = tracing::field::Empty,
                        command = tracing::field::Empty,
                        pid = tracing::field::Empty,
                        width = tracing::field::Empty,
                        height = tracing::field::Empty,
                        error.kind = tracing::field::Empty,
                        error.message = tracing::field::Empty,
                    );
                    if parent_ctx.span().span_context().is_valid() {
                        let _ = span.set_parent(parent_ctx);
                    }
                    record_cmd_attrs(&span, &cmd);
                    let _enter = span.enter();
                    handle_command(
                        cmd,
                        &mut process_manager,
                        &tx,
                        window_router,
                    ).await;
                }
                Some(msg) = rx.recv() => {
                    let traceparent = x11_web_telemetry::current_traceparent();
                    let Some(builder) = wire_bridge::build_from_sidecar(&msg, &traceparent) else {
                        continue;
                    };
                    if let Err(e) = writer.write_message(&builder).await {
                        warn!("wire write failed: {e}");
                        return;
                    }
                }
                Some((client_id, update)) = display_rx.recv() => {
                    // The compositor emits raw RGBA in PutImage. The
                    // wire format is WebP-lossless; encode here so the
                    // compositor library doesn't have to know about
                    // pixel codecs. Other DisplayUpdate variants pass
                    // through unchanged.
                    let update = encode_for_wire(update);
                    if let Some(m) = telemetry::metrics() {
                        let kind = match &update {
                            DisplayUpdate::PutImage { .. } => "put_image",
                            DisplayUpdate::WindowThumbnail { .. } => "thumbnail",
                            _ => "other",
                        };
                        m.display_updates
                            .add(1, &[opentelemetry::KeyValue::new("kind", kind)]);
                    }
                    let _ = tx.send(SidecarToBackend::DisplayUpdate { client_id, update });
                }
                Some((client_id, peer_pid)) = client_connected_rx.recv() => {
                    // Identical to the X11 sidecar, and it works for
                    // the same reason: the compositor reads the
                    // connecting process's pid off the accepted
                    // socket's SO_PEERCRED, which is the same fact the
                    // X server reports for an X11 client. Prefer
                    // matching against the full spawn history (covers
                    // wrapper-exits-fast launchers), fall back to the
                    // live process table for the command-name lookup.
                    let history: Vec<u32> = process_manager.spawned_pid_history()
                        .iter().copied().collect();
                    if let Some(pid) = find_ancestor_pid(peer_pid, &history) {
                        let command = process_manager.get_command(pid).unwrap_or("").to_string();
                        info!(
                            "Process {pid} ({command}) (peer {peer_pid}) connected as Wayland client {client_id}"
                        );
                        let _ = tx.send(SidecarToBackend::ProcessConnected { pid, client_id, command });
                    } else {
                        info!(
                            "Wayland client {client_id} connected (peer PID {peer_pid}, no matching spawned process)"
                        );
                    }
                }
                _ = check_interval.tick() => {
                    let exited = process_manager.check_exited().await;
                    for (pid, exit_code) in exited {
                        let _ = tx.send(SidecarToBackend::ProcessExited { pid, exit_code });
                    }
                }
            }
        }
    };

    tokio::select! {
        _ = recv_loop => {}
        _ = events_loop => {}
    }

    heartbeat_task.abort();
}

/// Stamp variant-specific IDs onto the active `sidecar.handle_command`
/// span so a single trace shows exactly which window / process the
/// sidecar is acting on. Copied from the X11 sidecar verbatim, and it
/// has to stay that way: `mark_span_error` records `error.kind` /
/// `error.message`, and a tracing field that wasn't declared on the
/// span at creation is silently dropped.
fn record_cmd_attrs(span: &tracing::Span, cmd: &BackendToSidecar) {
    use BackendToSidecar::*;
    match cmd {
        SpawnProcess {
            request_id,
            command,
            ..
        } => {
            span.record("cmd.kind", "SpawnProcess");
            span.record("request.id", request_id.as_str());
            span.record("command", command.as_str());
        }
        KillProcess { request_id, pid } => {
            span.record("cmd.kind", "KillProcess");
            span.record("request.id", request_id.as_str());
            span.record("pid", *pid);
        }
        ListProcesses { request_id } => {
            span.record("cmd.kind", "ListProcesses");
            span.record("request.id", request_id.as_str());
        }
        InputEvent { window_id, .. } => {
            span.record("cmd.kind", "InputEvent");
            span.record("window.id", window_id.as_str());
        }
        ResizeWindow {
            window_id,
            width,
            height,
        } => {
            span.record("cmd.kind", "ResizeWindow");
            span.record("window.id", window_id.as_str());
            span.record("width", *width);
            span.record("height", *height);
        }
        StartWindowCapture { window_id } => {
            span.record("cmd.kind", "StartWindowCapture");
            span.record("window.id", window_id.as_str());
        }
        StopWindowCapture { window_id } => {
            span.record("cmd.kind", "StopWindowCapture");
            span.record("window.id", window_id.as_str());
        }
    }
}

async fn handle_command(
    cmd: BackendToSidecar,
    pm: &mut ProcessManager,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    window_router: &WindowRouter,
) {
    match cmd {
        BackendToSidecar::SpawnProcess {
            request_id,
            command,
            args,
        } => match pm.spawn(&command, &args).await {
            Ok(pid) => {
                let _ = tx.send(SidecarToBackend::ProcessSpawned { request_id, pid });
            }
            Err(message) => {
                tracing::warn!(error = %message, "SpawnProcess failed");
                x11_web_telemetry::mark_span_error("spawn_failed", message.clone());
                let _ = tx.send(SidecarToBackend::Error {
                    request_id: Some(request_id),
                    message,
                });
            }
        },
        BackendToSidecar::KillProcess { request_id, pid } => match pm.kill(pid).await {
            Ok(()) => {
                let _ = tx.send(SidecarToBackend::ProcessKilled { request_id, pid });
                let _ = tx.send(SidecarToBackend::ProcessExited {
                    pid,
                    exit_code: None,
                });
            }
            Err(message) => {
                tracing::warn!(error = %message, "KillProcess failed");
                x11_web_telemetry::mark_span_error("kill_failed", message.clone());
                let _ = tx.send(SidecarToBackend::Error {
                    request_id: Some(request_id),
                    message,
                });
            }
        },
        BackendToSidecar::ListProcesses { request_id } => {
            let processes = pm.list();
            let _ = tx.send(SidecarToBackend::ProcessList {
                request_id,
                processes,
            });
        }
        // The `false` arm is logged rather than ignored (the X11
        // sidecar ignores it) because on Wayland a routing miss is
        // ambiguous in a way it isn't on X11: it means either "no such
        // window" or "the compositor thread hasn't installed its
        // command channel yet", and both look exactly like working
        // delivery from the backend's side. `debug!` not `warn!` —
        // events for a window the client just destroyed are normal.
        BackendToSidecar::InputEvent { window_id, event } => {
            if !window_router.send_input(&window_id, event) {
                tracing::debug!(%window_id, "InputEvent not routed: unknown or unmapped window");
            }
        }
        BackendToSidecar::ResizeWindow {
            window_id,
            width,
            height,
        } => {
            if !window_router.send_resize(&window_id, width, height) {
                tracing::debug!(%window_id, "ResizeWindow not routed: unknown or unmapped window");
            }
        }
        // This sidecar streams unconditionally — these on-demand
        // capture controls only matter for sidecars (currently macOS)
        // that don't auto-stream every enumerated window.
        BackendToSidecar::StartWindowCapture { .. }
        | BackendToSidecar::StopWindowCapture { .. } => {}
    }
}

/// Compress `DisplayUpdate::PutImage`'s raw-RGBA payload into the
/// WebP-lossless format the wire (and the frontend's `createImageBitmap`
/// decoder) expects. All other variants are returned unchanged.
fn encode_for_wire(update: DisplayUpdate) -> DisplayUpdate {
    match update {
        DisplayUpdate::PutImage {
            window_id,
            x,
            y,
            width,
            height,
            data,
        } => {
            let encoded =
                x11_web_pixel_codec::encode_rgba_lossless(&data, width as u32, height as u32);
            DisplayUpdate::PutImage {
                window_id,
                x,
                y,
                width,
                height,
                data: encoded,
            }
        }
        other => other,
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

#[cfg(test)]
mod tests {
    use super::{parse_screen_size, DEFAULT_SCREEN_SIZE};

    #[test]
    fn screen_size_parses_and_falls_back() {
        assert_eq!(parse_screen_size(Some("1920x1080")), (1920, 1080));
        assert_eq!(parse_screen_size(Some(" 800 X 600 ")), (800, 600));
        assert_eq!(parse_screen_size(None), DEFAULT_SCREEN_SIZE);
        assert_eq!(parse_screen_size(Some("garbage")), DEFAULT_SCREEN_SIZE);
        // 0-sized outputs are rejected: wl_output.mode with a zero
        // dimension makes toolkits compute a zero-sized window.
        assert_eq!(parse_screen_size(Some("0x600")), DEFAULT_SCREEN_SIZE);
        // 70000 doesn't fit in u16 — must fall back, not wrap.
        assert_eq!(parse_screen_size(Some("70000x600")), DEFAULT_SCREEN_SIZE);
    }
}
