//! Pure pixel arithmetic — **portable**, no smithay, no `cfg`.
//!
//! Everything the compositor does to bytes between "the client's shm
//! pool" and "`DisplayUpdate::PutImage`" lives here so it can be
//! unit-tested on the macOS host without a Linux container. The
//! single most likely silent bug in the whole sidecar is a
//! red/blue-swapped window: wl_shm's `Argb8888`/`Xrgb8888` are
//! little-endian 32-bit words, so in memory they are `[B, G, R, A]`,
//! while `DisplayUpdate::PutImage` carries `[R, G, B, A]`. A window
//! that renders with red and blue transposed looks *plausible* and
//! produces no error anywhere — hence the tests.
//!
//! ## Why `[R, G, B, A]` is the target, verified rather than assumed
//!
//! `DisplayUpdate::PutImage.data` is handed to the sidecar, which
//! calls `x11_web_pixel_codec::encode_rgba_auto` — which feeds
//! `webp::Encoder::from_rgba` either way, i.e. literally "bytes in R,
//! G, B, A order". The X11 server library documents the same contract
//! ("Pixels are raw RGBA", crates/x11-server/.../sync_flush.rs) and
//! does its own BGRA→RGBA swap at the X11 wire boundary. So the swap
//! belongs here, once, at the wl_shm boundary — and nowhere else.

/// The two wl_shm formats this compositor accepts.
///
/// `ShmState::new::<D>(&dh, vec![])` advertises exactly these: they
/// are mandatory in the wl_shm protocol and always implicitly
/// supported, so passing an empty extra-format list is not a
/// restriction, it is the complete set. Any other format a client
/// somehow attaches is logged and skipped rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmFormat {
    /// 32-bit ARGB, little-endian word => `[B, G, R, A]` in memory.
    /// The alpha channel is meaningful.
    Argb8888,
    /// 32-bit xRGB, little-endian word => `[B, G, R, x]` in memory.
    /// The top byte is undefined and must be forced to 0xFF, not
    /// copied — clients routinely leave garbage there.
    Xrgb8888,
}

impl ShmFormat {
    /// Whether the source's fourth byte carries real alpha, or is
    /// padding that must be replaced with 0xFF.
    pub fn has_alpha(self) -> bool {
        matches!(self, ShmFormat::Argb8888)
    }
}

/// Bytes per pixel in every buffer this module touches. Both accepted
/// wl_shm formats are 32bpp, and `PutImage` is RGBA8888.
pub const BPP: usize = 4;

/// Swizzle one row of wl_shm pixels into RGBA8888, **in place**.
///
/// In place, rather than `src -> dst`, on purpose. The plan's byte
/// path is: `ptr::copy_nonoverlapping` the row straight out of the
/// client's live mmap into an owned `Vec` (never forming a `&[u8]`
/// over shared memory the client may be writing — that would be UB),
/// and only *then* touch it. Since the copy already produced an owned
/// buffer of exactly the right length, a second one would be pure
/// waste.
///
/// `row` must be at least `width * 4` bytes; any trailing stride
/// padding is left untouched (it is never emitted).
pub fn shm_row_to_rgba(row: &mut [u8], width: usize, format: ShmFormat) {
    let n = width.min(row.len() / BPP);
    let opaque = !format.has_alpha();
    for px in row[..n * BPP].chunks_exact_mut(BPP) {
        // [B, G, R, A] -> [R, G, B, A]: green and alpha stay put.
        px.swap(0, 2);
        if opaque {
            px[3] = 0xFF;
        }
    }
}

/// Swizzle a whole tightly-packed `width * height * 4` buffer.
///
/// Tightly packed because the caller has already de-strided the rows
/// during the copy out of the shm pool; stride only exists on the
/// client's side of that copy.
pub fn shm_rows_to_rgba(buf: &mut [u8], width: usize, height: usize, format: ShmFormat) {
    let row_bytes = width * BPP;
    if row_bytes == 0 {
        return;
    }
    for y in 0..height {
        let start = y * row_bytes;
        let Some(row) = buf.get_mut(start..start + row_bytes) else {
            break;
        };
        shm_row_to_rgba(row, width, format);
    }
}

