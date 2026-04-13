//! Drawing handlers (opcodes 53-77).

use super::*;

mod pixmap;
mod gc;
mod primitives;
mod image;
mod text;

// Re-export all handler functions from submodules so handlers/mod.rs can call
// drawing::handle_create_pixmap etc. (visibility: accessible from handlers level)
pub(super) use pixmap::handle_create_pixmap;
pub(super) use pixmap::handle_free_pixmap;
pub(super) use gc::handle_create_gc;
pub(super) use gc::handle_change_gc;
pub(super) use gc::handle_copy_gc;
pub(super) use gc::handle_set_dashes;
pub(super) use gc::handle_set_clip_rectangles;
pub(super) use gc::handle_free_gc;
pub(super) use primitives::handle_clear_area;
pub(super) use primitives::handle_copy_area;
pub(super) use primitives::handle_copy_plane;
pub(super) use primitives::handle_poly_point;
pub(super) use primitives::handle_poly_line;
pub(super) use primitives::handle_poly_segment;
pub(super) use primitives::handle_poly_rectangle;
pub(super) use primitives::handle_poly_arc;
pub(super) use primitives::handle_fill_poly;
pub(super) use primitives::handle_poly_fill_rectangle;
pub(super) use primitives::handle_poly_fill_arc;
pub(super) use image::handle_put_image;
pub(super) use image::handle_get_image;
pub(super) use text::handle_poly_text8;
pub(super) use text::handle_poly_text16;
pub(super) use text::handle_image_text8;
pub(super) use text::handle_image_text16;

// ---------------------------------------------------------------------------
// Clip and ROP helpers
// ---------------------------------------------------------------------------

/// Check if a pixel at (x, y) should be drawn given the GC's clip rectangles.
/// The clip_x/clip_y origin has already been applied to the stored rectangles
/// during SetClipRectangles (or converted from bitmap mask), so the rectangles
/// are in drawable coordinates.
/// If clip_rects is empty, all pixels are valid (no clipping).
#[inline]
pub(crate) fn should_draw_pixel(x: i32, y: i32, clip_rects: &[(i16, i16, u16, u16)]) -> bool {
    if clip_rects.is_empty() {
        return true;
    }
    for &(rx, ry, rw, rh) in clip_rects {
        if x >= rx as i32 && x < rx as i32 + rw as i32
            && y >= ry as i32 && y < ry as i32 + rh as i32
        {
            return true;
        }
    }
    false
}

/// Apply X11 GC raster operation (ALU function) to source and destination pixels.
/// This mirrors `apply_gc_function` in framebuffer.rs but is available in the
/// drawing handler for pixel-level ROP when building images.
#[inline]
pub(crate) fn apply_rop(function: u8, src: u32, dst: u32) -> u32 {
    match function {
        0 => 0,                      // GXclear
        1 => src & dst,              // GXand
        2 => src & !dst,             // GXandReverse
        3 => src,                    // GXcopy
        4 => !src & dst,             // GXandInverted
        5 => dst,                    // GXnoop
        6 => src ^ dst,              // GXxor
        7 => src | dst,              // GXor
        8 => !(src | dst),           // GXnor
        9 => !(src ^ dst),           // GXequiv
        10 => !dst,                  // GXinvert
        11 => src | !dst,            // GXorReverse
        12 => !src,                  // GXcopyInverted
        13 => !src | dst,            // GXorInverted
        14 => !(src & dst),          // GXnand
        15 => 0xFFFFFFFF,            // GXset
        _ => src,                    // default to copy
    }
}

