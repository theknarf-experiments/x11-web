//! XVideo image operations: PutImage, ShmPutImage, PutStill, GetStill, PutVideo, GetVideo,
//! StopVideo, ListImageFormats, QueryImageAttributes, plus YUV conversion code.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::parse_or_void;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;
use super::{
    CapturedFrame, FOURCC_I420, FOURCC_NV12, FOURCC_NV21, FOURCC_RGB3, FOURCC_RV32, FOURCC_UYVY,
    FOURCC_Y800, FOURCC_YUY2, FOURCC_YV12, FOURCC_YV16,
};
use x11rb_protocol::protocol::xv::{
    GetStillRequest, GetVideoRequest, PutImageRequest, PutStillRequest, PutVideoRequest,
    QueryImageAttributesRequest as XvQueryImageAttributesRequest, ShmPutImageRequest,
    StopVideoRequest,
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

/// Convert a single YUV pixel to (R, G, B) using BT.601 coefficients.
///
/// Integer approximations:
/// - R = Y + ((351 * (V - 128)) >> 8)
/// - G = Y - ((179 * (V - 128) + 86 * (U - 128)) >> 8)
/// - B = Y + ((443 * (U - 128)) >> 8)
#[inline(always)]
fn yuv_to_rgb_bt601(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let y = y as i32;
    let cb = u as i32 - 128;
    let cr = v as i32 - 128;

    let r = y + ((351 * cr) >> 8);
    let g = y - ((179 * cr + 86 * cb) >> 8);
    let b = y + ((443 * cb) >> 8);

    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Convert a single YUV pixel to (R, G, B) using BT.709 coefficients.
///
/// BT.709 integer approximations:
/// - R = Y + ((403 * (V - 128)) >> 8)
/// - G = Y - ((48 * (U - 128) + 120 * (V - 128)) >> 8)
/// - B = Y + ((475 * (U - 128)) >> 8)
#[inline(always)]
fn yuv_to_rgb_bt709(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let y = y as i32;
    let cb = u as i32 - 128;
    let cr = v as i32 - 128;

    let r = y + ((403 * cr) >> 8);
    let g = y - ((120 * cr + 48 * cb) >> 8);
    let b = y + ((475 * cb) >> 8);

    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Type alias for YUV→RGB conversion function pointer.
type YuvToRgb = fn(u8, u8, u8) -> (u8, u8, u8);

/// Select the conversion function based on colorspace.
fn select_converter(colorspace: i32) -> YuvToRgb {
    if colorspace == 1 {
        yuv_to_rgb_bt709
    } else {
        yuv_to_rgb_bt601
    }
}

/// Convert I420 (planar YUV 4:2:0) data to ARGB32.
///
/// Layout: Y plane (width * height), U plane (width/2 * height/2), V plane (width/2 * height/2).
fn convert_i420_to_argb(yuv: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2);
    let uv_size = uv_stride * h.div_ceil(2);

    if yuv.len() < y_size + 2 * uv_size {
        return vec![0u8; w * h * 4];
    }

    let y_plane = &yuv[..y_size];
    let u_plane = &yuv[y_size..y_size + uv_size];
    let v_plane = &yuv[y_size + uv_size..y_size + 2 * uv_size];

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col];
            let uv_row = row / 2;
            let uv_col = col / 2;
            let u = u_plane[uv_row * uv_stride + uv_col];
            let v = v_plane[uv_row * uv_stride + uv_col];
            let (r, g, b) = conv(y, u, v);
            let off = (row * w + col) * 4;
            argb[off] = b; // B
            argb[off + 1] = g; // G
            argb[off + 2] = r; // R
            argb[off + 3] = 0xFF; // A
        }
    }
    argb
}

