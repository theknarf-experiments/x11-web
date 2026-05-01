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
use std::time::{Duration, Instant};

use core_graphics::window::CGWindowID;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{info, warn};
use uuid::Uuid;
use x11_web_protocol::DisplayUpdate;
use x11_web_wire::SidecarToBackend;

use crate::capture::{build_session, recv_frame_timeout};
use crate::router::{WindowRoute, WindowRouter};
use crate::screenshot;
use crate::windows::{visible_windows, WindowBounds, WindowInfo};

/// Target FPS for SCStream delivery. SCK dedups idle frames
/// internally, so this is a *cap*, not a fixed cadence — windows that
/// aren't repainting won't generate callbacks. The latest-frame-wins
/// channel + RTC backpressure mean over-requesting is safe.
const CAPTURE_FPS: u32 = 30;

/// Polling interval for the shutdown signal while waiting on the
/// SCStream frame channel. Worst-case shutdown latency = this value.
const SHUTDOWN_POLL: Duration = Duration::from_millis(100);

/// Cap the longer side of a thumbnail at this many pixels. Sized for
/// the spawn-popover picker — small enough to encode quickly and
/// keep the WebRTC payload under 30 KB at the WebP quality below.
const THUMBNAIL_MAX_DIM: u32 = 320;

/// Period between successive thumbnail captures of the same window.
/// 1 Hz is enough to feel live while the user hovers the picker
/// without hammering the screenshot API (which still pays a small
/// per-call cost even with the filter cached).
const THUMBNAIL_PERIOD: Duration = Duration::from_secs(1);

/// WebP quality for thumbnails. Lower than the 90 we use for live
/// frames — we're targeting <30 KB per thumbnail and a few pixels of
/// blocky-ness in the picker is invisible at 320 px.
const THUMBNAIL_QUALITY: f32 = 70.0;

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
    /// Live SCStream capture handle. `None` by default — only
    /// populated when the backend asks for it via
    /// `CaptureControl::Start`. Drops back to `None` when the
    /// backend asks to stop or the window vanishes.
    capture_stop: Option<CaptureStop>,
    thumbnail_stop: Option<CaptureStop>,
}

/// Commands the enumerator listens for so on-demand SCStream
/// captures can be started / stopped without rebuilding the whole
/// `Tracked` map. `window_id` is the UUID the enumerator handed out
/// for the window.
#[derive(Debug)]
pub enum CaptureControl {
    Start { window_id: String },
    Stop { window_id: String },
}

/// Handle for stopping a per-window capture thread. The thread owns
/// non-Send ObjC state (a `Retained<SCContentFilter>` inside its
/// cached session), so it has to be a real OS thread, not a tokio
/// task. We signal it to exit by sending on `shutdown`; the thread
/// breaks out of its loop on the next tick.
struct CaptureStop {
    shutdown: std::sync::mpsc::Sender<()>,
    handle: std::thread::JoinHandle<()>,
}

impl CaptureStop {
    fn abort(self) {
        let _ = self.shutdown.send(());
        // Don't join — the thread can take up to one CAPTURE_PERIOD to
        // notice the shutdown, and we don't want to block the
        // enumerator tick. The thread shuts itself down cleanly.
        drop(self.handle);
    }
}

/// Spawn the enumeration task. Sends `SidecarToBackend` messages on
/// `tx` for every window state change, plus a per-window thumbnail
/// task. Live SCStream capture is started / stopped on demand via
/// `capture_ctl_rx` (the backend asks when a workspace attaches a
/// window). `router` is updated in lockstep so the input path can
/// resolve window UUIDs back to pid + screen origin.
pub fn spawn(
    tx: mpsc::UnboundedSender<SidecarToBackend>,
    router: WindowRouter,
    mut capture_ctl_rx: mpsc::UnboundedReceiver<CaptureControl>,
) {
    tokio::spawn(async move {
        let mut tracked: HashMap<CGWindowID, Tracked> = HashMap::new();
        let mut announced_pids: HashMap<i32, String> = HashMap::new();
        let mut tick = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    enumerate_step(&mut tracked, &mut announced_pids, &tx, &router);
                }
                Some(cmd) = capture_ctl_rx.recv() => {
                    handle_capture_control(cmd, &mut tracked, &tx);
                }
            }
        }
    });
}

