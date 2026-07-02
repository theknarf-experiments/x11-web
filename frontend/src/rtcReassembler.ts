// Reassembler for the chunked WebRTC media DataChannel.
//
// The backend splits each Cap'n Proto Frame into N chunks, prefixed
// with an 8-byte header:
//
//   [u32 LE msg_id][u16 LE chunk_idx][u16 LE total_chunks][...payload...]
//
// On an unordered+unreliable channel chunks may arrive out of order
// or be lost. We track the highest msg_id we've seen, drop any
// chunks for older messages, and evict partial reassemblies older
// than the newest seen msg_id — so the receiver only ever surfaces
// the freshest fully-arrived frame, never a stale one. A frame with
// any lost chunks just never assembles; the next frame supersedes.
//
// `msg_id` wraps every ~4 billion frames; comparisons use serial-
// number arithmetic (RFC 1982 style) so wraparound is benign.

const HEADER_BYTES = 8;

interface PartialMessage {
	chunks: Array<Uint8Array | undefined>;
	received: number;
	total: number;
}

export class Reassembler {
	private latestSeen: number | null = null;
	private partials = new Map<number, PartialMessage>();

	/** Feed a chunk; returns the assembled payload when complete, or
	 *  null while the message is still being collected (or has been
	 *  superseded by a newer one). */
	onChunk(buf: Uint8Array): Uint8Array | null {
		if (buf.byteLength < HEADER_BYTES) return null;
		const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
		const msgId = dv.getUint32(0, true);
		const chunkIdx = dv.getUint16(4, true);
		const totalChunks = dv.getUint16(6, true);
		if (totalChunks === 0 || chunkIdx >= totalChunks) return null;
		const payload = buf.subarray(HEADER_BYTES);

		if (this.latestSeen !== null && isStrictlyOlder(msgId, this.latestSeen)) {
			// Stale chunk for a message we've already moved past.
			return null;
		}
		if (this.latestSeen === null || isStrictlyNewer(msgId, this.latestSeen)) {
			this.latestSeen = msgId;
			for (const id of [...this.partials.keys()]) {
				if (isStrictlyOlder(id, msgId)) this.partials.delete(id);
			}
		}

		let state = this.partials.get(msgId);
		if (!state) {
			state = {
				chunks: new Array(totalChunks),
				received: 0,
				total: totalChunks,
			};
			this.partials.set(msgId, state);
		} else if (state.total !== totalChunks) {
			// Sender disagrees with itself — drop the whole message.
			this.partials.delete(msgId);
			return null;
		}
		if (state.chunks[chunkIdx]) return null; // duplicate
		state.chunks[chunkIdx] = payload;
		state.received += 1;

		if (state.received < state.total) return null;

		const totalLen = state.chunks.reduce((s, c) => s + (c?.byteLength ?? 0), 0);
		const out = new Uint8Array(totalLen);
		let offset = 0;
		for (const c of state.chunks) {
			if (!c) return null; // shouldn't happen given received === total
			out.set(c, offset);
			offset += c.byteLength;
		}
		this.partials.delete(msgId);
		return out;
	}
}

/** RFC 1982-style serial number comparison for u32. `a` is strictly
 *  older than `b` if the wrapping unsigned distance from `a` to `b`
 *  is between 1 and 2^31-1. */
function isStrictlyOlder(a: number, b: number): boolean {
	if (a === b) return false;
	const diff = (b - a) >>> 0;
	return diff < 0x80000000;
}
function isStrictlyNewer(a: number, b: number): boolean {
	return isStrictlyOlder(b, a);
}
