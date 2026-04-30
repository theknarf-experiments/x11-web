//! QUIC sidecar listener — accepts connections from sidecars
//! using the wire crate's protocol (TLS 1.3 + Cap'n Proto), runs
//! the same `dispatch_sidecar_msg` / `cleanup_sidecar` helpers the
//! existing WebSocket handler uses.
//!
//! Each accepted connection becomes a `SidecarConnection` in
//! `AppState.sidecars` (same registry the WebSocket path
//! populates) — frontends don't need to know which transport a
//! given sidecar is using.
//!
//! For now: WebSocket sidecars (X11) and QUIC sidecars (macOS)
//! coexist. When the X11 sidecar migrates we drop the WebSocket
//! sidecar listener and only this stays.

use std::net::SocketAddr;

use tokio::sync::mpsc;
use tracing::{info, warn};
use x11_web_protocol::{BackendToSidecar, SidecarInfo};
use x11_web_wire::conn::{accept, listen};
use x11_web_wire::tls::ServerCert;
use x11_web_wire::wire_capnp;

use crate::wire_bridge;
use crate::{
    broadcast_to_frontends, cleanup_sidecar, dispatch_sidecar_msg, AppState, SidecarConnection,
};

/// Spawn the QUIC accept loop on its own OS thread. Each accepted
/// connection runs on the same thread via `tokio::task::spawn_local`
/// inside a `LocalSet` — `tokio::spawn` would reject these because
/// capnp message readers contain raw pointers that aren't `Send`.
///
/// Cross-runtime communication is fine: `tokio::sync::mpsc` and
/// `RwLock` are runtime-agnostic, so QUIC handlers can read/write
/// the same `AppState` registries the main multi-thread runtime
/// uses.
pub fn spawn_listener(
    state: AppState,
    bind: SocketAddr,
    cert: ServerCert,
    expected_token: Vec<u8>,
) {
    std::thread::Builder::new()
        .name("quic-listener".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("quic runtime build");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let endpoint = match listen(bind, &cert) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!("QUIC listen on {bind}: {e}");
                        return;
                    }
                };
                info!("QUIC sidecar listener bound on {bind}");
                loop {
                    let expected = expected_token.clone();
                    match accept(&endpoint, move |token, _name| {
                        if token == expected.as_slice() {
                            Ok(uuid::Uuid::new_v4().to_string())
                        } else {
                            Err("invalid bearer token".into())
                        }
                    })
                    .await
                    {
                        Ok(accepted) => {
                            let state = state.clone();
                            tokio::task::spawn_local(handle_quic_session(state, accepted));
                        }
                        Err(e) => {
                            warn!("QUIC accept failed: {e}");
                        }
                    }
                }
            });
        })
        .expect("spawn quic-listener thread");
}

async fn handle_quic_session(state: AppState, accepted: x11_web_wire::conn::AcceptedConnection) {
    let sidecar_id = accepted.sidecar_id.clone();
    let info = SidecarInfo {
        id: sidecar_id.clone(),
        name: accepted.sidecar_name.clone(),
    };
    info!("Sidecar connected over QUIC: {} ({})", info.name, info.id);

    // Register sidecar with a tx that translates BackendToSidecar
    // into wire frames before they hit the QUIC stream. This
    // matches what the WebSocket handler does (its tx serialises
    // to JSON in a send_task), keeping `dispatch_sidecar_msg` /
    // existing frontend code transport-agnostic.
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendToSidecar>();
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

    // Notify frontends that a new sidecar joined.
    broadcast_to_frontends(
        &state,
        x11_web_protocol::BackendToFrontend::SidecarConnected {
            sidecar: info.clone(),
        },
    )
    .await;

    let mut writer = accepted.writer;
    let mut reader = accepted.reader;

    // recv loop + send loop run in the same task because capnp
    // message readers contain raw pointers that aren't `Send` —
    // tokio::spawn would reject them. `tokio::select!` on the
    // same task is fine (no Send required).
    let recv_loop = async {
        loop {
            let msg = match reader
                .read_message::<wire_capnp::from_sidecar::Owned>()
                .await
            {
                Ok(Some(m)) => m,
                Ok(None) => return,
                Err(e) => {
                    warn!("QUIC recv error: {e}");
                    return;
                }
            };
            let from: wire_capnp::from_sidecar::Reader = match msg.get_root() {
                Ok(r) => r,
                Err(e) => {
                    warn!("FromSidecar root: {e}");
                    continue;
                }
            };
            let internal = match wire_bridge::read_from_sidecar(from) {
                Ok(m) => m,
                Err(e) => {
                    warn!("FromSidecar translate: {e:?}");
                    continue;
                }
            };
            dispatch_sidecar_msg(&state, &sidecar_id, internal).await;
        }
    };
    let send_loop = async {
        while let Some(msg) = rx.recv().await {
            let Some(builder) = wire_bridge::build_to_sidecar(&msg) else {
                continue;
            };
            if let Err(e) = writer.write_message(&builder).await {
                warn!("QUIC write error: {e}");
                return;
            }
        }
    };

    tokio::select! {
        _ = recv_loop => {}
        _ = send_loop => {}
    }

    cleanup_sidecar(&state, &sidecar_id).await;
}
