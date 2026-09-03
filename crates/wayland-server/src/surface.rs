// Derived from waylandcraft — https://github.com/EVV1E/waylandcraft
// Upstream file:   native/src/bridge.rs
// Upstream commit: 233d1431e6acbad1d0c47dfba44d971ce0cebfe8
// GPLv3 — see crates/wayland-server/NOTICE
//
// Changed from upstream: every JNI-ism is gone (jptr marshalling,
// the Vec<Box<T>> handle tables, bind_java_type!, JIntArray damage
// marshalling); the dmabuf attach arm is deleted along with the EGL
// import, which makes try_attach_buffer's terminal `unreachable!()`
// reachable — it is now a logged error, because aborting the whole
// compositor when one client attaches an exotic buffer is not an
// option for a multi-tenant sidecar. Where upstream handed Java the
// *address* of the shm mapping and let the JVM read it, this copies
// the pixels out row by row and swizzles them to RGBA on the way.

use std::collections::HashMap;
use std::ops::DerefMut;

use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::Size;
use smithay::wayland::compositor::{
    get_parent, with_states, BufferAssignment, Damage, SubsurfaceCachedState, SurfaceAttributes,
    SurfaceData,
};
use smithay::wayland::shell::xdg::{SurfaceCachedState, XdgToplevelSurfaceData};
use smithay::wayland::shm::{self, with_buffer_contents};
use smithay::wayland::single_pixel_buffer::get_single_pixel_buffer;
use smithay::wayland::viewporter::{ensure_viewport_valid, ViewportCachedState};
use tracing::{debug, warn};

use crate::pixels::{self, DamageAccumulator, Rect, ShmFormat, BPP};
use crate::utils::get_time;

/// The compositor's private copy of one surface's current contents.
///
/// A copy, not a borrow, because the client's `wl_buffer` is released
/// the instant it is attached (upstream does the same). Holding the
/// buffer instead would be the "correct" Wayland thing, but it forces
/// clients into double-buffering and gains us nothing: we have to
/// linearise the pixels for `PutImage` anyway.
pub(crate) struct SurfaceBuffer {
    /// Tightly-packed RGBA8888, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    pub width: i32,
    pub height: i32,
    /// `wl_surface.set_buffer_scale`. Recorded and reported, but not
    /// acted on: see the note on `logical_size`.
    pub scale: i32,
    /// Damage the client declared since the last render tick, in
    /// surface-local coordinates.
    pub damage: DamageAccumulator,
}

impl SurfaceBuffer {
    /// Size the surface occupies in the window framebuffer.
    ///
    /// We composite in **buffer pixels**, so this is just the buffer
    /// size. wl_output advertises scale 1 and we never send
    /// `wl_surface.preferred_buffer_scale`, so a conforming client
    /// uses `buffer_scale = 1` and buffer pixels *are* logical pixels.
    /// A client that sets a scale anyway gets a window that is
    /// `scale`× larger than it intended rather than a downsampled one
    /// — visually correct, just not HiDPI-aware. That is a deliberate
    /// bound on the vertical slice; resampling belongs with a real
    /// renderer, not a `for` loop.
    pub fn logical_size(&self) -> (i32, i32) {
        (self.width, self.height)
    }
}

/// What a `wl_surface.commit` changed, as far as the window registry
/// needs to care.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CommitOutcome {
    /// A new buffer arrived and was successfully read back.
    pub attached: bool,
    /// The client attached a NULL buffer — on a toplevel root, that
    /// is Wayland's only "unmap me" signal.
    pub removed: bool,
    /// The surface's pixel dimensions changed (including 0 -> N on
    /// the very first buffer). Forces a full-window `PutImage`.
    pub resized: bool,
}

