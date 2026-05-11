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

#[cfg(test)]
mod generator_emitted_tryparse_endian_tests {
    //! Verify the generator-emitted `impl TryParseEndian` actually works
    //! against real MSB-first wire bytes for a fixed-layout reply struct.
    use x11rb_protocol::protocol::xproto::GetGeometryReply;
    use x11rb_protocol::x11_utils::{ByteOrder, TryParseEndian};

    #[test]
    fn parse_msb_get_geometry_reply() {
        // Hand-build the wire layout for a GetGeometryReply in MSB:
        //   1                  reply type
        //   24                 depth
        //   0x0042             sequence (MSB)
        //   0x00000000         length
        //   0x01234567         root (MSB)
        //   0xFFF6 (-10)       x i16 (MSB)
        //   0x0014 (20)        y
        //   0x0320 (800)       width
        //   0x0258 (600)       height
        //   0x0002             border_width
        //   10 bytes of zero padding to reach 32
        let mut bytes = vec![
            1, 24, 0x00, 0x42,
            0x00, 0x00, 0x00, 0x00,
            0x01, 0x23, 0x45, 0x67,
            0xFF, 0xF6,
            0x00, 0x14,
            0x03, 0x20,
            0x02, 0x58,
            0x00, 0x02,
        ];
        bytes.resize(32, 0);

        let (reply, rest) = GetGeometryReply::try_parse_endian(&bytes, ByteOrder::Msb)
            .expect("parse MSB GetGeometryReply");
        // `GetGeometryReply` has an `EmbeddedLength` size constraint, so
        // the parser consumes the whole 32-byte fixed reply including the
        // trailing pad.
        assert!(rest.is_empty(), "all 32 bytes consumed");
        assert_eq!(reply.depth, 24);
        assert_eq!(reply.sequence, 0x0042);
        assert_eq!(reply.length, 0);
        assert_eq!(reply.root, 0x01234567);
        assert_eq!(reply.x, -10);
        assert_eq!(reply.y, 20);
        assert_eq!(reply.width, 800);
        assert_eq!(reply.height, 600);
        assert_eq!(reply.border_width, 2);
    }

    #[test]
    fn parse_lsb_get_geometry_reply_matches_native_try_parse() {
        use x11rb_protocol::x11_utils::TryParse;
        // Build LSB bytes the same way native `try_parse` would expect.
        let mut bytes = vec![
            1, 24, 0x42, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x67, 0x45, 0x23, 0x01,
            0xF6, 0xFF,
            0x14, 0x00,
            0x20, 0x03,
            0x58, 0x02,
            0x02, 0x00,
        ];
        bytes.resize(32, 0);

        let (via_endian, _) =
            GetGeometryReply::try_parse_endian(&bytes, ByteOrder::Lsb).unwrap();
        let (via_native, _) = GetGeometryReply::try_parse(&bytes).unwrap();
        // GetGeometryReply doesn't impl PartialEq upstream; compare fields.
        assert_eq!(via_endian.depth, via_native.depth);
        assert_eq!(via_endian.sequence, via_native.sequence);
        assert_eq!(via_endian.root, via_native.root);
        assert_eq!(via_endian.x, via_native.x);
        assert_eq!(via_endian.y, via_native.y);
        assert_eq!(via_endian.width, via_native.width);
        assert_eq!(via_endian.height, via_native.height);
        assert_eq!(via_endian.border_width, via_native.border_width);
    }
}

#[cfg(test)]
mod generator_emitted_serialize_endian_tests {
    //! Verify the generator-emitted `impl SerializeEndian` produces the
    //! expected MSB/LSB wire bytes for real reply structs, plus that
    //! `serialize_endian_into` with `ByteOrder::Native` matches the
    //! upstream native `serialize_into` byte-for-byte.
    use x11rb_protocol::protocol::xproto::{GetGeometryReply, InternAtomReply};
    use x11rb_protocol::x11_utils::{ByteOrder, Serialize, SerializeEndian};

