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
use std::sync::Arc;
use std::time::Duration;

use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{channel, EventLoop, Interest, Mode, PostAction};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::utils::Size;
use smithay::wayland::compositor::CompositorClientState;
use smithay::wayland::socket::ListeningSocketSource;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

use crate::input;
use crate::router::{Command, WindowRouter};
use crate::state::{State, WaylandClientData};
use crate::windows;
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
/// Four calloop sources, and that is the whole compositor:
///
///   1. the listening socket — accept, identify, `insert_client`;
///   2. the display fd — dispatch client requests;
///   3. the embedder's command channel — input, resize, screen size;
///   4. a `Timer` — the render tick.
///
/// Everything `!Send` (the `Display`, the `EventLoop`, the whole
/// `State`) is constructed here and never leaves.
#[allow(clippy::too_many_arguments)]
fn compositor_thread(
    ready_tx: std::sync::mpsc::SyncSender<io::Result<String>>,
    xdg_runtime_dir: PathBuf,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    window_router: WindowRouter,
    screen_size: (u16, u16),
    frame_interval: Duration,
) {
    // Anything that fails before the socket is bound has to travel
    // back out through `ready_tx`, or `new()` blocks forever.
    macro_rules! bail {
        ($ctx:expr, $err:expr) => {{
            let _ = ready_tx.send(Err(io::Error::new(
                io::ErrorKind::Other,
                format!("{}: {}", $ctx, $err),
            )));
            return;
        }};
    }

    // `ListeningSocket::bind_auto` resolves the name against
    // $XDG_RUNTIME_DIR, so this thread's view of the variable has to
    // match the directory `new()` created and the one children are
    // told about. Setting it here rather than relying on the ambient
    // environment is what makes the library correct when embedded by
    // a process that never set it.
    //
    // SAFETY: `set_var` is only unsound when another thread reads the
    // environment concurrently. This runs before any client exists
    // and before the sidecar spawns anything.
    std::env::set_var("XDG_RUNTIME_DIR", &xdg_runtime_dir);

    let mut event_loop: EventLoop<State> = match EventLoop::try_new() {
        Ok(l) => l,
        Err(e) => bail!("calloop event loop", e),
    };
    let display: Display<State> = match Display::new() {
        Ok(d) => d,
        Err(e) => bail!("wayland display", e),
    };
    let dh = display.handle();

    let socket = match ListeningSocketSource::new_auto() {
        Ok(s) => s,
        Err(e) => bail!("binding wayland socket", e),
    };
    let socket_name = socket.socket_name().to_string_lossy().into_owned();

    let mut state = match State::new(
        dh.clone(),
        update_tx,
        client_connected_tx,
        window_router.clone(),
        screen_size,
    ) {
        Ok(s) => s,
        Err(e) => bail!("building compositor state", e),
    };

    let handle = event_loop.handle();

    // --- 1. accept ------------------------------------------------
    //
    // Peer credentials are read off the accepted stream *before* the
    // client is inserted: this is the only point where the connecting
    // process is unambiguously identified. It is the Wayland
    // equivalent of the X11 sidecar reading the socket's peer pid,
    // and it is what feeds the backend's process list.
    if let Err(e) = handle.insert_source(socket, move |stream, _, state: &mut State| {
        let pid = peer_pid(&stream);
        let client_id = uuid::Uuid::new_v4().to_string();

        let data = Arc::new(WaylandClientData {
            client_id: client_id.clone(),
            pid,
            compositor_state: CompositorClientState::default(),
        });

        match state.dh.insert_client(stream, data) {
            Ok(_) => {
                info!(%client_id, pid, "wayland client connected");
                let _ = state.client_connected_tx.send((client_id, pid));
            }
            Err(e) => warn!("failed to insert wayland client: {e}"),
        }
    }) {
        bail!("inserting socket source", e);
    }

    // --- 2. client requests ---------------------------------------
    let display_source = Generic::new(display, Interest::READ, Mode::Level);
    if let Err(e) = handle.insert_source(display_source, |_, display, state: &mut State| {
        // SAFETY: the `Display` is owned by this source for the
        // lifetime of the event loop and is never moved out of it,
        // which is exactly the invariant `dispatch_clients` requires.
        unsafe {
            if let Err(e) = display.get_mut().dispatch_clients(state) {
                warn!("dispatch_clients failed: {e}");
            }
        }
        Ok(PostAction::Continue)
    }) {
        bail!("inserting display source", e);
    }

    // --- 3. embedder commands -------------------------------------
    let (cmd_tx, cmd_rx) = channel::channel::<Command>();
    if let Err(e) = handle.insert_source(cmd_rx, |event, _, state: &mut State| {
        if let channel::Event::Msg(cmd) = event {
            apply_command(state, cmd);
        }
    }) {
        bail!("inserting command channel", e);
    }
    // Only now can `WindowRouter::send_*` succeed. Everything the
    // embedder sent before this returns `false`, which is the same
    // answer the X11 router gives for a window it has no route for.
    window_router.install(cmd_tx);

    // --- 4. render tick -------------------------------------------
    //
    // A fixed-period timer rather than "render when a client
    // commits": the tick is also what releases frame callbacks, so it
    // has to fire on a schedule the clients cannot influence.
    if let Err(e) = handle.insert_source(
        Timer::from_duration(frame_interval),
        move |_, _, state: &mut State| {
            windows::tick(state);
            TimeoutAction::ToDuration(frame_interval)
        },
    ) {
        bail!("inserting render timer", e);
    }

    // Exactly once, and load-bearing: `kb_active` starts false in the
    // ported seat, and while it is false `keyboard_key` returns before
    // it touches the wire. Every stage of the pipeline — router,
    // command channel, focus, xkb — would keep working and no key
    // would ever reach a client, with nothing logged anywhere.
    state.seat.activate_keyboard();

    if ready_tx.send(Ok(socket_name.clone())).is_err() {
        // `new()` gave up on us (it can only do that by panicking),
        // so there is nobody to serve.
        return;
    }

    info!(socket = %socket_name, "wayland compositor event loop running");

    // The `Some(frame_interval)` timeout bounds how long the loop can
    // sit in poll(), which matters because `flush_clients` only runs
    // between dispatches — without it, a client waiting on a frame
    // callback could stall behind an idle poll.
    let result = event_loop.run(Some(frame_interval), &mut state, |state| {
        state.dh.flush_clients().ok();
    });

    match result {
        Ok(()) => info!("wayland compositor event loop exited"),
        Err(e) => error!("wayland compositor event loop failed: {e}"),
    }
}