/// Handle one `wl_surface.commit`: read back the attached buffer and
/// drain the damage the client declared.
///
/// The read-back happens *here*, in the commit, rather than in the
/// render tick, because the buffer is released before this function
/// returns. Deferring would mean either holding client buffers (see
/// [`SurfaceBuffer`]) or reading freed memory.
pub(crate) fn commit_surface(
    surfaces: &mut HashMap<ObjectId, SurfaceBuffer>,
    surface: &WlSurface,
) -> CommitOutcome {
    let id = surface.id();
    with_states(surface, |data| {
        let mut outcome = CommitOutcome::default();

        let mut attr_guard = data.cached_state.get::<SurfaceAttributes>();
        let attr = attr_guard.deref_mut().current();
        let scale = attr.buffer_scale.max(1);

        // `take()` is upstream's `attr.buffer = None`: the assignment
        // is one-shot state, and leaving it in place would make the
        // next commit re-attach a buffer that has already been
        // released back to the client.
        match attr.buffer.take() {
            Some(BufferAssignment::NewBuffer(buf)) => {
                match read_back(&buf, data) {
                    Ok(Some(image)) => {
                        let prev = surfaces.get(&id).map(|s| (s.width, s.height));
                        outcome.attached = true;
                        outcome.resized = prev != Some((image.width, image.height));

                        let entry = surfaces.entry(id.clone()).or_insert_with(|| SurfaceBuffer {
                            rgba: Vec::new(),
                            width: 0,
                            height: 0,
                            scale,
                            damage: DamageAccumulator::new(),
                        });
                        entry.rgba = image.rgba;
                        entry.width = image.width;
                        entry.height = image.height;
                        entry.scale = scale;
                        if outcome.resized {
                            // Damage rects from before a resize
                            // describe a different image; the only
                            // safe interpretation is "all of it".
                            entry.damage.mark_full();
                        }
                    }
                    // Buffer we can't read (unknown format, bad
                    // bounds, dmabuf). Keep whatever pixels we had:
                    // dropping them would flash the window black.
                    Ok(None) => {}
                    Err(e) => warn!(surface = ?id, "buffer read-back failed: {e}"),
                }
                // Unconditional, exactly as upstream: even a failed
                // attach must release, or the client waits forever
                // for a buffer it can reuse.
                buf.release();
            }
            Some(BufferAssignment::Removed) => {
                outcome.removed = true;
                surfaces.remove(&id);
            }
            None => {}
        }

        // wp_viewporter. `dst` scales the surface and `src` crops it;
        // both need a resampler we don't have, so they are recorded
        // in the log and otherwise ignored — the surface is presented
        // 1:1. Clients in the slice's scope (weston-simple-shm, foot)
        // never set either.
        {
            let mut vp_guard = data.cached_state.get::<ViewportCachedState>();
            let vp = vp_guard.deref_mut().current();
            if vp.src.is_some() || vp.dst.is_some() {
                debug!(
                    surface = ?id,
                    src = ?vp.src, dst = ?vp.dst,
                    "wp_viewport src/dst set; presenting 1:1 (no resampler in this slice)"
                );
            }
        }

        // Damage. `Damage::Surface` is already surface-local;
        // `Damage::Buffer` is in buffer pixels, which — see
        // `SurfaceBuffer::logical_size` — is the same coordinate
        // system here. Both are unioned into the surface's box and
        // translated into window space by the render tick, which is
        // the only place that knows where this surface sits.
        if let Some(entry) = surfaces.get_mut(&id) {
            for damage in attr.damage.drain(..) {
                let r = match damage {
                    Damage::Surface(d) => Rect::new(d.loc.x, d.loc.y, d.size.w, d.size.h),
                    Damage::Buffer(d) => Rect::new(d.loc.x, d.loc.y, d.size.w, d.size.h),
                };
                entry.damage.add(r);
            }
        } else {
            attr.damage.clear();
        }

        outcome
    })
}

/// A linearised, RGBA8888 copy of a client buffer.
struct Image {
    rgba: Vec<u8>,
    width: i32,
    height: i32,
}

