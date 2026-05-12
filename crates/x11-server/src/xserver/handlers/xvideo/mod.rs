//! XVideo (Xv) extension handler.
//!
//! Software-only video adaptor supporting basic YUV/RGB overlay rendering.
//! Replies are constructed via x11rb's `xv` reply structs and the
//! generator-emitted `SerializeEndian` impls, which produce wire-correct
//! bytes for either LSB or MSB clients directly.

mod dcv_convert;
mod image;
mod notify;
mod port;

use tracing::debug;

use super::super::client::ClientState;
use super::parse_minor;
use x11rb_protocol::protocol::xv::{
    AdaptorInfo, EncodingInfo, Format, ImageFormatInfo, QueryAdaptorsReply, QueryAdaptorsRequest,
    QueryEncodingsReply, QueryEncodingsRequest, QueryExtensionReply, QueryExtensionRequest,
    Rational, ScanlineOrder, Type, GET_PORT_ATTRIBUTE_REQUEST, GET_STILL_REQUEST,
    GET_VIDEO_REQUEST, GRAB_PORT_REQUEST, LIST_IMAGE_FORMATS_REQUEST, PUT_IMAGE_REQUEST,
    PUT_STILL_REQUEST, PUT_VIDEO_REQUEST, QUERY_ADAPTORS_REQUEST, QUERY_BEST_SIZE_REQUEST,
    QUERY_ENCODINGS_REQUEST, QUERY_EXTENSION_REQUEST, QUERY_IMAGE_ATTRIBUTES_REQUEST,
    QUERY_PORT_ATTRIBUTES_REQUEST, SELECT_PORT_NOTIFY_REQUEST, SELECT_VIDEO_NOTIFY_REQUEST,
    SET_PORT_ATTRIBUTE_REQUEST, SHM_PUT_IMAGE_REQUEST, STOP_VIDEO_REQUEST, UNGRAB_PORT_REQUEST,
};
use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

