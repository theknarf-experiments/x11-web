import {
	startTransition,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";
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
	sidecarId: string;
	pid: number;
	title: string;
	x: number;
	y: number;
	color: string;
	zIndex: number;
}

const PASTEL_COLORS = [
	"#fce4ec", // pink
	"#e8eaf6", // indigo
	"#e0f2f1", // teal
	"#fff9c4", // yellow
	"#f3e5f5", // purple
	"#e8f5e9", // green
	"#fff3e0", // orange
	"#e1f5fe", // light blue
	"#fbe9e7", // deep orange
	"#f1f8e9", // light green
	"#ede7f6", // deep purple
	"#e0f7fa", // cyan
];

let spawnCounter = 0;
let nextZIndex = 1;

function App() {
	const {
		connected,
		sidecars,
		processes,
		connectedProcesses,
		initialWindowStates,
		send,
		onDisplayUpdate,
		onWindowStateChange,
	} = useBackendSocket();

	const [windows, setWindows] = useState<CanvasWindow[]>([]);
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	const closedClientsRef = useRef<Set<string>>(new Set());

	// Register display callback
	useEffect(() => {
		onDisplayUpdate((_sidecarId, clientId, update) => {
			// Handle title changes
			if (update.kind === "TitleChanged") {
				setWindows((prev) =>
					prev.map((w) =>
						w.clientId === clientId ? { ...w, title: update.title } : w,
					),
				);
				return;
			}

			const renderers = renderersRef.current;
			let r = renderers.get(clientId);
			if (!r) {
				r = new ClientRenderer(1, 1);
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
			if (
				!existing.has(cp.clientId) &&
				!closedClientsRef.current.has(cp.clientId)
			) {
				const procList = processes[cp.sidecarId] || [];
				const proc = procList.find((p) => p.pid === cp.pid);
				const title = proc ? `${proc.command} (${cp.pid})` : `PID ${cp.pid}`;

				if (!renderersRef.current.has(cp.clientId)) {
					renderersRef.current.set(cp.clientId, new ClientRenderer(1, 1));
				}

				// Check if we have persisted state for this window
				const saved = initialWindowStates.find(
					(ws) => ws.clientId === cp.clientId,
				);

				let cx: number;
				let cy: number;
				let color: string;
				if (saved) {
					cx = saved.x;
					cy = saved.y;
					color = saved.color;
				} else {
					const idx = spawnCounter++;
					const offset = idx * 30;
					cx = window.innerWidth / 4 + offset;
					cy = window.innerHeight / 4 + offset;
					color = PASTEL_COLORS[idx % PASTEL_COLORS.length];
				}

				// Auto-subscribe to display updates for this sidecar
				send({
					type: "SubscribeDisplay",
					sidecar_id: cp.sidecarId,
				});

				setWindows((prev) => [
					...prev,
					{
						clientId: cp.clientId,
						sidecarId: cp.sidecarId,
						pid: cp.pid,
						title,
						x: cx,
						y: cy,
						color,
						zIndex: nextZIndex++,
					},
				]);

				// Send initial window state to backend for new windows
				if (!saved) {
					send({
						type: "UpdateWindowState",
						client_id: cp.clientId,
						sidecar_id: cp.sidecarId,
						x: cx,
						y: cy,
						color,
					});
				}
			}
		}
	}, [connectedProcesses, processes, windows, initialWindowStates, send]);

	// Handle window state changes from other tabs
	useEffect(() => {
		onWindowStateChange((clientId, x, y, color) => {
			setWindows((prev) =>
				prev.map((w) => (w.clientId === clientId ? { ...w, x, y, color } : w)),
			);
		});
		return () => onWindowStateChange(null);
	}, [onWindowStateChange]);

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
		closedClientsRef.current.add(clientId);
		send({
			type: "KillProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			pid,
		});
		startTransition(() => {
			setWindows((prev) => prev.filter((w) => w.clientId !== clientId));
		});
	}

	const handleMove = useCallback(
		(clientId: string, x: number, y: number) => {
			setWindows((prev) => {
				const win = prev.find((w) => w.clientId === clientId);
				if (win) {
					send({
						type: "UpdateWindowState",
						client_id: win.clientId,
						sidecar_id: win.sidecarId,
						x,
						y,
						color: win.color,
					});
				}
				return prev.map((w) => (w.clientId === clientId ? { ...w, x, y } : w));
			});
		},
		[send],
	);

	// Debounced resize — sends to X11 server
	const resizeTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

	const handleResize = useCallback(
		(clientId: string, sidecarId: string, width: number, height: number) => {
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

	const handleFocus = useCallback((clientId: string) => {
		setWindows((prev) =>
			prev.map((w) =>
				w.clientId === clientId ? { ...w, zIndex: nextZIndex++ } : w,
			),
		);
	}, []);

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

	return (
		<>
			<InfiniteCanvas>
				{windows.map((win) => {
					const renderer = renderersRef.current.get(win.clientId);
					if (!renderer) return null;
					return (
						<WindowFrame
							key={win.clientId}
							clientId={win.clientId}
							title={win.title}
							x={win.x}
							y={win.y}
							zIndex={win.zIndex}
							color={win.color}
							renderer={renderer}
							onClose={() => handleKill(win.clientId, win.pid, win.sidecarId)}
							onMove={(nx, ny) => handleMove(win.clientId, nx, ny)}
							onResize={(nw, nh) =>
								handleResize(win.clientId, win.sidecarId, nw, nh)
							}
							onInput={(event) =>
								handleInput(win.clientId, win.sidecarId, event)
							}
							onFocus={() => handleFocus(win.clientId)}
						/>
					);
				})}
			</InfiniteCanvas>
			<Dock
				connected={connected}
				sidecars={sidecars}
				windows={windows.map((w) => ({
					clientId: w.clientId,
					sidecarId: w.sidecarId,
					title: w.title,
					color: w.color,
				}))}
				onSpawn={handleSpawn}
				onClose={(clientId) => {
					const win = windows.find((w) => w.clientId === clientId);
					if (win) handleKill(win.clientId, win.pid, win.sidecarId);
				}}
				onFocusWindow={(_clientId) => {
					// TODO: scroll canvas to center on this window
				}}
			/>
		</>
	);
}

export default App;
