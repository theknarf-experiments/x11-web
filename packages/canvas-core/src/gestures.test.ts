import { describe, expect, it } from "vitest";
import { PinchTracker, wheelIntent } from "./gestures.ts";

describe("wheelIntent", () => {
	it("treats plain wheel as pan", () => {
		expect(
			wheelIntent({ deltaX: 3, deltaY: -7, ctrlKey: false, metaKey: false }),
		).toEqual({ type: "pan", dx: 3, dy: -7 });
	});

	it("treats ctrl/meta wheel as zoom, scrolling up zooms in", () => {
		const zoomIn = wheelIntent({
			deltaX: 0,
			deltaY: -10,
			ctrlKey: true,
			metaKey: false,
		});
		const zoomOut = wheelIntent({
			deltaX: 0,
			deltaY: 10,
			ctrlKey: false,
			metaKey: true,
		});
		if (zoomIn.type !== "zoom" || zoomOut.type !== "zoom") {
			throw new Error("expected zoom intents");
		}
		expect(zoomIn.factor).toBeGreaterThan(1);
		expect(zoomOut.factor).toBeLessThan(1);
		// Symmetric: in then out cancels.
		expect(zoomIn.factor * zoomOut.factor).toBeCloseTo(1);
	});
});

describe("PinchTracker", () => {
	const touch = (pointerId: number, x: number, y: number) => ({
		pointerId,
		pointerType: "touch",
		clientX: x,
		clientY: y,
	});

	it("reports factor and midpoint while two touches move apart", () => {
		const t = new PinchTracker();
		t.down(touch(1, 100, 100));
		t.down(touch(2, 200, 100));
		expect(t.active).toBe(true);
		// Move pointer 2 from 100px away to 150px away.
		const update = t.move({ pointerId: 2, clientX: 250, clientY: 100 });
		expect(update).not.toBeNull();
		expect(update?.factor).toBeCloseTo(1.5);
		expect(update?.midX).toBeCloseTo(175);
		expect(update?.midY).toBeCloseTo(100);
	});

	it("goes inactive when a finger lifts and stops reporting", () => {
		const t = new PinchTracker();
		t.down(touch(1, 0, 0));
		t.down(touch(2, 100, 0));
		t.up(2);
		expect(t.active).toBe(false);
		expect(t.move({ pointerId: 1, clientX: 5, clientY: 0 })).toBeNull();
	});

	it("ignores mouse pointers entirely", () => {
		const t = new PinchTracker();
		t.down({ pointerId: 1, pointerType: "mouse", clientX: 0, clientY: 0 });
		t.down(touch(2, 100, 0));
		expect(t.active).toBe(false);
		expect(t.move({ pointerId: 1, clientX: 10, clientY: 0 })).toBeNull();
	});
});
