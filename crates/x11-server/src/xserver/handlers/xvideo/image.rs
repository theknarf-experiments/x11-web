//! XVideo image operations: PutImage, ShmPutImage, PutStill, GetStill, PutVideo, GetVideo,
//! StopVideo, ListImageFormats, QueryImageAttributes, plus YUV conversion code.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::parse_minor;
use super::{
    build_var_reply, fourcc_yuv_format, rgb_format, CapturedFrame,
    FOURCC_I420, FOURCC_NV12, FOURCC_NV21, FOURCC_RGB3, FOURCC_RV32, FOURCC_UYVY, FOURCC_Y800,
    FOURCC_YUY2, FOURCC_YV12, FOURCC_YV16, XV_MAJOR_OPCODE,
};
use x11rb_protocol::protocol::xv::{
    GetStillRequest, GetVideoRequest, ImageFormatInfoFormat, ListImageFormatsReply,
    ListImageFormatsRequest, PutImageRequest, PutStillRequest, PutVideoRequest,
    QueryImageAttributesReply, QueryImageAttributesRequest as XvQueryImageAttributesRequest,
    ShmPutImageRequest, StopVideoRequest, GET_STILL_REQUEST, GET_VIDEO_REQUEST,
    LIST_IMAGE_FORMATS_REQUEST, PUT_IMAGE_REQUEST, PUT_STILL_REQUEST, PUT_VIDEO_REQUEST,
    QUERY_IMAGE_ATTRIBUTES_REQUEST, SHM_PUT_IMAGE_REQUEST, STOP_VIDEO_REQUEST,
};

// ---------------------------------------------------------------------------
// YUV → RGB conversion (integer math, no floating point)
// ---------------------------------------------------------------------------

