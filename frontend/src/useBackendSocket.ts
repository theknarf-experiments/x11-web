import { useCallback, useEffect, useRef, useState } from "react";
import type {
	BackendToFrontend,
	FrontendToBackend,
	ProcessInfo,
	SidecarInfo,
	WindowDescriptor,
	WindowUpdate,
} from "./types";

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

export interface ConnectedProcess {
	sidecarId: string;
	pid: number;
	clientId: string;
	command: string;
}

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

export function useBackendSocket() {
	const wsRef = useRef<WebSocket | null>(null);
	const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const disposed = useRef(false);
	const windowUpdateCallbackRef = useRef<WindowUpdateCallback | null>(null);
	const bellCallbackRef = useRef<BellCallback | null>(null);
	const [connected, setConnected] = useState(false);
	const [sidecars, setSidecars] = useState<SidecarInfo[]>([]);
	const [processes, setProcesses] = useState<Record<string, ProcessInfo[]>>({});
	const [connectedProcesses, setConnectedProcesses] = useState<
		ConnectedProcess[]
	>([]);
	const [windowList, setWindowList] = useState<WindowDescriptor[]>([]);
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
					case "SidecarList": {
						// The list is authoritative — replace state, then
						// prune any per-sidecar caches whose owner left.
						const liveIds = new Set(msg.sidecars.map((s) => s.id));
						setSidecars(msg.sidecars);
						setProcesses((prev) => {
							const next: typeof prev = {};
							for (const [id, ps] of Object.entries(prev)) {
								if (liveIds.has(id)) next[id] = ps;
							}
							return next;
						});
						setConnectedProcesses((prev) =>
							prev.filter((p) => liveIds.has(p.sidecarId)),
						);
						break;
					}
					case "ProcessList": {
						// Authoritative for one sidecar — replace its
						// per-sidecar list and rebuild the flat
						// cross-sidecar `connectedProcesses` array.
						const sidecarId = msg.sidecar_id;
						setProcesses((prev) => ({
							...prev,
							[sidecarId]: msg.processes,
						}));
						setConnectedProcesses((prev) => [
							...prev.filter((p) => p.sidecarId !== sidecarId),
							...msg.processes.map((p) => ({
								sidecarId,
								pid: p.pid,
								clientId: p.client_id,
								command: p.command,
							})),
						]);
						break;
					}
					case "WindowUpdate":
						windowUpdateCallbackRef.current?.(msg.update);
						break;
					case "WindowList":
						setWindowList(msg.windows);
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

	const onWindowUpdate = useCallback((cb: WindowUpdateCallback | null) => {
		windowUpdateCallbackRef.current = cb;
	}, []);

	const onBell = useCallback((cb: BellCallback | null) => {
		bellCallbackRef.current = cb;
	}, []);

	return {
		connected,
		sidecars,
		processes,
		connectedProcesses,
		windowList,
		send,
		onWindowUpdate,
		onBell,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	};
}
