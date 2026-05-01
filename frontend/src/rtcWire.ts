// Decoder for the Cap'n Proto `Frame` message type defined in
// `crates/rtc-wire/schema/wire.capnp`. The backend encodes the
// `Frame` union into a single DataChannel message; this module
// decodes the bytes back into a discriminated TS union.
//
// The schema is small enough that hand-rolling the decoder is
// cheaper than pulling in `capnp-ts` (~100KB gz). The backend forces
// single-segment messages so we don't need to handle far pointers.
//
// Cap'n Proto wire-format reference:
// https://capnproto.org/encoding.html

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

export type FrameMessage = PutImageMessage | ThumbnailMessage;

const FRAME_DISCRIMINANT_PUT_IMAGE = 1;
const FRAME_DISCRIMINANT_THUMBNAIL = 2;
const ELEMENT_SIZE_BYTE = 2;
const POINTER_TAG_STRUCT = 0;
const POINTER_TAG_LIST = 1;

/** Decode a Cap'n Proto-serialised `Frame`. Returns null if the
 *  message isn't a known variant or the layout doesn't match. */
export function decodeFrame(buf: Uint8Array): FrameMessage | null {
	const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);

	// Stream framing: u32 LE (segCount-1), u32 LE (seg-0 size in
	// 8-byte words). Single-segment header is exactly 8 bytes.
	if (buf.length < 8) return null;
	const segCount = dv.getUint32(0, true) + 1;
	if (segCount !== 1) return null;
	const segOffset = 8;

	// Root pointer at the start of the segment — points to Frame.
	const root = readStructPointer(dv, segOffset);
	if (!root) return null;
	if (root.dataWords < 1 || root.ptrWords < 1) return null;

	// Frame: data section is the union discriminant (16-bit at offset 0).
	const frameStart = segOffset + 8 + root.offsetWords * 8;
	const discriminant = dv.getUint16(frameStart, true);
	const framePtrSection = frameStart + root.dataWords * 8;

	if (discriminant === FRAME_DISCRIMINANT_PUT_IMAGE) {
		return decodePutImagePayload(buf, dv, framePtrSection);
	}
	if (discriminant === FRAME_DISCRIMINANT_THUMBNAIL) {
		return decodeThumbnailPayload(buf, dv, framePtrSection);
	}
	return null;
}

function decodePutImagePayload(
	buf: Uint8Array,
	dv: DataView,
	framePtrSection: number,
): PutImageMessage | null {
	// Frame's pointer 0 → PutImage.
	const ref = readStructPointer(dv, framePtrSection);
	if (!ref) return null;
	if (ref.dataWords < 1 || ref.ptrWords < 2) return null;
	const piStart = framePtrSection + 8 + ref.offsetWords * 8;

	// PutImage data: x (Int16), y (Int16), width (UInt16), height (UInt16)
	const x = dv.getInt16(piStart, true);
	const y = dv.getInt16(piStart + 2, true);
	const width = dv.getUint16(piStart + 4, true);
	const height = dv.getUint16(piStart + 6, true);

	// PutImage pointer 0 → windowId Text, pointer 1 → data.
	const ptrSection = piStart + ref.dataWords * 8;
	const wid = readBytePointer(buf, dv, ptrSection, /* dropTrailingNul */ true);
	const data = readBytePointer(
		buf,
		dv,
		ptrSection + 8,
		/* dropTrailingNul */ false,
	);
	if (!wid || !data) return null;
	const windowId = new TextDecoder().decode(wid);
	return { kind: "putImage", windowId, x, y, width, height, data };
}

function decodeThumbnailPayload(
	buf: Uint8Array,
	dv: DataView,
	framePtrSection: number,
): ThumbnailMessage | null {
	const ref = readStructPointer(dv, framePtrSection);
	if (!ref) return null;
	if (ref.dataWords < 1 || ref.ptrWords < 2) return null;
	const tStart = framePtrSection + 8 + ref.offsetWords * 8;

	// WindowThumbnail data: width (UInt16), height (UInt16) at the
	// start of the data section. (The first 4 bytes of the data
	// section are unused — Cap'n Proto pads structs to 8-byte words.)
	const width = dv.getUint16(tStart, true);
	const height = dv.getUint16(tStart + 2, true);

	// Pointer 0 → windowId Text, pointer 1 → data bytes.
	const ptrSection = tStart + ref.dataWords * 8;
	const wid = readBytePointer(buf, dv, ptrSection, /* dropTrailingNul */ true);
	const data = readBytePointer(
		buf,
		dv,
		ptrSection + 8,
		/* dropTrailingNul */ false,
	);
	if (!wid || !data) return null;
	const windowId = new TextDecoder().decode(wid);
	return { kind: "thumbnail", windowId, width, height, data };
}

interface StructRef {
	offsetWords: number;
	dataWords: number;
	ptrWords: number;
}

function readStructPointer(dv: DataView, offset: number): StructRef | null {
	if (offset + 8 > dv.byteLength) return null;
	const lo = dv.getUint32(offset, true);
	const hi = dv.getUint32(offset + 4, true);
	if ((lo & 3) !== POINTER_TAG_STRUCT) return null;
	// 30-bit signed offset (bits 2-31 of lo). JS's `>> 2` on the
	// uint32 sign-extends from bit 31, which is exactly the offset's
	// sign bit, so this Just Works.
	const offsetWords = lo >> 2;
	const dataWords = hi & 0xffff;
	const ptrWords = (hi >>> 16) & 0xffff;
	return { offsetWords, dataWords, ptrWords };
}

function readBytePointer(
	buf: Uint8Array,
	dv: DataView,
	offset: number,
	dropTrailingNul: boolean,
): Uint8Array | null {
	if (offset + 8 > dv.byteLength) return null;
	const lo = dv.getUint32(offset, true);
	const hi = dv.getUint32(offset + 4, true);
	if ((lo & 3) !== POINTER_TAG_LIST) return null;
	const offsetWords = lo >> 2;
	const elementSize = hi & 7;
	const elementCount = hi >>> 3;
	if (elementSize !== ELEMENT_SIZE_BYTE) return null;
	const start = offset + 8 + offsetWords * 8;
	const end = start + elementCount;
	if (end > buf.byteLength) return null;
	const slice = buf.subarray(start, end);
	return dropTrailingNul && slice.length > 0
		? slice.subarray(0, slice.length - 1)
		: slice;
}
