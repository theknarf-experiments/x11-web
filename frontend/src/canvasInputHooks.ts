import { type RefObject, useEffect } from "react";
import {
	type CanvasGeometry,
	clientToCanvas,
	mouseButtonMask,
} from "./inputProtocol";
import type { InputEvent } from "./types";

/**
 * Per-canvas input hooks that emit X11 protocol `InputEvent`s.
 * Each hook lives in its own `useEffect`; the actual DOM wiring
 * is delegated to a corresponding `attach*` function so the
 * hook bodies stay 5 lines and the DOM logic can be tested in
 * isolation without mounting a React tree.
 *
 * All five hooks read the latest `onInput` through a ref pattern
 * the caller already holds — they take a plain `onInput` and
 * close over it via a sentinel ref so the listeners don't need
 * to be re-bound on every render.
 */

type OnInput = (event: InputEvent) => void;

/** Wheel scroll → X11 buttons 4/5 (vertical) and 6/7 (horizontal).
 *  The browser fires many small `wheel` events for a single
 *  trackpad gesture, so we accumulate deltas and only emit a
 *  press/release pair once a configurable threshold is crossed.
 *  Each crossing also re-emits a `MotionNotify` so the X server
 *  knows where the wheel "click" landed. */
const WHEEL_THRESHOLD = 15;

interface WheelAccumulator {
	accY: number;
	accX: number;
}

/** Pure helper: feed one `wheel` event through the accumulator
 *  and produce zero or more `InputEvent`s plus the new state. The
 *  threshold loop is the easiest piece to silently regress, so
 *  this is the bit that actually gets unit-tested. */
export function wheelAccumulate(
	state: WheelAccumulator,
	deltaX: number,
	deltaY: number,
	x: number,
	y: number,
	buttonsState: number,
	threshold: number = WHEEL_THRESHOLD,
): { state: WheelAccumulator; events: InputEvent[] } {
	const events: InputEvent[] = [];
	let accY = state.accY + deltaY;
	let accX = state.accX + deltaX;

	if (Math.abs(accY) >= threshold || Math.abs(accX) >= threshold) {
		events.push({ kind: "MotionNotify", x, y, state: buttonsState });
	}
	while (Math.abs(accY) >= threshold) {
		const button = accY > 0 ? 5 : 4;
		events.push({ kind: "ButtonPress", button, x, y, state: buttonsState });
		events.push({ kind: "ButtonRelease", button, x, y, state: buttonsState });
		accY -= Math.sign(accY) * threshold;
	}
	while (Math.abs(accX) >= threshold) {
		const button = accX > 0 ? 7 : 6;
		events.push({ kind: "ButtonPress", button, x, y, state: buttonsState });
		events.push({ kind: "ButtonRelease", button, x, y, state: buttonsState });
		accX -= Math.sign(accX) * threshold;
	}
	return { state: { accY, accX }, events };
}

/** Read the canvas's current geometry — bounding rect plus
 *  intrinsic dimensions — for `clientToCanvas`. Inline so the
 *  hooks don't all duplicate it. */
function geom(el: HTMLCanvasElement): CanvasGeometry {
	return {
		rect: el.getBoundingClientRect(),
		width: el.width,
		height: el.height,
	};
}

export function attachWheelInput(
	el: HTMLCanvasElement,
	onInput: OnInput,
): () => void {
	let state: WheelAccumulator = { accY: 0, accX: 0 };
	const onWheel = (e: WheelEvent) => {
		e.preventDefault();
		e.stopPropagation();
		const { x, y } = clientToCanvas(e.clientX, e.clientY, geom(el));
		const result = wheelAccumulate(
			state,
			e.deltaX,
			e.deltaY,
			x,
			y,
			mouseButtonMask(e.buttons),
		);
		state = result.state;
		for (const ev of result.events) onInput(ev);
	};
	el.addEventListener("wheel", onWheel, { passive: false });
	return () => el.removeEventListener("wheel", onWheel);
}

