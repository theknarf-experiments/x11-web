import { useLiveQuery } from "@tanstack/react-db";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getAppContextMenuItems } from "./AppContextMenu";
import { ClientRenderer } from "./ClientRenderer";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { Dock, type DockProcess } from "./Dock";
import {
	patchWindow,
	processesCollection,
	raiseProcess,
	raiseWindow,
	setFocusedWindow,
	windowsCollection,
} from "./db";
import { GlobalMenuBar } from "./GlobalMenuBar";
import { InfiniteCanvas } from "./InfiniteCanvas";
import { decodeFrame } from "./rtcWire";
import { SettingsPanel } from "./SettingsPanel";
import type {
	FocusPolicy,
	InputEvent,
	MenuAction,
	WindowWmState,
} from "./types";
import {
	useAttachedWindowIds,
	useBackendSocket,
	useWorkspaceName,
} from "./useBackendSocket";
import { WindowFrame } from "./WindowFrame";
import {
	getAllPositions,
	setPosition as setWorkspacePosition,
	subscribe as subscribeWorkspace,
} from "./workspaceSync";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

function App() {
	const {
		connected,
		activeWorkspace,
		send,
		onWindowUpdate,
		onBell,
		onDataChannelMessage,
		setWorkspaceName,
		attachWindowToWorkspace,
		detachWindowFromWorkspace,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	} = useBackendSocket();
	const workspaceName = useWorkspaceName(activeWorkspace?.id ?? null);
	const attachedWindowIds = useAttachedWindowIds(activeWorkspace?.id ?? null);

	const { data: processes = [] } = useLiveQuery((q) =>
		q.from({ p: processesCollection }).select(({ p }) => p),
	);
	// DOM order matches collection insertion order; visual stacking comes
	// from `stackingOrder` via CSS z-index. Reordering the array would flip
	// the DOM whenever a window is raised, which breaks any hit-testing that
	// uses DOM-position locators (e.g. Playwright's `nth(...)`).
	const { data: windows = [] } = useLiveQuery((q) =>
		q.from({ w: windowsCollection }).select(({ w }) => w),
	);

	/** One renderer per top-level X11 window (keyed by window_id). */
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	/** Windows the user has locally closed (KillProcess in flight); filtered
	 *  out of the visible set until the backend's next WindowList drops them. */
	const [closedWindowIds, setClosedWindowIds] = useState<Set<string>>(
		() => new Set(),
	);
	/** Focus policy setting. */
	const [focusPolicy, setFocusPolicy] = useState<FocusPolicy>("click-to-focus");

	// Reap renderers and locally-closed entries for windows the
	// backend has dropped. Renderer creation happens lazily during
	// render so a window appears as soon as it lands in the list.
	useEffect(() => {
		const live = new Set(windows.map((w) => w.windowId));
		for (const wid of [...renderersRef.current.keys()]) {
			if (!live.has(wid)) renderersRef.current.delete(wid);
		}
		setClosedWindowIds((prev) => {
			let changed = false;
			const next = new Set(prev);
			for (const wid of prev) {
				if (!live.has(wid)) {
					next.delete(wid);
					changed = true;
				}
			}
			return changed ? next : prev;
		});
	}, [windows]);

	// Per-window thumbnail object URLs, keyed by window_id.
	// Driven by `Frame::WindowThumbnail` arrivals over the DC; the
	// previous URL is revoked when superseded so we don't leak.
	const [thumbnails, setThumbnails] = useState<Map<string, string>>(
		() => new Map(),
	);

	// Frame messages over the WebRTC DataChannel: decode the
	// Cap'n Proto envelope and dispatch on variant. PutImage routes
	// to the per-window renderer; WindowThumbnail updates the
	// thumbnail map (consumed by the dock's spawn popover).
	useEffect(() => {
		onDataChannelMessage((bytes) => {
			const msg = decodeFrame(bytes);
			if (!msg) return;
			if (msg.kind === "putImage") {
				const renderers = renderersRef.current;
				let r = renderers.get(msg.windowId);
				if (!r) {
					r = new ClientRenderer(msg.width || 1, msg.height || 1);
					renderers.set(msg.windowId, r);
				}
				r.pushPutImage(msg.x, msg.y, msg.width, msg.height, msg.data);
				return;
			}
			if (msg.kind === "thumbnail") {
				// Copy the bytes into a fresh ArrayBuffer because the
				// Uint8Array we got from the decoder is a view into
				// the reassembler's buffer, which gets reused.
				const copy = new Uint8Array(msg.data);
				const url = URL.createObjectURL(
					new Blob([copy], { type: "image/webp" }),
				);
				setThumbnails((prev) => {
					const next = new Map(prev);
					const old = next.get(msg.windowId);
					if (old) URL.revokeObjectURL(old);
					next.set(msg.windowId, url);
					return next;
				});
			}
		});
		return () => onDataChannelMessage(null);
	}, [onDataChannelMessage]);

	// Top-level Bell event — play an audible/visual notification.
	useEffect(() => {
		onBell((percent) => {
			try {
				const ctx = new AudioContext();
				const osc = ctx.createOscillator();
				const gain = ctx.createGain();
				osc.connect(gain);
				gain.connect(ctx.destination);
				osc.frequency.value = 800;
				gain.gain.value = Math.max(0.01, percent / 100);
				osc.start();
				osc.stop(ctx.currentTime + 0.1);
			} catch {
				document.body.style.backgroundColor = "#fff";
				setTimeout(() => {
					document.body.style.backgroundColor = "";
				}, 100);
			}
		});
		return () => onBell(null);
	}, [onBell]);

	// Mirror doc positions onto `WindowRow.{x,y}`. Drag handlers
	// optimistically patch the row for a smooth interactive feel,
	// but the doc is the cross-tab source of truth — when a sibling
	// tab moves a window, this listener picks up the sync and brings
	// our row in line.
	useEffect(() => {
		if (!activeWorkspace) return;
		const apply = () => {
			const positions = getAllPositions(activeWorkspace.id);
			for (const [windowId, pos] of positions) {
				const row = windowsCollection.state.get(windowId);
				if (!row || row.overrideRedirect) continue;
				if (row.x !== pos.x || row.y !== pos.y) {
					patchWindow(windowId, { x: pos.x, y: pos.y });
				}
			}
		};
		apply();
		return subscribeWorkspace(activeWorkspace.id, apply);
	}, [activeWorkspace]);

	function handleSpawn(sidecarId: string, command: string, args: string[]) {
		if (!activeWorkspace) return;
		send({
			type: "SpawnProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			workspace_id: activeWorkspace.id,
			command,
			args,
		});
	}

	const handleMove = useCallback(
		(windowId: string, x: number, y: number) => {
			const win = windowsCollection.state.get(windowId);
			// Top-level windows: position is user-collaborative
			// state in the workspace doc. Mutating the doc syncs to
			// every other peer; the local mirror loop also picks it
			// up and patches the row. Patch optimistically here too
			// so drag stays smooth without waiting for the round
			// trip through `notify`.
			if (win && !win.overrideRedirect && activeWorkspace) {
				setWorkspacePosition(activeWorkspace.id, windowId, x, y);
			}
			patchWindow(windowId, { x, y });
		},
		[activeWorkspace],
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
		raiseWindow(windowId);
		// Set focus locally too. For X11 this is redundant — the X
		// server forwards a `WindowUpdate::Focused` after it
		// processes the click — but the macOS sidecar doesn't emit
		// focus events, so without setting it client-side the
		// global menu bar would never light up for macOS windows.
		setFocusedWindow(windowId);
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

	/** Kill a process and locally hide all of its windows until the
	 *  backend's next `WindowList` drops them. */
	const handleCloseProcess = useCallback(
		(sidecarId: string, pid: number) => {
			const wids: string[] = [];
			for (const w of windowsCollection.state.values()) {
				if (w.sidecarId === sidecarId && w.pid === pid) {
					wids.push(w.windowId);
					const timer = animCursorTimersRef.current.get(w.windowId);
					if (timer) {
						clearInterval(timer);
						animCursorTimersRef.current.delete(w.windowId);
					}
				}
			}
			if (wids.length > 0) {
				setClosedWindowIds((prev) => new Set([...prev, ...wids]));
			}
			send({
				type: "KillProcess",
				request_id: nextRequestId(),
				sidecar_id: sidecarId,
				pid,
			});
		},
		[send],
	);

	/** Minimize a window (hide it, show in dock). */
	const handleMinimize = useCallback(
		(windowId: string, sidecarId: string) => {
			patchWindow(windowId, { wmState: "minimized" });
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "minimized" },
			});
		},
		[send],
	);

	/** Maximize a window (expand to fill viewport). */
	const handleMaximize = useCallback(
		(windowId: string, sidecarId: string) => {
			const win = windowsCollection.state.get(windowId);
			patchWindow(windowId, {
				wmState: "maximized",
				...(win ? { savedPosition: { x: win.x, y: win.y } } : {}),
			});
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "maximized" },
			});
		},
		[send],
	);

	/** Close a window gracefully via ICCCM WM_DELETE_WINDOW. */
	const handleCloseWindow = useCallback(
		(windowId: string, sidecarId: string) => {
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "close" },
			});
		},
		[send],
	);

	/** Restore a window from maximized/fullscreen/minimized to normal. */
	const handleRestore = useCallback(
		(windowId: string, sidecarId: string) => {
			const win = windowsCollection.state.get(windowId);
			const patch: { wmState: WindowWmState; x?: number; y?: number } = {
				wmState: "normal",
			};
			if (win?.savedPosition) {
				patch.x = win.savedPosition.x;
				patch.y = win.savedPosition.y;
			}
			patchWindow(windowId, patch);
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "normal" },
			});
		},
		[send],
	);

	/** Focus follows mouse: focus window on mouse enter. */
	const handleMouseEnterWindow = useCallback(
		(windowId: string) => {
			if (focusPolicy !== "focus-follows-mouse") return;
			raiseWindow(windowId);
			setFocusedWindow(windowId);
		},
		[focusPolicy],
	);

	// Deduplicate windows by process for the dock -- one entry per (sidecarId, pid)
	// Include minimized windows so they appear in the dock
	const dockProcesses = useMemo(() => {
		const seen = new Set<string>();
		const result: DockProcess[] = [];
		for (const w of windows) {
			if (w.pid === 0) continue;
			const key = `${w.sidecarId}:${w.pid}`;
			if (!seen.has(key)) {
				seen.add(key);
				const proc = processes.find(
					(p) => p.sidecar_id === w.sidecarId && p.pid === w.pid,
				);
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

	const focusedWindow = windows.find((w) => w.focused) ?? null;
	const focusedTitle = focusedWindow?.title ?? null;
	const focusedMenu =
		focusedWindow && focusedWindow.menu.length > 0 ? focusedWindow.menu : null;

	const handleMenuActivate = useCallback(
		(action: MenuAction) => {
			if (!focusedWindow) return;
			send({
				type: "InputEvent",
				sidecar_id: focusedWindow.sidecarId,
				window_id: focusedWindow.windowId,
				event: { kind: "MenuActivate", action },
			});
		},
		[focusedWindow, send],
	);

	const focusedAppContextMenuItems =
		focusedWindow && focusedWindow.pid > 0
			? getAppContextMenuItems(focusedWindow.sidecarId, focusedWindow.pid, {
					onClose: handleCloseProcess,
				})
			: null;

	const visibleWindows = useMemo(
		() =>
			windows.filter(
				(w) =>
					!closedWindowIds.has(w.windowId) &&
					attachedWindowIds.has(w.windowId),
			),
		[windows, closedWindowIds, attachedWindowIds],
	);

	// Block rendering until the backend has bound this session to a
	// workspace. The frontend asks for one (by URL hash, or fresh) on
	// WS open, so this gate sits up just for the request/response
	// roundtrip — but everything below (canvas, dock, window frames)
	// needs a workspace context.
	if (!activeWorkspace) {
		return (
			<div
				style={{
					position: "fixed",
					inset: 0,
					display: "flex",
					alignItems: "center",
					justifyContent: "center",
					color: "rgba(255, 255, 255, 0.6)",
					font: "13px system-ui, sans-serif",
					background: "#1a1a1a",
				}}
			>
				{connected ? "Loading workspace…" : "Connecting to backend…"}
			</div>
		);
	}

	return (
		<>
			<GlobalMenuBar
				focusedTitle={focusedTitle}
				workspaceName={workspaceName}
				onRenameWorkspace={(name) => {
					if (activeWorkspace) setWorkspaceName(activeWorkspace.id, name);
				}}
				menu={focusedMenu}
				onActivate={handleMenuActivate}
				appContextMenuItems={focusedAppContextMenuItems}
			/>
			<InfiniteCanvas
				onCanvasDrop={(point, event) => {
					const windowId = event.dataTransfer.getData(
						"application/x-x11web-window-id",
					);
					if (!windowId || !activeWorkspace) return;
					attachWindowToWorkspace(activeWorkspace.id, windowId);
					// Drop the new WindowFrame at the cursor's canvas
					// coordinate. The doc carries the position so
					// sibling tabs converge on the same drop point;
					// the local patch is the optimistic preview so
					// the frame appears at the drop point on the next
					// render without waiting for the doc-mirror.
					setWorkspacePosition(
						activeWorkspace.id,
						windowId,
						point.x,
						point.y,
					);
					patchWindow(windowId, { x: point.x, y: point.y });
				}}
			>
				{visibleWindows.map((win) => {
					// Lazy-create the renderer so a window appearing in
					// the authoritative list shows up immediately, before
					// the first PutImage arrives over the DC. Sync
					// the back buffer to the descriptor's authoritative
					// size on every render — `pushPutImage`'s built-in
					// resize is grow-only, so a shrinking window (e.g.
					// Calculator going from Scientific back to Basic)
					// otherwise leaves the canvas at the previous larger
					// size with stale pixels in the unused region.
					let renderer = renderersRef.current.get(win.windowId);
					if (!renderer) {
						renderer = new ClientRenderer(win.width || 1, win.height || 1);
						renderersRef.current.set(win.windowId, renderer);
					} else if (
						renderer.width !== win.width ||
						renderer.height !== win.height
					) {
						renderer.resize(win.width || 1, win.height || 1);
					}
					return (
						<div
							key={win.windowId}
							onMouseEnter={() => handleMouseEnterWindow(win.windowId)}
						>
							<WindowFrame
								clientId={win.windowId}
								title={win.title}
								x={win.x}
								y={win.y}
								zIndex={win.stackingOrder}
								color={win.color}
								renderer={renderer}
								overrideRedirect={win.overrideRedirect}
								resizable={win.resizable}
								wmState={win.wmState}
								onClose={() => handleCloseWindow(win.windowId, win.sidecarId)}
								onMove={(nx, ny) => handleMove(win.windowId, nx, ny)}
								onResize={(nw, nh) =>
									handleResize(win.windowId, win.sidecarId, nw, nh)
								}
								onInput={(event) =>
									handleInput(win.windowId, win.sidecarId, event)
								}
								onFocus={() => handleFocus(win.windowId)}
								onMinimize={() => handleMinimize(win.windowId, win.sidecarId)}
								onMaximize={() => handleMaximize(win.windowId, win.sidecarId)}
								onRestore={() => handleRestore(win.windowId, win.sidecarId)}
								borderWidth={win.borderWidth}
								borderPixel={win.borderPixel}
							/>
						</div>
					);
				})}
			</InfiniteCanvas>
			<Dock
				connected={connected}
				processes={dockProcesses}
				thumbnails={thumbnails}
				attachedWindowIds={attachedWindowIds}
				onSpawn={handleSpawn}
				onClose={handleCloseProcess}
				onFocusWindow={raiseProcess}
			/>
			<DiagnosticsPanel
				diagnostics={diagnostics}
				onDismiss={dismissDiagnostic}
				onClear={clearDiagnostics}
			/>
			<SettingsPanel
				focusPolicy={focusPolicy}
				onFocusPolicyChange={setFocusPolicy}
			/>
		</>
	);
}

export default App;
