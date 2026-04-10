import {
	startTransition,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { ClientRenderer } from "./ClientRenderer";
import { Dock, type DockProcess } from "./Dock";
import { GlobalMenuBar } from "./GlobalMenuBar";
import { InfiniteCanvas } from "./InfiniteCanvas";
import type { InputEvent } from "./types";
import { useBackendSocket } from "./useBackendSocket";
import { WindowFrame } from "./WindowFrame";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

/**
 * One WindowFrame per top-level mapped X11 window.
 * Multiple windows may share the same clientId (and thus the same renderer).
 */
interface CanvasWindow {
	windowId: string;
	clientId: string;
	sidecarId: string;
	pid: number;
	title: string;
	x: number;
	y: number;
	color: string;
	zIndex: number;
	cursor: string;
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
	/** UUID of the currently X11-focused top-level window, or null. */
	const [focusedWindowId, setFocusedWindowId] = useState<string | null>(null);
	/** One renderer per top-level X11 window (keyed by window_id as string). */
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	const closedWindowsRef = useRef<Set<string>>(new Set());
	/** Map clientId -> { sidecarId, pid, command } for process association. */
	const clientInfoRef = useRef<
		Map<string, { sidecarId: string; pid: number; command: string }>
	>(new Map());
	/** Track which sidecars we've already subscribed to. */
	const subscribedRef = useRef<Set<string>>(new Set());
	/** Ref to always-current processes map (avoids stale closures in callbacks). */
	const processesRef = useRef(processes);
	processesRef.current = processes;
	const initialWindowStatesRef = useRef(initialWindowStates);
	initialWindowStatesRef.current = initialWindowStates;

	// Keep clientInfoRef in sync with connectedProcesses
	useEffect(() => {
		for (const cp of connectedProcesses) {
			clientInfoRef.current.set(cp.clientId, {
				sidecarId: cp.sidecarId,
				pid: cp.pid,
				command: cp.command,
			});
			// Auto-subscribe to display updates and request process list
			if (!subscribedRef.current.has(cp.sidecarId)) {
				subscribedRef.current.add(cp.sidecarId);
				send({ type: "SubscribeDisplay", sidecar_id: cp.sidecarId });
				send({
					type: "ListProcesses",
					request_id: nextRequestId(),
					sidecar_id: cp.sidecarId,
				});
			}
		}

		// Update any windows that were created before ProcessConnected arrived
		// (fixes the race where WindowMapped arrives before we know the PID)
		setWindows((prev) => {
			let changed = false;
			const next = prev.map((w) => {
				if (w.pid === 0 || w.title.startsWith("PID ")) {
					const info = clientInfoRef.current.get(w.clientId);
					if (info && info.pid !== 0) {
						const title = info.command
							|| processes[info.sidecarId]?.find((p) => p.pid === info.pid)?.command
							|| w.title;
						if (w.pid !== info.pid || w.title !== title) {
							changed = true;
							return {
								...w,
								pid: info.pid,
								sidecarId: info.sidecarId,
								title,
							};
						}
					}
				}
				return w;
			});
			return changed ? next : prev;
		});
	}, [connectedProcesses, send, processes]);

	// Update window titles when process list changes
	useEffect(() => {
		setWindows((prev) => {
			let changed = false;
			const next = prev.map((w) => {
				const procList = processes[w.sidecarId] || [];
				const proc = procList.find((p) => p.pid === w.pid);
				if (proc && w.title !== proc.command) {
					changed = true;
					return { ...w, title: proc.command };
				}
				return w;
			});
			return changed ? next : prev;
		});
	}, [processes]);

	// Register display callback — creates WindowFrames on WindowMapped
	useEffect(() => {
		onDisplayUpdate((sidecarId, clientId, update) => {
			// Title changes update the matching window
			if (update.kind === "TitleChanged") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, title: update.title }
							: w,
					),
				);
			}

			// Cursor changes update the matching window
			if (update.kind === "CursorChanged") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, cursor: update.cursor }
							: w,
					),
				);
			}

			// WindowMapped with is_top_level — create a WindowFrame
			if (update.kind === "WindowMapped" && update.is_top_level) {
				const windowId = update.window_id;
				if (closedWindowsRef.current.has(windowId)) return;

				setWindows((prev) => {
					if (prev.some((w) => w.windowId === windowId)) return prev;

					const info = clientInfoRef.current.get(clientId);
					const pid = info?.pid ?? 0;
					const sid = info?.sidecarId ?? sidecarId;
					const command = info?.command;
					const title = command
						|| processesRef.current[sid]?.find((p) => p.pid === pid)?.command
						|| `PID ${pid}`;

					const saved = initialWindowStatesRef.current.find(
						(ws) => ws.clientId === clientId,
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

					if (!saved) {
						send({
							type: "UpdateWindowState",
							client_id: clientId,
							sidecar_id: sid,
							x: cx,
							y: cy,
							color,
						});
					}

					return [
						...prev,
						{
							windowId,
							clientId,
							sidecarId: sid,
							pid,
							title,
							x: cx,
							y: cy,
							color,
							zIndex: nextZIndex++,
							cursor: "default",
						},
					];
				});
			}

			// WindowUnmapped — hide the WindowFrame
			if (update.kind === "WindowUnmapped") {
				setWindows((prev) => {
					if (!prev.some((w) => w.windowId === update.window_id))
						return prev;
					return prev.filter((w) => w.windowId !== update.window_id);
				});
				setFocusedWindowId((prev) =>
					prev === update.window_id ? null : prev,
				);
			}

			// WindowDestroyed — remove frame and renderer
			if (update.kind === "WindowDestroyed") {
				renderersRef.current.delete(update.window_id);
				setWindows((prev) => {
					if (!prev.some((w) => w.windowId === update.window_id))
						return prev;
					return prev.filter((w) => w.windowId !== update.window_id);
				});
				setFocusedWindowId((prev) =>
					prev === update.window_id ? null : prev,
				);
			}

			// WindowFocused — the X11 server tells us which top-level
			// window has input focus. Drives the global menu bar.
			if (update.kind === "WindowFocused") {
				setFocusedWindowId(update.window_id);
			}

			// Route display updates to the per-window renderer.
			// The server composites children into parents and only sends
			// PutImage for top-level windows, so we key renderers by window_id.
			const windowId = "window_id" in update ? update.window_id : undefined;
			if (windowId == null) return;

			const key = windowId;
			const renderers = renderersRef.current;
			let r = renderers.get(key);
			if (!r) {
				const w = "width" in update ? (update as { width: number }).width : 1;
				const h = "height" in update ? (update as { height: number }).height : 1;
				r = new ClientRenderer(w || 1, h || 1);
				renderers.set(key, r);
			}
			r.pushUpdate(update);
		});
		return () => onDisplayUpdate(null);
	// biome-ignore lint/correctness/useExhaustiveDependencies: initialWindowStates used via ref to avoid re-registering callback
	}, [onDisplayUpdate, send]);

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
		if (!subscribedRef.current.has(sidecarId)) {
			subscribedRef.current.add(sidecarId);
			send({ type: "SubscribeDisplay", sidecar_id: sidecarId });
		}
		send({
			type: "SpawnProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			command,
			args,
		});
	}

	const handleMove = useCallback(
		(windowId: string, x: number, y: number) => {
			setWindows((prev) => {
				const win = prev.find((w) => w.windowId === windowId);
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
				return prev.map((w) =>
					w.windowId === windowId ? { ...w, x, y } : w,
				);
			});
		},
		[send],
	);

	const resizeTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

	const handleResize = useCallback(
		(windowId: string, sidecarId: string, width: number, height: number) => {
			clearTimeout(resizeTimerRef.current);
			resizeTimerRef.current = setTimeout(() => {
				send({
					type: "ResizeWindow",
					sidecar_id: sidecarId,
					window_id: windowId,
					width,
					height,
				});
			}, 100);
		},
		[send],
	);

	const handleFocus = useCallback((windowId: string) => {
		setWindows((prev) =>
			prev.map((w) =>
				w.windowId === windowId ? { ...w, zIndex: nextZIndex++ } : w,
			),
		);
	}, []);

	const handleInput = useCallback(
		(windowId: string, sidecarId: string, event: InputEvent) => {
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event,
			});
		},
		[send],
	);

	// Deduplicate windows by process for the dock — one entry per (sidecarId, pid)
	const dockProcesses = useMemo(() => {
		const seen = new Set<string>();
		const result: DockProcess[] = [];
		for (const w of windows) {
			if (w.pid === 0) continue; // Skip windows with unknown process
			const key = `${w.sidecarId}:${w.pid}`;
			if (!seen.has(key)) {
				seen.add(key);
				const procList = processes[w.sidecarId] || [];
				const proc = procList.find((p) => p.pid === w.pid);
				result.push({
					sidecarId: w.sidecarId,
					pid: w.pid,
					title: proc ? proc.command : w.title,
					color: w.color,
				});
			}
		}
		return result;
	}, [windows, processes]);

	const focusedTitle =
		windows.find((w) => w.windowId === focusedWindowId)?.title ?? null;

	return (
		<>
			<GlobalMenuBar focusedTitle={focusedTitle} />
			<InfiniteCanvas>
				{windows.map((win) => {
					// Use the per-client renderer (shared across all windows from the same client)
					const renderer = renderersRef.current.get(win.windowId);
					if (!renderer) return null;
					return (
						<WindowFrame
							key={win.windowId}
							clientId={win.windowId}
							title={win.title}
							x={win.x}
							y={win.y}
							zIndex={win.zIndex}
							color={win.color}
							cursor={win.cursor}
							renderer={renderer}
							onClose={() => {
								// Kill process — removes ALL windows for this process
								const matching = windows.filter(
									(w) =>
										w.sidecarId === win.sidecarId && w.pid === win.pid,
								);
								for (const w of matching)
									closedWindowsRef.current.add(w.windowId);
								send({
									type: "KillProcess",
									request_id: nextRequestId(),
									sidecar_id: win.sidecarId,
									pid: win.pid,
								});
								startTransition(() => {
									setWindows((prev) =>
										prev.filter(
											(w) =>
												!(
													w.sidecarId === win.sidecarId &&
													w.pid === win.pid
												),
										),
									);
								});
							}}
							onMove={(nx, ny) => handleMove(win.windowId, nx, ny)}
							onResize={(nw, nh) =>
								handleResize(win.windowId, win.sidecarId, nw, nh)
							}
							onInput={(event) =>
								handleInput(win.windowId, win.sidecarId, event)
							}
							onFocus={() => handleFocus(win.windowId)}
						/>
					);
				})}
			</InfiniteCanvas>
			<Dock
				connected={connected}
				sidecars={sidecars}
				processes={dockProcesses}
				onSpawn={handleSpawn}
				onClose={(sidecarId, pid) => {
					const matching = windows.filter(
						(w) => w.sidecarId === sidecarId && w.pid === pid,
					);
					for (const w of matching)
						closedWindowsRef.current.add(w.windowId);
					send({
						type: "KillProcess",
						request_id: nextRequestId(),
						sidecar_id: sidecarId,
						pid,
					});
					startTransition(() => {
						setWindows((prev) =>
							prev.filter(
								(w) => !(w.sidecarId === sidecarId && w.pid === pid),
							),
						);
					});
				}}
				onFocusWindow={(sidecarId, pid) => {
					// Bring all windows for this process to front
					setWindows((prev) =>
						prev.map((w) =>
							w.sidecarId === sidecarId && w.pid === pid
								? { ...w, zIndex: nextZIndex++ }
								: w,
						),
					);
				}}
			/>
		</>
	);
}

export default App;
