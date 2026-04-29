//! XKB Geometry stub.
//!
//! XKB geometry was deprecated upstream a long time ago — libxkbcommon dropped
//! its geometry parser in 2013, and the only client that ever depended on it
//! (`xkbprint`) is unmaintained. Modern toolkits, X server builds and tools
//! like `xkbcomp` / `xmodmap` / `xset` either ignore the geometry section or
//! tolerate `foundGeometry = FALSE` on the wire.
//!
//! We previously shipped a 1000-line hand-rolled PC-105 description here.
//! That code drifted from upstream xkb-data over time, no test ever caught
//! the divergence, and the data was never actually exercised by the e2e
//! suite. The stub below returns a spec-compliant "no geometry available"
//! reply, which is the same answer libxkbcommon-based servers give today.
//!
//! - `XkbGetGeometry` (opcode 19): returns `foundGeometry = FALSE`, all
//!   counts zero, body empty.
//! - `XkbSetGeometry` (opcode 20): void; we accept the request and discard.

use crate::xserver::client::ClientState;
use crate::xserver::reply::ReplyBuf;
use tracing::debug;

/// Build a minimal XKB GetGeometry reply that reports "no geometry data".
///
/// Takes `&mut ClientState` for signature parity with the other XKB
/// handlers; the parameter is unused.
pub(crate) fn build_xkb_get_geometry_reply(
    _state: &mut ClientState,
    seq: u16,
    device_id: u8,
) -> Vec<u8> {
    build_no_geometry_reply(seq, device_id)
}

fn build_no_geometry_reply(seq: u16, device_id: u8) -> Vec<u8> {
    // The reply is exactly 32 bytes: a standard X11 reply header (8 bytes)
    // plus the GetGeometry-specific header. With foundGeometry = 0 the body
    // is empty, so length = 0.
    let reply = ReplyBuf::fixed(seq, false /* msb_first */).set_data_byte(device_id);
    let mut bytes = reply.build();
    // foundGeometry byte at offset 12. Everything else is already zeroed by
    // the ReplyBuf constructor (name atom = 0, dimensions = 0, all counts =
    // 0, baseColorNdx = 0, labelColorNdx = 0).
    bytes[12] = 0;
    bytes
}

/// Handle XKB SetGeometry (void request).
pub(crate) fn handle_xkb_set_geometry(
    _state: &mut ClientState,
    data: &[u8],
    _seq: u16,
) -> Vec<u8> {
    debug!("XKB SetGeometry: ignored ({} bytes received)", data.len());
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_geometry_reply_reports_no_geometry() {
        let reply = build_no_geometry_reply(17, 3);
        assert_eq!(reply.len(), 32, "stub reply is exactly the 32-byte header");
        assert_eq!(reply[0], 1, "response_type = Reply");
        assert_eq!(reply[1], 3, "device_id echo");
        assert_eq!(
            u16::from_le_bytes([reply[2], reply[3]]),
            17,
            "sequence echoed"
        );
        let length = u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]);
        assert_eq!(length, 0, "no extra body words");
        assert_eq!(reply[12], 0, "foundGeometry = FALSE");
        // All count fields default to zero.
        for off in [18, 20, 22, 24, 26, 28] {
            assert_eq!(
                u16::from_le_bytes([reply[off], reply[off + 1]]),
                0,
                "count at offset {off} should be zero"
            );
        }
    }

    #[test]
    fn set_geometry_reply_size() {
        // Sanity: the no-geometry reply is exactly 32 bytes, the X11 reply
        // header. (We don't construct a full ClientState in unit tests; the
        // void SetGeometry handler is exercised end-to-end by the e2e suite.)
        assert_eq!(build_no_geometry_reply(0, 0).len(), 32);
    }
}
