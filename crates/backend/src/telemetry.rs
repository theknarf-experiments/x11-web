//! OpenTelemetry init for the backend.
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set in the environment
//! we install both pipelines (traces + metrics) over OTLP gRPC,
//! plus bridge `tracing` spans into OTel via
//! `tracing-opentelemetry`. When unset, only the stdout fmt layer
//! runs — same behaviour the backend always had.
//!
//! `Telemetry::shutdown()` flushes both pipelines on graceful
//! exit so we don't lose the last batch of spans / metric points.

use std::sync::OnceLock;

use opentelemetry::global;
use opentelemetry::metrics::{Counter, Meter, UpDownCounter};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{MetricExporter, SpanExporter};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "x11-web-backend";

/// Cached after first init so the rest of the codebase can
/// `metrics().counter("...")` without threading a handle through.
static METER: OnceLock<Meter> = OnceLock::new();

/// Returned from `init()` so the caller can flush on shutdown.
/// `None` when OTel is disabled (no providers were installed).
///
/// Fields are held just to keep the providers alive for the
/// lifetime of the process; the global registries hold the
/// references that are actually consulted at runtime. `shutdown`
/// is plumbed for a future SIGINT/SIGTERM handler but not yet
/// wired — process kill drops the last in-flight batch, which is
/// the same behaviour the pre-OTel backend had.
#[allow(dead_code)]
pub struct Telemetry {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

#[allow(dead_code)]
impl Telemetry {
    pub fn shutdown(self) {
        if let Some(tp) = self.tracer_provider {
            if let Err(e) = tp.shutdown() {
                eprintln!("OTel tracer shutdown failed: {e}");
            }
        }
        if let Some(mp) = self.meter_provider {
            if let Err(e) = mp.shutdown() {
                eprintln!("OTel meter shutdown failed: {e}");
            }
        }
    }
}

/// Set up tracing + metrics.
///
/// Always installs the stdout `tracing-subscriber` fmt layer with
/// `RUST_LOG`-style env filter (default `info`).
///
/// Additionally, when `OTEL_EXPORTER_OTLP_ENDPOINT` is set:
///   * Spins up an OTLP gRPC `SpanExporter` and registers a
///     `SdkTracerProvider` globally; bridges the existing
///     `tracing::info_span!` / `#[tracing::instrument]` calls
///     into OTel spans via `tracing-opentelemetry`'s layer.
///   * Spins up an OTLP gRPC `MetricExporter` with a periodic
///     reader on the default 60s cadence; registers an
///     `SdkMeterProvider` globally.
///
/// Returns a [`Telemetry`] handle the caller drops / shuts down
/// on graceful exit.
pub fn init() -> Telemetry {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.trim().is_empty());

    if otlp_endpoint.is_none() {
        // No OTel; just the stdout subscriber. Same as the
        // pre-OTel backend.
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
        return Telemetry {
            tracer_provider: None,
            meter_provider: None,
        };
    }

    let resource = Resource::builder()
        .with_service_name(SERVICE_NAME)
        .with_attribute(KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .build();

    // Traces — `with_export_config` honours `OTEL_EXPORTER_OTLP_ENDPOINT`
    // (and other standard `OTEL_*` env vars) by default.
    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("OTLP span exporter init");
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    global::set_tracer_provider(tracer_provider.clone());

    // Metrics — periodic exporter (default 60s tick).
    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .build()
        .expect("OTLP metric exporter init");
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER.set(global::meter(SERVICE_NAME));

    let tracer = tracer_provider.tracer(SERVICE_NAME);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    info!("OTel enabled — exporting traces + metrics to OTLP gRPC");

    Telemetry {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
    }
}

/// Process-wide meter. Returns `None` until `init()` has run with
/// OTel enabled. Callers should `if let Some(m) = meter() { ... }`
/// so the metric path is a no-op in OTel-disabled mode.
pub fn meter() -> Option<&'static Meter> {
    METER.get()
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
            let m = meter()?;
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
