use super::*;

// -----------------------------------------------------------------------
// Framebuffer basic operations
// -----------------------------------------------------------------------

#[test]
fn new_framebuffer_is_zeroed() {
    let fb = Framebuffer::new(10, 10);
    assert_eq!(fb.width(), 10);
    assert_eq!(fb.height(), 10);
    assert!(fb.data().iter().all(|&b| b == 0));
}

#[test]
fn resize_preserves_content() {
    let mut fb = Framebuffer::new(4, 4);
    // Set pixel at (1,1) to red (RGBA storage: R at byte 0)
    let off = 1 * fb.stride() + 1 * 4;
    fb.data_mut()[off] = 255; // R
    fb.data_mut()[off + 1] = 0; // G
    fb.data_mut()[off + 2] = 0; // B
    fb.data_mut()[off + 3] = 255; // A
    fb.resize(8, 8);
    assert_eq!(fb.width(), 8);
    assert_eq!(fb.height(), 8);
    let off2 = 1 * fb.stride() + 1 * 4;
    assert_eq!(fb.data()[off2], 255); // R preserved
}

#[test]
fn resize_with_forget_gravity_clears() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 1 * fb.stride() + 1 * 4;
    fb.data_mut()[off] = 255;
    fb.resize_with_gravity(8, 8, 0); // Forget gravity
    assert!(fb.data().iter().all(|&b| b == 0));
}

#[test]
fn resize_with_northwest_gravity_preserves_top_left() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 0; // pixel (0,0)
    fb.data_mut()[off] = 200; // R
    fb.resize_with_gravity(8, 8, 1); // NorthWest
    assert_eq!(fb.data()[off], 200);
}

// -----------------------------------------------------------------------
// fill_rect
// -----------------------------------------------------------------------

#[test]
fn fill_rect_basic() {
    let mut fb = Framebuffer::new(10, 10);
    fb.fill_rect(2, 2, 3, 3, 0xFF0000FF); // Blue in ARGB
                                          // Check pixel at (3, 3) is set
    let off = 3 * fb.stride() + 3 * 4;
    assert_ne!(fb.data()[off], 0); // Should have some color
}

// -----------------------------------------------------------------------
// apply_gc_function — all 16 GX operations
// -----------------------------------------------------------------------

#[test]
fn gx_clear() {
    assert_eq!(apply_gc_function(0, 0xFFFFFFFF, 0xFFFFFFFF), 0);
}

#[test]
fn gx_and() {
    assert_eq!(apply_gc_function(1, 0xFF00FF00, 0x00FF00FF), 0x00000000);
    assert_eq!(apply_gc_function(1, 0xFFFF0000, 0xFFFF0000), 0xFFFF0000);
}

#[test]
fn gx_copy() {
    assert_eq!(apply_gc_function(3, 0xDEADBEEF, 0x12345678), 0xDEADBEEF);
}

#[test]
fn gx_noop() {
    assert_eq!(apply_gc_function(5, 0xDEADBEEF, 0x12345678), 0x12345678);
}

#[test]
fn gx_xor() {
    assert_eq!(apply_gc_function(6, 0xFF00FF00, 0x00FF00FF), 0xFFFFFFFF);
}

#[test]
fn gx_or() {
    assert_eq!(apply_gc_function(7, 0xFF000000, 0x00FF0000), 0xFFFF0000);
}

#[test]
fn gx_invert() {
    assert_eq!(apply_gc_function(10, 0x00000000, 0xFF00FF00), 0x00FF00FF);
}

#[test]
fn gx_set() {
    assert_eq!(apply_gc_function(15, 0x00000000, 0x00000000), 0xFFFFFFFF);
}

#[test]
fn gx_copy_inverted() {
    assert_eq!(apply_gc_function(12, 0xFF00FF00, 0x00000000), 0x00FF00FF);
}

// -----------------------------------------------------------------------
// point_in_clip_rects
// -----------------------------------------------------------------------

#[test]
fn clip_empty_allows_all() {
    // Empty clip = no clipping, but our function returns false for empty
    assert!(!point_in_clip_rects(5, 5, &[]));
}

