import { describe, expect, it } from "vitest";
import {
	canvasToViewport,
	createCameraStore,
	fitView,
	panBy,
	viewportToCanvas,
	zoomAt,
} from "./camera.ts";

describe("zoomAt", () => {
	it("keeps the anchor point stationary across a zoom", () => {
		const cam = { x: 37, y: -12, scale: 0.8 };
		const before = viewportToCanvas(cam, 200, 150);
		const zoomed = zoomAt(cam, 200, 150, 2);
		const after = viewportToCanvas(zoomed, 200, 150);
		expect(after.x).toBeCloseTo(before.x);
		expect(after.y).toBeCloseTo(before.y);
		expect(zoomed.scale).toBe(2);
	});

	it("clamps to the limits", () => {
		const cam = { x: 0, y: 0, scale: 1 };
		expect(zoomAt(cam, 0, 0, 99).scale).toBe(3);
		expect(zoomAt(cam, 0, 0, 0.0001).scale).toBe(0.1);
		expect(zoomAt(cam, 0, 0, 99, { minScale: 0.05, maxScale: 80 }).scale).toBe(
			80,
		);
	});
});

describe("coordinate conversion", () => {
	it("round-trips viewport -> canvas -> viewport", () => {
		const cam = { x: 100, y: 250, scale: 1.5 };
		const c = viewportToCanvas(cam, 60, 90);
		const v = canvasToViewport(cam, c.x, c.y);
		expect(v.x).toBeCloseTo(60);
		expect(v.y).toBeCloseTo(90);
	});
});

describe("panBy", () => {
	it("pans in canvas units scaled by zoom", () => {
		const cam = panBy({ x: 0, y: 0, scale: 2 }, 10, -20);
		expect(cam.x).toBe(5);
		expect(cam.y).toBe(-10);
	});
});

describe("fitView", () => {
	it("centers the rect and fits the tighter axis", () => {
		const cam = fitView(
			{ x: 0, y: 0, width: 100, height: 100 },
			{ width: 400, height: 200 },
			{ minScale: 0.01, maxScale: 100 },
			0,
		);
		expect(cam.scale).toBe(2); // limited by the 200px-tall viewport
		// Rect center (50,50) lands at the viewport center.
		const v = canvasToViewport(cam, 50, 50);
		expect(v.x).toBeCloseTo(200);
		expect(v.y).toBeCloseTo(100);
	});
});

describe("createCameraStore", () => {
	it("notifies subscribers and supports unsubscribe", () => {
		const store = createCameraStore();
		const seen: number[] = [];
		const off = store.subscribe((cam) => seen.push(cam.scale));
		store.set({ x: 0, y: 0, scale: 2 });
		off();
		store.set({ x: 0, y: 0, scale: 3 });
		expect(seen).toEqual([2]);
		expect(store.get().scale).toBe(3);
	});
});