/// An axis-aligned rectangle in window-local pixels.
///
/// `i32` rather than `u16` because damage arrives in surface-local
/// coordinates that can legitimately be negative once translated by a
/// subsurface offset or an xdg window-geometry origin; clipping to the
/// window happens at the end, not at every intermediate step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Smallest rectangle containing both. Empty operands are
    /// absorbed rather than growing the result to include their
    /// origin, which would silently inflate damage to the top-left.
    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Clip to `0..w × 0..h`. Returns `None` when nothing survives.
    pub fn clip(self, w: i32, h: i32) -> Option<Rect> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w).min(w);
        let y1 = (self.y + self.h).min(h);
        if x1 <= x0 || y1 <= y0 {
            None
        } else {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        }
    }

    pub fn translate(self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// Per-window damage bookkeeping.
///
/// Deliberately a single bounding box rather than a rect list. One
/// `PutImage` per window per tick is the contract (see
/// `windows::tick`), so a list would only ever be collapsed to its
/// bbox before emission — keeping the bbox incrementally is the same
/// answer for less code. `full` short-circuits the whole thing on
/// first map and on every size change, where the frontend needs a
/// complete frame regardless of what the client claims it touched.
#[derive(Debug, Default, Clone)]
pub struct DamageAccumulator {
    bbox: Option<Rect>,
    full: bool,
}

impl DamageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        self.bbox = Some(match self.bbox {
            Some(b) => b.union(r),
            None => r,
        });
    }

    /// Force the next emission to cover the entire window.
    pub fn mark_full(&mut self) {
        self.full = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.full || self.bbox.is_some()
    }

    /// Consume the accumulated damage, clipped to a `w × h` window.
    /// `None` means "nothing to send this tick".
    pub fn take(&mut self, w: i32, h: i32) -> Option<Rect> {
        let full = std::mem::replace(&mut self.full, false);
        let bbox = self.bbox.take();
        if full {
            return Rect::new(0, 0, w, h).clip(w, h);
        }
        bbox?.clip(w, h)
    }
}

/// Copy `src` (a tightly-packed `src_w × src_h` RGBA image) into
/// `dst` at `(x, y)`, overwriting the destination alpha too.
///
/// This is the *root* surface's blit. A toplevel's root surface owns
/// the whole window: it is the background, not a layer over one, so
/// its alpha must land verbatim rather than being composited against
/// the cleared framebuffer (which would leave a translucent window
/// looking milky where it should be see-through).
///
/// Offsets may be negative and the source may overhang any edge; the
/// overhang is clipped, not wrapped.
//
// clippy::too_many_arguments: a blit is (dst, dst dims, src, src dims,
// offset) and there is no smaller honest spelling. Bundling the dims
// into a `Rect`-ish struct would only move the eight values one level
// down while adding a constructor at every one of the call sites in
// `windows.rs`, which is a loss.
#[allow(clippy::too_many_arguments)]
pub fn blit_copy(
    dst: &mut [u8],
    dst_w: i32,
    dst_h: i32,
    src: &[u8],
    src_w: i32,
    src_h: i32,
    x: i32,
    y: i32,
) {
    for_each_overlapping_row(dst_w, dst_h, src_w, src_h, x, y, |dy, sy, x0, sx0, cols| {
        let d = (dy * dst_w + x0) as usize * BPP;
        let s = (sy * src_w + sx0) as usize * BPP;
        let n = cols as usize * BPP;
        if d + n <= dst.len() && s + n <= src.len() {
            dst[d..d + n].copy_from_slice(&src[s..s + n]);
        }
    });
}

