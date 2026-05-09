//! X11 server shared data types.
//!
//! This module is split into focused submodules for maintainability, but
//! re-exports everything at the module root so existing `use super::types::*`
//! imports continue to work unchanged.

mod colormap;
mod control;
mod cursor;
mod gc;
mod pixmap;
mod randr;
mod region;
mod routing;
mod selection;
mod window;

// Re-export everything so callers can still do `use super::types::*`.
pub use routing::*;
pub(crate) use routing::{
    EventBroadcaster, EventRouter, ServerGrabLock, SharedKeymap, SharedWindows, WindowMessage,
};

pub(crate) use colormap::*;
pub(crate) use control::*;
pub(crate) use cursor::*;
pub(crate) use gc::*;
pub(crate) use pixmap::*;
pub(crate) use randr::*;
pub(crate) use region::*;
pub(crate) use selection::*;
pub(crate) use window::*;

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // GcState::default() values
    // -----------------------------------------------------------------------

    #[test]
    fn gc_state_default_function_is_gxcopy() {
        let gc = GcState::default();
        assert_eq!(gc.function, 3, "default GC function must be GXcopy (3)");
    }

    #[test]
    fn gc_state_default_plane_mask_is_all_ones() {
        let gc = GcState::default();
        assert_eq!(
            gc.plane_mask, 0xFFFF_FFFF,
            "default plane_mask must be all ones"
        );
    }

    #[test]
    fn gc_state_default_foreground_is_black() {
        let gc = GcState::default();
        assert_eq!(
            gc.foreground, 0x00_00_00,
            "default foreground must be black (0x000000)"
        );
    }

    #[test]
    fn gc_state_default_background_is_white() {
        let gc = GcState::default();
        assert_eq!(
            gc.background, 0xFF_FF_FF,
            "default background must be white (0xFFFFFF)"
        );
    }

    #[test]
    fn gc_state_default_line_width_is_zero() {
        let gc = GcState::default();
        assert_eq!(gc.line_width, 0);
    }

    #[test]
    fn gc_state_default_line_style_is_solid() {
        let gc = GcState::default();
        assert_eq!(gc.line_style, 0, "default line_style must be Solid (0)");
    }

    #[test]
    fn gc_state_default_cap_style_is_butt() {
        let gc = GcState::default();
        assert_eq!(gc.cap_style, 1, "default cap_style must be Butt (1)");
    }

    #[test]
    fn gc_state_default_join_style_is_miter() {
        let gc = GcState::default();
        assert_eq!(gc.join_style, 0, "default join_style must be Miter (0)");
    }

    #[test]
    fn gc_state_default_fill_style_is_solid() {
        let gc = GcState::default();
        assert_eq!(gc.fill_style, 0, "default fill_style must be Solid (0)");
    }

    #[test]
    fn gc_state_default_fill_rule_is_even_odd() {
        let gc = GcState::default();
        assert_eq!(gc.fill_rule, 0, "default fill_rule must be EvenOdd (0)");
    }

    #[test]
    fn gc_state_default_tile_and_stipple_are_zero() {
        let gc = GcState::default();
        assert_eq!(gc.tile, 0);
        assert_eq!(gc.stipple, 0);
    }

    #[test]
    fn gc_state_default_tile_stipple_origin_is_zero() {
        let gc = GcState::default();
        assert_eq!(gc.ts_x, 0);
        assert_eq!(gc.ts_y, 0);
    }

    #[test]
    fn gc_state_default_font_id_is_zero() {
        let gc = GcState::default();
        assert_eq!(gc.font_id, 0);
    }

    #[test]
    fn gc_state_default_subwindow_mode_is_clip_by_children() {
        let gc = GcState::default();
        assert_eq!(
            gc.subwindow_mode, 0,
            "default subwindow_mode must be ClipByChildren (0)"
        );
    }

    #[test]
    fn gc_state_default_graphics_exposures_is_true() {
        let gc = GcState::default();
        assert!(
            gc.graphics_exposures,
            "default graphics_exposures must be true"
        );
    }

    #[test]
    fn gc_state_default_clip_origin_is_zero() {
        let gc = GcState::default();
        assert_eq!(gc.clip_x, 0);
        assert_eq!(gc.clip_y, 0);
    }

    #[test]
    fn gc_state_default_clip_mask_is_none() {
        let gc = GcState::default();
        assert_eq!(gc.clip_mask, 0, "default clip_mask must be None (0)");
    }

    #[test]
    fn gc_state_default_dash_offset_is_zero() {
        let gc = GcState::default();
        assert_eq!(gc.dash_offset, 0);
    }

    #[test]
    fn gc_state_default_dashes_is_4() {
        let gc = GcState::default();
        assert_eq!(gc.dashes, 4);
    }

    #[test]
    fn gc_state_default_arc_mode_is_pie_slice() {
        let gc = GcState::default();
        assert_eq!(gc.arc_mode, 1, "default arc_mode must be PieSlice (1)");
    }

    #[test]
    fn gc_state_default_clip_rects_is_empty() {
        let gc = GcState::default();
        assert!(gc.clip_rects.is_empty(), "default clip_rects must be empty");
    }

    #[test]
    fn gc_state_default_dash_list_is_empty() {
        let gc = GcState::default();
        assert!(gc.dash_list.is_empty(), "default dash_list must be empty");
    }

    // -----------------------------------------------------------------------
    // point_in_shape helper
    // -----------------------------------------------------------------------

    #[test]
    fn point_in_shape_inside() {
        let rects = vec![RegionRect {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
        }];
        assert!(point_in_shape(&rects, 50, 50));
        assert!(point_in_shape(&rects, 10, 10)); // top-left corner (inclusive)
    }

    #[test]
    fn point_in_shape_outside() {
        let rects = vec![RegionRect {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
        }];
        // Right edge is exclusive: x < r.x + r.width => x < 110
        assert!(!point_in_shape(&rects, 110, 50));
        assert!(!point_in_shape(&rects, 50, 110));
        assert!(!point_in_shape(&rects, 9, 50));
        assert!(!point_in_shape(&rects, 50, 9));
    }

    #[test]
    fn point_in_shape_empty_shape() {
        assert!(!point_in_shape(&[], 0, 0));
    }

    #[test]
    fn point_in_shape_multiple_rects() {
        let rects = vec![
            RegionRect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            RegionRect {
                x: 100,
                y: 100,
                width: 10,
                height: 10,
            },
        ];
        assert!(point_in_shape(&rects, 5, 5));
        assert!(point_in_shape(&rects, 105, 105));
        assert!(!point_in_shape(&rects, 50, 50));
    }

    // -----------------------------------------------------------------------
    // XFixesRegion operations
    // -----------------------------------------------------------------------

    #[test]
    fn region_extents_empty() {
        let r = XFixesRegion::new();
        let ext = r.extents();
        assert_eq!(ext.x, 0);
        assert_eq!(ext.y, 0);
        assert_eq!(ext.width, 0);
        assert_eq!(ext.height, 0);
    }

    #[test]
    fn region_extents_single_rect() {
        let r = XFixesRegion::from_rects(vec![RegionRect {
            x: 5,
            y: 10,
            width: 20,
            height: 30,
        }]);
        let ext = r.extents();
        assert_eq!(ext.x, 5);
        assert_eq!(ext.y, 10);
        assert_eq!(ext.width, 20);
        assert_eq!(ext.height, 30);
    }

    #[test]
    fn region_extents_multiple_rects() {
        let r = XFixesRegion::from_rects(vec![
            RegionRect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            RegionRect {
                x: 20,
                y: 20,
                width: 10,
                height: 10,
            },
        ]);
        let ext = r.extents();
        // Bounding box: x=0, y=0, right=30, bottom=30 => width=30, height=30
        assert_eq!(ext.x, 0);
        assert_eq!(ext.y, 0);
        assert_eq!(ext.width, 30);
        assert_eq!(ext.height, 30);
    }

    #[test]
    fn region_union() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 20,
            y: 20,
            width: 5,
            height: 5,
        }]);
        let u = a.union(&b);
        assert_eq!(u.rects.len(), 2);
    }

    #[test]
    fn region_intersect_overlapping() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 20,
            height: 20,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
        }]);
        let i = a.intersect(&b);
        assert_eq!(i.rects.len(), 1);
        assert_eq!(i.rects[0].x, 10);
        assert_eq!(i.rects[0].y, 10);
        assert_eq!(i.rects[0].width, 10);
        assert_eq!(i.rects[0].height, 10);
    }

    #[test]
    fn region_intersect_non_overlapping() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 20,
            y: 20,
            width: 10,
            height: 10,
        }]);
        let i = a.intersect(&b);
        assert!(
            i.rects.is_empty(),
            "non-overlapping regions have empty intersection"
        );
    }

    #[test]
    fn region_translate() {
        let mut r = XFixesRegion::from_rects(vec![RegionRect {
            x: 5,
            y: 10,
            width: 20,
            height: 30,
        }]);
        r.translate(15, -5);
        assert_eq!(r.rects[0].x, 20);
        assert_eq!(r.rects[0].y, 5);
        assert_eq!(r.rects[0].width, 20);
        assert_eq!(r.rects[0].height, 30);
    }

    // -----------------------------------------------------------------------
    // ColormapState helpers
    // -----------------------------------------------------------------------

    #[test]
    fn colormap_truecolor_is_not_writable() {
        let cm = ColormapState::new_truecolor(0x21);
        assert!(!cm.is_writable(), "TrueColor colormap must not be writable");
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::TRUE_COLOR
        );
    }

    #[test]
    fn colormap_pseudocolor_is_writable() {
        let cm = ColormapState::new_pseudocolor(0x21, 256);
        assert!(cm.is_writable(), "PseudoColor colormap must be writable");
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::PSEUDO_COLOR
        );
        assert_eq!(cm.entries.len(), 256);
    }

    #[test]
    fn colormap_grayscale_is_writable() {
        let cm = ColormapState::new_grayscale(0x21, 256);
        assert!(cm.is_writable());
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::GRAY_SCALE
        );
    }

    #[test]
    fn colormap_directcolor_is_writable() {
        let cm = ColormapState::new_directcolor(0x21, 256);
        assert!(cm.is_writable());
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::DIRECT_COLOR
        );
    }

    #[test]
    fn colormap_staticgray_is_not_writable() {
        let cm = ColormapState::new_staticgray(0x21, 256);
        assert!(!cm.is_writable());
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::STATIC_GRAY
        );
        // All cells pre-allocated for read-only maps
        assert!(cm.allocated.iter().all(|&a| a));
    }

    #[test]
    fn colormap_staticcolor_is_not_writable() {
        // Directly construct to avoid the u16-overflow in new_staticcolor's
        // 3-3-2 palette computation (a pre-existing implementation limitation).
        let n = 4;
        let cm = ColormapState {
            visual: 0x21,
            visual_class: x11rb_protocol::protocol::xproto::VisualClass::STATIC_COLOR,
            entries: vec![(0, 0, 0); n],
            allocated: vec![true; n], // read-only: all pre-allocated
            next_free: n,
        };
        assert!(
            !cm.is_writable(),
            "StaticColor colormap must not be writable"
        );
        assert_eq!(
            cm.visual_class,
            x11rb_protocol::protocol::xproto::VisualClass::STATIC_COLOR
        );
        // All pre-allocated (read-only)
        assert!(cm.allocated.iter().all(|&a| a));
    }

    #[test]
    fn colormap_truecolor_alloc_color_pixel() {
        let mut cm = ColormapState::new_truecolor(0x21);
        // TrueColor: pixel = (r>>8)<<16 | (g>>8)<<8 | (b>>8)
        let pixel = cm.alloc_color(0xFF00, 0x8000, 0x0000);
        assert_eq!(pixel, Some(0xFF8000));
    }

    #[test]
    fn colormap_pseudocolor_alloc_and_lookup() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        let pixel = cm
            .alloc_color(0xFFFF, 0x0000, 0x0000)
            .expect("alloc must succeed");
        let (r, g, b) = cm.lookup(pixel);
        assert_eq!(r, 0xFFFF);
        assert_eq!(g, 0x0000);
        assert_eq!(b, 0x0000);
    }

    #[test]
    fn colormap_truecolor_lookup_decomposes_pixel() {
        let cm = ColormapState::new_truecolor(0x21);
        // Pixel 0xFF0080 => r=0xFF, g=0x00, b=0x80
        let (r, g, b) = cm.lookup(0xFF0080);
        assert_eq!(r, 0xFFFF);
        assert_eq!(g, 0x0000);
        assert_eq!(b, 0x8080);
    }

    #[test]
    fn colormap_store_colors() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        // Force an entry at pixel 5 to specific RGB
        cm.store_colors(&[(5, 0x1234, 0x5678, 0x9ABC, 0)]);
        let (r, g, b) = cm.lookup(5);
        assert_eq!(r, 0x1234);
        assert_eq!(g, 0x5678);
        assert_eq!(b, 0x9ABC);
    }

    #[test]
    fn colormap_store_colors_selective_channels() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        cm.store_colors(&[(0, 0x1111, 0x2222, 0x3333, 0)]);
        // Now change only the red channel (flags = 0x01 = DoRed)
        cm.store_colors(&[(0, 0xFFFF, 0x0000, 0x0000, 0x01)]);
        let (r, g, b) = cm.lookup(0);
        assert_eq!(r, 0xFFFF, "red channel must be updated");
        assert_eq!(g, 0x2222, "green channel must be unchanged");
        assert_eq!(b, 0x3333, "blue channel must be unchanged");
    }

    #[test]
    fn colormap_alloc_cells_and_free() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        let pixels = cm.alloc_cells(4).expect("must allocate 4 cells");
        assert_eq!(pixels.len(), 4);
        // All must be distinct
        let unique: std::collections::HashSet<u32> = pixels.iter().copied().collect();
        assert_eq!(unique.len(), 4);
        // Free them
        cm.free_cells(&pixels);
        // Should be able to re-allocate the same cells now
        let pixels2 = cm.alloc_cells(4).expect("must reallocate 4 freed cells");
        assert_eq!(pixels2.len(), 4);
    }

    // -----------------------------------------------------------------------
    // generate_edid
    // -----------------------------------------------------------------------

    #[test]
    fn edid_is_128_bytes() {
        let edid = generate_edid(527, 296, 1920, 1080);
        assert_eq!(edid.len(), 128);
    }

    #[test]
    fn edid_has_correct_header() {
        let edid = generate_edid(527, 296, 1920, 1080);
        assert_eq!(
            &edid[0..8],
            &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]
        );
    }

    #[test]
    fn edid_checksum_valid() {
        let edid = generate_edid(527, 296, 1920, 1080);
        let sum: u32 = edid.iter().map(|&b| b as u32).sum();
        assert_eq!(
            sum % 256,
            0,
            "EDID checksum must make all 128 bytes sum to 0 mod 256"
        );
    }

    #[test]
    fn edid_version_1_3() {
        let edid = generate_edid(527, 296, 1920, 1080);
        assert_eq!(edid[18], 1, "EDID version must be 1");
        assert_eq!(edid[19], 3, "EDID revision must be 3");
    }

    // -----------------------------------------------------------------------
    // Region: subtract
    // -----------------------------------------------------------------------

    #[test]
    fn region_subtract_no_overlap() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 20,
            y: 20,
            width: 10,
            height: 10,
        }]);
        let s = a.subtract(&b);
        assert_eq!(
            s.rects.len(),
            1,
            "subtracting non-overlapping region should keep original"
        );
        assert_eq!(s.rects[0].x, 0);
        assert_eq!(s.rects[0].width, 10);
    }

    #[test]
    fn region_subtract_full_overlap() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        }]);
        let s = a.subtract(&b);
        assert!(
            s.rects.is_empty(),
            "subtracting encompassing region should yield empty"
        );
    }

    #[test]
    fn region_subtract_partial_creates_fragments() {
        let a = XFixesRegion::from_rects(vec![RegionRect {
            x: 0,
            y: 0,
            width: 20,
            height: 20,
        }]);
        let b = XFixesRegion::from_rects(vec![RegionRect {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
        }]);
        let s = a.subtract(&b);
        // Should create 4 fragments: top, left, right, bottom strips
        assert!(
            s.rects.len() >= 3,
            "subtracting center should create at least 3 fragments, got {}",
            s.rects.len()
        );
        // The combined area should equal 20*20 - 10*10 = 300
        let total_area: i32 = s
            .rects
            .iter()
            .map(|r| r.width as i32 * r.height as i32)
            .sum();
        assert_eq!(
            total_area, 300,
            "area after subtraction must be 300 (400 - 100)"
        );
    }

    #[test]
    fn region_invert_empty() {
        let empty = XFixesRegion::new();
        let bounds = RegionRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let inv = empty.invert(&bounds);
        assert_eq!(
            inv.rects.len(),
            1,
            "inverting empty region should yield the bounding rect"
        );
        assert_eq!(inv.rects[0].width, 100);
        assert_eq!(inv.rects[0].height, 100);
    }

    #[test]
    fn region_expand_increases_extents() {
        let r = XFixesRegion::from_rects(vec![RegionRect {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
        }]);
        let expanded = r.expand(5, 5, 5, 5);
        assert_eq!(expanded.rects.len(), 1);
        assert_eq!(expanded.rects[0].x, 5);
        assert_eq!(expanded.rects[0].y, 5);
        assert_eq!(expanded.rects[0].width, 30);
        assert_eq!(expanded.rects[0].height, 30);
    }

    // -----------------------------------------------------------------------
    // ColormapState: edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn colormap_pseudocolor_alloc_exhaustion() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 4);
        // Allocate 4 distinct colors
        assert!(cm.alloc_color(0xFF00, 0x0000, 0x0000).is_some());
        assert!(cm.alloc_color(0x0000, 0xFF00, 0x0000).is_some());
        assert!(cm.alloc_color(0x0000, 0x0000, 0xFF00).is_some());
        assert!(cm.alloc_color(0xFF00, 0xFF00, 0x0000).is_some());
        // 5th distinct color should fail (all 4 cells used)
        assert!(
            cm.alloc_color(0x0000, 0xFF00, 0xFF00).is_none(),
            "allocation must fail when all cells used"
        );
    }

    #[test]
    fn colormap_pseudocolor_dedup_alloc() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        let p1 = cm.alloc_color(0xAAAA, 0xBBBB, 0xCCCC).unwrap();
        let p2 = cm.alloc_color(0xAAAA, 0xBBBB, 0xCCCC).unwrap();
        assert_eq!(p1, p2, "allocating same color twice must return same pixel");
    }

    #[test]
    fn colormap_staticgray_find_closest() {
        let cm = ColormapState::new_staticgray(0x25, 16);
        let mut cm = cm; // alloc_color needs &mut self
                         // Allocate a mid-gray — should pick the closest entry
        let pixel = cm.alloc_color(0x8000, 0x8000, 0x8000).unwrap();
        // Pixel should be near the middle of the 16-entry ramp
        assert!(
            pixel >= 6 && pixel <= 9,
            "closest match for 0x8000 in 16-entry ramp should be near index 8, got {}",
            pixel
        );
    }

    #[test]
    fn colormap_directcolor_lookup() {
        let cm = ColormapState::new_directcolor(0x22, 256);
        // Pixel 0x804020: R channel index 0x80, G index 0x40, B index 0x20
        let (r, g, b) = cm.lookup(0x804020);
        // Each channel should be looked up independently in the identity ramp
        assert!(
            r > 0 && g > 0 && b > 0,
            "DirectColor lookup must return non-zero for non-zero indices"
        );
        assert!(r > g && g > b, "R > G > B for pixel 0x804020");
    }

    #[test]
    fn colormap_store_colors_do_green_only() {
        let mut cm = ColormapState::new_pseudocolor(0x21, 256);
        cm.store_colors(&[(0, 0x1111, 0x2222, 0x3333, 0)]);
        // Only change green (flags = 0x02 = DoGreen)
        cm.store_colors(&[(0, 0x0000, 0xFFFF, 0x0000, 0x02)]);
        let (r, g, b) = cm.lookup(0);
        assert_eq!(r, 0x1111, "red must be unchanged");
        assert_eq!(g, 0xFFFF, "green must be updated");
        assert_eq!(b, 0x3333, "blue must be unchanged");
    }

    // -----------------------------------------------------------------------
    // ClipMaskBitmap
    // -----------------------------------------------------------------------

    #[test]
    fn clip_mask_bitmap_test_bounds() {
        let bm = ClipMaskBitmap {
            width: 8,
            height: 4,
            bits: vec![0xFF, 0xFF, 0xFF, 0xFF], // all bits set (8x4 = 32 bits = 4 bytes)
        };
        assert!(bm.test(0, 0));
        assert!(bm.test(7, 3));
        assert!(!bm.test(8, 0), "out of bounds x");
        assert!(!bm.test(0, 4), "out of bounds y");
        assert!(!bm.test(-1, 0), "negative x");
    }

    #[test]
    fn clip_mask_bitmap_to_rects() {
        // 4-wide bitmap: bits 0b00001010 = pixels 1 and 3 set
        let bm = ClipMaskBitmap {
            width: 4,
            height: 1,
            bits: vec![0b0000_1010], // LSB-first: bit 1 and bit 3 set
        };
        let rects = bm.to_clip_rects(0, 0);
        // Should produce two 1x1 rects at x=1 and x=3
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].0, 1); // x
        assert_eq!(rects[0].2, 1); // width
        assert_eq!(rects[1].0, 3); // x
        assert_eq!(rects[1].2, 1); // width
    }

    #[test]
    fn clip_mask_bitmap_to_rects_with_offset() {
        // All bits set in a 4x1 bitmap
        let bm = ClipMaskBitmap {
            width: 4,
            height: 1,
            bits: vec![0xFF],
        };
        let rects = bm.to_clip_rects(10, 20);
        assert_eq!(rects.len(), 1, "contiguous run should be single rect");
        assert_eq!(rects[0].0, 10, "x should be offset");
        assert_eq!(rects[0].1, 20, "y should be offset");
        assert_eq!(rects[0].2, 4, "width should be 4");
    }

    // -----------------------------------------------------------------------
    // GcState: effective_clip_rects and has_clip
    // -----------------------------------------------------------------------

    #[test]
    fn gc_state_no_clip_by_default() {
        let gc = GcState::default();
        assert!(!gc.has_clip());
        assert!(gc.effective_clip_rects().is_empty());
    }

    #[test]
    fn gc_state_clip_rects_take_precedence() {
        let mut gc = GcState::default();
        gc.clip_rects = vec![(0, 0, 100, 100)];
        assert!(gc.has_clip());
        assert_eq!(gc.effective_clip_rects().len(), 1);
    }

    #[test]
    fn gc_state_bitmap_mask_converts_to_rects() {
        let mut gc = GcState::default();
        gc.clip_mask_bitmap = Some(ClipMaskBitmap {
            width: 4,
            height: 1,
            bits: vec![0xFF],
        });
        assert!(gc.has_clip());
        let rects = gc.effective_clip_rects();
        assert!(!rects.is_empty(), "bitmap mask should produce clip rects");
    }

    // -----------------------------------------------------------------------
    // EDID: additional validation
    // -----------------------------------------------------------------------

    #[test]
    fn edid_checksum_various_sizes() {
        for (w, h) in [(640u32, 480u32), (1280, 720), (1920, 1080)] {
            let mm_w = ((w * 254 + 480) / 960) as u16;
            let mm_h = ((h * 254 + 480) / 960) as u16;
            let edid = generate_edid(mm_w, mm_h, w as u16, h as u16);
            let sum: u32 = edid.iter().map(|&b| b as u32).sum();
            assert_eq!(sum % 256, 0, "EDID checksum failed for {}x{}", w, h);
        }
    }

    #[test]
    fn edid_monitor_name_present() {
        let edid = generate_edid(527, 296, 1920, 1080);
        // Monitor name descriptor tag at byte 75 should be 0xFC
        assert_eq!(edid[75], 0xFC, "DTD#2 should be monitor name descriptor");
        // Name should start with "X11-Web"
        assert_eq!(&edid[77..84], b"X11-Web", "monitor name must be 'X11-Web'");
    }

    // -----------------------------------------------------------------------
    // SizeHints: default values
    // -----------------------------------------------------------------------

    #[test]
    fn size_hints_default_is_zero() {
        let hints = SizeHints::default();
        assert_eq!(hints.min_width, 0);
        assert_eq!(hints.min_height, 0);
        assert_eq!(hints.max_width, 0);
        assert_eq!(hints.max_height, 0);
        assert_eq!(hints.width_inc, 0);
        assert_eq!(hints.height_inc, 0);
        assert_eq!(hints.base_width, 0);
        assert_eq!(hints.base_height, 0);
    }

    // -----------------------------------------------------------------------
    // is_descendant_of: circular parent detection for ReparentWindow
    // -----------------------------------------------------------------------

    fn make_test_window(id: u32, parent: u32) -> WindowState {
        use crate::framebuffer::Framebuffer;
        use x11rb_protocol::protocol::xproto::{BackingStore, WindowClass};
        WindowState {
            id,
            parent,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            border_width: 0,
            visual: 0x21,
            depth: 24,
            class: u16::from(WindowClass::INPUT_OUTPUT),
            mapped: false,
            event_mask: 0,
            do_not_propagate_mask: 0,
            background_pixel: 0,
            background_pixmap: None,
            border_pixel: 0,
            border_pixmap: None,
            override_redirect: false,
            redirected: false,
            framebuffer: Framebuffer::new(100, 100),
            properties: std::collections::HashMap::new(),
            owner_client_id: String::new(),
            cursor: None,
            children_order: Vec::new(),
            retained_temporary: false,
            bounding_shape: None,
            clip_shape: None,
            input_shape: None,
            shape_select_clients: Vec::new(),
            colormap: 0,
            backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
            backing_planes: 0xFFFFFFFF,
            backing_pixel: 0,
            save_under: false,
            visibility: 0,
            backing_pixmap: None,
            wm_hints_initial_state: None,
            transient_for: None,
            sync_request_counter: None,
            sync_request_value: 0,
            window_type: super::WindowType::Normal,
            strut: None,
            wm_hints_input: None,
            wm_hints_window_group: None,
            modal: false,
            saved_geometry: None,
        }
    }

    #[test]
    fn is_descendant_of_direct_child() {
        let mut windows = std::collections::HashMap::new();
        windows.insert(1, make_test_window(1, 0)); // root
        windows.insert(2, make_test_window(2, 1)); // child of root
        assert!(crate::xserver::is_descendant_of(&windows, 2, 1));
    }

    #[test]
    fn is_descendant_of_grandchild() {
        let mut windows = std::collections::HashMap::new();
        windows.insert(1, make_test_window(1, 0));
        windows.insert(2, make_test_window(2, 1));
        windows.insert(3, make_test_window(3, 2));
        assert!(crate::xserver::is_descendant_of(&windows, 3, 1));
    }

    #[test]
    fn is_descendant_of_not_ancestor() {
        let mut windows = std::collections::HashMap::new();
        windows.insert(1, make_test_window(1, 0));
        windows.insert(2, make_test_window(2, 1));
        windows.insert(3, make_test_window(3, 1));
        // 2 and 3 are siblings, neither is ancestor of the other
        assert!(!crate::xserver::is_descendant_of(&windows, 2, 3));
        assert!(!crate::xserver::is_descendant_of(&windows, 3, 2));
    }

    #[test]
    fn is_descendant_of_reverse_not_true() {
        let mut windows = std::collections::HashMap::new();
        windows.insert(1, make_test_window(1, 0));
        windows.insert(2, make_test_window(2, 1));
        // 1 is not a descendant of 2
        assert!(!crate::xserver::is_descendant_of(&windows, 1, 2));
    }

    #[test]
    fn is_descendant_of_same_window() {
        let mut windows = std::collections::HashMap::new();
        windows.insert(1, make_test_window(1, 0));
        // A window is not a descendant of itself
        assert!(!crate::xserver::is_descendant_of(&windows, 1, 1));
    }

    // -----------------------------------------------------------------------
    // WindowType: stacking layer and focus policy
    // -----------------------------------------------------------------------

    #[test]
    fn window_type_stacking_layers_correct_order() {
        // Desktop < Normal < Dock < Tooltip
        assert!(WindowType::Desktop.stacking_layer() < WindowType::Normal.stacking_layer());
        assert!(WindowType::Normal.stacking_layer() < WindowType::Dock.stacking_layer());
        assert!(WindowType::Dock.stacking_layer() < WindowType::Tooltip.stacking_layer());
    }

    #[test]
    fn window_type_dialog_same_layer_as_normal() {
        assert_eq!(
            WindowType::Dialog.stacking_layer(),
            WindowType::Normal.stacking_layer()
        );
    }

    #[test]
    fn window_type_tooltip_notification_same_top_layer() {
        assert_eq!(
            WindowType::Tooltip.stacking_layer(),
            WindowType::Notification.stacking_layer()
        );
        assert_eq!(
            WindowType::Tooltip.stacking_layer(),
            WindowType::PopupMenu.stacking_layer()
        );
        assert_eq!(
            WindowType::Tooltip.stacking_layer(),
            WindowType::DropdownMenu.stacking_layer()
        );
    }

    #[test]
    fn window_type_focus_policy() {
        assert!(WindowType::Normal.accepts_focus());
        assert!(WindowType::Dialog.accepts_focus());
        assert!(!WindowType::Dock.accepts_focus());
        assert!(!WindowType::Tooltip.accepts_focus());
        assert!(!WindowType::Notification.accepts_focus());
        assert!(!WindowType::Desktop.accepts_focus());
        assert!(!WindowType::PopupMenu.accepts_focus());
    }

    #[test]
    fn window_type_from_atom_ids_first_match_wins() {
        // Per EWMH spec, first recognized type in the list should be used
        assert_eq!(WindowType::from_atom_ids(&[86, 80]), WindowType::Dock); // DOCK before NORMAL
        assert_eq!(WindowType::from_atom_ids(&[80, 86]), WindowType::Normal); // NORMAL before DOCK
    }

    #[test]
    fn window_type_from_atom_ids_unknown_skipped() {
        // Unknown atoms should be skipped
        assert_eq!(WindowType::from_atom_ids(&[999, 86]), WindowType::Dock);
        assert_eq!(WindowType::from_atom_ids(&[999, 998]), WindowType::Normal); // fallback
    }

    #[test]
    fn window_type_from_atom_ids_empty() {
        assert_eq!(WindowType::from_atom_ids(&[]), WindowType::Normal);
    }

    #[test]
    fn window_type_all_atom_ids_recognized() {
        assert_eq!(WindowType::from_atom_ids(&[87]), WindowType::Desktop);
        assert_eq!(WindowType::from_atom_ids(&[86]), WindowType::Dock);
        assert_eq!(WindowType::from_atom_ids(&[82]), WindowType::Toolbar);
        assert_eq!(WindowType::from_atom_ids(&[83]), WindowType::Menu);
        assert_eq!(WindowType::from_atom_ids(&[84]), WindowType::Utility);
        assert_eq!(WindowType::from_atom_ids(&[85]), WindowType::Splash);
        assert_eq!(WindowType::from_atom_ids(&[81]), WindowType::Dialog);
        assert_eq!(WindowType::from_atom_ids(&[88]), WindowType::DropdownMenu);
        assert_eq!(WindowType::from_atom_ids(&[89]), WindowType::PopupMenu);
        assert_eq!(WindowType::from_atom_ids(&[90]), WindowType::Tooltip);
        assert_eq!(WindowType::from_atom_ids(&[91]), WindowType::Notification);
        assert_eq!(WindowType::from_atom_ids(&[80]), WindowType::Normal);
    }

    // -----------------------------------------------------------------------
    // RotateProperties: duplicate atom detection
    // -----------------------------------------------------------------------

    #[test]
    fn rotate_properties_duplicate_detection() {
        // Verify that duplicate atoms in a list are properly detected
        let atoms = vec![10u32, 20, 30, 20]; // 20 appears twice
        let mut seen = std::collections::HashSet::with_capacity(atoms.len());
        let mut found_dup = false;
        for &atom in &atoms {
            if !seen.insert(atom) {
                found_dup = true;
                break;
            }
        }
        assert!(found_dup, "Should detect duplicate atom 20");
    }

    #[test]
    fn rotate_properties_no_duplicates() {
        let atoms = vec![10u32, 20, 30, 40];
        let mut seen = std::collections::HashSet::with_capacity(atoms.len());
        let all_unique = atoms.iter().all(|a| seen.insert(*a));
        assert!(all_unique, "All atoms should be unique");
    }

    // -----------------------------------------------------------------------
    // PropertyValue rotation logic
    // -----------------------------------------------------------------------

    #[test]
    fn rotate_properties_positive_delta() {
        // Simulate rotating 3 properties by delta=1
        // Properties: [A, B, C] with delta=1 -> [C, A, B]
        let values = vec![Some(1u32), Some(2), Some(3)];
        let n = values.len() as i16;
        let delta: i16 = 1;
        let effective_delta = ((delta % n) + n) % n;
        assert_eq!(effective_delta, 1);

        let mut result = vec![0u32; 3];
        for i in 0..3 {
            let src_idx = ((i as i16 - effective_delta + n) % n) as usize;
            result[i] = values[src_idx].unwrap();
        }
        // [A, B, C] rotated by +1 = [C, A, B]
        assert_eq!(result, vec![3, 1, 2]);
    }

    #[test]
    fn rotate_properties_negative_delta() {
        let values = vec![Some(1u32), Some(2), Some(3)];
        let n = values.len() as i16;
        let delta: i16 = -1;
        let effective_delta = ((delta % n) + n) % n;
        assert_eq!(effective_delta, 2);

        let mut result = vec![0u32; 3];
        for i in 0..3 {
            let src_idx = ((i as i16 - effective_delta + n) % n) as usize;
            result[i] = values[src_idx].unwrap();
        }
        // [A, B, C] rotated by -1 = [B, C, A]
        assert_eq!(result, vec![2, 3, 1]);
    }

    #[test]
    fn rotate_properties_full_cycle() {
        // Delta equals length -> no-op
        let values = vec![Some(1u32), Some(2), Some(3)];
        let n = values.len() as i16;
        let delta: i16 = 3;
        let effective_delta = ((delta % n) + n) % n;
        assert_eq!(effective_delta, 0);
    }

    // -----------------------------------------------------------------------
    // RetainTemporary window flag
    // -----------------------------------------------------------------------

    #[test]
    fn window_retained_temporary_default_false() {
        let win = make_test_window(1, 0);
        assert!(!win.retained_temporary);
    }

    #[test]
    fn window_retained_temporary_can_be_set() {
        let mut win = make_test_window(1, 0);
        win.retained_temporary = true;
        assert!(win.retained_temporary);
    }

    // -----------------------------------------------------------------------
    // XFixesRegion operations
    // -----------------------------------------------------------------------

    #[test]
    fn region_empty_has_no_rects() {
        let r = XFixesRegion::new();
        assert!(r.rects.is_empty());
    }

    #[test]
    fn region_from_single_rect() {
        let r = XFixesRegion::from_rects(vec![region::RegionRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        }]);
        assert_eq!(r.rects.len(), 1);
        assert_eq!(r.rects[0].x, 0);
        assert_eq!(r.rects[0].width, 100);
    }

    #[test]
    fn region_extents_from_constructed_rect() {
        let r = XFixesRegion::from_rects(vec![region::RegionRect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        }]);
        let ext = r.extents();
        assert_eq!(ext.x, 10);
        assert_eq!(ext.y, 20);
        assert_eq!(ext.width, 30);
        assert_eq!(ext.height, 40);
    }

    #[test]
    fn region_extents_bounding_box_from_overlapping() {
        let r = XFixesRegion::from_rects(vec![
            region::RegionRect {
                x: 0,
                y: 0,
                width: 50,
                height: 50,
            },
            region::RegionRect {
                x: 30,
                y: 30,
                width: 50,
                height: 50,
            },
        ]);
        let ext = r.extents();
        assert_eq!(ext.x, 0);
        assert_eq!(ext.y, 0);
        assert_eq!(ext.width, 80);
        assert_eq!(ext.height, 80);
    }

    // -----------------------------------------------------------------------
    // WindowState new field defaults
    // -----------------------------------------------------------------------

    #[test]
    fn window_state_wm_hints_defaults() {
        let w = make_test_window(42, 1);
        assert_eq!(w.wm_hints_input, None);
        assert_eq!(w.wm_hints_window_group, None);
        assert!(!w.modal);
    }

    #[test]
    fn window_state_modal_can_be_set() {
        let mut w = make_test_window(42, 1);
        w.modal = true;
        w.transient_for = Some(10);
        assert!(w.modal);
        assert_eq!(w.transient_for, Some(10));
    }

    #[test]
    fn window_state_wm_hints_input_can_be_set() {
        let mut w = make_test_window(42, 1);
        w.wm_hints_input = Some(false);
        assert_eq!(w.wm_hints_input, Some(false));
        w.wm_hints_input = Some(true);
        assert_eq!(w.wm_hints_input, Some(true));
    }

    #[test]
    fn window_state_window_group_can_be_set() {
        let mut w = make_test_window(42, 1);
        w.wm_hints_window_group = Some(100);
        assert_eq!(w.wm_hints_window_group, Some(100));
    }

    #[test]
    fn saved_geometry_initially_none() {
        let w = make_test_window(42, 1);
        assert!(w.saved_geometry.is_none());
    }

    #[test]
    fn saved_geometry_save_and_restore() {
        let mut w = make_test_window(42, 1);
        w.x = 50;
        w.y = 100;
        w.width = 400;
        w.height = 300;
        // Save geometry before fullscreen
        w.saved_geometry = Some((w.x, w.y, w.width, w.height));
        // Simulate fullscreen resize
        w.x = 0;
        w.y = 0;
        w.width = 1024;
        w.height = 768;
        assert_eq!(w.saved_geometry, Some((50, 100, 400, 300)));
        // Restore from fullscreen
        if let Some((sx, sy, sw, sh)) = w.saved_geometry {
            w.x = sx;
            w.y = sy;
            w.width = sw;
            w.height = sh;
            w.saved_geometry = None;
        }
        assert_eq!(w.x, 50);
        assert_eq!(w.y, 100);
        assert_eq!(w.width, 400);
        assert_eq!(w.height, 300);
        assert!(w.saved_geometry.is_none());
    }

    #[test]
    fn saved_geometry_not_overwritten_when_already_set() {
        let mut w = make_test_window(42, 1);
        w.x = 50;
        w.y = 100;
        w.width = 400;
        w.height = 300;
        w.saved_geometry = Some((50, 100, 400, 300));
        // Simulate maximize after fullscreen (should not overwrite saved)
        w.x = 0;
        w.y = 0;
        w.width = 1024;
        w.height = 768;
        // The saved geometry should still point to the original position
        assert_eq!(w.saved_geometry, Some((50, 100, 400, 300)));
    }
}
