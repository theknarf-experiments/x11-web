import { useCallback, useEffect, useRef } from "react";
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
// Pure browser-event → X11 protocol translation lives in
// `inputProtocol.ts` (table-tested in `inputProtocol.test.ts`).