/// Clamp an i32 to the 0..255 range and return as u8.
#[inline(always)]
fn clamp_u8(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

/// Chroma offset: U/V are stored as `value + 128` so the unsigned byte
/// covers -128..=127.
const CHROMA_OFFSET: i32 = 128;
/// Right-shift count corresponding to the 1/256 scaling factor baked into
/// the integer coefficients below.
const COEFF_SHIFT: u32 = 8;

/// BT.601 (SDTV) integer coefficients for YUV→RGB.
///
/// `(R, G_cr, G_cb, B)` — multiplied by `(V-128)` or `(U-128)` and then
/// shifted right by `COEFF_SHIFT` to recover ~1.0 scaling.
const BT601_R_CR: i32 = 351;
const BT601_G_CR: i32 = 179;
const BT601_G_CB: i32 = 86;
const BT601_B_CB: i32 = 443;

/// BT.709 (HDTV) integer coefficients for YUV→RGB.
const BT709_R_CR: i32 = 403;
const BT709_G_CR: i32 = 120;
const BT709_G_CB: i32 = 48;
const BT709_B_CB: i32 = 475;

/// Convert a single YUV pixel to (R, G, B) using BT.601 coefficients.
#[inline(always)]
fn yuv_to_rgb_bt601(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let y = y as i32;
    let cb = u as i32 - CHROMA_OFFSET;
    let cr = v as i32 - CHROMA_OFFSET;

    let r = y + ((BT601_R_CR * cr) >> COEFF_SHIFT);
    let g = y - ((BT601_G_CR * cr + BT601_G_CB * cb) >> COEFF_SHIFT);
    let b = y + ((BT601_B_CB * cb) >> COEFF_SHIFT);

    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Convert a single YUV pixel to (R, G, B) using BT.709 coefficients.
#[inline(always)]
fn yuv_to_rgb_bt709(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let y = y as i32;
    let cb = u as i32 - CHROMA_OFFSET;
    let cr = v as i32 - CHROMA_OFFSET;

    let r = y + ((BT709_R_CR * cr) >> COEFF_SHIFT);
    let g = y - ((BT709_G_CR * cr + BT709_G_CB * cb) >> COEFF_SHIFT);
    let b = y + ((BT709_B_CB * cb) >> COEFF_SHIFT);

    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Type alias for YUV→RGB conversion function pointer.
type YuvToRgb = fn(u8, u8, u8) -> (u8, u8, u8);

/// XVideo `XV_COLORSPACE` port-attribute values.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XvColorspace {
    /// ITU-R BT.601 (SDTV — default).
    Bt601 = 0,
    /// ITU-R BT.709 (HDTV).
    Bt709 = 1,
}

impl From<i32> for XvColorspace {
    fn from(value: i32) -> Self {
        match value {
            1 => XvColorspace::Bt709,
            _ => XvColorspace::Bt601,
        }
    }
}

/// Select the conversion function based on colorspace.
fn select_converter(colorspace: i32) -> YuvToRgb {
    match XvColorspace::from(colorspace) {
        XvColorspace::Bt709 => yuv_to_rgb_bt709,
        XvColorspace::Bt601 => yuv_to_rgb_bt601,
    }
}

/// Convert YUY2 (packed YUV 4:2:2) data to ARGB32.
///
/// Layout: [Y0, U0, Y1, V0] repeated. Each 4-byte macro-pixel encodes 2 horizontal pixels.
fn convert_yuy2_to_argb(yuv: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = w * 2; // 2 bytes per pixel (16bpp packed)

    if yuv.len() < src_stride * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        let row_off = row * src_stride;
        for pair in 0..(w / 2) {
            let off = row_off + pair * 4;
            let y0 = yuv[off];
            let u = yuv[off + 1];
            let y1 = yuv[off + 2];
            let v = yuv[off + 3];

            let (r0, g0, b0) = conv(y0, u, v);
            let (r1, g1, b1) = conv(y1, u, v);

            let dst0 = (row * w + pair * 2) * 4;
            argb[dst0] = r0;
            argb[dst0 + 1] = g0;
            argb[dst0 + 2] = b0;
            argb[dst0 + 3] = 0xFF;

            let dst1 = dst0 + 4;
            argb[dst1] = r1;
            argb[dst1 + 1] = g1;
            argb[dst1 + 2] = b1;
            argb[dst1 + 3] = 0xFF;
        }
        // Handle odd trailing pixel
        if w & 1 != 0 {
            let off = row_off + (w - 1) * 2;
            if off + 2 <= yuv.len() {
                let y0 = yuv[off];
                let u = yuv[off + 1];
                let (r, g, b) = conv(y0, u, 128);
                let dst = (row * w + w - 1) * 4;
                argb[dst] = r;
                argb[dst + 1] = g;
                argb[dst + 2] = b;
                argb[dst + 3] = 0xFF;
            }
        }
    }
    argb
}

/// Convert UYVY (packed YUV 4:2:2, alternate byte order) data to ARGB32.
///
/// Layout: [U0, Y0, V0, Y1] repeated. Each 4-byte macro-pixel encodes 2 horizontal pixels.
fn convert_uyvy_to_argb(yuv: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = w * 2;

    if yuv.len() < src_stride * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        let row_off = row * src_stride;
        for pair in 0..(w / 2) {
            let off = row_off + pair * 4;
            let u = yuv[off];
            let y0 = yuv[off + 1];
            let v = yuv[off + 2];
            let y1 = yuv[off + 3];

            let (r0, g0, b0) = conv(y0, u, v);
            let (r1, g1, b1) = conv(y1, u, v);

            let dst0 = (row * w + pair * 2) * 4;
            argb[dst0] = r0;
            argb[dst0 + 1] = g0;
            argb[dst0 + 2] = b0;
            argb[dst0 + 3] = 0xFF;

            let dst1 = dst0 + 4;
            argb[dst1] = r1;
            argb[dst1 + 1] = g1;
            argb[dst1 + 2] = b1;
            argb[dst1 + 3] = 0xFF;
        }
        // Handle odd trailing pixel
        if w & 1 != 0 {
            let off = row_off + (w - 1) * 2;
            if off + 2 <= yuv.len() {
                let u = yuv[off];
                let y0 = yuv[off + 1];
                let (r, g, b) = conv(y0, u, 128);
                let dst = (row * w + w - 1) * 4;
                argb[dst] = r;
                argb[dst + 1] = g;
                argb[dst + 2] = b;
                argb[dst + 3] = 0xFF;
            }
        }
    }
    argb
}

/// Convert YV16 (planar YUV 4:2:2) data to ARGB32.
///
/// Layout: Y plane (width * height), V plane (width/2 * height), U plane (width/2 * height).
/// Like YV12 but chroma is subsampled only horizontally (not vertically).
fn convert_yv16_to_argb(data: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2);
    let uv_size = uv_stride * h; // full height, half width

    if data.len() < y_size + 2 * uv_size {
        return vec![0u8; w * h * 4];
    }

    let y_plane = &data[..y_size];
    // YV16: V before U (same convention as YV12)
    let v_plane = &data[y_size..y_size + uv_size];
    let u_plane = &data[y_size + uv_size..y_size + 2 * uv_size];

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col];
            let uv_col = col / 2;
            let u = u_plane[row * uv_stride + uv_col];
            let v = v_plane[row * uv_stride + uv_col];
            let (r, g, b) = conv(y, u, v);
            let off = (row * w + col) * 4;
            argb[off] = r;
            argb[off + 1] = g;
            argb[off + 2] = b;
            argb[off + 3] = 0xFF;
        }
    }
    argb
}

