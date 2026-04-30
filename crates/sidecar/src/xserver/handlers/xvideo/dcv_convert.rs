//! Thin wrappers around `dcv-color-primitives` for YUV->RGBA conversion.
//!
//! dcv covers I420 and NV12 directly. YV12 (V/U plane swap) and NV21 (interleaved
//! VU) are handled by reordering the source planes before the call.
//!
//! All converters output RGBA in memory layout `[R, G, B, A]`, matching the
//! framebuffer's storage with full alpha.

use dcv_color_primitives::{convert_image, ColorSpace, ImageFormat, PixelFormat};

/// Pick BT.601 (SD) or BT.709 (HD) based on the XV port's colorspace attribute.
/// `colorspace == 1` means BT.709 in the XV port state.
fn pick_color_space(colorspace: i32) -> ColorSpace {
    if colorspace == 1 {
        ColorSpace::Bt709
    } else {
        ColorSpace::Bt601
    }
}

fn empty_rgba(width: u32, height: u32) -> Vec<u8> {
    vec![0u8; (width as usize) * (height as usize) * 4]
}

/// Set alpha channel to 0xFF (dcv writes alpha=0).
fn fill_alpha(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

/// Build the dcv source/dst formats and run the conversion. Returns RGBA on
/// success, an empty buffer on any error (matching the previous fallback).
fn convert_planar(
    src_format: PixelFormat,
    width: u32,
    height: u32,
    colorspace: i32,
    src_planes: &[&[u8]],
) -> Vec<u8> {
    let mut dst = empty_rgba(width, height);
    let src_fmt = ImageFormat {
        pixel_format: src_format,
        color_space: pick_color_space(colorspace),
        num_planes: src_planes.len() as u32,
    };
    let dst_fmt = ImageFormat {
        pixel_format: PixelFormat::Rgba,
        color_space: ColorSpace::Rgb,
        num_planes: 1,
    };
    if convert_image(
        width,
        height,
        &src_fmt,
        None,
        src_planes,
        &dst_fmt,
        None,
        &mut [&mut dst],
    )
    .is_err()
    {
        return empty_rgba(width, height);
    }
    fill_alpha(&mut dst);
    dst
}

/// I420: planar Y, U, V with 4:2:0 subsampling.
pub fn i420_to_rgba(yuv: &[u8], width: u32, height: u32, colorspace: i32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2);
    let uv_size = uv_stride * h.div_ceil(2);
    if yuv.len() < y_size + 2 * uv_size {
        return empty_rgba(width, height);
    }
    let y = &yuv[..y_size];
    let u = &yuv[y_size..y_size + uv_size];
    let v = &yuv[y_size + uv_size..y_size + 2 * uv_size];
    convert_planar(PixelFormat::I420, width, height, colorspace, &[y, u, v])
}

/// YV12: planar Y, V, U (V before U). Swap plane pointers and use the I420
/// path.
pub fn yv12_to_rgba(yuv: &[u8], width: u32, height: u32, colorspace: i32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2);
    let uv_size = uv_stride * h.div_ceil(2);
    if yuv.len() < y_size + 2 * uv_size {
        return empty_rgba(width, height);
    }
    let y = &yuv[..y_size];
    let v = &yuv[y_size..y_size + uv_size];
    let u = &yuv[y_size + uv_size..y_size + 2 * uv_size];
    convert_planar(PixelFormat::I420, width, height, colorspace, &[y, u, v])
}

/// NV12: planar Y followed by interleaved UV pairs.
pub fn nv12_to_rgba(yuv: &[u8], width: u32, height: u32, colorspace: i32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2) * 2;
    let uv_size = uv_stride * h.div_ceil(2);
    if yuv.len() < y_size + uv_size {
        return empty_rgba(width, height);
    }
    let y = &yuv[..y_size];
    let uv = &yuv[y_size..y_size + uv_size];
    convert_planar(PixelFormat::Nv12, width, height, colorspace, &[y, uv])
}

