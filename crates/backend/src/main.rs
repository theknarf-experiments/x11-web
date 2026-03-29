use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use uuid::Uuid;
use x11_web_protocol::*;

#[derive(Clone)]
struct AppState {
    sidecars: Arc<RwLock<HashMap<String, SidecarConnection>>>,
    frontends: Arc<RwLock<HashMap<String, FrontendConnection>>>,
    /// Registry of connected X11 processes: client_id → (sidecar_id, pid)
    connected_processes: Arc<RwLock<HashMap<String, (String, u32, String)>>>,
    /// Window state for position/color sync: client_id → WindowState
    window_states: Arc<RwLock<HashMap<String, WindowState>>>,
    /// Display update buffer per client_id for replay on frontend connect
    display_buffers: Arc<RwLock<HashMap<String, Vec<BackendToFrontend>>>>,
}

struct SidecarConnection {
    info: SidecarInfo,
    tx: mpsc::UnboundedSender<BackendToSidecar>,
}

struct FrontendConnection {
    tx: mpsc::UnboundedSender<BackendToFrontend>,
    subscribed_sidecars: Vec<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = AppState {
        sidecars: Arc::new(RwLock::new(HashMap::new())),
        frontends: Arc::new(RwLock::new(HashMap::new())),
        connected_processes: Arc::new(RwLock::new(HashMap::new())),
        window_states: Arc::new(RwLock::new(HashMap::new())),
        display_buffers: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/ws/sidecar", get(sidecar_ws_handler))
        .route("/ws/frontend", get(frontend_ws_handler))
        .route("/health", get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    info!("Backend listening on 0.0.0.0:3001");
    axum::serve(listener, app).await.unwrap();
}

async fn sidecar_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_sidecar_ws(socket, state))
}

