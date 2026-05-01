mod quic;

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
use x11_web_wire::tls::generate_self_signed;
use x11_web_wire::{BackendToSidecar, SidecarToBackend};

#[derive(Clone)]
struct AppState {
    sidecars: Arc<RwLock<HashMap<String, SidecarConnection>>>,
    frontends: Arc<RwLock<HashMap<String, FrontendConnection>>>,
    /// Authoritative per-sidecar list of X11-connected processes.
    /// Mutated by `ProcessConnected` / `ProcessExited` events from
    /// sidecars; broadcast to frontends as `BackendToFrontend::ProcessList`
    /// after every change.
    processes: Arc<RwLock<HashMap<String, Vec<ProcessInfo>>>>,
    /// Window state mirror per `(sidecar_id, client_id, window_id)`,
    /// updated from the X11 lifecycle events the sidecar pushes
    /// (Created / Mapped / Unmapped / Destroyed / Configured / Raised).
    /// `window_order` is the stacking order keyed by the same triple,
    /// last entry on top. The filtered list (mapped + top-level or
    /// override-redirect) is published as `BackendToFrontend::WindowList`
    /// on every change.
    window_track: Arc<RwLock<HashMap<(String, String, String), TrackedWindow>>>,
    window_order: Arc<RwLock<Vec<(String, String, String)>>>,
    /// Window state for position/color sync: client_id → WindowState
    window_states: Arc<RwLock<HashMap<String, WindowState>>>,
    /// Display update buffer per client_id for replay on frontend connect
    display_buffers: Arc<RwLock<HashMap<String, Vec<BackendToFrontend>>>>,
}

struct SidecarConnection {
    info: SidecarInfo,
    tx: mpsc::UnboundedSender<BackendToSidecar>,
}

/// Per-window state mirrored in the backend, keyed by
/// `(sidecar_id, client_id, window_id)`.
#[derive(Clone)]
struct TrackedWindow {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    border_width: u16,
    border_pixel: u32,
    is_top_level: bool,
    override_redirect: bool,
    mapped: bool,
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
        processes: Arc::new(RwLock::new(HashMap::new())),
        window_track: Arc::new(RwLock::new(HashMap::new())),
        window_order: Arc::new(RwLock::new(Vec::new())),
        window_states: Arc::new(RwLock::new(HashMap::new())),
        display_buffers: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/ws/frontend", get(frontend_ws_handler))
        .route("/health", get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    // QUIC sidecar listener — the only sidecar transport. Generates a
    // fresh self-signed cert on startup and prints its SHA-256
    // fingerprint so the operator can copy it into the sidecar's
    // `X11WEB_SERVER_FINGERPRINT` env (or just bind-mount the file
    // it gets persisted to below).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cert = generate_self_signed(vec!["localhost".into()]).expect("self-signed cert generation");
    info!(
        "QUIC sidecar TLS fingerprint (paste into sidecar's \
         X11WEB_SERVER_FINGERPRINT):\n  sha256:{}",
        cert.fingerprint_hex()
    );
    // Persist fingerprint to disk so locally-running sidecars can
    // pick it up without the operator copy-pasting on every backend
    // restart. mprocs polls this file before starting the sidecar.
    //
    // Default path is `$HOME/.x11web-fingerprint`, not workspace-
    // relative. macOS sidecars launch via `open` and don't inherit
    // the operator's CWD; an absolute path under the user's home
    // is the only reliable convention without a wrapper.
    let fp_path = std::env::var("X11WEB_FINGERPRINT_FILE").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.x11web-fingerprint")
    });
    if let Some(parent) = std::path::Path::new(&fp_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&fp_path, cert.fingerprint_hex()) {
        warn!("could not persist fingerprint to {fp_path}: {e}");
    } else {
        info!("Fingerprint also written to {fp_path}");
    }
    let token = std::env::var("X11WEB_BEARER_TOKEN")
        .unwrap_or_else(|_| "dev-token".into())
        .into_bytes();
    let quic_addr: std::net::SocketAddr = std::env::var("X11WEB_QUIC_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3002".into())
        .parse()
        .expect("X11WEB_QUIC_ADDR must be host:port");
    quic::spawn_listener(state.clone(), quic_addr, cert, token);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    info!("Backend listening on 0.0.0.0:3001");
    axum::serve(listener, app).await.unwrap();
}

