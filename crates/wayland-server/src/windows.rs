//! Window registry, lifecycle synthesis and the render tick.
//!
//! Written from scratch — upstream waylandcraft has no equivalent
//! (its "window manager" is Java code on the other side of the JNI
//! bridge, driven by Minecraft's render loop), so there is no GPLv3
//! header on this file.
//!
//! ## Why lifecycle events have to be *synthesised*
//!
//! The frontend speaks an X11-shaped protocol: a window is Created
//! with a geometry, Mapped, Configured, Unmapped, Destroyed. Wayland
//! has none of those events. An `xdg_toplevel` exists the moment the
//! client asks for one, has no size until it has negotiated one, and
//! is "mapped" by the pure convention that it has committed a
//! non-null buffer. So:
//!
//!   * `WindowCreated` is deferred to the **first buffer commit** —
//!     that is the first instant a size exists. Emitting it from
//!     `new_toplevel` would report `0x0`, and the frontend would size
//!     its back buffer from that.
//!   * `WindowMapped` fires at the same instant, because the backend
//!     refuses to list a window that never got one.
//!   * `WindowUnmapped` comes from a NULL buffer attach, Wayland's
//!     only unmap signal.
//!
//! ## Why one framebuffer per window
//!
//! Subsurfaces commit independently and at their own rate, so "the
//! window's pixels" only exist once someone composites the tree.
//! Keeping a persistent RGBA framebuffer per window means a
//! subsurface repaint costs one small blit plus one small `PutImage`
//! instead of a full-window readback, and it gives the damage bbox
//! something to be relative to.

use std::collections::HashMap;

use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::{with_surface_tree_upward, TraversalAction};
use smithay::wayland::shell::xdg::XdgShellState;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, trace};
use x11_web_protocol::{DisplayUpdate, WindowWmState};

use crate::pixels::{self, DamageAccumulator, Rect, BPP};
use crate::seat::SeatState;
use crate::state::State;
use crate::surface;
use crate::TaggedDisplayUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowKind {
    Toplevel,
    Popup,
}

impl WindowKind {
    /// How the frontend's X11-shaped flags describe this window.
    /// A popup is the Wayland analogue of an override-redirect X11
    /// window: positioned by the client, not managed, not in the
    /// window list as a first-class frame.
    fn flags(self) -> (bool, bool) {
        match self {
            // (is_top_level, override_redirect)
            WindowKind::Toplevel => (true, false),
            WindowKind::Popup => (false, true),
        }
    }
}

pub(crate) struct WindowEntry {
    pub uuid: String,
    /// UUID of the Wayland client that owns the window — the tag on
    /// every `TaggedDisplayUpdate`, and what the backend keys its
    /// per-process bookkeeping on.
    pub client_id: String,
    pub kind: WindowKind,
    /// Always `(0, 0)` for toplevels: Wayland gives the compositor no
    /// client-supplied position, and the frontend places top-level
    /// windows from the workspace document anyway. Popups carry the
    /// position their `xdg_positioner` asked for.
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub created: bool,
    pub mapped: bool,
    pub title: String,
    /// Persistent RGBA framebuffer, `width * height * 4`.
    pub fb: Vec<u8>,
    pub damage: DamageAccumulator,
    /// A commit landed somewhere in this window's surface tree since
    /// the last tick. Gates the composite so an idle window costs
    /// nothing per frame.
    pub dirty: bool,
    pub pending_unmap: bool,
    pub wm_state: WindowWmState,
}

#[derive(Default)]
pub(crate) struct WindowRegistry {
    /// Keyed by the **root** `wl_surface`'s object id — the toplevel's
    /// or popup's own surface. Subsurfaces are looked up by walking to
    /// their root, never registered here.
    pub entries: HashMap<ObjectId, WindowEntry>,
    pub focused: Option<ObjectId>,
}