/// NV21: planar Y followed by interleaved VU pairs (V before U). dcv only
/// supports NV12, so we copy Y as-is and swap the chroma byte order.
pub fn nv21_to_rgba(yuv: &[u8], width: u32, height: u32, colorspace: i32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_stride = w.div_ceil(2) * 2;
    let uv_size = uv_stride * h.div_ceil(2);
    if yuv.len() < y_size + uv_size {
        return empty_rgba(width, height);
    }
    let y = &yuv[..y_size];
    let vu = &yuv[y_size..y_size + uv_size];

    // Build a swapped UV plane.
    let mut uv = vec![0u8; uv_size];
    for pair in 0..(uv_size / 2) {
        uv[pair * 2] = vu[pair * 2 + 1]; // U from V slot
        uv[pair * 2 + 1] = vu[pair * 2]; // V from U slot
    }
    convert_planar(PixelFormat::Nv12, width, height, colorspace, &[y, &uv])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic I420 buffer with deterministic Y/U/V values.
    fn synth_i420(w: u32, h: u32) -> Vec<u8> {
        let wu = w as usize;
        let hu = h as usize;
        let y_size = wu * hu;
        let uv_size = wu.div_ceil(2) * hu.div_ceil(2);
        let mut buf = Vec::with_capacity(y_size + 2 * uv_size);
        for r in 0..hu {
            for c in 0..wu {
                buf.push(((r + c) as u8).wrapping_mul(7));
            }
        }
        for _ in 0..uv_size {
            buf.push(110);
        }
        for _ in 0..uv_size {
            buf.push(140);
        }
        buf
    }

    #[test]
    fn i420_round_size() {
        let yuv = synth_i420(16, 16);
        let out = i420_to_rgba(&yuv, 16, 16, 0);
        assert_eq!(out.len(), 16 * 16 * 4);
        // alpha column is 0xFF
        for px in out.chunks_exact(4) {
            assert_eq!(px[3], 0xFF);
        }
    }

    #[test]
    fn yv12_swaps_planes() {
        // YV12 layout = Y, V, U. With identical U/V the result must match I420.
        let yuv = synth_i420(16, 16); // y_size + uv + uv where both uv equal
        let mut yv12 = yuv.clone();
        let y_size = 16 * 16;
        let uv_size = 8 * 8;
        // U was at [y_size .. y_size+uv_size]; V at [y_size+uv_size..]
        // Simulate YV12 by swapping U and V.
        let (u_part, v_part) = yv12[y_size..y_size + 2 * uv_size].split_at_mut(uv_size);
        u_part.swap_with_slice(v_part);
        let i420_out = i420_to_rgba(&yuv, 16, 16, 0);
        let yv12_out = yv12_to_rgba(&yv12, 16, 16, 0);
        assert_eq!(i420_out, yv12_out);
    }

    #[test]
    fn nv12_round_size() {
        let w = 16u32;
        let h = 16u32;
        let mut buf = vec![128u8; (w * h) as usize];
        buf.extend(vec![128u8; (w * h / 2) as usize]);
        let out = nv12_to_rgba(&buf, w, h, 0);
        assert_eq!(out.len(), (w * h * 4) as usize);
        for px in out.chunks_exact(4) {
            assert_eq!(px[3], 0xFF);
        }
    }

    #[test]
    fn nv21_matches_swapped_nv12() {
        let w = 16u32;
        let h = 16u32;
        let y_size = (w * h) as usize;
        let uv_size = (w * h / 2) as usize;
        // Build NV12 with non-trivial UV pattern
        let mut nv12 = vec![100u8; y_size];
        for i in 0..uv_size {
            nv12.push(if i % 2 == 0 { 90 } else { 150 });
        }
        // Build NV21 as the same bytes with each UV pair reversed
        let mut nv21 = nv12[..y_size].to_vec();
        for chunk in nv12[y_size..].chunks_exact(2) {
            nv21.push(chunk[1]); // V first
            nv21.push(chunk[0]); // U second
        }
        let a = nv12_to_rgba(&nv12, w, h, 0);
        let b = nv21_to_rgba(&nv21, w, h, 0);
        assert_eq!(a, b);
    }
}