/// Convert YV12 (planar YUV 4:2:0, V before U) data to ARGB32.
///
/// Layout: Y plane (width * height), V plane (width/2 * height/2), U plane (width/2 * height/2).
fn convert_yv12_to_argb(yuv: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2);
    let uv_size = uv_stride * h.div_ceil(2);

    if yuv.len() < y_size + 2 * uv_size {
        return vec![0u8; w * h * 4];
    }

    let y_plane = &yuv[..y_size];
    // YV12: V before U
    let v_plane = &yuv[y_size..y_size + uv_size];
    let u_plane = &yuv[y_size + uv_size..y_size + 2 * uv_size];

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col];
            let uv_row = row / 2;
            let uv_col = col / 2;
            let u = u_plane[uv_row * uv_stride + uv_col];
            let v = v_plane[uv_row * uv_stride + uv_col];
            let (r, g, b) = conv(y, u, v);
            let off = (row * w + col) * 4;
            argb[off] = b;
            argb[off + 1] = g;
            argb[off + 2] = r;
            argb[off + 3] = 0xFF;
        }
    }
    argb
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
            argb[dst0] = b0;
            argb[dst0 + 1] = g0;
            argb[dst0 + 2] = r0;
            argb[dst0 + 3] = 0xFF;

            let dst1 = dst0 + 4;
            argb[dst1] = b1;
            argb[dst1 + 1] = g1;
            argb[dst1 + 2] = r1;
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
                argb[dst] = b;
                argb[dst + 1] = g;
                argb[dst + 2] = r;
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
            argb[dst0] = b0;
            argb[dst0 + 1] = g0;
            argb[dst0 + 2] = r0;
            argb[dst0 + 3] = 0xFF;

            let dst1 = dst0 + 4;
            argb[dst1] = b1;
            argb[dst1 + 1] = g1;
            argb[dst1 + 2] = r1;
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
                argb[dst] = b;
                argb[dst + 1] = g;
                argb[dst + 2] = r;
                argb[dst + 3] = 0xFF;
            }
        }
    }
    argb
}

/// Convert NV12 (semi-planar YUV 4:2:0, interleaved UV) data to ARGB32.
///
/// Layout: Y plane (width * height), then interleaved UV pairs (width/2 * height/2 * 2 bytes).
fn convert_nv12_to_argb(data: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2) * 2; // interleaved UV, so stride is width (rounded up to even)
    let uv_rows = h.div_ceil(2);
    let uv_size = uv_stride * uv_rows;

    if data.len() < y_size + uv_size {
        return vec![0u8; w * h * 4];
    }

    let y_plane = &data[..y_size];
    let uv_plane = &data[y_size..y_size + uv_size];

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col];
            let uv_row = row / 2;
            let uv_col = col / 2;
            let u = uv_plane[uv_row * uv_stride + uv_col * 2];
            let v = uv_plane[uv_row * uv_stride + uv_col * 2 + 1];
            let (r, g, b) = conv(y, u, v);
            let off = (row * w + col) * 4;
            argb[off] = b;
            argb[off + 1] = g;
            argb[off + 2] = r;
            argb[off + 3] = 0xFF;
        }
    }
    argb
}

/// Convert NV21 (semi-planar YUV 4:2:0, interleaved VU) data to ARGB32.
///
/// Layout: Y plane (width * height), then interleaved VU pairs (width/2 * height/2 * 2 bytes).
/// Same as NV12 but with V and U swapped in the interleaved plane.
fn convert_nv21_to_argb(data: &[u8], width: u32, height: u32, conv: YuvToRgb) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2) * 2;
    let uv_rows = h.div_ceil(2);
    let uv_size = uv_stride * uv_rows;

    if data.len() < y_size + uv_size {
        return vec![0u8; w * h * 4];
    }

    let y_plane = &data[..y_size];
    let vu_plane = &data[y_size..y_size + uv_size];

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col];
            let uv_row = row / 2;
            let uv_col = col / 2;
            let v = vu_plane[uv_row * uv_stride + uv_col * 2];
            let u = vu_plane[uv_row * uv_stride + uv_col * 2 + 1];
            let (r, g, b) = conv(y, u, v);
            let off = (row * w + col) * 4;
            argb[off] = b;
            argb[off + 1] = g;
            argb[off + 2] = r;
            argb[off + 3] = 0xFF;
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
            argb[off] = b;
            argb[off + 1] = g;
            argb[off + 2] = r;
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
            argb[dst_off] = b; // B
            argb[dst_off + 1] = g; // G
            argb[dst_off + 2] = r; // R
            argb[dst_off + 3] = 0xFF; // A
        }
    }
    argb
}