/// Try every buffer type we understand, in order.
///
/// `Ok(None)` means "not a buffer this compositor can read" — the
/// dmabuf case, and anything a future protocol adds. Upstream ended
/// this chain with `unreachable!()` because its third arm imported
/// dmabufs via EGL and therefore matched everything; with that arm
/// deleted the fall-through is reachable, and a panic here would kill
/// every client's compositor because one of them used a GPU buffer.
fn read_back(buf: &WlBuffer, data: &SurfaceData) -> Result<Option<Image>, String> {
    match try_attach_shm(buf, data) {
        Ok(Some(img)) => return Ok(Some(img)),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    if let Some(img) = try_attach_single_pixel(buf, data) {
        return Ok(Some(img));
    }
    warn!(
        "client attached a buffer this compositor cannot read (dmabuf/GPU?); \
         keeping previous contents"
    );
    Ok(None)
}

/// Read back a `wl_shm` buffer.
///
/// `Ok(None)` = not an shm buffer (try the next mechanism).
///
/// # Safety
///
/// The pointer smithay hands us addresses a shared mapping the client
/// may be writing *concurrently*. Forming a `&[u8]` over it would be
/// undefined behaviour even if we only read — which is exactly why
/// smithay's API gives a raw pointer and upstream only ever passed
/// the address across to Java. Every access below is a
/// `copy_nonoverlapping` out of that mapping into freshly-allocated
/// memory; the resulting `Vec` is ours alone and the swizzle happens
/// on that copy.
fn try_attach_shm(buf: &WlBuffer, data: &SurfaceData) -> Result<Option<Image>, String> {
    let result = with_buffer_contents(buf, |ptr, pool_len, meta| {
        let format = match meta.format {
            wl_shm::Format::Argb8888 => ShmFormat::Argb8888,
            wl_shm::Format::Xrgb8888 => ShmFormat::Xrgb8888,
            // `ShmState::new(&dh, vec![])` advertises exactly the two
            // mandatory formats, so a third one means a
            // non-conforming client. Skip rather than guess: a wrong
            // guess produces a plausible-looking wrongly-coloured
            // window, which is far harder to diagnose than a log line.
            other => {
                warn!(?other, "unsupported wl_shm format; skipping buffer");
                return Ok(None);
            }
        };

        let (w, h) = (meta.width, meta.height);
        if w <= 0 || h <= 0 {
            return Err(format!("degenerate shm buffer {w}x{h}"));
        }
        ensure_viewport_valid(data, Size::from((w, h)));

        // Bounds arithmetic in i64 so a hostile stride/offset can't
        // wrap into a value that passes the check. The last byte we
        // touch is on row h-1, at column w-1.
        let (offset, stride) = (meta.offset as i64, meta.stride as i64);
        let row_bytes = w as i64 * BPP as i64;
        if offset < 0 || stride < row_bytes {
            return Err(format!("bad shm geometry: offset={offset} stride={stride}"));
        }
        let last = offset + (h as i64 - 1) * stride + row_bytes;
        if last > pool_len as i64 {
            return Err(format!(
                "shm buffer {w}x{h} stride={stride} offset={offset} overruns {pool_len}-byte pool"
            ));
        }

        let (uw, uh) = (w as usize, h as usize);
        let mut rgba = vec![0u8; uw * uh * BPP];
        for y in 0..uh {
            // SAFETY: `offset + y*stride + row_bytes <= pool_len` was
            // checked above for the largest y, and both terms grow
            // monotonically in y, so every row is inside the mapping.
            // Source and destination cannot overlap: `rgba` was just
            // allocated. No reference into the mapping is created.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    ptr.offset((offset + y as i64 * stride) as isize),
                    rgba.as_mut_ptr().add(y * uw * BPP),
                    uw * BPP,
                );
            }
        }

        // Now that the bytes are ours, the little-endian
        // ARGB/XRGB -> RGBA swap is safe (and unit-tested on the host).
        pixels::shm_rows_to_rgba(&mut rgba, uw, uh, format);

        Ok(Some(Image {
            rgba,
            width: w,
            height: h,
        }))
    });

    match result {
        Ok(inner) => inner,
        Err(shm::BufferAccessError::NotManaged) => Ok(None),
        Err(e) => Err(format!("{e}")),
    }
}

