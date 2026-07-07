import { describe, expect, it } from "vitest";
import {
	distToPolyline,
	distToSegment,
	pointInPolygon,
	pointInRoundedRect,
	sampleQuadratic,
} from "./hitTest.ts";

describe("pointInRoundedRect", () => {
	it("hits inside, misses outside", () => {
		expect(pointInRoundedRect(50, 50, 0, 0, 100, 100)).toBe(true);
		expect(pointInRoundedRect(101, 50, 0, 0, 100, 100)).toBe(false);
	});

	it("rounds the corners", () => {
		// (1,1) is inside the sharp corner but outside a r=10 corner disc.
		expect(pointInRoundedRect(1, 1, 0, 0, 100, 100, 0)).toBe(true);
		expect(pointInRoundedRect(1, 1, 0, 0, 100, 100, 10)).toBe(false);
		// Corner-disc surface still hits.
		expect(pointInRoundedRect(10, 1, 0, 0, 100, 100, 10)).toBe(true);
	});

	it("clamps radius to half the smaller side", () => {
		// radius 100 on a 40-tall rect behaves like radius 20.
		expect(pointInRoundedRect(20, 20, 0, 0, 200, 40, 100)).toBe(true);
	});
});

describe("distToSegment / distToPolyline", () => {
	it("measures perpendicular distance within the segment", () => {
		expect(distToSegment(5, 3, 0, 0, 10, 0)).toBeCloseTo(3);
	});

	it("measures to the nearest endpoint beyond the segment", () => {
		expect(distToSegment(-3, 4, 0, 0, 10, 0)).toBeCloseTo(5);
	});

	it("takes the minimum across polyline segments", () => {
		const pts: Array<readonly [number, number]> = [
			[0, 0],
			[10, 0],
			[10, 10],
		];
		expect(distToPolyline(11, 5, pts)).toBeCloseTo(1);
		expect(distToPolyline(0, 0, [])).toBe(Number.POSITIVE_INFINITY);
	});
});

describe("pointInPolygon", () => {
	const square: Array<readonly [number, number]> = [
		[0, 0],
		[10, 0],
		[10, 10],
		[0, 10],
	];

	it("hits inside, misses outside", () => {
		expect(pointInPolygon(5, 5, square)).toBe(true);
		expect(pointInPolygon(15, 5, square)).toBe(false);
	});

	it("handles concave polygons", () => {
		const lShape: Array<readonly [number, number]> = [
			[0, 0],
			[10, 0],
			[10, 5],
			[5, 5],
			[5, 10],
			[0, 10],
		];
		expect(pointInPolygon(2, 8, lShape)).toBe(true);
		expect(pointInPolygon(8, 8, lShape)).toBe(false);
	});
});

describe("sampleQuadratic", () => {
	it("starts and ends at the endpoints and passes near the middle", () => {
		const pts = sampleQuadratic(0, 0, 50, 100, 100, 0, 10);
		expect(pts[0]).toEqual([0, 0]);
		expect(pts[10]).toEqual([100, 0]);
		// t=0.5 of this symmetric curve is (50, 50).
		expect(pts[5][0]).toBeCloseTo(50);
		expect(pts[5][1]).toBeCloseTo(50);
	});
});