fn enumerate_step(
    tracked: &mut HashMap<CGWindowID, Tracked>,
    announced_pids: &mut HashMap<i32, String>,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
    router: &WindowRouter,
) {
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
        announce_pid_if_new(announced_pids, win, tx);
        emit_created(tx, &uuid, win);
        router.insert(
            uuid.clone(),
            WindowRoute {
                cg_id: *id,
                pid: win.pid,
                bounds: win.bounds,
            },
        );
        // Thumbnails always run so the picker has live previews.
        // Live capture is started later via CaptureControl::Start.
        let thumbnail_stop = spawn_thumbnail_loop(*id, uuid.clone(), win.pid, tx.clone());
        tracked.insert(
            *id,
            Tracked {
                uuid,
                pid: win.pid,
                bounds: win.bounds,
                title: win.name.clone(),
                capture_stop: None,
                thumbnail_stop: Some(thumbnail_stop),
            },
        );
    }

    // Existing windows: check for bounds / title changes.
    for (id, win) in &current {
        if let Some(prev) = tracked.get_mut(id) {
            if !bounds_eq(&prev.bounds, &win.bounds) {
                emit_configured(tx, &prev.uuid, &win.bounds);
                router.update_bounds(&prev.uuid, win.bounds);
                prev.bounds = win.bounds;
            }
            if prev.title != win.name {
                emit_title(tx, &prev.uuid, &win.name);
                prev.title = win.name.clone();
            }
        }
    }

    // Vanished windows.
    tracked.retain(|id, prev| {
        if current.contains_key(id) {
            true
        } else {
            if let Some(stop) = prev.capture_stop.take() {
                stop.abort();
            }
            if let Some(stop) = prev.thumbnail_stop.take() {
                stop.abort();
            }
            router.remove(&prev.uuid);
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

fn handle_capture_control(
    cmd: CaptureControl,
    tracked: &mut HashMap<CGWindowID, Tracked>,
    tx: &mpsc::UnboundedSender<SidecarToBackend>,
) {
    match cmd {
        CaptureControl::Start { window_id } => {
            // Find the Tracked entry by UUID. The map is keyed by
            // CGWindowID for cheap lookup during enumeration; here
            // we iterate since per-second start/stop is rare.
            let entry = tracked.iter_mut().find(|(_, t)| t.uuid == window_id);
            let Some((cg_id, t)) = entry else {
                warn!("StartWindowCapture for unknown window_id={window_id}");
                return;
            };
            if t.capture_stop.is_some() {
                return; // already streaming
            }
            let stop = spawn_capture_loop(*cg_id, t.uuid.clone(), t.pid, tx.clone());
            t.capture_stop = Some(stop);
            info!("Started live capture for window {} (cg_id={cg_id})", t.uuid);
            // Mirror the app's macOS menu bar into a MenuStructure
            // event so the frontend's GlobalMenuBar can render it.
            // Reads block on AX RPC into the target process, so we
            // hand it off to the blocking thread pool — the
            // enumerator loop must stay responsive.
            spawn_menu_read(t.pid, t.uuid.clone(), tx.clone());
        }
        CaptureControl::Stop { window_id } => {
            let entry = tracked.iter_mut().find(|(_, t)| t.uuid == window_id);
            let Some((cg_id, t)) = entry else {
                warn!("StopWindowCapture for unknown window_id={window_id}");
                return;
            };
            if let Some(stop) = t.capture_stop.take() {
                stop.abort();
                info!("Stopped live capture for window {} (cg_id={cg_id})", t.uuid);
            }
        }
    }
}

/// Read the macOS menu bar of the app at `pid` and emit a
/// `MenuStructure` for `window_id`. AX is RPC into the target
/// process and can take 5–50 ms (longer if the app is slow), so we
/// run on the blocking pool with a wall-clock budget. Empty menu on
/// failure / timeout is a benign no-op for the frontend's
/// GlobalMenuBar.
fn spawn_menu_read(pid: i32, window_id: String, tx: mpsc::UnboundedSender<SidecarToBackend>) {
    let client_id = client_id_for_pid(pid);
    let log_window_id = window_id.clone();
    tokio::task::spawn_blocking(move || {
        let menu = crate::menu::read_menu_bar_with_timeout(pid, Duration::from_millis(500));
        // Surface the AX outcome so an empty menu (the most common
        // failure mode — missing TCC grant, or the app isn't
        // frontmost) is visible in the sidecar log instead of
        // failing silently into the frontend.
        if menu.is_empty() {
            warn!(
                "menu: AX read returned 0 items for pid={pid} window={} \
                 (check Accessibility permission; some apps only expose \
                 their menu bar when frontmost)",
                &log_window_id[..log_window_id.len().min(8)]
            );
        } else {
            info!(
                "menu: pid={pid} window={} → {} top-level items",
                &log_window_id[..log_window_id.len().min(8)],
                menu.len()
            );
        }
        let _ = tx.send(SidecarToBackend::DisplayUpdate {
            client_id,
            update: DisplayUpdate::MenuStructure { window_id, menu },
        });
    });
}

/// Per-window capture loop. Runs on a dedicated OS thread because
/// the SCStream owns ObjC state we don't want migrating across the
/// tokio worker pool, and `recv_frame_timeout` blocks. SCK pushes
/// frames through the session's bounded(1) channel; we encode each
/// one and forward it as a `PutImage`.
fn spawn_capture_loop(
    cg_id: CGWindowID,
    uuid: String,
    pid: i32,
    tx: mpsc::UnboundedSender<SidecarToBackend>,
) -> CaptureStop {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::Builder::new()
        .name(format!("macos-capture-{cg_id}"))
        .spawn(move || run_capture_loop(cg_id, uuid, pid, tx, shutdown_rx))
        .expect("spawn macos capture thread");
    CaptureStop {
        shutdown: shutdown_tx,
        handle,
    }
}

fn run_capture_loop(
    cg_id: CGWindowID,
    uuid: String,
    pid: i32,
    tx: mpsc::UnboundedSender<SidecarToBackend>,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) {
    let client_id = client_id_for_pid(pid);
    let session = match build_session(cg_id, CAPTURE_FPS) {
        Ok(s) => s,
        Err(e) => {
            warn!("session build failed for window {cg_id} ({client_id}): {e}");
            return;
        }
    };
    loop {
        // Non-blocking shutdown check before each frame. Combined with
        // recv_frame_timeout(SHUTDOWN_POLL) below, worst-case
        // shutdown latency is ~100ms.
        if shutdown_rx.try_recv().is_ok() {
            return;
        }
        let t0 = Instant::now();
        let frame = match recv_frame_timeout(&session, SHUTDOWN_POLL) {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                warn!("capture stream ended for window {cg_id} ({client_id}): {e}");
                return;
            }
        };
        let t_capture = t0.elapsed();
        let t1 = Instant::now();
        // Lossy q90: visually identical to lossless for typical UI /
        // screen content but ~5-10× faster encode and smaller wire
        // payload. Switch to `encode_rgba_lossless` if fidelity for
        // tiny text or high-contrast edges matters.
        let compressed =
            x11_web_pixel_codec::encode_rgba_lossy(&frame.rgba, frame.width, frame.height, 90.0);
        let t_encode = t1.elapsed();
        let raw_kb = frame.rgba.len() / 1024;
        let comp_kb = compressed.len() / 1024;
        info!(
            "capture[{}] {}x{} raw={}KB comp={}KB capture={:?} encode={:?}",
            &uuid[..8],
            frame.width,
            frame.height,
            raw_kb,
            comp_kb,
            t_capture,
            t_encode,
        );
        if tx
            .send(SidecarToBackend::DisplayUpdate {
                client_id: client_id.clone(),
                update: DisplayUpdate::PutImage {
                    window_id: uuid.clone(),
                    x: 0,
                    y: 0,
                    width: frame.width.min(u16::MAX as u32) as u16,
                    height: frame.height.min(u16::MAX as u32) as u16,
                    data: compressed,
                },
            })
            .is_err()
        {
            // Backend channel closed — sidecar is shutting down.
            return;
        }
    }
}

/// Per-window thumbnail loop. Runs at `THUMBNAIL_PERIOD` cadence on
/// its own OS thread, capturing a `THUMBNAIL_MAX_DIM`-capped
/// downscaled screenshot via the one-shot `SCScreenshotManager` path
/// (separate from the live SCStream — they have different cadences
/// and consumers).
fn spawn_thumbnail_loop(
    cg_id: CGWindowID,
    uuid: String,
    pid: i32,
    tx: mpsc::UnboundedSender<SidecarToBackend>,
) -> CaptureStop {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::Builder::new()
        .name(format!("macos-thumb-{cg_id}"))
        .spawn(move || run_thumbnail_loop(cg_id, uuid, pid, tx, shutdown_rx))
        .expect("spawn macos thumbnail thread");
    CaptureStop {
        shutdown: shutdown_tx,
        handle,
    }
}

fn run_thumbnail_loop(
    cg_id: CGWindowID,
    uuid: String,
    pid: i32,
    tx: mpsc::UnboundedSender<SidecarToBackend>,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) {
    use std::sync::mpsc::RecvTimeoutError;

    let client_id = client_id_for_pid(pid);
    let session = match screenshot::build_session(cg_id, THUMBNAIL_MAX_DIM) {
        Ok(s) => s,
        Err(e) => {
            warn!("thumbnail session failed for window {cg_id} ({client_id}): {e}");
            return;
        }
    };
    loop {
        // recv_timeout doubles as the period ticker. On shutdown
        // (Ok(_)) or producer drop (Disconnected) we exit; on
        // timeout we capture the next frame.
        match shutdown_rx.recv_timeout(THUMBNAIL_PERIOD) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let frame = match screenshot::capture_with_session(&session) {
            Ok(f) => f,
            Err(e) => {
                warn!("thumbnail capture failed for window {cg_id} ({client_id}): {e}");
                continue;
            }
        };
        let compressed = x11_web_pixel_codec::encode_rgba_lossy(
            &frame.rgba,
            frame.width,
            frame.height,
            THUMBNAIL_QUALITY,
        );
        if tx
            .send(SidecarToBackend::DisplayUpdate {
                client_id: client_id.clone(),
                update: DisplayUpdate::WindowThumbnail {
                    window_id: uuid.clone(),
                    width: frame.width.min(u16::MAX as u32) as u16,
                    height: frame.height.min(u16::MAX as u32) as u16,
                    data: compressed,
                },
            })
            .is_err()
        {
            return;
        }
    }
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

fn emit_created(tx: &mpsc::UnboundedSender<SidecarToBackend>, uuid: &str, win: &WindowInfo) {
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

fn emit_title(tx: &mpsc::UnboundedSender<SidecarToBackend>, uuid: &str, title: &str) {
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