impl WindowRegistry {
    pub fn register(
        &mut self,
        root: ObjectId,
        client_id: String,
        kind: WindowKind,
        x: i16,
        y: i16,
    ) {
        let uuid = uuid::Uuid::new_v4().to_string();
        debug!(%uuid, ?kind, "registering wayland window");
        self.entries.insert(
            root,
            WindowEntry {
                uuid,
                client_id,
                kind,
                x,
                y,
                width: 0,
                height: 0,
                created: false,
                mapped: false,
                title: String::new(),
                fb: Vec::new(),
                damage: DamageAccumulator::new(),
                dirty: false,
                pending_unmap: false,
                wm_state: WindowWmState::Normal,
            },
        );
    }

    pub fn id_for_uuid(&self, uuid: &str) -> Option<ObjectId> {
        self.entries
            .iter()
            .find(|(_, e)| e.uuid == uuid)
            .map(|(id, _)| id.clone())
    }
}

fn send(tx: &UnboundedSender<TaggedDisplayUpdate>, client_id: &str, update: DisplayUpdate) {
    // A closed channel means the embedder is shutting down. There is
    // nothing useful to do about it from inside a calloop callback,
    // and logging every dropped update would drown the log.
    let _ = tx.send((client_id.to_string(), update));
}

/// The render tick, driven by a calloop `Timer` at `frame_interval`.
///
/// Three jobs, in this order, and the order matters:
///   1. composite each dirty window's surface tree into its
///      framebuffer and emit **at most one** `PutImage` for it;
///   2. drain every surface's frame callbacks;
///   3. (the caller) flush the display.
///
/// Emitting before releasing the frame callbacks is what makes the
/// tick a throttle: a client cannot start the next frame until we
/// have finished shipping the last one, so a busy client renders at
/// the tick rate rather than at whatever rate it can memcpy, and the
/// unbounded update channel cannot be flooded.
pub(crate) fn tick(state: &mut State) {
    // Destructured rather than accessed through `&mut state`, so the
    // borrow checker can see that the window map, the surface map and
    // the update channel are disjoint. Going through methods on
    // `State` would make all three one borrow.
    let State {
        xdg_state,
        surfaces,
        windows,
        update_tx,
        router,
        seat,
        ..
    } = state;

    // `to_vec()` releases the borrow on `xdg_state` immediately —
    // the loop below needs `&mut` access to state that lives beside
    // it, and a `ToplevelSurface` is a cheap refcounted handle.
    let roots: Vec<WlSurface> = xdg_state
        .toplevel_surfaces()
        .iter()
        .map(|t| t.wl_surface().clone())
        .chain(
            xdg_state
                .popup_surfaces()
                .iter()
                .map(|p| p.wl_surface().clone()),
        )
        .collect();

    for root in &roots {
        let id = root.id();
        let Some(entry) = windows.entries.get_mut(&id) else {
            continue;
        };

        if entry.pending_unmap {
            entry.pending_unmap = false;
            if entry.mapped {
                entry.mapped = false;
                entry.damage = DamageAccumulator::new();
                entry.fb = Vec::new();
                // Zero the size along with the framebuffer. They are
                // one piece of state: if a client unmaps and remaps
                // at the same size, a stale `width`/`height` makes
                // the remap look like "no resize", the framebuffer is
                // never reallocated, and every subsequent crop comes
                // up short and is silently dropped — a window that
                // reappears frozen on its last frame.
                entry.width = 0;
                entry.height = 0;
                send(
                    update_tx,
                    &entry.client_id,
                    DisplayUpdate::WindowUnmapped {
                        window_id: entry.uuid.clone(),
                    },
                );
            }
            continue;
        }

        // No buffer on the root surface yet => the window has been
        // created but never painted. Nothing to composite, and
        // crucially nothing to announce: see the module docs.
        let Some(root_buf) = surfaces.get(&id) else {
            continue;
        };
        let (root_w, root_h) = root_buf.logical_size();

        // Everything below is in *window* coordinates, i.e. surface
        // coordinates minus this rectangle's origin.
        let geo = window_rect(root, root_w, root_h);
        let (w, h) = (
            geo.w.clamp(1, u16::MAX as i32),
            geo.h.clamp(1, u16::MAX as i32),
        );
        let (uw, uh) = (w as u16, h as u16);

        let resized = entry.width != uw || entry.height != uh;
        if resized {
            entry.width = uw;
            entry.height = uh;
            // Opaque black rather than transparent: a window whose
            // subsurfaces don't cover the whole geometry should read
            // as a window, not as a hole in the canvas.
            entry.fb = vec![0, 0, 0, 255].repeat((w * h) as usize);
            entry.damage.mark_full();
        }

        if !entry.created {
            entry.created = true;
            entry.mapped = true;
            entry.title = surface::toplevel_label(root).unwrap_or_default();
            let is_toplevel = entry.kind == WindowKind::Toplevel;
            emit_map(update_tx, entry);
            // Only now is the window addressable from the embedder.
            // Tracking earlier would let input reach a window with no
            // size and no pixels.
            router.track(&entry.uuid);
            // Focus the newly-mapped toplevel. Popups are skipped:
            // `apply_focus` resolves its argument against the toplevel
            // list, so a popup id would resolve to no surface and
            // unfocus the keyboard — a menu opening would take the
            // keyboard away from the application that opened it.
            if is_toplevel {
                apply_focus(xdg_state, windows, seat, update_tx, Some(id.clone()));
            }
            // `apply_focus` took a &mut borrow of the registry, so
            // re-acquire the entry.
            let Some(entry) = windows.entries.get_mut(&id) else {
                continue;
            };
            composite_and_emit(entry, surfaces, root, geo, update_tx);
            continue;
        }

        if !entry.mapped {
            entry.mapped = true;
            let (is_top_level, override_redirect) = entry.kind.flags();
            send(
                update_tx,
                &entry.client_id,
                DisplayUpdate::WindowMapped {
                    window_id: entry.uuid.clone(),
                    is_top_level,
                    override_redirect,
                },
            );
            entry.damage.mark_full();
        }

        if resized {
            send(
                update_tx,
                &entry.client_id,
                DisplayUpdate::WindowConfigured {
                    window_id: entry.uuid.clone(),
                    x: entry.x,
                    y: entry.y,
                    width: uw,
                    height: uh,
                    border_width: 0,
                    border_pixel: 0,
                    resizable: true,
                },
            );
        }

        composite_and_emit(entry, surfaces, root, geo, update_tx);
    }

    // Frame callbacks last, and for every window whether or not it
    // was dirty — a client that committed with no damage is still
    // waiting on its callback, and a window we skipped compositing
    // must not be starved into a permanent stall.
    for root in &roots {
        send_tree_frame_callbacks(root);
    }
}

