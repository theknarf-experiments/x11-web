// Cap'n Proto wire format codec for the WebRTC DataChannel,
// generated from `crates/rtc-wire/schema/wire.capnp` via
// `pnpm codegen` (which runs `capnp-es`). This module wraps the
// generated readers / writers in a discriminated TS union so
// callers don't need to know about capnp internals.

import { Message } from "capnp-es";
import { Frame } from "./generated/wire";

export interface PutImageMessage {
	kind: "putImage";
	windowId: string;
	x: number;
	y: number;
	width: number;
	height: number;
	data: Uint8Array;
}

export interface ThumbnailMessage {
	kind: "thumbnail";
	windowId: string;
	width: number;
	height: number;
	data: Uint8Array;
}

export interface WorkspaceSyncMessage {
	kind: "workspaceSync";
	workspaceId: string;
	message: Uint8Array;
}

export type FrameMessage =
	| PutImageMessage
	| ThumbnailMessage
	| WorkspaceSyncMessage;

/** Decode one DataChannel `Frame`. Returns `null` if the buffer
 *  isn't a known variant or the layout is malformed. */
export function decodeFrame(buf: Uint8Array): FrameMessage | null {
	try {
		// `Message` accepts `ArrayBuffer`, not views — copy into a
		// fresh ArrayBuffer so callers can pass a `Uint8Array` view
		// over any backing store (incl. `SharedArrayBuffer`).
		const copy = new Uint8Array(buf.byteLength);
		copy.set(buf);
		const msg = new Message(copy.buffer, /* packed */ false);
		const frame = msg.getRoot(Frame);

		if (frame._isPutImage) {
			const p = frame.putImage;
			return {
				kind: "putImage",
				windowId: p.windowId,
				x: p.x,
				y: p.y,
				width: p.width,
				height: p.height,
				data: p.data.toUint8Array().slice(),
			};
		}
		if (frame._isWindowThumbnail) {
			const t = frame.windowThumbnail;
			return {
				kind: "thumbnail",
				windowId: t.windowId,
				width: t.width,
				height: t.height,
				data: t.data.toUint8Array().slice(),
			};
		}
		if (frame._isWorkspaceSync) {
			const w = frame.workspaceSync;
			return {
				kind: "workspaceSync",
				workspaceId: w.workspaceId,
				message: w.message.toUint8Array().slice(),
			};
		}
		return null;
	} catch {
		return null;
	}
}

/** Encode a `Frame::WorkspaceSync` for outbound transmission on
 *  the dedicated control DataChannel. The backend reads the same
 *  schema from the Rust side, so the wire layout is symmetric. */
export function encodeWorkspaceSync(
	workspaceId: string,
	message: Uint8Array,
): Uint8Array {
	const msg = new Message();
	const frame = msg.initRoot(Frame);
	const w = frame._initWorkspaceSync();
	w.workspaceId = workspaceId;
	const data = w._initMessage(message.byteLength);
	data.copyBuffer(message);
	return new Uint8Array(msg.toArrayBuffer());
}
