import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";
import { initTelemetry, tracer } from "./telemetry.ts";

// Stand up OTel before the React tree mounts so the very first
// spans (workspace open, WS connect) have a live provider.
initTelemetry();

// Smoke-test span — emits one span per page load so we can see in
// OpenObserve that the SDK + backend proxy + ingest path are all
// alive. Cheap; removable once richer instrumentation lands.
tracer().startActiveSpan("frontend.boot", (span) => {
	span.setAttribute("user_agent", navigator.userAgent);
	span.end();
});

const root = document.getElementById("root");
if (!root) throw new Error("Root element not found");
createRoot(root).render(
	<StrictMode>
		<App />
	</StrictMode>,
);
