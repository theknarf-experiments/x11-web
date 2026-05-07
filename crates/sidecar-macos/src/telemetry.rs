//! macOS sidecar's metric instruments. SDK init lives in
//! `x11-web-telemetry`; this module defines the `Metrics` bag the
//! sidecar records on, plus a thin `init()` wrapper that names
//! this binary's service. Mirrors `crates/sidecar/src/telemetry.rs`
//! so an operator browsing OpenObserve sees the same shape across
//! both sidecars.

use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, UpDownCounter};
pub use x11_web_telemetry::Telemetry;

const SERVICE_NAME: &str = "x11-web-sidecar-macos";

pub fn init() -> Telemetry {
    x11_web_telemetry::init(SERVICE_NAME)
}

pub struct Metrics {
    /// DisplayUpdate envelopes pushed to the backend, keyed by
    /// `kind` (put_image / thumbnail / window_created / mapped /
    /// destroyed / configured / title / menu / cleared / cursor /
    /// other) so the dominant shape on the wire is visible at a
    /// glance.
    pub display_updates: Counter<u64>,
    /// Input events the sidecar attempted to inject, keyed by
    /// `kind` (key / button / scroll / motion / menu). Counts
    /// attempts even when AX/permission-gating drops the dispatch
    /// silently — pairs with the AX-permission warnings in logs.
    pub input_events: Counter<u64>,
    /// Lifetime count of newly-discovered macOS windows surfaced
    /// as `WindowCreated`. A persistent gauge of currently-tracked
    /// windows would need a stronger lifecycle hook than we have
    /// today; this is enough to spot enumeration spikes.
    pub windows_enumerated: Counter<u64>,
    /// Live SCStream captures currently running. UpDown reflects
    /// the active workload (start/stop pairs), not lifetime starts.
    pub capture_sessions_active: UpDownCounter<i64>,
}

static METRICS: OnceLock<Option<Metrics>> = OnceLock::new();

pub fn metrics() -> Option<&'static Metrics> {
    METRICS
        .get_or_init(|| {
            let m = x11_web_telemetry::meter()?;
            Some(Metrics {
                display_updates: m
                    .u64_counter("x11web.display_updates")
                    .with_description("DisplayUpdate envelopes pushed to the backend, by kind.")
                    .build(),
                input_events: m
                    .u64_counter("x11web.input_events")
                    .with_description("Input events the sidecar attempted to inject, by kind.")
                    .build(),
                windows_enumerated: m
                    .u64_counter("x11web.windows_enumerated")
                    .with_description("Lifetime count of newly-discovered macOS windows.")
                    .build(),
                capture_sessions_active: m
                    .i64_up_down_counter("x11web.capture_sessions_active")
                    .with_description("Live SCStream captures currently running.")
                    .build(),
            })
        })
        .as_ref()
}

/// Map a `DisplayUpdate` to the `kind` attribute we tag the
/// `display_updates` counter with. Kept in one place so both
/// metric and log labelling agree.
pub fn display_update_kind(update: &x11_web_protocol::DisplayUpdate) -> &'static str {
    use x11_web_protocol::DisplayUpdate as DU;
    match update {
        DU::PutImage { .. } => "put_image",
        DU::WindowThumbnail { .. } => "thumbnail",
        DU::WindowCreated { .. } => "window_created",
        DU::WindowMapped { .. } => "window_mapped",
        DU::WindowDestroyed { .. } => "window_destroyed",
        DU::WindowConfigured { .. } => "window_configured",
        DU::TitleChanged { .. } => "title_changed",
        DU::MenuStructure { .. } => "menu_structure",
        _ => "other",
    }
}
