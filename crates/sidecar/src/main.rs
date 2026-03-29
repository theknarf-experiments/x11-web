mod fonts;
mod framebuffer;
mod render;
mod xserver;

use std::collections::HashMap;
use std::process::Stdio;

use futures::{SinkExt, StreamExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};
use x11_web_protocol::*;

use crate::xserver::{TaggedDisplayUpdate, X11Server};

struct ProcessManager {
    processes: HashMap<u32, ManagedProcess>,
    display_string: String,
}

struct ManagedProcess {
    child: Child,
    command: String,
}

impl ProcessManager {
    fn new(display_string: String) -> Self {
        Self {
            processes: HashMap::new(),
            display_string,
        }
    }

    async fn spawn(&mut self, command: &str, args: &[String]) -> Result<u32, String> {
        let child = Command::new(command)
            .args(args)
            .env("DISPLAY", &self.display_string)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let backend_url =
        std::env::var("BACKEND_URL").unwrap_or_else(|_| "ws://127.0.0.1:3001/ws/sidecar".into());
    let sidecar_name =
        std::env::var("SIDECAR_NAME").unwrap_or_else(|_| hostname().unwrap_or("sidecar".into()));
    let display_number: u32 = std::env::var("DISPLAY_NUMBER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(99);

    // Start X11 server
    let (display_tx, mut display_rx) = mpsc::unbounded_channel::<TaggedDisplayUpdate>();
    let (client_connected_tx, mut client_connected_rx) = mpsc::unbounded_channel::<(String, u32)>();
    let window_router = crate::xserver::WindowRouter::new();
    let x11_server = X11Server::new(
        display_number,
        display_tx,
        client_connected_tx,
        window_router.clone(),
    );
    let display_string = x11_server.display_string();
    info!("Starting X11 server on DISPLAY={}", display_string);

    tokio::spawn(async move {
        if let Err(e) = x11_server.run().await {
            error!("X11 server error: {e}");
        }
    });

    info!("Connecting to backend at {}", backend_url);

    loop {
        match connect_async(&backend_url).await {
            Ok((ws_stream, _)) => {
                info!("Connected to backend");
                run_session(
                    ws_stream,
                    &sidecar_name,
                    &display_string,
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
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn run_session(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    sidecar_name: &str,
    display_string: &str,
    display_rx: &mut mpsc::UnboundedReceiver<TaggedDisplayUpdate>,
    window_router: &crate::xserver::WindowRouter,
    client_connected_rx: &mut mpsc::UnboundedReceiver<(String, u32)>,
) {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let mut process_manager = ProcessManager::new(display_string.to_string());

    // Send registration
    let register = SidecarToBackend::Register {
        sidecar_name: sidecar_name.to_string(),
    };
    let json = serde_json::to_string(&register).unwrap();
    if ws_tx.send(Message::Text(json.into())).await.is_err() {
        return;
    }

    // Channel for outgoing messages
    let (tx, mut rx) = mpsc::unbounded_channel::<SidecarToBackend>();

    // Heartbeat task
    let tx_heartbeat = tx.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut heartbeat_interval = interval(Duration::from_secs(30));
        loop {
            heartbeat_interval.tick().await;
            if tx_heartbeat.send(SidecarToBackend::Heartbeat).is_err() {
                break;
            }
        }
    });

    // Forward outgoing messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap();
            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Main event loop
    let mut check_interval = interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(cmd) = serde_json::from_str::<BackendToSidecar>(&text) {
                            handle_command(cmd, &mut process_manager, &tx, window_router).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            Some((client_id, update)) = display_rx.recv() => {
                let _ = tx.send(SidecarToBackend::DisplayUpdate { client_id, update });
            }
            Some((client_id, peer_pid)) = client_connected_rx.recv() => {
                // Find which spawned process this X11 client belongs to by
                // walking up the process tree from the peer PID.
                let spawned_pids: Vec<u32> = process_manager.list().iter().map(|p| p.pid).collect();
                let pid = find_ancestor_pid(peer_pid, &spawned_pids);

                if let Some(pid) = pid {
                    let command = process_manager.get_command(pid).unwrap_or("").to_string();
                    info!("Process {pid} ({command}) (peer {peer_pid}) connected as X11 client {client_id}");
                    let _ = tx.send(SidecarToBackend::ProcessConnected { pid, client_id, command });
                } else {
                    info!("X11 client {client_id} connected (peer PID {peer_pid}, no matching spawned process)");
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

    heartbeat_task.abort();
    send_task.abort();
}

async fn handle_command(
    cmd: BackendToSidecar,
    pm: &mut ProcessManager,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    window_router: &crate::xserver::WindowRouter,
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
            window_router.send_input(&window_id, event);
        }
        BackendToSidecar::RequestRedraw { window_id } => {
            // TODO: implement redraw via router
            let _ = window_id;
        }
        BackendToSidecar::ResizeWindow {
            window_id,
            width,
            height,
        } => {
            window_router.send_resize(&window_id, width, height);
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
