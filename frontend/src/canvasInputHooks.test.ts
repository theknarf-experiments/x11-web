// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import {
	attachDndBridge,
	attachPinchGesture,
	attachSwipeGesture,
	attachTouchInput,
	attachWheelInput,
	dragTypesToMimeTypes,
	touchListCenter,
	touchPairAngle,
	touchPairDistance,
	wheelAccumulate,
} from "./canvasInputHooks";
import type { InputEvent } from "./types";

/** Build a canvas-shaped element with a known geometry. The
 *  hooks read `width` / `height` (intrinsic) and the bounding
 *  rect (CSS), so we override `getBoundingClientRect` to return
 *  a stable rect. */
function makeCanvas(): HTMLCanvasElement {
	const el = document.createElement("canvas");
	el.width = 800;
	el.height = 600;
	const rect = {
		left: 0,
		top: 0,
		right: 800,
		bottom: 600,
		width: 800,
		height: 600,
		x: 0,
		y: 0,
		toJSON() {
			return {};
		},
	};
	el.getBoundingClientRect = () => rect as DOMRect;
	document.body.appendChild(el);
	return el;
}

let canvas: HTMLCanvasElement;
let captured: InputEvent[];
let cleanup: () => void = () => {};

beforeEach(() => {
	canvas = makeCanvas();
	captured = [];
});

afterEach(() => {
	cleanup();
	document.body.removeChild(canvas);
});

/* ------------------------------------------------------------------
 *  Pure helpers
 * ------------------------------------------------------------------ */

describe("wheelAccumulate", () => {
	const stateAt = (accY: number, accX: number) => ({ accY, accX });

	test("emits nothing under threshold", () => {
		const r = wheelAccumulate(stateAt(0, 0), 0, 5, 10, 20, 0);
		expect(r.events).toEqual([]);
		expect(r.state).toEqual({ accY: 5, accX: 0 });
	});

	test("emits MotionNotify + press/release on first vertical crossing (down)", () => {
		const r = wheelAccumulate(stateAt(0, 0), 0, 20, 50, 60, 0x100);
		// Down scroll = button 5
		expect(r.events).toEqual([
			{ kind: "MotionNotify", x: 50, y: 60, state: 0x100 },
			{ kind: "ButtonPress", button: 5, x: 50, y: 60, state: 0x100 },
			{ kind: "ButtonRelease", button: 5, x: 50, y: 60, state: 0x100 },
		]);
		// 20 in, threshold 15 out → 5 carried
		expect(r.state).toEqual({ accY: 5, accX: 0 });
	});

	test("emits multiple ticks when delta exceeds 2× threshold", () => {
		const r = wheelAccumulate(stateAt(0, 0), 0, 35, 0, 0, 0);
		const presses = r.events.filter((e) => e.kind === "ButtonPress");
		expect(presses).toHaveLength(2);
		// 35 - 15 - 15 = 5 carried
		expect(r.state.accY).toBe(5);
	});

	test("up scroll uses button 4", () => {
		const r = wheelAccumulate(stateAt(0, 0), 0, -20, 0, 0, 0);
		expect(r.events.filter((e) => e.kind === "ButtonPress")).toEqual([
			{ kind: "ButtonPress", button: 4, x: 0, y: 0, state: 0 },
		]);
	});

	test("horizontal scroll uses buttons 6/7", () => {
		// Right (positive X) → 7
		const right = wheelAccumulate(stateAt(0, 0), 20, 0, 0, 0, 0);
		expect(
			right.events.find((e) => e.kind === "ButtonPress"),
		).toMatchObject({ button: 7 });
		// Left (negative X) → 6
		const left = wheelAccumulate(stateAt(0, 0), -20, 0, 0, 0, 0);
		expect(
			left.events.find((e) => e.kind === "ButtonPress"),
		).toMatchObject({ button: 6 });
	});

	test("preserves carry across calls", () => {
		const a = wheelAccumulate(stateAt(0, 0), 0, 10, 0, 0, 0);
		// 10 in, no events; carry 10
		expect(a.events).toEqual([]);
		const b = wheelAccumulate(a.state, 0, 10, 0, 0, 0);
		// 10 + 10 = 20, one tick, carry 5
		expect(b.events.filter((e) => e.kind === "ButtonPress")).toHaveLength(1);
		expect(b.state.accY).toBe(5);
	});
});

