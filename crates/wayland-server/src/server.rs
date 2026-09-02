//! The tokio ↔ calloop boundary.
//!
//! smithay's `Display` and `EventLoop` are `!Send`; tokio owns the
//! process. Rather than fight that, the compositor gets its own OS
//! thread and *all* smithay state is constructed inside it — nothing
//! `!Send` is ever moved across a thread boundary, which is what
//! makes `tokio::spawn(server.run())` compile.
//!
//! The two threads meet in three places, all of them `Send`:
//!
//!   * a `std::sync::mpsc::sync_channel` handshake carrying the bound
//!     socket name (or the bind error) back out of the thread, so
//!     `new()` can be fallible and callers know `WAYLAND_DISPLAY`
//!     before they spawn any child process;
//!   * `tokio::sync::mpsc::UnboundedSender`s going out (non-async
//!     `send`, callable straight from a calloop callback);
//!   * a `calloop::channel` going in, whose sender lives in
//!     [`WindowRouter`](crate::WindowRouter).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::router::WindowRouter;
use crate::TaggedDisplayUpdate;

pub struct WaylandServer {
    display_name: String,
    xdg_runtime_dir: PathBuf,
    thread: std::thread::JoinHandle<()>,
    exited_rx: oneshot::Receiver<()>,
}

impl WaylandServer {
    /// Ensures `XDG_RUNTIME_DIR` exists (mode 0700), starts the
    /// compositor thread, and blocks until the Wayland socket is
    /// bound — or returns the bind error.
    ///
    /// Binding eagerly (rather than inside `run()`, as `X11Server`
    /// does) is deliberate: unlike X11, where we pick the display
    /// number ourselves, the socket name here is chosen by
    /// `ListeningSocketSource::new_auto` from `wayland-1..wayland-32`.
    /// The embedder has to know the name before it can put
    /// `WAYLAND_DISPLAY` into a child's environment, and a child
    /// spawned against a socket that doesn't exist yet just fails.
    ///
    /// `screen_size` seeds the wl_output mode; change it later via
    /// [`WindowRouter::set_screen_size`]. `frame_interval` is the
    /// render tick period (16 ms ≈ 60 Hz) — it is also what drives
    /// `wl_surface.frame` callback delivery, without which toolkit
    /// clients paint exactly one frame and then block forever.
    pub fn new(
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
        window_router: WindowRouter,
        screen_size: (u16, u16),
        frame_interval: Duration,
    ) -> io::Result<Self> {
        let xdg_runtime_dir = ensure_xdg_runtime_dir()?;

        // Handshake back out of the compositor thread. Capacity 1:
        // the thread sends exactly one message (bound name, or the
        // error that stopped it) and then never touches this again.
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<io::Result<String>>(1);
        let (exited_tx, exited_rx) = oneshot::channel::<()>();

        let thread_runtime_dir = xdg_runtime_dir.clone();
        let thread = std::thread::Builder::new()
            .name("wayland-compositor".into())
            .spawn(move || {
                compositor_thread(
                    ready_tx,
                    thread_runtime_dir,
                    update_tx,
                    client_connected_tx,
                    window_router,
                    screen_size,
                    frame_interval,
                );
                // Fires whether the loop returned cleanly or the
                // thread is unwinding, so `run()` always resolves.
                let _ = exited_tx.send(());
            })?;

        // A `RecvError` here means the thread panicked before it got
        // as far as reporting — surface that as an io error rather
        // than hanging the caller.
        let display_name = ready_rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "wayland compositor thread exited before binding a socket",
            )
        })??;

        info!(
            "Wayland compositor listening on {}/{display_name}",
            xdg_runtime_dir.display()
        );

        Ok(Self {
            display_name,
            xdg_runtime_dir,
            thread,
            exited_rx,
        })
    }

    /// The value to put in a child process's `WAYLAND_DISPLAY`, e.g.
    /// `"wayland-1"`.
    pub fn wayland_display_name(&self) -> &str {
        &self.display_name
    }

    /// The value to put in a child process's `XDG_RUNTIME_DIR`.
    pub fn xdg_runtime_dir(&self) -> &Path {
        &self.xdg_runtime_dir
    }

    /// Absolute path of the listening socket. Handy for readiness
    /// probes (`ls $XDG_RUNTIME_DIR/wayland-*`).
    pub fn socket_path(&self) -> PathBuf {
        self.xdg_runtime_dir.join(&self.display_name)
    }

    /// Resolves when the compositor thread exits.
    ///
    /// The compositor is a blocking calloop loop, but this is `async`
    /// on purpose: it lets `crates/sidecar-wayland` read exactly like
    /// `crates/sidecar` —
    /// `tokio::spawn(async move { if let Err(e) = server.run().await { … } })`.
    pub async fn run(self) -> io::Result<()> {
        let _ = self.exited_rx.await;
        match self.thread.join() {
            Ok(()) => Ok(()),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "wayland compositor thread panicked",
            )),
        }
    }
}

/// Body of the `"wayland-compositor"` thread: build the smithay
/// state, bind the socket, report the bound name back through
/// `ready_tx`, then run the calloop event loop until it stops.
///
/// STAGE: Compositor — everything below the signature. It must:
///   1. create `Display<State>` + `EventLoop`;
///   2. insert `ListeningSocketSource::new_auto()` (on accept: read
///      the peer's pid off the stream credentials, mint a client-id
///      UUID, `insert_client`, then push `(client_id, pid)` into
///      `client_connected_tx` — this is the `ProcessConnected`
///      plumbing);
///   3. insert `Generic(display, READ, Level)` → `dispatch_clients`;
///   4. insert the `calloop::channel` receiver whose sender it hands
///      to `window_router.install(...)`;
///   5. insert a `Timer` re-armed every `frame_interval` — the render
///      tick that emits `PutImage` into `update_tx` and *then* drains
///      `frame_callbacks`;
///   6. call `seat.activate_keyboard()` once (it defaults to false
///      and silently swallows every key otherwise);
///   7. send `Ok(socket_name)` (or `Err`) on `ready_tx` and run
///      `event_loop.run(Some(frame_interval), &mut state, |st| st.dh.flush_clients())`.
#[allow(clippy::too_many_arguments, unused_variables)]
fn compositor_thread(
    ready_tx: std::sync::mpsc::SyncSender<io::Result<String>>,
    xdg_runtime_dir: PathBuf,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    window_router: WindowRouter,
    screen_size: (u16, u16),
    frame_interval: Duration,
) {
    unimplemented!("STAGE: Compositor — smithay state, calloop sources, event loop");
}

/// Resolve and create `XDG_RUNTIME_DIR`.
///
/// `debian:bookworm-slim` has no `/run/user/0`, and
/// `ListeningSocket::bind` fails outright without it — which would
/// kill the compositor before any client ever connects, with the only
/// symptom being a sidecar that exits at startup. The entrypoint
/// script does this too; doing it here as well costs one `mkdir` and
/// covers the case where the library is embedded by something else.
///
/// The 0700 mode is not cosmetic: libwayland's client-side
/// `wl_display_connect` warns loudly about a group/world-accessible
/// runtime dir, and some toolkits refuse it.
fn ensure_xdg_runtime_dir() -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        // SAFETY: geteuid() is always safe — it takes no arguments,
        // touches no memory, and cannot fail.
        _ => PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })),
    };

    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
