import { useCallback, useEffect, useRef } from "react";
import type { ClientRenderer } from "./ClientRenderer";
import {
	useDndBridge,
	usePinchGesture,
	useSwipeGesture,
	useTouchInput,
	useWheelInput,
} from "./canvasInputHooks";
import {
	keyDownToInput,
	keyUpToInput,
	mouseDownToInput,
	mouseMoveToInput,
	mouseUpToInput,
} from "./inputProtocol";
import { computeResize, startPointerDrag } from "./pointerDrag";
import type { InputEvent, WindowWmState } from "./types";
import s from "./WindowFrame.module.css";

interface WindowFrameProps {
	clientId: string;
	title: string;
	x: number;
	y: number;
	zIndex: number;
	color: string;
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

	// Title bar drag — `startPointerDrag` owns the pointer-capture +
	// listener bookkeeping; we just translate the cursor delta into
	// a new top-left position.
	const handleTitlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (wmState === "maximized" || wmState === "fullscreen") return;
			e.stopPropagation();
			const origX = x;
			const origY = y;
			startPointerDrag(e, ({ dx, dy, scale }) => {
				onMove(origX + dx / scale, origY + dy / scale);
			});
		},
		[x, y, onMove, wmState],
	);

	// Corner resize — `flipX` / `flipY` identify the grabbed corner.
	// `computeResize` does the geometry; the helper handles the
	// pointer-capture wiring.
	const makeResizeHandler = useCallback(
		(flipX: boolean, flipY: boolean) => (e: React.PointerEvent) => {
			if (wmState === "maximized" || wmState === "fullscreen") return;
			e.stopPropagation();
			const canvas = canvasRef.current;
			const origW = canvas ? canvas.width : renderer.width;
			const origH = canvas ? canvas.height : renderer.height;
			const origX = x;
			const origY = y;
			startPointerDrag(e, ({ dx, dy, scale }) => {
				const r = computeResize({
					origX,
					origY,
					origW,
					origH,
					dx: dx / scale,
					dy: dy / scale,
					flipX,
					flipY,
					minW: MIN_WIDTH,
					minH: MIN_HEIGHT,
				});
				renderer.resize(r.width, r.height);
				onResize(r.width, r.height);
				if (flipX || flipY) onMove(r.x, r.y);
			});
		},
		[renderer, onResize, onMove, x, y, wmState],
	);

	// X11 input forwarding
	const sendInput = useCallback((event: InputEvent) => {
		onInputRef.current(event);
	}, []);

	/** Build the canvas geometry the protocol module needs to map
	 *  client coords into canvas pixels. */
	const canvasGeom = useCallback((target: HTMLCanvasElement) => {
		return {
			rect: target.getBoundingClientRect(),
			width: target.width,
			height: target.height,
		};
	}, []);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			sendInput(mouseMoveToInput(e, canvasGeom(e.currentTarget)));
		},
		[sendInput, canvasGeom],
	);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			e.stopPropagation();
			sendInput(mouseDownToInput(e, canvasGeom(e.currentTarget)));
		},
		[sendInput, canvasGeom],
	);

	const handleMouseUp = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			sendInput(mouseUpToInput(e, canvasGeom(e.currentTarget)));
		},
		[sendInput, canvasGeom],
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
			const ev = keyDownToInput(e);
			if (ev) sendInput(ev);
		},
		[sendInput, wmState, onRestore],
	);

	const handleKeyUp = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
			// Suppress regular key events during IME composition to avoid double input
			if (composingRef.current) return;
			e.preventDefault();
			const ev = keyUpToInput(e);
			if (ev) sendInput(ev);
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

	// Gesture / wheel / touch / DnD wiring lives in dedicated
	// hooks (`canvasInputHooks.ts`); each hook installs its own
	// listeners on the canvas element and emits `InputEvent`s
	// through `onInputRef`. The DOM-level `attach*` helpers behind
	// the hooks are tested directly via jsdom.
	useWheelInput(canvasRef, onInputRef);
	useTouchInput(canvasRef, onInputRef);
	usePinchGesture(canvasRef, onInputRef);
	useSwipeGesture(canvasRef, onInputRef);
	useDndBridge(canvasRef, onInputRef);

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
					style={
						borderWidth > 0
							? {
									border: `${borderWidth}px solid ${argb32ToCss(borderPixel)}`,
									boxSizing: "content-box",
								}
							: undefined
					}
					data-testid="x11-canvas"
					tabIndex={0}
					onPointerDown={(e) => {
						e.stopPropagation();
						onFocus();
						// preventScroll: plain focus() scroll-into-views inside the
						// overflow-hidden viewport, silently shifting all canvas
						// content ~100px mid-click for windows taller than the
						// viewport — the click then lands at the wrong X coords
						// (Firefox was the only app big enough to trigger it).
						e.currentTarget.focus({ preventScroll: true });
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
	const containerStyle: React.CSSProperties =
		isMaximized || isFullscreen
			? { zIndex, background: color }
			: { left: x, top: y, zIndex, background: color };

	return (
		<div
			ref={containerRef}
			// `application`: the frame wraps a live remote X11 window whose
			// own toolkit owns the keyboard. Announcing it as a generic
			// container would put a screen reader into browse mode and
			// swallow the keystrokes we forward to the client.
			role="application"
			className={containerClass}
			style={containerStyle}
			onPointerDown={(e) => {
				onFocus();
				handleTitlePointerDown(e);
			}}
			onClick={() => canvasRef.current?.focus({ preventScroll: true })}
			// The click handler only moves DOM focus onto the inner
			// canvas; keyboard users reach that canvas directly via Tab,
			// so pressing a key here does the same job as the click.
			onKeyDown={(e) => {
				if (e.target !== e.currentTarget) return;
				if (e.key !== "Enter" && e.key !== " ") return;
				e.preventDefault();
				canvasRef.current?.focus({ preventScroll: true });
			}}
			data-testid="window-frame"
			data-client-id={clientId}
			data-wm-state={wmState}
			data-ocif-attachable={overrideRedirect ? undefined : clientId}
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
				style={
					borderWidth > 0
						? {
								border: `${borderWidth}px solid ${argb32ToCss(borderPixel)}`,
								boxSizing: "content-box",
							}
						: undefined
				}
				data-testid="x11-canvas"
				tabIndex={0}
				onPointerDown={(e) => {
					e.stopPropagation();
					onFocus();
					// preventScroll: plain focus() scroll-into-views inside the
					// overflow-hidden viewport, silently shifting all canvas
					// content ~100px mid-click for windows taller than the
					// viewport — the click then lands at the wrong X coords
					// (Firefox was the only app big enough to trigger it).
					e.currentTarget.focus({ preventScroll: true });
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
// Pure browser-event → X11 protocol translation lives in
// `inputProtocol.ts` (table-tested in `inputProtocol.test.ts`).