/// Convert packed BGRA32 (RV32) data to ARGB32.
///
/// Layout: 4 bytes per pixel [B, G, R, A] in row-major order.
/// The input is BGRA which maps directly to our ARGB32 framebuffer format (BGRA in memory).
fn convert_rv32_to_argb(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = w * 4;

    if data.len() < src_stride * h {
        return vec![0u8; w * h * 4];
    }

    let mut argb = vec![0u8; w * h * 4];
    for row in 0..h {
        let src_row = row * src_stride;
        let dst_row = row * w * 4;
        argb[dst_row..dst_row + w * 4].copy_from_slice(&data[src_row..src_row + w * 4]);
    }
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
            argb[off] = y; // B
            argb[off + 1] = y; // G
            argb[off + 2] = y; // R
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
        FOURCC_I420 => Some(convert_i420_to_argb(yuv, width, height, conv)),
        FOURCC_YV12 => Some(convert_yv12_to_argb(yuv, width, height, conv)),
        FOURCC_YUY2 => Some(convert_yuy2_to_argb(yuv, width, height, conv)),
        FOURCC_UYVY => Some(convert_uyvy_to_argb(yuv, width, height, conv)),
        FOURCC_NV12 => Some(convert_nv12_to_argb(yuv, width, height, conv)),
        FOURCC_NV21 => Some(convert_nv21_to_argb(yuv, width, height, conv)),
        FOURCC_YV16 => Some(convert_yv16_to_argb(yuv, width, height, conv)),
        FOURCC_RGB3 => Some(convert_rgb3_to_argb(yuv, width, height)),
        FOURCC_RV32 => Some(convert_rv32_to_argb(yuv, width, height)),
        FOURCC_Y800 => Some(convert_grey_to_argb(yuv, width, height)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ARGB → YUV conversion (for GetImage — reverse direction)
// ---------------------------------------------------------------------------

/// Convert a single ARGB pixel to YUV (BT.601) using integer math.
#[inline(always)]
fn rgb_to_yuv_bt601(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let ri = r as i32;
    let gi = g as i32;
    let bi = b as i32;
    let y = ((66 * ri + 129 * gi + 25 * bi + 128) >> 8) + 16;
    let u = ((-38 * ri - 74 * gi + 112 * bi + 128) >> 8) + 128;
    let v = ((112 * ri - 94 * gi - 18 * bi + 128) >> 8) + 128;
    (clamp_u8(y), clamp_u8(u), clamp_u8(v))
}

/// Convert ARGB32 framebuffer data to the requested FOURCC format.
/// Returns None if the format is not supported for export.
#[allow(dead_code)]
fn convert_argb_to_format(argb: &[u8], width: u32, height: u32, fourcc: u32) -> Option<Vec<u8>> {
    match fourcc {
        FOURCC_I420 => Some(convert_argb_to_i420(argb, width, height)),
        FOURCC_YV12 => Some(convert_argb_to_yv12(argb, width, height)),
        FOURCC_YUY2 => Some(convert_argb_to_yuy2(argb, width, height)),
        FOURCC_UYVY => Some(convert_argb_to_uyvy(argb, width, height)),
        FOURCC_NV12 => Some(convert_argb_to_nv12(argb, width, height)),
        FOURCC_NV21 => Some(convert_argb_to_nv21(argb, width, height)),
        FOURCC_RGB3 => Some(convert_argb_to_rgb3(argb, width, height)),
        FOURCC_RV32 => Some(convert_argb_to_rv32(argb, width, height)),
        FOURCC_Y800 => Some(convert_argb_to_grey(argb, width, height)),
        FOURCC_YV16 => Some(convert_argb_to_yv16(argb, width, height)),
        _ => None,
    }
}

fn convert_argb_to_i420(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (size, _, _) = query_image_attributes(FOURCC_I420, width, height);
    let mut out = vec![0u8; size as usize];
    let y_stride = width;
    let uv_stride = width.div_ceil(2);
    let y_size = y_stride * height;
    let uv_size = uv_stride * height.div_ceil(2);
    let u_off = y_size as usize;
    let v_off = u_off + uv_size as usize;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() {
                break;
            }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if row % 2 == 0 && col % 2 == 0 {
                let uv_row = row / 2;
                let uv_col = col / 2;
                out[u_off + (uv_row * uv_stride + uv_col) as usize] = u;
                out[v_off + (uv_row * uv_stride + uv_col) as usize] = v;
            }
        }
    }
    out
}