/// XVideo (Xv) extension major opcode (assigned by ListExtensions).
pub(crate) const XV_MAJOR_OPCODE: u8 = 156;

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
#[allow(dead_code)]
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
        QUERY_EXTENSION_REQUEST => {
            let _req = parse_minor!(
                QueryExtensionRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let reply = QueryExtensionReply {
                sequence: seq,
                length: 0,
                major: 2,
                minor: 2,
            };
            build_reply(&reply, state.byte_order())
        }
        QUERY_ADAPTORS_REQUEST => {
            let _req = parse_minor!(
                QueryAdaptorsRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let reply = QueryAdaptorsReply {
                sequence: seq,
                length: 0,
                info: vec![AdaptorInfo {
                    base_id: XV_PORT_BASE,
                    num_ports: XV_NUM_PORTS as u16,
                    type_: Type::INPUT_MASK | Type::IMAGE_MASK,
                    name: b"x11-web Software Video Adaptor".to_vec(),
                    formats: vec![Format {
                        visual: 0x21,
                        depth: 24,
                    }],
                }],
            };
            build_var_reply(&reply, state.byte_order())
        }
        QUERY_ENCODINGS_REQUEST => {
            let _req = parse_minor!(
                QueryEncodingsRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let reply = QueryEncodingsReply {
                sequence: seq,
                length: 0,
                info: vec![EncodingInfo {
                    encoding: 0,
                    width: state.screen_width,
                    height: state.screen_height,
                    rate: Rational {
                        numerator: 30,
                        denominator: 1,
                    },
                    name: b"XV_IMAGE".to_vec(),
                }],
            };
            build_var_reply(&reply, state.byte_order())
        }
        GRAB_PORT_REQUEST
        | UNGRAB_PORT_REQUEST
        | QUERY_BEST_SIZE_REQUEST
        | SET_PORT_ATTRIBUTE_REQUEST
        | GET_PORT_ATTRIBUTE_REQUEST
        | QUERY_PORT_ATTRIBUTES_REQUEST => port::handle_port_request(state, data, seq, minor),
        PUT_VIDEO_REQUEST
        | PUT_STILL_REQUEST
        | GET_VIDEO_REQUEST
        | GET_STILL_REQUEST
        | STOP_VIDEO_REQUEST
        | LIST_IMAGE_FORMATS_REQUEST
        | QUERY_IMAGE_ATTRIBUTES_REQUEST
        | PUT_IMAGE_REQUEST
        | SHM_PUT_IMAGE_REQUEST => image::handle_image_request(state, data, seq, minor),
        SELECT_VIDEO_NOTIFY_REQUEST | SELECT_PORT_NOTIFY_REQUEST => {
            notify::handle_notify_request(state, data, seq, minor)
        }
        _ => {
            debug!("XVideo: unhandled minor opcode {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                u32::from(minor),
                XV_MAJOR_OPCODE,
                u16::from(minor),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Reply serialization (shared with port.rs / image.rs)
// ---------------------------------------------------------------------------

/// Serialize a fixed-size reply via the codegen-emitted
/// `SerializeEndian` impl, padding short reply types (e.g.
/// `GrabPortReply`) to the X11 32-byte minimum.
pub(super) fn build_reply<R: SerializeEndian>(reply: &R, byte_order: ByteOrder) -> Vec<u8> {
    const REPLY_MIN: usize = 32;
    let mut bytes = Vec::with_capacity(REPLY_MIN);
    reply.serialize_endian_into(&mut bytes, byte_order);
    if bytes.len() < REPLY_MIN {
        bytes.resize(REPLY_MIN, 0);
    }
    bytes
}

/// Variable-length reply: serialize via `SerializeEndian` and stamp the
/// X11 length field from the actual buffer size so it can never
/// disagree with the trailing payload.
pub(super) fn build_var_reply<R: SerializeEndian>(reply: &R, byte_order: ByteOrder) -> Vec<u8> {
    let mut bytes = Vec::new();
    reply.serialize_endian_into(&mut bytes, byte_order);
    fix_length(&mut bytes, byte_order);
    bytes
}

fn fix_length(bytes: &mut Vec<u8>, byte_order: ByteOrder) {
    const HEADER_BYTES: usize = 32;
    const WORD_BYTES: usize = 4;
    debug_assert!(bytes.len() >= HEADER_BYTES);
    debug_assert!((bytes.len() - HEADER_BYTES) % WORD_BYTES == 0);
    let length = u32::try_from((bytes.len() - HEADER_BYTES) / WORD_BYTES).expect("reply fits");
    let length_bytes = match byte_order {
        ByteOrder::Lsb => length.to_le_bytes(),
        ByteOrder::Msb => length.to_be_bytes(),
    };
    bytes[4..8].copy_from_slice(&length_bytes);
}

/// Build a YUV-format `ImageFormatInfo` for our software adaptor. The
/// canonical FOURCC GUID is the 4-char fourcc followed by a fixed
/// DirectShow tail that downstream tooling expects.
pub(super) fn fourcc_yuv_format(
    fourcc: u32,
    bpp: u8,
    num_planes: u8,
    horz_u_period: u32,
    vert_u_period: u32,
    horz_v_period: u32,
    vert_v_period: u32,
    format: x11rb_protocol::protocol::xv::ImageFormatInfoFormat,
) -> ImageFormatInfo {
    use x11rb_protocol::protocol::xproto::ImageOrder;
    use x11rb_protocol::protocol::xv::ImageFormatInfoType;
    ImageFormatInfo {
        id: fourcc,
        type_: ImageFormatInfoType::YUV,
        byte_order: ImageOrder::LSB_FIRST,
        guid: fourcc_guid(fourcc),
        bpp,
        num_planes,
        depth: 0,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
        format,
        y_sample_bits: 8,
        u_sample_bits: 8,
        v_sample_bits: 8,
        vhorz_y_period: 1,
        vhorz_u_period: horz_u_period,
        vhorz_v_period: horz_v_period,
        vvert_y_period: 1,
        vvert_u_period: vert_u_period,
        vvert_v_period: vert_v_period,
        vcomp_order: [0u8; 32],
        vscanline_order: ScanlineOrder::TOP_TO_BOTTOM,
    }
}

/// Build an RGB-format `ImageFormatInfo` for our software adaptor.
pub(super) fn rgb_format(fourcc: u32, bpp: u8, depth: u8) -> ImageFormatInfo {
    use x11rb_protocol::protocol::xproto::ImageOrder;
    use x11rb_protocol::protocol::xv::{ImageFormatInfoFormat, ImageFormatInfoType};
    ImageFormatInfo {
        id: fourcc,
        type_: ImageFormatInfoType::RGB,
        byte_order: ImageOrder::LSB_FIRST,
        guid: fourcc_guid(fourcc),
        bpp,
        num_planes: 1,
        depth,
        red_mask: 0x00FF_0000,
        green_mask: 0x0000_FF00,
        blue_mask: 0x0000_00FF,
        format: ImageFormatInfoFormat::PACKED,
        y_sample_bits: 0,
        u_sample_bits: 0,
        v_sample_bits: 0,
        vhorz_y_period: 0,
        vhorz_u_period: 0,
        vhorz_v_period: 0,
        vvert_y_period: 0,
        vvert_u_period: 0,
        vvert_v_period: 0,
        vcomp_order: [0u8; 32],
        vscanline_order: ScanlineOrder::TOP_TO_BOTTOM,
    }
}

/// Canonical FOURCC GUID (DirectShow / X11): first 4 bytes are the
/// fourcc, remaining 12 bytes are the fixed media-subtype tail.
fn fourcc_guid(fourcc: u32) -> [u8; 16] {
    let mut guid = [
        0, 0, 0, 0, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ];
    guid[0..4].copy_from_slice(&fourcc.to_le_bytes());
    guid
}
