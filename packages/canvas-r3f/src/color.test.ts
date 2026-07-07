import { describe, expect, it } from "vitest";
import { parseColor } from "./color.ts";

describe("parseColor", () => {
	it("parses 6-digit hex as opaque", () => {
		expect(parseColor("#ff0000")).toEqual({ r: 1, g: 0, b: 0, a: 1 });
	});

	it("parses 8-digit hex alpha", () => {
		const c = parseColor("#00ff0080");
		expect(c.g).toBe(1);
		expect(c.a).toBeCloseTo(0.5, 1);
	});

	it("parses shorthand hex", () => {
		expect(parseColor("#fff")).toEqual({ r: 1, g: 1, b: 1, a: 1 });
		expect(parseColor("#f00c").a).toBeCloseTo(0.8);
	});

	it("parses rgb()/rgba()", () => {
		expect(parseColor("rgb(255, 0, 0)")).toEqual({ r: 1, g: 0, b: 0, a: 1 });
		expect(parseColor("rgba(0, 0, 255, 0.25)").a).toBeCloseTo(0.25);
	});

	it("maps transparent and empty to alpha 0", () => {
		expect(parseColor("transparent").a).toBe(0);
		expect(parseColor(undefined).a).toBe(0);
	});

	it("renders unknown input as visible magenta, not invisible", () => {
		expect(parseColor("mauve-ish")).toEqual({ r: 1, g: 0, b: 1, a: 1 });
	});
});
