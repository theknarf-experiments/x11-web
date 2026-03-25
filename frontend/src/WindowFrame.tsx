import { useCallback, useEffect, useRef } from "react";
import type { ClientRenderer } from "./ClientRenderer";
import type { InputEvent } from "./types";
import s from "./WindowFrame.module.css";

interface WindowFrameProps {
	clientId: string;
	title: string;
	x: number;
	y: number;
	renderer: ClientRenderer;
	onClose: () => void;
	onMove: (x: number, y: number) => void;
	onInput: (event: InputEvent) => void;
	canvasWidth?: number;
	canvasHeight?: number;
}

export function WindowFrame({
	clientId,
	title,
	x,
	y,
	renderer,
	onClose,
	onMove,
	onInput,
	canvasWidth = 1024,
	canvasHeight = 768,
}: WindowFrameProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const onInputRef = useRef(onInput);
	onInputRef.current = onInput;

	// rAF loop: blit back buffer to visible canvas
	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;
		const maybeCtx = canvas.getContext("2d");
		if (!maybeCtx) return;
		const ctx: CanvasRenderingContext2D = maybeCtx;

		ctx.drawImage(renderer.backBuffer, 0, 0);

		let running = true;
		function frame() {
			if (!running) return;
			if (renderer.dirty) {
				renderer.dirty = false;
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

			const onPointerMove = (ev: Event) => {
				const { clientX, clientY } = ev as PointerEvent;
				const dx = clientX - startX;
				const dy = clientY - startY;
				// Divide by scale to get canvas-space delta.
				// The parent InfiniteCanvas applies a CSS scale transform,
				// so screen-space deltas need to be divided by scale.
				// We read scale from a data attribute on the canvas wrapper.
				const wrapper = target.closest("[data-canvas-scale]");
				const scale = wrapper
					? Number.parseFloat(wrapper.getAttribute("data-canvas-scale") || "1")
					: 1;
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

	// X11 input forwarding
	const sendInput = useCallback((event: InputEvent) => {
		onInputRef.current(event);
	}, []);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = canvasWidth / rect.width;
			const scaleY = canvasHeight / rect.height;
			sendInput({
				kind: "MotionNotify",
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, canvasWidth, canvasHeight],
	);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			e.stopPropagation();
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = canvasWidth / rect.width;
			const scaleY = canvasHeight / rect.height;
			sendInput({
				kind: "ButtonPress",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, canvasWidth, canvasHeight],
	);

	const handleMouseUp = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = canvasWidth / rect.width;
			const scaleY = canvasHeight / rect.height;
			sendInput({
				kind: "ButtonRelease",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, canvasWidth, canvasHeight],
	);

	const handleKeyDown = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
			e.preventDefault();
			sendInput({
				kind: "KeyPress",
				keycode: e.keyCode + 8,
				state: keyboardMask(e),
			});
		},
		[sendInput],
	);

	const handleKeyUp = useCallback(
		(e: React.KeyboardEvent<HTMLCanvasElement>) => {
			e.preventDefault();
			sendInput({
				kind: "KeyRelease",
				keycode: e.keyCode + 8,
				state: keyboardMask(e),
			});
		},
		[sendInput],
	);

	const handleContextMenu = useCallback(
		(e: React.MouseEvent) => e.preventDefault(),
		[],
	);

	return (
		<div
			className={s.window}
			style={{ left: x, top: y }}
			data-testid="window-frame"
			data-client-id={clientId}
		>
			<div className={s.titleBar} onPointerDown={handleTitlePointerDown}>
				<span className={s.titleText}>{title}</span>
				<button
					type="button"
					className={s.closeButton}
					onClick={(e) => {
						e.stopPropagation();
						onClose();
					}}
					data-testid="window-close"
				>
					×
				</button>
			</div>
			<canvas
				ref={canvasRef}
				width={canvasWidth}
				height={canvasHeight}
				className={s.canvas}
				data-testid="x11-canvas"
				tabIndex={0}
				onMouseMove={handleMouseMove}
				onMouseDown={handleMouseDown}
				onMouseUp={handleMouseUp}
				onKeyDown={handleKeyDown}
				onKeyUp={handleKeyUp}
				onContextMenu={handleContextMenu}
			/>
		</div>
	);
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

function keyboardMask(e: React.KeyboardEvent): number {
	let mask = 0;
	if (e.shiftKey) mask |= 0x01;
	if (e.ctrlKey) mask |= 0x04;
	if (e.altKey) mask |= 0x08;
	if (e.metaKey) mask |= 0x40;
	return mask;
}
