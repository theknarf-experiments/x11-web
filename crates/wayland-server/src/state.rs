// Derived from waylandcraft — https://github.com/EVV1E/waylandcraft
// Upstream file:   native/src/lib.rs
// Upstream commit: 233d1431e6acbad1d0c47dfba44d971ce0cebfe8
// GPLv3 — see crates/wayland-server/NOTICE
//
// Changed from upstream: the EGL/dmabuf state and its handler are
// deleted wholesale (upstream's `init_dmabuf` calls
// `egl.get_render_node().expect(...)`, which panics outright in a
// container with no /dev/dri); so are the xwayland-satellite,
// .desktop-launcher and wl_data_device fields. `commit()` is no
// longer a no-op — it does the shm read-back and damage drain that
// upstream deferred to a JNI poll from the Minecraft render thread.
// The `WindowRequests` vectors, which existed so Java could poll for
// pending maximize/fullscreen/minimize requests, are gone: the
// handlers act synchronously. `ClientData` gained the client UUID and
// pid the backend's process bookkeeping needs.

use std::collections::HashMap;
use std::io;

use smithay::delegate_compositor;
use smithay::delegate_shm;
use smithay::delegate_single_pixel_buffer;
use smithay::delegate_viewporter;
use smithay::delegate_xdg_decoration;
use smithay::delegate_xdg_shell;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, ObjectId,
};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::viewporter::ViewporterState;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, trace};
use x11_web_protocol::{DisplayUpdate, WindowWmState};

use crate::output::WlcOutput;
use crate::router::WindowRouter;
use crate::seat::SeatState;
use crate::surface::{self, SurfaceBuffer};
use crate::translate::ModifierSynth;
use crate::windows::{self, WindowKind, WindowRegistry};
use crate::TaggedDisplayUpdate;

/// Per-client data hung off every accepted connection.
///
/// `CompositorClientState` is smithay's requirement; `client_id` and
/// `pid` are ours. They are minted at accept time from the socket's
/// peer credentials (see `server::compositor_thread`) because that is
/// the only moment the identity is available — Wayland has no
/// equivalent of X11's client-supplied identification, and by the
/// time a surface exists the connecting process may already have
/// forked away.
pub(crate) struct WaylandClientData {
    pub client_id: String,
    #[allow(dead_code)] // reported once, at accept, via client_connected_tx
    pub pid: u32,
    pub compositor_state: CompositorClientState,
}

impl ClientData for WaylandClientData {
    fn initialized(&self, _id: ClientId) {}
    fn disconnected(&self, _id: ClientId, _reason: DisconnectReason) {}
}

pub(crate) struct State {
    pub dh: DisplayHandle,
    pub compositor_state: CompositorState,
    pub shm_state: ShmState,
    pub xdg_state: XdgShellState,
    pub output: WlcOutput,
    pub seat: SeatState,
    /// Reconciles the frontend's X11 modifier mask against the modifier
    /// keys that actually arrived. Lives on `State` rather than on the
    /// seat because it is a property of the *embedder's* protocol, not
    /// of Wayland: a second input source would want its own.
    pub modifiers: ModifierSynth,

    /// Live pixel copies, keyed by `wl_surface` object id. Includes
    /// subsurfaces, which is why this is separate from `windows`.
    pub surfaces: HashMap<ObjectId, SurfaceBuffer>,
    pub windows: WindowRegistry,

    pub update_tx: UnboundedSender<TaggedDisplayUpdate>,
    pub client_connected_tx: UnboundedSender<(String, u32)>,
    pub router: WindowRouter,

    // These three are never read after construction, but must not be
    // dropped: each owns the `GlobalId` of the protocol global it
    // created, and the delegate macros resolve requests through the
    // type, not through the value. Dropping one would leave clients
    // holding a global whose backing state is gone.
    #[allow(dead_code)]
    viewporter_state: ViewporterState,
    #[allow(dead_code)]
    single_pixel_buffer_state: SinglePixelBufferState,
    #[allow(dead_code)]
    xdg_decoration_state: XdgDecorationState,
}

impl State {
    pub(crate) fn new(
        dh: DisplayHandle,
        update_tx: UnboundedSender<TaggedDisplayUpdate>,
        client_connected_tx: UnboundedSender<(String, u32)>,
        router: WindowRouter,
        screen_size: (u16, u16),
    ) -> io::Result<Self> {
        let compositor_state = CompositorState::new::<State>(&dh);
        // `vec![]` is the complete accepted format set, not an empty
        // one: wl_shm mandates ARGB8888 and XRGB8888, so they are
        // always advertised implicitly. See `pixels::ShmFormat`.
        let shm_state = ShmState::new::<State>(&dh, vec![]);
        let xdg_state = XdgShellState::new::<State>(&dh);
        let viewporter_state = ViewporterState::new::<State>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<State>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<State>(&dh);

        let output = WlcOutput::new(&dh, screen_size);
        output.create_global();

        // The seat compiles an xkb keymap, which is the one part of
        // building this state that can fail for an environmental
        // reason (no `xkb-data` in the image, a bad
        // `XKB_DEFAULT_LAYOUT`). It is also the only global whose
        // construction has to happen before it is advertised, since
        // the keymap fd goes out with the first `get_keyboard`.
        let seat = SeatState::new()?;
        seat.create_globals(&dh);

        Ok(Self {
            dh,
            compositor_state,
            shm_state,
            xdg_state,
            output,
            seat,
            modifiers: ModifierSynth::new(),
            surfaces: HashMap::new(),
            windows: WindowRegistry::default(),
            update_tx,
            client_connected_tx,
            router,
            viewporter_state,
            single_pixel_buffer_state,
            xdg_decoration_state,
        })
    }

