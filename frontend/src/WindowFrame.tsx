import { useCallback, useEffect, useRef } from "react";
import type { ClientRenderer } from "./ClientRenderer";
import type { InputEvent, WindowWmState } from "./types";
import s from "./WindowFrame.module.css";

interface WindowFrameProps {
	clientId: string;
	title: string;
	x: number;
	y: number;
	zIndex: number;
	color: string;
	cursor: string;
	renderer: ClientRenderer;
	overrideRedirect?: boolean;
	/** Whether the underlying app supports drag-resize. False for
	 * fixed-size apps (e.g., macOS Calculator) — we hide the resize
	 * handles entirely so the user doesn't get a no-op gesture. */
	resizable?: boolean;
	/** Current WM state (normal, minimized, maximized, fullscreen). */
	wmState?: WindowWmState;
	onClose: () => void;
	onMove: (x: number, y: number) => void;
	onResize: (width: number, height: number) => void;
	onInput: (event: InputEvent) => void;
	onFocus: () => void;
	onMinimize?: () => void;
	onMaximize?: () => void;
	onRestore?: () => void;
	/** X11 border width in pixels. */
	borderWidth?: number;
	/** X11 border color (ARGB32). */
	borderPixel?: number;
}

const MIN_WIDTH = 50;
const MIN_HEIGHT = 50;

function getScale(el: Element): number {
	const wrapper = el.closest("[data-canvas-scale]");
	return wrapper
		? Number.parseFloat(wrapper.getAttribute("data-canvas-scale") || "1")
		: 1;
}

/** Convert X11 ARGB32 pixel to CSS color string. */
function argb32ToCss(pixel: number): string {
	const r = (pixel >> 16) & 0xff;
	const g = (pixel >> 8) & 0xff;
	const b = pixel & 0xff;
	return `rgb(${r},${g},${b})`;
}

