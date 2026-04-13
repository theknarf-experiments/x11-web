//! XVideo port operations: GrabPort, UngrabPort, QueryBestSize, SetPortAttribute,
//! GetPortAttribute, QueryPortAttributes.

use tracing::debug;

use super::super::super::client::ClientState;
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
        3 => { // XvGrabPort
            if data.len() >= 8 {
                let port = state.read_u32(data, 4);
                debug!("XVideo GrabPort: port={port}");
                // Ensure port state exists and mark as grabbed
                let ps = state.xv_ports.entry(port).or_default();
                if ps.grabbed {
                    // Already grabbed — return AlreadyGrabbed (1)
                    let mut reply = [0u8; 32];
                    reply[0] = 1;
                    reply[1] = 1; // result = AlreadyGrabbed
                    state.write_u16(&mut reply, 2, seq);
                    return reply.to_vec();
                }
                ps.grabbed = true;
                let mut reply = [0u8; 32];
                reply[0] = 1;
                reply[1] = 0; // result = Success
                state.write_u16(&mut reply, 2, seq);
                reply.to_vec()
            } else {
                Vec::new()
            }
        }
        4 => { // XvUngrabPort
            if data.len() >= 8 {
                let port = state.read_u32(data, 4);
                debug!("XVideo UngrabPort: port={port}");
                if let Some(ps) = state.xv_ports.get_mut(&port) {
                    ps.grabbed = false;
                }
            }
            Vec::new()
        }
        9 => { // XvQueryBestSize
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            if data.len() >= 16 {
                let w = state.read_u16(data, 8);
                let h = state.read_u16(data, 10);
                state.write_u16(&mut reply, 8, w);
                state.write_u16(&mut reply, 10, h);
            }
            reply.to_vec()
        }
        10 => { // XvSetPortAttribute
            if data.len() >= 16 {
                let port = state.read_u32(data, 4);
                let atom = state.read_u32(data, 8);
                let value = state.read_u32(data, 12) as i32;

                let name = state.get_atom_name(atom).unwrap_or_default();
                let ps = state.xv_ports.entry(port).or_default();

                match name.as_str() {
                    XV_ATTR_BRIGHTNESS => ps.brightness = value.clamp(-1000, 1000),
                    XV_ATTR_CONTRAST   => ps.contrast = value.clamp(0, 2000),
                    XV_ATTR_SATURATION => ps.saturation = value.clamp(0, 2000),
                    XV_ATTR_HUE        => ps.hue = value.clamp(-180, 180),
                    XV_ATTR_COLORSPACE => ps.colorspace = value.clamp(0, 1),
                    _ => debug!("XVideo SetPortAttribute: unknown attr {name} (atom={atom})"),
                }
                debug!("XVideo SetPortAttribute: port={port} {name}={value}");
            }
            Vec::new()
        }
        11 => { // XvGetPortAttribute
            if data.len() >= 12 {
                let port = state.read_u32(data, 4);
                let atom = state.read_u32(data, 8);

                let name = state.get_atom_name(atom).unwrap_or_default();
                let ps = state.xv_ports.entry(port).or_default();

                let value: i32 = match name.as_str() {
                    XV_ATTR_BRIGHTNESS => ps.brightness,
                    XV_ATTR_CONTRAST   => ps.contrast,
                    XV_ATTR_SATURATION => ps.saturation,
                    XV_ATTR_HUE        => ps.hue,
                    XV_ATTR_COLORSPACE => ps.colorspace,
                    _ => 0,
                };

                let mut reply = [0u8; 32];
                reply[0] = 1;
                state.write_u16(&mut reply, 2, seq);
                state.write_u32(&mut reply, 8, value as u32);
                reply.to_vec()
            } else {
                Vec::new()
            }
        }
        15 => { // XvQueryPortAttributes
            // Return 5 attributes: BRIGHTNESS, CONTRAST, SATURATION, HUE, COLORSPACE
            struct AttrDef {
                name: &'static [u8],
                min: i32,
                max: i32,
                flags: u32, // bit 0 = Gettable, bit 1 = Settable
            }
            let attrs = [
                AttrDef { name: b"XV_BRIGHTNESS", min: -1000, max: 1000, flags: 3 },
                AttrDef { name: b"XV_CONTRAST",   min: 0,     max: 2000, flags: 3 },
                AttrDef { name: b"XV_SATURATION", min: 0,     max: 2000, flags: 3 },
                AttrDef { name: b"XV_HUE",        min: -180,  max: 180,  flags: 3 },
                AttrDef { name: b"XV_COLORSPACE",  min: 0,     max: 1,    flags: 3 },
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

            let extra_words = extra_data.len() / 4;
            let mut reply = vec![0u8; 32 + extra_data.len()];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, extra_words as u32);
            state.write_u32(&mut reply, 8, attrs.len() as u32); // num_attributes
            reply[32..].copy_from_slice(&extra_data);
            reply
        }
        _ => Vec::new(),
    }
}
