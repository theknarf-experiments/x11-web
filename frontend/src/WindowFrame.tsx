import { useCallback, useEffect, useRef } from "react";
import type { ClientRenderer } from "./ClientRenderer";
import type { InputEvent } from "./types";
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
	onClose: () => void;
	onMove: (x: number, y: number) => void;
	onResize: (width: number, height: number) => void;
	onInput: (event: InputEvent) => void;
	onFocus: () => void;
}

const MIN_WIDTH = 50;
const MIN_HEIGHT = 50;

function getScale(el: Element): number {
	const wrapper = el.closest("[data-canvas-scale]");
	return wrapper
		? Number.parseFloat(wrapper.getAttribute("data-canvas-scale") || "1")
		: 1;
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
	onClose,
	onMove,
	onResize,
	onInput,
	onFocus,
}: WindowFrameProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const onInputRef = useRef(onInput);
	onInputRef.current = onInput;

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
		[x, y, onMove],
	);

	// Corner resize — dx/dy signs determine which edges move
	const makeResizeHandler = useCallback(
		(flipX: boolean, flipY: boolean) => (e: React.PointerEvent) => {
			e.stopPropagation();
			const startMX = e.clientX;
			const startMY = e.clientY;
			// Read current dimensions from the canvas element to avoid stale closure values
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

				// Move origin if resizing from left or top edge
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
		[renderer, onResize, onMove, x, y],
	);

	// X11 input forwarding
	const sendInput = useCallback((event: InputEvent) => {
		onInputRef.current(event);
	}, []);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = e.currentTarget.width / rect.width;
			const scaleY = e.currentTarget.height / rect.height;
			sendInput({
				kind: "MotionNotify",
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput],
	);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			e.stopPropagation();
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = e.currentTarget.width / rect.width;
			const scaleY = e.currentTarget.height / rect.height;
			// X11 state = buttons/modifiers BEFORE the press.
			// Browser e.buttons already includes the just-pressed button, so mask it out.
			// X11 state = buttons/modifiers BEFORE the press.
			// Browser e.buttons already includes the just-pressed button, so mask it out.
			const browserBit = [1, 4, 2][e.button] ?? 0;
			const prePressButtons = e.buttons & ~browserBit;
			sendInput({
				kind: "ButtonPress",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(prePressButtons) | mouseMods(e),
			});
		},
		[sendInput],
	);

	const handleMouseUp = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = e.currentTarget.width / rect.width;
			const scaleY = e.currentTarget.height / rect.height;
			// X11 state = buttons/modifiers BEFORE the release (including the button being released).
			// Browser e.buttons already excludes the just-released button, so add it back.
			const browserBit = [1, 4, 2][e.button] ?? 0;
			const preReleaseButtons = e.buttons | browserBit;
			sendInput({
				kind: "ButtonRelease",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(preReleaseButtons) | mouseMods(e),
			});
		},
		[sendInput],
	);

	const handleKeyDown = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
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
		[sendInput],
	);

	const handleKeyUp = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
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

	// Scroll wheel → X11 button 4/5/6/7 press+release
	// Must use addEventListener with { passive: false } to call preventDefault().
	// Re-attach when canvasRef changes by keying on renderer (which changes per window).
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

			// Send MotionNotify first so GTK knows the pointer position
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

	const handleContextMenu = useCallback(
		(e: React.MouseEvent) => e.preventDefault(),
		[],
	);

	return (
		<div
			className={s.window}
			style={{ left: x, top: y, zIndex, background: color }}
			onPointerDown={(e) => {
				onFocus();
				handleTitlePointerDown(e);
			}}
			onClick={() => canvasRef.current?.focus()}
			data-testid="window-frame"
			data-client-id={clientId}
		>
			<div className={s.header}>
				<button
					type="button"
					className={s.closeButton}
					onPointerDown={(e) => e.stopPropagation()}
					onClick={(e) => {
						e.stopPropagation();
						onClose();
					}}
					data-testid="window-close"
				>
					×
				</button>
				<span className={s.titleText}>{title}</span>
			</div>
			<canvas
				ref={canvasRef}
				className={s.canvas}
				style={{ cursor }}
				data-testid="x11-canvas"
				tabIndex={0}
				onPointerDown={(e) => {
					e.stopPropagation();
					onFocus(); // Bring window to front
					e.currentTarget.focus(); // Take keyboard focus
				}}
				onMouseMove={handleMouseMove}
				onMouseDown={handleMouseDown}
				onMouseUp={handleMouseUp}
				onKeyDown={handleKeyDown}
				onKeyUp={handleKeyUp}
				onContextMenu={handleContextMenu}
			/>
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
		</div>
	);
}

/** Map browser KeyboardEvent to X11 keycode using e.code (physical key).
 *  Falls back to e.keyCode + 8 for Playwright compatibility. */
function browserKeyToX11Keycode(e: React.KeyboardEvent): number {
	// e.code gives the physical key (reliable in real browsers)
	// Map to X11 keycodes (which = evdev keycode = Linux input code + 8)
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

	// Fallback for Playwright which may not set e.code
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
