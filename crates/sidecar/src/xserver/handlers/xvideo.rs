//\! XVideo (Xv) extension handler.

use tracing::debug;

use super::super::client::ClientState;

/// XVideo (Xv) (opcode 156)
///
/// Software-only video adaptor supporting basic YUV/RGB overlay rendering.
/// Port 100 is the single available port on the software adaptor.
const XV_PORT_BASE: u32 = 100;

/// Number of XVideo ports exposed by our software adaptor.
const XV_NUM_PORTS: u32 = 1;

/// FOURCC identifiers for supported image formats.
const FOURCC_YUY2: u32 = 0x32595559; // 'YUY2'
const FOURCC_I420: u32 = 0x30323449; // 'I420'
const FOURCC_YV12: u32 = 0x32315659; // 'YV12'
const FOURCC_UYVY: u32 = 0x59565955; // 'UYVY'
const FOURCC_NV12: u32 = 0x3231564E; // 'NV12'
const FOURCC_NV21: u32 = 0x3132564E; // 'NV21'
const FOURCC_YV16: u32 = 0x36315659; // 'YV16'
const FOURCC_RGB3: u32 = 0x33424752; // 'RGB3' (packed RGB24)
const FOURCC_RV32: u32 = 0x32335652; // 'RV32' (packed RGBA32/BGRA32)
const FOURCC_Y800: u32 = 0x30303859; // 'Y800' / 'GREY' (8-bit grayscale)

/// A frame captured by GetStill: raw ARGB32 pixels plus dimensions.
#[derive(Clone, Debug)]
#[allow(dead_code)]
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

// ---------------------------------------------------------------------------
// YUV → RGB conversion (integer math, no floating point)
// ---------------------------------------------------------------------------

/// Clamp an i32 to the 0..255 range and return as u8.
#[inline(always)]
fn clamp_u8(v: i32) -> u8 {
    if v < 0 { 0 } else if v > 255 { 255 } else { v as u8 }
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
    if colorspace == 1 { yuv_to_rgb_bt709 } else { yuv_to_rgb_bt601 }
}