describe("touchPairDistance / touchPairAngle", () => {
	test("distance of horizontal pair equals dx", () => {
		const a = { clientX: 0, clientY: 0 } as Touch;
		const b = { clientX: 30, clientY: 0 } as Touch;
		expect(touchPairDistance(a, b)).toBe(30);
		expect(touchPairAngle(a, b)).toBe(0);
	});

	test("3-4-5 right triangle distance is 5", () => {
		const a = { clientX: 0, clientY: 0 } as Touch;
		const b = { clientX: 3, clientY: 4 } as Touch;
		expect(touchPairDistance(a, b)).toBe(5);
	});

	test("vertical pair has angle π/2", () => {
		const a = { clientX: 0, clientY: 0 } as Touch;
		const b = { clientX: 0, clientY: 10 } as Touch;
		expect(touchPairAngle(a, b)).toBeCloseTo(Math.PI / 2);
	});
});

describe("touchListCenter", () => {
	test("centroid of three touches", () => {
		const list = {
			length: 3,
			0: { clientX: 0, clientY: 0 },
			1: { clientX: 30, clientY: 0 },
			2: { clientX: 0, clientY: 60 },
		} as unknown as TouchList;
		expect(touchListCenter(list)).toEqual({ x: 10, y: 20 });
	});

	test("empty list returns origin", () => {
		const list = { length: 0 } as unknown as TouchList;
		expect(touchListCenter(list)).toEqual({ x: 0, y: 0 });
	});
});

describe("dragTypesToMimeTypes", () => {
	test("preserves text/plain, text/html, text/uri-list", () => {
		expect(dragTypesToMimeTypes(["text/plain", "text/html"])).toEqual([
			"text/plain",
			"text/html",
		]);
	});

	test("rewrites Files → application/octet-stream", () => {
		expect(dragTypesToMimeTypes(["Files"])).toEqual([
			"application/octet-stream",
		]);
	});

	test("ignores unknown types", () => {
		expect(dragTypesToMimeTypes(["unknown/foo"])).toEqual([]);
	});
});

/* ------------------------------------------------------------------
 *  attach* DOM wiring (jsdom)
 * ------------------------------------------------------------------ */

describe("attachWheelInput", () => {
	test("emits MotionNotify + press/release on a 20px vertical scroll", () => {
		cleanup = attachWheelInput(canvas, (ev) => captured.push(ev));
		canvas.dispatchEvent(
			new WheelEvent("wheel", {
				bubbles: true,
				cancelable: true,
				deltaY: 20,
				clientX: 50,
				clientY: 60,
			}),
		);
		expect(captured.map((e) => e.kind)).toEqual([
			"MotionNotify",
			"ButtonPress",
			"ButtonRelease",
		]);
	});

	test("cleanup removes the wheel listener", () => {
		cleanup = attachWheelInput(canvas, (ev) => captured.push(ev));
		cleanup();
		cleanup = () => {};
		canvas.dispatchEvent(new WheelEvent("wheel", { deltaY: 20 }));
		expect(captured).toEqual([]);
	});
});

describe("attachTouchInput", () => {
	test("forwards each changed touch as a TouchBegin event", () => {
		cleanup = attachTouchInput(canvas, (ev) => captured.push(ev));
		// jsdom's `new TouchEvent` is unreliable across versions —
		// hand-build an Event with a `touches` / `changedTouches`
		// payload that the listener reads.
		const e = new Event("touchstart", { bubbles: true, cancelable: true });
		Object.assign(e, {
			touches: [
				{ identifier: 1, clientX: 100, clientY: 200 },
				{ identifier: 2, clientX: 300, clientY: 400 },
			],
			changedTouches: [
				{ identifier: 1, clientX: 100, clientY: 200 },
				{ identifier: 2, clientX: 300, clientY: 400 },
			],
		});
		canvas.dispatchEvent(e);
		expect(captured).toEqual([
			{ kind: "TouchBegin", touch_id: 1, x: 100, y: 200, state: 0 },
			{ kind: "TouchBegin", touch_id: 2, x: 300, y: 400, state: 0 },
		]);
	});
});

