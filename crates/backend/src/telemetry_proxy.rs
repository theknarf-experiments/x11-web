//! Same-origin OTLP/HTTP trace proxy.
//!
//! The browser SDK posts protobuf-encoded spans to
//! `/api/telemetry/v1/traces` on this backend (same origin as the
//! SPA, so no CORS headache and no OpenObserve credentials in the
//! browser). We forward the body verbatim to the upstream OTLP/HTTP
//! receiver — OpenObserve in dev — with the auth + org + stream
//! metadata headers that the receiver expects.
//!
//! Env knobs (default values match `compose.dev.yml`):
//!   * `FRONTEND_OTLP_FORWARD_URL` — full upstream URL,
//!     default `http://localhost:5080/api/default/v1/traces`.
//!   * `FRONTEND_OTLP_FORWARD_HEADERS` — comma-separated `key=value`
//!     pairs, default the OpenObserve dev creds + org + stream.

use std::sync::OnceLock;

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use tracing::warn;

const DEFAULT_FORWARD_URL: &str = "http://localhost:5080/api/default/v1/traces";
const DEFAULT_FORWARD_HEADERS: &str =
    "authorization=Basic YWRtaW5AYWRtaW4uY29tOmFkbWlu,organization=default,stream-name=default";

struct ForwardConfig {
    url: String,
    headers: Vec<(HeaderName, HeaderValue)>,
}

fn forward_config() -> &'static ForwardConfig {
    static CFG: OnceLock<ForwardConfig> = OnceLock::new();
    CFG.get_or_init(|| {
        let url = std::env::var("FRONTEND_OTLP_FORWARD_URL")
            .unwrap_or_else(|_| DEFAULT_FORWARD_URL.into());
        let raw = std::env::var("FRONTEND_OTLP_FORWARD_HEADERS")
            .unwrap_or_else(|_| DEFAULT_FORWARD_HEADERS.into());
        let headers = parse_header_list(&raw);
        ForwardConfig { url, headers }
    })
}

fn parse_header_list(raw: &str) -> Vec<(HeaderName, HeaderValue)> {
    raw.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            let k = k.trim();
            let v = v.trim();
            if k.is_empty() {
                return None;
            }
            let name = HeaderName::from_bytes(k.as_bytes()).ok()?;
            let value = HeaderValue::from_str(v).ok()?;
            Some((name, value))
        })
        .collect()
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

pub async fn traces_handler(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let cfg = forward_config();
    // Pass the SDK's content-type through unchanged. The SDK sends
    // `application/x-protobuf` by default; if a future client sends
    // JSON instead, OpenObserve still accepts it.
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/x-protobuf")
        .to_string();

    let mut req = http_client()
        .post(&cfg.url)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .body(body.to_vec());
    for (k, v) in &cfg.headers {
        req = req.header(k.clone(), v.clone());
    }

    match req.send().await {
        Ok(res) => {
            let status = StatusCode::from_u16(res.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            let bytes = res.bytes().await.unwrap_or_default();
            (status, bytes).into_response()
        }
        Err(e) => {
            // Don't spam the log on every retry — once per failure
            // is enough for the operator to notice the upstream is
            // wrong / down.
            warn!(target: "telemetry_proxy", "OTLP forward to {} failed: {e}", cfg.url);
            (StatusCode::BAD_GATEWAY, "telemetry upstream unreachable").into_response()
        }
    }
}
