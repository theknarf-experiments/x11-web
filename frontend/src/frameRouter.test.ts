// @vitest-environment jsdom
import { Message } from "capnp-es";
import { describe, expect, test, vi } from "vitest";
import { act, createElement, useImperativeHandle, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "./generated/wire";
import { useFrameRouter } from "./frameRouter";

// React 19's `act(...)` shouts at us without this opt-in.
// biome-ignore lint/suspicious/noExplicitAny: test-only flag
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom doesn't ship `OffscreenCanvas`; `ClientRenderer` only
// touches it for the back-buffer + a 2D context, so a minimal
// stub is enough to let the routing test exercise the
// renderers-map allocation path.
if (typeof globalThis.OffscreenCanvas === "undefined") {
	class OffscreenCanvasStub {
		width: number;
		height: number;
		constructor(w: number, h: number) {
			this.width = w;
			this.height = h;
		}
		getContext() {
			return {
				putImageData: () => {},
				drawImage: () => {},
				clearRect: () => {},
			};
		}
	}
	// biome-ignore lint/suspicious/noExplicitAny: test stub
	(globalThis as any).OffscreenCanvas = OffscreenCanvasStub;
}

interface RouterHandle {
	getRenderers: () => Map<string, unknown>;
	getThumbnails: () => Map<string, string>;
}

function FrameRouterHost({
	onDataChannelMessage,
	ref,
}: {
	onDataChannelMessage: (cb: ((b: Uint8Array) => void) | null) => void;
	ref: React.Ref<RouterHandle>;
}) {
	const result = useFrameRouter({ onDataChannelMessage });
	const renderersRefCurrent = useRef(result.renderersRef);
	renderersRefCurrent.current = result.renderersRef;
	useImperativeHandle(ref, () => ({
		getRenderers: () => renderersRefCurrent.current.current,
		getThumbnails: () => result.thumbnails,
	}));
	return null;
}

function mountRouter() {
	let registered: ((b: Uint8Array) => void) | null = null;
	const onDataChannelMessage = vi.fn(
		(cb: ((b: Uint8Array) => void) | null) => {
			registered = cb;
		},
	);
	const handleRef: { current: RouterHandle | null } = { current: null };
	const container = document.createElement("div");
	document.body.appendChild(container);
	let root: Root;
	act(() => {
		root = createRoot(container);
		root.render(
			createElement(FrameRouterHost, {
				onDataChannelMessage,
				ref: handleRef,
			}),
		);
	});
	const cleanup = () => {
		act(() => root.unmount());
		document.body.removeChild(container);
	};
	if (!registered) throw new Error("router never registered a callback");
	return {
		dispatch: (bytes: Uint8Array) => {
			act(() => registered!(bytes));
		},
		handle: () => {
			if (!handleRef.current) throw new Error("handle not set");
			return handleRef.current;
		},
		onDataChannelMessage,
		cleanup,
	};
}

/** Build a serialised `Frame` for a given variant. */
function encodeFrame(setup: (f: Frame) => void): Uint8Array {
	const msg = new Message();
	const root = msg.initRoot(Frame);
	setup(root);
	return new Uint8Array(msg.toArrayBuffer());
}

describe("useFrameRouter", () => {
	test("registers a DC message listener on mount and clears it on unmount", () => {
		const { onDataChannelMessage, cleanup } = mountRouter();
		// Mount registered a non-null callback.
		expect(onDataChannelMessage).toHaveBeenCalledWith(expect.any(Function));
		cleanup();
		// Unmount cleared with `null`.
		expect(onDataChannelMessage).toHaveBeenLastCalledWith(null);
	});

	test("PutImage creates a `ClientRenderer` keyed by windowId", () => {
		const { dispatch, handle, cleanup } = mountRouter();
		const buf = encodeFrame((f) => {
			const p = f._initPutImage();
			p.windowId = "win-1";
			p.x = 0;
			p.y = 0;
			p.width = 100;
			p.height = 80;
			const data = p._initData(0);
			void data; // empty payload is fine — exercises map allocation only
		});
		dispatch(buf);
		expect(handle().getRenderers().has("win-1")).toBe(true);
		cleanup();
	});

	test("WindowThumbnail produces a blob-URL keyed by windowId", () => {
		// jsdom doesn't implement `URL.createObjectURL` — stub it.
		const created: string[] = [];
		const realCreate = URL.createObjectURL;
		const realRevoke = URL.revokeObjectURL;
		URL.createObjectURL = vi.fn(() => {
			const url = `blob:fake-${created.length}`;
			created.push(url);
			return url;
		}) as typeof URL.createObjectURL;
		URL.revokeObjectURL = vi.fn();

		const { dispatch, handle, cleanup } = mountRouter();
		const buf = encodeFrame((f) => {
			const t = f._initWindowThumbnail();
			t.windowId = "win-7";
			t.width = 256;
			t.height = 192;
			t._initData(4).copyBuffer(new Uint8Array([1, 2, 3, 4]));
		});
		dispatch(buf);
		expect(handle().getThumbnails().get("win-7")).toBe("blob:fake-0");
		cleanup();

		URL.createObjectURL = realCreate;
		URL.revokeObjectURL = realRevoke;
	});

	test("a second WindowThumbnail revokes the previous URL", () => {
		const created: string[] = [];
		const realCreate = URL.createObjectURL;
		const realRevoke = URL.revokeObjectURL;
		URL.createObjectURL = vi.fn(() => {
			const url = `blob:fake-${created.length}`;
			created.push(url);
			return url;
		}) as typeof URL.createObjectURL;
		const revoke = vi.fn();
		URL.revokeObjectURL = revoke;

		const { dispatch, cleanup } = mountRouter();
		const make = (data: number[]) =>
			encodeFrame((f) => {
				const t = f._initWindowThumbnail();
				t.windowId = "win-7";
				t.width = 1;
				t.height = 1;
				t._initData(data.length).copyBuffer(new Uint8Array(data));
			});
		dispatch(make([1]));
		dispatch(make([2]));
		expect(revoke).toHaveBeenCalledWith("blob:fake-0");
		cleanup();

		URL.createObjectURL = realCreate;
		URL.revokeObjectURL = realRevoke;
	});

	test("undecodable bytes are silently dropped", () => {
		const { dispatch, handle, cleanup } = mountRouter();
		dispatch(new Uint8Array([1, 2, 3]));
		expect(handle().getRenderers().size).toBe(0);
		expect(handle().getThumbnails().size).toBe(0);
		cleanup();
	});
});
