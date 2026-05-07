//! Sidecar's metric instruments. SDK init lives in
//! `x11-web-telemetry`; this module defines the `Metrics` bag the
//! sidecar records on, plus a thin `init()` wrapper that names
//! this binary's service.

use std::sync::OnceLock;

use opentelemetry::metrics::Counter;
pub use x11_web_telemetry::Telemetry;

const SERVICE_NAME: &str = "x11-web-sidecar";

pub fn init() -> Telemetry {
    x11_web_telemetry::init(SERVICE_NAME)
}

pub struct Metrics {
    /// Processes the sidecar has spawned over its lifetime.
    pub processes_spawned: Counter<u64>,
    /// Display updates pushed to the backend (PutImage / cursor /
    /// thumbnails / cleared). The byte volume on the wire is
    /// covered by the backend's `frame_bytes`.
    pub display_updates: Counter<u64>,
}

static METRICS: OnceLock<Option<Metrics>> = OnceLock::new();

pub fn metrics() -> Option<&'static Metrics> {
    METRICS
        .get_or_init(|| {
            let m = x11_web_telemetry::meter()?;
            Some(Metrics {
                processes_spawned: m
                    .u64_counter("x11web.processes_spawned")
                    .with_description("Processes spawned over the sidecar's lifetime.")
                    .build(),
                display_updates: m
                    .u64_counter("x11web.display_updates")
                    .with_description("Display updates pushed to the backend, by kind.")
                    .build(),
            })
        })
        .as_ref()
}