/// Per-message dispatch from the QUIC sidecar handler. Anything that
/// fans out to frontends or updates internal registries lives here.
async fn dispatch_sidecar_msg(state: &AppState, sidecar_id: &str, msg: SidecarToBackend) {
    let sidecar_id = sidecar_id.to_string();
    {
        match msg {
            SidecarToBackend::Heartbeat => {}
            SidecarToBackend::ProcessSpawned { request_id, pid } => {
                broadcast_to_frontends(
                    state,
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
                    state,
                    BackendToFrontend::CommandResult {
                        request_id,
                        success: true,
                        message: format!("Process {pid} killed"),
                    },
                )
                .await;
            }
            SidecarToBackend::ProcessExited {
                pid,
                exit_code: _,
            } => {
                // Drop matching entries from per-sidecar list and the
                // window-state index; clear any buffered display
                // updates keyed by the freed client_ids.
                let freed_client_ids: Vec<String> = {
                    let mut procs = state.processes.write().await;
                    let list = procs.entry(sidecar_id.clone()).or_default();
                    let freed: Vec<String> = list
                        .iter()
                        .filter(|p| p.pid == pid)
                        .map(|p| p.client_id.clone())
                        .collect();
                    list.retain(|p| p.pid != pid);
                    freed
                };
                state.window_states.write().await.retain(|_, ws| ws.pid != pid);
                let mut window_list_changed = false;
                if !freed_client_ids.is_empty() {
                    let mut bufs = state.display_buffers.write().await;
                    for cid in &freed_client_ids {
                        bufs.remove(cid);
                        if drop_client_windows(state, cid).await {
                            window_list_changed = true;
                        }
                    }
                }
                broadcast_process_list(state, &sidecar_id).await;
                if window_list_changed {
                    broadcast_window_list(state).await;
                }
            }
            SidecarToBackend::ProcessList {
                request_id: _,
                processes: _,
            } => {
                // Sidecar's spawned-process listing is no longer
                // forwarded — the frontend works exclusively from the
                // X11-connected list maintained via ProcessConnected /
                // ProcessExited events.
            }
            SidecarToBackend::ProcessConnected {
                pid,
                client_id,
                command,
            } => {
                {
                    let mut procs = state.processes.write().await;
                    let list = procs.entry(sidecar_id.clone()).or_default();
                    list.retain(|p| p.client_id != client_id);
                    list.push(ProcessInfo {
                        pid,
                        client_id,
                        command,
                    });
                }
                broadcast_process_list(state, &sidecar_id).await;
            }
            SidecarToBackend::DisplayUpdate { client_id, update } => {
                // Window-lifecycle variants are absorbed by the
                // backend's tracker; the frontend only sees the
                // resulting `WindowList`. Everything else is forwarded
                // through the existing per-sidecar subscription path.
                if apply_window_lifecycle(state, &sidecar_id, &client_id, &update).await {
                    broadcast_window_list(state).await;
                    return;
                }

                let put_image_wid = match &update {
                    x11_web_protocol::DisplayUpdate::PutImage { window_id, .. } => {
                        Some(window_id.clone())
                    }
                    _ => None,
                };
                let msg = BackendToFrontend::DisplayUpdate {
                    sidecar_id: sidecar_id.clone(),
                    client_id: client_id.clone(),
                    update,
                };

                // Buffer for replay to new frontends. Keep only the
                // latest `PutImage` per window_id; everything else
                // accumulates.
                {
                    let mut bufs = state.display_buffers.write().await;
                    let buf = bufs.entry(client_id.clone()).or_default();
                    if let Some(wid) = put_image_wid {
                        buf.retain(|m| {
                            if let BackendToFrontend::DisplayUpdate { update: u, .. } = m {
                                if let x11_web_protocol::DisplayUpdate::PutImage {
                                    window_id,
                                    ..
                                } = u
                                {
                                    return window_id != &wid;
                                }
                            }
                            true
                        });
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
            SidecarToBackend::InputDropped { window_id, reason } => {
                broadcast_to_frontends(
                    &state,
                    BackendToFrontend::InputDropped {
                        sidecar_id: sidecar_id.clone(),
                        window_id,
                        reason,
                    },
                )
                .await;
            }
            SidecarToBackend::ClipboardData {
                selection,
                mime_type,
                data,
            } => {
                let frontends = state.frontends.read().await;
                for frontend in frontends.values() {
                    if frontend.subscribed_sidecars.contains(&sidecar_id) {
                        let _ = frontend.tx.send(BackendToFrontend::ClipboardData {
                            sidecar_id: sidecar_id.clone(),
                            selection: selection.clone(),
                            mime_type: mime_type.clone(),
                            data: data.clone(),
                        });
                    }
                }
            }
            SidecarToBackend::ClipboardOffer {
                selection,
                mime_types,
            } => {
                let frontends = state.frontends.read().await;
                for frontend in frontends.values() {
                    if frontend.subscribed_sidecars.contains(&sidecar_id) {
                        let _ = frontend.tx.send(BackendToFrontend::ClipboardOffer {
                            sidecar_id: sidecar_id.clone(),
                            selection: selection.clone(),
                            mime_types: mime_types.clone(),
                        });
                    }
                }
            }
        }
    }
}

/// Tear down sidecar registration when its QUIC connection drops.
async fn cleanup_sidecar(state: &AppState, sidecar_id: &str) {
    info!("Sidecar disconnected: {}", sidecar_id);
    state.sidecars.write().await.remove(sidecar_id);
    // Clean up processes, window states, and display buffers for
    // this sidecar.
    let client_ids: Vec<String> = state
        .processes
        .write()
        .await
        .remove(sidecar_id)
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.client_id)
        .collect();
    state
        .window_states
        .write()
        .await
        .retain(|_, ws| ws.sidecar_id != sidecar_id);
    {
        let mut bufs = state.display_buffers.write().await;
        for cid in &client_ids {
            bufs.remove(cid);
        }
    }

    let windows_changed = drop_sidecar_windows(state, sidecar_id).await;
    broadcast_sidecar_list(state).await;
    // The sidecar's per-sidecar process list went to zero; let
    // frontends know with one final empty broadcast.
    broadcast_process_list(state, sidecar_id).await;
    if windows_changed {
        broadcast_window_list(state).await;
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

    // Send current sidecar list to just this frontend.
    {
        let sidecars: Vec<SidecarInfo> = state
            .sidecars
            .read()
            .await
            .values()
            .map(|s| s.info.clone())
            .collect();
        let _ = tx.send(BackendToFrontend::SidecarList { sidecars });
    }

    // Send the per-sidecar X11-connected process list to this frontend.
    {
        let procs = state.processes.read().await;
        for (sidecar_id, processes) in procs.iter() {
            let _ = tx.send(BackendToFrontend::ProcessList {
                sidecar_id: sidecar_id.clone(),
                processes: processes.clone(),
            });
        }
    }

    // Send the current authoritative window list (visible windows
    // across all sidecars/clients, in stacking order).
    {
        let track = state.window_track.read().await;
        let order = state.window_order.read().await;
        let mut windows = Vec::new();
        for key @ (sid, cid, wid) in order.iter() {
            let Some(w) = track.get(key) else { continue };
            if !w.mapped {
                continue;
            }
            if !(w.is_top_level || w.override_redirect) {
                continue;
            }
            windows.push(WindowDescriptor {
                sidecar_id: sid.clone(),
                client_id: cid.clone(),
                window_id: wid.clone(),
                x: w.x,
                y: w.y,
                width: w.width,
                height: w.height,
                border_width: w.border_width,
                border_pixel: w.border_pixel,
                override_redirect: w.override_redirect,
            });
        }
        let _ = tx.send(BackendToFrontend::WindowList { windows });
    }

    // Send current window states
    {
        let states = state.window_states.read().await;
        let windows: Vec<WindowState> = states.values().cloned().collect();
        if !windows.is_empty() {
            let _ = tx.send(BackendToFrontend::WindowStateList { windows });
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
            FrontendToBackend::SubscribeDisplay { sidecar_id } => {
                let mut frontends = state.frontends.write().await;
                if let Some(frontend) = frontends.get_mut(&frontend_id) {
                    if !frontend.subscribed_sidecars.contains(&sidecar_id) {
                        frontend.subscribed_sidecars.push(sidecar_id.clone());

                        // Replay buffered display updates for all clients of this sidecar
                        let bufs = state.display_buffers.read().await;
                        let procs = state.processes.read().await;
                        if let Some(list) = procs.get(&sidecar_id) {
                            for p in list {
                                if let Some(buf) = bufs.get(&p.client_id) {
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
            FrontendToBackend::RequestClipboard {
                sidecar_id,
                selection,
                mime_type,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::RequestClipboard {
                        selection,
                        mime_type,
                    },
                )
                .await;
            }
            FrontendToBackend::SetClipboard {
                sidecar_id,
                selection,
                mime_type,
                data,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::SetClipboard {
                        selection,
                        mime_type,
                        data,
                    },
                )
                .await;
            }
            FrontendToBackend::ResizeScreen {
                sidecar_id,
                width,
                height,
            } => {
                forward_to_sidecar(
                    &state,
                    &sidecar_id,
                    BackendToSidecar::ResizeScreen { width, height },
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
                // Look up pid by client_id by scanning the per-sidecar list.
                let pid = state
                    .processes
                    .read()
                    .await
                    .get(&sidecar_id)
                    .and_then(|list| list.iter().find(|p| p.client_id == client_id))
                    .map(|p| p.pid)
                    .unwrap_or(0);

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

/// Snapshot the current sidecar list and broadcast it to every
/// connected frontend. Called on initial frontend connect, on a new
/// sidecar joining, and on a sidecar leaving.
pub async fn broadcast_sidecar_list(state: &AppState) {
    let sidecars: Vec<SidecarInfo> = state
        .sidecars
        .read()
        .await
        .values()
        .map(|s| s.info.clone())
        .collect();
    broadcast_to_frontends(state, BackendToFrontend::SidecarList { sidecars }).await;
}

/// Apply a `DisplayUpdate` to the backend's window-state mirror.
/// Returns `true` if the variant is a window-lifecycle event that the
/// backend has absorbed (and therefore should *not* forward to the
/// frontend); `false` otherwise (caller forwards as usual).
async fn apply_window_lifecycle(
    state: &AppState,
    sidecar_id: &str,
    client_id: &str,
    update: &DisplayUpdate,
) -> bool {
    use DisplayUpdate::*;
    let mut track = state.window_track.write().await;
    let mut order = state.window_order.write().await;
    let key = |wid: &str| (sidecar_id.to_string(), client_id.to_string(), wid.to_string());
    match update {
        WindowCreated {
            window_id,
            x,
            y,
            width,
            height,
            is_top_level,
            override_redirect,
            border_width,
            border_pixel,
        } => {
            let k = key(window_id);
            track.insert(
                k.clone(),
                TrackedWindow {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    border_width: *border_width,
                    border_pixel: *border_pixel,
                    is_top_level: *is_top_level,
                    override_redirect: *override_redirect,
                    mapped: false,
                },
            );
            if !order.contains(&k) {
                order.push(k);
            }
            true
        }
        WindowMapped {
            window_id,
            is_top_level,
            override_redirect,
        } => {
            let k = key(window_id);
            let entry = track.entry(k.clone()).or_insert(TrackedWindow {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                border_width: 0,
                border_pixel: 0,
                is_top_level: *is_top_level,
                override_redirect: *override_redirect,
                mapped: false,
            });
            entry.is_top_level = *is_top_level;
            entry.override_redirect = *override_redirect;
            entry.mapped = true;
            if !order.contains(&k) {
                order.push(k);
            }
            true
        }
        WindowUnmapped { window_id } => {
            if let Some(w) = track.get_mut(&key(window_id)) {
                w.mapped = false;
            }
            true
        }
        WindowDestroyed { window_id } => {
            let k = key(window_id);
            track.remove(&k);
            order.retain(|x| x != &k);
            true
        }
        WindowConfigured {
            window_id,
            x,
            y,
            width,
            height,
            border_width,
            border_pixel,
        } => {
            if let Some(w) = track.get_mut(&key(window_id)) {
                w.x = *x;
                w.y = *y;
                w.width = *width;
                w.height = *height;
                w.border_width = *border_width;
                w.border_pixel = *border_pixel;
            }
            true
        }
        WindowRaised { window_id } => {
            let k = key(window_id);
            order.retain(|x| x != &k);
            order.push(k);
            true
        }
        _ => false,
    }
}

/// Build the current authoritative window list (visible windows only,
/// in stacking order — last on top) and broadcast it to all frontends.
pub async fn broadcast_window_list(state: &AppState) {
    let track = state.window_track.read().await;
    let order = state.window_order.read().await;
    let mut windows = Vec::with_capacity(order.len());
    for key @ (sidecar_id, client_id, window_id) in order.iter() {
        let Some(w) = track.get(key) else { continue };
        if !w.mapped {
            continue;
        }
        if !(w.is_top_level || w.override_redirect) {
            continue;
        }
        windows.push(WindowDescriptor {
            sidecar_id: sidecar_id.clone(),
            client_id: client_id.clone(),
            window_id: window_id.clone(),
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            border_width: w.border_width,
            border_pixel: w.border_pixel,
            override_redirect: w.override_redirect,
        });
    }
    drop(track);
    drop(order);
    broadcast_to_frontends(state, BackendToFrontend::WindowList { windows }).await;
}

/// Drop every window owned by a given client, used when a process
/// exits. Returns `true` if anything changed (so the caller can
/// trigger a `WindowList` broadcast).
async fn drop_client_windows(state: &AppState, client_id: &str) -> bool {
    let mut track = state.window_track.write().await;
    let mut order = state.window_order.write().await;
    let before = track.len();
    track.retain(|(_, c, _), _| c != client_id);
    order.retain(|(_, c, _)| c != client_id);
    track.len() != before
}

/// Drop every window owned by a given sidecar, used when a sidecar
/// disconnects.
async fn drop_sidecar_windows(state: &AppState, sidecar_id: &str) -> bool {
    let mut track = state.window_track.write().await;
    let mut order = state.window_order.write().await;
    let before = track.len();
    track.retain(|(s, _, _), _| s != sidecar_id);
    order.retain(|(s, _, _)| s != sidecar_id);
    track.len() != before
}

/// Snapshot one sidecar's X11-connected process list and broadcast it
/// to every frontend. Called on `ProcessConnected` / `ProcessExited`
/// events from the sidecar and on sidecar disconnect.
pub async fn broadcast_process_list(state: &AppState, sidecar_id: &str) {
    let processes = state
        .processes
        .read()
        .await
        .get(sidecar_id)
        .cloned()
        .unwrap_or_default();
    broadcast_to_frontends(
        state,
        BackendToFrontend::ProcessList {
            sidecar_id: sidecar_id.to_string(),
            processes,
        },
    )
    .await;
}