/// Composite one window's surface tree into its framebuffer and emit
/// the accumulated damage as a single `PutImage`.
fn composite_and_emit(
    entry: &mut WindowEntry,
    surfaces: &mut HashMap<ObjectId, surface::SurfaceBuffer>,
    root: &WlSurface,
    geo: Rect,
    update_tx: &UnboundedSender<TaggedDisplayUpdate>,
) {
    if !entry.dirty && !entry.damage.is_dirty() {
        return;
    }
    entry.dirty = false;

    // Collect the tree first, blit second. The traversal closure
    // cannot hold a borrow of `surfaces` (it needs `&mut` for the
    // per-surface damage drain), and the visit order — parents before
    // children, which is exactly back-to-front stacking order for
    // subsurfaces — is all we need out of it.
    let mut visited: Vec<(ObjectId, i32, i32)> = Vec::new();
    with_surface_tree_upward(
        root,
        (0i32, 0i32),
        |_s, data, parent_off| {
            let (dx, dy) = surface::subsurface_offset(data);
            TraversalAction::DoChildren((parent_off.0 + dx, parent_off.1 + dy))
        },
        |s, data, parent_off| {
            // Recomputed rather than taken from the filter's return
            // value: `with_surface_tree_upward` hands the *processor*
            // the value produced for the parent, not the one the
            // filter just computed for this surface.
            let (dx, dy) = surface::subsurface_offset(data);
            visited.push((s.id(), parent_off.0 + dx, parent_off.1 + dy));
        },
        |_s, _data, _off| true,
    );

    let root_id = root.id();
    let (w, h) = (entry.width as i32, entry.height as i32);
    for (id, ox, oy) in visited.iter() {
        let Some(sb) = surfaces.get_mut(id) else {
            continue;
        };
        // Window-space position of this surface's top-left corner.
        let (px, py) = (ox - geo.x, oy - geo.y);

        if let Some(d) = sb.damage.take(sb.width, sb.height) {
            entry.damage.add(d.translate(px, py));
        }

        // Identified by object id, not by traversal position: a
        // subsurface can be placed *below* its parent
        // (`wl_subsurface.place_below`), in which case the root is not
        // the first surface the walk visits.
        if *id == root_id {
            // The root surface *is* the window background, so its
            // alpha lands verbatim — compositing it over the cleared
            // framebuffer would make translucent windows milky.
            pixels::blit_copy(&mut entry.fb, w, h, &sb.rgba, sb.width, sb.height, px, py);
        } else {
            pixels::blit_over(&mut entry.fb, w, h, &sb.rgba, sb.width, sb.height, px, py);
        }
    }

    let Some(rect) = entry.damage.take(w, h) else {
        return;
    };
    let data = pixels::crop_rgba(&entry.fb, w, h, rect);
    if data.len() != rect.w as usize * rect.h as usize * BPP {
        trace!(uuid = %entry.uuid, "skipping short PutImage crop");
        return;
    }
    // The one observable trace of the pixel path. With no backend
    // attached the updates just queue in the channel, so this line is
    // the only way a container smoke test (e2e/scripts/wayland-smoke.sh)
    // can prove pixels were produced rather than merely that a window
    // mapped. `trace!` because it fires up to 60x/s per window.
    trace!(
        uuid = %entry.uuid,
        x = rect.x, y = rect.y, w = rect.w, h = rect.h,
        "emitting PutImage",
    );
    send(
        update_tx,
        &entry.client_id,
        DisplayUpdate::PutImage {
            window_id: entry.uuid.clone(),
            x: rect.x as i16,
            y: rect.y as i16,
            width: rect.w as u16,
            height: rect.h as u16,
            data,
        },
    );
}