export function WindowFrame({
	clientId,
	title,
	x,
	y,
	zIndex,
	color,
	cursor,
	renderer,
	overrideRedirect,
	resizable = true,
	wmState = "normal",
	onClose,
	onMove,
	onResize,
	onInput,
	onFocus,
	onMinimize,
	onMaximize,
	onRestore,
	borderWidth = 0,
	borderPixel = 0,
}: WindowFrameProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const containerRef = useRef<HTMLDivElement>(null);
	const onInputRef = useRef(onInput);
	onInputRef.current = onInput;

	// IME composition state: true between compositionstart and compositionend
	const composingRef = useRef(false);

	// rAF loop: blit back buffer to visible canvas, sync dimensions
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;
		const maybeCtx = el.getContext("2d");
		if (!maybeCtx) return;
		const ctx: CanvasRenderingContext2D = maybeCtx;
		const canvasEl = el;

		// Set initial dimensions from renderer
		canvasEl.width = renderer.width;
		canvasEl.height = renderer.height;
		ctx.drawImage(renderer.backBuffer, 0, 0);

		let running = true;
		function frame() {
			if (!running) return;
			if (renderer.dirty) {
				renderer.dirty = false;
				if (
					canvasEl.width !== renderer.width ||
					canvasEl.height !== renderer.height
				) {
					canvasEl.width = renderer.width;
					canvasEl.height = renderer.height;
				}
				ctx.drawImage(renderer.backBuffer, 0, 0);
			}
			requestAnimationFrame(frame);
		}
		requestAnimationFrame(frame);

		return () => {
			running = false;
		};
	}, [renderer]);

	// Title bar drag
	const handleTitlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (wmState === "maximized" || wmState === "fullscreen") return;
			e.stopPropagation();
			const startX = e.clientX;
			const startY = e.clientY;
			const origX = x;
			const origY = y;
			const target = e.currentTarget;
			target.setPointerCapture(e.pointerId);
			const scale = getScale(target);

			const onPointerMove = (ev: Event) => {
				const { clientX, clientY } = ev as PointerEvent;
				const dx = clientX - startX;
				const dy = clientY - startY;
				onMove(origX + dx / scale, origY + dy / scale);
			};

			const onPointerUp = () => {
				target.removeEventListener("pointermove", onPointerMove);
				target.removeEventListener("pointerup", onPointerUp);
			};

			target.addEventListener("pointermove", onPointerMove);
			target.addEventListener("pointerup", onPointerUp);
		},
		[x, y, onMove, wmState],
	);

	// Corner resize -- dx/dy signs determine which edges move
	const makeResizeHandler = useCallback(
		(flipX: boolean, flipY: boolean) => (e: React.PointerEvent) => {
			if (wmState === "maximized" || wmState === "fullscreen") return;
			e.stopPropagation();
			const startMX = e.clientX;
			const startMY = e.clientY;
			const canvas = canvasRef.current;
			const origW = canvas ? canvas.width : renderer.width;
			const origH = canvas ? canvas.height : renderer.height;
			const origX = x;
			const origY = y;
			const target = e.currentTarget;
			target.setPointerCapture(e.pointerId);
			const scale = getScale(target);

			const onPointerMove = (ev: Event) => {
				const { clientX, clientY } = ev as PointerEvent;
				const dx = (clientX - startMX) / scale;
				const dy = (clientY - startMY) / scale;

				const newW = Math.max(
					MIN_WIDTH,
					Math.round(origW + (flipX ? -dx : dx)),
				);
				const newH = Math.max(
					MIN_HEIGHT,
					Math.round(origH + (flipY ? -dy : dy)),
				);

				renderer.resize(newW, newH);
				onResize(newW, newH);

				if (flipX)
					onMove(
						origX + (origW - newW),
						flipY ? origY + (origH - newH) : origY,
					);
				else if (flipY) onMove(origX, origY + (origH - newH));
			};

			const onPointerUp = () => {
				target.removeEventListener("pointermove", onPointerMove);
				target.removeEventListener("pointerup", onPointerUp);
			};

			target.addEventListener("pointermove", onPointerMove);
			target.addEventListener("pointerup", onPointerUp);
		},
		[renderer, onResize, onMove, x, y, wmState],
	);

	// X11 input forwarding
	const sendInput = useCallback((event: InputEvent) => {
		onInputRef.current(event);
	}, []);

	/** Translate a mouse event's client coordinates into canvas pixels. */
	const clampToCanvas = useCallback(
		(
			e: React.MouseEvent<HTMLCanvasElement>,
		): { x: number; y: number; scaleX: number; scaleY: number } => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = e.currentTarget.width / rect.width;
			const scaleY = e.currentTarget.height / rect.height;
			const mx = Math.round((e.clientX - rect.left) * scaleX);
			const my = Math.round((e.clientY - rect.top) * scaleY);
			return { x: mx, y: my, scaleX, scaleY };
		},
		[],
	);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const { x: mx, y: my } = clampToCanvas(e);
			sendInput({
				kind: "MotionNotify",
				x: mx,
				y: my,
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, clampToCanvas],
	);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			e.stopPropagation();
			const { x: mx, y: my } = clampToCanvas(e);
			const browserBit = [1, 4, 2][e.button] ?? 0;
			const prePressButtons = e.buttons & ~browserBit;
			sendInput({
				kind: "ButtonPress",
				button: x11Button(e.button),
				x: mx,
				y: my,
				state: mouseButtonMask(prePressButtons) | mouseMods(e),
			});
		},
		[sendInput, clampToCanvas],
	);

	const handleMouseUp = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const { x: mx, y: my } = clampToCanvas(e);
			const browserBit = [1, 4, 2][e.button] ?? 0;
			const preReleaseButtons = e.buttons | browserBit;
			sendInput({
				kind: "ButtonRelease",
				button: x11Button(e.button),
				x: mx,
				y: my,
				state: mouseButtonMask(preReleaseButtons) | mouseMods(e),
			});
		},
		[sendInput, clampToCanvas],
	);

	const handleKeyDown = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
			// Escape exits fullscreen
			if (wmState === "fullscreen" && e.key === "Escape") {
				e.preventDefault();
				onRestore?.();
				return;
			}
			// Suppress regular key events during IME composition to avoid double input
			if (composingRef.current) return;
			e.preventDefault();
			const keycode = browserKeyToX11Keycode(e);
			if (keycode > 0) {
				sendInput({
					kind: "KeyPress",
					keycode,
					state: keyboardMask(e),
				});
			}
		},
		[sendInput, wmState, onRestore],
	);

	const handleKeyUp = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
			// Suppress regular key events during IME composition to avoid double input
			if (composingRef.current) return;
			e.preventDefault();
			const keycode = browserKeyToX11Keycode(e);
			if (keycode > 0) {
				sendInput({
					kind: "KeyRelease",
					keycode,
					state: keyboardMask(e),
				});
			}
		},
		[sendInput],
	);

	// IME composition handlers for CJK input
	const handleCompositionStart = useCallback(
		(_e: React.CompositionEvent<HTMLCanvasElement>) => {
			composingRef.current = true;
			sendInput({ kind: "CompositionEvent", phase: "start", text: "" });
		},
		[sendInput],
	);

	const handleCompositionUpdate = useCallback(
		(e: React.CompositionEvent<HTMLCanvasElement>) => {
			sendInput({ kind: "CompositionEvent", phase: "update", text: e.data });
		},
		[sendInput],
	);

	const handleCompositionEnd = useCallback(
		(e: React.CompositionEvent<HTMLCanvasElement>) => {
			composingRef.current = false;
			sendInput({ kind: "CompositionEvent", phase: "end", text: e.data });
		},
		[sendInput],
	);

	// Scroll wheel
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;

		let accY = 0;
		let accX = 0;
		const THRESHOLD = 15;

		const onWheel = (e: WheelEvent) => {
			e.preventDefault();
			e.stopPropagation();

			const rect = el.getBoundingClientRect();
			const scaleX = el.width / rect.width;
			const scaleY = el.height / rect.height;
			const x = Math.round((e.clientX - rect.left) * scaleX);
			const y = Math.round((e.clientY - rect.top) * scaleY);
			const state = mouseButtonMask(e.buttons);

			accY += e.deltaY;
			accX += e.deltaX;

			if (Math.abs(accY) >= THRESHOLD || Math.abs(accX) >= THRESHOLD) {
				onInputRef.current({ kind: "MotionNotify", x, y, state });
			}
			while (Math.abs(accY) >= THRESHOLD) {
				const button = accY > 0 ? 5 : 4;
				onInputRef.current({ kind: "ButtonPress", button, x, y, state });
				onInputRef.current({ kind: "ButtonRelease", button, x, y, state });
				accY -= Math.sign(accY) * THRESHOLD;
			}
			while (Math.abs(accX) >= THRESHOLD) {
				const button = accX > 0 ? 7 : 6;
				onInputRef.current({ kind: "ButtonPress", button, x, y, state });
				onInputRef.current({ kind: "ButtonRelease", button, x, y, state });
				accX -= Math.sign(accX) * THRESHOLD;
			}
		};

		el.addEventListener("wheel", onWheel, { passive: false });
		return () => el.removeEventListener("wheel", onWheel);
	}, [renderer]);

	// Touch input: map touch events to XInput2 touch protocol
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;

		const getCanvasCoords = (touch: Touch) => {
			const rect = el.getBoundingClientRect();
			const scaleX = el.width / rect.width;
			const scaleY = el.height / rect.height;
			return {
				x: Math.round((touch.clientX - rect.left) * scaleX),
				y: Math.round((touch.clientY - rect.top) * scaleY),
			};
		};

		const onTouchStart = (e: TouchEvent) => {
			e.preventDefault();
			for (let i = 0; i < e.changedTouches.length; i++) {
				const t = e.changedTouches[i];
				const { x, y } = getCanvasCoords(t);
				onInputRef.current({ kind: "TouchBegin", touch_id: t.identifier, x, y, state: 0 });
			}
		};

		const onTouchMove = (e: TouchEvent) => {
			e.preventDefault();
			for (let i = 0; i < e.changedTouches.length; i++) {
				const t = e.changedTouches[i];
				const { x, y } = getCanvasCoords(t);
				onInputRef.current({ kind: "TouchUpdate", touch_id: t.identifier, x, y, state: 0 });
			}
		};

		const onTouchEnd = (e: TouchEvent) => {
			e.preventDefault();
			for (let i = 0; i < e.changedTouches.length; i++) {
				const t = e.changedTouches[i];
				const { x, y } = getCanvasCoords(t);
				onInputRef.current({ kind: "TouchEnd", touch_id: t.identifier, x, y, state: 0 });
			}
		};

		el.addEventListener("touchstart", onTouchStart, { passive: false });
		el.addEventListener("touchmove", onTouchMove, { passive: false });
		el.addEventListener("touchend", onTouchEnd, { passive: false });
		el.addEventListener("touchcancel", onTouchEnd, { passive: false });
		return () => {
			el.removeEventListener("touchstart", onTouchStart);
			el.removeEventListener("touchmove", onTouchMove);
			el.removeEventListener("touchend", onTouchEnd);
			el.removeEventListener("touchcancel", onTouchEnd);
		};
	}, [renderer]);

	// Gesture detection: pinch (two-finger) gestures
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;

		let initialDistance = 0;
		let initialAngle = 0;
		let lastScale = 1;
		let gestureActive = false;

		const dist = (t: TouchList) => {
			if (t.length < 2) return 0;
			const dx = t[1].clientX - t[0].clientX;
			const dy = t[1].clientY - t[0].clientY;
			return Math.sqrt(dx * dx + dy * dy);
		};

		const angle = (t: TouchList) => {
			if (t.length < 2) return 0;
			return Math.atan2(t[1].clientY - t[0].clientY, t[1].clientX - t[0].clientX);
		};

		const mid = (t: TouchList) => {
			if (t.length < 2) return { x: 0, y: 0 };
			const rect = el.getBoundingClientRect();
			const scaleX = el.width / rect.width;
			const scaleY = el.height / rect.height;
			return {
				x: Math.round(((t[0].clientX + t[1].clientX) / 2 - rect.left) * scaleX),
				y: Math.round(((t[0].clientY + t[1].clientY) / 2 - rect.top) * scaleY),
			};
		};

		const onTouchStart = (e: TouchEvent) => {
			if (e.touches.length === 2 && !gestureActive) {
				gestureActive = true;
				initialDistance = dist(e.touches);
				initialAngle = angle(e.touches);
				lastScale = 1;
				const m = mid(e.touches);
				onInputRef.current({
					kind: "GesturePinch", dx: m.x, dy: m.y,
					scale: 1, rotation: 0, fingers: 2, phase: "Begin",
				});
			}
		};

		const onTouchMove = (e: TouchEvent) => {
			if (gestureActive && e.touches.length >= 2) {
				const d = dist(e.touches);
				const a = angle(e.touches);
				const scale = initialDistance > 0 ? d / initialDistance : 1;
				const rotation = ((a - initialAngle) * 180) / Math.PI;
				const m = mid(e.touches);
				lastScale = scale;
				onInputRef.current({
					kind: "GesturePinch", dx: m.x, dy: m.y,
					scale, rotation, fingers: 2, phase: "Update",
				});
			}
		};

		const onTouchEnd = (e: TouchEvent) => {
			if (gestureActive && e.touches.length < 2) {
				gestureActive = false;
				onInputRef.current({
					kind: "GesturePinch", dx: 0, dy: 0,
					scale: lastScale, rotation: 0, fingers: 2, phase: "End",
				});
			}
		};

		el.addEventListener("touchstart", onTouchStart, { passive: false });
		el.addEventListener("touchmove", onTouchMove, { passive: false });
		el.addEventListener("touchend", onTouchEnd, { passive: false });
		el.addEventListener("touchcancel", onTouchEnd, { passive: false });
		return () => {
			el.removeEventListener("touchstart", onTouchStart);
			el.removeEventListener("touchmove", onTouchMove);
			el.removeEventListener("touchend", onTouchEnd);
			el.removeEventListener("touchcancel", onTouchEnd);
		};
	}, [renderer]);

	// Gesture detection: swipe (3+ finger) gestures
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;

		let swipeActive = false;
		let startTouches: Map<number, { x: number; y: number }> = new Map();
		let lastCenterX = 0;
		let lastCenterY = 0;
		let fingerCount = 0;

		const center = (t: TouchList) => {
			let sx = 0;
			let sy = 0;
			for (let i = 0; i < t.length; i++) {
				sx += t[i].clientX;
				sy += t[i].clientY;
			}
			return { x: sx / t.length, y: sy / t.length };
		};

		const onTouchStart = (e: TouchEvent) => {
			// Only activate for 3+ fingers; 2-finger is handled by pinch
			if (e.touches.length >= 3 && !swipeActive) {
				swipeActive = true;
				fingerCount = e.touches.length;
				startTouches.clear();
				for (let i = 0; i < e.touches.length; i++) {
					const t = e.touches[i];
					startTouches.set(t.identifier, { x: t.clientX, y: t.clientY });
				}
				const c = center(e.touches);
				lastCenterX = c.x;
				lastCenterY = c.y;
				onInputRef.current({
					kind: "GestureSwipe",
					dx: 0,
					dy: 0,
					fingers: fingerCount,
					phase: "Begin",
				});
			} else if (swipeActive && e.touches.length > fingerCount) {
				// Additional fingers joined an active swipe
				fingerCount = e.touches.length;
			}
		};

		const onTouchMove = (e: TouchEvent) => {
			if (!swipeActive) return;
			const c = center(e.touches);
			const rect = el.getBoundingClientRect();
			const scaleX = el.width / rect.width;
			const scaleY = el.height / rect.height;
			const dx = (c.x - lastCenterX) * scaleX;
			const dy = (c.y - lastCenterY) * scaleY;
			lastCenterX = c.x;
			lastCenterY = c.y;
			onInputRef.current({
				kind: "GestureSwipe",
				dx,
				dy,
				fingers: fingerCount,
				phase: "Update",
			});
		};

		const onTouchEnd = (e: TouchEvent) => {
			if (swipeActive && e.touches.length < 3) {
				swipeActive = false;
				startTouches.clear();
				onInputRef.current({
					kind: "GestureSwipe",
					dx: 0,
					dy: 0,
					fingers: fingerCount,
					phase: "End",
				});
				fingerCount = 0;
			}
		};

		el.addEventListener("touchstart", onTouchStart, { passive: false });
		el.addEventListener("touchmove", onTouchMove, { passive: false });
		el.addEventListener("touchend", onTouchEnd, { passive: false });
		el.addEventListener("touchcancel", onTouchEnd, { passive: false });
		return () => {
			el.removeEventListener("touchstart", onTouchStart);
			el.removeEventListener("touchmove", onTouchMove);
			el.removeEventListener("touchend", onTouchEnd);
			el.removeEventListener("touchcancel", onTouchEnd);
		};
	}, [renderer]);

	// DnD bridge: map HTML5 drag events to XdndDrop protocol
	useEffect(() => {
		const el = canvasRef.current;
		if (!el) return;

		const onDragEnter = (e: DragEvent) => {
			e.preventDefault();
			const types = Array.from(e.dataTransfer?.types ?? []);
			const mimeTypes: string[] = [];
			if (types.includes("text/plain")) mimeTypes.push("text/plain");
			if (types.includes("text/html")) mimeTypes.push("text/html");
			if (types.includes("text/uri-list")) mimeTypes.push("text/uri-list");
			if (types.includes("Files")) mimeTypes.push("application/octet-stream");
			onInputRef.current({
				kind: "DndBridge",
				event: { kind: "Enter", mime_types: mimeTypes },
			});
		};

		const onDragOver = (e: DragEvent) => {
			e.preventDefault();
			if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
			const rect = el.getBoundingClientRect();
			const scaleX = el.width / rect.width;
			const scaleY = el.height / rect.height;
			onInputRef.current({
				kind: "DndBridge",
				event: {
					kind: "Position",
					x: Math.round((e.clientX - rect.left) * scaleX),
					y: Math.round((e.clientY - rect.top) * scaleY),
				},
			});
		};

		const onDrop = (e: DragEvent) => {
			e.preventDefault();
			const dt = e.dataTransfer;
			if (!dt) return;

			// Try text first
			const text = dt.getData("text/plain");
			if (text) {
				onInputRef.current({
					kind: "DndBridge",
					event: {
						kind: "Drop",
						mime_type: "text/plain",
						data: btoa(text),
					},
				});
				return;
			}

			// Try HTML
			const html = dt.getData("text/html");
			if (html) {
				onInputRef.current({
					kind: "DndBridge",
					event: {
						kind: "Drop",
						mime_type: "text/html",
						data: btoa(html),
					},
				});
				return;
			}

			// Try files (first file as PNG/octet-stream)
			if (dt.files.length > 0) {
				const file = dt.files[0];
				const reader = new FileReader();
				reader.onloadend = () => {
					const result = reader.result as ArrayBuffer;
					const bytes = new Uint8Array(result);
					let binary = "";
					for (let i = 0; i < bytes.length; i++) {
						binary += String.fromCharCode(bytes[i]);
					}
					onInputRef.current({
						kind: "DndBridge",
						event: {
							kind: "Drop",
							mime_type: file.type || "application/octet-stream",
							data: btoa(binary),
						},
					});
				};
				reader.readAsArrayBuffer(file);
			}
		};

		const onDragLeave = (e: DragEvent) => {
			e.preventDefault();
			onInputRef.current({
				kind: "DndBridge",
				event: { kind: "Leave" },
			});
		};

		el.addEventListener("dragenter", onDragEnter);
		el.addEventListener("dragover", onDragOver);
		el.addEventListener("drop", onDrop);
		el.addEventListener("dragleave", onDragLeave);
		return () => {
			el.removeEventListener("dragenter", onDragEnter);
			el.removeEventListener("dragover", onDragOver);
			el.removeEventListener("drop", onDrop);
			el.removeEventListener("dragleave", onDragLeave);
		};
	}, [renderer]);

	const handleContextMenu = useCallback(
		(e: React.MouseEvent) => e.preventDefault(),
		[],
	);

	// Hidden (minimized) windows render nothing
	if (wmState === "minimized") {
		return null;
	}

	if (overrideRedirect) {
		return (
			<div
				className={s.overrideRedirect}
				style={{ left: x, top: y, zIndex }}
				data-testid="window-frame"
				data-client-id={clientId}
				data-override-redirect="true"
			>
				<canvas
					ref={canvasRef}
					className={s.canvas}
					style={{
						cursor,
						...(borderWidth > 0 ? {
							border: `${borderWidth}px solid ${argb32ToCss(borderPixel)}`,
							boxSizing: "content-box",
						} : {}),
					}}
					data-testid="x11-canvas"
					tabIndex={0}
					onPointerDown={(e) => {
						e.stopPropagation();
						onFocus();
						e.currentTarget.focus();
					}}
					onMouseMove={handleMouseMove}
					onMouseDown={handleMouseDown}
					onMouseUp={handleMouseUp}
					onKeyDown={handleKeyDown}
					onKeyUp={handleKeyUp}
					onCompositionStart={handleCompositionStart}
					onCompositionUpdate={handleCompositionUpdate}
					onCompositionEnd={handleCompositionEnd}
					onContextMenu={handleContextMenu}
				/>
			</div>
		);
	}

	const isMaximized = wmState === "maximized";
	const isFullscreen = wmState === "fullscreen";

	// Determine container class
	let containerClass = s.window;
	if (isFullscreen) containerClass = `${s.window} ${s.fullscreen}`;
	else if (isMaximized) containerClass = `${s.window} ${s.maximized}`;

	// For maximized/fullscreen, override position
	const containerStyle: React.CSSProperties = isMaximized || isFullscreen
		? { zIndex, background: color }
		: { left: x, top: y, zIndex, background: color };

	return (
		<div
			ref={containerRef}
			className={containerClass}
			style={containerStyle}
			onPointerDown={(e) => {
				onFocus();
				handleTitlePointerDown(e);
			}}
			onClick={() => canvasRef.current?.focus()}
			data-testid="window-frame"
			data-client-id={clientId}
			data-wm-state={wmState}
		>
			<div className={s.header}>
				<div className={s.trafficLights}>
					<button
						type="button"
						className={s.closeButton}
						onPointerDown={(e) => e.stopPropagation()}
						onClick={(e) => {
							e.stopPropagation();
							onClose();
						}}
						data-testid="window-close"
						title="Close"
					>
						x
					</button>
					<button
						type="button"
						className={s.minimizeButton}
						onPointerDown={(e) => e.stopPropagation()}
						onClick={(e) => {
							e.stopPropagation();
							onMinimize?.();
						}}
						data-testid="window-minimize"
						title="Minimize"
					>
						-
					</button>
					<button
						type="button"
						className={s.maximizeButton}
						onPointerDown={(e) => e.stopPropagation()}
						onClick={(e) => {
							e.stopPropagation();
							if (isMaximized || isFullscreen) {
								onRestore?.();
							} else {
								onMaximize?.();
							}
						}}
						data-testid="window-maximize"
						title={isMaximized ? "Restore" : "Maximize"}
					>
						+
					</button>
				</div>
				<span className={s.titleText}>{title}</span>
			</div>
			<canvas
				ref={canvasRef}
				className={s.canvas}
				style={{
					cursor,
					...(borderWidth > 0 ? {
						border: `${borderWidth}px solid ${argb32ToCss(borderPixel)}`,
						boxSizing: "content-box",
					} : {}),
				}}
				data-testid="x11-canvas"
				tabIndex={0}
				onPointerDown={(e) => {
					e.stopPropagation();
					onFocus();
					e.currentTarget.focus();
				}}
				onMouseMove={handleMouseMove}
				onMouseDown={handleMouseDown}
				onMouseUp={handleMouseUp}
				onKeyDown={handleKeyDown}
				onKeyUp={handleKeyUp}
				onCompositionStart={handleCompositionStart}
				onCompositionUpdate={handleCompositionUpdate}
				onCompositionEnd={handleCompositionEnd}
				onContextMenu={handleContextMenu}
			/>
			{resizable && !isMaximized && !isFullscreen && (
				<>
					<div
						className={`${s.resizeHandle} ${s.resizeSE}`}
						onPointerDown={makeResizeHandler(false, false)}
					/>
					<div
						className={`${s.resizeHandle} ${s.resizeSW}`}
						onPointerDown={makeResizeHandler(true, false)}
					/>
					<div
						className={`${s.resizeHandle} ${s.resizeNE}`}
						onPointerDown={makeResizeHandler(false, true)}
					/>
					<div
						className={`${s.resizeHandle} ${s.resizeNW}`}
						onPointerDown={makeResizeHandler(true, true)}
					/>
				</>
			)}
		</div>
	);
}

