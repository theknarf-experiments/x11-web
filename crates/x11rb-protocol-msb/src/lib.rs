//! Endian-aware fork of `x11rb-protocol`.
//!
//! For now this is a thin re-export of the upstream crate; the workspace
//! `[patch.crates-io]` redirects to our regenerated fork at
//! `tools/x11rb-fork/x11rb-protocol/` (see `tools/setup-x11rb-fork.sh`),
//! so this re-export already picks up any generator-side patches.
//!
//! Endian-aware traits and serialize helpers will land here as a layer
//! on top of `x11rb_protocol::*` in subsequent commits.

pub use x11rb_protocol::*;

#[cfg(test)]
mod endian_primitive_tests {
    use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian, TryParseEndian};

    #[test]
    fn u16_round_trip_both_orders() {
        let n: u16 = 0x1234;
        for order in [ByteOrder::Lsb, ByteOrder::Msb] {
            let bytes = n.serialize_endian(order);
            let (parsed, rest) = u16::try_parse_endian(&bytes, order).unwrap();
            assert_eq!(parsed, n, "round-trip {order:?}");
            assert!(rest.is_empty(), "no trailing bytes");
        }
    }

    #[test]
    fn u16_wire_layout_differs() {
        let n: u16 = 0x1234;
        assert_eq!(n.serialize_endian(ByteOrder::Lsb), vec![0x34, 0x12]);
        assert_eq!(n.serialize_endian(ByteOrder::Msb), vec![0x12, 0x34]);
    }

    #[test]
    fn u32_wire_layout_differs() {
        let n: u32 = 0xDEAD_BEEF;
        assert_eq!(
            n.serialize_endian(ByteOrder::Lsb),
            vec![0xEF, 0xBE, 0xAD, 0xDE],
        );
        assert_eq!(
            n.serialize_endian(ByteOrder::Msb),
            vec![0xDE, 0xAD, 0xBE, 0xEF],
        );
    }

    #[test]
    fn cross_parse_misinterprets() {
        // The whole point: parsing MSB bytes as LSB (or vice-versa) gives
        // the byte-swapped value. This is what protected the upstream
        // wire path from server-side use until now.
        let n: u32 = 0xDEAD_BEEF;
        let msb = n.serialize_endian(ByteOrder::Msb);
        let (mis, _) = u32::try_parse_endian(&msb, ByteOrder::Lsb).unwrap();
        assert_eq!(mis, n.swap_bytes());
    }

    #[test]
    fn float_round_trip() {
        let n: f32 = -1.5;
        for order in [ByteOrder::Lsb, ByteOrder::Msb] {
            let bytes = n.serialize_endian(order);
            let (parsed, _) = f32::try_parse_endian(&bytes, order).unwrap();
            assert_eq!(parsed, n, "round-trip f32 {order:?}");
        }
    }
}
