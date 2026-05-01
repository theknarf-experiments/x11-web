import { inflateRaw } from "pako";

/**
 * Owns the back buffer for a single X11 client. The visible canvas
 * blits from this buffer on each rAF; `pushPutImage` paints
 * synchronously outside React.
 *
 * Pixel updates ride the WebRTC DataChannel as Cap'n Proto frames
 * (decoded by `rtcWire.ts`); this renderer just receives the raw
 * deflate-compressed RGBA payload, inflates with pako, and blits.
 *
 * Per-window geometry / lifecycle is tracked centrally in `App.tsx`
 * from the backend's authoritative `WindowList`.
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

	/** Paint a PutImage rectangle from deflate-compressed RGBA bytes. */
	pushPutImage(
		x: number,
		y: number,
		width: number,
		height: number,
		compressed: Uint8Array,
	) {
		const right = x + width;
		const bottom = y + height;
		if (right > this.backBuffer.width || bottom > this.backBuffer.height) {
			this.resizeBuffer(
				Math.max(right, this.backBuffer.width),
				Math.max(bottom, this.backBuffer.height),
			);
		}
		let raw: Uint8Array;
		try {
			raw = inflateRaw(compressed);
		} catch {
			raw = compressed;
		}
		const imageData = this.ctx.createImageData(width, height);
		imageData.data.set(raw.subarray(0, imageData.data.length));
		this.ctx.putImageData(imageData, x, y);
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