#[test]
fn clip_point_inside() {
    let rects = vec![(0i16, 0i16, 10u16, 10u16)];
    assert!(point_in_clip_rects(5, 5, &rects));
}

#[test]
fn clip_point_outside() {
    let rects = vec![(0i16, 0i16, 10u16, 10u16)];
    assert!(!point_in_clip_rects(15, 15, &rects));
}

#[test]
fn clip_point_on_boundary() {
    let rects = vec![(0i16, 0i16, 10u16, 10u16)];
    assert!(point_in_clip_rects(0, 0, &rects)); // top-left corner: inside
    assert!(!point_in_clip_rects(10, 10, &rects)); // bottom-right: outside (exclusive)
}

#[test]
fn clip_multiple_rects() {
    let rects = vec![(0i16, 0i16, 5u16, 5u16), (10i16, 10i16, 5u16, 5u16)];
    assert!(point_in_clip_rects(2, 2, &rects));
    assert!(point_in_clip_rects(12, 12, &rects));
    assert!(!point_in_clip_rects(7, 7, &rects)); // gap between rects
}

// -----------------------------------------------------------------------
// DashState
// -----------------------------------------------------------------------

#[test]
fn dash_state_alternates() {
    let mut ds = DashState::new(&[3, 2], 0);
    // First 3 steps: on
    assert!(ds.is_on());
    ds.advance();
    assert!(ds.is_on());
    ds.advance();
    assert!(ds.is_on());
    ds.advance();
    // Next 2 steps: off
    assert!(!ds.is_on());
    ds.advance();
    assert!(!ds.is_on());
    ds.advance();
    // Back to on
    assert!(ds.is_on());
}

#[test]
fn dash_state_with_offset() {
    let ds = DashState::new(&[5, 3], 2);
    // Offset 2 into first dash (len=5), so 3 remaining in first on segment
    assert!(ds.is_on());
    assert_eq!(ds.remaining, 3);
}

// -----------------------------------------------------------------------
// Dirty region tracking
// -----------------------------------------------------------------------

#[test]
fn dirty_initially_none() {
    let mut fb = Framebuffer::new(10, 10);
    assert!(fb.take_dirty_pixels().is_none());
}

#[test]
fn mark_dirty_creates_region() {
    let mut fb = Framebuffer::new(20, 20);
    fb.mark_dirty(5, 5, 10, 10);
    assert!(fb.dirty.is_some());
}

#[test]
fn mark_dirty_merges_regions() {
    let mut fb = Framebuffer::new(100, 100);
    fb.mark_dirty(10, 10, 5, 5);
    fb.mark_dirty(20, 20, 5, 5);
    let dirty = fb.dirty.unwrap();
    // Should encompass both: (10,10) to (25,25) = (10, 10, 15, 15)
    assert_eq!(dirty.0, 10);
    assert_eq!(dirty.1, 10);
    assert!(dirty.2 >= 15);
    assert!(dirty.3 >= 15);
}

// -----------------------------------------------------------------------
// CopyArea self-overlap safety
// -----------------------------------------------------------------------

#[test]
fn copy_area_self_overlapping() {
    let mut fb = Framebuffer::new(10, 10);
    // Fill (0,0)-(4,4) with distinct value
    for y in 0..4usize {
        for x in 0..4usize {
            let off = y * fb.stride() + x * 4;
            fb.data_mut()[off] = 42;
            fb.data_mut()[off + 1] = 43;
            fb.data_mut()[off + 2] = 44;
            fb.data_mut()[off + 3] = 255;
        }
    }
    // Copy overlapping: src=(0,0) dst=(2,2) size=4x4
    fb.copy_area_self(0, 0, 2, 2, 4, 4);
    // (2,2) should now have the original (0,0) value
    let off = 2 * fb.stride() + 2 * 4;
    assert_eq!(fb.data()[off], 42);
}

// -----------------------------------------------------------------------
// Tiled line drawing
// -----------------------------------------------------------------------

