import { useCallback, useEffect, useRef, useState } from "react";
import {
	applyWindowList,
	type NewWindowSeed,
	patchWindow,
	replaceSidecarProcesses,
	replaceSidecars,
	setFocusedWindow,
	windowsCollection,
} from "./db";
import type {
	BackendToFrontend,
	FrontendToBackend,
	WindowDescriptor,
	WindowUpdate,
} from "./types";
import { colorForWindowId } from "./windowColors";

// Resolve order: ?ws=... query param > VITE_WS_URL build-time env > default.
// The query-param branch lets parallel e2e workers share a single built
// bundle but route each browser page to its own backend.
const WS_URL = (() => {
	if (typeof window !== "undefined") {
		const fromQuery = new URLSearchParams(window.location.search).get("ws");
		if (fromQuery) return fromQuery;
	}
	return import.meta.env.VITE_WS_URL || "ws://localhost:3001/ws/frontend";
})();

export type WindowUpdateCallback = (update: WindowUpdate) => void;
export type BellCallback = (percent: number) => void;

/**
 * Per-event diagnostic surfaced from the backend / WebSocket layer.
 * Rendered by `<DiagnosticsPanel>` so the user has *some* visibility
 * into errors that previously vanished into the void (sidecar errors,
 * socket errors).
 */
export interface Diagnostic {
	id: string;
	level: "info" | "warn" | "error";
	source: "ws" | "command" | "sidecar";
	message: string;
	timestamp: number;
}

const MAX_DIAGNOSTICS = 100;
let diagnosticCounter = 0;
function nextDiagnosticId() {
	return `diag-${++diagnosticCounter}-${Date.now()}`;
}

let spawnCounter = 0;

function seedForDescriptor(
	d: WindowDescriptor,
	send: (msg: FrontendToBackend) => void,
): NewWindowSeed {
	let x: number;
	let y: number;
	if (d.placed || d.override_redirect) {
		// Backend gave us an authoritative position (X11 server for popups,
		// cross-frontend tracked for top-level windows).
		x = d.x;
		y = d.y;
	} else {
		// First time *any* frontend has seen this top-level window — pick a
		// cascading position and broadcast it so other tabs converge.
		const idx = spawnCounter++;
		const offset = idx * 30;
		x = window.innerWidth / 4 + offset;
		y = window.innerHeight / 4 + offset;
		send({
			type: "UpdateWindowPosition",
			window_id: d.window_id,
			x,
			y,
		});
	}
	return {
		x,
		y,
		color: d.override_redirect ? "transparent" : colorForWindowId(d.window_id),
		title: d.command || `PID ${d.pid}`,
		cursor: "default",
		wmState: "normal",
	};
}

/** Apply a `WindowUpdate` to the windows collection where it represents a
 *  persistent UI-state change. Returns true if the hook handled the update
 *  (the App can still observe it via the `onWindowUpdate` callback for
 *  side-effect kinds like `PutImage` and animated/bitmap cursors). */
function applyWindowUpdate(update: WindowUpdate) {
	switch (update.kind) {
		case "TitleChanged":
			patchWindow(update.window_id, { title: update.title });
			break;
		case "CursorChanged":
			patchWindow(update.window_id, { cursor: update.cursor });
			break;
		case "Focused":
			setFocusedWindow(update.window_id);
			break;
		case "PositionChanged":
			patchWindow(update.window_id, { x: update.x, y: update.y });
			break;
		case "MenuStructure":
			patchWindow(update.window_id, { menu: update.menu });
			break;
		case "StateChanged":
			if (update.state === "maximized" || update.state === "fullscreen") {
				// Save the current position so Restore can put the window
				// back where it was before the WM transition.
				const existing = windowsCollection.state.get(update.window_id);
				if (existing) {
					patchWindow(update.window_id, {
						wmState: update.state,
						savedPosition: { x: existing.x, y: existing.y },
					});
				}
			} else {
				patchWindow(update.window_id, { wmState: update.state });
			}
			break;
		// PutImage / CursorBitmap / CursorAnimated are handled by App.tsx
		// (renderers and async cursor decoding live there).
	}
}

