import { Message } from "capnp-es";
import { describe, expect, test } from "vitest";
import { Frame } from "./generated/wire";
import { decodeFrame, encodeWorkspaceSync } from "./rtcWire";

/** Build a serialised `Frame` with the given variant set up.
 *  Drives the decoder from the Rust-side perspective — the
 *  backend writes Frames against the same schema. */
function encodeFrame(setup: (f: Frame) => void): Uint8Array {
	const msg = new Message();
	const root = msg.initRoot(Frame);
	setup(root);
	return new Uint8Array(msg.toArrayBuffer());
}

describe("decodeFrame", () => {
	test("PutImage round-trips through the wire", () => {
		const payload = new Uint8Array([0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03]);
		const buf = encodeFrame((f) => {
			const p = f._initPutImage();
			p.windowId = "win-42";
			p.x = -3;
			p.y = 5;
			p.width = 800;
			p.height = 600;
			const data = p._initData(payload.length);
			data.copyBuffer(payload);
		});

		const decoded = decodeFrame(buf);
		expect(decoded).toMatchObject({
			kind: "putImage",
			windowId: "win-42",
			x: -3,
			y: 5,
			width: 800,
			height: 600,
		});
		expect(
			Array.from(decoded?.kind === "putImage" ? decoded.data : []),
		).toEqual(Array.from(payload));
	});

	test("WindowThumbnail round-trips", () => {
		const data = new Uint8Array([1, 2, 3, 4, 5]);
		const buf = encodeFrame((f) => {
			const t = f._initWindowThumbnail();
			t.windowId = "win-7";
			t.width = 256;
			t.height = 192;
			const d = t._initData(data.length);
			d.copyBuffer(data);
		});

		expect(decodeFrame(buf)).toMatchObject({
			kind: "thumbnail",
			windowId: "win-7",
			width: 256,
			height: 192,
		});
	});

	test("WorkspaceSync round-trips", () => {
		const message = new Uint8Array([0x85, 0x6f, 0x4a, 0x83]);
		const buf = encodeFrame((f) => {
			const w = f._initWorkspaceSync();
			w.workspaceId = "ws-main";
			const m = w._initMessage(message.length);
			m.copyBuffer(message);
		});
		expect(decodeFrame(buf)).toMatchObject({
			kind: "workspaceSync",
			workspaceId: "ws-main",
		});
	});

	test("noVariant returns null", () => {
		const buf = encodeFrame(() => {
			// Default variant — discriminant 0.
		});
		expect(decodeFrame(buf)).toBeNull();
	});

	test("malformed buffer returns null instead of throwing", () => {
		expect(decodeFrame(new Uint8Array([1, 2, 3]))).toBeNull();
	});
});

describe("encodeWorkspaceSync", () => {
	test("output decodes back to the same workspaceId + message", () => {
		const message = new Uint8Array([0x42, 0x13, 0x37, 0x99, 0xaa]);
		const buf = encodeWorkspaceSync("my-workspace", message);
		const decoded = decodeFrame(buf);
		expect(decoded?.kind).toBe("workspaceSync");
		if (decoded?.kind !== "workspaceSync") return;
		expect(decoded.workspaceId).toBe("my-workspace");
		expect(Array.from(decoded.message)).toEqual(Array.from(message));
	});

	test("handles empty message payload", () => {
		const buf = encodeWorkspaceSync("ws", new Uint8Array());
		const decoded = decodeFrame(buf);
		expect(decoded?.kind).toBe("workspaceSync");
		if (decoded?.kind !== "workspaceSync") return;
		expect(decoded.workspaceId).toBe("ws");
		expect(decoded.message.byteLength).toBe(0);
	});
});
