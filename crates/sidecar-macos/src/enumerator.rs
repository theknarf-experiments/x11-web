//! 1Hz window-enumeration poll → `DisplayUpdate` diff stream.
//!
//! Runs alongside the WS session: re-enumerates `visible_windows()`
//! every second, compares against the previous snapshot, and emits the
//! protocol-level events the frontend needs to render window frames.
//!
//! The mapping is intentionally simple for v0.1: one CGWindowID = one
//! UUID, one pid = one synthetic `client_id` of the form
//! `"macos-pid-{pid}"`. We don't try to reflect the macOS app/window
//! hierarchy any deeper than that — every window is a top-level
//! `WindowFrame` from the frontend's perspective.

use std::collections::HashMap;
use std::time::Duration;

use core_graphics::window::CGWindowID;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{info, warn};
use uuid::Uuid;
use x11_web_protocol::{DisplayUpdate, SidecarToBackend};

use crate::capture::capture_window;
use crate::windows::{visible_windows, WindowBounds, WindowInfo};

/// Cap the longer side of each capture (in points). Keeps WS payload
/// per frame bounded — RGBA at 800×600 is ~1.9 MB, comfortable for
/// JSON+base64 over WebSocket at the cadence we run at.
const CAPTURE_MAX_DIM: u32 = 800;

/// Period between successive captures of the same window. Low-rate
/// for v0.2 — once we move pixels onto the WebRTC data channel we'll
/// crank this up.
const CAPTURE_PERIOD: Duration = Duration::from_secs(1);

/// Layer 0 = "normal" application windows. Higher layers are menubar
/// items (25), the dock (20), tooltips/popovers, etc. cua-driver uses
/// the same predicate.
const TOP_LEVEL_LAYER: i32 = 0;

/// Drop windows below this size — WindowServer keeps 1×1 placeholders
/// for some system services that we don't want to surface as frames.
const MIN_DIMENSION: f64 = 2.0;

struct Tracked {
    uuid: String,
    pid: i32,
    bounds: WindowBounds,
    title: String,
    capture_handle: tokio::task::JoinHandle<()>,
}

/// Spawn the enumeration task. Sends `SidecarToBackend` messages on
/// `tx` for every window state change, and a per-window capture task
/// per tracked window that streams `PutImage` updates at
/// `CAPTURE_PERIOD`.
pub fn spawn(tx: mpsc::UnboundedSender<SidecarToBackend>) {
    tokio::spawn(async move {
        let mut tracked: HashMap<CGWindowID, Tracked> = HashMap::new();
        let mut announced_pids: HashMap<i32, String> = HashMap::new();
        let mut tick = interval(Duration::from_secs(1));

        loop {
            tick.tick().await;
            let current = visible_windows()
                .into_iter()
                .filter(is_renderable)
                .map(|w| (w.id, w))
                .collect::<HashMap<_, _>>();

            // Newly-seen windows.
            for (id, win) in &current {
                if tracked.contains_key(id) {
                    continue;
                }
                let uuid = Uuid::new_v4().to_string();
                announce_pid_if_new(&mut announced_pids, win, &tx);
                emit_created(&tx, &uuid, win);
                let capture_handle = spawn_capture_loop(*id, uuid.clone(), win.pid, tx.clone());
                tracked.insert(
                    *id,
                    Tracked {
                        uuid,
                        pid: win.pid,
                        bounds: win.bounds,
                        title: win.name.clone(),
                        capture_handle,
                    },
                );
            }

            // Existing windows: check for bounds / title changes.
            for (id, win) in &current {
                if let Some(prev) = tracked.get_mut(id) {
                    if !bounds_eq(&prev.bounds, &win.bounds) {
                        emit_configured(&tx, &prev.uuid, &win.bounds);
                        prev.bounds = win.bounds;
                    }
                    if prev.title != win.name {
                        emit_title(&tx, &prev.uuid, &win.name);
                        prev.title = win.name.clone();
                    }
                }
            }

            // Vanished windows.
            tracked.retain(|id, prev| {
                if current.contains_key(id) {
                    true
                } else {
                    prev.capture_handle.abort();
                    let _ = tx.send(SidecarToBackend::DisplayUpdate {
                        client_id: client_id_for_pid(prev.pid),
                        update: DisplayUpdate::WindowDestroyed {
                            window_id: prev.uuid.clone(),
                        },
                    });
                    false
                }
            });
        }
    });
}