fn convert_argb_to_yv12(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (size, _, _) = query_image_attributes(FOURCC_YV12, width, height);
    let mut out = vec![0u8; size as usize];
    let y_stride = width;
    let uv_stride = width.div_ceil(2);
    let y_size = y_stride * height;
    let uv_size = uv_stride * height.div_ceil(2);
    let v_off = y_size as usize; // YV12: V before U
    let u_off = v_off + uv_size as usize;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() {
                break;
            }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if row % 2 == 0 && col % 2 == 0 {
                let uv_row = row / 2;
                let uv_col = col / 2;
                out[u_off + (uv_row * uv_stride + uv_col) as usize] = u;
                out[v_off + (uv_row * uv_stride + uv_col) as usize] = v;
            }
        }
    }
    out
}

fn convert_argb_to_yuy2(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pitch = width * 2;
    let mut out = vec![0u8; (pitch * height) as usize];
    for row in 0..height {
        for col in (0..width).step_by(2) {
            let px0 = ((row * width + col) * 4) as usize;
            let px1 = if col + 1 < width {
                ((row * width + col + 1) * 4) as usize
            } else {
                px0
            };
            let (b0, g0, r0) = (
                argb.get(px0).copied().unwrap_or(0),
                argb.get(px0 + 1).copied().unwrap_or(0),
                argb.get(px0 + 2).copied().unwrap_or(0),
            );
            let (b1, g1, r1) = (
                argb.get(px1).copied().unwrap_or(0),
                argb.get(px1 + 1).copied().unwrap_or(0),
                argb.get(px1 + 2).copied().unwrap_or(0),
            );
            let (y0, u0, v0) = rgb_to_yuv_bt601(r0, g0, b0);
            let (y1, u1, v1) = rgb_to_yuv_bt601(r1, g1, b1);
            let u = ((u0 as u16 + u1 as u16) / 2) as u8;
            let v = ((v0 as u16 + v1 as u16) / 2) as u8;
            let off = (row * pitch + col * 2) as usize;
            if off + 3 < out.len() {
                out[off] = y0;
                out[off + 1] = u;
                out[off + 2] = y1;
                out[off + 3] = v;
            }
        }
    }
    out
}

fn convert_argb_to_uyvy(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pitch = width * 2;
    let mut out = vec![0u8; (pitch * height) as usize];
    for row in 0..height {
        for col in (0..width).step_by(2) {
            let px0 = ((row * width + col) * 4) as usize;
            let px1 = if col + 1 < width {
                ((row * width + col + 1) * 4) as usize
            } else {
                px0
            };
            let (b0, g0, r0) = (
                argb.get(px0).copied().unwrap_or(0),
                argb.get(px0 + 1).copied().unwrap_or(0),
                argb.get(px0 + 2).copied().unwrap_or(0),
            );
            let (b1, g1, r1) = (
                argb.get(px1).copied().unwrap_or(0),
                argb.get(px1 + 1).copied().unwrap_or(0),
                argb.get(px1 + 2).copied().unwrap_or(0),
            );
            let (y0, u0, v0) = rgb_to_yuv_bt601(r0, g0, b0);
            let (y1, u1, v1) = rgb_to_yuv_bt601(r1, g1, b1);
            let u = ((u0 as u16 + u1 as u16) / 2) as u8;
            let v = ((v0 as u16 + v1 as u16) / 2) as u8;
            let off = (row * pitch + col * 2) as usize;
            if off + 3 < out.len() {
                out[off] = u;
                out[off + 1] = y0;
                out[off + 2] = v;
                out[off + 3] = y1;
            }
        }
    }
    out
}