export function useWheelInput(
	ref: RefObject<HTMLCanvasElement | null>,
	onInputRef: RefObject<OnInput>,
): void {
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		return attachWheelInput(el, (ev) => onInputRef.current(ev));
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);
}

/* ------------------------------------------------------------------
 *  Touch input — XInput2 TouchBegin/Update/End passthrough
 * ------------------------------------------------------------------ */

export function attachTouchInput(
	el: HTMLCanvasElement,
	onInput: OnInput,
): () => void {
	const emit = (
		kind: "TouchBegin" | "TouchUpdate" | "TouchEnd",
		e: TouchEvent,
	) => {
		const g = geom(el);
		for (let i = 0; i < e.changedTouches.length; i++) {
			const t = e.changedTouches[i];
			const { x, y } = clientToCanvas(t.clientX, t.clientY, g);
			onInput({ kind, touch_id: t.identifier, x, y, state: 0 });
		}
	};
	const onStart = (e: TouchEvent) => {
		e.preventDefault();
		emit("TouchBegin", e);
	};
	const onMove = (e: TouchEvent) => {
		e.preventDefault();
		emit("TouchUpdate", e);
	};
	const onEnd = (e: TouchEvent) => {
		e.preventDefault();
		emit("TouchEnd", e);
	};
	el.addEventListener("touchstart", onStart, { passive: false });
	el.addEventListener("touchmove", onMove, { passive: false });
	el.addEventListener("touchend", onEnd, { passive: false });
	el.addEventListener("touchcancel", onEnd, { passive: false });
	return () => {
		el.removeEventListener("touchstart", onStart);
		el.removeEventListener("touchmove", onMove);
		el.removeEventListener("touchend", onEnd);
		el.removeEventListener("touchcancel", onEnd);
	};
}

export function useTouchInput(
	ref: RefObject<HTMLCanvasElement | null>,
	onInputRef: RefObject<OnInput>,
): void {
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		return attachTouchInput(el, (ev) => onInputRef.current(ev));
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);
}

/* ------------------------------------------------------------------
 *  Pinch (two-finger) gesture — emits `GesturePinch`
 * ------------------------------------------------------------------ */

/** Distance between two touch points. */
export function touchPairDistance(t0: Touch, t1: Touch): number {
	const dx = t1.clientX - t0.clientX;
	const dy = t1.clientY - t0.clientY;
	return Math.sqrt(dx * dx + dy * dy);
}

/** Angle (radians) between two touch points. */
export function touchPairAngle(t0: Touch, t1: Touch): number {
	return Math.atan2(t1.clientY - t0.clientY, t1.clientX - t0.clientX);
}

export function attachPinchGesture(
	el: HTMLCanvasElement,
	onInput: OnInput,
): () => void {
	let initialDistance = 0;
	let initialAngle = 0;
	let lastScale = 1;
	let active = false;

	const midCanvas = (touches: TouchList): { x: number; y: number } => {
		if (touches.length < 2) return { x: 0, y: 0 };
		const cx = (touches[0].clientX + touches[1].clientX) / 2;
		const cy = (touches[0].clientY + touches[1].clientY) / 2;
		return clientToCanvas(cx, cy, geom(el));
	};

	const onStart = (e: TouchEvent) => {
		if (e.touches.length === 2 && !active) {
			active = true;
			initialDistance = touchPairDistance(e.touches[0], e.touches[1]);
			initialAngle = touchPairAngle(e.touches[0], e.touches[1]);
			lastScale = 1;
			const m = midCanvas(e.touches);
			onInput({
				kind: "GesturePinch",
				dx: m.x,
				dy: m.y,
				scale: 1,
				rotation: 0,
				fingers: 2,
				phase: "Begin",
			});
		}
	};

	const onMove = (e: TouchEvent) => {
		if (active && e.touches.length >= 2) {
			const d = touchPairDistance(e.touches[0], e.touches[1]);
			const a = touchPairAngle(e.touches[0], e.touches[1]);
			const scale = initialDistance > 0 ? d / initialDistance : 1;
			const rotation = ((a - initialAngle) * 180) / Math.PI;
			const m = midCanvas(e.touches);
			lastScale = scale;
			onInput({
				kind: "GesturePinch",
				dx: m.x,
				dy: m.y,
				scale,
				rotation,
				fingers: 2,
				phase: "Update",
			});
		}
	};

	const onEnd = (e: TouchEvent) => {
		if (active && e.touches.length < 2) {
			active = false;
			onInput({
				kind: "GesturePinch",
				dx: 0,
				dy: 0,
				scale: lastScale,
				rotation: 0,
				fingers: 2,
				phase: "End",
			});
		}
	};

	el.addEventListener("touchstart", onStart, { passive: false });
	el.addEventListener("touchmove", onMove, { passive: false });
	el.addEventListener("touchend", onEnd, { passive: false });
	el.addEventListener("touchcancel", onEnd, { passive: false });
	return () => {
		el.removeEventListener("touchstart", onStart);
		el.removeEventListener("touchmove", onMove);
		el.removeEventListener("touchend", onEnd);
		el.removeEventListener("touchcancel", onEnd);
	};
}

