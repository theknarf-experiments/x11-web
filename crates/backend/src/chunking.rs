//! Chunking layer for the WebRTC media DataChannel.
//!
//! Each application-level message (a Cap'n Proto serialised `Frame`)
//! is split into N chunks of at most `CHUNK_PAYLOAD_BYTES` bytes
//! before being written as separate DataChannel messages. Every
//! chunk carries an 8-byte header so the receiver can reassemble:
//!
//! ```text
//! [u32 LE msg_id][u16 LE chunk_idx][u16 LE total_chunks][...payload...]
//! ```
//!
//! Why chunk at all: str0m's SCTP send buffer caps at 128 KB across
//! all streams, and individual writes that exceed `available()` are
//! rejected with `Ok(false)`. Capping per-write at well under that
//! buffer keeps writes flowing. The chunked frame format also gives
//! the receiver a cheap way to drop stale partial frames in
//! unordered+unreliable mode (just compare `msg_id`s).
//!
//! `msg_id` increments per logical message and wraps via
//! `wrapping_add`. Receiver compares with serial-number arithmetic
//! (RFC 1982 style) so wraparound after ~4 billion frames is benign.

const HEADER_BYTES: usize = 8;

/// Total wire size per chunk including the 8-byte header. Must be
/// ≤ SCTP's `max_send_message_size` (sctp-proto default 65536), and
/// must be small enough that several chunks fit in str0m's 128 KB
/// SCTP send buffer to keep throughput up. 16 KB lets ~8 fit in
/// flight at once, which gives the backpressure queue something to
/// push as `Event::ChannelBufferedAmountLow` fires. 32 KB / 64 KB
/// also work — switch by editing this single line. Reassembly is
/// chunk-size-agnostic.
pub const CHUNK_TOTAL_BYTES: usize = 16 * 1024;

/// Payload bytes per chunk, header excluded.
pub const CHUNK_PAYLOAD_BYTES: usize = CHUNK_TOTAL_BYTES - HEADER_BYTES;

pub struct Chunker {
    next_msg_id: u32,
}

impl Chunker {
    pub fn new() -> Self {
        Self { next_msg_id: 0 }
    }

    /// Split `payload` into chunks ready for DC writes. Each chunk is
    /// `≤ HEADER_BYTES + CHUNK_PAYLOAD_BYTES` bytes. Empty `payload`
    /// produces a single zero-length chunk (preserves message
    /// boundaries even for empty messages).
    pub fn chunk(&mut self, payload: &[u8]) -> Vec<Vec<u8>> {
        let msg_id = self.next_msg_id;
        self.next_msg_id = self.next_msg_id.wrapping_add(1);

        let total: u16 = if payload.is_empty() {
            1
        } else {
            payload
                .len()
                .div_ceil(CHUNK_PAYLOAD_BYTES)
                .min(u16::MAX as usize) as u16
        };

        let mut out = Vec::with_capacity(total as usize);
        if payload.is_empty() {
            out.push(make_chunk(msg_id, 0, total, &[]));
            return out;
        }
        for (idx, slice) in payload.chunks(CHUNK_PAYLOAD_BYTES).enumerate() {
            out.push(make_chunk(msg_id, idx as u16, total, slice));
        }
        out
    }
}

fn make_chunk(msg_id: u32, idx: u16, total: u16, slice: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_BYTES + slice.len());
    buf.extend_from_slice(&msg_id.to_le_bytes());
    buf.extend_from_slice(&idx.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    buf.extend_from_slice(slice);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_header(chunk: &[u8]) -> (u32, u16, u16) {
        let msg_id = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let idx = u16::from_le_bytes(chunk[4..6].try_into().unwrap());
        let total = u16::from_le_bytes(chunk[6..8].try_into().unwrap());
        (msg_id, idx, total)
    }

    #[test]
    fn single_chunk_fits_under_limit() {
        let mut c = Chunker::new();
        let payload = vec![42u8; CHUNK_PAYLOAD_BYTES];
        let chunks = c.chunk(&payload);
        assert_eq!(chunks.len(), 1);
        let (msg_id, idx, total) = read_header(&chunks[0]);
        assert_eq!(msg_id, 0);
        assert_eq!(idx, 0);
        assert_eq!(total, 1);
        assert_eq!(chunks[0].len(), HEADER_BYTES + payload.len());
    }

    #[test]
    fn split_three_chunks() {
        let mut c = Chunker::new();
        let payload = vec![1u8; CHUNK_PAYLOAD_BYTES * 2 + 100];
        let chunks = c.chunk(&payload);
        assert_eq!(chunks.len(), 3);
        for (i, chunk) in chunks.iter().enumerate() {
            let (_msg_id, idx, total) = read_header(chunk);
            assert_eq!(idx as usize, i);
            assert_eq!(total, 3);
        }
        // Last chunk is short, others full.
        assert_eq!(chunks[0].len(), HEADER_BYTES + CHUNK_PAYLOAD_BYTES);
        assert_eq!(chunks[1].len(), HEADER_BYTES + CHUNK_PAYLOAD_BYTES);
        assert_eq!(chunks[2].len(), HEADER_BYTES + 100);
    }

    #[test]
    fn msg_ids_monotonic_and_wrap() {
        let mut c = Chunker {
            next_msg_id: u32::MAX - 1,
        };
        let a = c.chunk(b"a");
        let b = c.chunk(b"b");
        let d = c.chunk(b"d");
        assert_eq!(read_header(&a[0]).0, u32::MAX - 1);
        assert_eq!(read_header(&b[0]).0, u32::MAX);
        assert_eq!(read_header(&d[0]).0, 0);
    }

    #[test]
    fn empty_payload_produces_one_chunk() {
        let mut c = Chunker::new();
        let chunks = c.chunk(&[]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), HEADER_BYTES);
    }
}
