mod chunking;
mod quic;
mod rtc;
mod rtc_codec;

use std::collections::{HashMap, HashSet};
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
use x11_web_wire::{BackendToSidecar, SidecarKind, SidecarToBackend};

#[derive(Clone)]
struct AppState {
    sidecars: Arc<RwLock<HashMap<String, SidecarConnection>>>,
    frontends: Arc<RwLock<HashMap<String, FrontendConnection>>>,
    /// Authoritative workspace list. Auto-populated with one
    /// "Workspace 1" entry the first time any frontend connects (so
    /// the list is non-empty for the lifetime of the backend after
    /// that). Future revisions will hold per-workspace attached-
    /// window sets; today it's identity only.
    workspaces: Arc<RwLock<HashMap<String, Workspace>>>,
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
    /// Display update buffer per client_id for replay on frontend connect.
    /// PutImage is no longer in here — see `pixel_buffers`.
    display_buffers: Arc<RwLock<HashMap<String, Vec<BackendToFrontend>>>>,
    /// Latest Cap'n Proto-encoded PutImage frame per (client_id,
    /// window_id), replayed over the DataChannel once a frontend's
    /// DC opens. Replaces the old WS-shaped PutImage replay; pixels
    /// don't ride the WS anymore.
    pixel_buffers: Arc<RwLock<HashMap<String, HashMap<String, Vec<u8>>>>>,
    /// Latest Cap'n Proto-encoded WindowThumbnail frame per
    /// (client_id, window_id). Sidecars (currently macOS only) emit
    /// these at low rate so the frontend can render previews in the
    /// spawn-popover picker. Same DC fan-out + replay-on-open story
    /// as `pixel_buffers`; kept parallel rather than merged so
    /// thumbnails and live frames don't overwrite each other.
    thumbnail_buffers: Arc<RwLock<HashMap<String, HashMap<String, Vec<u8>>>>>,
    /// Per-workspace set of windows that are attached to its canvas.
    /// X11-sidecar windows auto-attach on creation; macOS-sidecar
    /// windows attach only when the user drags a polaroid out of the
    /// picker. Drives the frontend's canvas render filter.
    attached_windows: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Reference count of how many workspaces have a given window
    /// attached. When the count goes 0→1 the backend asks the
    /// owning sidecar to start live capture; on 1→0 it asks to stop.
    /// Decoupled from `attached_windows` so multi-workspace attaches
    /// of the same window don't double-start the SCStream.
    streaming_refcount: Arc<RwLock<HashMap<String, usize>>>,
    /// Reverse index window_id → sidecar_id, populated on
    /// `WindowCreated`. Used to route `Start/StopWindowCapture` to
    /// the right sidecar without scanning `window_track` twice.
    window_owner: Arc<RwLock<HashMap<String, String>>>,
    /// `request_id → workspace_id` for in-flight `SpawnProcess`
    /// commands. Stored when the frontend's spawn request arrives
    /// and consumed when the sidecar replies with `ProcessSpawned`,
    /// at which point we promote the entry into `spawn_origin`.
    pending_spawns: Arc<RwLock<HashMap<String, String>>>,
    /// `(sidecar_id, pid) → workspace_id`. The workspace that
    /// asked the sidecar to spawn this pid. When an X11 client of
    /// that pid emits `WindowCreated`, the backend uses this to
    /// auto-attach the window only to that workspace — different
    /// workspaces can run different apps without cross-pollination.
    spawn_origin: Arc<RwLock<HashMap<(String, u32), String>>>,
}

struct SidecarConnection {
    info: SidecarInfo,
    kind: SidecarKind,
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
    /// Whether the user can drag-resize this window. Sidecar reports
    /// `true` for X11 windows unconditionally; macOS sidecars probe AX
    /// and report whatever `AXSize`-settable says.
    resizable: bool,
}

