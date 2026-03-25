import { useCallback, useEffect, useRef, useState } from "react";
import { ClientRenderer } from "./ClientRenderer";
import { Dock } from "./Dock";
import { InfiniteCanvas } from "./InfiniteCanvas";
import type { InputEvent } from "./types";
import { useBackendSocket } from "./useBackendSocket";
import { WindowFrame } from "./WindowFrame";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

interface CanvasWindow {
	clientId: string;
	pid: number;
	title: string;
	x: number;
	y: number;
}

let spawnCounter = 0;

function App() {
	const {
		connected,
		sidecars,
		processes,
		connectedProcesses,
		send,
		onDisplayUpdate,
	} = useBackendSocket();

	const [windows, setWindows] = useState<CanvasWindow[]>([]);
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());

	// Register display callback
	useEffect(() => {
		onDisplayUpdate((_sidecarId, clientId, update) => {
			const renderers = renderersRef.current;
			let r = renderers.get(clientId);
			if (!r) {
				r = new ClientRenderer(1024, 768);
				renderers.set(clientId, r);
			}
			r.pushUpdate(update);
		});
		return () => onDisplayUpdate(null);
	}, [onDisplayUpdate]);

	// When a new process connects, add a window for it
	useEffect(() => {
		const existing = new Set(windows.map((w) => w.clientId));
		for (const cp of connectedProcesses) {
			if (!existing.has(cp.clientId)) {
				const procList = processes[cp.sidecarId] || [];
				const proc = procList.find((p) => p.pid === cp.pid);
				const title = proc ? `${proc.command} (${cp.pid})` : `PID ${cp.pid}`;

				if (!renderersRef.current.has(cp.clientId)) {
					renderersRef.current.set(cp.clientId, new ClientRenderer(1024, 768));
				}

				const offset = spawnCounter++ * 30;
				const cx = window.innerWidth / 2 - 512 + offset;
				const cy = window.innerHeight / 2 - 384 + offset;

				setWindows((prev) => [
					...prev,
					{ clientId: cp.clientId, pid: cp.pid, title, x: cx, y: cy },
				]);
			}
		}
	}, [connectedProcesses, processes, windows]);

	function handleSpawn(sidecarId: string, command: string, args: string[]) {
		send({ type: "SubscribeDisplay", sidecar_id: sidecarId });
		send({
			type: "SpawnProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			command,
			args,
		});
	}

	function handleKill(clientId: string, pid: number, sidecarId: string) {
		send({
			type: "KillProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			pid,
		});
		setWindows((prev) => prev.filter((w) => w.clientId !== clientId));
	}

	const handleMove = useCallback((clientId: string, x: number, y: number) => {
		setWindows((prev) =>
			prev.map((w) => (w.clientId === clientId ? { ...w, x, y } : w)),
		);
	}, []);

	// Debounced resize — sends to X11 server
	const resizeTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

	const handleResize = useCallback(
		(
			clientId: string,
			sidecarId: string | undefined,
			width: number,
			height: number,
		) => {
			if (!sidecarId) return;
			clearTimeout(resizeTimerRef.current);
			resizeTimerRef.current = setTimeout(() => {
				send({
					type: "ResizeWindow",
					sidecar_id: sidecarId,
					client_id: clientId,
					width,
					height,
				});
			}, 100);
		},
		[send],
	);

	const handleInput = useCallback(
		(clientId: string, sidecarId: string, event: InputEvent) => {
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				client_id: clientId,
				event,
			});
		},
		[send],
	);

	function sidecarForClient(clientId: string): string | undefined {
		return connectedProcesses.find((p) => p.clientId === clientId)?.sidecarId;
	}

	return (
		<>
			<InfiniteCanvas>
				{windows.map((win) => {
					const renderer = renderersRef.current.get(win.clientId);
					if (!renderer) return null;
					const sidecarId = sidecarForClient(win.clientId);
					return (
						<WindowFrame
							key={win.clientId}
							clientId={win.clientId}
							title={win.title}
							x={win.x}
							y={win.y}
							renderer={renderer}
							onClose={() => {
								if (sidecarId) handleKill(win.clientId, win.pid, sidecarId);
							}}
							onMove={(nx, ny) => handleMove(win.clientId, nx, ny)}
							onResize={(nw, nh) =>
								handleResize(win.clientId, sidecarId, nw, nh)
							}
							onInput={(event) => {
								if (sidecarId) handleInput(win.clientId, sidecarId, event);
							}}
						/>
					);
				})}
			</InfiniteCanvas>
			<Dock connected={connected} sidecars={sidecars} onSpawn={handleSpawn} />
		</>
	);
}

export default App;
