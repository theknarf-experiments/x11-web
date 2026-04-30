mod audio;
mod colors;
mod compose;
mod fonts;
mod framebuffer;
mod menus;
#[cfg(feature = "osmesa")]
mod osmesa;
mod xinput2;
mod xserver;

use std::collections::HashMap;
use std::process::Stdio;

use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use x11_web_protocol::*;
use x11_web_wire::bridge as wire_bridge;
use x11_web_wire::conn::{dial, DialedConnection};
use x11_web_wire::tls::parse_fingerprint;
use x11_web_wire::{wire_capnp, BackendToSidecar, SidecarToBackend};

use crate::xserver::{TaggedDisplayUpdate, X11Server};

struct ProcessManager {
    processes: HashMap<u32, ManagedProcess>,
    display_string: String,
    /// `DBUS_SESSION_BUS_ADDRESS` for the per-sidecar session bus.
    /// `None` if dbus-daemon failed to start. When set, every spawned
    /// X11 app inherits it so GTK / Qt apps can export their AppMenu.
    dbus_session_address: Option<String>,
}

struct ManagedProcess {
    child: Child,
    command: String,
}

impl ProcessManager {
    fn new(display_string: String, dbus_session_address: Option<String>) -> Self {
        Self {
            processes: HashMap::new(),
            display_string,
            dbus_session_address,
        }
    }

    async fn spawn(&mut self, command: &str, args: &[String]) -> Result<u32, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .env("DISPLAY", &self.display_string)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(addr) = &self.dbus_session_address {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", addr);
        }
        if let Ok(xauth) = std::env::var("XAUTHORITY") {
            cmd.env("XAUTHORITY", xauth);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {e}"))?;

        let pid = child.id().ok_or("Failed to get process id")?;
        self.processes.insert(
            pid,
            ManagedProcess {
                child,
                command: command.to_string(),
            },
        );
        info!(
            "Spawned process: {} (pid {}) with DISPLAY={}",
            command, pid, self.display_string
        );
        Ok(pid)
    }

    async fn kill(&mut self, pid: u32) -> Result<(), String> {
        if let Some(mut proc) = self.processes.remove(&pid) {
            proc.child
                .kill()
                .await
                .map_err(|e| format!("Failed to kill process {pid}: {e}"))?;
            info!("Killed process: {}", pid);
            Ok(())
        } else {
            Err(format!("Process {pid} not found"))
        }
    }

    fn get_command(&self, pid: u32) -> Option<&str> {
        self.processes.get(&pid).map(|p| p.command.as_str())
    }

    fn list(&self) -> Vec<ProcessInfo> {
        self.processes
            .iter()
            .map(|(&pid, proc)| ProcessInfo {
                pid,
                command: proc.command.clone(),
            })
            .collect()
    }

    async fn check_exited(&mut self) -> Vec<(u32, Option<i32>)> {
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();

        for (&pid, proc) in self.processes.iter_mut() {
            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    exited.push((pid, status.code()));
                    to_remove.push(pid);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("Error checking process {pid}: {e}");
                    to_remove.push(pid);
                    exited.push((pid, None));
                }
            }
        }

        for pid in to_remove {
            self.processes.remove(&pid);
        }

        exited
    }
}

/// Result of starting the per-sidecar DBus session bus. Holding the
/// `Child` keeps the daemon alive for the lifetime of the sidecar
/// process; dropping it would let dbus-daemon exit.
struct DbusSession {
    address: String,
    daemon: Child,
}

