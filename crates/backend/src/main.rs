mod auth_routes;
mod chunking;
mod quic;
mod rtc;
mod rtc_codec;
mod telemetry;
mod telemetry_proxy;
mod workspace_doc;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::{get, post};
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
    /// Reference count of how many workspaces have a given window
    /// attached (i.e. carry an `OcifNode` with the `@x11web/window`
    /// extension for it). Rebuilt by
    /// `reconcile_streaming_after_change` after every doc mutation.
    /// When a window's count goes 0→N the backend tells the owning
    /// sidecar to start live capture, N→0 stops it.
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
    /// Per-workspace Automerge document. Authoritative for the
    /// canvas — name, OCIF nodes (boxes / text / arrows / pen
    /// strokes / windows-as-nodes), and OCIF resources. Synced
    /// over each frontend's control DataChannel via
    /// `automerge::sync` — see `workspace_doc` module.
    workspace_docs: Arc<RwLock<HashMap<String, workspace_doc::WorkspaceEntry>>>,
}

struct SidecarConnection {
    info: SidecarInfo,
    kind: SidecarKind,
    /// Outbound QUIC channel. The String is the W3C `traceparent`
    /// captured at the call site (where the WS handler's span is
    /// still active) — the QUIC writer task that drains this
    /// channel has no active span of its own, so we'd otherwise
    /// inject an empty traceparent and break cross-process
    /// propagation.
    tx: mpsc::UnboundedSender<(BackendToSidecar, String)>,
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
    let telemetry = telemetry::init();

    let state = AppState {
        sidecars: Arc::new(RwLock::new(HashMap::new())),
        frontends: Arc::new(RwLock::new(HashMap::new())),
        workspaces: Arc::new(RwLock::new(HashMap::new())),
        processes: Arc::new(RwLock::new(HashMap::new())),
        window_track: Arc::new(RwLock::new(HashMap::new())),
        window_order: Arc::new(RwLock::new(Vec::new())),
        display_buffers: Arc::new(RwLock::new(HashMap::new())),
        pixel_buffers: Arc::new(RwLock::new(HashMap::new())),
        thumbnail_buffers: Arc::new(RwLock::new(HashMap::new())),
        streaming_refcount: Arc::new(RwLock::new(HashMap::new())),
        window_owner: Arc::new(RwLock::new(HashMap::new())),
        pending_spawns: Arc::new(RwLock::new(HashMap::new())),
        spawn_origin: Arc::new(RwLock::new(HashMap::new())),
        workspace_docs: Arc::new(RwLock::new(HashMap::new())),
    };

    // OIDC + session middleware. `OidcConfig::from_env()` returns
    // `None` when `OIDC_ISSUER` is unset → anonymous-only mode;
    // `/auth/login` then 503s but `/auth/me` still works (returns
    // `null`). The mode is logged on startup so prod can't silently
    // regress to anonymous.
    let authenticator = match x11_web_auth::OidcConfig::from_env() {
        Some(cfg) => {
            info!("OIDC enabled (issuer={})", cfg.issuer);
            Some(x11_web_auth::Authenticator::new(cfg).expect("OIDC authenticator init"))
        }
        None => {
            info!("OIDC disabled — anonymous-only mode (set OIDC_ISSUER to enable)");
            None
        }
    };
    let auth_state = auth_routes::AuthState::new(authenticator);

    // Cookie-based sessions, in-memory store. `SameSite=Lax`
    // means the cookie rides along on cross-port localhost
    // requests in dev (same eTLD+1) without the `Secure` flag
    // that would require HTTPS. Production deployments behind
    // TLS should flip `with_secure(true)`.
    let session_store = tower_sessions_memory_store::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_name("x11web.sid")
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_http_only(true)
        .with_secure(false);

    let mut app = Router::new()
        .route("/ws/frontend", get(frontend_ws_handler))
        .route("/health", get(|| async { "ok" }))
        // Same-origin OTLP/HTTP proxy for the browser. The SDK
        // doesn't speak gRPC and we don't want OpenObserve creds
        // in the browser, so the frontend posts here and we forward.
        .route(
            "/api/telemetry/v1/traces",
            post(telemetry_proxy::traces_handler),
        )
        .with_state(state.clone())
        .merge(auth_routes::router().with_state(auth_state));

