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

/// Hand-written endian-aware reply serializers. Once the generator
/// patch lands, the same body will be emitted automatically for every
/// generated reply type alongside the native-endian `Serialize` impl.
///
/// The shape demonstrates what the codegen output will look like and
/// guards against regressions in `x11_utils::SerializeEndian`.
pub mod reply_serialize {
    use x11rb_protocol::protocol::xproto::{
        GetGeometryReply, InternAtomReply, QueryPointerReply,
    };
    use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

    /// Serialize an X11 fixed-size reply header (32 bytes minimum) for
    /// a struct with no extra trailing data. Layout per spec §8 "Reply":
    ///   - byte  0: reply type (always 1)
    ///   - byte  1: optional "data byte" (depth, format flag, ...) — caller picks
    ///   - bytes 2..4:  CARD16 sequence number
    ///   - bytes 4..8:  CARD32 reply length (in 4-byte units beyond 32 bytes)
    fn write_header(
        bytes: &mut Vec<u8>,
        data_byte: u8,
        sequence: u16,
        extra_length_words: u32,
        endian: ByteOrder,
    ) {
        bytes.push(1);
        bytes.push(data_byte);
        sequence.serialize_endian_into(bytes, endian);
        extra_length_words.serialize_endian_into(bytes, endian);
    }

    /// Pad `bytes` to at least 32 bytes (the minimum reply size).
    fn pad_to_min_reply(bytes: &mut Vec<u8>) {
        if bytes.len() < 32 {
            bytes.resize(32, 0);
        }
    }

    pub fn serialize_intern_atom(
        reply: &InternAtomReply,
        endian: ByteOrder,
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        write_header(&mut bytes, 0, reply.sequence, 0, endian);
        reply.atom.serialize_endian_into(&mut bytes, endian);
        pad_to_min_reply(&mut bytes);
        bytes
    }

    pub fn serialize_get_geometry(
        reply: &GetGeometryReply,
        endian: ByteOrder,
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        write_header(&mut bytes, reply.depth, reply.sequence, 0, endian);
        reply.root.serialize_endian_into(&mut bytes, endian);
        reply.x.serialize_endian_into(&mut bytes, endian);
        reply.y.serialize_endian_into(&mut bytes, endian);
        reply.width.serialize_endian_into(&mut bytes, endian);
        reply.height.serialize_endian_into(&mut bytes, endian);
        reply.border_width.serialize_endian_into(&mut bytes, endian);
        pad_to_min_reply(&mut bytes);
        bytes
    }

    pub fn serialize_query_pointer(
        reply: &QueryPointerReply,
        endian: ByteOrder,
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        write_header(&mut bytes, reply.same_screen.into(), reply.sequence, 0, endian);
        reply.root.serialize_endian_into(&mut bytes, endian);
        reply.child.serialize_endian_into(&mut bytes, endian);
        reply.root_x.serialize_endian_into(&mut bytes, endian);
        reply.root_y.serialize_endian_into(&mut bytes, endian);
        reply.win_x.serialize_endian_into(&mut bytes, endian);
        reply.win_y.serialize_endian_into(&mut bytes, endian);
        // KeyButMask is a u16-backed bitmask newtype; round-trip via the
        // primitive impl. The generator will need a similar shim.
        u16::from(reply.mask).serialize_endian_into(&mut bytes, endian);
        pad_to_min_reply(&mut bytes);
        bytes
    }
}

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
mod reply_serialize_tests {
    use super::reply_serialize::*;
    use x11rb_protocol::protocol::xproto::{
        GetGeometryReply, InternAtomReply, QueryPointerReply,
    };
    use x11rb_protocol::x11_utils::ByteOrder;

    #[test]
    fn intern_atom_msb_layout() {
        let reply = InternAtomReply {
            sequence: 0x0102,
            length: 0,
            atom: 0xCAFEBABE,
        };
        let bytes = serialize_intern_atom(&reply, ByteOrder::Msb);
        assert_eq!(bytes.len(), 32);
        // reply type
        assert_eq!(bytes[0], 1);
        // unused data byte
        assert_eq!(bytes[1], 0);
        // sequence (MSB-first)
        assert_eq!(&bytes[2..4], &[0x01, 0x02]);
        // length = 0
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x00]);
        // atom (MSB-first)
        assert_eq!(&bytes[8..12], &[0xCA, 0xFE, 0xBA, 0xBE]);
        // padding
        assert!(bytes[12..].iter().all(|&b| b == 0));
    }

    #[test]
    fn intern_atom_lsb_layout() {
        let reply = InternAtomReply {
            sequence: 0x0102,
            length: 0,
            atom: 0xCAFEBABE,
        };
        let bytes = serialize_intern_atom(&reply, ByteOrder::Lsb);
        assert_eq!(&bytes[2..4], &[0x02, 0x01]);
        assert_eq!(&bytes[8..12], &[0xBE, 0xBA, 0xFE, 0xCA]);
    }

    #[test]
    fn get_geometry_msb_layout() {
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
        let bytes = serialize_get_geometry(&reply, ByteOrder::Msb);
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 1);
        // depth (data byte)
        assert_eq!(bytes[1], 24);
        // sequence
        assert_eq!(&bytes[2..4], &[0x00, 0x42]);
        // length
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x00]);
        // root
        assert_eq!(&bytes[8..12], &[0x01, 0x23, 0x45, 0x67]);
        // x = -10 as i16 MSB
        assert_eq!(&bytes[12..14], &(-10_i16).to_be_bytes());
        // y = 20
        assert_eq!(&bytes[14..16], &20_i16.to_be_bytes());
        // width = 800
        assert_eq!(&bytes[16..18], &800_u16.to_be_bytes());
        // height = 600
        assert_eq!(&bytes[18..20], &600_u16.to_be_bytes());
        // border_width = 2
        assert_eq!(&bytes[20..22], &2_u16.to_be_bytes());
    }

    #[test]
    fn query_pointer_msb_layout() {
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
        let bytes = serialize_query_pointer(&reply, ByteOrder::Msb);
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[1], 1, "same_screen = 1");
        assert_eq!(&bytes[8..12], &0x10_u32.to_be_bytes());
        assert_eq!(&bytes[12..16], &0x20_u32.to_be_bytes());
    }
}
