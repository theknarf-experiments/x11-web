/** Gesture normalization shared by every renderer. Pure data-in /
 *  data-out — no DOM listener management, so it works with React
 *  synthetic events, native listeners, or tests alike. */

export type WheelIntent =
	| { type: "zoom"; factor: number }
	| { type: "pan"; dx: number; dy: number };

/** Classify a wheel event: `ctrl`/`meta` + wheel is zoom (trackpad
 *  pinches arrive as ctrl+wheel), plain wheel is pan. The zoom
 *  factor is exponential in deltaY so mouse-wheel notches and
 *  fine-grained trackpad deltas both feel proportional. */
export function wheelIntent(e: {
	deltaX: number;
	deltaY: number;
	ctrlKey: boolean;
	metaKey: boolean;
}): WheelIntent {
	if (e.ctrlKey || e.metaKey) {
		return { type: "zoom", factor: Math.exp(-e.deltaY * 0.01) };
	}
	return { type: "pan", dx: e.deltaX, dy: e.deltaY };
}

export interface PinchUpdate {
	/** Midpoint between the two touches, in the event's coord space. */
	midX: number;
	midY: number;
	/** Scale factor since the previous update (ratio of distances). */
	factor: number;
}

interface TrackedPointer {
	x: number;
	y: number;
}

/** Two-finger pinch tracking over pointer events. Only `touch`
 *  pointers participate — mouse/pen must not pair up with a stale
 *  touch into a phantom pinch. Feed every pointerdown/move/up; a
 *  non-null `move()` result means "apply this incremental zoom". */
export class PinchTracker {
	private pointers = new Map<number, TrackedPointer>();

	down(e: {
		pointerId: number;
		pointerType: string;
		clientX: number;
		clientY: number;
	}): void {
		if (e.pointerType !== "touch") return;
		this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
	}

	move(e: {
		pointerId: number;
		clientX: number;
		clientY: number;
	}): PinchUpdate | null {
		const prev = this.pointers.get(e.pointerId);
		if (!prev) return null;
		if (this.pointers.size !== 2) {
			prev.x = e.clientX;
			prev.y = e.clientY;
			return null;
		}
		let other: TrackedPointer | undefined;
		for (const [id, p] of this.pointers) {
			if (id !== e.pointerId) other = p;
		}
		if (!other) return null;
		const prevDist = Math.hypot(prev.x - other.x, prev.y - other.y);
		prev.x = e.clientX;
		prev.y = e.clientY;
		const dist = Math.hypot(prev.x - other.x, prev.y - other.y);
		if (prevDist < 1) return null;
		return {
			midX: (prev.x + other.x) / 2,
			midY: (prev.y + other.y) / 2,
			factor: dist / prevDist,
		};
	}

	up(pointerId: number): void {
		this.pointers.delete(pointerId);
	}

	/** True while two touches are down — pan/drag handlers should
	 *  stand down and let the pinch own the gesture. */
	get active(): boolean {
		return this.pointers.size >= 2;
	}
}