export function usePinchGesture(
	ref: RefObject<HTMLCanvasElement | null>,
	onInputRef: RefObject<OnInput>,
): void {
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		return attachPinchGesture(el, (ev) => onInputRef.current(ev));
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);
}

/* ------------------------------------------------------------------
 *  Swipe (3+ finger) gesture — emits `GestureSwipe`
 * ------------------------------------------------------------------ */

/** Centroid of a `TouchList` in client coords. */
export function touchListCenter(touches: TouchList): { x: number; y: number } {
	let sx = 0;
	let sy = 0;
	for (let i = 0; i < touches.length; i++) {
		sx += touches[i].clientX;
		sy += touches[i].clientY;
	}
	return touches.length > 0 ? { x: sx / touches.length, y: sy / touches.length } : { x: 0, y: 0 };
}

export function attachSwipeGesture(
	el: HTMLCanvasElement,
	onInput: OnInput,
): () => void {
	let active = false;
	let lastCenterX = 0;
	let lastCenterY = 0;
	let fingerCount = 0;

	const onStart = (e: TouchEvent) => {
		// Only activate for 3+ fingers; 2-finger is handled by pinch.
		if (e.touches.length >= 3 && !active) {
			active = true;
			fingerCount = e.touches.length;
			const c = touchListCenter(e.touches);
			lastCenterX = c.x;
			lastCenterY = c.y;
			onInput({
				kind: "GestureSwipe",
				dx: 0,
				dy: 0,
				fingers: fingerCount,
				phase: "Begin",
			});
		} else if (active && e.touches.length > fingerCount) {
			// Additional fingers joined an active swipe.
			fingerCount = e.touches.length;
		}
	};

	const onMove = (e: TouchEvent) => {
		if (!active) return;
		const c = touchListCenter(e.touches);
		const g = geom(el);
		const scaleX = g.width / g.rect.width;
		const scaleY = g.height / g.rect.height;
		const dx = (c.x - lastCenterX) * scaleX;
		const dy = (c.y - lastCenterY) * scaleY;
		lastCenterX = c.x;
		lastCenterY = c.y;
		onInput({
			kind: "GestureSwipe",
			dx,
			dy,
			fingers: fingerCount,
			phase: "Update",
		});
	};

	const onEnd = (e: TouchEvent) => {
		if (active && e.touches.length < 3) {
			active = false;
			onInput({
				kind: "GestureSwipe",
				dx: 0,
				dy: 0,
				fingers: fingerCount,
				phase: "End",
			});
			fingerCount = 0;
		}
	};

	el.addEventListener("touchstart", onStart, { passive: false });
	el.addEventListener("touchmove", onMove, { passive: false });
	el.addEventListener("touchend", onEnd, { passive: false });
	el.addEventListener("touchcancel", onEnd, { passive: false });
	return () => {
		el.removeEventListener("touchstart", onStart);
		el.removeEventListener("touchmove", onMove);
		el.removeEventListener("touchend", onEnd);
		el.removeEventListener("touchcancel", onEnd);
	};
}