/// Per-window capture task: every `CAPTURE_PERIOD`, capture and emit
/// a `PutImage` covering the whole window. Aborted by the enumerator
/// when the window vanishes.
fn spawn_capture_loop(
    cg_id: CGWindowID,
    uuid: String,
    pid: i32,
    tx: mpsc::UnboundedSender<SidecarToBackend>,
) -> tokio::task::JoinHandle<()> {
    let client_id = client_id_for_pid(pid);
    tokio::spawn(async move {
        let mut tick = interval(CAPTURE_PERIOD);
        // Skip the first tick — `interval` fires immediately, and we
        // already raced the WindowCreated/Mapped emit; let the
        // frontend draw the empty frame first to avoid the canvas
        // flickering between sizes.
        tick.tick().await;
        loop {
            tick.tick().await;
            match capture_window(cg_id, CAPTURE_MAX_DIM).await {
                Ok(frame) => {
                    let _ = tx.send(SidecarToBackend::DisplayUpdate {
                        client_id: client_id.clone(),
                        update: DisplayUpdate::PutImage {
                            window_id: uuid.clone(),
                            x: 0,
                            y: 0,
                            width: frame.width.min(u16::MAX as u32) as u16,
                            height: frame.height.min(u16::MAX as u32) as u16,
                            data: frame.rgba,
                        },
                    });
                }
                Err(e) => {
                    warn!("capture failed for window {cg_id} ({client_id}): {e}");
                }
            }
        }
    })
}

fn is_renderable(w: &WindowInfo) -> bool {
    w.layer == TOP_LEVEL_LAYER
        && w.on_screen
        && w.bounds.width >= MIN_DIMENSION
        && w.bounds.height >= MIN_DIMENSION
}

fn announce_pid_if_new(
    announced: &mut HashMap<i32, String>,
    win: &WindowInfo,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
) {
    if announced.contains_key(&win.pid) {
        return;
    }
    let client_id = client_id_for_pid(win.pid);
    let command = if win.owner.is_empty() {
        format!("pid-{}", win.pid)
    } else {
        win.owner.clone()
    };
    info!(
        "Discovered macOS process pid={} command={:?} client_id={}",
        win.pid, command, client_id
    );
    let _ = tx.send(SidecarToBackend::ProcessConnected {
        pid: win.pid as u32,
        client_id: client_id.clone(),
        command,
    });
    announced.insert(win.pid, client_id);
}

fn emit_created(
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    uuid: &str,
    win: &WindowInfo,
) {
    let (x, y, w, h) = clamp_bounds(&win.bounds);
    let client_id = client_id_for_pid(win.pid);
    let _ = tx.send(SidecarToBackend::DisplayUpdate {
        client_id: client_id.clone(),
        update: DisplayUpdate::WindowCreated {
            window_id: uuid.to_string(),
            x,
            y,
            width: w,
            height: h,
            is_top_level: true,
            override_redirect: false,
            border_width: 0,
            border_pixel: 0,
        },
    });
    let _ = tx.send(SidecarToBackend::DisplayUpdate {
        client_id: client_id.clone(),
        update: DisplayUpdate::WindowMapped {
            window_id: uuid.to_string(),
            is_top_level: true,
            override_redirect: false,
        },
    });
    if !win.name.is_empty() {
        let _ = tx.send(SidecarToBackend::DisplayUpdate {
            client_id,
            update: DisplayUpdate::TitleChanged {
                window_id: uuid.to_string(),
                title: win.name.clone(),
            },
        });
    }
}

fn emit_configured(
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    uuid: &str,
    bounds: &WindowBounds,
) {
    let (x, y, w, h) = clamp_bounds(bounds);
    // We don't know the pid from a UUID alone — this would matter for
    // the WS protocol's `client_id` routing, but the backend just uses
    // it as an opaque key, so any consistent value works. Using the
    // window's UUID prefix keeps it stable per-window.
    let _ = tx.send(SidecarToBackend::DisplayUpdate {
        client_id: format!("macos-win-{}", &uuid[..8]),
        update: DisplayUpdate::WindowConfigured {
            window_id: uuid.to_string(),
            x,
            y,
            width: w,
            height: h,
            border_width: 0,
            border_pixel: 0,
        },
    });
}

fn emit_title(
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    uuid: &str,
    title: &str,
) {
    let _ = tx.send(SidecarToBackend::DisplayUpdate {
        client_id: format!("macos-win-{}", &uuid[..8]),
        update: DisplayUpdate::TitleChanged {
            window_id: uuid.to_string(),
            title: title.to_string(),
        },
    });
}

fn client_id_for_pid(pid: i32) -> String {
    format!("macos-pid-{pid}")
}

fn bounds_eq(a: &WindowBounds, b: &WindowBounds) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.width - b.width).abs() < 0.5
        && (a.height - b.height).abs() < 0.5
}

fn clamp_bounds(b: &WindowBounds) -> (i16, i16, u16, u16) {
    let x = b.x.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    let y = b.y.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    let w = b.width.clamp(0.0, u16::MAX as f64) as u16;
    let h = b.height.clamp(0.0, u16::MAX as f64) as u16;
    (x, y, w, h)
}
