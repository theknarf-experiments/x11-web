import { describe, expect, test } from "vitest";
import { Reassembler } from "./rtcReassembler";

/** Build one chunk-with-header from raw payload bytes. Header is
 *  `[u32 LE msg_id][u16 LE chunk_idx][u16 LE total_chunks]`. */
function chunk(
	msgId: number,
	chunkIdx: number,
	totalChunks: number,
	payload: number[],
): Uint8Array {
	const buf = new Uint8Array(8 + payload.length);
	const dv = new DataView(buf.buffer);
	dv.setUint32(0, msgId, true);
	dv.setUint16(4, chunkIdx, true);
	dv.setUint16(6, totalChunks, true);
	buf.set(payload, 8);
	return buf;
}

describe("Reassembler.onChunk", () => {
	test("single-chunk message assembles immediately", () => {
		const r = new Reassembler();
		const out = r.onChunk(chunk(1, 0, 1, [0xaa, 0xbb, 0xcc]));
		expect(out).toEqual(new Uint8Array([0xaa, 0xbb, 0xcc]));
	});

	test("multi-chunk in order assembles", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(7, 0, 3, [1, 2]))).toBeNull();
		expect(r.onChunk(chunk(7, 1, 3, [3, 4]))).toBeNull();
		expect(r.onChunk(chunk(7, 2, 3, [5, 6]))).toEqual(
			new Uint8Array([1, 2, 3, 4, 5, 6]),
		);
	});

	test("multi-chunk out of order still assembles in order", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(1, 2, 3, [5, 6]))).toBeNull();
		expect(r.onChunk(chunk(1, 0, 3, [1, 2]))).toBeNull();
		expect(r.onChunk(chunk(1, 1, 3, [3, 4]))).toEqual(
			new Uint8Array([1, 2, 3, 4, 5, 6]),
		);
	});

	test("duplicate chunk is dropped silently", () => {
		const r = new Reassembler();
		r.onChunk(chunk(2, 0, 2, [0xa]));
		expect(r.onChunk(chunk(2, 0, 2, [0xa]))).toBeNull();
		// Completing with the other chunk still works.
		expect(r.onChunk(chunk(2, 1, 2, [0xb]))).toEqual(
			new Uint8Array([0xa, 0xb]),
		);
	});

	test("missing chunk → never returns the message", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(3, 0, 3, [1]))).toBeNull();
		expect(r.onChunk(chunk(3, 2, 3, [3]))).toBeNull();
		// Index 1 never arrives.
	});

	test("stale chunk for an older msg_id is dropped", () => {
		const r = new Reassembler();
		// Bring `latestSeen` up to 100.
		r.onChunk(chunk(100, 0, 1, [0x77]));
		// A chunk for msg_id 50 should now be ignored.
		expect(r.onChunk(chunk(50, 0, 1, [0x88]))).toBeNull();
	});

	test("newer msg_id evicts the older partials", () => {
		const r = new Reassembler();
		// Open a partial at msg_id 5 — never completes.
		r.onChunk(chunk(5, 0, 3, [1]));
		// A newer message arrives complete — should assemble normally.
		expect(r.onChunk(chunk(10, 0, 1, [0x42]))).toEqual(new Uint8Array([0x42]));
		// Late chunk for msg 5 is now stale and dropped.
		expect(r.onChunk(chunk(5, 1, 3, [2]))).toBeNull();
		expect(r.onChunk(chunk(5, 2, 3, [3]))).toBeNull();
	});

	test("conflicting total_chunks drops the in-flight message", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(9, 0, 3, [1]))).toBeNull();
		// Sender claims 4 chunks now — internal contradiction; reset.
		expect(r.onChunk(chunk(9, 1, 4, [2]))).toBeNull();
		// Even providing the original 3-chunk completion shouldn't
		// resurrect the broken state.
		expect(r.onChunk(chunk(9, 2, 3, [3]))).toBeNull();
	});

	test("buffer shorter than the 8-byte header returns null", () => {
		const r = new Reassembler();
		expect(r.onChunk(new Uint8Array([1, 2, 3, 4, 5, 6, 7]))).toBeNull();
	});

	test("chunk_idx >= total_chunks returns null", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(1, 5, 3, [0x55]))).toBeNull();
	});

	test("total_chunks === 0 returns null", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(1, 0, 0, [0x55]))).toBeNull();
	});

	test("msg_id wraparound: 0 is newer than 0xFFFFFFFE", () => {
		const r = new Reassembler();
		// Seed `latestSeen` near the u32 ceiling.
		r.onChunk(chunk(0xfffffffe, 0, 1, [0x10]));
		// A wrap-around msg_id of 0 must be treated as newer.
		expect(r.onChunk(chunk(0, 0, 1, [0x20]))).toEqual(new Uint8Array([0x20]));
		// And a chunk for the now-stale older id should be dropped.
		expect(r.onChunk(chunk(0xfffffffd, 0, 1, [0x30]))).toBeNull();
	});

	test("zero-length payload chunks reassemble to empty", () => {
		const r = new Reassembler();
		expect(r.onChunk(chunk(1, 0, 1, []))).toEqual(new Uint8Array());
	});

	test("large payload preserves chunk order across many chunks", () => {
		const r = new Reassembler();
		const total = 16;
		// Feed in a randomised order; each chunk's payload is a
		// 4-byte LE encoding of its own index, so the assembled
		// output trivially reveals ordering bugs.
		const order = [3, 0, 12, 1, 7, 8, 15, 4, 5, 14, 2, 9, 11, 13, 6, 10];
		let out: Uint8Array | null = null;
		for (let i = 0; i < total; i++) {
			const idx = order[i];
			const payload = [idx & 0xff, (idx >> 8) & 0xff, 0, 0];
			out = r.onChunk(chunk(42, idx, total, payload));
		}
		expect(out).not.toBeNull();
		// Verify each 4-byte segment matches its position.
		for (let i = 0; i < total; i++) {
			const dv = new DataView(out!.buffer, out!.byteOffset + i * 4, 4);
			expect(dv.getUint32(0, true)).toBe(i);
		}
	});
});
