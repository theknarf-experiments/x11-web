import { useEffect, useRef } from "react";
import type { DisplayUpdate } from "./types";
import s from "./X11Canvas.module.css";

interface X11CanvasProps {
	updates: DisplayUpdate[];
	width?: number;
	height?: number;
}

interface WindowInfo {
	x: number;
	y: number;
	width: number;
	height: number;
	mapped: boolean;
}

export function X11Canvas({
	updates,
	width = 1024,
	height = 768,
}: X11CanvasProps) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const windowsRef = useRef<Map<number, WindowInfo>>(new Map());
	const processedRef = useRef<number>(0);

	useEffect(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;
		const ctx = canvas.getContext("2d");
		if (!ctx) return;

		// Process only new updates
		const startIdx = processedRef.current;
		const newUpdates = updates.slice(startIdx);
		processedRef.current = updates.length;

		for (const update of newUpdates) {
			renderUpdate(ctx, update, windowsRef.current);
		}
	}, [updates]);

	return (
		<canvas
			ref={canvasRef}
			width={width}
			height={height}
			className={s.canvas}
			data-testid="x11-canvas"
		/>
	);
}

function colorToCSS(color: number): string {
	const r = (color >> 16) & 0xff;
	const g = (color >> 8) & 0xff;
	const b = color & 0xff;
	return `rgb(${r},${g},${b})`;
}

function renderUpdate(
	ctx: CanvasRenderingContext2D,
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
				// Clear old position
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
			// X11 ZPixmap format: BGRA (or BGRX with 32bpp on 24-depth)
			for (let i = 0; i < update.width * update.height; i++) {
				const srcOff = i * 4;
				const dstOff = i * 4;
				if (srcOff + 3 < update.data.length) {
					imageData.data[dstOff] = update.data[srcOff + 2]; // R (from B position)
					imageData.data[dstOff + 1] = update.data[srcOff + 1]; // G
					imageData.data[dstOff + 2] = update.data[srcOff]; // B (from R position)
					imageData.data[dstOff + 3] = 255; // A
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
			// X11 angles are in 64ths of a degree, counter-clockwise from 3 o'clock
			// Canvas angles are clockwise from 3 o'clock
			const startDeg = update.angle1 / 64;
			const sweepDeg = update.angle2 / 64;
			// Convert to Canvas radians (negate for clockwise)
			const startAngle = -startDeg * (Math.PI / 180);
			const endAngle = -(startDeg + sweepDeg) * (Math.PI / 180);
			const cx = update.x + update.width / 2;
			const cy = update.y + update.height / 2;
			const rx = update.width / 2;
			const ry = update.height / 2;

			ctx.beginPath();
			// For sweeps >= 360°, draw a full ellipse to avoid Canvas arc direction issues
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