/// Convert packed RGB24 (RGB3) data to ARGB32.
///
/// Layout: 3 bytes per pixel [R, G, B] in row-major order.
fn convert_rgb3_to_argb(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = w * 3;

    if data.len() < src_stride * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let src_off = row * src_stride + col * 3;
            let r = data[src_off];
            let g = data[src_off + 1];
            let b = data[src_off + 2];
            let dst_off = (row * w + col) * 4;
            argb[dst_off] = r; // R
            argb[dst_off + 1] = g; // G
            argb[dst_off + 2] = b; // B
            argb[dst_off + 3] = 0xFF; // A
        }
    }
    argb
}

/// Convert packed BGRA32 (RV32) data to RGBA32.
///
/// Layout: input is 4 bytes per pixel `[B, G, R, A]`; framebuffer
/// storage expects `[R, G, B, A]`, so we swap channels 0 and 2.
fn convert_rv32_to_argb(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = w * 4;

    if data.len() < src_stride * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = data[..src_stride * h].to_vec();
    crate::framebuffer::swap_br_in_place(&mut argb);
    argb
}

/// Convert 8-bit grayscale (Y800/GREY) data to ARGB32.
///
/// Layout: 1 byte per pixel (luma only). Each pixel is replicated to R=G=B=Y.
fn convert_grey_to_argb(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;

    if data.len() < w * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = data[row * w + col];
            let off = (row * w + col) * 4;
            argb[off] = y; // R
            argb[off + 1] = y; // G
            argb[off + 2] = y; // B
            argb[off + 3] = 0xFF; // A
        }
    }
    argb
}