fn convert_argb_to_nv12(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (size, _, _) = query_image_attributes(FOURCC_NV12, width, height);
    let mut out = vec![0u8; size as usize];
    let y_stride = width;
    let y_size = y_stride * height;
    let uv_stride = width.div_ceil(2) * 2;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() {
                break;
            }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if row % 2 == 0 && col % 2 == 0 {
                let uv_row = row / 2;
                let uv_col = col / 2;
                let uv_off = y_size as usize + (uv_row * uv_stride + uv_col * 2) as usize;
                if uv_off + 1 < out.len() {
                    out[uv_off] = u;
                    out[uv_off + 1] = v;
                }
            }
        }
    }
    out
}

fn convert_argb_to_nv21(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (size, _, _) = query_image_attributes(FOURCC_NV21, width, height);
    let mut out = vec![0u8; size as usize];
    let y_stride = width;
    let y_size = y_stride * height;
    let uv_stride = width.div_ceil(2) * 2;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() {
                break;
            }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if row % 2 == 0 && col % 2 == 0 {
                let uv_row = row / 2;
                let uv_col = col / 2;
                let uv_off = y_size as usize + (uv_row * uv_stride + uv_col * 2) as usize;
                if uv_off + 1 < out.len() {
                    out[uv_off] = v; // NV21: V before U
                    out[uv_off + 1] = u;
                }
            }
        }
    }
    out
}

fn convert_argb_to_yv16(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (size, _, _) = query_image_attributes(FOURCC_YV16, width, height);
    let mut out = vec![0u8; size as usize];
    let y_stride = width;
    let uv_stride = width.div_ceil(2);
    let y_size = y_stride * height;
    let uv_size = uv_stride * height;
    let v_off = y_size as usize;
    let u_off = v_off + uv_size as usize;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() {
                break;
            }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if col % 2 == 0 {
                let uv_col = col / 2;
                out[u_off + (row * uv_stride + uv_col) as usize] = u;
                out[v_off + (row * uv_stride + uv_col) as usize] = v;
            }
        }
    }
    out
}

fn convert_argb_to_rgb3(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 3) as usize);
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            let (b, g, r) = (
                argb.get(px).copied().unwrap_or(0),
                argb.get(px + 1).copied().unwrap_or(0),
                argb.get(px + 2).copied().unwrap_or(0),
            );
            out.push(r);
            out.push(g);
            out.push(b);
        }
    }
    out
}

fn convert_argb_to_rv32(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    // BGRA32 — same layout as our framebuffer
    let size = (width * height * 4) as usize;
    if argb.len() >= size {
        argb[..size].to_vec()
    } else {
        let mut out = argb.to_vec();
        out.resize(size, 0);
        out
    }
}