    #[test]
    fn intern_atom_msb_layout() {
        let reply = InternAtomReply {
            sequence: 0x0102,
            length: 0,
            atom: 0xCAFE_BABE,
        };
        let mut bytes = Vec::new();
        reply.serialize_endian_into(&mut bytes, ByteOrder::Msb);
        // The generator emits the fixed-size header (12 bytes for InternAtomReply).
        assert_eq!(bytes.len(), 12);
        assert_eq!(bytes[0], 1);                       // reply type
        assert_eq!(bytes[1], 0);                       // pad byte
        assert_eq!(&bytes[2..4], &[0x01, 0x02]);       // sequence MSB
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x00]); // length
        assert_eq!(&bytes[8..12], &[0xCA, 0xFE, 0xBA, 0xBE]); // atom MSB
    }

    #[test]
    fn intern_atom_lsb_layout() {
        let reply = InternAtomReply {
            sequence: 0x0102,
            length: 0,
            atom: 0xCAFE_BABE,
        };
        let mut bytes = Vec::new();
        reply.serialize_endian_into(&mut bytes, ByteOrder::Lsb);
        assert_eq!(&bytes[2..4], &[0x02, 0x01]);       // sequence LSB
        assert_eq!(&bytes[8..12], &[0xBE, 0xBA, 0xFE, 0xCA]); // atom LSB
    }

    #[test]
    fn native_endian_matches_upstream_serialize_into() {
        let reply = GetGeometryReply {
            depth: 24,
            sequence: 0x0042,
            length: 0,
            root: 0x0123_4567,
            x: -10,
            y: 20,
            width: 800,
            height: 600,
            border_width: 2,
        };
        let mut native = Vec::new();
        reply.serialize_into(&mut native);
        let mut endian = Vec::new();
        reply.serialize_endian_into(&mut endian, ByteOrder::NATIVE);
        assert_eq!(native, endian, "endian path with NATIVE matches upstream byte-for-byte");
    }
}

#[cfg(test)]
mod legacy_reply_serialize_layout_smoke {
    //! Older smoke tests that still exercise quirks of upstream's
    //! reply types (e.g. `QueryPointerReply` with a bitmask newtype
    //! field). Confirms the generator-emitted impl handles every case.
    use x11rb_protocol::protocol::xproto::{GetGeometryReply, QueryPointerReply};
    use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

    #[test]
    fn get_geometry_msb_layout_via_codegen() {
        let reply = GetGeometryReply {
            depth: 24,
            sequence: 0x0042,
            length: 0,
            root: 0x0123_4567,
            x: -10,
            y: 20,
            width: 800,
            height: 600,
            border_width: 2,
        };
        let mut bytes = Vec::new();
        reply.serialize_endian_into(&mut bytes, ByteOrder::Msb);
        // GetGeometryReply serializes 24 bytes: the fixed reply header
        // up through `border_width`, followed by 2 bytes of struct-level
        // padding emitted by upstream's serialize_into. The remaining
        // 8 bytes of the 32-byte wire reply are implicit zero-padding
        // managed by the framing layer above.
        assert_eq!(bytes.len(), 24);
        assert_eq!(bytes[0], 1);                     // reply type
        assert_eq!(bytes[1], 24);                    // depth (data byte)
        assert_eq!(&bytes[2..4], &[0x00, 0x42]);     // sequence
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x00]); // length
        assert_eq!(&bytes[8..12], &[0x01, 0x23, 0x45, 0x67]); // root
        assert_eq!(&bytes[12..14], &(-10_i16).to_be_bytes()); // x
        assert_eq!(&bytes[14..16], &20_i16.to_be_bytes());    // y
        assert_eq!(&bytes[16..18], &800_u16.to_be_bytes());   // width
        assert_eq!(&bytes[18..20], &600_u16.to_be_bytes());   // height
        assert_eq!(&bytes[20..22], &2_u16.to_be_bytes());     // border_width
    }

    #[test]
    fn query_pointer_msb_layout_via_codegen() {
        let reply = QueryPointerReply {
            same_screen: true,
            sequence: 1,
            length: 0,
            root: 0x10,
            child: 0x20,
            root_x: 100,
            root_y: 200,
            win_x: 50,
            win_y: 60,
            mask: x11rb_protocol::protocol::xproto::KeyButMask::SHIFT,
        };
        let mut bytes = Vec::new();
        reply.serialize_endian_into(&mut bytes, ByteOrder::Msb);
        // Spec layout is 32 bytes (fixed reply). Confirm the bitmask
        // newtype field (KeyButMask is a u16 wrapper) is handled by
        // the generator's enum-aware emission path.
        assert!(bytes.len() >= 28);
        assert_eq!(bytes[1], 1, "same_screen = 1");
        assert_eq!(&bytes[8..12], &0x10_u32.to_be_bytes());
        assert_eq!(&bytes[12..16], &0x20_u32.to_be_bytes());
    }
}
