//! `xdg_decoration` — always server-side, never drawn.
//!
//! Written from scratch (upstream waylandcraft has no decoration
//! support at all), so no GPLv3 header here.
//!
//! The frontend draws the titlebar, the close button and the resize
//! handles itself, from the workspace document — exactly as it does
//! for X11 windows. So the correct answer to a client asking about
//! decorations is "server side": the client draws *nothing*, we draw
//! nothing either, and the window's pixels are pure content. Letting
//! clients fall back to CSD instead would give every GTK window a
//! second titlebar inside the frontend's titlebar, plus an invisible
//! shadow margin that inflates the window geometry.
//!
//! This is one global with three trivial handlers; if it ever starts
//! costing more than it delivers, deleting the module is a safe
//! retreat (clients revert to CSD, which is ugly but works).

use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::state::State;

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        force_server_side(&toplevel);
    }

    /// The client's *preference* is advisory; the protocol explicitly
    /// permits the compositor to answer with a different mode, and a
    /// conforming client must honour whatever configure it gets.
    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: Mode) {
        force_server_side(&toplevel);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        force_server_side(&toplevel);
    }
}

fn force_server_side(toplevel: &ToplevelSurface) {
    toplevel.with_pending_state(|state| {
        state.decoration_mode = Some(Mode::ServerSide);
    });
    // `send_pending_configure` rather than `send_configure`: before
    // the initial configure has been acked, an extra unconditional
    // configure makes some clients (GTK in particular) re-enter their
    // map sequence. The pending variant is a no-op when nothing
    // actually changed.
    toplevel.send_pending_configure();
}