/// Spawn a private `dbus-daemon --session` and return its bus address.
///
/// The daemon prints its address on stdout when invoked with
/// `--print-address`. We read the first line, then leave the process
/// running for the lifetime of the sidecar. Errors are non-fatal: if
/// dbus-daemon isn't installed (e.g. local dev outside the container)
/// the sidecar continues without DBus support and AppMenu export
/// simply won't work.
async fn start_dbus_session() -> Option<DbusSession> {
    let mut child = match Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("dbus-daemon not started ({e}); AppMenu export disabled");
            return None;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            warn!("dbus-daemon stdout unavailable; killing daemon");
            let _ = child.start_kill();
            return None;
        }
    };

    let mut lines = BufReader::new(stdout).lines();
    let address = match tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await {
        Ok(Ok(Some(line))) => line.trim().to_string(),
        Ok(Ok(None)) => {
            warn!("dbus-daemon closed stdout before printing address");
            let _ = child.start_kill();
            return None;
        }
        Ok(Err(e)) => {
            warn!("Failed reading dbus-daemon stdout: {e}");
            let _ = child.start_kill();
            return None;
        }
        Err(_) => {
            warn!("dbus-daemon address read timed out after 5s");
            let _ = child.start_kill();
            return None;
        }
    };

    if address.is_empty() {
        warn!("dbus-daemon printed empty address");
        let _ = child.start_kill();
        return None;
    }

    info!("Started session DBus at {address}");
    Some(DbusSession {
        address,
        daemon: child,
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Attempt to load OSMesa for GLX software rendering
    #[cfg(feature = "osmesa")]
    {
        if osmesa::init() {
            info!("OSMesa software OpenGL rendering available");
        } else {
            warn!("OSMesa not available — GLX will return stub responses");
        }
    }

    // Install rustls's default crypto provider — quinn's TLS
    // config rejects building without one. `ring` is what the
    // wire crate is feature-gated to.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // `BACKEND_QUIC_ADDR` may be either a literal `IP:PORT` (macOS
    // sidecar against a host backend) or `hostname:PORT` (X11
    // sidecar in Docker reaching `backend:3002` over the network
    // alias). Resolve via DNS at dial time rather than parse here so
    // we re-resolve on each reconnect — handy when the backend
    // container restarts and gets a new IP.
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
    let display_number: u32 = std::env::var("DISPLAY_NUMBER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(99);

    // Start the per-sidecar DBus session bus before anything else so
    // every spawned X11 app inherits DBUS_SESSION_BUS_ADDRESS. The
    // returned handle is held for the lifetime of `main` to keep the
    // daemon alive.
    let dbus_session = start_dbus_session().await;
    let dbus_address = dbus_session.as_ref().map(|s| s.address.clone());

    // Start X11 server
    let (display_tx, mut display_rx) = mpsc::unbounded_channel::<TaggedDisplayUpdate>();
    let (client_connected_tx, mut client_connected_rx) = mpsc::unbounded_channel::<(String, u32)>();
    let window_router = crate::xserver::WindowRouter::new();
    // Clipboard bridge channels
    let (clipboard_notify_tx, mut clipboard_notify_rx) =
        mpsc::unbounded_channel::<crate::xserver::types::ClipboardEvent>();
    let shared_clipboard: crate::xserver::types::SharedClipboard =
        std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
    // MenuTracker connects to the same session bus the apps use; on
    // failure it becomes a no-op so the rest of the sidecar still
    // works without DBus.
    let menu_tracker =
        crate::menus::MenuTracker::new(display_tx.clone(), dbus_address.clone()).await;
    // Watch channel for dynamic screen resize (RandR).
    let (screen_size_tx, screen_size_rx) = tokio::sync::watch::channel((1024u16, 768u16));
    let x11_server = X11Server::new(
        display_number,
        display_tx,
        client_connected_tx,
        window_router.clone(),
        menu_tracker,
        clipboard_notify_tx,
        shared_clipboard.clone(),
        screen_size_rx,
    );
    let display_string = x11_server.display_string();
    let shared_selections = x11_server.shared_selections();

    // Write .Xauthority file and set env var so spawned processes inherit it.
    match x11_server.write_xauthority() {
        Ok(xauth_path) => {
            std::env::set_var("XAUTHORITY", &xauth_path);
            info!("XAUTHORITY={}", xauth_path.display());
        }
        Err(e) => {
            warn!("Failed to write Xauthority file: {e}");
        }
    }

    info!("Starting X11 server on DISPLAY={}", display_string);

    tokio::spawn(async move {
        if let Err(e) = x11_server.run().await {
            error!("X11 server error: {e}");
        }
    });

    // Start PulseAudio for audio capture/playback.
    let _pulse_daemon = audio::start_pulseaudio().await;

    info!("Connecting to backend at {backend_addr_str} (server-name={server_name})");

    loop {
        let fingerprint = match read_fingerprint(&fingerprint_source) {
            Ok(fp) => fp,
            Err(e) => {
                warn!("Fingerprint not available yet: {e}. Retrying in 2s.");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let backend_addr: SocketAddr = match resolve_backend_addr(&backend_addr_str).await {
            Ok(a) => a,
            Err(e) => {
                warn!("Failed to resolve {backend_addr_str}: {e}. Retrying in 2s.");
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
                    &display_string,
                    dbus_address.clone(),
                    &mut display_rx,
                    &window_router,
                    &mut client_connected_rx,
                    &screen_size_tx,
                    &mut clipboard_notify_rx,
                    &shared_clipboard,
                    &shared_selections,
                )
                .await;
                warn!("Disconnected from backend, reconnecting in 5s...");
            }
            Err(e) => {
                error!("Failed to connect to backend: {e}");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Where the fingerprint comes from. Mirror of the macOS sidecar's
/// equivalent — re-resolved on every dial attempt so a backend
/// restart picks up the new fingerprint without restarting the
/// sidecar.
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

async fn run_session(
    connection: DialedConnection,
    display_string: &str,
    dbus_address: Option<String>,
    display_rx: &mut mpsc::UnboundedReceiver<TaggedDisplayUpdate>,
    window_router: &crate::xserver::WindowRouter,
    client_connected_rx: &mut mpsc::UnboundedReceiver<(String, u32)>,
    screen_size_tx: &crate::xserver::types::ScreenSizeTx,
    clipboard_notify_rx: &mut mpsc::UnboundedReceiver<crate::xserver::types::ClipboardEvent>,
    shared_clipboard: &crate::xserver::types::SharedClipboard,
    shared_selections: &crate::xserver::types::SharedSelections,
) {
    let DialedConnection {
        mut reader,
        mut writer,
        ..
    } = connection;
    let mut process_manager = ProcessManager::new(display_string.to_string(), dbus_address);

    // Outgoing messages: every event source pushes SidecarToBackend
    // here, the events loop drains it through the wire writer.
    let (tx, mut rx) = mpsc::unbounded_channel::<SidecarToBackend>();

    // Incoming messages: the recv loop owns the wire reader, decodes
    // Cap'n Proto, forwards BackendToSidecar over this channel so the
    // events loop can keep `process_manager` borrowed exclusively.
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<BackendToSidecar>();

    // Heartbeat — pushes Heartbeat into tx every 30s.
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
                Ok(cmd) => {
                    if in_tx.send(cmd).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    warn!("ToSidecar translate: {e:?}");
                }
            }
        }
    };

    // Events + send loop owns the wire writer plus every other
    // event source.
    let events_loop = async {
        let mut check_interval = interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                Some(cmd) = in_rx.recv() => {
                    handle_command(
                        cmd,
                        &mut process_manager,
                        &tx,
                        window_router,
                        screen_size_tx,
                        shared_clipboard,
                        shared_selections,
                    ).await;
                }
                Some(msg) = rx.recv() => {
                    let Some(builder) = wire_bridge::build_from_sidecar(&msg) else {
                        continue;
                    };
                    if let Err(e) = writer.write_message(&builder).await {
                        warn!("wire write failed: {e}");
                        return;
                    }
                }
                Some((client_id, update)) = display_rx.recv() => {
                    let _ = tx.send(SidecarToBackend::DisplayUpdate { client_id, update });
                }
                Some((client_id, peer_pid)) = client_connected_rx.recv() => {
                    let spawned_pids: Vec<u32> = process_manager.list().iter().map(|p| p.pid).collect();
                    if let Some(pid) = find_ancestor_pid(peer_pid, &spawned_pids) {
                        let command = process_manager.get_command(pid).unwrap_or("").to_string();
                        info!(
                            "Process {pid} ({command}) (peer {peer_pid}) connected as X11 client {client_id}"
                        );
                        let _ = tx.send(SidecarToBackend::ProcessConnected { pid, client_id, command });
                    } else {
                        info!(
                            "X11 client {client_id} connected (peer PID {peer_pid}, no matching spawned process)"
                        );
                    }
                }
                Some(clipboard_event) = clipboard_notify_rx.recv() => {
                    let crate::xserver::types::ClipboardEvent::OwnerChanged { selection, owner }
                        = clipboard_event;
                    if owner != 0 {
                        let mime_types = vec!["text/plain".into(), "UTF8_STRING".into()];
                        let _ = tx.send(SidecarToBackend::ClipboardOffer {
                            selection,
                            mime_types,
                        });
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

async fn handle_command(
    cmd: BackendToSidecar,
    pm: &mut ProcessManager,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    window_router: &crate::xserver::WindowRouter,
    screen_size_tx: &crate::xserver::types::ScreenSizeTx,
    shared_clipboard: &crate::xserver::types::SharedClipboard,
    shared_selections: &crate::xserver::types::SharedSelections,
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
        BackendToSidecar::InputEvent { window_id, event } => {
            if !window_router.send_input(&window_id, event) {
                let _ = tx.send(SidecarToBackend::InputDropped {
                    window_id,
                    reason: "no route entry for window UUID".into(),
                });
            }
        }
        BackendToSidecar::RequestRedraw { window_id } => {
            window_router.send_resize(&window_id, 0, 0);
        }
        BackendToSidecar::ResizeWindow {
            window_id,
            width,
            height,
        } => {
            window_router.send_resize(&window_id, width, height);
        }
        BackendToSidecar::RequestClipboard {
            selection,
            mime_type,
        } => {
            info!("Clipboard request: selection={selection} mime={mime_type}");
            if let Ok(sels) = shared_selections.lock() {
                if sels.values().next().is_some() {
                    info!(
                        "Selection owner exists — clipboard data flows via X11 selection protocol"
                    );
                }
            }
        }
        BackendToSidecar::SetClipboard {
            selection,
            mime_type,
            data,
        } => {
            info!(
                "Clipboard set: selection={selection} mime={mime_type} len={}",
                data.len()
            );
            if let Ok(mut cb) = shared_clipboard.lock() {
                cb.insert(
                    selection,
                    crate::xserver::types::ServerClipboardData { mime_type, data },
                );
            }
        }
        BackendToSidecar::ResizeScreen { width, height } => {
            if width > 0 && height > 0 {
                info!("Screen resize request: {width}x{height}");
                let _ = screen_size_tx.send((width, height));
            }
        }
    }
}

/// Walk up the process tree from `peer_pid` to find the first ancestor
/// that is in `spawned_pids`. Returns `Some(pid)` if found.
/// This uses /proc/<pid>/status to read PPid.
fn find_ancestor_pid(peer_pid: u32, spawned_pids: &[u32]) -> Option<u32> {
    if peer_pid == 0 {
        return None;
    }
    // Check if the peer itself is a spawned process
    if spawned_pids.contains(&peer_pid) {
        return Some(peer_pid);
    }
    // Walk up the tree
    let mut current = peer_pid;
    for _ in 0..50 {
        let ppid = get_ppid(current);
        match ppid {
            Some(0) | Some(1) | None => return None, // reached init or failed
            Some(p) => {
                if spawned_pids.contains(&p) {
                    return Some(p);
                }
                current = p;
            }
        }
    }
    None
}

/// Read the parent PID of a process from /proc/<pid>/status.
fn get_ppid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn hostname() -> Option<String> {
    std::env::var("HOSTNAME").ok().or_else(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })
}