/// Compute the intersection of two rectangles.
/// Returns (x, y, width, height) of the intersection.
/// Width/height will be 0 if there is no intersection.
#[inline]
pub(crate) fn intersect_rects(
    x1: i16, y1: i16, w1: u16, h1: u16,
    x2: i16, y2: i16, w2: u16, h2: u16,
) -> (i16, i16, u16, u16) {
    let ix0 = (x1 as i32).max(x2 as i32);
    let iy0 = (y1 as i32).max(y2 as i32);
    let ix1 = (x1 as i32 + w1 as i32).min(x2 as i32 + w2 as i32);
    let iy1 = (y1 as i32 + h1 as i32).min(y2 as i32 + h2 as i32);
    if ix0 < ix1 && iy0 < iy1 {
        (ix0 as i16, iy0 as i16, (ix1 - ix0) as u16, (iy1 - iy0) as u16)
    } else {
        (0, 0, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // apply_rop — all 16 GC raster functions
    // -----------------------------------------------------------------------

    #[test]
    fn rop_gx_clear() {
        assert_eq!(apply_rop(0, 0xFF00FF, 0x00FF00), 0);
    }

    #[test]
    fn rop_gx_and() {
        assert_eq!(apply_rop(1, 0xFF00FF, 0x00FF00), 0x000000);
        assert_eq!(apply_rop(1, 0xFFFFFF, 0x123456), 0x123456);
    }

    #[test]
    fn rop_gx_and_reverse() {
        assert_eq!(apply_rop(2, 0xFF0000, 0x00FF00), 0xFF0000);
    }

    #[test]
    fn rop_gx_copy() {
        assert_eq!(apply_rop(3, 0xABCDEF, 0x000000), 0xABCDEF);
    }

    #[test]
    fn rop_gx_and_inverted() {
        assert_eq!(apply_rop(4, 0xFF0000, 0x00FF00), 0x00FF00);
    }

    #[test]
    fn rop_gx_noop() {
        assert_eq!(apply_rop(5, 0xABCDEF, 0x123456), 0x123456);
    }

    #[test]
    fn rop_gx_xor() {
        assert_eq!(apply_rop(6, 0xFF00FF, 0x00FF00), 0xFFFFFF);
    }

    #[test]
    fn rop_gx_or() {
        assert_eq!(apply_rop(7, 0xFF0000, 0x00FF00), 0xFFFF00);
    }

    #[test]
    fn rop_gx_nor() {
        // !(0xFF0000 | 0x00FF00) = !0xFFFF00 = 0xFF0000FF (we only care about low 24 bits)
        let result = apply_rop(8, 0xFF0000, 0x00FF00);
        assert_eq!(result & 0x00FFFFFF, 0x0000FF);
    }

    #[test]
    fn rop_gx_equiv() {
        // !(src ^ dst) = !(0xFF0000 ^ 0x00FF00) = !0xFFFF00
        let result = apply_rop(9, 0xFF0000, 0x00FF00);
        assert_eq!(result & 0x00FFFFFF, 0x0000FF);
    }

    #[test]
    fn rop_gx_invert() {
        let result = apply_rop(10, 0x000000, 0x00FF00);
        assert_eq!(result & 0x00FFFFFF, 0xFF00FF);
    }

    #[test]
    fn rop_gx_or_reverse() {
        assert_eq!(apply_rop(11, 0xFF0000, 0x000000) & 0x00FFFFFF, 0xFFFFFF);
    }

    #[test]
    fn rop_gx_copy_inverted() {
        let result = apply_rop(12, 0xFF0000, 0x000000);
        assert_eq!(result & 0x00FFFFFF, 0x00FFFF);
    }

    #[test]
    fn rop_gx_or_inverted() {
        // !src | dst = !0xFF0000 | 0x00FF00
        let result = apply_rop(13, 0xFF0000, 0x00FF00);
        assert_eq!(result & 0x00FFFFFF, 0x00FFFF);
    }

    #[test]
    fn rop_gx_nand() {
        let result = apply_rop(14, 0xFFFFFF, 0xFFFFFF);
        assert_eq!(result & 0x00FFFFFF, 0x000000);
    }

    #[test]
    fn rop_gx_set() {
        assert_eq!(apply_rop(15, 0, 0), 0xFFFFFFFF);
    }

    // -----------------------------------------------------------------------
    // intersect_rects
    // -----------------------------------------------------------------------

    #[test]
    fn intersect_rects_overlap() {
        let (x, y, w, h) = intersect_rects(0, 0, 10, 10, 5, 5, 10, 10);
        assert_eq!((x, y, w, h), (5, 5, 5, 5));
    }

    #[test]
    fn intersect_rects_no_overlap() {
        let (_, _, w, h) = intersect_rects(0, 0, 5, 5, 10, 10, 5, 5);
        assert_eq!((w, h), (0, 0));
    }

    #[test]
    fn intersect_rects_contained() {
        let (x, y, w, h) = intersect_rects(0, 0, 20, 20, 5, 5, 5, 5);
        assert_eq!((x, y, w, h), (5, 5, 5, 5));
    }

    // -----------------------------------------------------------------------
    // should_draw_pixel
    // -----------------------------------------------------------------------

    #[test]
    fn should_draw_pixel_empty_clips() {
        assert!(should_draw_pixel(0, 0, &[]));
    }

    #[test]
    fn should_draw_pixel_inside_clip() {
        let clips = vec![(0i16, 0i16, 10u16, 10u16)];
        assert!(should_draw_pixel(5, 5, &clips));
    }

    #[test]
    fn should_draw_pixel_outside_clip() {
        let clips = vec![(0i16, 0i16, 10u16, 10u16)];
        assert!(!should_draw_pixel(15, 15, &clips));
    }

    // -----------------------------------------------------------------------
    // plane_mask application
    // -----------------------------------------------------------------------

    #[test]
    fn plane_mask_preserves_masked_bits() {
        let src = 0xFFFFFF;
        let dst = 0x123456;
        let plane_mask: u32 = 0xFF0000; // only red channel
        let result = apply_rop(3, src, dst); // GXcopy
        let masked = (result & plane_mask) | (dst & !plane_mask);
        assert_eq!(masked & 0xFF0000, 0xFF0000); // red from src
        assert_eq!(masked & 0x00FF00, 0x003400); // green from dst
        assert_eq!(masked & 0x0000FF, 0x000056); // blue from dst
    }
}