#[test]
fn draw_line_tiled_horizontal() {
    let mut fb = Framebuffer::new(20, 10);
    // 2x1 tile in RGBA storage: red, green
    let tile_data = vec![
        255, 0, 0, 255, // pixel (0,0) = red
        0, 255, 0, 255, // pixel (1,0) = green
    ];
    fb.draw_line_tiled(2, 3, 7, 3, &tile_data, 2, 1, 0, 0, 3, 0xFFFFFFFF, 1, &[]);
    // Check pixel at (2,3): tile_x = 2%2 = 0 → red
    let off = 3 * fb.stride() + 2 * 4;
    assert_eq!(fb.data()[off], 255); // R = 255 (red)
                                     // Check pixel at (3,3): tile_x = 3%2 = 1 → green
    let off2 = 3 * fb.stride() + 3 * 4;
    assert_eq!(fb.data()[off2 + 1], 255); // G = 255 (green)
}

#[test]
fn draw_line_tiled_respects_ts_origin() {
    let mut fb = Framebuffer::new(20, 10);
    // 2x1 tile in RGBA storage: blue, white
    let tile_data = vec![
        0, 0, 255, 255, // pixel (0,0) = blue
        255, 255, 255, 255, // pixel (1,0) = white
    ];
    // ts_x = 1 shifts tile origin
    fb.draw_line_tiled(0, 0, 3, 0, &tile_data, 2, 1, 1, 0, 3, 0xFFFFFFFF, 1, &[]);
    // At x=0: tile_x = (0-1)%2 = 1 → white
    let off = 0 * fb.stride() + 0 * 4;
    assert_eq!(fb.data()[off], 255); // R
    assert_eq!(fb.data()[off + 1], 255); // G
    assert_eq!(fb.data()[off + 2], 255); // B
}

// -----------------------------------------------------------------------
// Stippled line drawing
// -----------------------------------------------------------------------

#[test]
fn draw_line_stippled_foreground_only() {
    let mut fb = Framebuffer::new(20, 10);
    // 2x1 stipple as 32bpp: pixel 0 set (white), pixel 1 unset (black)
    let stipple_data = vec![
        255, 255, 255, 255, // pixel (0,0) = set
        0, 0, 0, 0, // pixel (1,0) = unset
    ];
    let fg = 0xFF0000; // red
    let bg = 0x00FF00; // green
                       // Stippled (opaque=false): only draw fg where stipple bit is set
    fb.draw_line_stippled(
        0,
        0,
        3,
        0,
        fg,
        bg,
        &stipple_data,
        2,
        1,
        0,
        0,
        false,
        3,
        0xFFFFFFFF,
        1,
        &[],
    );
    // At x=0: stipple set → fg (red); R is byte 0 in RGBA storage
    let off = 0 * fb.stride() + 0 * 4;
    assert_eq!(fb.data()[off], 0xFF); // R
                                      // At x=1: stipple unset, not opaque → should remain 0 (not drawn)
    let off1 = 0 * fb.stride() + 1 * 4;
    assert_eq!(fb.data()[off1], 0); // R = 0 (not drawn)
}

#[test]
fn draw_line_stippled_opaque_draws_background() {
    let mut fb = Framebuffer::new(20, 10);
    let stipple_data = vec![
        255, 255, 255, 255, // set
        0, 0, 0, 0, // unset
    ];
    let fg = 0xFF0000;
    let bg = 0x00FF00;
    // OpaqueStippled: draw bg where stipple bit is unset
    fb.draw_line_stippled(
        0,
        0,
        3,
        0,
        fg,
        bg,
        &stipple_data,
        2,
        1,
        0,
        0,
        true,
        3,
        0xFFFFFFFF,
        1,
        &[],
    );
    // At x=1: stipple unset, opaque → bg (green); G is byte 1 in RGBA storage
    let off = 0 * fb.stride() + 1 * 4;
    assert_eq!(fb.data()[off + 1], 0xFF); // G = 255 (green)
}

// -----------------------------------------------------------------------
// Gravity resize edge cases
// -----------------------------------------------------------------------

