import { inflateRaw } from "pako";
import type { DisplayUpdate } from "./types";

interface WindowInfo {
	x: number;
	y: number;
	width: number;
	height: number;
	mapped: boolean;
}

type RenderContext =
	| CanvasRenderingContext2D
	| OffscreenCanvasRenderingContext2D;

/**
 * Owns the back buffer, window map, and pending queue for a single X11 client.
 * Rendering happens immediately when pushUpdate is called (outside React).
 * The visible canvas blits from the back buffer on each rAF.
 */
export class ClientRenderer {
	backBuffer: OffscreenCanvas;
	private ctx: OffscreenCanvasRenderingContext2D;
	private windows = new Map<string, WindowInfo>();
	dirty = false;

	constructor(width: number, height: number) {
		this.backBuffer = new OffscreenCanvas(width, height);
		const ctx = this.backBuffer.getContext("2d");
		if (!ctx) throw new Error("Failed to get offscreen 2d context");
		this.ctx = ctx;
	}

	get width() {
		return this.backBuffer.width;
	}

	get height() {
		return this.backBuffer.height;
	}

	/** Resize the back buffer immediately (for live resize feedback). */
	resize(width: number, height: number) {
		if (width === this.backBuffer.width && height === this.backBuffer.height)
			return;
		this.resizeBuffer(width, height);
		this.dirty = true;
	}

	pushUpdate(update: DisplayUpdate) {
		if (update.kind === "WindowCreated") {
			if (
				update.width > this.backBuffer.width ||
				update.height > this.backBuffer.height
			) {
				this.resizeBuffer(
					Math.max(update.width, this.backBuffer.width),
					Math.max(update.height, this.backBuffer.height),
				);
			}
		} else if (update.kind === "WindowConfigured") {
			if (
				update.width !== this.backBuffer.width ||
				update.height !== this.backBuffer.height
			) {
				this.resizeBuffer(update.width, update.height);
			}
		}
		renderUpdate(this.ctx, update, this.windows);
		this.dirty = true;
	}

	private resizeBuffer(width: number, height: number) {
		const oldData = this.ctx.getImageData(
			0,
			0,
			this.backBuffer.width,
			this.backBuffer.height,
		);
		this.backBuffer.width = width;
		this.backBuffer.height = height;
		this.ctx.putImageData(oldData, 0, 0);
	}
}

function renderUpdate(
	ctx: RenderContext,
	update: DisplayUpdate,
	windows: Map<string, WindowInfo>,
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
		case "PutImage": {
			if (update.data.length === 0) break;
			// Decode base64 string to binary, then decompress deflate
			const binaryStr = atob(update.data as unknown as string);
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
			const imageData = ctx.createImageData(update.width, update.height);
			// Server sends A8R8G8B8 (BGRA in memory on little-endian)
			// Convert to canvas RGBA format
			for (let i = 0; i < update.width * update.height; i++) {
				const srcOff = i * 4;
				const dstOff = i * 4;
				if (srcOff + 3 < rawData.length) {
					imageData.data[dstOff] = rawData[srcOff + 2]; // R
					imageData.data[dstOff + 1] = rawData[srcOff + 1]; // G
					imageData.data[dstOff + 2] = rawData[srcOff]; // B
					imageData.data[dstOff + 3] = 255; // A — always opaque from server
				}
			}
			ctx.putImageData(imageData, update.x, update.y);
			break;
		}
		// Legacy display update types — kept for backwards compatibility but
		// no longer sent by the server (all rendering is now server-side).
		case "FillRect": {
			const r = (update.color >> 16) & 0xff;
			const g = (update.color >> 8) & 0xff;
			const b = update.color & 0xff;
			ctx.fillStyle = `rgb(${r},${g},${b})`;
			ctx.fillRect(update.x, update.y, update.width, update.height);
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
		case "DrawLines": {
			if (update.points.length < 2) break;
			const r = (update.color >> 16) & 0xff;
			const g = (update.color >> 8) & 0xff;
			const b = update.color & 0xff;
			ctx.strokeStyle = `rgb(${r},${g},${b})`;
			ctx.lineWidth = Math.max(1, update.line_width);
			ctx.beginPath();
			ctx.moveTo(update.points[0][0], update.points[0][1]);
			for (let i = 1; i < update.points.length; i++) {
				ctx.lineTo(update.points[i][0], update.points[i][1]);
			}
			ctx.stroke();
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
				ctx.ellipse(
					cx,
					cy,
					rx,
					ry,
					0,
					startAngle,
					endAngle,
					sweepDeg < 0,
				);
			}
			if (update.filled) {
				const r = (update.color >> 16) & 0xff;
				const g = (update.color >> 8) & 0xff;
				const b = update.color & 0xff;
				ctx.fillStyle = `rgb(${r},${g},${b})`;
				ctx.fill();
			} else {
				const r = (update.color >> 16) & 0xff;
				const g = (update.color >> 8) & 0xff;
				const b = update.color & 0xff;
				ctx.strokeStyle = `rgb(${r},${g},${b})`;
				ctx.lineWidth = 1;
				ctx.stroke();
			}
			break;
		}
	}
}
