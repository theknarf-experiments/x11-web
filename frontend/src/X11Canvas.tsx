import { useCallback, useEffect, useRef } from "react";
import type { DisplayUpdate, InputEvent } from "./types";
import s from "./X11Canvas.module.css";

interface X11CanvasProps {
	/** Push a display update into the render queue (called outside React) */
	queueRef: React.RefObject<DisplayUpdate[]>;
	width?: number;
	height?: number;
	onInput?: (event: InputEvent) => void;
}

interface WindowInfo {
	x: number;
	y: number;
	width: number;
	height: number;
	mapped: boolean;
}

export function X11Canvas({
	queueRef,
	width = 1024,
	height = 768,
	onInput,
}: X11CanvasProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const backBufferRef = useRef<OffscreenCanvas | null>(null);
	const backCtxRef = useRef<OffscreenCanvasRenderingContext2D | null>(null);
	const windowsRef = useRef<Map<number, WindowInfo>>(new Map());
	const onInputRef = useRef(onInput);
	onInputRef.current = onInput;

	// rAF render loop: drain queue → back buffer → visible canvas
	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;
		const ctx = canvas.getContext("2d");
		if (!ctx) return;

		const backBuffer = new OffscreenCanvas(width, height);
		const backCtx = backBuffer.getContext("2d") as
			| OffscreenCanvasRenderingContext2D
			| undefined;
		if (!backCtx) return;
		backBufferRef.current = backBuffer;
		backCtxRef.current = backCtx;

		// Capture non-null refs for the closure so TS narrows them
		const renderCtx: RenderContext = backCtx;
		const visibleCtx = ctx;

		let running = true;

		function frame() {
			if (!running) return;

			const queue = queueRef.current;
			if (queue.length > 0) {
				// Drain all pending updates into the back buffer
				const batch = queue.splice(0, queue.length);
				for (const update of batch) {
					renderUpdate(renderCtx, update, windowsRef.current);
				}
				// Blit to visible canvas
				visibleCtx.drawImage(backBuffer, 0, 0);
			}

			requestAnimationFrame(frame);
		}

		requestAnimationFrame(frame);

		return () => {
			running = false;
		};
	}, [queueRef, width, height]);

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

type RenderContext =
	| CanvasRenderingContext2D
	| OffscreenCanvasRenderingContext2D;

function renderUpdate(
	ctx: RenderContext,
	update: DisplayUpdate,
	windows: Map<number, WindowInfo>,
) {
	switch (update.kind) {
		case "WindowCreated": {
			windows.set(update.window_id, {
				x: update.x,
				y: update.y,
				width: update.width,
				height: update.height,
				mapped: false,
			});
			break;
		}
		case "WindowDestroyed": {
			const win = windows.get(update.window_id);
			if (win) {
				ctx.clearRect(win.x, win.y, win.width, win.height);
				windows.delete(update.window_id);
			}
			break;
		}
		case "WindowMapped": {
			const win = windows.get(update.window_id);
			if (win) {
				win.mapped = true;
			}
			break;
		}
		case "WindowUnmapped": {
			const win = windows.get(update.window_id);
			if (win) {
				win.mapped = false;
				ctx.clearRect(win.x, win.y, win.width, win.height);
			}
			break;
		}
		case "WindowConfigured": {
			const win = windows.get(update.window_id);
			if (win) {
				ctx.clearRect(win.x, win.y, win.width, win.height);
				win.x = update.x;
				win.y = update.y;
				win.width = update.width;
				win.height = update.height;
			}
			break;
		}
		case "FillRect": {
			ctx.fillStyle = colorToCSS(update.color);
			ctx.fillRect(update.x, update.y, update.width, update.height);
			break;
		}
		case "DrawLines": {
			if (update.points.length < 2) break;
			ctx.strokeStyle = colorToCSS(update.color);
			ctx.lineWidth = Math.max(1, update.line_width);
			ctx.beginPath();
			ctx.moveTo(update.points[0][0], update.points[0][1]);
			for (let i = 1; i < update.points.length; i++) {
				ctx.lineTo(update.points[i][0], update.points[i][1]);
			}
			ctx.stroke();
			break;
		}
		case "PutImage": {
			if (update.data.length === 0) break;
			const imageData = ctx.createImageData(update.width, update.height);
			for (let i = 0; i < update.width * update.height; i++) {
				const srcOff = i * 4;
				const dstOff = i * 4;
				if (srcOff + 3 < update.data.length) {
					imageData.data[dstOff] = update.data[srcOff + 2];
					imageData.data[dstOff + 1] = update.data[srcOff + 1];
					imageData.data[dstOff + 2] = update.data[srcOff];
					imageData.data[dstOff + 3] = 255;
				}
			}
			ctx.putImageData(imageData, update.x, update.y);
			break;
		}
		case "CopyArea": {
			try {
				const imgData = ctx.getImageData(
					update.src_x,
					update.src_y,
					update.width,
					update.height,
				);
				ctx.putImageData(imgData, update.dst_x, update.dst_y);
			} catch {
				// May fail if source is out of bounds
			}
			break;
		}
		case "ClearArea": {
			ctx.clearRect(update.x, update.y, update.width, update.height);
			break;
		}
		case "DrawArc": {
			const startDeg = update.angle1 / 64;
			const sweepDeg = update.angle2 / 64;
			const startAngle = -startDeg * (Math.PI / 180);
			const endAngle = -(startDeg + sweepDeg) * (Math.PI / 180);
			const cx = update.x + update.width / 2;
			const cy = update.y + update.height / 2;
			const rx = update.width / 2;
			const ry = update.height / 2;

			ctx.beginPath();
			if (Math.abs(sweepDeg) >= 360) {
				ctx.ellipse(cx, cy, rx, ry, 0, 0, 2 * Math.PI);
			} else {
				ctx.ellipse(cx, cy, rx, ry, 0, startAngle, endAngle, sweepDeg < 0);
			}

			if (update.filled) {
				ctx.fillStyle = colorToCSS(update.color);
				ctx.fill();
			} else {
				ctx.strokeStyle = colorToCSS(update.color);
				ctx.lineWidth = 1;
				ctx.stroke();
			}
			break;
		}
	}
}

function colorToCSS(color: number): string {
	const r = (color >> 16) & 0xff;
	const g = (color >> 8) & 0xff;
	const b = color & 0xff;
	return `rgb(${r},${g},${b})`;
}