export function useSwipeGesture(
	ref: RefObject<HTMLCanvasElement | null>,
	onInputRef: RefObject<OnInput>,
): void {
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		return attachSwipeGesture(el, (ev) => onInputRef.current(ev));
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);
}

/* ------------------------------------------------------------------
 *  HTML5 drag-and-drop bridge → XdndDrop protocol
 * ------------------------------------------------------------------ */

/** Translate the browser's `dataTransfer.types` strings into the
 *  X11/Xdnd MIME types we actually forward. The browser exposes
 *  `Files` for any file drop; X11 apps speak in real MIME types,
 *  so we surface that as `application/octet-stream`. */
export function dragTypesToMimeTypes(types: readonly string[]): string[] {
	const mimeTypes: string[] = [];
	if (types.includes("text/plain")) mimeTypes.push("text/plain");
	if (types.includes("text/html")) mimeTypes.push("text/html");
	if (types.includes("text/uri-list")) mimeTypes.push("text/uri-list");
	if (types.includes("Files")) mimeTypes.push("application/octet-stream");
	return mimeTypes;
}

export function attachDndBridge(
	el: HTMLCanvasElement,
	onInput: OnInput,
): () => void {
	const onDragEnter = (e: DragEvent) => {
		e.preventDefault();
		const types = Array.from(e.dataTransfer?.types ?? []);
		onInput({
			kind: "DndBridge",
			event: { kind: "Enter", mime_types: dragTypesToMimeTypes(types) },
		});
	};

	const onDragOver = (e: DragEvent) => {
		e.preventDefault();
		if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
		const { x, y } = clientToCanvas(e.clientX, e.clientY, geom(el));
		onInput({
			kind: "DndBridge",
			event: { kind: "Position", x, y },
		});
	};

	const onDrop = (e: DragEvent) => {
		e.preventDefault();
		const dt = e.dataTransfer;
		if (!dt) return;

		const enc = new TextEncoder();
		const text = dt.getData("text/plain");
		if (text) {
			onInput({
				kind: "DndBridge",
				event: { kind: "Drop", mime_type: "text/plain", data: enc.encode(text) },
			});
			return;
		}

		const html = dt.getData("text/html");
		if (html) {
			onInput({
				kind: "DndBridge",
				event: { kind: "Drop", mime_type: "text/html", data: enc.encode(html) },
			});
			return;
		}

		if (dt.files.length > 0) {
			const file = dt.files[0];
			const reader = new FileReader();
			reader.onloadend = () => {
				const result = reader.result as ArrayBuffer;
				onInput({
					kind: "DndBridge",
					event: {
						kind: "Drop",
						mime_type: file.type || "application/octet-stream",
						data: new Uint8Array(result),
					},
				});
			};
			reader.readAsArrayBuffer(file);
		}
	};

	const onDragLeave = (e: DragEvent) => {
		e.preventDefault();
		onInput({ kind: "DndBridge", event: { kind: "Leave" } });
	};

	el.addEventListener("dragenter", onDragEnter);
	el.addEventListener("dragover", onDragOver);
	el.addEventListener("drop", onDrop);
	el.addEventListener("dragleave", onDragLeave);
	return () => {
		el.removeEventListener("dragenter", onDragEnter);
		el.removeEventListener("dragover", onDragOver);
		el.removeEventListener("drop", onDrop);
		el.removeEventListener("dragleave", onDragLeave);
	};
}

export function useDndBridge(
	ref: RefObject<HTMLCanvasElement | null>,
	onInputRef: RefObject<OnInput>,
): void {
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		return attachDndBridge(el, (ev) => onInputRef.current(ev));
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);
}