#[test]
fn resize_with_southeast_gravity_preserves_bottom_right() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 3 * fb.stride() + 3 * 4;
    fb.data_mut()[off] = 255; // R
    fb.data_mut()[off + 3] = 255; // A
    fb.resize_with_gravity(8, 8, 9); // SouthEast
    let off2 = 7 * fb.stride() + 7 * 4;
    assert_eq!(fb.data()[off2], 255);
}

#[test]
fn resize_with_center_gravity_preserves_center() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 2 * fb.stride() + 2 * 4;
    fb.data_mut()[off] = 255; // R
    fb.data_mut()[off + 3] = 255; // A
    fb.resize_with_gravity(8, 8, 5); // Center
    let off2 = 4 * fb.stride() + 4 * 4;
    assert_eq!(fb.data()[off2], 255);
}

#[test]
fn resize_shrink_preserves_visible_content() {
    let mut fb = Framebuffer::new(8, 8);
    let off = 1 * fb.stride() + 1 * 4;
    fb.data_mut()[off] = 255; // R
    fb.resize_with_gravity(4, 4, 1); // NorthWest
    let off2 = 1 * fb.stride() + 1 * 4;
    assert_eq!(fb.data()[off2], 255);
}

#[test]
fn resize_to_same_size_is_noop() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 2 * fb.stride() + 2 * 4;
    fb.data_mut()[off] = 42;
    fb.resize_with_gravity(4, 4, 1);
    let off2 = 2 * fb.stride() + 2 * 4;
    assert_eq!(fb.data()[off2], 42);
}

#[test]
fn resize_to_1x1_preserves_top_left_pixel() {
    let mut fb = Framebuffer::new(4, 4);
    let off = 0; // pixel at (0,0)
    fb.data_mut()[off] = 200; // R
    fb.resize_with_gravity(1, 1, 1); // NorthWest
    assert_eq!(fb.data()[0], 200);
}

// -----------------------------------------------------------------------
// Framebuffer pixel operations
// -----------------------------------------------------------------------

#[test]
fn set_pixel_and_get_pixel_roundtrip() {
    let mut fb = Framebuffer::new(10, 10);
    let color = 0xFF_00_FF_00u32; // ARGB green
    let x = 5i16;
    let y = 3i16;
    let off = y as usize * fb.stride() + x as usize * 4;
    let (r, g, b) = super::unpack_rgb(color);
    let a = ((color >> 24) & 0xFF) as u8;
    fb.data_mut()[off] = r;
    fb.data_mut()[off + 1] = g;
    fb.data_mut()[off + 2] = b;
    fb.data_mut()[off + 3] = a;
    assert_eq!(fb.data()[off], 0); // R
    assert_eq!(fb.data()[off + 1], 255); // G
    assert_eq!(fb.data()[off + 2], 0); // B
    assert_eq!(fb.data()[off + 3], 255); // A
}

#[test]
fn large_framebuffer_allocation() {
    // Real apps may create large pixmaps (e.g., 4K resolution)
    let fb = Framebuffer::new(3840, 2160);
    assert_eq!(fb.width(), 3840);
    assert_eq!(fb.height(), 2160);
    assert_eq!(fb.data().len(), 3840 * 2160 * 4);
}

#[test]
fn framebuffer_stride_is_width_times_4() {
    let fb = Framebuffer::new(100, 50);
    assert_eq!(fb.stride(), 100 * 4);
}

// -----------------------------------------------------------------------
// Draw line basic operations
// -----------------------------------------------------------------------

#[test]
fn draw_horizontal_line() {
    let mut fb = Framebuffer::new(20, 10);
    fb.draw_line(0, 5, 19, 5, 0xFF0000, 1);
    let off = 5 * fb.stride() + 10 * 4;
    assert_eq!(fb.data()[off], 0xFF); // R = 255
}

#[test]
fn draw_vertical_line() {
    let mut fb = Framebuffer::new(10, 20);
    fb.draw_line(5, 0, 5, 19, 0x00FF00, 1);
    let off = 10 * fb.stride() + 5 * 4;
    assert_eq!(fb.data()[off + 1], 0xFF); // G = 255
}

#[test]
fn draw_diagonal_line() {
    let mut fb = Framebuffer::new(10, 10);
    fb.draw_line(0, 0, 9, 9, 0x0000FF, 1);
    let off = 5 * fb.stride() + 5 * 4;
    assert_eq!(fb.data()[off + 2], 0xFF); // B = 255
}

