//! Type-safe X11 event construction using x11rb protocol structs.
//!
//! Wraps x11rb's typed event structs with byte-order-aware serialization.
//! Instead of manual byte offset construction:
//! ```ignore
//! let mut ev = [0u8; 32];
//! ev[0] = 22; // ConfigureNotify
//! write_u32_bo(&mut ev, 4, window, bo);
//! ```
//! Use:
//! ```ignore
//! use x11rb_protocol::protocol::xproto::ConfigureNotifyEvent;
//! serialize_event(&ConfigureNotifyEvent {
//!     response_type: CONFIGURE_NOTIFY,
//!     sequence: 0,
//!     event: window,
//!     window,
//!     ..Default::default()
//! }, msb_first)
//! ```

use x11rb_protocol::x11_utils::Serialize;

/// Serialize an x11rb event struct to 32 wire bytes, respecting client byte order.
///
/// x11rb's `Serialize` produces native-endian bytes (LE on our platform) and
/// may produce fewer than 32 bytes (only the defined fields). We pad to 32.
/// For MSB-first clients, we byte-swap all multi-byte fields.
pub(crate) fn serialize_event<E: Serialize>(event: &E, msb_first: bool) -> Vec<u8> {
    let mut bytes = vec![0u8; 32];
    let mut data = Vec::new();
    event.serialize_into(&mut data);
    let len = data.len().min(32);
    bytes[..len].copy_from_slice(&data[..len]);
    if msb_first {
        byteswap_event_inplace(&mut bytes);
    }
    bytes
}

/// Serialize an event using a wire-field layout for byteswapping.
///
/// Use this for events where the default `serialize_event` byteswap (which
/// assumes u32 alignment from byte 4) would corrupt mixed-width fields like
/// CARD16/INT16 or raw byte payloads (XKB events, ClientMessage format=8/16,
/// XGE events). Each `(offset, size)` pair describes a multi-byte wire field;
/// for 64-bit values, pass two consecutive 4-byte entries (low first).
///
/// The output buffer is padded to at least 32 bytes for core X11 events.
pub(crate) fn serialize_event_with_layout<E: Serialize>(
    event: &E,
    msb_first: bool,
    field_layout: &[(usize, usize)],
) -> Vec<u8> {
    let mut buf = Vec::new();
    event.serialize_into(&mut buf);
    if buf.len() < 32 {
        buf.resize(32, 0);
    }
    if msb_first {
        for &(off, sz) in field_layout {
            buf[off..off + sz].reverse();
        }
    }
    buf
}

/// Byte-swap a 32-byte X11 event for MSB-first clients.
///
/// X11 events have a known field layout: the first byte is the event type,
/// byte 1 is event-specific, bytes 2-3 are sequence (u16), and bytes 4-31
/// contain u32/u16/i16 fields depending on the event type.
///
/// For simplicity, we swap all u16-aligned pairs and u32-aligned quads
/// after the first 2 bytes (type + detail are single bytes).
fn byteswap_event_inplace(bytes: &mut [u8]) {
    if bytes.len() < 4 { return; }
    // Sequence number at [2..4] — swap as u16
    bytes.swap(2, 3);
    // Remaining fields [4..32] are all u32-aligned in standard X11 events.
    // Swap each 4-byte word.
    let end = bytes.len().min(32);
    for i in (4..end).step_by(4) {
        if i + 3 < bytes.len() {
            bytes.swap(i, i + 3);
            bytes.swap(i + 1, i + 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb_protocol::protocol::xproto::{ConfigureNotifyEvent, UnmapNotifyEvent};

    #[test]
    fn event_padded_to_32_bytes() {
        // UnmapNotifyEvent serializes to 16 bytes but wire events are 32
        let ev = UnmapNotifyEvent {
            response_type: 18,
            sequence: 1,
            event: 0x65,
            window: 0x65,
            from_configure: false,
        };
        let bytes = serialize_event(&ev, false);
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 18);
    }

    #[test]
    fn serialize_le_event() {
        let ev = ConfigureNotifyEvent {
            response_type: 22,
            sequence: 0x1234,
            event: 0x65,
            window: 0x65,
            above_sibling: 0,
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            border_width: 0,
            override_redirect: false,
        };
        let bytes = serialize_event(&ev, false);
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 22);
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0x1234);
        assert_eq!(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 0x65);
    }

    #[test]
    fn serialize_be_event_swaps_fields() {
        let ev = ConfigureNotifyEvent {
            response_type: 22,
            sequence: 0x1234,
            event: 0x65,
            window: 0x65,
            above_sibling: 0,
            x: 0,
            y: 0,
            width: 640,
            height: 480,
            border_width: 0,
            override_redirect: false,
        };
        let bytes = serialize_event(&ev, true);
        assert_eq!(bytes[0], 22); // type byte unchanged
        assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), 0x1234);
        assert_eq!(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 0x65);
    }
}