/// The burst of updates that stands in for X11's map sequence. Order
/// matters to the frontend: it needs a window to exist before it can
/// be mapped, and to be mapped before a title or pixels mean anything.
fn emit_map(tx: &UnboundedSender<TaggedDisplayUpdate>, entry: &WindowEntry) {
    let (is_top_level, override_redirect) = entry.kind.flags();
    send(
        tx,
        &entry.client_id,
        DisplayUpdate::WindowCreated {
            window_id: entry.uuid.clone(),
            x: entry.x,
            y: entry.y,
            width: entry.width,
            height: entry.height,
            is_top_level,
            override_redirect,
            border_width: 0,
            border_pixel: 0,
            // Wayland toplevels are always resizable from the
            // compositor's side: we can send any configure we like and
            // the client either honours it or clamps it to its own
            // min/max hints. Matches how the X11 sidecar reports.
            resizable: true,
        },
    );
    send(
        tx,
        &entry.client_id,
        DisplayUpdate::WindowMapped {
            window_id: entry.uuid.clone(),
            is_top_level,
            override_redirect,
        },
    );
    if !entry.title.is_empty() {
        send(
            tx,
            &entry.client_id,
            DisplayUpdate::TitleChanged {
                window_id: entry.uuid.clone(),
                title: entry.title.clone(),
            },
        );
    }
}

