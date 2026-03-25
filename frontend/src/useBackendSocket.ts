import { useCallback, useEffect, useRef, useState } from "react";
import type {
	BackendToFrontend,
	DisplayUpdate,
	FrontendToBackend,
	ProcessInfo,
	SidecarInfo,
} from "./types";

const WS_URL = import.meta.env.VITE_WS_URL || "ws://localhost:3001/ws/frontend";

export function useBackendSocket() {
	const wsRef = useRef<WebSocket | null>(null);
	const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const disposed = useRef(false);
	const [connected, setConnected] = useState(false);
	const [sidecars, setSidecars] = useState<SidecarInfo[]>([]);
	const [processes, setProcesses] = useState<Record<string, ProcessInfo[]>>({});
	const [displayUpdates, setDisplayUpdates] = useState<
		Record<string, DisplayUpdate[]>
	>({});

	useEffect(() => {
		disposed.current = false;

		function connect() {
			if (disposed.current) return;

			const ws = new WebSocket(WS_URL);
			wsRef.current = ws;

			ws.onopen = () => setConnected(true);

			ws.onerror = () => {
				// Suppress console error — onclose handles reconnect
			};

			ws.onclose = () => {
				setConnected(false);
				if (!disposed.current) {
					reconnectTimer.current = setTimeout(connect, 3000);
				}
			};

			ws.onmessage = (event) => {
				const msg: BackendToFrontend = JSON.parse(event.data);

				switch (msg.type) {
					case "SidecarList":
						setSidecars(msg.sidecars);
						break;
					case "SidecarConnected":
						setSidecars((prev) => [
							...prev.filter((s) => s.id !== msg.sidecar.id),
							msg.sidecar,
						]);
						break;
					case "SidecarDisconnected":
						setSidecars((prev) => prev.filter((s) => s.id !== msg.sidecar_id));
						setProcesses((prev) => {
							const next = { ...prev };
							delete next[msg.sidecar_id];
							return next;
						});
						setDisplayUpdates((prev) => {
							const next = { ...prev };
							delete next[msg.sidecar_id];
							return next;
						});
						break;
					case "ProcessList":
						setProcesses((prev) => ({
							...prev,
							[msg.sidecar_id]: msg.processes,
						}));
						break;
					case "ProcessExited":
						setProcesses((prev) => ({
							...prev,
							[msg.sidecar_id]: (prev[msg.sidecar_id] || []).filter(
								(p) => p.pid !== msg.pid,
							),
						}));
						break;
					case "DisplayUpdate":
						setDisplayUpdates((prev) => ({
							...prev,
							[msg.sidecar_id]: [...(prev[msg.sidecar_id] || []), msg.update],
						}));
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
				// Don't close a socket that's still connecting — that triggers the
				// "WebSocket is closed before the connection is established" warning.
				// Instead, let it connect and then immediately close, or just detach.
				if (ws.readyState === WebSocket.OPEN) {
					ws.close();
				} else {
					// Detach handlers so the pending connect is a no-op
					ws.onopen = ws.onclose = ws.onerror = ws.onmessage = null;
				}
			}
		};
	}, []);

	const send = useCallback((msg: FrontendToBackend) => {
		if (wsRef.current?.readyState === WebSocket.OPEN) {
			wsRef.current.send(JSON.stringify(msg));
		}
	}, []);

	return { connected, sidecars, processes, displayUpdates, send };
}