/// Composite `src` over `dst` at `(x, y)` using straight (non
/// premultiplied) source-over.
///
/// wl_shm's ARGB8888 is defined as **premultiplied** alpha, so the
/// textbook formula would be `d = s + d*(1-a)`. We un-premultiply
/// nothing and use `d = s + d*(1-a)` exactly: that is correct for
/// premultiplied sources and is what every Wayland compositor does.
/// (`Xrgb8888` arrives here with alpha already forced to 0xFF, so it
/// degenerates to a copy — which is why the fast path exists.)
//
// clippy::too_many_arguments: see `blit_copy` above.
#[allow(clippy::too_many_arguments)]
pub fn blit_over(
    dst: &mut [u8],
    dst_w: i32,
    dst_h: i32,
    src: &[u8],
    src_w: i32,
    src_h: i32,
    x: i32,
    y: i32,
) {
    for_each_overlapping_row(dst_w, dst_h, src_w, src_h, x, y, |dy, sy, x0, sx0, cols| {
        for i in 0..cols as usize {
            let d = ((dy * dst_w + x0) as usize + i) * BPP;
            let s = ((sy * src_w + sx0) as usize + i) * BPP;
            if d + BPP > dst.len() || s + BPP > src.len() {
                break;
            }
            let a = src[s + 3] as u32;
            if a == 255 {
                dst[d..d + BPP].copy_from_slice(&src[s..s + BPP]);
                continue;
            }
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            for c in 0..4 {
                // +127 then /255 is round-to-nearest without a
                // divide-by-255 approximation drifting the result
                // dark over repeated composites.
                let v = src[s + c] as u32 + (dst[d + c] as u32 * inv + 127) / 255;
                dst[d + c] = v.min(255) as u8;
            }
        }
    });
}

/// Shared clipping walk for the two blits: yields
/// `(dst_row, src_row, dst_x0, src_x0, columns)` for each row where
/// the source actually overlaps the destination.
fn for_each_overlapping_row(
    dst_w: i32,
    dst_h: i32,
    src_w: i32,
    src_h: i32,
    x: i32,
    y: i32,
    mut f: impl FnMut(i32, i32, i32, i32, i32),
) {
    if dst_w <= 0 || dst_h <= 0 || src_w <= 0 || src_h <= 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + src_w).min(dst_w);
    let y1 = (y + src_h).min(dst_h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let cols = x1 - x0;
    for dy in y0..y1 {
        f(dy, dy - y, x0, x0 - x, cols);
    }
}

/// Cut `rect` out of a tightly-packed `fb_w × fb_h` RGBA framebuffer.
///
/// The result is the `PutImage` payload; `rect` must already be
/// clipped to the framebuffer (see [`DamageAccumulator::take`]).
/// Returns an empty vec for a degenerate rect rather than panicking —
/// a zero-area `PutImage` is dropped by the caller.
pub fn crop_rgba(fb: &[u8], fb_w: i32, fb_h: i32, rect: Rect) -> Vec<u8> {
    let Some(r) = rect.clip(fb_w, fb_h) else {
        return Vec::new();
    };
    let row_bytes = r.w as usize * BPP;
    let mut out = Vec::with_capacity(row_bytes * r.h as usize);
    for row in 0..r.h {
        let start = ((r.y + row) * fb_w + r.x) as usize * BPP;
        match fb.get(start..start + row_bytes) {
            Some(s) => out.extend_from_slice(s),
            None => return out,
        }
    }
    out
}

