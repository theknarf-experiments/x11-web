mod fonts;
mod framebuffer;
mod render;
mod xserver;

use std::collections::{HashMap, VecDeque};
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
    let (input_tx, _) =
        tokio::sync::broadcast::channel::<(String, x11_web_protocol::InputEvent)>(256);
    let (resize_tx, _) = tokio::sync::broadcast::channel::<(String, u32, u16, u16)>(64);
    let (client_connected_tx, mut client_connected_rx) = mpsc::unbounded_channel::<String>();
    let x11_server = X11Server::new(
        display_number,
        display_tx,
        input_tx.clone(),
        resize_tx.clone(),
        client_connected_tx,
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
                    &input_tx,
                    &resize_tx,
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
    input_tx: &tokio::sync::broadcast::Sender<(String, x11_web_protocol::InputEvent)>,
    resize_tx: &tokio::sync::broadcast::Sender<(String, u32, u16, u16)>,
    client_connected_rx: &mut mpsc::UnboundedReceiver<String>,
) {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let mut process_manager = ProcessManager::new(display_string.to_string());
    let mut pending_pids: VecDeque<u32> = VecDeque::new();
    let mut last_spawned_pid: Option<u32> = None;

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
                            handle_command(cmd, &mut process_manager, &tx, input_tx, resize_tx, &mut pending_pids).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            Some((client_id, update)) = display_rx.recv() => {
                let _ = tx.send(SidecarToBackend::DisplayUpdate { client_id, update });
            }
            Some(client_id) = client_connected_rx.recv() => {
                // Associate the new X11 client with the most recently spawned process.
                // If no pending PID, use the last associated PID (for child processes
                // like Firefox content processes that open additional X11 connections).
                let pid = if let Some(pid) = pending_pids.pop_front() {
                    last_spawned_pid = Some(pid);
                    pid
                } else if let Some(pid) = last_spawned_pid {
                    pid
                } else {
                    info!("X11 client {client_id} connected (no process to associate)");
                    continue;
                };
                info!("Process {pid} connected as X11 client {client_id}");
                let _ = tx.send(SidecarToBackend::ProcessConnected { pid, client_id });
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
    input_tx: &tokio::sync::broadcast::Sender<(String, x11_web_protocol::InputEvent)>,
    resize_tx: &tokio::sync::broadcast::Sender<(String, u32, u16, u16)>,
    pending_pids: &mut VecDeque<u32>,
) {
    match cmd {
        BackendToSidecar::SpawnProcess {
            request_id,
            command,
            args,
        } => match pm.spawn(&command, &args).await {
            Ok(pid) => {
                pending_pids.push_back(pid);
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
        BackendToSidecar::InputEvent { client_id, event } => {
            let _ = input_tx.send((client_id, event));
        }
        BackendToSidecar::RequestRedraw { client_id } => {
            let _ = resize_tx.send((client_id, 0, 0, 0));
        }
        BackendToSidecar::ResizeWindow {
            client_id,
            window_id,
            width,
            height,
        } => {
            let _ = resize_tx.send((client_id, window_id, width, height));
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
