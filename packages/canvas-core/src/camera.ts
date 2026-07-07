/** Camera over an infinite 2D canvas. `x`/`y` is the canvas-space
 *  point visible at the viewport's top-left corner; `scale` is the
 *  zoom (canvas units × scale = viewport px). Renderer-agnostic: a
 *  DOM layer applies it as `scale() translate()`, a GL layer as an
 *  orthographic frustum. */
export interface Camera {
	x: number;
	y: number;
	scale: number;
}

export interface CameraLimits {
	minScale: number;
	maxScale: number;
}

export const DEFAULT_LIMITS: CameraLimits = { minScale: 0.1, maxScale: 3 };

export interface Point {
	x: number;
	y: number;
}

export function clampScale(scale: number, limits: CameraLimits): number {
	return Math.min(limits.maxScale, Math.max(limits.minScale, scale));
}

/** Convert a viewport-relative point (px from the viewport's
 *  top-left) into canvas coordinates. */
export function viewportToCanvas(cam: Camera, vx: number, vy: number): Point {
	return { x: cam.x + vx / cam.scale, y: cam.y + vy / cam.scale };
}

/** Convert a canvas-space point into viewport-relative pixels. */
export function canvasToViewport(cam: Camera, cx: number, cy: number): Point {
	return { x: (cx - cam.x) * cam.scale, y: (cy - cam.y) * cam.scale };
}

/** Zoom to `newScale` keeping the canvas point under the viewport
 *  point `(vx, vy)` anchored — the invariant behind cursor-centered
 *  wheel zoom, pinch zoom, and preset jumps alike. */
export function zoomAt(
	cam: Camera,
	vx: number,
	vy: number,
	newScale: number,
	limits: CameraLimits = DEFAULT_LIMITS,
): Camera {
	const scale = clampScale(newScale, limits);
	const anchor = viewportToCanvas(cam, vx, vy);
	return {
		x: anchor.x - vx / scale,
		y: anchor.y - vy / scale,
		scale,
	};
}

/** Pan by a viewport-pixel delta (e.g. wheel deltas). */
export function panBy(cam: Camera, dx: number, dy: number): Camera {
	return { ...cam, x: cam.x + dx / cam.scale, y: cam.y + dy / cam.scale };
}

/** Camera fitted so the canvas-space rect fills the viewport with
 *  `padding` (fraction of the rect, e.g. 0.1) around it. */
export function fitView(
	rect: { x: number; y: number; width: number; height: number },
	viewport: { width: number; height: number },
	limits: CameraLimits = DEFAULT_LIMITS,
	padding = 0.1,
): Camera {
	const w = Math.max(rect.width * (1 + padding * 2), 1);
	const h = Math.max(rect.height * (1 + padding * 2), 1);
	const scale = clampScale(
		Math.min(viewport.width / w, viewport.height / h),
		limits,
	);
	const cx = rect.x + rect.width / 2;
	const cy = rect.y + rect.height / 2;
	return {
		x: cx - viewport.width / 2 / scale,
		y: cy - viewport.height / 2 / scale,
		scale,
	};
}

/** Minimal external store for the camera, so multiple renderers (a
 *  DOM transform layer, a GL ortho camera) can follow one source of
 *  truth without threading React state through both. Compatible
 *  with `useSyncExternalStore`. */
export interface CameraStore {
	get(): Camera;
	set(next: Camera): void;
	subscribe(listener: (cam: Camera) => void): () => void;
}

export function createCameraStore(initial?: Partial<Camera>): CameraStore {
	let cam: Camera = { x: 0, y: 0, scale: 1, ...initial };
	const listeners = new Set<(cam: Camera) => void>();
	return {
		get: () => cam,
		set: (next) => {
			cam = next;
			for (const l of listeners) l(cam);
		},
		subscribe: (listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
	};
}
