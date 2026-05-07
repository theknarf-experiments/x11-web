/**
 * Browser-side OpenTelemetry init. Mirrors the Rust binaries:
 *   * one process-wide `WebTracerProvider`,
 *   * an OTLP/HTTP exporter posting protobuf to a same-origin
 *     proxy on the backend (`/api/telemetry/v1/traces`), so the
 *     browser never needs OpenObserve credentials and CORS stays
 *     a non-issue,
 *   * `service.name=x11-web-frontend` so OpenObserve groups these
 *     spans the same way it groups the backend's.
 *
 * Same env-gating as the Rust side: with no exporter URL configured
 * we still register a provider but the BatchSpanProcessor's queued
 * spans will just sit there. Keeping the SDK live unconditionally
 * means `tracer.startActiveSpan(...)` calls scattered through the
 * UI never have to branch.
 */

import { type Attributes, trace } from "@opentelemetry/api";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { resourceFromAttributes } from "@opentelemetry/resources";
import { BatchSpanProcessor, WebTracerProvider } from "@opentelemetry/sdk-trace-web";
import { ATTR_SERVICE_NAME, ATTR_SERVICE_VERSION } from "@opentelemetry/semantic-conventions";
import type { FrontendToBackend } from "./types";

const SERVICE_NAME = "x11-web-frontend";
const TRACER_NAME = SERVICE_NAME;

let initialised = false;

export function initTelemetry(): void {
	if (initialised) return;
	initialised = true;

	const exporter = new OTLPTraceExporter({
		// Same-origin proxy on the backend — see the matching
		// route in `crates/backend/src/main.rs`. Using a relative
		// URL means dev (Vite proxy / direct backend) and prod
		// (backend serves the SPA) both Just Work.
		url: "/api/telemetry/v1/traces",
	});

	const provider = new WebTracerProvider({
		resource: resourceFromAttributes({
			[ATTR_SERVICE_NAME]: SERVICE_NAME,
			[ATTR_SERVICE_VERSION]: import.meta.env.VITE_APP_VERSION ?? "0.0.0-dev",
		}),
		spanProcessors: [new BatchSpanProcessor(exporter)],
	});
	provider.register();
}

export function tracer() {
	return trace.getTracer(TRACER_NAME);
}

/**
 * Per-variant attributes for `frontend.ws_send.<variant>` spans.
 * Picks the IDs you'd actually filter by in OpenObserve when
 * looking at "what did the user just do" — sidecar/window/workspace
 * IDs, command name, request_id for matching the eventual
 * `ProcessSpawned` reply, etc. Long opaque blobs (SDP, ICE
 * candidate strings) are intentionally omitted.
 */
export function spanAttrsFor(msg: FrontendToBackend): Attributes {
	switch (msg.type) {
		case "OpenWorkspace":
			return msg.id ? { "workspace.id": msg.id } : {};
		case "SpawnProcess":
			return {
				"request.id": msg.request_id,
				"sidecar.id": msg.sidecar_id,
				"workspace.id": msg.workspace_id,
				command: msg.command,
				"args.count": msg.args.length,
			};
		case "KillProcess":
			return {
				"request.id": msg.request_id,
				"sidecar.id": msg.sidecar_id,
				pid: msg.pid,
			};
		case "InputEvent":
			return {
				"sidecar.id": msg.sidecar_id,
				"window.id": msg.window_id,
				"event.kind": (msg.event as { type?: string }).type ?? "unknown",
			};
		case "ResizeWindow":
			return {
				"sidecar.id": msg.sidecar_id,
				"window.id": msg.window_id,
				width: msg.width,
				height: msg.height,
			};
		default:
			return {};
	}
}
