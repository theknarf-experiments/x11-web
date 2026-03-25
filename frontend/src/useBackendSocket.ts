import { useCallback, useEffect, useRef, useState } from "react";
import type {
	BackendToFrontend,
	DisplayUpdate,
	FrontendToBackend,
	ProcessInfo,
	SidecarInfo,
} from "./types";

const WS_URL = import.meta.env.VITE_WS_URL || "ws://localhost:3001/ws/frontend";

export type DisplayUpdateCallback = (
	sidecarId: string,
	clientId: string,
	update: DisplayUpdate,
) => void;

export interface ConnectedProcess {
	sidecarId: string;
	pid: number;
	clientId: string;
}

export function useBackendSocket() {
	const wsRef = useRef<WebSocket | null>(null);
	const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const disposed = useRef(false);
	const displayCallbackRef = useRef<DisplayUpdateCallback | null>(null);
	const [connected, setConnected] = useState(false);
	const [sidecars, setSidecars] = useState<SidecarInfo[]>([]);
	const [processes, setProcesses] = useState<Record<string, ProcessInfo[]>>({});
	const [connectedProcesses, setConnectedProcesses] = useState<
		ConnectedProcess[]
	>([]);

	useEffect(() => {
		disposed.current = false;

		function connect() {
			if (disposed.current) return;

			const ws = new WebSocket(WS_URL);
			wsRef.current = ws;

			ws.onopen = () => setConnected(true);

			ws.onerror = () => {};

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
						setConnectedProcesses((prev) =>
							prev.filter((p) => p.sidecarId !== msg.sidecar_id),
						);
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
						setConnectedProcesses((prev) =>
							prev.filter((p) => p.pid !== msg.pid),
						);
						break;
					case "ProcessConnected":
						setConnectedProcesses((prev) => [
							...prev,
							{
								sidecarId: msg.sidecar_id,
								pid: msg.pid,
								clientId: msg.client_id,
							},
						]);
						break;
					case "DisplayUpdate":
						displayCallbackRef.current?.(
							msg.sidecar_id,
							msg.client_id,
							msg.update,
						);
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
	}, []);

	const send = useCallback((msg: FrontendToBackend) => {
		if (wsRef.current?.readyState === WebSocket.OPEN) {
			wsRef.current.send(JSON.stringify(msg));
		}
	}, []);

	const onDisplayUpdate = useCallback((cb: DisplayUpdateCallback | null) => {
		displayCallbackRef.current = cb;
	}, []);

	return {
		connected,
		sidecars,
		processes,
		connectedProcesses,
		send,
		onDisplayUpdate,
	};
}
