//! XVideo (Xv) extension handler.

mod dcv_convert;
mod image;
mod notify;
mod port;

use super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;

/// XVideo (Xv) (opcode 156)
///
/// Software-only video adaptor supporting basic YUV/RGB overlay rendering.
/// Port 100 is the single available port on the software adaptor.
pub(crate) const XV_PORT_BASE: u32 = 100;

/// Number of XVideo ports exposed by our software adaptor.
pub(crate) const XV_NUM_PORTS: u32 = 1;

/// FOURCC identifiers for supported image formats.
pub(crate) const FOURCC_YUY2: u32 = 0x32595559; // 'YUY2'
pub(crate) const FOURCC_I420: u32 = 0x30323449; // 'I420'
pub(crate) const FOURCC_YV12: u32 = 0x32315659; // 'YV12'
pub(crate) const FOURCC_UYVY: u32 = 0x59565955; // 'UYVY'
pub(crate) const FOURCC_NV12: u32 = 0x3231564E; // 'NV12'
pub(crate) const FOURCC_NV21: u32 = 0x3132564E; // 'NV21'
pub(crate) const FOURCC_YV16: u32 = 0x36315659; // 'YV16'
pub(crate) const FOURCC_RGB3: u32 = 0x33424752; // 'RGB3' (packed RGB24)
pub(crate) const FOURCC_RV32: u32 = 0x32335652; // 'RV32' (packed RGBA32/BGRA32)
pub(crate) const FOURCC_Y800: u32 = 0x30303859; // 'Y800' / 'GREY' (8-bit grayscale)

/// A frame captured by GetStill: raw ARGB32 pixels plus dimensions.
#[derive(Clone, Debug)]
pub(crate) struct CapturedFrame {
    /// Width in pixels.
    pub(crate) width: u16,
    /// Height in pixels.
    pub(crate) height: u16,
    /// ARGB32 pixel data (4 bytes per pixel, row-major).
    pub(crate) data: Vec<u8>,
}

/// Per-port XVideo state: attributes and allocation tracking.
#[derive(Clone, Debug)]
pub(crate) struct XvPortState {
    /// Whether this port is currently grabbed by a client.
    pub(crate) grabbed: bool,
    /// Brightness adjustment (-1000..1000, default 0).
    pub(crate) brightness: i32,
    /// Contrast adjustment (0..2000, default 1000 = 1.0x).
    pub(crate) contrast: i32,
    /// Saturation adjustment (0..2000, default 1000 = 1.0x).
    pub(crate) saturation: i32,
    /// Hue adjustment (-180..180 degrees, default 0).
    pub(crate) hue: i32,
    /// Colorspace: 0 = BT.601 (SD), 1 = BT.709 (HD).
    pub(crate) colorspace: i32,
    /// Captured frame from GetStill: ARGB32 pixel data and dimensions.
    /// Populated by GetStill, consumed by PutStill / PutVideo.
    pub(crate) captured_frame: Option<CapturedFrame>,
}

impl Default for XvPortState {
    fn default() -> Self {
        Self {
            grabbed: false,
            brightness: 0,
            contrast: 1000,
            saturation: 1000,
            hue: 0,
            colorspace: 0, // BT.601
            captured_frame: None,
        }
    }
}

/// Atom name constants for XVideo port attributes.
pub(crate) const XV_ATTR_BRIGHTNESS: &str = "XV_BRIGHTNESS";
pub(crate) const XV_ATTR_CONTRAST: &str = "XV_CONTRAST";
pub(crate) const XV_ATTR_SATURATION: &str = "XV_SATURATION";
pub(crate) const XV_ATTR_HUE: &str = "XV_HUE";
pub(crate) const XV_ATTR_COLORSPACE: &str = "XV_COLORSPACE";

pub(crate) fn handle_xvideo_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // XvQueryExtension
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 2) // major
                .set_u16(10, 2) // minor
                .build()
        }
        1 => {
            // XvQueryAdaptors
            // Return one software adaptor with a single port
            let adaptor_name = b"x11-web Software Video Adaptor";
            let name_len = adaptor_name.len();
            let name_padded = (name_len + 3) & !3;

            // AdaptorInfo structure:
            // base_id(4) + name_size(2) + num_ports(2) + num_formats(2) + type(1) + pad(1) + name(padded)
            // + format list: visual(4) + depth(1) + pad(3) per format
            let format_entry_size = 8; // visual(4) + depth(1) + pad(3)
            let adaptor_size = 12 + name_padded + format_entry_size; // 1 format

            let mut reply = ReplyBuf::with_extra(seq, adaptor_size, state.msb_first).set_u16(8, 1); // num_adaptors

            // Adaptor info
            let off = 32;
            {
                let buf = reply.buf_mut();
                state.write_u32(&mut buf[off..], 0, XV_PORT_BASE); // base_id
                state.write_u16(&mut buf[off..], 4, name_len as u16); // name_size
                state.write_u16(&mut buf[off..], 6, XV_NUM_PORTS as u16); // num_ports
                state.write_u16(&mut buf[off..], 8, 1); // num_formats
                buf[off + 10] = 0x06; // type = InputMask | ImageMask (video input + image)

                // Name
                buf[off + 12..off + 12 + name_len].copy_from_slice(adaptor_name);

                // Format: visual + depth
                let fmt_off = off + 12 + name_padded;
                state.write_u32(&mut buf[fmt_off..], 0, 0x21); // root visual
                buf[fmt_off + 4] = 24; // depth
            }

            reply.build()
        }
        2 => {
            // XvQueryEncodings
            // Return one encoding (the screen itself)
            let enc_name = b"XV_IMAGE";
            let name_len = enc_name.len();
            let name_padded = (name_len + 3) & !3;
            let enc_size = 16 + name_padded; // id(4) + name_size(2) + width(2) + height(2) + rate_num(4) + rate_den(4) + name

            let mut reply = ReplyBuf::with_extra(seq, enc_size, state.msb_first).set_u16(8, 1); // num_encodings

            let off = 32;
            {
                let buf = reply.buf_mut();
                state.write_u32(&mut buf[off..], 0, 0); // encoding_id
                state.write_u16(&mut buf[off..], 4, name_len as u16);
                state.write_u16(&mut buf[off..], 6, state.screen_width);
                state.write_u16(&mut buf[off..], 8, state.screen_height);
                // rate = 30/1
                state.write_u32(&mut buf[off..], 12, 30); // numerator
                state.write_u32(&mut buf[off..], 16, 1); // denominator (skip 10-11 padding)
                buf[off + 20..off + 20 + name_len].copy_from_slice(enc_name);
            }
            reply.build()
        }
        3 | 4 | 9 | 10 | 11 | 15 => port::handle_port_request(state, data, seq, minor),
        5 | 6 | 7 | 8 | 12 | 16 | 17 | 18 | 19 | 20 => {
            image::handle_image_request(state, data, seq, minor)
        }
        13 | 14 => notify::handle_notify_request(state, data, seq, minor),
        _ => crate::xserver::core::build_error(
            crate::xserver::core::REQUEST_ERROR,
            seq,
            minor as u32,
            156,
            minor as u16,
        ),
    }
}
