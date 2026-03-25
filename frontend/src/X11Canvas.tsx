import { useCallback, useEffect, useRef } from "react";
import type { ClientRenderer } from "./ClientRenderer";
import type { InputEvent } from "./types";
import s from "./X11Canvas.module.css";

interface X11CanvasProps {
	renderer: ClientRenderer;
	width?: number;
	height?: number;
	onInput?: (event: InputEvent) => void;
}

export function X11Canvas({
	renderer,
	width = 1024,
	height = 768,
	onInput,
}: X11CanvasProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const onInputRef = useRef(onInput);
	onInputRef.current = onInput;

	// rAF loop: blit back buffer to visible canvas when dirty
	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;
		const maybeCtx = canvas.getContext("2d");
		if (!maybeCtx) return;
		const visibleCtx: CanvasRenderingContext2D = maybeCtx;

		// Immediately blit current state (restores content on tab switch)
		visibleCtx.drawImage(renderer.backBuffer, 0, 0);

		let running = true;

		function frame() {
			if (!running) return;

			if (renderer.dirty) {
				renderer.dirty = false;
				visibleCtx.drawImage(renderer.backBuffer, 0, 0);
			}

			requestAnimationFrame(frame);
		}

		requestAnimationFrame(frame);

		return () => {
			running = false;
		};
	}, [renderer]);

	const sendInput = useCallback((event: InputEvent) => {
		onInputRef.current?.(event);
	}, []);

	const handleMouseMove = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = width / rect.width;
			const scaleY = height / rect.height;
			sendInput({
				kind: "MotionNotify",
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, width, height],
	);

	const handleMouseDown = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = width / rect.width;
			const scaleY = height / rect.height;
			sendInput({
				kind: "ButtonPress",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, width, height],
	);

	const handleMouseUp = useCallback(
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			const scaleX = width / rect.width;
			const scaleY = height / rect.height;
			sendInput({
				kind: "ButtonRelease",
				button: x11Button(e.button),
				x: Math.round((e.clientX - rect.left) * scaleX),
				y: Math.round((e.clientY - rect.top) * scaleY),
				state: mouseButtonMask(e.buttons),
			});
		},
		[sendInput, width, height],
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
		(e: React.MouseEvent<HTMLCanvasElement>) => {
			e.preventDefault();
		},
		[],
	);

	return (
		<canvas
			ref={canvasRef}
			width={width}
			height={height}
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
