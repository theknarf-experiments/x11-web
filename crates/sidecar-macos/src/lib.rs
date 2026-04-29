//! macOS sidecar — observes/controls the host's running apps and
//! bridges them to the x11-web backend via the same wire protocol the
//! Linux sidecar speaks.
//!
//! Architecturally distinct from `crates/sidecar`: that one *hosts* an
//! X11 server that apps draw into; this one observes the macOS
//! WindowServer from the outside, captures pixels via ScreenCaptureKit,
//! and synthesizes input via CGEvent + SkyLight private SPI. See the
//! cua project (`https://github.com/trycua/cua`) for the technique
//! lineage — most of this crate's primitives mirror cua-driver's
//! `CuaDriverCore` module.

#[cfg(target_os = "macos")]
pub mod capture;
#[cfg(target_os = "macos")]
pub mod enumerator;
#[cfg(target_os = "macos")]
pub mod skylight;
#[cfg(target_os = "macos")]
pub mod windows;
