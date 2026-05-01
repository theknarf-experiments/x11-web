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
    /// Per-window tracked positions, populated by
    /// `UpdateWindowPosition` from frontends. Keyed by `window_id`.
    /// Folded into `WindowDescriptor.{x, y, placed}` on every
    /// `WindowList` broadcast so newly-connected frontends pick up
    /// positions other tabs already chose.
    window_positions: Arc<RwLock<HashMap<String, TrackedPosition>>>,
    /// Display update buffer per client_id for replay on frontend connect
    display_buffers: Arc<RwLock<HashMap<String, Vec<BackendToFrontend>>>>,
}

struct SidecarConnection {
    info: SidecarInfo,
    tx: mpsc::UnboundedSender<BackendToSidecar>,
}

/// Per-window tracked position. Just `(x, y)` — owner identity (pid /
/// sidecar / client) lives in `window_track` keyed by the same
/// `window_id`, so cleanup paths use that for filtering rather than
/// duplicating the metadata here.
#[derive(Clone, Copy)]
struct TrackedPosition {
    x: f64,
    y: f64,
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
        window_positions: Arc::new(RwLock::new(HashMap::new())),
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
                // Position state is keyed by window_id; orphan
                // entries from destroyed windows are harmless and
                // get cleaned up via `drop_client_windows` below.
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
                // resulting `WindowList`.
                if apply_window_lifecycle(state, &sidecar_id, &client_id, &update).await {
                    broadcast_window_list(state).await;
                    return;
                }

                // Bell isn't per-window — fan out as a top-level
                // message rather than wrapping in a WindowUpdate.
                if let x11_web_protocol::DisplayUpdate::Bell { percent } = update {
                    broadcast_to_frontends(state, BackendToFrontend::Bell { percent }).await;
                    return;
                }

                // Translate the remaining content/property variants
                // into the frontend-facing `WindowUpdate` shape.
                let window_update = match update_to_window_update(update) {
                    Some(u) => u,
                    None => return,
                };
                let put_image_wid = match &window_update {
                    WindowUpdate::PutImage { window_id, .. } => Some(window_id.clone()),
                    _ => None,
                };
                let msg = BackendToFrontend::WindowUpdate {
                    update: window_update,
                };

                // Buffer for replay to new frontends. Keep only the
                // latest `PutImage` per window_id; everything else
                // accumulates.
                {
                    let mut bufs = state.display_buffers.write().await;
                    let buf = bufs.entry(client_id.clone()).or_default();
                    if let Some(wid) = put_image_wid {
                        buf.retain(|m| {
                            if let BackendToFrontend::WindowUpdate { update: u } = m {
                                if let WindowUpdate::PutImage { window_id, .. } = u {
                                    return window_id != &wid;
                                }
                            }
                            true
                        });
                    }
                    buf.push(msg.clone());
                }