/// Read the connecting process's pid off an accepted socket.
///
/// `UnixStream::peer_cred` is still unstable, and
/// `Client::get_credentials` is only usable *after* `insert_client` —
/// but the pid has to go into the client's data at construction time.
/// So this is the raw `SO_PEERCRED` getsockopt, which is what both of
/// those wrap anyway.
///
/// A pid of 0 means "unknown" and is what the backend sees for a
/// client whose credentials could not be read (never observed in
/// practice on Linux, but a failed getsockopt must not take the
/// compositor down over a bookkeeping field).
fn peer_pid(stream: &std::os::unix::net::UnixStream) -> u32 {
    use std::os::fd::AsRawFd;

    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` is a live, correctly-sized `ucred` and `len`
    // describes it; `fd` is owned by `stream` for the duration of the
    // call. getsockopt writes at most `len` bytes into `cred`.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 || cred.pid <= 0 {
        warn!("could not read peer credentials for wayland client");
        return 0;
    }
    cred.pid as u32
}

/// Apply one embedder command inside the compositor thread.
fn apply_command(state: &mut State, cmd: Command) {
    match cmd {
        Command::Resize {
            window_id,
            width,
            height,
        } => {
            let Some(id) = state.windows.id_for_uuid(&window_id) else {
                return;
            };
            let Some(toplevel) = state
                .xdg_state
                .toplevel_surfaces()
                .iter()
                .find(|t| t.wl_surface().id() == id)
                .cloned()
            else {
                return;
            };
            // A configure is a *request*: the client acks it and then
            // commits a buffer at whatever size it settled on. The
            // window's reported size only changes when that buffer
            // arrives, which is why nothing is emitted here.
            toplevel.with_pending_state(|s| {
                s.size = Some(Size::from((width as i32, height as i32)));
                s.states.unset(xdg_toplevel::State::Maximized);
                s.states.unset(xdg_toplevel::State::Fullscreen);
            });
            toplevel.send_pending_configure();
        }
        Command::ScreenSize { width, height } => {
            state.output.resize(width as i32, height as i32);
        }
        Command::Input { window_id, event } => {
            input::apply(state, &window_id, event);
        }
    }
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