/// The rectangle of the root surface that *is* the window.
///
/// `xdg_surface.set_window_geometry` when the client set one — GTK and
/// Qt allocate a larger buffer and put their drop shadow in the margin,
/// then point geometry at the inner rectangle — else the whole buffer.
/// Shared with `input::hit_test`, which has to subtract the same origin
/// or every click in a CSD window lands offset by the shadow width.
pub(crate) fn window_rect(root: &WlSurface, root_w: i32, root_h: i32) -> Rect {
    surface::xdg_window_geometry(root)
        .and_then(|g| g.clip(root_w, root_h))
        .unwrap_or(Rect::new(0, 0, root_w, root_h))
}

/// Move keyboard focus to `id` (or clear it), applying the exact
/// recipe upstream uses.
///
/// The `Activated` flip is not cosmetic and not optional: GTK and Qt
/// only draw focused chrome, run their key handlers and show a text
/// cursor when their toplevel is in the `activated` state. Every
/// toplevel has to be un-activated first — a client left activated
/// keeps behaving as if it has focus.
///
/// And the `Activated` flip is only *half* of focus. Without the seat
/// half below, clients look focused and every key event still goes
/// nowhere, which is a uniquely unhelpful failure: the window has a
/// blinking cursor and ignores the keyboard.
pub(crate) fn set_focus(state: &mut State, id: Option<ObjectId>) {
    let State {
        xdg_state,
        windows,
        seat,
        update_tx,
        ..
    } = state;
    apply_focus(xdg_state, windows, seat, update_tx, id);
}

/// [`set_focus`] against individually-borrowed fields.
///
/// It exists in this shape because the render tick focuses a
/// newly-mapped window from inside a loop that has already destructured
/// `State` — going back through `&mut State` there would collide with
/// the borrows of the surface map and the update channel that the same
/// loop is holding.
fn apply_focus(
    xdg_state: &XdgShellState,
    windows: &mut WindowRegistry,
    seat: &mut SeatState,
    update_tx: &UnboundedSender<TaggedDisplayUpdate>,
    id: Option<ObjectId>,
) {
    if windows.focused == id {
        return;
    }
    windows.focused = id.clone();

    let focused_surface: Option<WlSurface> = xdg_state
        .toplevel_surfaces()
        .iter()
        .find(|t| Some(t.wl_surface().id()) == id)
        .map(|t| t.wl_surface().clone());

    for toplevel in xdg_state.toplevel_surfaces() {
        let active = Some(toplevel.wl_surface().id()) == id;
        toplevel.with_pending_state(|s| {
            if active {
                s.states.set(xdg_toplevel::State::Activated);
            } else {
                s.states.unset(xdg_toplevel::State::Activated);
            }
        });
        toplevel.send_pending_configure();
    }

    match &focused_surface {
        Some(s) => seat.keyboard_focus(s),
        None => seat.keyboard_unfocus(),
    }

    match id.as_ref().and_then(|i| windows.entries.get(i)) {
        Some(entry) => {
            send(
                update_tx,
                &entry.client_id,
                DisplayUpdate::WindowRaised {
                    window_id: entry.uuid.clone(),
                },
            );
            send(
                update_tx,
                &entry.client_id,
                DisplayUpdate::WindowFocused {
                    window_id: Some(entry.uuid.clone()),
                },
            );
        }
        // No window to attribute a focus-cleared event to, so it is
        // tagged with the empty client id; the backend broadcasts
        // `WindowFocused` regardless of tag.
        None => send(
            update_tx,
            "",
            DisplayUpdate::WindowFocused { window_id: None },
        ),
    }
}

/// Hide a window because the frontend asked to minimize it.
///
/// `entry.mapped` is deliberately **not** cleared. It tracks whether
/// the *client* has a buffer up, and the client knows nothing about
/// this: Wayland has no minimize protocol in the compositor→client
/// direction beyond `xdg_toplevel.set_minimized` going the other way.
/// Clearing it would make the very next render tick see an unmapped
/// window with live pixels and immediately re-emit `WindowMapped`,
/// producing a window that flickers back a frame after it is minimized.
/// So this is purely a frontend-visible hide, undone by
/// [`emit_restored`].
pub(crate) fn emit_minimized(state: &mut State, id: &ObjectId) {
    let Some(entry) = state.windows.entries.get(id) else {
        return;
    };
    if !entry.mapped {
        return;
    }
    send(
        &state.update_tx,
        &entry.client_id,
        DisplayUpdate::WindowUnmapped {
            window_id: entry.uuid.clone(),
        },
    );
}

