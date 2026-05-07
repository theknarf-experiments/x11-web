//! Shared OpenTelemetry init for the x11-web Rust binaries.
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set in the environment
//! the SDK installs both pipelines (traces + metrics) over OTLP
//! gRPC, plus bridges `tracing` spans into OTel via
//! `tracing-opentelemetry`. When unset, only the stdout fmt layer
//! runs — same behaviour the binaries always had.
//!
//! Each binary defines its own `Metrics` bag of instruments and
//! looks them up off the cached [`meter`]; this crate just owns
//! the SDK lifecycle.

use std::sync::OnceLock;

use opentelemetry::global;
use opentelemetry::metrics::Meter;
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

static METER: OnceLock<Meter> = OnceLock::new();

/// Returned from [`init`] so the caller can keep the providers
/// alive for the life of the process. Held by the `main` future.
///
/// `shutdown` is plumbed for a future SIGINT/SIGTERM handler but
/// not yet wired into either binary; on process kill the last
/// in-flight batch is dropped (the same behaviour the pre-OTel
/// binaries had).
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

/// Initialise telemetry for `service_name`. Always installs the
/// stdout `tracing-subscriber` fmt layer with `RUST_LOG`-style
/// env filter (default `info`).
///
/// Additionally, when `OTEL_EXPORTER_OTLP_ENDPOINT` is set:
///   * Spins up an OTLP gRPC `SpanExporter` and registers an
///     `SdkTracerProvider` globally; bridges `tracing` calls into
///     OTel spans via `tracing-opentelemetry`.
///   * Spins up an OTLP gRPC `MetricExporter` with a periodic
///     reader on the SDK's default 60 s cadence; registers an
///     `SdkMeterProvider` globally.
pub fn init(service_name: &'static str) -> Telemetry {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.trim().is_empty());

    if otlp_endpoint.is_none() {
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
        .with_service_name(service_name)
        .with_attribute(KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .build();

    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("OTLP span exporter init");
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    global::set_tracer_provider(tracer_provider.clone());

    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .build()
        .expect("OTLP metric exporter init");
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER.set(global::meter(service_name));

    let tracer = tracer_provider.tracer(service_name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    info!(service = service_name, "OTel enabled — exporting traces + metrics to OTLP gRPC");

    Telemetry {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
    }
}

/// Process-wide meter. Returns `None` until [`init`] has run with
/// OTel enabled. Each binary's `metrics()` accessor wraps this so
/// recording on a missing instrument is a no-op.
pub fn meter() -> Option<&'static Meter> {
    METER.get()
}