/// Force every pixel's alpha to 0xFF, in place.
///
/// The last thing that happens to a `PutImage` payload, and it is not
/// optional. Two facts collide otherwise:
///
///   1. The frontend paints a `PutImage` with
///      `ctx.drawImage(bitmap, x, y)` — plain source-over onto a
///      persistent back buffer, with no `clearRect` first (see
///      `ClientRenderer.pushPutImage`). It was written against the X11
///      sidecar, whose framebuffer is opaque by construction, so a
///      sub-255 alpha there means "blend with whatever was on screen
///      last frame" rather than "replace it".
///   2. wl_shm's `Argb8888` is **premultiplied**, while WebP →
///      `createImageBitmap` is straight alpha. Shipping a premultiplied
///      translucent pixel through that path darkens it.
///
/// So a client that renders any translucency inside its window geometry
/// would produce ghosting *and* wrong colours. Flattening to opaque is
/// exactly right rather than merely safe: the composite ran against an
/// opaque black framebuffer, and premultiplied-over-black *is* the
/// premultiplied RGB, so the bytes already are the correct opaque
/// colour — only the alpha byte is lying.
///
/// (Genuine per-window translucency would mean a `clearRect` in the
/// frontend plus an un-premultiply pass here. Out of scope, and not
/// something a window on an opaque canvas can show off anyway.)
pub fn force_opaque(rgba: &mut [u8]) {
    for a in rgba.iter_mut().skip(3).step_by(BPP) {
        *a = 0xFF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_opaque_rewrites_only_the_alpha_byte() {
        // Premultiplied half-transparent red, and a fully transparent
        // pixel — the two shapes that used to ghost.
        let mut buf = vec![128, 0, 0, 128, 0, 0, 0, 0];
        force_opaque(&mut buf);
        assert_eq!(buf, vec![128, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn force_opaque_tolerates_a_short_trailing_pixel() {
        // crop_rgba can return early on a truncated framebuffer; the
        // caller drops such a payload, but this must not panic first.
        let mut buf = vec![1, 2, 3];
        force_opaque(&mut buf);
        assert_eq!(buf, vec![1, 2, 3]);
    }

    #[test]
    fn xrgb_alpha_is_padding() {
        assert!(!ShmFormat::Xrgb8888.has_alpha());
        assert!(ShmFormat::Argb8888.has_alpha());
    }

    #[test]
    fn argb_memory_order_is_bgra_and_swizzles_to_rgba() {
        // A pure red, fully opaque ARGB8888 pixel is the 32-bit word
        // 0xFFFF0000, which little-endian is the byte sequence
        // [0x00, 0x00, 0xFF, 0xFF] = [B, G, R, A].
        let mut row = [0x00, 0x00, 0xFF, 0xFF];
        shm_row_to_rgba(&mut row, 1, ShmFormat::Argb8888);
        assert_eq!(row, [0xFF, 0x00, 0x00, 0xFF], "red must survive as R");
    }

    #[test]
    fn xrgb_forces_alpha_opaque() {
        // Garbage in the padding byte is normal for XRGB clients.
        let mut row = [0x10, 0x20, 0x30, 0x7A];
        shm_row_to_rgba(&mut row, 1, ShmFormat::Xrgb8888);
        assert_eq!(row, [0x30, 0x20, 0x10, 0xFF]);
    }

    #[test]
    fn swizzle_respects_width_and_leaves_stride_padding_alone() {
        // Two pixels of payload plus four bytes of stride padding.
        let mut buf = [0, 0, 255, 255, 255, 0, 0, 255, 0xAA, 0xBB, 0xCC, 0xDD];
        shm_row_to_rgba(&mut buf, 2, ShmFormat::Argb8888);
        assert_eq!(&buf[0..4], &[255, 0, 0, 255]);
        assert_eq!(&buf[4..8], &[0, 0, 255, 255]);
        assert_eq!(&buf[8..12], &[0xAA, 0xBB, 0xCC, 0xDD], "padding untouched");
    }

    #[test]
    fn multi_row_swizzle() {
        let mut buf = vec![0u8; 2 * 2 * BPP];
        buf[0] = 0xFF; // row 0 px 0: blue
        buf[8] = 0xFF; // row 1 px 0: blue
        shm_rows_to_rgba(&mut buf, 2, 2, ShmFormat::Xrgb8888);
        assert_eq!(&buf[0..4], &[0, 0, 0xFF, 0xFF]);
        assert_eq!(&buf[8..12], &[0, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn rect_union_ignores_empties() {
        let a = Rect::new(10, 10, 5, 5);
        assert_eq!(a.union(Rect::new(0, 0, 0, 0)), a);
        assert_eq!(Rect::new(0, 0, 0, 0).union(a), a);
        assert_eq!(a.union(Rect::new(20, 20, 5, 5)), Rect::new(10, 10, 15, 15));
    }

    #[test]
    fn rect_clip_drops_offscreen() {
        assert_eq!(
            Rect::new(-5, -5, 10, 10).clip(100, 100),
            Some(Rect::new(0, 0, 5, 5))
        );
        assert_eq!(Rect::new(200, 0, 10, 10).clip(100, 100), None);
    }

    #[test]
    fn damage_accumulator_unions_then_resets() {
        let mut d = DamageAccumulator::new();
        assert!(!d.is_dirty());
        assert_eq!(d.take(100, 100), None);

        d.add(Rect::new(1, 1, 2, 2));
        d.add(Rect::new(50, 60, 10, 10));
        assert!(d.is_dirty());
        assert_eq!(d.take(100, 100), Some(Rect::new(1, 1, 59, 69)));
        // take() is a drain: a second call in the same tick is empty.
        assert_eq!(d.take(100, 100), None);
    }

    #[test]
    fn damage_full_beats_bbox_and_clips_to_window() {
        let mut d = DamageAccumulator::new();
        d.add(Rect::new(1, 1, 2, 2));
        d.mark_full();
        assert_eq!(d.take(8, 4), Some(Rect::new(0, 0, 8, 4)));
        assert!(!d.is_dirty());
    }

    #[test]
    fn damage_out_of_bounds_is_dropped_not_clamped_to_garbage() {
        let mut d = DamageAccumulator::new();
        d.add(Rect::new(500, 500, 4, 4));
        assert_eq!(d.take(100, 100), None);
    }

    #[test]
    fn blit_copy_clips_negative_and_overhanging_offsets() {
        // 2x2 destination, 2x2 source placed at (-1, -1): only the
        // source's bottom-right pixel lands, at the dest origin.
        let mut dst = vec![0u8; 2 * 2 * BPP];
        let mut src = vec![0u8; 2 * 2 * BPP];
        src[3 * BPP..4 * BPP].copy_from_slice(&[1, 2, 3, 4]);
        blit_copy(&mut dst, 2, 2, &src, 2, 2, -1, -1);
        assert_eq!(&dst[0..4], &[1, 2, 3, 4]);
        assert_eq!(&dst[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn blit_copy_overwrites_alpha() {
        let mut dst = vec![255u8; BPP];
        let src = vec![0u8; BPP];
        blit_copy(&mut dst, 1, 1, &src, 1, 1, 0, 0);
        assert_eq!(dst, vec![0, 0, 0, 0], "root blit is a copy, not a blend");
    }

    #[test]
    fn blit_over_blends_premultiplied() {
        // Half-transparent premultiplied white (128,128,128,128) over
        // opaque black => ~(128,128,128,255).
        let mut dst = vec![0, 0, 0, 255];
        let src = vec![128, 128, 128, 128];
        blit_over(&mut dst, 1, 1, &src, 1, 1, 0, 0);
        assert_eq!(dst[0], 128);
        assert_eq!(dst[3], 255, "opaque backdrop stays opaque");
    }

    #[test]
    fn blit_over_skips_fully_transparent_source() {
        let mut dst = vec![9, 9, 9, 255];
        let src = vec![0, 0, 0, 0];
        blit_over(&mut dst, 1, 1, &src, 1, 1, 0, 0);
        assert_eq!(dst, vec![9, 9, 9, 255]);
    }

    #[test]
    fn crop_extracts_the_damage_box() {
        // 3x2 framebuffer, each pixel tagged with its index.
        let mut fb = vec![0u8; 3 * 2 * BPP];
        for i in 0..6 {
            fb[i * BPP] = i as u8;
        }
        let out = crop_rgba(&fb, 3, 2, Rect::new(1, 0, 2, 2));
        assert_eq!(out.len(), 2 * 2 * BPP);
        assert_eq!(out[0], 1);
        assert_eq!(out[BPP], 2);
        assert_eq!(out[2 * BPP], 4);
        assert_eq!(out[3 * BPP], 5);
    }

    #[test]
    fn crop_of_degenerate_rect_is_empty_not_a_panic() {
        let fb = vec![0u8; 4 * BPP];
        assert!(crop_rgba(&fb, 2, 2, Rect::new(5, 5, 1, 1)).is_empty());
        assert!(crop_rgba(&fb, 2, 2, Rect::new(0, 0, 0, 0)).is_empty());
    }
}
