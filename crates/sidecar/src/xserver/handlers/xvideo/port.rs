//! XVideo port operations: GrabPort, UngrabPort, QueryBestSize, SetPortAttribute,
//! GetPortAttribute, QueryPortAttributes.

use tracing::debug;

use super::super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;
use super::{
    XV_ATTR_BRIGHTNESS, XV_ATTR_COLORSPACE, XV_ATTR_CONTRAST, XV_ATTR_HUE, XV_ATTR_SATURATION,
};

pub(crate) fn handle_port_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
) -> Vec<u8> {
    match minor {
        3 => {
            // XvGrabPort
            if data.len() >= 8 {
                let port = state.read_u32(data, 4);
                debug!("XVideo GrabPort: port={port}");
                // Ensure port state exists and mark as grabbed
                let ps = state.xv_ports.entry(port).or_default();
                if ps.grabbed {
                    // Already grabbed — return AlreadyGrabbed (1)
                    return ReplyBuf::fixed(seq, state.msb_first)
                        .set_data_byte(1) // result = AlreadyGrabbed
                        .build();
                }
                ps.grabbed = true;
                ReplyBuf::fixed(seq, state.msb_first)
                    .set_data_byte(0) // result = Success
                    .build()
            } else {
                Vec::new()
            }
        }
        4 => {
            // XvUngrabPort
            if data.len() >= 8 {
                let port = state.read_u32(data, 4);
                debug!("XVideo UngrabPort: port={port}");
                if let Some(ps) = state.xv_ports.get_mut(&port) {
                    ps.grabbed = false;
                }
            }
            Vec::new()
        }
        9 => {
            // XvQueryBestSize
            let mut reply = ReplyBuf::fixed(seq, state.msb_first);
            if data.len() >= 16 {
                let w = state.read_u16(data, 8);
                let h = state.read_u16(data, 10);
                reply = reply.set_u16(8, w).set_u16(10, h);
            }
            reply.build()
        }
        10 => {
            // XvSetPortAttribute
            if data.len() >= 16 {
                let port = state.read_u32(data, 4);
                let atom = state.read_u32(data, 8);
                let value = state.read_u32(data, 12) as i32;

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
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::MATCH_ERROR,
                            seq,
                            atom,
                            156,
                            10,
                            state.msb_first,
                        );
                    }
                }
                debug!("XVideo SetPortAttribute: port={port} {name}={value}");
            }
            Vec::new()
        }
        11 => {
            // XvGetPortAttribute
            if data.len() >= 12 {
                let port = state.read_u32(data, 4);
                let atom = state.read_u32(data, 8);

                let name = state.get_atom_name(atom).unwrap_or_default();
                let ps = state.xv_ports.entry(port).or_default();

                let value: i32 = match name.as_str() {
                    XV_ATTR_BRIGHTNESS => ps.brightness,
                    XV_ATTR_CONTRAST => ps.contrast,
                    XV_ATTR_SATURATION => ps.saturation,
                    XV_ATTR_HUE => ps.hue,
                    XV_ATTR_COLORSPACE => ps.colorspace,
                    _ => {
                        debug!("XVideo GetPortAttribute: unknown attr {name} (atom={atom})");
                        return crate::xserver::core::build_error_bo(
                            crate::xserver::core::MATCH_ERROR,
                            seq,
                            atom,
                            156,
                            11,
                            state.msb_first,
                        );
                    }
                };

                ReplyBuf::fixed(seq, state.msb_first)
                    .set_u32(8, value as u32)
                    .build()
            } else {
                Vec::new()
            }
        }
        15 => {
            // XvQueryPortAttributes
            // Return 5 attributes: BRIGHTNESS, CONTRAST, SATURATION, HUE, COLORSPACE
            struct AttrDef {
                name: &'static [u8],
                min: i32,
                max: i32,
                flags: u32, // bit 0 = Gettable, bit 1 = Settable
            }
            let attrs = [
                AttrDef {
                    name: b"XV_BRIGHTNESS",
                    min: -1000,
                    max: 1000,
                    flags: 3,
                },
                AttrDef {
                    name: b"XV_CONTRAST",
                    min: 0,
                    max: 2000,
                    flags: 3,
                },
                AttrDef {
                    name: b"XV_SATURATION",
                    min: 0,
                    max: 2000,
                    flags: 3,
                },
                AttrDef {
                    name: b"XV_HUE",
                    min: -180,
                    max: 180,
                    flags: 3,
                },
                AttrDef {
                    name: b"XV_COLORSPACE",
                    min: 0,
                    max: 1,
                    flags: 3,
                },
            ];

            // Each AttributeInfo: flags(4) + min(4) + max(4) + size(4) + name(padded)
            let mut extra_data = Vec::new();
            for attr in &attrs {
                let name_padded = (attr.name.len() + 3) & !3;
                let mut buf = vec![0u8; 16 + name_padded];
                state.write_u32(&mut buf, 0, attr.flags);
                state.write_u32(&mut buf, 4, attr.min as u32);
                state.write_u32(&mut buf, 8, attr.max as u32);
                state.write_u32(&mut buf, 12, attr.name.len() as u32);
                buf[16..16 + attr.name.len()].copy_from_slice(attr.name);
                extra_data.extend_from_slice(&buf);
            }
            let reply = ReplyBuf::with_extra(seq, extra_data.len(), state.msb_first)
                .set_u32(8, attrs.len() as u32) // num_attributes
                .set_bytes(32, &extra_data);
            reply.build()
        }
        _ => {
            debug!("XVideo port: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                156,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