                broadcast_to_frontends(state, msg).await;
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
                broadcast_to_frontends(
                    state,
                    BackendToFrontend::ClipboardData {
                        sidecar_id: sidecar_id.clone(),
                        selection,
                        mime_type,
                        data,
                    },
                )
                .await;
            }
            SidecarToBackend::ClipboardOffer {
                selection,
                mime_types,
            } => {
                broadcast_to_frontends(
                    state,
                    BackendToFrontend::ClipboardOffer {
                        sidecar_id: sidecar_id.clone(),
                        selection,
                        mime_types,
                    },
                )
                .await;
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
    // Orphan tracked positions for the disconnected sidecar's windows
    // get cleaned up by `drop_sidecar_windows` below.
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
            FrontendConnection { tx: tx.clone() },
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
        let windows = build_window_list(&state).await;
        let _ = tx.send(BackendToFrontend::WindowList { windows });
    }

    // Replay buffered display updates for every X11 client so the
    // frontend's canvases aren't blank — the buffer keeps the latest
    // PutImage per window.
    {
        let bufs = state.display_buffers.read().await;
        for buf in bufs.values() {
            for msg in buf {
                let _ = tx.send(msg.clone());
            }
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
            FrontendToBackend::UpdateWindowPosition { window_id, x, y } => {
                state
                    .window_positions
                    .write()
                    .await
                    .insert(window_id.clone(), TrackedPosition { x, y });

                // Broadcast a tight delta to every *other* frontend
                // (the originator already has the latest position
                // locally). The next `WindowList` snapshot will also
                // reflect this tracked position.
                let frontends = state.frontends.read().await;
                for (fid, frontend) in frontends.iter() {
                    if fid != &frontend_id {
                        let _ = frontend.tx.send(BackendToFrontend::WindowUpdate {
                            update: WindowUpdate::PositionChanged {
                                window_id: window_id.clone(),
                                x,
                                y,
                            },
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

/// Translate a sidecar-emitted [`DisplayUpdate`] into the
/// frontend-facing [`WindowUpdate`]. Returns `None` for variants the
/// frontend doesn't see (lifecycle events absorbed by the tracker,
/// `Bell` lifted to the top level, `WindowRaised` since stacking is
/// expressed by `WindowList` order).
fn update_to_window_update(update: DisplayUpdate) -> Option<WindowUpdate> {
    use DisplayUpdate as D;
    Some(match update {
        D::PutImage {
            window_id,
            x,
            y,
            width,
            height,
            data,
        } => WindowUpdate::PutImage {
            window_id,
            x,
            y,
            width,
            height,
            data,
        },
        D::TitleChanged { window_id, title } => {
            WindowUpdate::TitleChanged { window_id, title }
        }
        D::WindowStateChanged { window_id, state } => {
            WindowUpdate::StateChanged { window_id, state }
        }
        D::WindowFocused { window_id } => WindowUpdate::Focused { window_id },
        D::MenuStructure { window_id, menu } => {
            WindowUpdate::MenuStructure { window_id, menu }
        }
        D::CursorChanged { window_id, cursor } => {
            WindowUpdate::CursorChanged { window_id, cursor }
        }
        D::CursorBitmap {
            window_id,
            width,
            height,
            hotspot_x,
            hotspot_y,
            data,
        } => WindowUpdate::CursorBitmap {
            window_id,
            width,
            height,
            hotspot_x,
            hotspot_y,
            data,
        },
        D::CursorAnimated { window_id, frames } => {
            WindowUpdate::CursorAnimated { window_id, frames }
        }
        // Lifecycle / Bell / Raised are handled elsewhere.
        _ => return None,
    })
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
/// Build the current authoritative window list, with each
/// descriptor's `(x, y, placed)` chosen as follows:
///   - override-redirect popups: X11 position, `placed = true`
///   - top-level with a tracked cross-frontend position: tracked
///     position, `placed = true`
///   - top-level without one: X11 default position, `placed = false`
///     (frontend may apply its own layout heuristic and broadcast
///     the result via `UpdateWindowPosition`).
pub async fn build_window_list(state: &AppState) -> Vec<WindowDescriptor> {
    let track = state.window_track.read().await;
    let order = state.window_order.read().await;
    let positions = state.window_positions.read().await;
    let procs = state.processes.read().await;
    let mut windows = Vec::with_capacity(order.len());
    for key @ (sidecar_id, client_id, window_id) in order.iter() {
        let Some(w) = track.get(key) else { continue };
        if !w.mapped {
            continue;
        }
        if !(w.is_top_level || w.override_redirect) {
            continue;
        }
        let (x, y, placed) = if w.override_redirect {
            (w.x as f64, w.y as f64, true)
        } else if let Some(p) = positions.get(window_id) {
            (p.x, p.y, true)
        } else {
            (w.x as f64, w.y as f64, false)
        };
        // Pull pid + command from the cached process list so the
        // frontend has everything needed for dock labels and the
        // kill button without a separate ProcessList lookup.
        let (pid, command) = procs
            .get(sidecar_id)
            .and_then(|list| list.iter().find(|p| &p.client_id == client_id))
            .map(|p| (p.pid, p.command.clone()))
            .unwrap_or((0, String::new()));
        windows.push(WindowDescriptor {
            window_id: window_id.clone(),
            sidecar_id: sidecar_id.clone(),
            pid,
            command,
            x,
            y,
            width: w.width,
            height: w.height,
            border_width: w.border_width,
            border_pixel: w.border_pixel,
            override_redirect: w.override_redirect,
            placed,
        });
    }
    windows
}

pub async fn broadcast_window_list(state: &AppState) {
    let windows = build_window_list(state).await;
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
