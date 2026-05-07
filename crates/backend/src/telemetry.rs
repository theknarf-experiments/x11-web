//! Backend's metric instruments. SDK init lives in
//! `x11-web-telemetry`; this module defines the `Metrics` bag the
//! rest of the backend records on, plus a thin `init()` wrapper
//! that names this binary's service.

use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, UpDownCounter};
pub use x11_web_telemetry::Telemetry;

const SERVICE_NAME: &str = "x11-web-backend";

pub fn init() -> Telemetry {
    x11_web_telemetry::init(SERVICE_NAME)
}

/// One bag of pre-built instruments the rest of the backend
/// reaches for. Recording on a missing instrument (OTel disabled)
/// is a no-op via `Option`. Built once and cached.
pub struct Metrics {
    pub frame_bytes: Counter<u64>,
    pub frame_count: Counter<u64>,
    pub sidecars_connected: UpDownCounter<i64>,
    pub frontends_connected: UpDownCounter<i64>,
}

static METRICS: OnceLock<Option<Metrics>> = OnceLock::new();

pub fn metrics() -> Option<&'static Metrics> {
    METRICS
        .get_or_init(|| {
            let m = x11_web_telemetry::meter()?;
            Some(Metrics {
                frame_bytes: m
                    .u64_counter("x11web.frame_bytes")
                    .with_description("Total bytes shipped over the WebRTC media DataChannel.")
                    .with_unit("By")
                    .build(),
                frame_count: m
                    .u64_counter("x11web.frame_count")
                    .with_description("Frames shipped, by variant.")
                    .build(),
                sidecars_connected: m
                    .i64_up_down_counter("x11web.sidecars_connected")
                    .with_description("Currently-connected sidecars.")
                    .build(),
                frontends_connected: m
                    .i64_up_down_counter("x11web.frontends_connected")
                    .with_description("Currently-connected frontend WS sessions.")
                    .build(),
            })
        })
        .as_ref()
}