describe("attachPinchGesture", () => {
	function fireTouch(
		type: "touchstart" | "touchmove" | "touchend",
		touches: { identifier: number; clientX: number; clientY: number }[],
	) {
		const e = new Event(type, { bubbles: true, cancelable: true });
		Object.assign(e, { touches, changedTouches: touches });
		canvas.dispatchEvent(e);
	}

	test("Begin → Update → End across the gesture lifecycle", () => {
		cleanup = attachPinchGesture(canvas, (ev) => captured.push(ev));

		fireTouch("touchstart", [
			{ identifier: 1, clientX: 100, clientY: 100 },
			{ identifier: 2, clientX: 200, clientY: 100 },
		]);
		fireTouch("touchmove", [
			{ identifier: 1, clientX: 80, clientY: 100 },
			{ identifier: 2, clientX: 220, clientY: 100 },
		]);
		fireTouch("touchend", []);

		const phases = captured
			.filter((e) => e.kind === "GesturePinch")
			.map((e) =>
				e.kind === "GesturePinch" ? e.phase : null,
			);
		expect(phases).toEqual(["Begin", "Update", "End"]);

		// The middle Update should report scale > 1 (fingers moved
		// apart from 100px to 140px → 1.4×).
		const update = captured.find(
			(e) => e.kind === "GesturePinch" && e.phase === "Update",
		);
		expect(update).toMatchObject({ scale: 1.4 });
	});

	test("ignores single-finger touchstart", () => {
		cleanup = attachPinchGesture(canvas, (ev) => captured.push(ev));
		fireTouch("touchstart", [{ identifier: 1, clientX: 0, clientY: 0 }]);
		expect(captured).toEqual([]);
	});
});

describe("attachSwipeGesture", () => {
	function fireTouch(
		type: "touchstart" | "touchmove" | "touchend",
		touches: { identifier: number; clientX: number; clientY: number }[],
	) {
		const e = new Event(type, { bubbles: true, cancelable: true });
		Object.assign(e, { touches, changedTouches: touches });
		canvas.dispatchEvent(e);
	}

	test("activates only at 3+ fingers and reports the count", () => {
		cleanup = attachSwipeGesture(canvas, (ev) => captured.push(ev));

		// 2 fingers: should NOT trigger swipe (pinch's territory).
		fireTouch("touchstart", [
			{ identifier: 1, clientX: 0, clientY: 0 },
			{ identifier: 2, clientX: 0, clientY: 0 },
		]);
		expect(captured).toEqual([]);

		// 3 fingers: Begin.
		fireTouch("touchstart", [
			{ identifier: 1, clientX: 0, clientY: 0 },
			{ identifier: 2, clientX: 0, clientY: 0 },
			{ identifier: 3, clientX: 0, clientY: 0 },
		]);
		expect(captured.at(-1)).toMatchObject({
			kind: "GestureSwipe",
			phase: "Begin",
			fingers: 3,
		});

		// Drop to 2 fingers: End.
		fireTouch("touchend", [
			{ identifier: 1, clientX: 0, clientY: 0 },
			{ identifier: 2, clientX: 0, clientY: 0 },
		]);
		expect(captured.at(-1)).toMatchObject({
			kind: "GestureSwipe",
			phase: "End",
			fingers: 3,
		});
	});
});

describe("attachDndBridge", () => {
	function fireDrag(
		type: "dragenter" | "dragover" | "dragleave",
		dataTransfer: Partial<DataTransfer>,
		clientX = 0,
		clientY = 0,
	) {
		const e = new Event(type, { bubbles: true, cancelable: true });
		Object.assign(e, { dataTransfer, clientX, clientY });
		canvas.dispatchEvent(e);
	}

	test("Enter announces translated mime types", () => {
		cleanup = attachDndBridge(canvas, (ev) => captured.push(ev));
		fireDrag("dragenter", {
			types: ["text/plain", "Files"] as unknown as readonly string[],
		});
		expect(captured).toEqual([
			{
				kind: "DndBridge",
				event: {
					kind: "Enter",
					mime_types: ["text/plain", "application/octet-stream"],
				},
			},
		]);
	});

	test("Leave fires", () => {
		cleanup = attachDndBridge(canvas, (ev) => captured.push(ev));
		fireDrag("dragleave", {});
		expect(captured).toEqual([
			{ kind: "DndBridge", event: { kind: "Leave" } },
		]);
	});
});