    /// The UUID minted for the client that owns this surface.
    ///
    /// Empty string if the client has already gone — the update is
    /// still emitted (the backend tolerates an unknown tag) rather
    /// than silently dropped, so a teardown race shows up as an
    /// orphaned window rather than a window that never closes.
    fn client_id_of(surface: &WlSurface) -> String {
        surface
            .client()
            .and_then(|c| {
                c.get_data::<WaylandClientData>()
                    .map(|d| d.client_id.clone())
            })
            .unwrap_or_default()
    }
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<WaylandClientData>()
            .expect("every client is inserted with WaylandClientData")
            .compositor_state
    }

    fn commit(&mut self, wl_surface: &WlSurface) {
        let outcome = surface::commit_surface(&mut self.surfaces, wl_surface);

        // A subsurface commit still dirties the window that owns it,
        // so everything is attributed to the tree's root.
        let root = surface::root_surface(wl_surface);
        let root_id = root.id();
        let is_root = root_id == wl_surface.id();

        let Some(entry) = self.windows.entries.get_mut(&root_id) else {
            // A surface with no window role yet (or a plain
            // wl_surface a client never gave a role). Its pixels are
            // kept — if it later becomes a subsurface, the tick will
            // find them already there.
            return;
        };

        entry.dirty = true;
        if outcome.removed && is_root {
            // The one and only unmap signal Wayland has.
            entry.pending_unmap = true;
        }
        if outcome.resized && is_root {
            entry.damage.mark_full();
        }
    }

    fn destroyed(&mut self, wl_surface: &WlSurface) {
        let id = wl_surface.id();
        self.surfaces.remove(&id);
        // A client that vanishes mid-session destroys its surfaces
        // without ever sending xdg_toplevel.destroy, so the window
        // teardown has to hang off surface destruction too.
        // `windows::destroy` is idempotent.
        if self.windows.entries.contains_key(&id) {
            windows::destroy(self, &id);
        }
    }
}