async fn handle_sidecar_ws(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendToSidecar>();
    let sidecar_id = Uuid::new_v4().to_string();

    // Forward messages from channel to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap();
            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Wait for registration message
    let sidecar_name = loop {
        match ws_rx.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(msg) = serde_json::from_str::<SidecarToBackend>(&text) {
                    if let SidecarToBackend::Register { sidecar_name } = msg {
                        break sidecar_name;
                    }
                }
            }
            _ => return,
        }
    };

    let info = SidecarInfo {
        id: sidecar_id.clone(),
        name: sidecar_name,
    };

    info!("Sidecar connected: {} ({})", info.name, info.id);

    // Register sidecar
    {
        let mut sidecars = state.sidecars.write().await;
        sidecars.insert(
            sidecar_id.clone(),
            SidecarConnection {
                info: info.clone(),
                tx,
            },
        );
    }

    // Notify all frontends
    notify_frontends_sidecar_connected(&state, info.clone()).await;

    // Process incoming messages from sidecar
    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(msg) = serde_json::from_str::<SidecarToBackend>(&text) else {
            continue;
        };

        match msg {
            SidecarToBackend::Heartbeat => {}
            SidecarToBackend::ProcessSpawned { request_id, pid } => {
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::CommandResult {
                        request_id,
                        success: true,
                        message: format!("Process spawned with pid {pid}"),
                    },
                )
                .await;
            }
            SidecarToBackend::ProcessKilled { request_id, pid } => {
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::CommandResult {
                        request_id,
                        success: true,
                        message: format!("Process {pid} killed"),
                    },
                )
                .await;
            }
            SidecarToBackend::ProcessExited { pid, exit_code } => {
                // Clean up from registries
                let mut procs = state.connected_processes.write().await;
                let mut states = state.window_states.write().await;
                // Find client_ids for this pid to clean up display buffers
                let client_ids: Vec<String> = procs
                    .iter()
                    .filter(|(_, (_, p, _))| *p == pid)
                    .map(|(cid, _)| cid.clone())
                    .collect();
                procs.retain(|_, (_, p, _)| *p != pid);
                states.retain(|_, ws| ws.pid != pid);
                drop(procs);
                drop(states);
                {
                    let mut bufs = state.display_buffers.write().await;
                    for cid in &client_ids {
                        bufs.remove(cid);
                    }
                }
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::ProcessExited {
                        sidecar_id: sidecar_id.clone(),
                        pid,
                        exit_code,
                    },
                )
                .await;
            }
            SidecarToBackend::ProcessList {
                request_id: _,
                processes,
            } => {
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::ProcessList {
                        sidecar_id: sidecar_id.clone(),
                        processes,
                    },
                )
                .await;
            }
            SidecarToBackend::ProcessConnected { pid, client_id, command } => {
                // Register in process registry
                state
                    .connected_processes
                    .write()
                    .await
                    .insert(client_id.clone(), (sidecar_id.clone(), pid, command.clone()));
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::ProcessConnected {
                        sidecar_id: sidecar_id.clone(),
                        pid,
                        client_id,
                        command,
                    },
                )
                .await;
            }
            SidecarToBackend::DisplayUpdate { client_id, update } => {
                let is_put_image = matches!(update, x11_web_protocol::DisplayUpdate::PutImage { .. });
                let put_image_wid = match &update {
                    x11_web_protocol::DisplayUpdate::PutImage { window_id, .. } => Some(window_id.clone()),
                    _ => None,
                };
                let msg = BackendToFrontend::DisplayUpdate {
                    sidecar_id: sidecar_id.clone(),
                    client_id: client_id.clone(),
                    update,
                };

                // Buffer for replay to new frontends.
                // Keep only the latest PutImage per window_id.
                // Lifecycle events (WindowCreated, etc.) are always kept.
                {
                    let mut bufs = state.display_buffers.write().await;
                    let buf = bufs.entry(client_id.clone()).or_default();
                    if is_put_image {
                        if let Some(wid) = put_image_wid {
                            buf.retain(|m| {
                                if let BackendToFrontend::DisplayUpdate { update: u, .. } = m {
                                    if let x11_web_protocol::DisplayUpdate::PutImage { window_id, .. } = u {
                                        return window_id != &wid;
                                    }
                                }
                                true
                            });
                        }
                    }
                    buf.push(msg.clone());
                }

                // Forward to subscribed frontends
                let frontends = state.frontends.read().await;
                for frontend in frontends.values() {
                    if frontend.subscribed_sidecars.contains(&sidecar_id) {
                        let _ = frontend.tx.send(msg.clone());
                    }
                }
            }
            SidecarToBackend::Error {
                request_id,
                message,
            } => {
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::CommandResult {
                        request_id: request_id.unwrap_or_default(),
                        success: false,
                        message,
                    },
                )
                .await;
            }
            SidecarToBackend::Register { .. } => {} // Already handled
        }
    }

    // Sidecar disconnected
    info!("Sidecar disconnected: {}", sidecar_id);
    state.sidecars.write().await.remove(&sidecar_id);
    // Clean up processes, window states, and display buffers for this sidecar
    {
        let mut procs = state.connected_processes.write().await;
        let client_ids: Vec<String> = procs
            .iter()
            .filter(|(_, (sid, _, _))| sid == &sidecar_id)
            .map(|(cid, _)| cid.clone())
            .collect();
        procs.retain(|_, (sid, _, _)| sid != &sidecar_id);
        drop(procs);
        state
            .window_states
            .write()
            .await
            .retain(|_, ws| ws.sidecar_id != sidecar_id);
        let mut bufs = state.display_buffers.write().await;
        for cid in &client_ids {
            bufs.remove(cid);
        }
    }
    send_task.abort();

    // Notify frontends
    let frontends = state.frontends.read().await;
    for frontend in frontends.values() {
        let _ = frontend.tx.send(BackendToFrontend::SidecarDisconnected {
            sidecar_id: sidecar_id.clone(),
        });
    }
}

async fn frontend_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_frontend_ws(socket, state))
}

