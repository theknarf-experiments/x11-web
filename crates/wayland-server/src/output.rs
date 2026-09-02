// Derived from waylandcraft — https://github.com/EVV1E/waylandcraft
// Upstream file:   native/src/output.rs
// Upstream commit: 233d1431e6acbad1d0c47dfba44d971ce0cebfe8
// GPLv3 — see crates/wayland-server/NOTICE
//
// Changed from upstream: the `bounds` concept (a second size used to
// distinguish "screen" from "usable content area" on Android) was
// dropped — there is no shell furniture here, so bounds would always
// equal size. Bound outputs are now tracked so `resize` can re-send
// the mode, and dead resources are reaped on every resize instead of
// accumulating for the process lifetime.

use smithay::reexports::wayland_server::{
    protocol::wl_output::{self, WlOutput},
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::utils::{Logical, Size};

use crate::state::State;

/// A single hand-rolled `wl_output` v4 global.
///
/// smithay ships `wayland::output::Output`, which is richer (xdg-output,
/// per-surface enter/leave, mode lists). We keep upstream's hand-rolled
/// version instead because this compositor has exactly one virtual
/// output whose only job is to tell clients how large the canvas is:
/// the smithay type would pull in `OutputManagerState` and a second set
/// of delegates for capabilities nothing here uses.
pub(crate) struct WlcOutput {
    /// Every `wl_output` a client has bound. Needed because a mode
    /// change has to be *pushed*: there is no request/response shape
    /// for it, the compositor re-sends `mode` + `done` to every
    /// binding or the client keeps believing the old size forever.
    outputs: Vec<WlOutput>,
    size: Size<i32, Logical>,
    display_handle: DisplayHandle,
}

impl WlcOutput {
    pub(crate) fn new(display_handle: &DisplayHandle, size: (u16, u16)) -> Self {
        Self {
            outputs: Vec::new(),
            size: Size::from((size.0 as i32, size.1 as i32)),
            display_handle: display_handle.clone(),
        }
    }

    /// Advertise the global. Version 4 so clients get `name` /
    /// `description`; toolkits that only understand v2 still work,
    /// since every event above their version is skipped below.
    pub(crate) fn create_global(&self) {
        self.display_handle
            .create_global::<State, WlOutput, ()>(4, ());
    }

    pub(crate) fn size(&self) -> Size<i32, Logical> {
        self.size
    }

    pub(crate) fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.size = Size::from((width, height));
        // Drop bindings whose client has gone away; `mode()` on a dead
        // resource is a no-op but the Vec would grow without bound.
        self.outputs.retain(|o| o.is_alive());
        for output in &self.outputs {
            output.mode(wl_output::Mode::Current, self.size.w, self.size.h, 0);
            if output.version() >= 2 {
                output.done();
            }
        }
    }
}

impl GlobalDispatch<WlOutput, ()> for State {
    fn bind(
        state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<WlOutput>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let output: WlOutput = data_init.init(resource, ());

        let size = state.output.size;
        // Refresh 0 = "unknown/variable". We are not scanning out to
        // real hardware; claiming a rate would just make clients
        // schedule against a lie. The render tick is the real clock.
        output.mode(wl_output::Mode::Current, size.w, size.h, 0);
        output.geometry(
            0,
            0,
            // Physical size 0x0 is the documented way to say "no
            // physical display"; toolkits fall back to scale for DPI.
            0,
            0,
            wl_output::Subpixel::None,
            "x11-web".into(),
            "Virtual Output".into(),
            wl_output::Transform::Normal,
        );

        if output.version() >= 4 {
            output.name("wayland-0".into());
            output.description("x11-web virtual output".into());
        }

        if output.version() >= 2 {
            // Scale 1 always. The compositor composites in buffer
            // pixels and reports window sizes in buffer pixels, so
            // advertising a HiDPI scale would make clients allocate
            // buffers we then report at the wrong logical size.
            output.scale(1);
            output.done();
        }

        state.output.outputs.push(output);
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        output: &WlOutput,
        request: wl_output::Request,
        _data: &(),
        _disp: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            // v3+ teardown. Upstream had `_ => unreachable!()` here;
            // a future protocol revision adding a request would then
            // abort the whole compositor, so this logs instead.
            wl_output::Request::Release => {
                state.output.outputs.retain(|o| o != output);
            }
            other => {
                tracing::warn!(?other, "unhandled wl_output request");
            }
        }
    }
}
