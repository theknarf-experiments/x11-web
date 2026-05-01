import { inflateRaw } from "pako";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getAppContextMenuItems } from "./AppContextMenu";
import { ClientRenderer } from "./ClientRenderer";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { Dock, type DockProcess } from "./Dock";
import { GlobalMenuBar } from "./GlobalMenuBar";
import { InfiniteCanvas } from "./InfiniteCanvas";
import { SettingsPanel } from "./SettingsPanel";
import type {
	AnimCursorFrame,
	FocusPolicy,
	InputEvent,
	MenuAction,
	MenuItem,
	WindowWmState,
} from "./types";
import { useBackendSocket } from "./useBackendSocket";
import { WindowFrame } from "./WindowFrame";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

/**
 * One WindowFrame per top-level mapped X11 window.
 * Each window has its own renderer, keyed by window_id.
 */
interface CanvasWindow {
	windowId: string;
	sidecarId: string;
	pid: number;
	title: string;
	x: number;
	y: number;
	color: string;
	zIndex: number;
	cursor: string;
	overrideRedirect: boolean;
	/** Current WM state. */
	wmState: WindowWmState;
	/** Saved position before maximize/fullscreen. */
	savedPosition?: { x: number; y: number };
	/** X11 border width in pixels. */
	borderWidth: number;
	/** X11 border color (ARGB32). */
	borderPixel: number;
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

/**
 * Pick a tint from `PASTEL_COLORS` deterministically from a window UUID.
 * Same window_id → same colour across browser tabs / reloads, so no
 * cross-frontend syncing is required.
 */
function colorForWindowId(windowId: string): string {
	let hash = 0;
	for (let i = 0; i < windowId.length; i++) {
		hash = ((hash << 5) - hash + windowId.charCodeAt(i)) | 0;
	}
	const idx = Math.abs(hash) % PASTEL_COLORS.length;
	return PASTEL_COLORS[idx];
}

/** Convert ARGB pixel data to a CSS cursor URL data-uri. Returns a promise. */
function argbToCursorUrl(
	data: string,
	width: number,
	height: number,
	hotX: number,
	hotY: number,
): Promise<string> {
	const canvas = new OffscreenCanvas(width, height);
	const ctx = canvas.getContext("2d")!;
	const binaryStr = atob(data);
	const compressed = new Uint8Array(binaryStr.length);
	for (let i = 0; i < binaryStr.length; i++) {
		compressed[i] = binaryStr.charCodeAt(i);
	}
	let rawData: Uint8Array;
	try {
		rawData = inflateRaw(compressed);
	} catch {
		rawData = compressed;
	}
	const imageData = ctx.createImageData(width, height);
	imageData.data.set(rawData.subarray(0, imageData.data.length));
	ctx.putImageData(imageData, 0, 0);
	return canvas.convertToBlob({ type: "image/png" }).then((blob) => {
		return new Promise<string>((resolve) => {
			const reader = new FileReader();
			reader.onloadend = () => {
				const dataUrl = reader.result as string;
				resolve(`url(${dataUrl}) ${hotX} ${hotY}, auto`);
			};
			reader.readAsDataURL(blob);
		});
	});
}

function App() {
	const {
		connected,
		sidecars,
		processes,
		windowList,
		send,
		onWindowUpdate,
		onBell,
		onClipboardData,
		onClipboardOffer,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	} = useBackendSocket();

	const [windows, setWindows] = useState<CanvasWindow[]>([]);
	/** UUID of the currently X11-focused top-level window, or null. */
	const [focusedWindowId, setFocusedWindowId] = useState<string | null>(null);
	/** Per-window menu trees mirrored from GTK / Qt apps via the sidecar. */
	const [menus, setMenus] = useState<Map<string, MenuItem[]>>(new Map());
	/** One renderer per top-level X11 window (keyed by window_id as string). */
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	const closedWindowsRef = useRef<Set<string>>(new Set());

	/** Animated cursor timers: windowId -> interval handle. */
	const animCursorTimersRef = useRef<Map<string, ReturnType<typeof setInterval>>>(new Map());

	/** Focus policy setting. */
	const [focusPolicy, setFocusPolicy] = useState<FocusPolicy>("click-to-focus");

	// Clean up animated cursor timers on unmount
	useEffect(() => {
		return () => {
			for (const timer of animCursorTimersRef.current.values()) {
				clearInterval(timer);
			}
		};
	}, []);

	// Send ResizeScreen to all connected sidecars when the viewport resizes.
	const sidecarsRef = useRef(sidecars);
	sidecarsRef.current = sidecars;
	useEffect(() => {
		let timer: ReturnType<typeof setTimeout> | undefined;
		const sendScreenSize = () => {
			const w = Math.round(window.innerWidth);
			const h = Math.round(window.innerHeight);
			for (const sc of sidecarsRef.current) {
				send({
					type: "ResizeScreen",
					sidecar_id: sc.id,
					width: w,
					height: h,
				});
			}
		};
		const onResize = () => {
			clearTimeout(timer);
			timer = setTimeout(sendScreenSize, 150);
		};
		window.addEventListener("resize", onResize);
		// Send initial size once connected
		if (connected && sidecarsRef.current.length > 0) {
			sendScreenSize();
		}
		return () => {
			window.removeEventListener("resize", onResize);
			clearTimeout(timer);
		};
	}, [connected, sidecars, send]);

	/** Start an animated cursor cycle for a window. */
	const startAnimCursor = useCallback((windowId: string, frames: AnimCursorFrame[]) => {
		// Clear any existing animation for this window
		const existing = animCursorTimersRef.current.get(windowId);
		if (existing) clearInterval(existing);

		if (frames.length === 0) return;

		let frameIndex = 0;

		const advanceFrame = () => {
			const frame = frames[frameIndex];
			argbToCursorUrl(
				frame.pixels,
				frame.width,
				frame.height,
				frame.hotspot_x,
				frame.hotspot_y,
			).then((cursorCss) => {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === windowId ? { ...w, cursor: cursorCss } : w,
					),
				);
			});
			frameIndex = (frameIndex + 1) % frames.length;
		};

		// Show first frame immediately
		advanceFrame();

		// Use the first frame's delay for the interval (simplification --
		// ideally each frame has its own delay, but setInterval is fixed)
		const delay = frames[0].delay_ms || 100;
		const timer = setInterval(advanceFrame, delay);
		animCursorTimersRef.current.set(windowId, timer);
	}, []);

	// Reconcile our `windows` state against the backend's authoritative
	// `WindowList`. The backend filters out non-visible / non-top-level
	// windows; we just merge the descriptor's geometry into our existing
	// per-window UI state (or seed defaults for newly-arrived windows).
	useEffect(() => {
		// User-initiated close: reap any closed-window IDs the backend
		// has already dropped from its list (race window over).
		for (const wid of [...closedWindowsRef.current]) {
			if (!windowList.some((w) => w.window_id === wid)) {
				closedWindowsRef.current.delete(wid);
			}
		}
		const filtered = windowList.filter(
			(w) => !closedWindowsRef.current.has(w.window_id),
		);
		setWindows((prev) => {
			const desired = new Set(filtered.map((w) => w.window_id));
			const byId = new Map(prev.map((w) => [w.windowId, w]));
			const next: CanvasWindow[] = filtered.map((d, i) => {
				// Stacking order from the backend's WindowList — last
				// item is on top.
				const zIndex = i + 1;
				const existing = byId.get(d.window_id);
				if (existing) {
					// X11 server position wins for popups; for regular
					// top-level windows the user-driven frontend
					// position takes precedence (`PositionChanged`
					// updates apply incrementally).
					return {
						...existing,
						sidecarId: d.sidecar_id,
						pid: d.pid,
						overrideRedirect: d.override_redirect,
						borderWidth: d.border_width,
						borderPixel: d.border_pixel,
						zIndex,
						...(d.override_redirect ? { x: d.x, y: d.y } : {}),
					};
				}
				// First time seeing this window — seed defaults.
				const title = d.command || `PID ${d.pid}`;
				let cx: number;
				let cy: number;
				const color = d.override_redirect
					? "transparent"
					: colorForWindowId(d.window_id);
				if (d.placed) {
					// Backend gave us an authoritative position (X11 for
					// popups, cross-frontend tracked for top-level).
					cx = d.x;
					cy = d.y;
				} else {
					// First time *any* frontend has seen this top-level
					// window — pick a cascading position and broadcast it
					// so other tabs converge on the same spot.
					const idx = spawnCounter++;
					const offset = idx * 30;
					cx = window.innerWidth / 4 + offset;
					cy = window.innerHeight / 4 + offset;
					send({
						type: "UpdateWindowPosition",
						window_id: d.window_id,
						x: cx,
						y: cy,
					});
				}
				return {
					windowId: d.window_id,
					sidecarId: d.sidecar_id,
					pid: d.pid,
					title,
					x: cx,
					y: cy,
					color,
					zIndex,
					cursor: "default",
					overrideRedirect: d.override_redirect,
					wmState: "normal" as WindowWmState,
					borderWidth: d.border_width,
					borderPixel: d.border_pixel,
				};
			});

			// Anything not in the new list is gone — clean up renderers
			// and per-window timers/menus for those windows.
			for (const w of prev) {
				if (!desired.has(w.windowId)) {
					renderersRef.current.delete(w.windowId);
					const timer = animCursorTimersRef.current.get(w.windowId);
					if (timer) {
						clearInterval(timer);
						animCursorTimersRef.current.delete(w.windowId);
					}
				}
			}

			// Equality check — return prev if nothing changed (avoids
			// re-render on every WindowList that's identical).
			if (
				next.length === prev.length &&
				next.every((w, i) => {
					const old = prev[i];
					return (
						old.windowId === w.windowId
						&& old.x === w.x
						&& old.y === w.y
						&& old.zIndex === w.zIndex
						&& old.borderWidth === w.borderWidth
						&& old.borderPixel === w.borderPixel
						&& old.overrideRedirect === w.overrideRedirect
					);
				})
			) {
				return prev;
			}
			return next;
		});

		// If the focused window left the list, clear focus.
		const liveIds = new Set(filtered.map((w) => w.window_id));
		setFocusedWindowId((prev) => (prev && !liveIds.has(prev) ? null : prev));
		setMenus((prev) => {
			let changed = false;
			const next = new Map(prev);
			for (const wid of prev.keys()) {
				if (!liveIds.has(wid)) {
					next.delete(wid);
					changed = true;
				}
			}
			return changed ? next : prev;
		});
	}, [windowList, send]);

	// Register window-update callback for per-window content events
	// (titles, cursors, focus, WM state, menus, position deltas).
	useEffect(() => {
		onWindowUpdate((update) => {
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
				// Stop any animated cursor
				const existing = animCursorTimersRef.current.get(update.window_id);
				if (existing) {
					clearInterval(existing);
					animCursorTimersRef.current.delete(update.window_id);
				}
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, cursor: update.cursor }
							: w,
					),
				);
			}

			// Custom cursor bitmap -- convert ARGB data to a CSS cursor URL
			if (update.kind === "CursorBitmap") {
				// Stop any animated cursor
				const existing = animCursorTimersRef.current.get(update.window_id);
				if (existing) {
					clearInterval(existing);
					animCursorTimersRef.current.delete(update.window_id);
				}
				const windowId = update.window_id;
				argbToCursorUrl(
					update.data,
					update.width,
					update.height,
					update.hotspot_x,
					update.hotspot_y,
				).then((cursor) => {
					setWindows((prev) =>
						prev.map((w) =>
							w.windowId === windowId ? { ...w, cursor } : w,
						),
					);
				});
			}

			// Animated cursor -- cycle through frames
			if (update.kind === "CursorAnimated") {
				startAnimCursor(update.window_id, update.frames);
			}

			// X11 WM state changed
			if (update.kind === "StateChanged") {
				setWindows((prev) =>
					prev.map((w) => {
						if (w.windowId !== update.window_id) return w;
						const newW = { ...w, wmState: update.state };
						if (update.state === "maximized" || update.state === "fullscreen") {
							newW.savedPosition = { x: w.x, y: w.y };
						}
						return newW;
					}),
				);
			}

			// Focus — drives the global menu bar.
			if (update.kind === "Focused") {
				setFocusedWindowId(update.window_id);
			}

			// Cross-frontend drag delta from another tab.
			if (update.kind === "PositionChanged") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, x: update.x, y: update.y }
							: w,
					),
				);
			}

			// MenuStructure -- full menu tree from a GTK / Qt app.
			if (update.kind === "MenuStructure") {
				setMenus((prev) => {
					const next = new Map(prev);
					if (update.menu.length === 0) {
						next.delete(update.window_id);
					} else {
						next.set(update.window_id, update.menu);
					}
					return next;
				});
			}

			// Route display updates to the per-window renderer.
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
		return () => onWindowUpdate(null);
	}, [onWindowUpdate, startAnimCursor]);

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

	// Clipboard bridge: browser <-> X11
	const clipboardOfferRef = useRef<
		Map<string, { selection: string; mimeTypes: string[] }>
	>(new Map());

	useEffect(() => {
		onClipboardOffer((sidecarId, selection, mimeTypes) => {
			clipboardOfferRef.current.set(sidecarId, { selection, mimeTypes });
		});
		return () => onClipboardOffer(null);
	}, [onClipboardOffer]);

	useEffect(() => {
		onClipboardData((_sidecarId, _selection, mimeType, data) => {
			try {
				if (mimeType === "text/plain" || mimeType.startsWith("text/")) {
					const text = atob(data);
					navigator.clipboard.writeText(text).catch(() => {});
				} else if (mimeType === "text/html") {
					const html = atob(data);
					const blob = new Blob([html], { type: "text/html" });
					const textBlob = new Blob([html], { type: "text/plain" });
					const item = new ClipboardItem({
						"text/html": blob,
						"text/plain": textBlob,
					});
					navigator.clipboard.write([item]).catch(() => {});
				} else if (mimeType.startsWith("image/")) {
					// Decode base64 to binary
					const binaryStr = atob(data);
					const bytes = new Uint8Array(binaryStr.length);
					for (let i = 0; i < binaryStr.length; i++) {
						bytes[i] = binaryStr.charCodeAt(i);
					}
					const blob = new Blob([bytes], { type: mimeType });
					const item = new ClipboardItem({ [mimeType]: blob });
					navigator.clipboard.write([item]).catch(() => {});
				}
			} catch {
				// ignore decode errors
			}
		});
		return () => onClipboardData(null);
	}, [onClipboardData]);

	// Listen for paste events: send browser clipboard content to X11
	useEffect(() => {
		function handlePaste(e: ClipboardEvent) {
			const dt = e.clipboardData;
			if (!dt) return;

			for (const sidecar of sidecars) {
				// Try HTML first
				const html = dt.getData("text/html");
				if (html) {
					send({
						type: "SetClipboard",
						sidecar_id: sidecar.id,
						selection: "CLIPBOARD",
						mime_type: "text/html",
						data: btoa(html),
					});
				}

				// Always send plain text
				const text = dt.getData("text/plain");
				if (text) {
					send({
						type: "SetClipboard",
						sidecar_id: sidecar.id,
						selection: "CLIPBOARD",
						mime_type: "text/plain",
						data: btoa(text),
					});
				}

				// Try images from files
				for (const file of Array.from(dt.files)) {
					if (file.type.startsWith("image/")) {
						const reader = new FileReader();
						reader.onloadend = () => {
							const result = reader.result as ArrayBuffer;
							const bytes = new Uint8Array(result);
							let binary = "";
							for (let j = 0; j < bytes.length; j++) {
								binary += String.fromCharCode(bytes[j]);
							}
							send({
								type: "SetClipboard",
								sidecar_id: sidecar.id,
								selection: "CLIPBOARD",
								mime_type: file.type,
								data: btoa(binary),
							});
						};
						reader.readAsArrayBuffer(file);
					}
				}
			}
		}

		function handleCopy() {
			for (const [sidecarId, offer] of clipboardOfferRef.current) {
				// Request multiple types if available
				const requestedTypes = new Set<string>();
				for (const mime of offer.mimeTypes) {
					if (
						mime === "text/plain" ||
						mime === "text/html" ||
						mime.startsWith("image/png")
					) {
						requestedTypes.add(mime);
					}
				}
				// Fallback to first available type
				if (requestedTypes.size === 0 && offer.mimeTypes.length > 0) {
					requestedTypes.add(offer.mimeTypes[0]);
				}
				// Also request TARGETS for negotiation
				if (offer.mimeTypes.includes("TARGETS")) {
					send({
						type: "RequestClipboard",
						sidecar_id: sidecarId,
						selection: offer.selection,
						mime_type: "TARGETS",
					});
				}
				for (const mimeType of requestedTypes) {
					send({
						type: "RequestClipboard",
						sidecar_id: sidecarId,
						selection: offer.selection,
						mime_type: mimeType,
					});
				}
			}
		}

		document.addEventListener("paste", handlePaste);
		document.addEventListener("copy", handleCopy);
		return () => {
			document.removeEventListener("paste", handlePaste);
			document.removeEventListener("copy", handleCopy);
		};
	}, [sidecars, send]);

	function handleSpawn(sidecarId: string, command: string, args: string[]) {
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
				if (win && !win.overrideRedirect) {
					send({
						type: "UpdateWindowPosition",
						window_id: windowId,
						x,
						y,
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
		setWindows((prev) => {
			const win = prev.find((w) => w.windowId === windowId);
			if (!win) return prev;

			return prev.map((w) =>
				w.windowId === windowId ? { ...w, zIndex: nextZIndex++ } : w,
			);
		});
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

	/** Kill a process and remove all of its windows. */
	const handleCloseProcess = useCallback(
		(sidecarId: string, pid: number) => {
			setWindows((prev) => {
				for (const w of prev) {
					if (w.sidecarId === sidecarId && w.pid === pid) {
						closedWindowsRef.current.add(w.windowId);
						// Clean up animated cursor timer
						const timer = animCursorTimersRef.current.get(w.windowId);
						if (timer) {
							clearInterval(timer);
							animCursorTimersRef.current.delete(w.windowId);
						}
					}
				}
				return prev.filter(
					(w) => !(w.sidecarId === sidecarId && w.pid === pid),
				);
			});
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
			setWindows((prev) =>
				prev.map((w) =>
					w.windowId === windowId
						? { ...w, wmState: "minimized" as WindowWmState }
						: w,
				),
			);
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
			setWindows((prev) =>
				prev.map((w) =>
					w.windowId === windowId
						? {
								...w,
								wmState: "maximized" as WindowWmState,
								savedPosition: { x: w.x, y: w.y },
							}
						: w,
				),
			);
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
			setWindows((prev) =>
				prev.map((w) => {
					if (w.windowId !== windowId) return w;
					const restored = {
						...w,
						wmState: "normal" as WindowWmState,
					};
					if (w.savedPosition) {
						restored.x = w.savedPosition.x;
						restored.y = w.savedPosition.y;
					}
					return restored;
				}),
			);
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
			handleFocus(windowId);
			setFocusedWindowId(windowId);
		},
		[focusPolicy, handleFocus],
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

	const focusedWindow = windows.find((w) => w.windowId === focusedWindowId);
	const focusedTitle = focusedWindow?.title ?? null;
	const focusedMenu = focusedWindowId
		? (menus.get(focusedWindowId) ?? null)
		: null;

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

	const sortedWindows = useMemo(() => [...windows], [windows]);

	return (
		<>
			<GlobalMenuBar
				focusedTitle={focusedTitle}
				menu={focusedMenu}
				onActivate={handleMenuActivate}
				appContextMenuItems={focusedAppContextMenuItems}
			/>
			<InfiniteCanvas>
				{sortedWindows.map((win) => {
					const renderer = renderersRef.current.get(win.windowId);
					if (!renderer) return null;
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
								zIndex={win.zIndex}
								color={win.color}
								cursor={win.cursor}
								renderer={renderer}
								overrideRedirect={win.overrideRedirect}
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
				sidecars={sidecars}
				processes={dockProcesses}
				onSpawn={handleSpawn}
				onClose={handleCloseProcess}
				onFocusWindow={(sidecarId, pid) => {
					// Restore minimized windows and bring all windows for this process to front
					setWindows((prev) =>
						prev.map((w) =>
							w.sidecarId === sidecarId && w.pid === pid
								? {
										...w,
										zIndex: nextZIndex++,
										wmState:
											w.wmState === "minimized"
												? ("normal" as WindowWmState)
												: w.wmState,
									}
								: w,
						),
					);
				}}
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
