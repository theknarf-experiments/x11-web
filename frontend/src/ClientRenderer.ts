import { inflateRaw } from "pako";
import type { DisplayUpdate } from "./types";

type RenderContext =
	| CanvasRenderingContext2D
	| OffscreenCanvasRenderingContext2D;

/**
 * Owns the back buffer for a single X11 client. The visible canvas
 * blits from this buffer on each rAF; pushUpdate paints synchronously
 * outside React.
 *
 * Per-window geometry / lifecycle is tracked centrally in `App.tsx`
 * from the backend's authoritative `WindowList`; the renderer only
 * needs to handle pixel updates (`PutImage`) — everything else is a
 * no-op here.
 */
export class ClientRenderer {
	backBuffer: OffscreenCanvas;
	private ctx: OffscreenCanvasRenderingContext2D;
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
		if (update.kind !== "PutImage") return;
		// Grow the back buffer if a region update extends past current bounds.
		const right = update.x + update.width;
		const bottom = update.y + update.height;
		if (right > this.backBuffer.width || bottom > this.backBuffer.height) {
			this.resizeBuffer(
				Math.max(right, this.backBuffer.width),
				Math.max(bottom, this.backBuffer.height),
			);
		}
		renderPutImage(this.ctx, update);
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

function renderPutImage(
	ctx: RenderContext,
	update: Extract<DisplayUpdate, { kind: "PutImage" }>,
) {
	if (update.data.length === 0) return;
	// Decode base64 string to binary, then decompress deflate.
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
	// Server sends packed RGBA, the canvas ImageData layout — copy directly.
	imageData.data.set(rawData.subarray(0, imageData.data.length));
	ctx.putImageData(imageData, update.x, update.y);
}