/** Map browser KeyboardEvent to X11 keycode using e.code (physical key).
 *  Falls back to e.keyCode + 8 for Playwright compatibility. */
function browserKeyToX11Keycode(e: React.KeyboardEvent): number {
	const codeMap: Record<string, number> = {
		Escape: 9,
		Digit1: 10,
		Digit2: 11,
		Digit3: 12,
		Digit4: 13,
		Digit5: 14,
		Digit6: 15,
		Digit7: 16,
		Digit8: 17,
		Digit9: 18,
		Digit0: 19,
		Minus: 20,
		Equal: 21,
		Backspace: 22,
		Tab: 23,
		KeyQ: 24,
		KeyW: 25,
		KeyE: 26,
		KeyR: 27,
		KeyT: 28,
		KeyY: 29,
		KeyU: 30,
		KeyI: 31,
		KeyO: 32,
		KeyP: 33,
		BracketLeft: 34,
		BracketRight: 35,
		Enter: 36,
		ControlLeft: 37,
		KeyA: 38,
		KeyS: 39,
		KeyD: 40,
		KeyF: 41,
		KeyG: 42,
		KeyH: 43,
		KeyJ: 44,
		KeyK: 45,
		KeyL: 46,
		Semicolon: 47,
		Quote: 48,
		Backquote: 49,
		ShiftLeft: 50,
		Backslash: 51,
		KeyZ: 52,
		KeyX: 53,
		KeyC: 54,
		KeyV: 55,
		KeyB: 56,
		KeyN: 57,
		KeyM: 58,
		Comma: 59,
		Period: 60,
		Slash: 61,
		ShiftRight: 62,
		NumpadMultiply: 63,
		AltLeft: 64,
		Space: 65,
		CapsLock: 66,
		F1: 67,
		F2: 68,
		F3: 69,
		F4: 70,
		F5: 71,
		F6: 72,
		F7: 73,
		F8: 74,
		F9: 75,
		F10: 76,
		NumLock: 77,
		ScrollLock: 78,
		F11: 95,
		F12: 96,
		ControlRight: 105,
		AltRight: 108,
		Home: 110,
		ArrowUp: 111,
		PageUp: 112,
		ArrowLeft: 113,
		ArrowRight: 114,
		End: 115,
		ArrowDown: 116,
		PageDown: 117,
		Insert: 118,
		Delete: 119,
		MetaLeft: 133,
		MetaRight: 134,
	};

	if (e.code && codeMap[e.code] !== undefined) {
		return codeMap[e.code];
	}

	if (e.keyCode > 0) {
		return e.keyCode + 8;
	}

	return 0;
}

function x11Button(browserButton: number): number {
	switch (browserButton) {
		case 0:
			return 1;
		case 1:
			return 2;
		case 2:
			return 3;
		default:
			return browserButton + 1;
	}
}

function mouseButtonMask(buttons: number): number {
	let mask = 0;
	if (buttons & 1) mask |= 0x100;
	if (buttons & 4) mask |= 0x200;
	if (buttons & 2) mask |= 0x400;
	return mask;
}

function mouseMods(e: React.MouseEvent): number {
	let mask = 0;
	if (e.shiftKey) mask |= 0x01;
	if (e.ctrlKey) mask |= 0x04;
	if (e.altKey) mask |= 0x08;
	if (e.metaKey) mask |= 0x40;
	return mask;
}

function keyboardMask(e: React.KeyboardEvent): number {
	let mask = 0;
	if (e.shiftKey) mask |= 0x01;
	if (e.ctrlKey) mask |= 0x04;
	if (e.altKey) mask |= 0x08;
	if (e.metaKey) mask |= 0x40;
	return mask;
}