/// Convert I420 (planar YUV 4:2:0) data to ARGB32.
///
/// Layout: Y plane (width * height), U plane (width/2 * height/2), V plane (width/2 * height/2).
fn convert_i420_to_argb(
    yuv: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
            argb[off] = b;     // B
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
fn convert_yv12_to_argb(
    yuv: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_yuy2_to_argb(
    yuv: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_uyvy_to_argb(
    yuv: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_nv12_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_nv21_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_yv16_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
    conv: YuvToRgb,
) -> Vec<u8> {
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
fn convert_rgb3_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
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
            argb[dst_off] = b;     // B
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
fn convert_rv32_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
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
fn convert_grey_to_argb(
    data: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
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
            argb[off] = y;     // B
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
fn convert_argb_to_format(
    argb: &[u8],
    width: u32,
    height: u32,
    fourcc: u32,
) -> Option<Vec<u8>> {
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
            if px + 3 >= argb.len() { break; }
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
    let v_off = y_size as usize;           // YV12: V before U
    let u_off = v_off + uv_size as usize;
    for row in 0..height {
        for col in 0..width {
            let px = ((row * width + col) * 4) as usize;
            if px + 3 >= argb.len() { break; }
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
            let px1 = if col + 1 < width { ((row * width + col + 1) * 4) as usize } else { px0 };
            let (b0, g0, r0) = (argb.get(px0).copied().unwrap_or(0), argb.get(px0+1).copied().unwrap_or(0), argb.get(px0+2).copied().unwrap_or(0));
            let (b1, g1, r1) = (argb.get(px1).copied().unwrap_or(0), argb.get(px1+1).copied().unwrap_or(0), argb.get(px1+2).copied().unwrap_or(0));
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
            let px1 = if col + 1 < width { ((row * width + col + 1) * 4) as usize } else { px0 };
            let (b0, g0, r0) = (argb.get(px0).copied().unwrap_or(0), argb.get(px0+1).copied().unwrap_or(0), argb.get(px0+2).copied().unwrap_or(0));
            let (b1, g1, r1) = (argb.get(px1).copied().unwrap_or(0), argb.get(px1+1).copied().unwrap_or(0), argb.get(px1+2).copied().unwrap_or(0));
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
            if px + 3 >= argb.len() { break; }
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
            if px + 3 >= argb.len() { break; }
            let (b, g, r) = (argb[px], argb[px + 1], argb[px + 2]);
            let (y, u, v) = rgb_to_yuv_bt601(r, g, b);
            out[(row * y_stride + col) as usize] = y;
            if row % 2 == 0 && col % 2 == 0 {
                let uv_row = row / 2;
                let uv_col = col / 2;
                let uv_off = y_size as usize + (uv_row * uv_stride + uv_col * 2) as usize;
                if uv_off + 1 < out.len() {
                    out[uv_off] = v;     // NV21: V before U
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
            if px + 3 >= argb.len() { break; }
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
            let (b, g, r) = (argb.get(px).copied().unwrap_or(0), argb.get(px+1).copied().unwrap_or(0), argb.get(px+2).copied().unwrap_or(0));
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
            let (b, g, r) = (argb.get(px).copied().unwrap_or(0), argb.get(px+1).copied().unwrap_or(0), argb.get(px+2).copied().unwrap_or(0));
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
            (total, [y_pitch, uv_pitch, uv_pitch], [0, y_size, y_size + uv_size])
        }
        FOURCC_YV12 => {
            let y_pitch = width;
            let uv_pitch = width.div_ceil(2);
            let y_size = y_pitch * height;
            let uv_size = uv_pitch * height.div_ceil(2);
            let total = y_size + 2 * uv_size;
            // YV12: V plane before U plane
            (total, [y_pitch, uv_pitch, uv_pitch], [0, y_size, y_size + uv_size])
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
            (total, [y_pitch, uv_pitch, uv_pitch], [0, y_size, y_size + uv_size])
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
fn scale_argb_nearest(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
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
    let scaled = scale_argb_nearest(&argb, src_w as u32, src_h as u32, dst_w as u32, dst_h as u32);

    xv_blit_to_drawable(state, drawable, &scaled, dst_x, dst_y, dst_w, dst_h);

    debug!(
        "XVideo PutImage: port={port} fourcc={fourcc:#010x} src={src_w}x{src_h} \
         dst=({dst_x},{dst_y} {dst_w}x{dst_h}) drawable={drawable:#x}"
    );
}

/// Atom name constants for XVideo port attributes.
const XV_ATTR_BRIGHTNESS: &str = "XV_BRIGHTNESS";
const XV_ATTR_CONTRAST: &str = "XV_CONTRAST";
const XV_ATTR_SATURATION: &str = "XV_SATURATION";
const XV_ATTR_HUE: &str = "XV_HUE";
const XV_ATTR_COLORSPACE: &str = "XV_COLORSPACE";

pub(crate) fn handle_xvideo_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => { // XvQueryExtension
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 2); // major
            state.write_u16(&mut reply, 10, 2); // minor
            reply.to_vec()
        }
        1 => { // XvQueryAdaptors
            // Return one software adaptor with a single port
            let adaptor_name = b"x11-web Software Video Adaptor";
            let name_len = adaptor_name.len();
            let name_padded = (name_len + 3) & !3;

            // AdaptorInfo structure:
            // base_id(4) + name_size(2) + num_ports(2) + num_formats(2) + type(1) + pad(1) + name(padded)
            // + format list: visual(4) + depth(1) + pad(3) per format
            let format_entry_size = 8; // visual(4) + depth(1) + pad(3)
            let adaptor_size = 12 + name_padded + format_entry_size; // 1 format
            let extra_words = (adaptor_size / 4) as u32;

            let mut reply = vec![0u8; 32 + adaptor_size];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, extra_words);
            state.write_u16(&mut reply, 8, 1); // num_adaptors

            // Adaptor info
            let off = 32;
            state.write_u32(&mut reply[off..], 0, XV_PORT_BASE); // base_id
            state.write_u16(&mut reply[off..], 4, name_len as u16); // name_size
            state.write_u16(&mut reply[off..], 6, XV_NUM_PORTS as u16); // num_ports
            state.write_u16(&mut reply[off..], 8, 1); // num_formats
            reply[off + 10] = 0x06; // type = InputMask | ImageMask (video input + image)

            // Name
            reply[off + 12..off + 12 + name_len].copy_from_slice(adaptor_name);

            // Format: visual + depth
            let fmt_off = off + 12 + name_padded;
            state.write_u32(&mut reply[fmt_off..], 0, 0x21); // root visual
            reply[fmt_off + 4] = 24; // depth

            reply
        }
        2 => { // XvQueryEncodings
            // Return one encoding (the screen itself)
            let enc_name = b"XV_IMAGE";
            let name_len = enc_name.len();
            let name_padded = (name_len + 3) & !3;
            let enc_size = 16 + name_padded; // id(4) + name_size(2) + width(2) + height(2) + rate_num(4) + rate_den(4) + name
            let extra_words = (enc_size / 4) as u32;

            let mut reply = vec![0u8; 32 + enc_size];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, extra_words);
            state.write_u16(&mut reply, 8, 1); // num_encodings

            let off = 32;
            state.write_u32(&mut reply[off..], 0, 0); // encoding_id
            state.write_u16(&mut reply[off..], 4, name_len as u16);
            state.write_u16(&mut reply[off..], 6, state.screen_width);
            state.write_u16(&mut reply[off..], 8, state.screen_height);
            // rate = 30/1
            state.write_u32(&mut reply[off..], 12, 30); // numerator
            state.write_u32(&mut reply[off..], 16, 1);  // denominator (skip 10-11 padding)
            reply[off + 20..off + 20 + name_len].copy_from_slice(enc_name);
            reply
        }
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
        5 => { // PutVideo — not supported (software adaptor has no video capture)
            // Per XVideo spec §4.3: return BadMatch for unsupported port operations.
            let port = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            debug!("XVideo PutVideo: port={port} — returning BadMatch (capture not supported)");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_MATCH, seq, port,
                156, minor as u16, state.msb_first,
            )
        }
        6 => { // PutStill — not supported (software adaptor has no video capture)
            // Per XVideo spec §4.4: return BadMatch for unsupported port operations.
            let port = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            debug!("XVideo PutStill: port={port} — returning BadMatch (capture not supported)");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_MATCH, seq, port,
                156, minor as u16, state.msb_first,
            )
        }
        7 => { // GetVideo — not supported (software adaptor has no video capture output)
            // Per XVideo spec §4.5: return BadMatch for unsupported port operations.
            let port = if data.len() >= 8 { state.read_u32(data, 4) } else { 0 };
            debug!("XVideo GetVideo: port={port} — returning BadMatch (capture not supported)");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_MATCH, seq, port,
                156, minor as u16, state.msb_first,
            )
        }
        8 => { // GetStill — capture current pixels from a drawable region
            if data.len() < 32 {
                return Vec::new();
            }
            let port = state.read_u32(data, 4);
            let drawable = state.read_u32(data, 8);
            let gc_id = state.read_u32(data, 12);
            let vid_x = state.read_i16(data, 16);
            let vid_y = state.read_i16(data, 18);
            let vid_w = state.read_u16(data, 20);
            let vid_h = state.read_u16(data, 22);
            let drw_x = state.read_i16(data, 24);
            let drw_y = state.read_i16(data, 26);
            let drw_w = state.read_u16(data, 28);
            let drw_h = state.read_u16(data, 30);

            debug!("XVideo GetStill: port={port} drawable={drawable:#x} gc={gc_id:#x} \
                    vid=({vid_x},{vid_y} {vid_w}x{vid_h}) drw=({drw_x},{drw_y} {drw_w}x{drw_h})");

            // Validate the drawable exists (window or pixmap).
            if !state.windows.contains_key(&drawable) && !state.pixmaps.contains_key(&drawable) {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_DRAWABLE, seq, drawable,
                    156, minor as u16, state.msb_first,
                );
            }

            // Validate the GC exists.
            if !state.gcs.contains_key(&gc_id) {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_GC, seq, gc_id,
                    156, minor as u16, state.msb_first,
                );
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
        12 => { // XvStopVideo
            if data.len() >= 8 {
                let port = state.read_u32(data, 4);
                debug!("XVideo StopVideo: port={port}");
            }
            Vec::new()
        }
        13 => { // SelectVideoNotify — register interest in XvVideoNotify events on a drawable
            // This is a void request. Track the subscription in client state so we can
            // deliver VideoNotify events if/when video operations complete on that drawable.
            if data.len() >= 9 {
                let drawable = state.read_u32(data, 4);
                let on_off = data[8] != 0;
                if on_off {
                    state.xv_video_notify_drawables.insert(drawable);
                } else {
                    state.xv_video_notify_drawables.remove(&drawable);
                }
                debug!("XVideo SelectVideoNotify: drawable={drawable:#x} on={on_off}");
            } else {
                debug!("XVideo SelectVideoNotify: short request (len={}), ignoring", data.len());
            }
            Vec::new()
        }
        14 => { // SelectPortNotify — register interest in XvPortNotify events on a port
            // This is a void request. Track the subscription so we can deliver
            // PortNotify events when port attributes change.
            if data.len() >= 9 {
                let port = state.read_u32(data, 4);
                let on_off = data[8] != 0;
                if on_off {
                    state.xv_port_notify_ports.insert(port);
                } else {
                    state.xv_port_notify_ports.remove(&port);
                }
                debug!("XVideo SelectPortNotify: port={port} on={on_off}");
            } else {
                debug!("XVideo SelectPortNotify: short request (len={}), ignoring", data.len());
            }
            Vec::new()
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
        16 => { // XvListImageFormats
            // Report all supported formats
            let num_formats: u32 = 10;
            let extra_bytes = (num_formats * 128) as usize;
            let extra_words = extra_bytes / 4;
            let mut reply = vec![0u8; 32 + extra_bytes];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, extra_words as u32);
            state.write_u32(&mut reply, 8, num_formats);

            // Helper to fill an ImageFormatInfo at a given offset
            // format_type: 1 = XvYUV, 0 = XvRGB
            let fill_format = |reply: &mut Vec<u8>, idx: usize, fourcc: u32, name: &[u8; 4], bpp: u8,
                               format_type: u32, num_planes: u32,
                               horz_u: u32, vert_u: u32, horz_v: u32, vert_v: u32| {
                let off = 32 + idx * 128;
                state.write_u32(&mut reply[off..], 0, fourcc);
                state.write_u32(&mut reply[off..], 4, format_type);
                reply[off + 8] = 0; // byte_order = LSBFirst
                reply[off + 16] = bpp; // bits_per_pixel
                reply[off + 9..off + 9 + 4].copy_from_slice(name);
                state.write_u32(&mut reply[off..], 20, num_planes);
                reply[off + 24] = 0; // depth = 0
                state.write_u32(&mut reply[off..], 40, 1); // horz_y_period
                state.write_u32(&mut reply[off..], 44, 1); // vert_y_period
                state.write_u32(&mut reply[off..], 48, horz_u);
                state.write_u32(&mut reply[off..], 52, vert_u);
                state.write_u32(&mut reply[off..], 56, horz_v);
                state.write_u32(&mut reply[off..], 60, vert_v);
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

            reply
        }
        17 => { // XvQueryImageAttributes
            if data.len() >= 16 {
                let _port = state.read_u32(data, 4);
                let fourcc = state.read_u32(data, 8);
                let width = state.read_u16(data, 12) as u32;
                let height = state.read_u16(data, 14) as u32;

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
                let extra_words = extra / 4;
                let mut reply = vec![0u8; 32 + extra];
                reply[0] = 1;
                state.write_u16(&mut reply, 2, seq);
                state.write_u32(&mut reply, 4, extra_words as u32);
                state.write_u32(&mut reply, 8, num_planes);
                state.write_u32(&mut reply, 12, data_size);
                state.write_u16(&mut reply, 16, width as u16);
                state.write_u16(&mut reply, 18, height as u16);

                // Write pitches
                for (i, &pitch) in pitches.iter().take(num_planes as usize).enumerate() {
                    state.write_u32(&mut reply, 32 + i * 4, pitch);
                }
                // Write offsets
                let off_base = 32 + num_planes as usize * 4;
                for (i, &offset) in offsets.iter().take(num_planes as usize).enumerate() {
                    state.write_u32(&mut reply, off_base + i * 4, offset);
                }

                reply
            } else {
                Vec::new()
            }
        }
        18 => { // XvPutImage
            // XvPutImage request layout:
            // [0]: major opcode
            // [1]: minor opcode (18)
            // [2..4]: request length
            // [4..8]: port
            // [8..12]: drawable
            // [12..16]: gc
            // [16..20]: id (FOURCC)
            // [20..22]: src_x
            // [22..24]: src_y
            // [24..26]: src_w
            // [26..28]: src_h
            // [28..30]: drw_x
            // [30..32]: drw_y
            // [32..34]: drw_w
            // [34..36]: drw_h
            // [36..38]: width (image width)
            // [38..40]: height (image height)
            // [40..]: image data
            if data.len() >= 40 {
                let port = state.read_u32(data, 4);
                let drawable = state.read_u32(data, 8);
                let _gc = state.read_u32(data, 12);
                let fourcc = state.read_u32(data, 16);
                let _src_x = state.read_i16(data, 20);
                let _src_y = state.read_i16(data, 22);
                let src_w = state.read_u16(data, 24);
                let src_h = state.read_u16(data, 26);
                let drw_x = state.read_i16(data, 28);
                let drw_y = state.read_i16(data, 30);
                let drw_w = state.read_u16(data, 32);
                let drw_h = state.read_u16(data, 34);
                let img_w = state.read_u16(data, 36);
                let img_h = state.read_u16(data, 38);

                let yuv_data = &data[40..];

                // Use image dimensions for conversion, then scale to drw dimensions
                xv_put_image_impl(
                    state, drawable, port, fourcc,
                    yuv_data,
                    if src_w > 0 { src_w } else { img_w },
                    if src_h > 0 { src_h } else { img_h },
                    drw_x, drw_y, drw_w, drw_h,
                );
            }
            Vec::new()
        }
        19 => { // XvShmPutImage
            // XvShmPutImage request layout:
            // [4..8]: port
            // [8..12]: drawable
            // [12..16]: gc
            // [16..20]: shmseg
            // [20..24]: id (FOURCC)
            // [24..28]: offset
            // [28..30]: src_x
            // [30..32]: src_y
            // [32..34]: src_w
            // [34..36]: src_h
            // [36..38]: drw_x
            // [38..40]: drw_y
            // [40..42]: drw_w
            // [42..44]: drw_h
            // [44..46]: width (image width)
            // [46..48]: height (image height)
            // [48]: send_event
            if data.len() >= 49 {
                let port = state.read_u32(data, 4);
                let drawable = state.read_u32(data, 8);
                let _gc = state.read_u32(data, 12);
                let shmseg = state.read_u32(data, 16);
                let fourcc = state.read_u32(data, 20);
                let offset = state.read_u32(data, 24) as usize;
                let _src_x = state.read_i16(data, 28);
                let _src_y = state.read_i16(data, 30);
                let src_w = state.read_u16(data, 32);
                let src_h = state.read_u16(data, 34);
                let drw_x = state.read_i16(data, 36);
                let drw_y = state.read_i16(data, 38);
                let drw_w = state.read_u16(data, 40);
                let drw_h = state.read_u16(data, 42);
                let img_w = state.read_u16(data, 44);
                let img_h = state.read_u16(data, 46);
                let send_event = data[48] != 0;

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
                            state, drawable, port, fourcc,
                            yuv_data,
                            w, h,
                            drw_x, drw_y, drw_w, drw_h,
                        );
                    } else {
                        debug!("XVideo ShmPutImage: out of bounds (offset={offset} + size={data_size} > seg.size={})", seg.size);
                    }
                } else {
                    debug!("XVideo ShmPutImage: unknown shmseg={shmseg}");
                }

                // If send_event, return a ShmCompletion event
                if send_event {
                    let mut event = [0u8; 32];
                    event[0] = 65; // ShmCompletion event type
                    state.write_u16(&mut event, 2, seq);
                    state.write_u32(&mut event, 4, drawable);
                    state.write_u32(&mut event, 8, shmseg);
                    state.write_u32(&mut event, 16, offset as u32);
                    return event.to_vec();
                }
            }
            Vec::new()
        }
        20 => { // XvGetStill — not meaningful for software rendering, return void
            Vec::new()
        }
        _ => {
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, minor as u32,
                156, minor as u16, state.msb_first,
            )
        }
    }
}
