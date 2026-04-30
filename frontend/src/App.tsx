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
	overrideRedirect: boolean;
	/** Current WM state. */
	wmState: WindowWmState;
	/** Whether cursor is confined to this window. */
	cursorConfined: boolean;
	/** Parent window id for transient windows. */
	transientFor: string | null;
	/** Saved position before maximize/fullscreen. */
	savedPosition?: { x: number; y: number };
	/** WM_HINTS urgency flag. */
	urgent?: boolean;
	/** Window icon dimensions and RGBA data (base64). */
	iconWidth?: number;
	iconHeight?: number;
	iconData?: string;
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
	for (let i = 0; i < width * height; i++) {
		const srcOff = i * 4;
		imageData.data[srcOff] = rawData[srcOff + 2]; // R
		imageData.data[srcOff + 1] = rawData[srcOff + 1]; // G
		imageData.data[srcOff + 2] = rawData[srcOff]; // B
		imageData.data[srcOff + 3] = rawData[srcOff + 3]; // A
	}
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
		connectedProcesses,
		initialWindowStates,
		send,
		onDisplayUpdate,
		onWindowStateChange,
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
	/** Map clientId -> { sidecarId, pid, command } for process association. */
	const clientInfoRef = useRef<
		Map<string, { sidecarId: string; pid: number; command: string }>
	>(new Map());
	/** Track creation metadata (position, override_redirect) for windows not yet mapped. */
	const windowCreationRef = useRef<
		Map<string, { x: number; y: number; overrideRedirect: boolean; borderWidth: number; borderPixel: number }>
	>(new Map());
	/** Track which sidecars we've already subscribed to. */
	const subscribedRef = useRef<Set<string>>(new Set());
	/** Ref to always-current processes map (avoids stale closures in callbacks). */
	const processesRef = useRef(processes);
	processesRef.current = processes;
	const initialWindowStatesRef = useRef(initialWindowStates);
	initialWindowStatesRef.current = initialWindowStates;

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

	// Register display callback -- creates WindowFrames on WindowMapped
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

			// Window WM state changed from server
			if (update.kind === "WindowStateChanged") {
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

			// WindowRaised -- server raised a window to the top of the stack
			if (update.kind === "WindowRaised") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, zIndex: nextZIndex++ }
							: w,
					),
				);
			}

			// Bell -- play an audible/visual bell
			if (update.kind === "Bell") {
				try {
					const ctx = new AudioContext();
					const osc = ctx.createOscillator();
					const gain = ctx.createGain();
					osc.connect(gain);
					gain.connect(ctx.destination);
					osc.frequency.value = 800;
					gain.gain.value = Math.max(0.01, update.percent / 100);
					osc.start();
					osc.stop(ctx.currentTime + 0.1);
				} catch {
					document.body.style.backgroundColor = "#fff";
					setTimeout(() => {
						document.body.style.backgroundColor = "";
					}, 100);
				}
			}

			// WindowUrgent -- set urgency hint (dock bounce / title flash)
			if (update.kind === "WindowUrgent") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? { ...w, urgent: update.urgent }
							: w,
					),
				);
			}

			// WindowIconChanged -- update window icon
			if (update.kind === "WindowIconChanged") {
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? {
									...w,
									iconWidth: update.width,
									iconHeight: update.height,
									iconData: update.data,
								}
							: w,
					),
				);
			}

			// WindowConfigured -- update override-redirect window positions from server
			if (update.kind === "WindowConfigured") {
				const bw = update.border_width ?? 0;
				const bp = update.border_pixel ?? 0;
				setWindows((prev) =>
					prev.map((w) =>
						w.windowId === update.window_id
							? {
								...w,
								...(w.overrideRedirect ? { x: update.x, y: update.y } : {}),
								borderWidth: bw,
								borderPixel: bp,
							}
							: w,
					),
				);
				const existing = windowCreationRef.current.get(update.window_id);
				if (existing) {
					existing.x = update.x;
					existing.y = update.y;
					existing.borderWidth = bw;
					existing.borderPixel = bp;
				}
			}

			// WindowCreated -- track creation metadata for later use at map time
			if (update.kind === "WindowCreated") {
				windowCreationRef.current.set(update.window_id, {
					x: update.x,
					y: update.y,
					overrideRedirect: !!update.override_redirect,
					borderWidth: update.border_width ?? 0,
					borderPixel: update.border_pixel ?? 0,
				});
			}

			// WindowMapped with is_top_level or override_redirect -- create a WindowFrame
			if (update.kind === "WindowMapped" && (update.is_top_level || update.override_redirect)) {
				const windowId = update.window_id;
				if (closedWindowsRef.current.has(windowId)) return;

				const isOverrideRedirect = !!update.override_redirect;
				const creationMeta = windowCreationRef.current.get(windowId);

				setWindows((prev) => {
					if (prev.some((w) => w.windowId === windowId)) return prev;

					const info = clientInfoRef.current.get(clientId);
					const pid = info?.pid ?? 0;
					const sid = info?.sidecarId ?? sidecarId;
					const command = info?.command;
					const title = command
						|| processesRef.current[sid]?.find((p) => p.pid === pid)?.command
						|| `PID ${pid}`;

					let cx: number;
					let cy: number;
					let color: string;

					if (isOverrideRedirect) {
						cx = creationMeta?.x ?? 0;
						cy = creationMeta?.y ?? 0;
						color = "transparent";
					} else {
						const saved = initialWindowStatesRef.current.find(
							(ws) => ws.clientId === clientId,
						);
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
							overrideRedirect: isOverrideRedirect,
							wmState: "normal" as WindowWmState,
							cursorConfined: false,
							transientFor: null,
							borderWidth: creationMeta?.borderWidth ?? 0,
							borderPixel: creationMeta?.borderPixel ?? 0,
						},
					];
				});
			}

			// WindowUnmapped -- hide the WindowFrame
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

			// WindowDestroyed -- remove frame and renderer
			if (update.kind === "WindowDestroyed") {
				renderersRef.current.delete(update.window_id);
				windowCreationRef.current.delete(update.window_id);
				// Clean up animated cursor timer
				const timer = animCursorTimersRef.current.get(update.window_id);
				if (timer) {
					clearInterval(timer);
					animCursorTimersRef.current.delete(update.window_id);
				}
				setWindows((prev) => {
					if (!prev.some((w) => w.windowId === update.window_id))
						return prev;
					return prev.filter((w) => w.windowId !== update.window_id);
				});
				setFocusedWindowId((prev) =>
					prev === update.window_id ? null : prev,
				);
				setMenus((prev) => {
					if (!prev.has(update.window_id)) return prev;
					const next = new Map(prev);
					next.delete(update.window_id);
					return next;
				});
			}

			// WindowFocused -- the X11 server tells us which top-level
			// window has input focus. Drives the global menu bar.
			if (update.kind === "WindowFocused") {
				setFocusedWindowId(update.window_id);
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
		return () => onDisplayUpdate(null);
	// biome-ignore lint/correctness/useExhaustiveDependencies: initialWindowStates used via ref to avoid re-registering callback
	}, [onDisplayUpdate, send, startAnimCursor]);

	// Handle window state changes from other tabs
	useEffect(() => {
		onWindowStateChange((clientId, x, y, color) => {
			setWindows((prev) =>
				prev.map((w) => (w.clientId === clientId ? { ...w, x, y, color } : w)),
			);
		});
		return () => onWindowStateChange(null);
	}, [onWindowStateChange]);

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
				if (!subscribedRef.current.has(sidecar.id)) continue;

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
				if (!subscribedRef.current.has(sidecarId)) continue;
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
				if (win && !win.overrideRedirect) {
					send({
						type: "UpdateWindowState",
						client_id: win.clientId,
						sidecar_id: win.sidecarId,
						x,
						y,
						color: win.color,
					});
				}
				// Move transient children along with parent
				const dx = win ? x - win.x : 0;
				const dy = win ? y - win.y : 0;
				return prev.map((w) => {
					if (w.windowId === windowId) return { ...w, x, y };
					// If this window is transient for the moved window, follow it
					if (w.transientFor === windowId) {
						return { ...w, x: w.x + dx, y: w.y + dy };
					}
					return w;
				});
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

			return prev.map((w) => {
				if (w.windowId === windowId) return { ...w, zIndex: nextZIndex++ };
				// Raise transient children above parent
				if (w.transientFor === windowId) return { ...w, zIndex: nextZIndex++ };
				return w;
			});
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

	/** Sort windows: transient windows always above their parent. */
	const sortedWindows = useMemo(() => {
		const result = [...windows];
		// Boost transient children z-index to be above parent
		for (const w of result) {
			if (w.transientFor) {
				const parent = result.find((p) => p.windowId === w.transientFor);
				if (parent && w.zIndex <= parent.zIndex) {
					w.zIndex = parent.zIndex + 1;
				}
			}
		}
		return result;
	}, [windows]);

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
								cursorConfined={win.cursorConfined}
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
