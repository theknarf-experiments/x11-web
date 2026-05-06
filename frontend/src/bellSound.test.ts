// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useBellSound } from "./bellSound";

// biome-ignore lint/suspicious/noExplicitAny: test-only flag
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

function BellHost({
	onBell,
}: {
	onBell: (cb: ((p: number) => void) | null) => void;
}) {
	useBellSound({ onBell });
	return null;
}

let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
	container = document.createElement("div");
	document.body.appendChild(container);
});
afterEach(() => {
	act(() => root.unmount());
	document.body.removeChild(container);
	document.body.style.backgroundColor = "";
	vi.useRealTimers();
});

function mount(
	onBell: (cb: ((p: number) => void) | null) => void,
): void {
	act(() => {
		root = createRoot(container);
		root.render(createElement(BellHost, { onBell }));
	});
}

describe("useBellSound", () => {
	test("registers a Bell listener on mount and clears on unmount", () => {
		const onBell = vi.fn();
		mount(onBell);
		expect(onBell).toHaveBeenCalledWith(expect.any(Function));
		act(() => root.unmount());
		expect(onBell).toHaveBeenLastCalledWith(null);
	});

	test("flashes the document body when AudioContext is unavailable", () => {
		// jsdom doesn't ship `AudioContext`, which is exactly the
		// fallback path we want to exercise.
		expect(globalThis.AudioContext).toBeUndefined();

		vi.useFakeTimers();
		let registered: ((p: number) => void) | null = null;
		mount((cb) => {
			registered = cb;
		});
		expect(document.body.style.backgroundColor).toBe("");
		registered!(80);
		expect(document.body.style.backgroundColor).toBe("rgb(255, 255, 255)");
		// Flash auto-clears after 100 ms.
		vi.advanceTimersByTime(100);
		expect(document.body.style.backgroundColor).toBe("");
	});

	test("plays a tone when AudioContext is available", () => {
		const start = vi.fn();
		const stop = vi.fn();
		const connect = vi.fn();
		// Hand-rolled stub — we only need the methods the hook calls.
		const fakeOsc = { connect, start, stop, frequency: { value: 0 } };
		const fakeGain = { connect, gain: { value: 0 } };
		const fakeCtx = {
			createOscillator: () => fakeOsc,
			createGain: () => fakeGain,
			destination: {},
			currentTime: 0,
		};
		// biome-ignore lint/suspicious/noExplicitAny: stubbing browser API
		(globalThis as any).AudioContext = function AudioContextStub() {
			return fakeCtx;
		};

		let registered: ((p: number) => void) | null = null;
		mount((cb) => {
			registered = cb;
		});
		registered!(50);
		expect(start).toHaveBeenCalled();
		expect(stop).toHaveBeenCalledWith(0.1);
		// 50% volume floors above the 0.01 minimum.
		expect(fakeGain.gain.value).toBe(0.5);

		// biome-ignore lint/suspicious/noExplicitAny: stubbing browser API
		delete (globalThis as any).AudioContext;
	});
});