struct FrontendConnection {
    tx: mpsc::UnboundedSender<BackendToFrontend>,
    rtc: rtc::RtcConn,
    /// Workspace this frontend is bound to. `None` until the
    /// frontend sends its first `OpenWorkspace`; future per-workspace
    /// scoping (attached-window sets, etc.) reads this to decide
    /// what to send.
    workspace_id: Option<String>,
}

fn main() {
    // Default tokio worker stack (2 MiB) overflows under sustained
    // str0m SCTP writes — the SCTP/DTLS path is deep enough that a
    // burst of large-payload writes tips it over. 8 MiB gives ample
    // headroom and matches what the chat example in str0m runs with.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
        .expect("build tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    tracing_subscriber::fmt::init();

    let state = AppState {
        sidecars: Arc::new(RwLock::new(HashMap::new())),
        frontends: Arc::new(RwLock::new(HashMap::new())),
        workspaces: Arc::new(RwLock::new(HashMap::new())),
        processes: Arc::new(RwLock::new(HashMap::new())),
        window_track: Arc::new(RwLock::new(HashMap::new())),
        window_order: Arc::new(RwLock::new(Vec::new())),
        window_positions: Arc::new(RwLock::new(HashMap::new())),
        display_buffers: Arc::new(RwLock::new(HashMap::new())),
        pixel_buffers: Arc::new(RwLock::new(HashMap::new())),
        thumbnail_buffers: Arc::new(RwLock::new(HashMap::new())),
        attached_windows: Arc::new(RwLock::new(HashMap::new())),
        streaming_refcount: Arc::new(RwLock::new(HashMap::new())),
        window_owner: Arc::new(RwLock::new(HashMap::new())),
        pending_spawns: Arc::new(RwLock::new(HashMap::new())),
        spawn_origin: Arc::new(RwLock::new(HashMap::new())),
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
                // Pop the pending spawn entry the frontend created
                // when it asked for this command, and promote it
                // into the (sidecar_id, pid) → workspace_id index
                // so `WindowCreated` later knows where to attach.
                let workspace_id = state.pending_spawns.write().await.remove(&request_id);
                if let Some(workspace_id) = workspace_id {
                    state
                        .spawn_origin
                        .write()
                        .await
                        .insert((sidecar_id.clone(), pid), workspace_id);
                }
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
            SidecarToBackend::ProcessExited { pid, exit_code: _ } => {
                // Drop matching entries from per-sidecar list and the
                // window-state index; clear any buffered display
                // updates keyed by the freed client_ids.
                state
                    .spawn_origin
                    .write()
                    .await
                    .remove(&(sidecar_id.clone(), pid));
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
                    {
                        let mut bufs = state.display_buffers.write().await;
                        let mut pixels = state.pixel_buffers.write().await;
                        let mut thumbs = state.thumbnail_buffers.write().await;
                        for cid in &freed_client_ids {
                            bufs.remove(cid);
                            pixels.remove(cid);
                            thumbs.remove(cid);
                        }
                    }
                    for cid in &freed_client_ids {
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
                    // Track ownership + auto-attach policy for the
                    // newly-created / destroyed window. Only fires
                    // for the relevant lifecycle variants; cheap
                    // no-op for others.
                    on_window_lifecycle_after(state, &sidecar_id, &client_id, &update).await;
                    broadcast_window_list(state).await;
                    return;
                }

                // Bell isn't per-window — fan out as a top-level
                // message rather than wrapping in a WindowUpdate.
                if let x11_web_protocol::DisplayUpdate::Bell { percent } = update {
                    broadcast_to_frontends(state, BackendToFrontend::Bell { percent }).await;
                    return;
                }

                // PutImage rides the WebRTC DataChannel as Cap'n
                // Proto; encode once and fan out to every frontend
                // whose DC is open. Buffer the latest per
                // (client_id, window_id) so freshly-connected
                // frontends get the current pixels once their DC
                // opens (see the replay task in `handle_socket`).
                if let DisplayUpdate::PutImage {
                    window_id,
                    x,
                    y,
                    width,
                    height,
                    data,
                } = update
                {
                    let bytes = rtc_codec::encode_put_image(&window_id, x, y, width, height, &data);
                    {
                        let mut bufs = state.pixel_buffers.write().await;
                        bufs.entry(client_id.clone())
                            .or_default()
                            .insert(window_id.clone(), bytes.clone());
                    }
                    let frontends = state.frontends.read().await;
                    for frontend in frontends.values() {
                        if frontend
                            .rtc
                            .dc_open
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            let _ = frontend.rtc.dc_tx.send(bytes.clone());
                        }
                    }
                    return;
                }

                // WindowThumbnail rides the same DataChannel path as
                // PutImage but lands in a parallel cache (see
                // `thumbnail_buffers`) so live frames and thumbnails
                // don't overwrite each other.
                if let DisplayUpdate::WindowThumbnail {
                    window_id,
                    width,
                    height,
                    data,
                } = update
                {
                    let bytes =
                        rtc_codec::encode_window_thumbnail(&window_id, width, height, &data);
                    {
                        let mut bufs = state.thumbnail_buffers.write().await;
                        bufs.entry(client_id.clone())
                            .or_default()
                            .insert(window_id.clone(), bytes.clone());
                    }
                    let frontends = state.frontends.read().await;
                    for frontend in frontends.values() {
                        if frontend
                            .rtc
                            .dc_open
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            let _ = frontend.rtc.dc_tx.send(bytes.clone());
                        }
                    }
                    return;
                }

                // Translate the remaining content/property variants
                // into the frontend-facing `WindowUpdate` shape.
                let window_update = match update_to_window_update(update) {
                    Some(u) => u,
                    None => return,
                };
                let msg = BackendToFrontend::WindowUpdate {
                    update: window_update,
                };

                // Buffer everything else for replay on WS connect.
                {
                    let mut bufs = state.display_buffers.write().await;
                    bufs.entry(client_id.clone()).or_default().push(msg.clone());
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
        let mut pixels = state.pixel_buffers.write().await;
        let mut thumbs = state.thumbnail_buffers.write().await;
        for cid in &client_ids {
            bufs.remove(cid);
            pixels.remove(cid);
            thumbs.remove(cid);
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

    // Spawn the per-frontend WebRTC driver. The DC isn't usable
    // until the browser sends an offer over the WS, but the task is
    // ready to receive signalling immediately.
    //
    // `control_inbound_tx` carries raw bytes received on the control
    // DC. The handler task below decodes the capnp Frame and
    // dispatches by variant — currently just logging for the slice
    // 1a hello round-trip, future Automerge sync replaces this.
    let (control_inbound_tx, mut control_inbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let rtc = rtc::spawn(frontend_id.clone(), tx.clone(), control_inbound_tx);
    {
        let frontend_id = frontend_id.clone();
        tokio::spawn(async move {
            while let Some(bytes) = control_inbound_rx.recv().await {
                match rtc_codec::decode_workspace_sync(&bytes) {
                    Some((workspace_id, message)) => {
                        info!(
                            "control inbound from {frontend_id}: workspace_id={workspace_id} \
                             {} bytes (preview: {:?})",
                            message.len(),
                            String::from_utf8_lossy(
                                &message[..message.len().min(64)],
                            ),
                        );
                    }
                    None => {
                        warn!(
                            "control inbound from {frontend_id}: failed to decode \
                             workspaceSync ({} bytes)",
                            bytes.len()
                        );
                    }
                }
            }
        });
    }

    // Slice 1a hello: when the control DC opens, send a single
    // workspaceSync frame so we can verify the round-trip end to
    // end. Replaced in 1b by the real Automerge sync handshake.
    {
        let control_opened = rtc.control_opened.clone();
        let control_tx = rtc.control_tx.clone();
        tokio::spawn(async move {
            control_opened.notified().await;
            let bytes = rtc_codec::encode_workspace_sync(
                "hello-test",
                b"hello-from-backend",
            );
            let _ = control_tx.send(bytes);
        });
    }

    // Once the DC opens for the first time, replay every buffered
    // PutImage *and* every buffered thumbnail so the frontend's
    // canvases + popover thumbnails populate immediately instead of
    // waiting for the next paint / refresh cycle.
    {
        let dc_opened = rtc.dc_opened.clone();
        let dc_tx = rtc.dc_tx.clone();
        let pixel_buffers = state.pixel_buffers.clone();
        let thumbnail_buffers = state.thumbnail_buffers.clone();
        tokio::spawn(async move {
            dc_opened.notified().await;
            {
                let bufs = pixel_buffers.read().await;
                for windows in bufs.values() {
                    for bytes in windows.values() {
                        let _ = dc_tx.send(bytes.clone());
                    }
                }
            }
            {
                let bufs = thumbnail_buffers.read().await;
                for windows in bufs.values() {
                    for bytes in windows.values() {
                        let _ = dc_tx.send(bytes.clone());
                    }
                }
            }
        });
    }

    // Register frontend. The workspace is bound later, in response
    // to the frontend's first `OpenWorkspace` request.
    {
        let mut frontends = state.frontends.write().await;
        frontends.insert(
            frontend_id.clone(),
            FrontendConnection {
                tx: tx.clone(),
                rtc,
                workspace_id: None,
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
            FrontendToBackend::OpenWorkspace { id } => {
                let workspace = open_or_create_workspace(&state, id).await;
                {
                    let mut frontends = state.frontends.write().await;
                    if let Some(conn) = frontends.get_mut(&frontend_id) {
                        conn.workspace_id = Some(workspace.id.clone());
                    }
                }
                let workspace_id = workspace.id.clone();
                let _ = tx.send(BackendToFrontend::Workspace { workspace });
                // Send the current attached-window set for the
                // workspace this frontend just bound to. Any X11
                // windows that auto-attached before connect appear
                // here.
                let window_ids: Vec<String> = state
                    .attached_windows
                    .read()
                    .await
                    .get(&workspace_id)
                    .map(|set| set.iter().cloned().collect())
                    .unwrap_or_default();
                let _ = tx.send(BackendToFrontend::AttachedWindows {
                    workspace_id,
                    window_ids,
                });
            }
            FrontendToBackend::AttachWindow {
                workspace_id,
                window_id,
            } => {
                attach_window(&state, &workspace_id, &window_id).await;
            }
            FrontendToBackend::DetachWindow {
                workspace_id,
                window_id,
            } => {
                detach_window(&state, &workspace_id, &window_id).await;
            }
            FrontendToBackend::SpawnProcess {
                request_id,
                sidecar_id,
                workspace_id,
                command,
                args,
            } => {
                // Remember which workspace asked for this spawn so
                // the resulting X11 window only auto-attaches there.
                state
                    .pending_spawns
                    .write()
                    .await
                    .insert(request_id.clone(), workspace_id);
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
            FrontendToBackend::RtcOffer { sdp } => {
                if let Some(frontend) = state.frontends.read().await.get(&frontend_id) {
                    let _ = frontend.rtc.signal_tx.send(rtc::RtcSignal::Offer(sdp));
                }
            }
            FrontendToBackend::RtcIceCandidate { candidate, .. } => {
                if let Some(frontend) = state.frontends.read().await.get(&frontend_id) {
                    let _ = frontend
                        .rtc
                        .signal_tx
                        .send(rtc::RtcSignal::IceCandidate(candidate));
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
async fn broadcast_sidecar_list(state: &AppState) {
    let sidecars: Vec<SidecarInfo> = state
        .sidecars
        .read()
        .await
        .values()
        .map(|s| s.info.clone())
        .collect();
    broadcast_to_frontends(state, BackendToFrontend::SidecarList { sidecars }).await;
}

/// Resolve `requested_id` to an existing workspace, or create a
/// fresh one if `None` was supplied or the requested id isn't known
/// (e.g. stale URL hash after a backend restart). Either way the
/// caller gets back a workspace to send to the frontend.
/// Side effects that run after `apply_window_lifecycle` has updated
/// the tracker: populate `window_owner`, auto-attach X11 windows to
/// the workspace that asked for the spawn (looked up via
/// `spawn_origin`), drop ownership / attached / refcount entries on
/// destroy. macOS windows always wait for a frontend `AttachWindow`;
/// X11 windows whose origin can't be traced (no spawn through the
/// dock — e.g. orphan clients) likewise stay detached.
async fn on_window_lifecycle_after(
    state: &AppState,
    sidecar_id: &str,
    client_id: &str,
    update: &DisplayUpdate,
) {
    match update {
        DisplayUpdate::WindowCreated { window_id, .. } => {
            let kind = state
                .sidecars
                .read()
                .await
                .get(sidecar_id)
                .map(|s| s.kind)
                .unwrap_or(SidecarKind::Unknown);
            {
                let mut owners = state.window_owner.write().await;
                owners.insert(window_id.clone(), sidecar_id.to_string());
            }
            if matches!(kind, SidecarKind::X11 | SidecarKind::Unknown) {
                // Resolve client_id → pid via the per-sidecar
                // processes table that ProcessConnected builds, then
                // pid → workspace via spawn_origin. The pair lets us
                // attach this window only to the workspace that
                // asked for it.
                let pid = state
                    .processes
                    .read()
                    .await
                    .get(sidecar_id)
                    .and_then(|list| {
                        list.iter()
                            .find(|p| p.client_id == client_id)
                            .map(|p| p.pid)
                    });
                let workspace_id = match pid {
                    Some(pid) => state
                        .spawn_origin
                        .read()
                        .await
                        .get(&(sidecar_id.to_string(), pid))
                        .cloned(),
                    None => None,
                };
                if let Some(workspace_id) = workspace_id {
                    attach_window(state, &workspace_id, window_id).await;
                }
            }
        }
        DisplayUpdate::WindowDestroyed { window_id } => {
            // Remove from every workspace's attached set; if any
            // were holding it, broadcast their updated set.
            let affected_workspaces: Vec<String> = {
                let mut attached = state.attached_windows.write().await;
                attached
                    .iter_mut()
                    .filter_map(|(ws_id, set)| set.remove(window_id).then(|| ws_id.clone()))
                    .collect()
            };
            for ws_id in affected_workspaces {
                broadcast_attached_windows(state, &ws_id).await;
            }
            state.streaming_refcount.write().await.remove(window_id);
            state.window_owner.write().await.remove(window_id);
        }
        _ => {}
    }
}

/// Add `window_id` to `workspace_id`'s attached set. Refcount-inc
/// the streaming map; on 0→1, send `StartWindowCapture` to the
/// owning sidecar (no-op for X11 sidecars, which stream
/// unconditionally and ignore the message). Broadcasts the updated
/// attached set to all frontends.
async fn attach_window(state: &AppState, workspace_id: &str, window_id: &str) {
    let already = {
        let mut attached = state.attached_windows.write().await;
        !attached
            .entry(workspace_id.to_string())
            .or_default()
            .insert(window_id.to_string())
    };
    if already {
        return;
    }
    let started = {
        let mut rc = state.streaming_refcount.write().await;
        let entry = rc.entry(window_id.to_string()).or_insert(0);
        *entry += 1;
        *entry == 1
    };
    if started {
        if let Some(owner_sidecar) = state.window_owner.read().await.get(window_id).cloned() {
            send_to_sidecar(
                state,
                &owner_sidecar,
                BackendToSidecar::StartWindowCapture {
                    window_id: window_id.to_string(),
                },
            )
            .await;
        }
    }
    broadcast_attached_windows(state, workspace_id).await;
}

/// Remove `window_id` from `workspace_id`'s attached set. Refcount-
/// dec the streaming map; on 1→0, send `StopWindowCapture` to the
/// owning sidecar.
async fn detach_window(state: &AppState, workspace_id: &str, window_id: &str) {
    let was_present = {
        let mut attached = state.attached_windows.write().await;
        attached
            .get_mut(workspace_id)
            .map(|set| set.remove(window_id))
            .unwrap_or(false)
    };
    if !was_present {
        return;
    }
    let stopped = {
        let mut rc = state.streaming_refcount.write().await;
        match rc.get_mut(window_id) {
            Some(n) if *n > 0 => {
                *n -= 1;
                if *n == 0 {
                    rc.remove(window_id);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    if stopped {
        if let Some(owner_sidecar) = state.window_owner.read().await.get(window_id).cloned() {
            send_to_sidecar(
                state,
                &owner_sidecar,
                BackendToSidecar::StopWindowCapture {
                    window_id: window_id.to_string(),
                },
            )
            .await;
        }
    }
    broadcast_attached_windows(state, workspace_id).await;
}

async fn send_to_sidecar(state: &AppState, sidecar_id: &str, msg: BackendToSidecar) {
    if let Some(sc) = state.sidecars.read().await.get(sidecar_id) {
        let _ = sc.tx.send(msg);
    }
}

async fn broadcast_attached_windows(state: &AppState, workspace_id: &str) {
    let window_ids: Vec<String> = state
        .attached_windows
        .read()
        .await
        .get(workspace_id)
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default();
    broadcast_to_frontends(
        state,
        BackendToFrontend::AttachedWindows {
            workspace_id: workspace_id.to_string(),
            window_ids,
        },
    )
    .await;
}

async fn open_or_create_workspace(state: &AppState, requested_id: Option<String>) -> Workspace {
    let mut workspaces = state.workspaces.write().await;
    if let Some(id) = requested_id {
        if let Some(existing) = workspaces.get(&id) {
            return existing.clone();
        }
    }
    let id = Uuid::new_v4().to_string();
    let next_index = workspaces.len() + 1;
    let workspace = Workspace {
        id: id.clone(),
        name: format!("Workspace {next_index}"),
    };
    workspaces.insert(id, workspace.clone());
    workspace
}

/// Translate a sidecar-emitted [`DisplayUpdate`] into the
/// frontend-facing [`WindowUpdate`]. Returns `None` for variants the
/// frontend doesn't see (lifecycle events absorbed by the tracker,
/// `Bell` lifted to the top level, `WindowRaised` since stacking is
/// expressed by `WindowList` order).
fn update_to_window_update(update: DisplayUpdate) -> Option<WindowUpdate> {
    use DisplayUpdate as D;
    Some(match update {
        // PutImage and WindowThumbnail both flow over the WebRTC
        // DataChannel, not the WS — handled directly in
        // `dispatch_sidecar_msg`.
        D::PutImage { .. } | D::WindowThumbnail { .. } => return None,
        D::TitleChanged { window_id, title } => WindowUpdate::TitleChanged { window_id, title },
        D::WindowStateChanged { window_id, state } => {
            WindowUpdate::StateChanged { window_id, state }
        }
        D::WindowFocused { window_id } => WindowUpdate::Focused { window_id },
        D::MenuStructure { window_id, menu } => WindowUpdate::MenuStructure { window_id, menu },
        D::CursorChanged { window_id, cursor } => WindowUpdate::CursorChanged { window_id, cursor },
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
    let key = |wid: &str| {
        (
            sidecar_id.to_string(),
            client_id.to_string(),
            wid.to_string(),
        )
    };
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
            resizable,
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
                    resizable: *resizable,
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
                resizable: true,
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
            resizable,
        } => {
            if let Some(w) = track.get_mut(&key(window_id)) {
                w.x = *x;
                w.y = *y;
                w.width = *width;
                w.height = *height;
                w.border_width = *border_width;
                w.border_pixel = *border_pixel;
                w.resizable = *resizable;
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
async fn build_window_list(state: &AppState) -> Vec<WindowDescriptor> {
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
            resizable: w.resizable,
        });
    }
    windows
}

async fn broadcast_window_list(state: &AppState) {
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
async fn broadcast_process_list(state: &AppState, sidecar_id: &str) {
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