/// Undo [`emit_minimized`]. Full damage is forced because the frontend
/// throws away the canvas backing an unmapped window, so a partial
/// update would restore a window that is blank except for whatever the
/// client happens to repaint next.
pub(crate) fn emit_restored(state: &mut State, id: &ObjectId) {
    let Some(entry) = state.windows.entries.get_mut(id) else {
        return;
    };
    if !entry.mapped {
        return;
    }
    entry.damage.mark_full();
    let (is_top_level, override_redirect) = entry.kind.flags();
    send(
        &state.update_tx,
        &entry.client_id,
        DisplayUpdate::WindowMapped {
            window_id: entry.uuid.clone(),
            is_top_level,
            override_redirect,
        },
    );
}

/// Drain `wl_surface.frame` callbacks over a whole surface tree.
/// See [`surface::send_frame_callbacks`] for why this is the single
/// most important call in the crate.
fn send_tree_frame_callbacks(root: &WlSurface) {
    with_surface_tree_upward(
        root,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, data, _| surface::send_frame_callbacks(data),
        |_, _, _| true,
    );
}

/// Report a WM state change the frontend should reflect (the
/// titlebar's maximize/restore affordance).
pub(crate) fn emit_state(state: &mut State, id: &ObjectId, wm_state: WindowWmState) {
    let Some(entry) = state.windows.entries.get_mut(id) else {
        return;
    };
    if entry.wm_state == wm_state {
        return;
    }
    entry.wm_state = wm_state;
    send(
        &state.update_tx,
        &entry.client_id,
        DisplayUpdate::WindowStateChanged {
            window_id: entry.uuid.clone(),
            state: wm_state,
        },
    );
}

/// Tear a window down: tell the frontend, stop routing input to it,
/// and forget its framebuffer.
pub(crate) fn destroy(state: &mut State, id: &ObjectId) {
    let Some(entry) = state.windows.entries.remove(id) else {
        return;
    };
    state.router.untrack(&entry.uuid);
    state.surfaces.remove(id);
    if entry.mapped {
        send(
            &state.update_tx,
            &entry.client_id,
            DisplayUpdate::WindowUnmapped {
                window_id: entry.uuid.clone(),
            },
        );
    }
    if entry.created {
        send(
            &state.update_tx,
            &entry.client_id,
            DisplayUpdate::WindowDestroyed {
                window_id: entry.uuid.clone(),
            },
        );
    }
    if state.windows.focused.as_ref() == Some(id) {
        state.windows.focused = None;
        // Hand focus to any other mapped toplevel so the frontend's
        // menu bar and the client's own chrome don't sit in a
        // permanently unfocused limbo after a window closes.
        let next = state
            .xdg_state
            .toplevel_surfaces()
            .iter()
            .map(|t| t.wl_surface().id())
            .find(|i| state.windows.entries.get(i).is_some_and(|e| e.mapped));
        set_focus(state, next);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn kind_flags_match_the_x11_shaped_contract() {
        // A toplevel is a real frame; a popup is the Wayland analogue
        // of override-redirect. Swapping these makes every menu render
        // as a full window with a titlebar.
        assert_eq!(WindowKind::Toplevel.flags(), (true, false));
        assert_eq!(WindowKind::Popup.flags(), (false, true));
    }

    #[test]
    fn unknown_uuid_resolves_to_no_window() {
        // An `ObjectId` cannot be minted outside a live display, so
        // the populated case is only reachable from an integration
        // run; what *is* worth pinning here is that a stale UUID
        // (a window the frontend still remembers after it closed)
        // resolves to `None` rather than to some other window.
        let reg = WindowRegistry::default();
        assert!(reg
            .id_for_uuid("00000000-0000-0000-0000-000000000000")
            .is_none());
    }
}