/// Read back a `wp_single_pixel_buffer`.
///
/// Kept because it is nearly free (`rgba8888()` hands us the four
/// bytes already in the right order — no swizzle) and because
/// toolkits use it for solid-colour backgrounds and subsurface
/// scrims, where the alternative is a mysteriously missing region.
fn try_attach_single_pixel(buf: &WlBuffer, data: &SurfaceData) -> Option<Image> {
    let pix = get_single_pixel_buffer(buf).ok()?;
    ensure_viewport_valid(data, Size::from((1, 1)));
    Some(Image {
        rgba: pix.rgba8888().to_vec(),
        width: 1,
        height: 1,
    })
}

/// Walk up to the root of a surface tree (the toplevel's or popup's
/// own `wl_surface`). Subsurfaces commit independently, so a commit
/// on a child still has to dirty the window that owns it.
pub(crate) fn root_surface(surface: &WlSurface) -> WlSurface {
    let mut current = surface.clone();
    while let Some(parent) = get_parent(&current) {
        current = parent;
    }
    current
}

/// This surface's position relative to its parent, from
/// `wl_subsurface.set_position`. Zero for anything that is not a
/// subsurface.
pub(crate) fn subsurface_offset(data: &SurfaceData) -> (i32, i32) {
    if !data.cached_state.has::<SubsurfaceCachedState>() {
        return (0, 0);
    }
    let mut guard = data.cached_state.get::<SubsurfaceCachedState>();
    let loc = guard.deref_mut().current().location;
    (loc.x, loc.y)
}

/// `xdg_surface.set_window_geometry`, if the client set one.
///
/// This is the rectangle of the surface that is "the window" — GTK
/// and Qt allocate a larger buffer and put their drop shadow in the
/// margin, then point geometry at the inner rectangle. Honouring it
/// is what stops every CSD window arriving with a fat translucent
/// border and a size that doesn't match its titlebar.
pub(crate) fn xdg_window_geometry(surface: &WlSurface) -> Option<Rect> {
    with_states(surface, |states| {
        let mut guard = states.cached_state.get::<SurfaceCachedState>();
        guard
            .current()
            .geometry
            .map(|r| Rect::new(r.loc.x, r.loc.y, r.size.w, r.size.h))
    })
}

/// `xdg_toplevel.set_title` / `set_app_id`, whichever is set.
///
/// The frontend needs *a* label; a toplevel that has only ever set an
/// app_id (common for terminals before the shell reports a command)
/// would otherwise show an empty titlebar.
pub(crate) fn toplevel_label(surface: &WlSurface) -> Option<String> {
    with_states(surface, |states| {
        let data = states.data_map.get::<XdgToplevelSurfaceData>()?;
        let attrs = data.lock().ok()?;
        attrs
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| attrs.app_id.clone().filter(|a| !a.is_empty()))
    })
}

/// Fire every pending `wl_surface.frame` callback on one surface.
///
/// **This is the single most load-bearing call in the crate.** A
/// Wayland client that uses frame callbacks — which is all of them,
/// because it is the only throttling mechanism the protocol offers —
/// draws one frame, requests a callback, and then blocks until it
/// arrives. Upstream only calls this from Minecraft's render loop; a
/// headless compositor with no render loop must supply the tick
/// itself, and if it doesn't, every toolkit client paints exactly one
/// frame and then hangs forever with no error on either side.
///
/// Takes `&SurfaceData`, **not** `&WlSurface`, and that is not a
/// stylistic choice. The only caller walks the surface tree, and
/// `with_surface_tree_upward` holds each surface's user-data
/// `std::sync::Mutex` for the duration of the visit — the same mutex
/// `with_states` acquires. Re-entering it from inside the walk
/// deadlocks the compositor thread outright: clients stay connected,
/// the socket stays up, and every window freezes on the frame it had
/// when the walk first ran.
pub(crate) fn send_frame_callbacks(data: &SurfaceData) {
    let mut guard = data.cached_state.get::<SurfaceAttributes>();
    for c in guard.deref_mut().current().frame_callbacks.drain(..) {
        c.done(get_time());
    }
}