async fn handle_frontend_ws(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendToFrontend>();
    let frontend_id = Uuid::new_v4().to_string();

    info!("Frontend connected: {}", frontend_id);

    // Forward messages from channel to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap();
            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Register frontend
    {
        let mut frontends = state.frontends.write().await;
        frontends.insert(
            frontend_id.clone(),
            FrontendConnection {
                tx: tx.clone(),
                subscribed_sidecars: Vec::new(),
            },
        );
    }

    // Send current sidecar list
    {
        let sidecars = state.sidecars.read().await;
        let sidecar_list: Vec<SidecarInfo> = sidecars.values().map(|s| s.info.clone()).collect();
        let _ = tx.send(BackendToFrontend::SidecarList {
            sidecars: sidecar_list,
        });
    }

    // Send currently connected processes
    {
        let procs = state.connected_processes.read().await;
        let processes: Vec<ConnectedProcessInfo> = procs
            .iter()
            .map(|(client_id, (sidecar_id, pid, command))| ConnectedProcessInfo {
                sidecar_id: sidecar_id.clone(),
                pid: *pid,
                client_id: client_id.clone(),
                command: command.clone(),
            })
            .collect();
        if !processes.is_empty() {
            let _ = tx.send(BackendToFrontend::ConnectedProcessesList { processes });
        }
    }

    // Send current window states
    {
        let states = state.window_states.read().await;
        let windows: Vec<WindowState> = states.values().cloned().collect();
        if !windows.is_empty() {
            let _ = tx.send(BackendToFrontend::WindowStateList { windows });
        }
    }

    // Request process lists from all connected sidecars so the frontend
    // gets command names (e.g. "xterm", "firefox-esr") immediately.
    {
        let sidecars = state.sidecars.read().await;
        for (sidecar_id, sidecar) in sidecars.iter() {
            let _ = sidecar.tx.send(BackendToSidecar::ListProcesses {
                request_id: format!("init-{}", sidecar_id),
            });
        }
    }

    // Process incoming messages from frontend
    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(msg) = serde_json::from_str::<FrontendToBackend>(&text) else {
            continue;
        };

        match msg {
            FrontendToBackend::SpawnProcess {
                request_id,
                sidecar_id,
                command,
                args,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::SpawnProcess {
                        request_id,
                        command,
                        args,
                    },
                )
                .await;
            }
            FrontendToBackend::KillProcess {
                request_id,
                sidecar_id,
                pid,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::KillProcess { request_id, pid },
                )
                .await;
            }
            FrontendToBackend::ListProcesses {
                request_id,
                sidecar_id,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::ListProcesses { request_id },
                )
                .await;
            }
            FrontendToBackend::SubscribeDisplay { sidecar_id } => {
                let mut frontends = state.frontends.write().await;
                if let Some(frontend) = frontends.get_mut(&frontend_id) {
                    if !frontend.subscribed_sidecars.contains(&sidecar_id) {
                        frontend.subscribed_sidecars.push(sidecar_id.clone());

                        // Replay buffered display updates for all clients of this sidecar
                        let bufs = state.display_buffers.read().await;
                        let procs = state.connected_processes.read().await;
                        for (client_id, (sid, _, _)) in procs.iter() {
                            if sid == &sidecar_id {
                                if let Some(buf) = bufs.get(client_id) {
                                    for msg in buf {
                                        let _ = frontend.tx.send(msg.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            FrontendToBackend::RequestRedraw {
                sidecar_id,
                window_id,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::RequestRedraw { window_id },
                )
                .await;
            }
            FrontendToBackend::InputEvent {
                sidecar_id,
                window_id,
                event,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::InputEvent { window_id, event },
                )
                .await;
            }
            FrontendToBackend::ResizeWindow {
                sidecar_id,
                window_id,
                width,
                height,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::ResizeWindow {
                        window_id,
                        width,
                        height,
                    },
                )
                .await;
            }
            FrontendToBackend::UpdateWindowState {
                client_id,
                sidecar_id,
                x,
                y,
                color,
            } => {
                // Look up pid from connected_processes
                let pid = {
                    let procs = state.connected_processes.read().await;
                    procs.get(&client_id).map(|(_, p, _)| *p).unwrap_or(0)
                };

                // Store window state
                state.window_states.write().await.insert(
                    client_id.clone(),
                    WindowState {
                        client_id: client_id.clone(),
                        sidecar_id,
                        pid,
                        x,
                        y,
                        color: color.clone(),
                    },
                );

                // Broadcast to OTHER frontends
                let frontends = state.frontends.read().await;
                for (fid, frontend) in frontends.iter() {
                    if fid != &frontend_id {
                        let _ = frontend.tx.send(BackendToFrontend::WindowStateChanged {
                            client_id: client_id.clone(),
                            x,
                            y,
                            color: color.clone(),
                        });
                    }
                }
            }
        }
    }

    // Frontend disconnected
    info!("Frontend disconnected: {}", frontend_id);
    state.frontends.write().await.remove(&frontend_id);
    send_task.abort();
}

async fn forward_to_sidecar(state: &AppState, sidecar_id: &str, msg: BackendToSidecar) {
    let sidecars = state.sidecars.read().await;
    if let Some(sidecar) = sidecars.get(sidecar_id) {
        let _ = sidecar.tx.send(msg);
    } else {
        warn!("Sidecar not found: {}", sidecar_id);
    }
}

async fn broadcast_to_frontends(state: &AppState, msg: BackendToFrontend) {
    let frontends = state.frontends.read().await;
    for frontend in frontends.values() {
        let _ = frontend.tx.send(msg.clone());
    }
}

async fn notify_frontends_sidecar_connected(state: &AppState, sidecar: SidecarInfo) {
    let frontends = state.frontends.read().await;
    for frontend in frontends.values() {
        let _ = frontend.tx.send(BackendToFrontend::SidecarConnected {
            sidecar: sidecar.clone(),
        });
    }
}
