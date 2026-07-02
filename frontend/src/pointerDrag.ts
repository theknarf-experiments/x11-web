/**
 * Pointer-drag primitives shared between the WindowFrame's title
 * bar (drag-to-move) and corner handles (drag-to-resize).
 *
 * `startPointerDrag` owns the boilerplate: pointer capture, the
 * `pointermove` / `pointerup` listener pair on the original
 * element, and the cleanup. The caller supplies a callback that
 * receives the live cursor delta plus the current canvas scale.
 *
 * `computeResize` is pure and table-tested — given the original
 * geometry, a scale-corrected cursor delta, and which corner the
 * user grabbed, it returns the new bounds with the opposite
 * corner pinned in canvas space.
 */

/** Read the inverse of any CSS scale applied to an ancestor that
 *  marks itself with `data-canvas-scale="<scale>"` (the
 *  InfiniteCanvas does this on its transform layer). Returns 1
 *  when no such ancestor exists. */
export function getCanvasScale(el: Element): number {
	const wrapper = el.closest("[data-canvas-scale]");
	return wrapper
		? Number.parseFloat(wrapper.getAttribute("data-canvas-scale") || "1")
		: 1;
}

interface PointerLike {
	clientX: number;
	clientY: number;
	pointerId: number;
	currentTarget: EventTarget & {
		setPointerCapture(pointerId: number): void;
		addEventListener: Element["addEventListener"];
		removeEventListener: Element["removeEventListener"];
	};
}

/** Drag delta, with `scale` already snapshotted at gesture start
 *  so callers don't have to redo the lookup on every move. The
 *  caller divides `dx` / `dy` by `scale` to convert from client
 *  pixels into canvas pixels. */
export interface DragDelta {
	dx: number;
	dy: number;
	scale: number;
}

/** Capture the pointer and stream `pointermove` deltas to `onMove`
 *  until `pointerup`. Listeners attach to `e.currentTarget` so
 *  the gesture survives the cursor leaving the element's bounds —
 *  pointer capture redirects subsequent pointer events back to
 *  it. */
export function startPointerDrag(
	e: PointerLike,
	onMove: (delta: DragDelta) => void,
): void {
	const startX = e.clientX;
	const startY = e.clientY;
	const target = e.currentTarget;
	target.setPointerCapture(e.pointerId);
	const scale = getCanvasScale(target as Element);

	const onPointerMove = (ev: Event) => {
		const { clientX, clientY } = ev as PointerEvent;
		onMove({ dx: clientX - startX, dy: clientY - startY, scale });
	};
	const onPointerUp = () => {
		target.removeEventListener("pointermove", onPointerMove);
		target.removeEventListener("pointerup", onPointerUp);
	};
	target.addEventListener("pointermove", onPointerMove);
	target.addEventListener("pointerup", onPointerUp);
}

/** Resize geometry calculation. The grabbed corner is identified
 *  by `flipX` / `flipY`: when set, the matching edge moves
 *  *opposite* to the cursor (e.g. dragging the left edge with
 *  `flipX=true` shrinks width while pinning the right edge). The
 *  opposite edge from the grabbed corner stays anchored in canvas
 *  space, which is why `x` / `y` shift to compensate. */
export interface ResizeInputs {
	origX: number;
	origY: number;
	origW: number;
	origH: number;
	/** Cursor delta in canvas pixels (already divided by scale). */
	dx: number;
	dy: number;
	flipX: boolean;
	flipY: boolean;
	minW: number;
	minH: number;
}

export interface ResizeResult {
	x: number;
	y: number;
	width: number;
	height: number;
}

export function computeResize(i: ResizeInputs): ResizeResult {
	const width = Math.max(
		i.minW,
		Math.round(i.origW + (i.flipX ? -i.dx : i.dx)),
	);
	const height = Math.max(
		i.minH,
		Math.round(i.origH + (i.flipY ? -i.dy : i.dy)),
	);
	const x = i.flipX ? i.origX + (i.origW - width) : i.origX;
	const y = i.flipY ? i.origY + (i.origH - height) : i.origY;
	return { x, y, width, height };
}