    // Optional static-file fallback. When `X11WEB_FRONTEND_DIR`
    // points at a built `frontend/dist`, the backend serves the
    // SPA at `/` (with index.html as the SPA fallback). Used in
    // e2e and production deploys where there's no separate
    // frontend host. In dev with Vite, leave it unset.
    if let Ok(dir) = std::env::var("X11WEB_FRONTEND_DIR") {
        info!("Serving SPA from {dir}");
        app = app.fallback_service(tower_http::services::ServeDir::new(&dir).not_found_service(
            tower_http::services::ServeFile::new(format!("{dir}/index.html")),
        ));
    }

    let app = app
        // CORS with credentials so the SPA dev server (Vite on
        // a different port) can call `/auth/me` etc. with the
        // session cookie attached.
        .layer(
            CorsLayer::new()
                .allow_credentials(true)
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([axum::http::header::CONTENT_TYPE])
                .allow_origin(tower_http::cors::AllowOrigin::predicate(|_origin, _| true)),
        )
        .layer(session_layer);

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

    /// Default HTTP listen address for the dev backend. Production deployments
    /// typically front this with a reverse proxy and override via env var.
    const DEFAULT_HTTP_LISTEN_ADDR: &str = "0.0.0.0:3001";
    let listener = tokio::net::TcpListener::bind(DEFAULT_HTTP_LISTEN_ADDR)
        .await
        .unwrap();
    info!("Backend listening on {DEFAULT_HTTP_LISTEN_ADDR}");
    // Graceful shutdown so OTel's last batch flushes on Ctrl-C —
    // axum returns from `serve` once the signal future resolves,
    // we then explicitly call `telemetry.shutdown()` to drain.
    axum::serve(listener, app)
        .with_graceful_shutdown(x11_web_telemetry::shutdown_signal())
        .await
        .unwrap();
    info!("Shutdown signal received; flushing telemetry...");
    telemetry.shutdown();
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
                //
                // Keep the spawn_origin entry alive past the process's
                // exit. Wrapper-style launchers (`libreoffice` forking
                // soffice.bin, qterminal forking the real terminal,
                // gimp's RPC server, etc.) reap before their
                // descendant connects to the X server. The sidecar
                // side already matches the connecting peer's pid
                // against its spawn history; the backend has to keep
                // (sidecar, wrapper_pid) → workspace mapped here for
                // the auto-attach lookup to succeed. The whole map is
                // freed on sidecar disconnect.
                //
                // Same reasoning applies to `processes`: the sidecar
                // reports `ProcessConnected { pid: wrapper_pid, ... }`
                // for the descendant connecting after the wrapper
                // exits, so the entry whose key is the wrapper pid
                // *also* identifies the descendant. Removing it on
                // ProcessExited would discard the live descendant's
                // mapping. Keep the entry; X11 client disconnect is
                // the right cleanup signal but we don't surface that
                // back to the backend yet — overstaying entries are
                // tolerable (orphan lookups simply fail to find a
                // workspace, which is the pre-existing behaviour).
                let freed_client_ids: Vec<String> = Vec::new();
                let _ = pid;
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
                    let mut sent = 0u64;
                    for frontend in frontends.values() {
                        if frontend
                            .rtc
                            .dc_open
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            let _ = frontend.rtc.dc_tx.send(bytes.clone());
                            sent += 1;
                        }
                    }
                    if let Some(m) = telemetry::metrics() {
                        let kind = [opentelemetry::KeyValue::new("kind", "put_image")];
                        m.frame_count.add(sent, &kind);
                        m.frame_bytes.add(sent * bytes.len() as u64, &kind);
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
                    let mut sent = 0u64;
                    for frontend in frontends.values() {
                        if frontend
                            .rtc
                            .dc_open
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            let _ = frontend.rtc.dc_tx.send(bytes.clone());
                            sent += 1;
                        }
                    }
                    if let Some(m) = telemetry::metrics() {
                        let kind = [opentelemetry::KeyValue::new("kind", "thumbnail")];
                        m.frame_count.add(sent, &kind);
                        m.frame_bytes.add(sent * bytes.len() as u64, &kind);
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
    if let Some(m) = telemetry::metrics() {
        m.sidecars_connected.add(-1, &[]);
    }
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

    // Forward messages from channel to WebSocket. Binary frames
    // carrying Cap'n Proto serialised `BackendMsg`s — the schema
    // lives in `crates/ws-wire/schema/ws.capnp` and the bridge in
    // the same crate handles the protocol-enum ↔ capnp dance.
    // Traceparent is sampled here so a future "trace
    // backend-pushed updates" feature can chain on the receiver.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let traceparent = x11_web_telemetry::current_traceparent();
            let bytes = x11_web_ws_wire::encode_backend_msg(&msg, &traceparent);
            if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    // Spawn the per-frontend WebRTC driver. The DC isn't usable
    // until the browser sends an offer over the WS, but the task is
    // ready to receive signalling immediately.
    //
    // `control_inbound_tx` carries raw bytes received on the control
    // DC. The handler task below decodes each capnp Frame and dispatches
    // workspaceSync variants into the Automerge sync protocol — apply
    // the inbound message, then send any generated reply back over the
    // control DC.
    let (control_inbound_tx, mut control_inbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let rtc = rtc::spawn(frontend_id.clone(), tx.clone(), control_inbound_tx);
    {
        let frontend_id = frontend_id.clone();
        let state = state.clone();
        tokio::spawn(async move {
            while let Some(bytes) = control_inbound_rx.recv().await {
                let Some((workspace_id, message)) = rtc_codec::decode_workspace_sync(&bytes) else {
                    warn!(
                        "control inbound from {frontend_id}: failed to decode workspaceSync \
                         ({} bytes)",
                        bytes.len()
                    );
                    continue;
                };
                let control_tx = {
                    let frontends = state.frontends.read().await;
                    let Some(conn) = frontends.get(&frontend_id) else {
                        continue;
                    };
                    conn.rtc.control_tx.clone()
                };
                let other_peers: Vec<String> = {
                    let mut docs = state.workspace_docs.write().await;
                    let Some(entry) = docs.get_mut(&workspace_id) else {
                        warn!(
                            "workspace sync from {frontend_id}: unknown workspace \
                             {workspace_id}"
                        );
                        continue;
                    };
                    if let Err(e) = entry.receive_sync(&frontend_id, &message) {
                        warn!("workspace sync receive: {e}");
                        continue;
                    }
                    while let Some(reply) = entry.generate_sync(&frontend_id) {
                        let frame = rtc_codec::encode_workspace_sync(&workspace_id, &reply);
                        let _ = control_tx.send(frame);
                    }
                    // Fan out to every other peer bound to this
                    // workspace so a rename in tab A reaches tab B.
                    entry
                        .peer_states
                        .keys()
                        .filter(|p| p.as_str() != frontend_id)
                        .cloned()
                        .collect()
                };
                for peer in other_peers {
                    kick_workspace_sync(&state, &peer, &workspace_id).await;
                }
                // Frontend-driven attach/detach can change which
                // windows need live capture — reconcile now that
                // the doc has caught up with the inbound change.
                reconcile_streaming_after_change(&state).await;
            }
        });
    }

    // Kick the initial sync round when the control DC opens. The
    // workspace_id may not be set yet (the frontend's OpenWorkspace
    // can arrive before or after the DC opens); if it's missing we
    // skip — the OpenWorkspace handler kicks too, so whichever event
    // arrives last triggers the round.
    {
        let frontend_id = frontend_id.clone();
        let state = state.clone();
        let control_opened = rtc.control_opened.clone();
        tokio::spawn(async move {
            control_opened.notified().await;
            let ws_id = {
                let frontends = state.frontends.read().await;
                frontends
                    .get(&frontend_id)
                    .and_then(|c| c.workspace_id.clone())
            };
            if let Some(ws) = ws_id {
                kick_workspace_sync(&state, &frontend_id, &ws).await;
            }
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
    if let Some(m) = telemetry::metrics() {
        m.frontends_connected.add(1, &[]);
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

    // Process incoming messages from frontend. Binary frames with
    // a Cap'n Proto `FrontendMsg`; the bridge gives us back the
    // typed enum + the traceparent header in one call.
    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Binary(bytes) = msg else {
            continue;
        };
        let (msg, traceparent) = match x11_web_ws_wire::decode_frontend_msg(&bytes) {
            Ok(pair) => pair,
            Err(e) => {
                warn!("frontend msg decode failed: {e:?}");
                continue;
            }
        };

        // Adopt the frontend's trace context as the parent for the
        // dispatch span. Anything further down (forward_to_sidecar
        // → QUIC → sidecar) reads `current_traceparent()` from the
        // active context, so the same trace continues end-to-end.
        // Fields are declared empty up front (tracing requires a
        // static field set per call site) and filled in below from
        // the matched variant.
        use x11_web_telemetry::{OpenTelemetrySpanExt, TraceContextExt};
        let parent_ctx = x11_web_telemetry::extract_traceparent(&traceparent);
        let span = tracing::info_span!(
            "backend.frontend_msg",
            traceparent = %traceparent,
            msg.kind = tracing::field::Empty,
            request.id = tracing::field::Empty,
            sidecar.id = tracing::field::Empty,
            workspace.id = tracing::field::Empty,
            window.id = tracing::field::Empty,
            command = tracing::field::Empty,
            pid = tracing::field::Empty,
            width = tracing::field::Empty,
            height = tracing::field::Empty,
            event.kind = tracing::field::Empty,
            error.kind = tracing::field::Empty,
            error.message = tracing::field::Empty,
        );
        if parent_ctx.span().span_context().is_valid() {
            let _ = span.set_parent(parent_ctx);
        }
        record_msg_attrs(&span, &msg);
        let _enter = span.enter();

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
                // Kick the Automerge sync handshake. The doc's
                // window-nodes and `name` are delivered by that
                // initial sync — no separate wire messages.
                // No-op if the control DC hasn't opened yet — the
                // rtc-task's control_opened watcher fires it then.
                kick_workspace_sync(&state, &frontend_id, &workspace_id).await;
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
    if let Some(m) = telemetry::metrics() {
        m.frontends_connected.add(-1, &[]);
    }
    // Drop per-peer sync state in every workspace doc — peer_states
    // would otherwise grow unboundedly across reconnects.
    {
        let mut docs = state.workspace_docs.write().await;
        for entry in docs.values_mut() {
            entry.forget_peer(&frontend_id);
        }
    }
    send_task.abort();
}

/// Stamp the variant-specific IDs onto the active
/// `backend.frontend_msg` span so a single trace shows what the
/// user actually clicked — workspace/sidecar/window IDs, the
/// command name on a spawn, etc. Fields must already be declared
/// (as `field::Empty`) at the span-creation site or `record` is a
/// silent no-op.
fn record_msg_attrs(span: &tracing::Span, msg: &FrontendToBackend) {
    use FrontendToBackend::*;
    match msg {
        OpenWorkspace { id } => {
            span.record("msg.kind", "OpenWorkspace");
            if let Some(id) = id {
                span.record("workspace.id", id.as_str());
            }
        }
        SpawnProcess {
            request_id,
            sidecar_id,
            workspace_id,
            command,
            ..
        } => {
            span.record("msg.kind", "SpawnProcess");
            span.record("request.id", request_id.as_str());
            span.record("sidecar.id", sidecar_id.as_str());
            span.record("workspace.id", workspace_id.as_str());
            span.record("command", command.as_str());
        }
        KillProcess {
            request_id,
            sidecar_id,
            pid,
        } => {
            span.record("msg.kind", "KillProcess");
            span.record("request.id", request_id.as_str());
            span.record("sidecar.id", sidecar_id.as_str());
            span.record("pid", *pid);
        }
        InputEvent {
            sidecar_id,
            window_id,
            event,
        } => {
            span.record("msg.kind", "InputEvent");
            span.record("sidecar.id", sidecar_id.as_str());
            span.record("window.id", window_id.as_str());
            span.record("event.kind", input_event_kind(event));
        }
        ResizeWindow {
            sidecar_id,
            window_id,
            width,
            height,
        } => {
            span.record("msg.kind", "ResizeWindow");
            span.record("sidecar.id", sidecar_id.as_str());
            span.record("window.id", window_id.as_str());
            span.record("width", *width);
            span.record("height", *height);
        }
        RtcOffer { .. } => {
            span.record("msg.kind", "RtcOffer");
        }
        RtcIceCandidate { .. } => {
            span.record("msg.kind", "RtcIceCandidate");
        }
    }
}

fn input_event_kind(e: &x11_web_protocol::InputEvent) -> &'static str {
    use x11_web_protocol::InputEvent::*;
    match e {
        KeyPress { .. } => "KeyPress",
        KeyRelease { .. } => "KeyRelease",
        ButtonPress { .. } => "ButtonPress",
        ButtonRelease { .. } => "ButtonRelease",
        MotionNotify { .. } => "MotionNotify",
        TouchBegin { .. } => "TouchBegin",
        TouchUpdate { .. } => "TouchUpdate",
        TouchEnd { .. } => "TouchEnd",
        GestureSwipe { .. } => "GestureSwipe",
        GesturePinch { .. } => "GesturePinch",
        MenuActivate { .. } => "MenuActivate",
        _ => "other",
    }
}

async fn forward_to_sidecar(state: &AppState, sidecar_id: &str, msg: BackendToSidecar) {
    // Snapshot the traceparent here, while the WS handler's
    // `backend.frontend_msg` span is still on top of the active
    // context. The QUIC writer reads this off the channel later.
    let traceparent = x11_web_telemetry::current_traceparent();
    let sidecars = state.sidecars.read().await;
    if let Some(sidecar) = sidecars.get(sidecar_id) {
        let _ = sidecar.tx.send((msg, traceparent));
    } else {
        warn!("Sidecar not found: {}", sidecar_id);
        // Caller's `backend.frontend_msg` is still active — mark
        // it failed so the trace shows red in OpenObserve.
        x11_web_telemetry::mark_span_error("sidecar_not_found", format!("sidecar id={sidecar_id}"));
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
        DisplayUpdate::WindowCreated {
            window_id,
            override_redirect,
            ..
        } => {
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
            // Pop-ups (override_redirect) keep X-server-authoritative
            // placement and don't get a doc node — they live entirely
            // in `WindowList` and disappear with their parent.
            if !*override_redirect && matches!(kind, SidecarKind::X11 | SidecarKind::Unknown) {
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
                    backend_attach_window(state, &workspace_id, sidecar_id, window_id).await;
                }
            }
        }
        DisplayUpdate::WindowDestroyed { window_id } => {
            // Remove the window-node from every workspace; for each
            // that changed, fan the doc mutation out so peers see
            // the disappearance. Then reconcile streaming so the
            // sidecar gets a `StopWindowCapture` if this was the
            // last attach.
            let affected: Vec<String> = {
                let mut docs = state.workspace_docs.write().await;
                docs.iter_mut()
                    .filter_map(|(ws_id, entry)| {
                        entry.detach_window_node(window_id).then(|| ws_id.clone())
                    })
                    .collect()
            };
            for ws_id in &affected {
                fan_out_workspace_sync(state, ws_id).await;
            }
            reconcile_streaming_after_change(state).await;
            state.window_owner.write().await.remove(window_id);
        }
        DisplayUpdate::WindowConfigured {
            window_id,
            width,
            height,
            ..
        } => {
            // Sidecar resized the window — mirror new dimensions
            // onto every workspace's matching window-node so peers
            // see the new size.
            let affected: Vec<String> = {
                let mut docs = state.workspace_docs.write().await;
                docs.iter_mut()
                    .filter_map(|(ws_id, entry)| {
                        entry
                            .set_window_node_size(window_id, *width as f64, *height as f64)
                            .then(|| ws_id.clone())
                    })
                    .collect()
            };
            for ws_id in &affected {
                fan_out_workspace_sync(state, ws_id).await;
            }
        }
        _ => {}
    }
}

/// Backend-side attach: insert a window-node into `workspace_id`'s
/// doc carrying the `@x11web/window` extension, fan the change out
/// to every connected peer, and reconcile streaming refcount.
/// Width/height come from `window_track` (the X server's reported
/// dimensions); position is cascaded from the workspace's existing
/// node count so successive spawns don't pile on top of each other.
/// Used by X11 auto-attach on `WindowCreated`. Frontend-side
/// attaches arrive as inbound sync messages and never call this
/// directly.
async fn backend_attach_window(
    state: &AppState,
    workspace_id: &str,
    sidecar_id: &str,
    window_id: &str,
) {
    // Pull the sidecar-reported dimensions for this window.
    let (width, height) = {
        let track = state.window_track.read().await;
        match track
            .iter()
            .find(|((_, _, w), _)| w == window_id)
            .map(|(_, w)| (w.width as f64, w.height as f64))
        {
            Some(dims) => dims,
            None => {
                warn!("backend_attach_window: window {window_id} not in tracker");
                return;
            }
        }
    };
    let changed = {
        let mut docs = state.workspace_docs.write().await;
        let Some(entry) = docs.get_mut(workspace_id) else {
            warn!("backend_attach_window: no doc for workspace {workspace_id}");
            return;
        };
        // Cascade position + z so the new window lands on top and
        // visibly offset from existing nodes. (200, 100) is the
        // canvas-space anchor; 30px stairsteps look natural.
        // `node_stats` reads via targeted ops — `entry.snapshot()`
        // would `hydrate` the whole doc, which panics if any
        // existing node was created on a peer that didn't write
        // explicit `Null`s for absent `Option` fields (the JS
        // Automerge API doesn't).
        let (node_count, max_z) = entry.node_stats();
        let x = 200.0 + node_count as f64 * 30.0;
        let y = 100.0 + node_count as f64 * 30.0;
        entry.attach_window_node(window_id, sidecar_id, x, y, max_z + 1.0, width, height)
    };
    if !changed {
        return;
    }
    fan_out_workspace_sync(state, workspace_id).await;
    reconcile_streaming_after_change(state).await;
}

async fn send_to_sidecar(state: &AppState, sidecar_id: &str, msg: BackendToSidecar) {
    let traceparent = x11_web_telemetry::current_traceparent();
    if let Some(sc) = state.sidecars.read().await.get(sidecar_id) {
        let _ = sc.tx.send((msg, traceparent));
    }
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
    let name = format!("Workspace {next_index}");
    let workspace = Workspace {
        id: id.clone(),
        name: name.clone(),
    };
    workspaces.insert(id.clone(), workspace.clone());
    drop(workspaces);
    // Seed the Automerge doc with the same name. Frontends will
    // pick this up via the sync protocol once they bind to the
    // workspace and the control DC is open.
    state
        .workspace_docs
        .write()
        .await
        .entry(id)
        .or_insert_with(|| workspace_doc::WorkspaceEntry::new(&name));
    workspace
}

/// Fan a backend-side mutation out to every peer that's already
/// done at least one sync round for this workspace. Newly-connected
/// peers don't need this — their first sync via the OpenWorkspace
/// handler picks up whatever's in the doc at that point.
async fn fan_out_workspace_sync(state: &AppState, workspace_id: &str) {
    let peers = {
        let docs = state.workspace_docs.read().await;
        docs.get(workspace_id)
            .map(|e| e.peers())
            .unwrap_or_default()
    };
    for peer in peers {
        kick_workspace_sync(state, &peer, workspace_id).await;
    }
}

/// Recompute the global per-window attach refcount from the union of
/// every workspace doc's window-node ids, diff against the cached
/// `streaming_refcount`, and drive `Start/StopWindowCapture` on the
/// owning sidecar for each transition. Idempotent — call after every
/// mutation that could change attached state (incoming sync,
/// backend-side attach/detach, window destroy).
async fn reconcile_streaming_after_change(state: &AppState) {
    use std::collections::HashMap;
    let new_counts: HashMap<String, usize> = {
        let docs = state.workspace_docs.read().await;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for entry in docs.values() {
            for window_id in entry.window_node_ids() {
                *counts.entry(window_id).or_insert(0) += 1;
            }
        }
        counts
    };
    let (starts, stops) = {
        let mut rc = state.streaming_refcount.write().await;
        let mut starts: Vec<String> = Vec::new();
        let mut stops: Vec<String> = Vec::new();
        for (window_id, old) in rc.iter() {
            let new = new_counts.get(window_id).copied().unwrap_or(0);
            if *old > 0 && new == 0 {
                stops.push(window_id.clone());
            }
        }
        for (window_id, new) in &new_counts {
            let old = rc.get(window_id).copied().unwrap_or(0);
            if old == 0 && *new > 0 {
                starts.push(window_id.clone());
            }
        }
        // Drop zero-count entries so the map only holds active windows.
        rc.clear();
        for (k, v) in new_counts {
            if v > 0 {
                rc.insert(k, v);
            }
        }
        (starts, stops)
    };
    let owners = state.window_owner.read().await.clone();
    for window_id in starts {
        if let Some(sidecar_id) = owners.get(&window_id) {
            send_to_sidecar(
                state,
                sidecar_id,
                BackendToSidecar::StartWindowCapture {
                    window_id: window_id.clone(),
                },
            )
            .await;
        }
    }
    for window_id in stops {
        if let Some(sidecar_id) = owners.get(&window_id) {
            send_to_sidecar(
                state,
                sidecar_id,
                BackendToSidecar::StopWindowCapture {
                    window_id: window_id.clone(),
                },
            )
            .await;
        }
    }
}

/// Drive a sync round for one (frontend, workspace) pair. Generates
/// outbound sync messages until the peer is caught up, encodes each
/// as a capnp `WorkspaceSync` frame, and pushes onto the control DC.
/// Safe to call repeatedly — `generate_sync_message` returns `None`
/// when there's nothing to send.
async fn kick_workspace_sync(state: &AppState, frontend_id: &str, workspace_id: &str) {
    use std::sync::atomic::Ordering;
    let (control_tx, control_open) = {
        let frontends = state.frontends.read().await;
        let Some(conn) = frontends.get(frontend_id) else {
            return;
        };
        (
            conn.rtc.control_tx.clone(),
            conn.rtc.control_open.load(Ordering::Acquire),
        )
    };
    if !control_open {
        // Caller will retry once the DC opens.
        return;
    }
    let mut docs = state.workspace_docs.write().await;
    let Some(entry) = docs.get_mut(workspace_id) else {
        warn!("kick_workspace_sync: no doc for workspace {workspace_id}");
        return;
    };
    while let Some(msg) = entry.generate_sync(frontend_id) {
        let bytes = rtc_codec::encode_workspace_sync(workspace_id, &msg);
        let _ = control_tx.send(bytes);
    }
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

/// Build the current authoritative window list (visible windows
/// only) and broadcast it to all frontends. `(x, y)` is sidecar
/// geometry — meaningful only for `override_redirect` popups,
/// which keep X-server placement. Top-level windows take their
/// canvas position from the matching `OcifNode` in the workspace
/// doc; the descriptor's `(x, y)` is ignored on the frontend.
async fn build_window_list(state: &AppState) -> Vec<WindowDescriptor> {
    let track = state.window_track.read().await;
    let order = state.window_order.read().await;
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
            x: w.x as f64,
            y: w.y as f64,
            width: w.width,
            height: w.height,
            border_width: w.border_width,
            border_pixel: w.border_pixel,
            override_redirect: w.override_redirect,
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
