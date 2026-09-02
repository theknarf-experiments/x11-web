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
//! STAGE: Compositor — this module still needs:
//!   * `shm_row_to_rgba(src_row, dst_row, width, format)` — the
//!     swizzle above, forcing alpha to 0xFF for `Xrgb8888`.
//!   * `blit_copy` / `blit_over` — root surface blitted opaque into
//!     the window framebuffer, subsurfaces composited src-over.
//!   * `DamageAccumulator` — union of damage rects, clipped to the
//!     window bounds, reset to full-window on (re)size.
//!   * `crop_rgba` — cut the damage bbox out of the framebuffer for
//!     the `PutImage` payload.
//! plus `#[cfg(test)]` coverage for each.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xrgb_alpha_is_padding() {
        assert!(!ShmFormat::Xrgb8888.has_alpha());
        assert!(ShmFormat::Argb8888.has_alpha());
    }
}