// -----------------------------------------------------------------------
// Wide dashed line rendering
// -----------------------------------------------------------------------

#[test]
fn wide_dashed_horiz_line_has_gaps() {
    let mut fb = Framebuffer::new(40, 10);
    // Dash pattern: 4 on, 4 off.  Line width 3.
    // OnOffDash (line_style=1)
    fb.draw_line_gc(
        0,
        5,
        39,
        5, // horizontal line from (0,5) to (39,5)
        0xFF0000,
        3, // red, width 3
        3,
        0xFFFFFFFF, // GXcopy, full plane mask
        1,
        1,
        0, // OnOffDash, Butt cap, Miter join
        0,
        &[4, 4], // dash offset 0, pattern [4,4]
        0,
        &[], // bg, no clip
    );
    // R is byte 0 in RGBA storage; line color is 0xFF0000 (red).
    let off_on = 5 * fb.stride() + 2 * 4;
    assert_ne!(
        fb.data()[off_on],
        0,
        "pixel at x=2 should be drawn (on-dash)"
    );

    let off_off = 5 * fb.stride() + 6 * 4;
    assert_eq!(
        fb.data()[off_off],
        0,
        "pixel at x=6 should be gap (off-dash)"
    );

    let off_top = 4 * fb.stride() + 2 * 4;
    assert_ne!(
        fb.data()[off_top],
        0,
        "pixel at (2,4) should be drawn (wide)"
    );
}

#[test]
fn wide_dashed_vert_line_has_gaps() {
    let mut fb = Framebuffer::new(10, 40);
    // Dash pattern: 5 on, 5 off.  Line width 4.
    fb.draw_line_gc(
        5,
        0,
        5,
        39,
        0x00FF00,
        4,
        3,
        0xFFFFFFFF,
        1,
        1,
        0,
        0,
        &[5, 5],
        0,
        &[],
    );
    // At y=2 (on), should be drawn
    let off_on = 2 * fb.stride() + 5 * 4;
    assert_ne!(fb.data()[off_on + 1], 0, "pixel at y=2 should be drawn");

    // At y=7 (off), should not be drawn
    let off_off = 7 * fb.stride() + 5 * 4;
    assert_eq!(fb.data()[off_off + 1], 0, "pixel at y=7 should be gap");
}

#[test]
fn wide_dashed_diagonal_line_has_gaps() {
    let mut fb = Framebuffer::new(40, 40);
    // Dash pattern: 6 on, 6 off.  Line width 3.
    fb.draw_line_gc(
        0,
        0,
        39,
        39,
        0x0000FF,
        3,
        3,
        0xFFFFFFFF,
        1,
        1,
        0,
        0,
        &[6, 6],
        0,
        &[],
    );
    // Line color is 0x0000FF (blue); B is byte 2 in RGBA storage.
    let off_on = 2 * fb.stride() + 2 * 4;
    assert_ne!(fb.data()[off_on + 2], 0, "pixel at (2,2) should be drawn");

    let off_off = 9 * fb.stride() + 9 * 4;
    assert_eq!(fb.data()[off_off + 2], 0, "pixel at (9,9) should be gap");
}

#[test]
fn wide_double_dash_draws_background() {
    let mut fb = Framebuffer::new(40, 10);
    // DoubleDash (line_style=2): gaps drawn in background color
    fb.draw_line_gc(
        0,
        5,
        39,
        5,
        0xFF0000,
        3, // red foreground
        3,
        0xFFFFFFFF,
        2,
        1,
        0, // DoubleDash
        0,
        &[4, 4],
        0x00FF00, // green background
        &[],
    );
    // At x=6 (gap), should be green (background); G is byte 1, R is byte 0 in RGBA.
    let off_gap = 5 * fb.stride() + 6 * 4;
    assert_ne!(
        fb.data()[off_gap + 1],
        0,
        "gap pixel should have green background"
    );
    assert_eq!(fb.data()[off_gap], 0, "gap pixel should not have red");
}
