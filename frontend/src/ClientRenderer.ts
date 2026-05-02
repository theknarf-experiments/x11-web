/**
 * Owns the back buffer for a single X11 client. The visible canvas
 * blits from this buffer on each rAF; `pushPutImage` paints
 * asynchronously outside React (decode happens via
 * `createImageBitmap`, which the browser can offload from the main
 * thread / accelerate).
 *
 * Pixel updates ride the WebRTC DataChannel as Cap'n Proto frames
 * (decoded by `rtcWire.ts`); the payload is a complete WebP-lossless
 * image, decoded natively by the browser.
 *
 * Per-window geometry / lifecycle is tracked centrally in `App.tsx`
 * from the backend's authoritative `WindowList`.
 */
export class ClientRenderer {
	backBuffer: OffscreenCanvas;
	private ctx: OffscreenCanvasRenderingContext2D;
	dirty = false;

	/** Serialise paints so out-of-order `createImageBitmap` resolutions
	 *  don't blit stale frames over fresher ones. The backend's
	 *  monotonic-msg_id reassembly already drops stale chunks — this is
	 *  belt-and-suspenders for the async decode boundary. */
	private paintChain: Promise<unknown> = Promise.resolve();

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

	/** Paint a PutImage rectangle from a WebP-encoded payload. */
	pushPutImage(
		x: number,
		y: number,
		width: number,
		height: number,
		encoded: Uint8Array,
	) {
		// Copy out of the chunked-reassembly buffer immediately so the
		// caller can free its source — `createImageBitmap` may take a
		// few ms and we want the buffer back ASAP.
		const blob = new Blob([encoded.slice()], { type: "image/webp" });
		this.paintChain = this.paintChain
			.then(() => createImageBitmap(blob))
			.then((bitmap) => {
				// No resize logic here — the back buffer's size is
				// driven exclusively by `resize()` calls from the
				// caller, which mirror the server's authoritative
				// `WindowDescriptor.{width, height}`. PutImage rectangles
				// outside the current canvas just clip; the next
				// frame at the right size paints them.
				this.ctx.drawImage(bitmap, x, y);
				bitmap.close();
				this.dirty = true;
			})
			.catch((err) => {
				console.warn("[ClientRenderer] WebP decode failed:", err);
			});
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
