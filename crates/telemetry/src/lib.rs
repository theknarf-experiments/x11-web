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

use std::collections::HashMap;
use std::sync::OnceLock;

use opentelemetry::global;
use opentelemetry::metrics::Meter;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{Context, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

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
    logger_provider: Option<SdkLoggerProvider>,
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
        if let Some(lp) = self.logger_provider {
            if let Err(e) = lp.shutdown() {
                eprintln!("OTel logger shutdown failed: {e}");
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
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

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
            logger_provider: None,
        };
    }

    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
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
        .with_resource(resource.clone())
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    // W3C Trace Context for cross-process propagation. Lets the
    // backend ↔ sidecar wire format inject/extract `traceparent`
    // through the helpers below and have the standard format
    // applied (older peers that just see the raw string still
    // round-trip it).
    global::set_text_map_propagator(TraceContextPropagator::new());
    let _ = METER.set(global::meter(service_name));

    // Logs — `tracing` events bridge into OTel `LogRecord`s via
    // `OpenTelemetryTracingBridge`, then the SDK ships them out
    // as the third pillar over OTLP gRPC. Bridge filters out its
    // own crate names so the exporter's tracing emissions don't
    // feed back into themselves and explode (a classic OTel
    // footgun).
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .build()
        .expect("OTLP log exporter init");
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(log_exporter)
        .build();
    let log_layer = OpenTelemetryTracingBridge::new(&logger_provider).with_filter(
        EnvFilter::new("info")
            .add_directive("opentelemetry=off".parse().unwrap())
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("tonic=off".parse().unwrap())
            .add_directive("h2=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap()),
    );

    let tracer = tracer_provider.tracer(service_name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .with(log_layer)
        .init();

    info!(
        service = service_name,
        "OTel enabled — exporting traces + metrics + logs to OTLP gRPC"
    );

    Telemetry {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        logger_provider: Some(logger_provider),
    }
}

/// Process-wide meter. Returns `None` until [`init`] has run with
/// OTel enabled. Each binary's `metrics()` accessor wraps this so
/// recording on a missing instrument is a no-op.
pub fn meter() -> Option<&'static Meter> {
    METER.get()
}

/// Resolves on the first SIGINT or SIGTERM the process receives.
/// Each binary `select!`s it next to its main loop and then calls
/// [`Telemetry::shutdown`] so the tail of the metric / span / log
/// batch flushes before the process exits — without this, the
/// last few seconds of telemetry are dropped on every Ctrl-C.
///
/// Windows has no SIGTERM; the `cfg(not(unix))` arm only listens
/// for Ctrl-C there.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        sig.recv().await;
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

/// Re-export — gives `Context::span_context()` on the receiver
/// side so callers can ask "is the parent context valid?" before
/// calling `set_parent`.
pub use opentelemetry::trace::TraceContextExt;
/// Re-export — lets a `tracing::Span` adopt an OTel `Context` as
/// its parent (`span.set_parent(ctx)`). Re-exported so binary
/// callers don't have to take a direct dependency on
/// `tracing-opentelemetry`.
pub use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Mark the active span as failed and record structured error
/// metadata so the failure is both visible (red in OpenObserve's
/// trace view) and queryable.
///
/// `kind` is a stable, low-cardinality discriminator —
/// "sidecar_not_found", "spawn_failed", "no_route_for_window",
/// etc. — suitable for `WHERE error.kind = '…'` queries.
/// `message` is the human-readable detail, often the underlying
/// `e.to_string()`.
///
/// The fields `error.kind` and `error.message` must be declared
/// on the active span (with `tracing::field::Empty`) for the
/// `record` calls to take effect — `tracing` requires a static
/// field set per call site. The helper always sets the status,
/// which works regardless.
pub fn mark_span_error(kind: &'static str, message: impl Into<String>) {
    let message: String = message.into();
    let span = tracing::Span::current();
    span.record("error.kind", kind);
    span.record("error.message", message.as_str());
    span.set_status(opentelemetry::trace::Status::error(message));
}

/* ----------------------------------------------------------------
 * Cross-process trace context propagation.
 *
 * The backend ↔ sidecar wire format carries a `traceparent` field
 * on every message. These helpers handle the W3C-format
 * inject/extract against an `opentelemetry::Context`, used by the
 * sender (`current_traceparent()`) and the receiver
 * (`extract_traceparent(s)`).
 *
 * Both are no-ops when telemetry is disabled — `inject_context`
 * sees no SpanContext and writes nothing; `extract` returns an
 * empty Context that doesn't influence any subsequent spans.
 * ---------------------------------------------------------------- */

struct MapInjector<'a>(&'a mut HashMap<String, String>);
impl Injector for MapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

struct MapExtractor<'a>(&'a HashMap<String, String>);
impl Extractor for MapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

/// Serialise the current task's trace context to a W3C
/// `traceparent` string, suitable for putting on the wire. Empty
/// string when there's no active span / OTel isn't running — the
/// receiving side treats empty as "no context, start fresh".
pub fn current_traceparent() -> String {
    let mut carrier = HashMap::new();
    let propagator = TraceContextPropagator::new();
    propagator.inject_context(&Context::current(), &mut MapInjector(&mut carrier));
    carrier.remove("traceparent").unwrap_or_default()
}

/// Parse a `traceparent` string back into an OTel [`Context`].
/// Empty / malformed input yields an empty Context (no parent —
/// any span opened against it becomes a fresh root, which is the
/// safe degradation for un-instrumented peers).
pub fn extract_traceparent(traceparent: &str) -> Context {
    if traceparent.is_empty() {
        return Context::new();
    }
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());
    let propagator = TraceContextPropagator::new();
    propagator.extract(&MapExtractor(&carrier))
}