/// Convert YUV data to ARGB32 based on the FOURCC format identifier.
fn convert_yuv_to_argb(
    fourcc: u32,
    yuv: &[u8],
    width: u32,
    height: u32,
    colorspace: i32,
) -> Option<Vec<u8>> {
    let conv = select_converter(colorspace);
    match fourcc {
        // SIMD-optimized planar paths via dcv-color-primitives.
        FOURCC_I420 => Some(super::dcv_convert::i420_to_rgba(
            yuv, width, height, colorspace,
        )),
        FOURCC_YV12 => Some(super::dcv_convert::yv12_to_rgba(
            yuv, width, height, colorspace,
        )),
        FOURCC_NV12 => Some(super::dcv_convert::nv12_to_rgba(
            yuv, width, height, colorspace,
        )),
        FOURCC_NV21 => Some(super::dcv_convert::nv21_to_rgba(
            yuv, width, height, colorspace,
        )),
        // Packed 4:2:2 and 4:2:2 planar formats — dcv 1.0 doesn't support these.
        FOURCC_YUY2 => Some(convert_yuy2_to_argb(yuv, width, height, conv)),
        FOURCC_UYVY => Some(convert_uyvy_to_argb(yuv, width, height, conv)),
        FOURCC_YV16 => Some(convert_yv16_to_argb(yuv, width, height, conv)),
        // Trivial / non-YUV formats.
        FOURCC_RGB3 => Some(convert_rgb3_to_argb(yuv, width, height)),
        FOURCC_RV32 => Some(convert_rv32_to_argb(yuv, width, height)),
        FOURCC_Y800 => Some(convert_grey_to_argb(yuv, width, height)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ARGB → YUV conversion (for GetImage — reverse direction)
// ---------------------------------------------------------------------------

/// Compute the image data size, pitches, and offsets for a given FOURCC + dimensions.
/// Returns (data_size, pitches[3], offsets[3]).
fn query_image_attributes(fourcc: u32, width: u32, height: u32) -> (u32, [u32; 3], [u32; 3]) {
    match fourcc {
        FOURCC_I420 => {
            let y_pitch = width;
            let uv_pitch = width.div_ceil(2);
            let y_size = y_pitch * height;
            let uv_size = uv_pitch * height.div_ceil(2);
            let total = y_size + 2 * uv_size;
            (
                total,
                [y_pitch, uv_pitch, uv_pitch],
                [0, y_size, y_size + uv_size],
            )
        }
        FOURCC_YV12 => {
            let y_pitch = width;
            let uv_pitch = width.div_ceil(2);
            let y_size = y_pitch * height;
            let uv_size = uv_pitch * height.div_ceil(2);
            let total = y_size + 2 * uv_size;
            // YV12: V plane before U plane
            (
                total,
                [y_pitch, uv_pitch, uv_pitch],
                [0, y_size, y_size + uv_size],
            )
        }
        FOURCC_YUY2 | FOURCC_UYVY => {
            let pitch = width * 2;
            let total = pitch * height;
            (total, [pitch, 0, 0], [0, 0, 0])
        }
        FOURCC_NV12 | FOURCC_NV21 => {
            let y_pitch = width;
            let uv_pitch = width.div_ceil(2) * 2; // interleaved UV pairs
            let y_size = y_pitch * height;
            let uv_size = uv_pitch * height.div_ceil(2);
            let total = y_size + uv_size;
            (total, [y_pitch, uv_pitch, 0], [0, y_size, 0])
        }
        FOURCC_YV16 => {
            let y_pitch = width;
            let uv_pitch = width.div_ceil(2);
            let y_size = y_pitch * height;
            let uv_size = uv_pitch * height; // full height, half width
            let total = y_size + 2 * uv_size;
            (
                total,
                [y_pitch, uv_pitch, uv_pitch],
                [0, y_size, y_size + uv_size],
            )
        }
        FOURCC_RGB3 => {
            let pitch = width * 3;
            let total = pitch * height;
            (total, [pitch, 0, 0], [0, 0, 0])
        }
        FOURCC_RV32 => {
            let pitch = width * 4;
            let total = pitch * height;
            (total, [pitch, 0, 0], [0, 0, 0])
        }
        FOURCC_Y800 => {
            let pitch = width;
            let total = pitch * height;
            (total, [pitch, 0, 0], [0, 0, 0])
        }
        _ => (0, [0; 3], [0; 3]),
    }
}

/// Scale ARGB32 pixel data from (src_w x src_h) to (dst_w x dst_h) using nearest-neighbor.
fn scale_argb_nearest(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    let sw = src_w as usize;
    let dw = dst_w as usize;
    let dh = dst_h as usize;
    let sh = src_h as usize;
    let mut out = vec![0u8; dw * dh * 4];

    for dy in 0..dh {
        let sy = (dy * sh) / dh;
        let sy = sy.min(sh.saturating_sub(1));
        for dx in 0..dw {
            let sx = (dx * sw) / dw;
            let sx = sx.min(sw.saturating_sub(1));
            let src_off = (sy * sw + sx) * 4;
            let dst_off = (dy * dw + dx) * 4;
            if src_off + 4 <= src.len() {
                out[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
            }
        }
    }
    out
}

/// Blit ARGB32 pixels onto a drawable framebuffer at (dst_x, dst_y) with size (dst_w x dst_h).
fn xv_blit_to_drawable(
    state: &mut ClientState,
    drawable: u32,
    argb: &[u8],
    dst_x: i16,
    dst_y: i16,
    dst_w: u16,
    dst_h: u16,
) {
    if let Some(fb) = state.get_framebuffer_mut(drawable) {
        fb.put_image(dst_x, dst_y, dst_w, dst_h, argb);
    }
}

/// Core XVideo PutImage logic: decode YUV, scale, blit.
fn xv_put_image_impl(
    state: &mut ClientState,
    drawable: u32,
    port: u32,
    fourcc: u32,
    yuv_data: &[u8],
    src_w: u16,
    src_h: u16,
    dst_x: i16,
    dst_y: i16,
    dst_w: u16,
    dst_h: u16,
) {
    // Ensure port state exists
    state.xv_ports.entry(port).or_default();
    let colorspace = state.xv_ports.get(&port).map(|p| p.colorspace).unwrap_or(0);

    let argb = match convert_yuv_to_argb(fourcc, yuv_data, src_w as u32, src_h as u32, colorspace) {
        Some(px) => px,
        None => {
            debug!("XVideo PutImage: unsupported FOURCC {fourcc:#010x}");
            return;
        }
    };

    // Scale if source and destination dimensions differ
    let scaled = scale_argb_nearest(
        &argb,
        src_w as u32,
        src_h as u32,
        dst_w as u32,
        dst_h as u32,
    );

    xv_blit_to_drawable(state, drawable, &scaled, dst_x, dst_y, dst_w, dst_h);

    debug!(
        "XVideo PutImage: port={port} fourcc={fourcc:#010x} src={src_w}x{src_h} \
         dst=({dst_x},{dst_y} {dst_w}x{dst_h}) drawable={drawable:#x}"
    );
}

pub(crate) fn handle_image_request(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
) -> Vec<u8> {
    let xv_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, XV_MAJOR_OPCODE, u16::from(minor))
    };
    match minor {
        // Capture/playback we don't implement; per XVideo §4.3-4.5 these
        // return BadMatch for unsupported port operations.
        PUT_VIDEO_REQUEST => {
            let port = PutVideoRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            )
            .map(|r| r.port)
            .unwrap_or(0);
            debug!("XVideo PutVideo: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        PUT_STILL_REQUEST => {
            let port = PutStillRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            )
            .map(|r| r.port)
            .unwrap_or(0);
            debug!("XVideo PutStill: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        GET_VIDEO_REQUEST => {
            let port = GetVideoRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            )
            .map(|r| r.port)
            .unwrap_or(0);
            debug!("XVideo GetVideo: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        GET_STILL_REQUEST => {
            let req = parse_minor!(GetStillRequest, data, state, seq, XV_MAJOR_OPCODE, minor);
            let port = req.port;
            let drawable = req.drawable;
            debug!(
                "XVideo GetStill: port={port} drawable={drawable:#x} gc={:#x} \
                 vid=({},{} {}x{}) drw=({},{} {}x{})",
                req.gc,
                req.vid_x,
                req.vid_y,
                req.vid_w,
                req.vid_h,
                req.drw_x,
                req.drw_y,
                req.drw_w,
                req.drw_h,
            );
            if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
                return xv_err(crate::xserver::core::DRAWABLE_ERROR, drawable);
            }
            if !state.gcs.contains_key(&req.gc) {
                return xv_err(crate::xserver::core::G_CONTEXT_ERROR, req.gc);
            }
            let drw_x = req.drw_x;
            let drw_y = req.drw_y;
            let drw_w = req.drw_w;
            let drw_h = req.drw_h;
            let resolved = state.resolve_drawable(drawable);
            let pixels = if let Some(fb) = state.get_framebuffer_mut(resolved) {
                fb.extract_pixels(drw_x, drw_y, drw_w, drw_h)
            } else {
                vec![0u8; drw_w as usize * drw_h as usize * 4]
            };
            let port_state = state.xv_ports.entry(port).or_default();
            port_state.captured_frame = Some(CapturedFrame {
                width: drw_w,
                height: drw_h,
                data: pixels,
            });
            Vec::new()
        }
        STOP_VIDEO_REQUEST => {
            if let Ok(req) = StopVideoRequest::try_parse_request(
                crate::xserver::request::request_header(data),
                &data[4..],
            ) {
                debug!("XVideo StopVideo: port={}", req.port);
            }
            Vec::new()
        }
        LIST_IMAGE_FORMATS_REQUEST => {
            let _req = parse_minor!(
                ListImageFormatsRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let format = supported_image_formats();
            let reply = ListImageFormatsReply {
                sequence: seq,
                length: 0,
                format,
            };
            build_var_reply(&reply, state.byte_order())
        }
        QUERY_IMAGE_ATTRIBUTES_REQUEST => {
            let req = parse_minor!(
                XvQueryImageAttributesRequest,
                data,
                state,
                seq,
                XV_MAJOR_OPCODE,
                minor
            );
            let fourcc = req.id;
            let width = u32::from(req.width);
            let height = u32::from(req.height);
            let (data_size, pitches, offsets) = query_image_attributes(fourcc, width, height);
            let num_planes = num_planes_for(fourcc) as usize;
            let reply = QueryImageAttributesReply {
                sequence: seq,
                length: 0,
                data_size,
                width: req.width,
                height: req.height,
                pitches: pitches[..num_planes].to_vec(),
                offsets: offsets[..num_planes].to_vec(),
            };
            build_var_reply(&reply, state.byte_order())
        }
        PUT_IMAGE_REQUEST => {
            let req = parse_minor!(PutImageRequest, data, state, seq, XV_MAJOR_OPCODE, minor);
            let src_w = if req.src_w > 0 { req.src_w } else { req.width };
            let src_h = if req.src_h > 0 { req.src_h } else { req.height };
            xv_put_image_impl(
                state,
                req.drawable,
                req.port,
                req.id,
                &req.data,
                src_w,
                src_h,
                req.drw_x,
                req.drw_y,
                req.drw_w,
                req.drw_h,
            );
            Vec::new()
        }
        SHM_PUT_IMAGE_REQUEST => {
            let req = parse_minor!(ShmPutImageRequest, data, state, seq, XV_MAJOR_OPCODE, minor);
            let send_event = req.send_event != 0;
            let w = if req.src_w > 0 { req.src_w } else { req.width };
            let h = if req.src_h > 0 { req.src_h } else { req.height };
            let (data_size, _, _) = query_image_attributes(req.id, u32::from(w), u32::from(h));
            let offset = req.offset as usize;
            debug!(
                "XVideo ShmPutImage: port={} drawable={:#x} shmseg={} fourcc={:#010x} \
                 offset={} src={}x{} drw=({},{} {}x{}) img={}x{}",
                req.port,
                req.drawable,
                req.shmseg,
                req.id,
                offset,
                req.src_w,
                req.src_h,
                req.drw_x,
                req.drw_y,
                req.drw_w,
                req.drw_h,
                req.width,
                req.height,
            );
            if let Some(seg) = state.shm_segments.get(&req.shmseg) {
                if offset + data_size as usize <= seg.size {
                    let yuv_data = unsafe {
                        std::slice::from_raw_parts(seg.addr.add(offset), data_size as usize)
                    };
                    xv_put_image_impl(
                        state,
                        req.drawable,
                        req.port,
                        req.id,
                        yuv_data,
                        w,
                        h,
                        req.drw_x,
                        req.drw_y,
                        req.drw_w,
                        req.drw_h,
                    );
                } else {
                    debug!(
                        "XVideo ShmPutImage: out of bounds (offset={offset} + size={data_size} > seg.size={})",
                        seg.size
                    );
                }
            } else {
                debug!("XVideo ShmPutImage: unknown shmseg={}", req.shmseg);
            }
            if send_event {
                use x11rb_protocol::protocol::shm::CompletionEvent;
                return crate::xserver::event::serialize_event(
                    &CompletionEvent {
                        response_type: crate::xserver::extensions::SHM_FIRST_EVENT,
                        sequence: seq,
                        drawable: req.drawable,
                        minor_event: 0,
                        major_event: 0,
                        shmseg: req.shmseg,
                        offset: offset as u32,
                    },
                    state.msb_first,
                );
            }
            Vec::new()
        }
        _ => {
            debug!("XVideo image: unhandled minor opcode {minor}");
            xv_err(crate::xserver::core::REQUEST_ERROR, u32::from(minor))
        }
    }
}

fn num_planes_for(fourcc: u32) -> u8 {
    match fourcc {
        FOURCC_I420 | FOURCC_YV12 | FOURCC_YV16 => 3,
        FOURCC_NV12 | FOURCC_NV21 => 2,
        // Packed formats (YUY2, UYVY, RGB3, RV32, Y800) and anything we
        // don't explicitly recognise are treated as single-plane.
        _ => 1,
    }
}

fn supported_image_formats() -> Vec<x11rb_protocol::protocol::xv::ImageFormatInfo> {
    let packed = ImageFormatInfoFormat::PACKED;
    let planar = ImageFormatInfoFormat::PLANAR;
    vec![
        // 4:2:2 packed
        fourcc_yuv_format(FOURCC_YUY2, 16, 1, 2, 1, 2, 1, packed),
        fourcc_yuv_format(FOURCC_UYVY, 16, 1, 2, 1, 2, 1, packed),
        // 4:2:0 planar
        fourcc_yuv_format(FOURCC_I420, 12, 3, 2, 2, 2, 2, planar),
        fourcc_yuv_format(FOURCC_YV12, 12, 3, 2, 2, 2, 2, planar),
        // 4:2:0 semi-planar
        fourcc_yuv_format(FOURCC_NV12, 12, 2, 2, 2, 2, 2, planar),
        fourcc_yuv_format(FOURCC_NV21, 12, 2, 2, 2, 2, 2, planar),
        // 4:2:2 planar
        fourcc_yuv_format(FOURCC_YV16, 16, 3, 2, 1, 2, 1, planar),
        // RGB
        rgb_format(FOURCC_RGB3, 24, 24),
        rgb_format(FOURCC_RV32, 32, 24),
        // Greyscale
        fourcc_yuv_format(FOURCC_Y800, 8, 1, 0, 0, 0, 0, packed),
    ]
}