impl BufferHandler for State {
    // Buffers are released the instant their pixels are copied out
    // (see `surface::commit_surface`), so by the time a client
    // destroys one we are already not referencing it.
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_state
    }

    fn new_toplevel(&mut self, toplevel: ToplevelSurface) {
        let wl_surface = toplevel.wl_surface().clone();
        let client_id = State::client_id_of(&wl_surface);
        self.windows
            .register(wl_surface.id(), client_id, WindowKind::Toplevel, 0, 0);

        // Empty initial configure: the protocol requires one before
        // the client may attach a buffer, and sending it with no size
        // is how a compositor says "pick your own". `WindowCreated`
        // waits for the buffer that follows — there is no size to
        // report until then.
        toplevel.send_configure();
    }

    fn new_popup(&mut self, popup: PopupSurface, positioner: PositionerState) {
        let geometry = positioner.get_geometry();
        popup.with_pending_state(|state| {
            state.geometry = geometry;
            state.positioner = positioner;
        });

        let wl_surface = popup.wl_surface().clone();
        let client_id = State::client_id_of(&wl_surface);
        // Parent-relative coordinates, which for a popup whose parent
        // is a toplevel are also absolute — every toplevel here sits
        // at (0, 0). Nested popups inherit that approximation; menus
        // land in roughly the right place, which is the bar for this
        // slice.
        self.windows.register(
            wl_surface.id(),
            client_id,
            WindowKind::Popup,
            geometry.loc.x as i16,
            geometry.loc.y as i16,
        );

        if let Err(e) = popup.send_configure() {
            debug!("popup initial configure failed: {e}");
        }
    }

    fn reposition_request(&mut self, popup: PopupSurface, positioner: PositionerState, token: u32) {
        let geometry = positioner.get_geometry();
        popup.with_pending_state(|state| {
            state.geometry = geometry;
            state.positioner = positioner;
        });
        if let Some(entry) = self.windows.entries.get_mut(&popup.wl_surface().id()) {
            entry.x = geometry.loc.x as i16;
            entry.y = geometry.loc.y as i16;
        }
        popup.send_repositioned(token);
    }

    /// Popup grabs are how menus get keyboard focus and
    /// dismiss-on-click-outside. Accepting one properly means keeping a
    /// grab *stack* (nested submenus) and rerouting every subsequent
    /// pointer and key event through it, which is a second focus model
    /// living alongside the browser-dictated one in `input.rs` — well
    /// outside this slice.
    ///
    /// Silently declining is the sanctioned degradation: the protocol
    /// lets a compositor ignore the grab, and the client keeps the
    /// popup up and dismisses it on its own logic. What is lost is
    /// keyboard navigation of menus and auto-dismiss on an outside
    /// click; the frontend's own click handling covers the latter.
    fn grab(&mut self, _popup: PopupSurface, _seat: WlSeat, _serial: Serial) {
        trace!("declining popup grab; this compositor has no grab stack");
    }

    fn maximize_request(&mut self, toplevel: ToplevelSurface) {
        let size = self.output.size();
        toplevel.with_pending_state(|state| {
            if state.states.contains(xdg_toplevel::State::Fullscreen) {
                return;
            }
            state.size = Some(size);
            state.states.set(xdg_toplevel::State::Maximized);
        });
        toplevel.send_pending_configure();
        let id = toplevel.wl_surface().id();
        windows::emit_state(self, &id, WindowWmState::Maximized);
    }

    fn unmaximize_request(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            // `None` hands sizing back to the client, which is what
            // "restore" means when the compositor never stored a
            // pre-maximize geometry (we don't: the frontend owns
            // window placement and size).
            state.size = None;
            state.states.unset(xdg_toplevel::State::Maximized);
        });
        toplevel.send_pending_configure();
        let id = toplevel.wl_surface().id();
        windows::emit_state(self, &id, WindowWmState::Normal);
    }

    fn fullscreen_request(&mut self, toplevel: ToplevelSurface, _output: Option<WlOutput>) {
        let size = self.output.size();
        toplevel.with_pending_state(|state| {
            state.size = Some(size);
            state.states.set(xdg_toplevel::State::Fullscreen);
        });
        toplevel.send_pending_configure();
        let id = toplevel.wl_surface().id();
        windows::emit_state(self, &id, WindowWmState::Fullscreen);
    }

    fn unfullscreen_request(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.size = None;
            state.states.unset(xdg_toplevel::State::Fullscreen);
        });
        toplevel.send_pending_configure();
        let id = toplevel.wl_surface().id();
        windows::emit_state(self, &id, WindowWmState::Normal);
    }

    /// Reported, not acted on. Unmapping the window here would leave
    /// no way to restore it: Wayland has no "un-minimize" request, so
    /// only the client can bring itself back, and it won't because it
    /// thinks it is still mapped. The frontend gets the state and can
    /// collapse the window frame itself.
    fn minimize_request(&mut self, toplevel: ToplevelSurface) {
        let id = toplevel.wl_surface().id();
        windows::emit_state(self, &id, WindowWmState::Minimized);
    }

    /// Interactive move/resize are compositor-side drags. The
    /// frontend does its own dragging against the canvas and pushes
    /// the result back as `ResizeWindow`, so these are dropped.
    fn move_request(&mut self, _toplevel: ToplevelSurface, _seat: WlSeat, _serial: Serial) {
        trace!("ignoring interactive move request; the frontend drags windows itself");
    }

    fn resize_request(
        &mut self,
        _toplevel: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
        trace!("ignoring interactive resize request; the frontend drives resizes");
    }

    fn title_changed(&mut self, toplevel: ToplevelSurface) {
        self.refresh_label(&toplevel);
    }

    fn app_id_changed(&mut self, toplevel: ToplevelSurface) {
        self.refresh_label(&toplevel);
    }

    fn toplevel_destroyed(&mut self, toplevel: ToplevelSurface) {
        let id = toplevel.wl_surface().id();
        windows::destroy(self, &id);
    }

    fn popup_destroyed(&mut self, popup: PopupSurface) {
        let id = popup.wl_surface().id();
        windows::destroy(self, &id);
    }
}

impl State {
    /// Push a title change through, falling back to the app_id — the
    /// frontend needs *some* label, and terminals in particular set
    /// an app_id long before they set a title.
    fn refresh_label(&mut self, toplevel: &ToplevelSurface) {
        let wl_surface = toplevel.wl_surface();
        let label = surface::toplevel_label(wl_surface).unwrap_or_default();
        let id = wl_surface.id();
        let Some(entry) = self.windows.entries.get_mut(&id) else {
            return;
        };
        if entry.title == label {
            return;
        }
        entry.title = label.clone();
        if !entry.created {
            // The map burst will carry it; sending now would be a
            // title for a window the frontend hasn't heard of.
            return;
        }
        let _ = self.update_tx.send((
            entry.client_id.clone(),
            DisplayUpdate::TitleChanged {
                window_id: entry.uuid.clone(),
                title: label,
            },
        ));
    }
}

delegate_compositor!(State);
delegate_shm!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_viewporter!(State);
delegate_single_pixel_buffer!(State);