export function useBackendSocket() {
	const wsRef = useRef<WebSocket | null>(null);
	const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const disposed = useRef(false);
	const windowUpdateCallbackRef = useRef<WindowUpdateCallback | null>(null);
	const bellCallbackRef = useRef<BellCallback | null>(null);
	const sendRef = useRef<(msg: FrontendToBackend) => void>(() => {});
	const [connected, setConnected] = useState(false);
	const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);

	const pushDiagnostic = useCallback(
		(d: Omit<Diagnostic, "id" | "timestamp">) => {
			setDiagnostics((prev) => {
				const next = [
					...prev,
					{ ...d, id: nextDiagnosticId(), timestamp: Date.now() },
				];
				// Keep only the most recent N entries.
				return next.length > MAX_DIAGNOSTICS
					? next.slice(next.length - MAX_DIAGNOSTICS)
					: next;
			});
		},
		[],
	);

	const dismissDiagnostic = useCallback((id: string) => {
		setDiagnostics((prev) => prev.filter((d) => d.id !== id));
	}, []);

	const clearDiagnostics = useCallback(() => {
		setDiagnostics([]);
	}, []);

	useEffect(() => {
		disposed.current = false;

		function connect() {
			if (disposed.current) return;

			const ws = new WebSocket(WS_URL);
			wsRef.current = ws;

			ws.onopen = () => setConnected(true);

			ws.onerror = () => {
				pushDiagnostic({
					level: "error",
					source: "ws",
					message: `WebSocket error connecting to ${WS_URL}`,
				});
			};

			ws.onclose = (event) => {
				setConnected(false);
				if (!disposed.current) {
					pushDiagnostic({
						level: "warn",
						source: "ws",
						message: `WebSocket closed (code ${event.code}); reconnecting in 3s`,
					});
					reconnectTimer.current = setTimeout(connect, 3000);
				}
			};

			ws.onmessage = (event) => {
				const msg: BackendToFrontend = JSON.parse(event.data);

				switch (msg.type) {
					case "SidecarList":
						replaceSidecars(msg.sidecars);
						break;
					case "ProcessList":
						replaceSidecarProcesses(msg.sidecar_id, msg.processes);
						break;
					case "WindowList":
						applyWindowList(msg.windows, (d) =>
							seedForDescriptor(d, sendRef.current),
						);
						break;
					case "WindowUpdate":
						applyWindowUpdate(msg.update);
						windowUpdateCallbackRef.current?.(msg.update);
						break;
					case "Bell":
						bellCallbackRef.current?.(msg.percent);
						break;
					case "CommandResult":
						pushDiagnostic({
							level: msg.success ? "info" : "error",
							source: "command",
							message: msg.message || (msg.success ? "OK" : "command failed"),
						});
						break;
				}
			};
		}

		connect();

		return () => {
			disposed.current = true;
			clearTimeout(reconnectTimer.current);
			const ws = wsRef.current;
			if (ws) {
				if (ws.readyState === WebSocket.OPEN) {
					ws.close();
				} else {
					ws.onopen = ws.onclose = ws.onerror = ws.onmessage = null;
				}
			}
		};
		// `pushDiagnostic` is a useCallback with [] deps and is therefore
		// stable for the lifetime of the component, so this effect still
		// runs only once on mount.
	}, [pushDiagnostic]);

	const send = useCallback((msg: FrontendToBackend) => {
		if (wsRef.current?.readyState === WebSocket.OPEN) {
			wsRef.current.send(JSON.stringify(msg));
		}
	}, []);
	sendRef.current = send;

	const onWindowUpdate = useCallback((cb: WindowUpdateCallback | null) => {
		windowUpdateCallbackRef.current = cb;
	}, []);

	const onBell = useCallback((cb: BellCallback | null) => {
		bellCallbackRef.current = cb;
	}, []);

	return {
		connected,
		send,
		onWindowUpdate,
		onBell,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	};
}
