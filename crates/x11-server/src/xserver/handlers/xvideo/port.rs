//! XVideo port operations: GrabPort, UngrabPort, QueryBestSize,
//! SetPortAttribute, GetPortAttribute, QueryPortAttributes.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::parse_minor;
use super::{
    build_reply, XV_ATTR_BRIGHTNESS, XV_ATTR_COLORSPACE, XV_ATTR_CONTRAST, XV_ATTR_HUE,
    XV_ATTR_SATURATION, XV_MAJOR_OPCODE,
};
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xv::{
    AttributeFlag, GetPortAttributeReply, GetPortAttributeRequest, GrabPortReply, GrabPortRequest,
    GrabPortStatus, QueryBestSizeReply, QueryBestSizeRequest, QueryPortAttributesRequest,
    SetPortAttributeRequest, UngrabPortRequest, GET_PORT_ATTRIBUTE_REQUEST, GRAB_PORT_REQUEST,
    QUERY_BEST_SIZE_REQUEST, QUERY_PORT_ATTRIBUTES_REQUEST, SET_PORT_ATTRIBUTE_REQUEST,
    UNGRAB_PORT_REQUEST,
};

pub(crate) fn handle_port_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
) -> Vec<u8> {
    let xv_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, XV_MAJOR_OPCODE, u16::from(minor))
    };
    match minor {
        GRAB_PORT_REQUEST => {
            let req = parse_minor!(GrabPortRequest, data, state, seq, XV_MAJOR_OPCODE, minor);
            let port = req.port;
            debug!("XVideo GrabPort: port={port}");
            let ps = state.xv_ports.entry(port).or_default();
            let result = if ps.grabbed {
                GrabPortStatus::ALREADY_GRABBED
            } else {
                ps.grabbed = true;
                GrabPortStatus::SUCCESS
            };
            let reply = GrabPortReply {
                result,
                sequence: seq,
                length: 0,
            };
            build_reply(&reply, state.byte_order())
        }
        UNGRAB_PORT_REQUEST => {
            let req = parse_minor!(UngrabPortRequest, data, state, seq, XV_MAJOR_OPCODE, minor);
            let port = req.port;
            debug!("XVideo UngrabPort: port={port}");
            if let Some(ps) = state.xv_ports.get_mut(&port) {
                ps.grabbed = false;
            }
            Vec::new()
        }
        QUERY_BEST_SIZE_REQUEST => {
            let req = parse_minor!(
                QueryBestSizeRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let reply = QueryBestSizeReply {
                sequence: seq,
                length: 0,
                actual_width: req.vid_w,
                actual_height: req.vid_h,
            };
            build_reply(&reply, state.byte_order())
        }
        SET_PORT_ATTRIBUTE_REQUEST => {
            let req = parse_minor!(
                SetPortAttributeRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let port = req.port;
            let atom = req.attribute;
            let value = req.value;
            let name = state.get_atom_name(atom).unwrap_or_default();
            let ps = state.xv_ports.entry(port).or_default();
            match name.as_str() {
                XV_ATTR_BRIGHTNESS => ps.brightness = value.clamp(-1000, 1000),
                XV_ATTR_CONTRAST => ps.contrast = value.clamp(0, 2000),
                XV_ATTR_SATURATION => ps.saturation = value.clamp(0, 2000),
                XV_ATTR_HUE => ps.hue = value.clamp(-180, 180),
                XV_ATTR_COLORSPACE => ps.colorspace = value.clamp(0, 1),
                _ => {
                    debug!("XVideo SetPortAttribute: unknown attr {name} (atom={atom})");
                    return xv_err(crate::xserver::core::MATCH_ERROR, atom);
                }
            }
            debug!("XVideo SetPortAttribute: port={port} {name}={value}");
            Vec::new()
        }
        GET_PORT_ATTRIBUTE_REQUEST => {
            let req = parse_minor!(
                GetPortAttributeRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let port = req.port;
            let atom = req.attribute;
            let name = state.get_atom_name(atom).unwrap_or_default();
            let ps = state.xv_ports.entry(port).or_default();
            let value = match name.as_str() {
                XV_ATTR_BRIGHTNESS => ps.brightness,
                XV_ATTR_CONTRAST => ps.contrast,
                XV_ATTR_SATURATION => ps.saturation,
                XV_ATTR_HUE => ps.hue,
                XV_ATTR_COLORSPACE => ps.colorspace,
                _ => {
                    debug!("XVideo GetPortAttribute: unknown attr {name} (atom={atom})");
                    return xv_err(crate::xserver::core::MATCH_ERROR, atom);
                }
            };
            let reply = GetPortAttributeReply {
                sequence: seq,
                length: 0,
                value,
            };
            build_reply(&reply, state.byte_order())
        }
        QUERY_PORT_ATTRIBUTES_REQUEST => {
            let _req = parse_minor!(
                QueryPortAttributesRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            // libXv reads `16 + info.size` bytes per attribute (no
            // inter-element alignment padding) and applies its own
            // null terminators. x11rb's `AttributeInfo::serialize_into`
            // pads every entry to a 4-byte boundary, which leaves
            // bytes unread and trips xcb's `extra reply data` guard.
            // So we hand-emit the trailing data using x11rb's struct
            // for the field values but matching libXv's layout.
            build_query_port_attributes_reply(seq, state.msb_first, &port_attributes())
        }
        _ => {
            debug!("XVideo port: unhandled minor opcode {minor}");
            xv_err(crate::xserver::core::REQUEST_ERROR, u32::from(minor))
        }
    }
}

struct PortAttribute {
    flags: AttributeFlag,
    min: i32,
    max: i32,
    name: &'static [u8],
}

fn port_attributes() -> [PortAttribute; 5] {
    let flags = AttributeFlag::GETTABLE | AttributeFlag::SETTABLE;
    [
        PortAttribute {
            flags,
            min: -1000,
            max: 1000,
            name: b"XV_BRIGHTNESS",
        },
        PortAttribute {
            flags,
            min: 0,
            max: 2000,
            name: b"XV_CONTRAST",
        },
        PortAttribute {
            flags,
            min: 0,
            max: 2000,
            name: b"XV_SATURATION",
        },
        PortAttribute {
            flags,
            min: -180,
            max: 180,
            name: b"XV_HUE",
        },
        PortAttribute {
            flags,
            min: 0,
            max: 1,
            name: b"XV_COLORSPACE",
        },
    ]
}

/// Build a `QueryPortAttributesReply` with the wire layout libXv
/// actually parses: 16-byte AttributeInfo (size = name.len()) plus
/// `name.len()` raw bytes per entry, no inter-element alignment.
/// x11rb's codegen pads every entry to a 4-byte boundary, which
/// leaves bytes unread and trips xcb's `extra reply data` guard.
fn build_query_port_attributes_reply(
    seq: u16,
    msb_first: bool,
    attrs: &[PortAttribute],
) -> Vec<u8> {
    const ATTR_INFO_BYTES: usize = 16;
    let trailing_bytes: usize = attrs.iter().map(|a| ATTR_INFO_BYTES + a.name.len()).sum();
    debug_assert!(trailing_bytes.is_multiple_of(4), "trailing must be 4-aligned");
    let text_size: u32 = attrs.iter().map(|a| a.name.len() as u32 + 1).sum();
    let mut reply = ReplyBuf::with_extra(seq, trailing_bytes, msb_first)
        .set_u32(8, attrs.len() as u32) // num_attributes
        .set_u32(12, text_size);
    let mut off = 32;
    for attr in attrs {
        reply = reply
            .set_u32(off, u32::from(attr.flags))
            .set_u32(off + 4, attr.min as u32)
            .set_u32(off + 8, attr.max as u32)
            .set_u32(off + 12, attr.name.len() as u32)
            .set_bytes(off + ATTR_INFO_BYTES, attr.name);
        off += ATTR_INFO_BYTES + attr.name.len();
    }
    reply.build()
}