fn convert_argb_to_grey(argb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height) as usize);
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            let (b, g, r) = (
                argb.get(px).copied().unwrap_or(0),
                argb.get(px + 1).copied().unwrap_or(0),
                argb.get(px + 2).copied().unwrap_or(0),
            );
            // Luma: Y = 0.299*R + 0.587*G + 0.114*B
            let y = ((77 * r as u32 + 150 * g as u32 + 29 * b as u32 + 128) >> 8) as u8;
            out.push(y);
        }
    }
    out
}

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
        crate::xserver::core::build_error(code, seq, bad_value, 156, minor as u16)
    };
    match minor {
        5 => {
            // PutVideo — not supported (software adaptor has no video capture)
            // Per XVideo spec §4.3: return BadMatch for unsupported port operations.
            let port = PutVideoRequest::try_parse_request(request_header(data), &data[4..])
                .map(|r| r.port)
                .unwrap_or(0);
            debug!("XVideo PutVideo: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        6 => {
            // PutStill — not supported (software adaptor has no video capture)
            // Per XVideo spec §4.4: return BadMatch for unsupported port operations.
            let port = PutStillRequest::try_parse_request(request_header(data), &data[4..])
                .map(|r| r.port)
                .unwrap_or(0);
            debug!("XVideo PutStill: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        7 => {
            // GetVideo — not supported (software adaptor has no video capture output)
            // Per XVideo spec §4.5: return BadMatch for unsupported port operations.
            let port = GetVideoRequest::try_parse_request(request_header(data), &data[4..])
                .map(|r| r.port)
                .unwrap_or(0);
            debug!("XVideo GetVideo: port={port} — returning BadMatch (capture not supported)");
            xv_err(crate::xserver::core::MATCH_ERROR, port)
        }
        8 => {
            // GetStill — capture current pixels from a drawable region
            if data.len() < 32 {
                return Vec::new();
            }
            let req = parse_or_void!(GetStillRequest, data);
            let port = req.port;
            let drawable = req.drawable;
            let gc_id = req.gc;
            let vid_x = req.vid_x;
            let vid_y = req.vid_y;
            let vid_w = req.vid_w;
            let vid_h = req.vid_h;
            let drw_x = req.drw_x;
            let drw_y = req.drw_y;
            let drw_w = req.drw_w;
            let drw_h = req.drw_h;

            debug!(
                "XVideo GetStill: port={port} drawable={drawable:#x} gc={gc_id:#x} \
                    vid=({vid_x},{vid_y} {vid_w}x{vid_h}) drw=({drw_x},{drw_y} {drw_w}x{drw_h})"
            );

            // Validate the drawable exists (window or pixmap).
            if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
                return xv_err(crate::xserver::core::DRAWABLE_ERROR, drawable);
            }

            // Validate the GC exists.
            if !state.gcs.contains_key(&gc_id) {
                return xv_err(crate::xserver::core::G_CONTEXT_ERROR, gc_id);
            }

            // Extract ARGB32 pixels from the drawable's framebuffer at the
            // draw region (drw_x, drw_y, drw_w, drw_h). The draw region
            // specifies which part of the drawable to capture from.
            let resolved = state.resolve_drawable(drawable);
            let pixels = if let Some(fb) = state.get_framebuffer_mut(resolved) {
                fb.extract_pixels(drw_x, drw_y, drw_w, drw_h)
            } else {
                // Drawable is valid but has no backing framebuffer yet;
                // return zeroed (transparent black) pixels.
                vec![0u8; drw_w as usize * drw_h as usize * 4]
            };

            // Store the captured frame in the port's state for subsequent
            // PutStill / PutVideo retrieval.
            let port_state = state.xv_ports.entry(port).or_default();
            port_state.captured_frame = Some(CapturedFrame {
                width: drw_w,
                height: drw_h,
                data: pixels,
            });

            // GetStill is a void request — no reply is sent.
            Vec::new()
        }
        12 => {
            // XvStopVideo
            if data.len() >= 8 {
                if let Ok(req) = StopVideoRequest::try_parse_request(request_header(data), &data[4..]) {
                    let port = req.port;
                    debug!("XVideo StopVideo: port={port}");
                }
            }
            Vec::new()
        }
        16 => {
            // XvListImageFormats
            // Report all supported formats
            let num_formats: u32 = 10;
            let extra_bytes = (num_formats * 128) as usize;
            let mut reply = ReplyBuf::with_extra(seq, extra_bytes, state.msb_first)
                .set_u32(8, num_formats);

            // Helper to fill an ImageFormatInfo at a given offset
            // format_type: 1 = XvYUV, 0 = XvRGB
            let fill_format = |reply: &mut ReplyBuf,
                               idx: usize,
                               fourcc: u32,
                               name: &[u8; 4],
                               bpp: u8,
                               format_type: u32,
                               num_planes: u32,
                               horz_u: u32,
                               vert_u: u32,
                               horz_v: u32,
                               vert_v: u32| {
                let off = 32 + idx * 128;
                let buf = reply.buf_mut();
                state.write_u32(&mut buf[off..], 0, fourcc);
                state.write_u32(&mut buf[off..], 4, format_type);
                buf[off + 8] = 0; // byte_order = LSBFirst
                buf[off + 16] = bpp; // bits_per_pixel
                buf[off + 9..off + 9 + 4].copy_from_slice(name);
                state.write_u32(&mut buf[off..], 20, num_planes);
                buf[off + 24] = 0; // depth = 0
                state.write_u32(&mut buf[off..], 40, 1); // horz_y_period
                state.write_u32(&mut buf[off..], 44, 1); // vert_y_period
                state.write_u32(&mut buf[off..], 48, horz_u);
                state.write_u32(&mut buf[off..], 52, vert_u);
                state.write_u32(&mut buf[off..], 56, horz_v);
                state.write_u32(&mut buf[off..], 60, vert_v);
            };

            // Format 0: YUY2 (packed 4:2:2)
            fill_format(&mut reply, 0, FOURCC_YUY2, b"YUY2", 16, 1, 1, 2, 1, 2, 1);
            // Format 1: UYVY (packed 4:2:2 alternate)
            fill_format(&mut reply, 1, FOURCC_UYVY, b"UYVY", 16, 1, 1, 2, 1, 2, 1);
            // Format 2: I420 (planar 4:2:0)
            fill_format(&mut reply, 2, FOURCC_I420, b"I420", 12, 1, 3, 2, 2, 2, 2);
            // Format 3: YV12 (planar 4:2:0 V-first)
            fill_format(&mut reply, 3, FOURCC_YV12, b"YV12", 12, 1, 3, 2, 2, 2, 2);
            // Format 4: NV12 (semi-planar 4:2:0, interleaved UV)
            fill_format(&mut reply, 4, FOURCC_NV12, b"NV12", 12, 1, 2, 2, 2, 2, 2);
            // Format 5: NV21 (semi-planar 4:2:0, interleaved VU)
            fill_format(&mut reply, 5, FOURCC_NV21, b"NV21", 12, 1, 2, 2, 2, 2, 2);
            // Format 6: YV16 (planar 4:2:2, V-first)
            fill_format(&mut reply, 6, FOURCC_YV16, b"YV16", 16, 1, 3, 2, 1, 2, 1);
            // Format 7: RGB3 (packed RGB24)
            fill_format(&mut reply, 7, FOURCC_RGB3, b"RGB3", 24, 0, 1, 0, 0, 0, 0);
            // Format 8: RV32 (packed BGRA32)
            fill_format(&mut reply, 8, FOURCC_RV32, b"RV32", 32, 0, 1, 0, 0, 0, 0);
            // Format 9: Y800 (8-bit grayscale)
            fill_format(&mut reply, 9, FOURCC_Y800, b"Y800", 8, 1, 1, 0, 0, 0, 0);

            reply.build()
        }
        17 => {
            // XvQueryImageAttributes
            if data.len() >= 16 {
                let req = parse_or_void!(XvQueryImageAttributesRequest, data);
                let fourcc = req.id;
                let width = req.width as u32;
                let height = req.height as u32;

                let (data_size, pitches, offsets) = query_image_attributes(fourcc, width, height);

                // Determine number of planes for this format
                let num_planes = match fourcc {
                    FOURCC_I420 | FOURCC_YV12 | FOURCC_YV16 => 3u32,
                    FOURCC_NV12 | FOURCC_NV21 => 2u32,
                    FOURCC_YUY2 | FOURCC_UYVY | FOURCC_RGB3 | FOURCC_RV32 | FOURCC_Y800 => 1u32,
                    _ => 1u32,
                };

                // Reply: 32 header + num_planes * 4 (pitches) + num_planes * 4 (offsets)
                let extra = (num_planes * 4 * 2) as usize;
                let mut reply = ReplyBuf::with_extra(seq, extra, state.msb_first)
                    .set_u32(8, num_planes)
                    .set_u32(12, data_size)
                    .set_u16(16, width as u16)
                    .set_u16(18, height as u16);

                // Write pitches
                for (i, &pitch) in pitches.iter().take(num_planes as usize).enumerate() {
                    reply = reply.set_u32(32 + i * 4, pitch);
                }
                // Write offsets
                let off_base = 32 + num_planes as usize * 4;
                for (i, &offset) in offsets.iter().take(num_planes as usize).enumerate() {
                    reply = reply.set_u32(off_base + i * 4, offset);
                }

                reply.build()
            } else {
                Vec::new()
            }
        }
        18 => {
            // XvPutImage
            if data.len() >= 40 {
                let req = parse_or_void!(PutImageRequest, data);
                let port = req.port;
                let drawable = req.drawable;
                let fourcc = req.id;
                let src_w = req.src_w;
                let src_h = req.src_h;
                let drw_x = req.drw_x;
                let drw_y = req.drw_y;
                let drw_w = req.drw_w;
                let drw_h = req.drw_h;
                let img_w = req.width;
                let img_h = req.height;

                let yuv_data = &*req.data;

                // Use image dimensions for conversion, then scale to drw dimensions
                xv_put_image_impl(
                    state,
                    drawable,
                    port,
                    fourcc,
                    yuv_data,
                    if src_w > 0 { src_w } else { img_w },
                    if src_h > 0 { src_h } else { img_h },
                    drw_x,
                    drw_y,
                    drw_w,
                    drw_h,
                );
            }
            Vec::new()
        }
        19 => {
            // XvShmPutImage
            if data.len() >= 49 {
                let req = parse_or_void!(ShmPutImageRequest, data);
                let port = req.port;
                let drawable = req.drawable;
                let shmseg = req.shmseg;
                let fourcc = req.id;
                let offset = req.offset as usize;
                let src_w = req.src_w;
                let src_h = req.src_h;
                let drw_x = req.drw_x;
                let drw_y = req.drw_y;
                let drw_w = req.drw_w;
                let drw_h = req.drw_h;
                let img_w = req.width;
                let img_h = req.height;
                let send_event = req.send_event != 0;

                debug!(
                    "XVideo ShmPutImage: port={port} drawable={drawable:#x} shmseg={shmseg} \
                     fourcc={fourcc:#010x} offset={offset} src={src_w}x{src_h} \
                     drw=({drw_x},{drw_y} {drw_w}x{drw_h}) img={img_w}x{img_h}"
                );

                // Read YUV data from shared memory segment
                let w = if src_w > 0 { src_w } else { img_w };
                let h = if src_h > 0 { src_h } else { img_h };
                let (data_size, _, _) = query_image_attributes(fourcc, w as u32, h as u32);

                if let Some(seg) = state.shm_segments.get(&shmseg) {
                    if offset + data_size as usize <= seg.size {
                        let yuv_data = unsafe {
                            std::slice::from_raw_parts(seg.addr.add(offset), data_size as usize)
                        };

                        xv_put_image_impl(
                            state, drawable, port, fourcc, yuv_data, w, h, drw_x, drw_y, drw_w,
                            drw_h,
                        );
                    } else {
                        debug!("XVideo ShmPutImage: out of bounds (offset={offset} + size={data_size} > seg.size={})", seg.size);
                    }
                } else {
                    debug!("XVideo ShmPutImage: unknown shmseg={shmseg}");
                }

                // If send_event, return a ShmCompletion event
                if send_event {
                    use x11rb_protocol::protocol::shm::CompletionEvent;
                    return crate::xserver::event::serialize_event(
                        &CompletionEvent {
                            response_type: 65,
                            sequence: seq,
                            drawable,
                            minor_event: 0,
                            major_event: 0,
                            shmseg,
                            offset: offset as u32,
                        },
                        state.msb_first,
                    );
                }
            }
            Vec::new()
        }
        20 => {
            // XvGetStill — not meaningful for software rendering, return void
            Vec::new()
        }
        _ => {
            debug!("XVideo image: unhandled minor opcode {minor}");
            xv_err(crate::xserver::core::REQUEST_ERROR, minor as u32)
        }
    }
}
