// @vitest-environment jsdom
import { describe, expect, test, vi } from "vitest";
import {
	computeResize,
	getCanvasScale,
	startPointerDrag,
} from "./pointerDrag";

describe("computeResize", () => {
	const base = {
		origX: 100,
		origY: 200,
		origW: 400,
		origH: 300,
		minW: 50,
		minH: 50,
	};

	test("south-east corner: bottom + right edges follow cursor", () => {
		// flipX=false, flipY=false → top-left pinned.
		const r = computeResize({
			...base,
			dx: 30,
			dy: -10,
			flipX: false,
			flipY: false,
		});
		expect(r).toEqual({ x: 100, y: 200, width: 430, height: 290 });
	});

	test("north-west corner: top + left edges follow cursor", () => {
		// flipX=true, flipY=true → bottom-right pinned.
		const r = computeResize({
			...base,
			dx: -20,
			dy: -40,
			flipX: true,
			flipY: true,
		});
		expect(r.width).toBe(420);
		expect(r.height).toBe(340);
		// origX shifts left as width grew on the left side: 100 + (400 - 420) = 80
		expect(r.x).toBe(80);
		expect(r.y).toBe(160);
	});

	test("south-west corner: only left edge follows cursor", () => {
		const r = computeResize({
			...base,
			dx: 50, // dragging right
			dy: 30,
			flipX: true,
			flipY: false,
		});
		// width shrinks by dx (flipped sign), height grows by dy
		expect(r).toEqual({
			x: 100 + (400 - 350), // 150
			y: 200,
			width: 350,
			height: 330,
		});
	});

	test("clamps to min width / height and pins the opposite edge", () => {
		// Try to collapse from the north-west corner.
		const r = computeResize({
			...base,
			dx: 1000,
			dy: 1000,
			flipX: true,
			flipY: true,
			minW: 50,
			minH: 50,
		});
		expect(r.width).toBe(50);
		expect(r.height).toBe(50);
		// Bottom-right corner (origX + origW = 500, origY + origH = 500) stays fixed.
		expect(r.x + r.width).toBe(500);
		expect(r.y + r.height).toBe(500);
	});

	test("rounds fractional deltas before clamping", () => {
		const r = computeResize({
			...base,
			dx: 12.7,
			dy: -3.3,
			flipX: false,
			flipY: false,
		});
		expect(r.width).toBe(413);
		expect(r.height).toBe(297);
	});
});

describe("getCanvasScale", () => {
	test("reads `data-canvas-scale` from the nearest ancestor", () => {
		const wrap = document.createElement("div");
		wrap.setAttribute("data-canvas-scale", "1.5");
		const child = document.createElement("button");
		wrap.appendChild(child);
		document.body.appendChild(wrap);
		expect(getCanvasScale(child)).toBe(1.5);
		document.body.removeChild(wrap);
	});

	test("returns 1 when no canvas-scale ancestor exists", () => {
		const el = document.createElement("button");
		document.body.appendChild(el);
		expect(getCanvasScale(el)).toBe(1);
		document.body.removeChild(el);
	});
});

describe("startPointerDrag", () => {
	function setup() {
		const target = document.createElement("button");
		document.body.appendChild(target);
		// jsdom doesn't implement pointer capture; stub it so the
		// drag helper's call doesn't throw. The capture has no
		// observable effect on these listener-based tests anyway.
		target.setPointerCapture = vi.fn();
		const onMove = vi.fn();
		const e = {
			clientX: 100,
			clientY: 200,
			pointerId: 7,
			currentTarget: target,
		};
		startPointerDrag(e, onMove);
		return { target, onMove, e };
	}

	test("captures the pointer + reports cursor delta on pointermove", () => {
		const { target, onMove } = setup();
		expect(target.setPointerCapture).toHaveBeenCalledWith(7);

		target.dispatchEvent(
			new (class extends Event {
				clientX = 130;
				clientY = 215;
				constructor() {
					super("pointermove", { bubbles: true });
				}
			})(),
		);
		expect(onMove).toHaveBeenCalledWith({
			dx: 30,
			dy: 15,
			scale: 1, // no canvas-scale ancestor → 1
		});
	});

	test("snapshots `data-canvas-scale` once at gesture start", () => {
		const wrap = document.createElement("div");
		wrap.setAttribute("data-canvas-scale", "2");
		document.body.appendChild(wrap);
		const target = document.createElement("button");
		wrap.appendChild(target);
		target.setPointerCapture = vi.fn();
		const onMove = vi.fn();
		startPointerDrag(
			{ clientX: 0, clientY: 0, pointerId: 1, currentTarget: target },
			onMove,
		);
		// Even if the ancestor's scale changes mid-drag, the value
		// we report stays at the snapshot.
		wrap.setAttribute("data-canvas-scale", "0.5");
		target.dispatchEvent(
			new (class extends Event {
				clientX = 40;
				clientY = 0;
				constructor() {
					super("pointermove", { bubbles: true });
				}
			})(),
		);
		expect(onMove).toHaveBeenCalledWith({ dx: 40, dy: 0, scale: 2 });
		document.body.removeChild(wrap);
	});

	test("pointerup detaches the listeners — further moves don't call onMove", () => {
		const { target, onMove } = setup();
		target.dispatchEvent(new Event("pointerup"));
		onMove.mockClear();
		target.dispatchEvent(
			new (class extends Event {
				clientX = 999;
				clientY = 999;
				constructor() {
					super("pointermove", { bubbles: true });
				}
			})(),
		);
		expect(onMove).not.toHaveBeenCalled();
	});
});
