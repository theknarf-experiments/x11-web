//! Pixel-buffer codec shared between the X11, Wayland and macOS sidecars.
//!
//! Encodes captured RGBA frames into the wire format the backend
//! ships over the WebRTC media DataChannel. The frontend decodes via
//! `createImageBitmap`, which natively understands WebP (so the
//! browser does the work, hardware-accelerated where possible).
//!
//! # Which codec, and why the choice has to be per-frame
//!
//! Both WebP modes live in the same RIFF/WEBP container and differ
//! only in the chunk they carry (`VP8L` for lossless, `VP8 ` for
//! lossy), so the wire needs **no codec discriminator** and the
//! frontend needs no change: `createImageBitmap` sniffs the chunk.
//! That leaves the choice entirely to us — and it matters a lot,
//! because the two encoders have completely different cost curves:
//!
//! * **Lossy is nearly content-independent**, ~31–140 ms per
//!   megapixel whatever you feed it.
//! * **Lossless is bimodal.** On flat UI it is 4–22 ms/Mpx (a solid
//!   fill is 4.5, an xterm 18, a GIMP window 22). The moment content
//!   becomes colour-dense it collapses to 147–366 ms/Mpx (a GTK
//!   dialog 147, a gradient 233, a photo 306, a Firefox window 366).
//!
//! So neither fixed choice is defensible. A hardcoded lossless
//! encoder pays 233 ms for one full Firefox repaint while *also*
//! producing a payload 2.1× the size of the lossy one. A hardcoded
//! lossy encoder is 6.8× slower and 19× bigger than lossless on a
//! solid fill, which is what most X11 damage rects actually are.
//!
//! [`encode_rgba_auto`] therefore probes the content and picks. See
//! [`select_codec`] for the probe and the measured justification of
//! its threshold; `tests/codec_bench.rs` regenerates the table.

/// Which WebP mode to encode a particular buffer with.
///
/// Split out from [`encode_rgba_auto`] so the *decision* can be unit
/// tested directly, without asserting on encoded bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec {
    /// WebP-lossless (`VP8L`). Bit-exact, and both faster and smaller
    /// than lossy on flat, low-colour-count content.
    Lossless,
    /// WebP-lossy (`VP8 `) at the given quality (0.0 – 100.0).
    Lossy(f32),
}

/// Quality [`select_codec`] uses when it picks lossy.
///
/// Fidelity is safe here for two independent reasons, both measured
/// rather than assumed:
///
/// * The content the e2e suite screenshot-compares never reaches this
///   path at all. Every xeyes, xterm, vim, xclock, xlogo and xmessage
///   capture in the corpus probes at 4–27 distinct colours, well
///   under [`AUTO_DISTINCT_CAP`], so the selector picks lossless and
///   those comparisons stay bit-exact. See `tests/codec_bench.rs`.
/// * Where it *is* picked, alpha survives exactly — libwebp stores
///   the alpha plane losslessly — so a SHAPE-clipped window cannot
///   grow a halo under the frontend's source-over blit. Asserted by
///   `lossy_preserves_alpha_exactly_for_shape_clipped_windows`.
///
/// The residual risk q90 accepts is antialiased light-on-dark
/// *coloured* text, where 4:2:0 chroma subsampling is weakest. That
/// is also where the selector errs (it picks lossy on `synth-aa-text`
/// where lossless narrowly wins), but the error is bounded at 2.5× /
/// 11.5 ms, so it is a cost question, not a correctness one.
pub const AUTO_LOSSY_QUALITY: f32 = 90.0;

/// Distinct-colour count at or below which [`select_codec`] picks
/// lossless.
///
/// Measured by `tests/codec_bench.rs` over 313 encode cases cut from
/// real captured X11/GTK windows (the e2e snapshot corpus), scored
/// against a per-case oracle with cost = `encode_ms + bytes / 10240`:
///
/// | policy | regret | worst single encode |
/// |---|---|---|
/// | always lossless (the old X11/Wayland behaviour) | 650.64 ms | 249.27 ms |
/// | always lossy q90 (the old macOS behaviour) | 99.82 ms | 33.28 ms |
/// | **this rule** | **2.60 ms** | **31.65 ms** |
///
/// The threshold sits on a shallow minimum — caps of 24 / 32 / **40**
/// / 64 / 96 score 50.8 / 44.8 / **29.3** / 38.3 / 48.0 ms of regret
/// over the full 381-case corpus — so treat it as "a few dozen
/// colours", not as a constant tuned to three digits. It degrades
/// gracefully: on real captures 10 of 313 cases are misclassified,
/// worst 1.96× / 1.02 ms, against the 200–250 ms tail it removes.
///
/// Re-run the benchmark rather than trusting these:
/// `cargo test -p x11-web-pixel-codec --release -- --ignored --nocapture`
pub const AUTO_DISTINCT_CAP: u32 = 40;

/// Upper bound on how many pixels [`probe_distinct_colors`] looks at.
const PROBE_MAX_SAMPLES: usize = 2048;

/// Slots in the probe's open-addressed table. A power of two so the
/// wrap is a mask, and comfortably larger than any cap we would use,
/// so the table can never fill (which would spin the linear probe).
const PROBE_TABLE_SLOTS: usize = 1024;

/// Count distinct RGBA values over a strided subsample of `rgba`,
/// saturating at `cap + 1`.
///
/// This is the whole content classifier. Distinct-colour count is
/// what predicts which of libwebp's two lossless regimes a buffer
/// will hit, which is why a probe this crude works at all — see the
/// module docs.
///
/// Cost is 0.0001–0.0035 ms: 0.003% of the encode it gates on a
/// full-frame Firefox window and, worst case over 381 measured
/// buffers, 3.6% of the encode of a 64×64 crop. The early exit is
/// what makes it free where it matters — colour-dense input trips
/// the cap after ~40 samples, in 0.0001 ms, and that is precisely the
/// case where picking lossless would have cost 200–250 ms.
///
/// Allocation-free (a fixed 5 KB of stack) and pure integer
/// arithmetic, so the count is bit-identical across platforms.
///
/// Degenerate inputs are handled rather than guarded against by the
/// caller: an empty buffer counts 0, a 1×1 buffer counts 1, and a
/// `width`/`height` that overstates `rgba` is clamped to the bytes
/// actually present rather than panicking.
pub fn probe_distinct_colors(rgba: &[u8], width: u32, height: u32, cap: u32) -> u32 {
    // Never let the caller ask for a cap the table cannot hold: the
    // linear probe below assumes it always finds a free slot.
    let cap = cap.min(PROBE_TABLE_SLOTS as u32 - 2);

    let declared = (width as usize).saturating_mul(height as usize);
    let pixels = declared.min(rgba.len() / 4);
    if pixels == 0 {
        return 0;
    }

    // Sample at most PROBE_MAX_SAMPLES pixels. For any rect of 2048
    // pixels or fewer (which is most X11 damage) the stride is 1 and
    // the count is exact.
    let stride = pixels.div_ceil(PROBE_MAX_SAMPLES);

    let mut keys = [0u32; PROBE_TABLE_SLOTS];
    let mut used = [false; PROBE_TABLE_SLOTS];
    let mut distinct = 0u32;

    let mut i = 0usize;
    while i < pixels {
        let o = i * 4;
        let key = u32::from_le_bytes([rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]);
        // Fibonacci hash: the top 10 bits of the scrambled key index
        // the 1024-slot table directly.
        let mut slot = (key.wrapping_mul(0x9E37_79B1) >> 22) as usize;
        loop {
            if !used[slot] {
                used[slot] = true;
                keys[slot] = key;
                distinct += 1;
                if distinct > cap {
                    return distinct;
                }
                break;
            }
            if keys[slot] == key {
                break;
            }
            slot = (slot + 1) % PROBE_TABLE_SLOTS;
        }
        i += stride;
    }

    distinct
}

/// Pick the codec for a buffer by probing its content.
///
/// Lossless when the buffer holds at most [`AUTO_DISTINCT_CAP`]
/// distinct colours in the subsample, lossy q[`AUTO_LOSSY_QUALITY`]
/// otherwise.
///
/// There is deliberately **no pixel-count bypass**. Routing small
/// rects straight to lossless was measured and is *worse* than the
/// plain rule, because a tiny rect cut out of a photographic or
/// Firefox window genuinely wants lossy — and the probe is cheap
/// enough on small rects that there is nothing to save by skipping
/// it. The crop grid in `tests/codec_bench.rs` is what keeps that
/// honest: 356 of its 381 cases are sub-window rects.
pub fn select_codec(rgba: &[u8], width: u32, height: u32) -> Codec {
    if probe_distinct_colors(rgba, width, height, AUTO_DISTINCT_CAP) <= AUTO_DISTINCT_CAP {
        Codec::Lossless
    } else {
        Codec::Lossy(AUTO_LOSSY_QUALITY)
    }
}

/// Encode an RGBA pixel buffer with the codec [`select_codec`] picks
/// for its content. This is the entry point every capture path should
/// use unless it has a measured reason not to.
///
/// Output is a WebP file in either mode, so callers, the wire and the
/// frontend all stay codec-agnostic — see the module docs.
pub fn encode_rgba_auto(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    encode_rgba(rgba, width, height, select_codec(rgba, width, height))
}

/// Encode with an explicitly chosen codec. Useful when the caller has
/// already probed (or has a policy reason to override).
pub fn encode_rgba(rgba: &[u8], width: u32, height: u32, codec: Codec) -> Vec<u8> {
    match codec {
        Codec::Lossless => encode_rgba_lossless(rgba, width, height),
        Codec::Lossy(quality) => encode_rgba_lossy(rgba, width, height, quality),
    }
}

/// Encode an RGBA pixel buffer (`width * height * 4` bytes,
/// row-major, [R, G, B, A] per pixel) into WebP-lossless bytes ready
/// to put into a `DisplayUpdate::PutImage.data` field.
///
/// Bit-exact, and on flat low-colour content (solid fills, xterm,
/// vim, xeyes — most X11 damage) both *faster and smaller* than
/// lossy. On colour-dense content it is catastrophic: 306 ms/Mpx on a
/// photo and 366 on a Firefox window, versus 31–140 ms/Mpx for lossy.
/// Prefer [`encode_rgba_auto`] unless you know the content is flat.
///
/// Width / height are the image dimensions in pixels; the encoder
/// expects `rgba.len() == width * height * 4`. Caller's responsibility
/// to ensure that — the underlying libwebp call will do its own
/// validation and emit an empty buffer on mismatch.
pub fn encode_rgba_lossless(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(rgba, width, height);
    encoder.encode_lossless().to_vec()
}

/// Encode an RGBA pixel buffer into WebP-lossy bytes at the given
/// quality (0.0 – 100.0; 90 is the UI default — see
/// [`AUTO_LOSSY_QUALITY`]).
///
/// **Lossy is not universally cheaper.** Its cost is roughly
/// content-independent, so it is a huge win on colour-dense content
/// (a real 921×691 Firefox window measures 29.7 ms / 25052 B lossy
/// against 233.1 ms / 52784 B lossless — 7.8× faster *and* 2.1×
/// smaller) and a clear loss on flat content (a 1024×768 solid fill
/// measures 24.2 ms / 1472 B lossy against 3.6 ms / 76 B lossless —
/// 6.8× slower and 19× bigger). An earlier version of this docstring
/// claimed lossy was "~5-10× faster than lossless and smaller"
/// unconditionally; that is backwards for exactly the flat UI content
/// this project mostly ships, and it is why both Linux sidecars were
/// hardcoded the other way.
///
/// Alpha survives lossy exactly — libwebp stores the alpha plane
/// losslessly — so SHAPE-clipped windows are safe.
///
/// Use [`encode_rgba_auto`] rather than choosing by hand.
pub fn encode_rgba_lossy(rgba: &[u8], width: u32, height: u32, quality: f32) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(rgba, width, height);
    encoder.encode(quality).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which WebP mode a buffer actually carries, read out of the
    /// RIFF container: the bitstream chunk is `VP8L` for lossless and
    /// `VP8 ` for lossy. This is what the browser sniffs, and it is
    /// the only way to assert that a *selection* really reached the
    /// encoder.
    ///
    /// Note the container is not always simple: a lossy image **with
    /// an alpha channel** is wrapped in the extended `VP8X` form,
    /// which carries a separate `ALPH` chunk before the `VP8 ` one.
    /// So this has to walk chunks rather than read bytes 12..16.
    fn webp_mode(bytes: &[u8]) -> &'static str {
        assert!(bytes.len() > 20, "not a webp file: {} bytes", bytes.len());
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        let mut off = 12;
        while off + 8 <= bytes.len() {
            let id = &bytes[off..off + 4];
            let size = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()) as usize;
            match id {
                b"VP8L" => return "lossless",
                b"VP8 " => return "lossy",
                // VP8X is a 10-byte header followed by sibling
                // chunks, and ALPH/ICCP/EXIF just get skipped. Chunk
                // payloads are padded to an even length.
                _ => off += 8 + size + (size & 1),
            }
        }
        panic!("no VP8/VP8L bitstream chunk in a {}-byte file", bytes.len());
    }

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Vec<u8> {
        px.iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect()
    }

    /// Text-like: a handful of colours (background, foreground, and a
    /// few antialiasing steps) laid out in glyph-ish runs. This is
    /// what an xterm or a vim buffer actually looks like.
    fn text_like(width: u32, height: u32) -> Vec<u8> {
        const PALETTE: [[u8; 4]; 5] = [
            [0x1e, 0x1e, 0x1e, 0xff],
            [0xd0, 0xd0, 0xd0, 0xff],
            [0x80, 0x80, 0x80, 0xff],
            [0x50, 0x50, 0x50, 0xff],
            [0xa0, 0xa0, 0xa0, 0xff],
        ];
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Glyph cells 6x13, with a few strokes inside.
                let inside = (x % 6) < 4 && (y % 13) < 9 && ((x / 6) + (y / 13)) % 3 != 0;
                let idx = if inside {
                    1 + ((x + y) % 4) as usize
                } else {
                    0
                };
                out.extend_from_slice(&PALETTE[idx]);
            }
        }
        out
    }

    /// Photographic: smoothly varying, colour-dense, plus enough
    /// per-pixel jitter that no two neighbours match. Stands in for
    /// the Firefox / GIMP-canvas / photo cases.
    fn photographic(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        let mut seed = 0x1234_5678u32;
        for y in 0..height {
            for x in 0..width {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let n = (seed >> 24) as u8 / 8;
                out.push(((x * 255 / width.max(1)) as u8).wrapping_add(n));
                out.push(((y * 255 / height.max(1)) as u8).wrapping_add(n));
                out.push((((x + y) * 255 / (width + height).max(1)) as u8).wrapping_add(n));
                out.push(0xff);
            }
        }
        out
    }

    #[test]
    fn flat_colour_selects_lossless_and_beats_lossy_on_both_axes() {
        let px = solid(1024, 768, [0x30, 0x40, 0x50, 0xff]);
        assert_eq!(probe_distinct_colors(&px, 1024, 768, AUTO_DISTINCT_CAP), 1);
        assert_eq!(select_codec(&px, 1024, 768), Codec::Lossless);
        assert_eq!(webp_mode(&encode_rgba_auto(&px, 1024, 768)), "lossless");

        // The claim the old docstring got backwards, asserted: on a
        // flat fill lossless is smaller. (Time is not asserted — it
        // is not reproducible in CI — but it is measured in
        // tests/codec_bench.rs.)
        let lossless = encode_rgba_lossless(&px, 1024, 768);
        let lossy = encode_rgba_lossy(&px, 1024, 768, AUTO_LOSSY_QUALITY);
        assert!(
            lossless.len() * 4 < lossy.len(),
            "expected lossless to be far smaller on a solid fill, got {} vs {}",
            lossless.len(),
            lossy.len()
        );
    }

    #[test]
    fn text_like_selects_lossless() {
        let px = text_like(400, 260);
        // Five palette entries, and the probe must find them all.
        assert_eq!(probe_distinct_colors(&px, 400, 260, AUTO_DISTINCT_CAP), 5);
        assert_eq!(select_codec(&px, 400, 260), Codec::Lossless);
        assert_eq!(webp_mode(&encode_rgba_auto(&px, 400, 260)), "lossless");
    }

    #[test]
    fn photographic_selects_lossy_and_probe_early_exits() {
        let px = photographic(640, 480);
        // Saturates at cap + 1 rather than counting on: that is the
        // early exit, and it is what keeps the probe free on exactly
        // the input where picking lossless would cost 300 ms.
        assert_eq!(
            probe_distinct_colors(&px, 640, 480, AUTO_DISTINCT_CAP),
            AUTO_DISTINCT_CAP + 1
        );
        assert_eq!(
            select_codec(&px, 640, 480),
            Codec::Lossy(AUTO_LOSSY_QUALITY)
        );
        assert_eq!(webp_mode(&encode_rgba_auto(&px, 640, 480)), "lossy");
    }

    #[test]
    fn small_rect_is_sampled_exhaustively() {
        // 40x20 = 800 pixels, under PROBE_MAX_SAMPLES, so stride is 1
        // and the count is exact rather than estimated. Tiny damage
        // rects dominate X11 traffic by count, so this is the case
        // that has to be right.
        let (w, h) = (40u32, 20u32);
        let mut px = solid(w, h, [0x10, 0x20, 0x30, 0xff]);
        // Give it exactly 7 more distinct colours, one pixel each,
        // including the very last pixel — an off-by-one in the stride
        // walk would miss it.
        let n = (w * h) as usize;
        for (k, i) in [1usize, 13, 77, 300, 511, 799, n - 1].iter().enumerate() {
            px[i * 4] = 0x80 + k as u8;
        }
        assert_eq!(probe_distinct_colors(&px, w, h, AUTO_DISTINCT_CAP), 7);
        assert_eq!(select_codec(&px, w, h), Codec::Lossless);

        // And a small rect cut from photographic content still goes
        // lossy — there is no size bypass, deliberately.
        let photo_rect = photographic(w, h);
        assert_eq!(
            select_codec(&photo_rect, w, h),
            Codec::Lossy(AUTO_LOSSY_QUALITY)
        );
    }

    #[test]
    fn degenerate_inputs_short_circuit_to_lossless() {
        // 1x1.
        let one = [0x11u8, 0x22, 0x33, 0xff];
        assert_eq!(probe_distinct_colors(&one, 1, 1, AUTO_DISTINCT_CAP), 1);
        assert_eq!(select_codec(&one, 1, 1), Codec::Lossless);
        assert_eq!(webp_mode(&encode_rgba_auto(&one, 1, 1)), "lossless");

        // Empty buffer, and a zero dimension: no panic, no encode
        // choice that depends on uninitialised state.
        assert_eq!(probe_distinct_colors(&[], 0, 0, AUTO_DISTINCT_CAP), 0);
        assert_eq!(select_codec(&[], 0, 0), Codec::Lossless);
        assert_eq!(probe_distinct_colors(&one, 1, 0, AUTO_DISTINCT_CAP), 0);

        // A single-pixel-wide column: width < 2 must not trip any
        // adjacent-pixel arithmetic.
        let column = solid(1, 512, [0, 0, 0, 0xff]);
        assert_eq!(probe_distinct_colors(&column, 1, 512, AUTO_DISTINCT_CAP), 1);
        assert_eq!(select_codec(&column, 1, 512), Codec::Lossless);
    }

    #[test]
    fn probe_clamps_dimensions_that_overstate_the_buffer() {
        // A truncated buffer must be read as far as it goes, not
        // panic and not read past the end.
        let px = solid(8, 8, [1, 2, 3, 4]);
        assert_eq!(probe_distinct_colors(&px, 1024, 1024, AUTO_DISTINCT_CAP), 1);
    }

    #[test]
    fn distinct_count_is_exact_up_to_the_cap_then_saturates() {
        // 64 pixels, each a distinct colour: exact below the cap...
        let mut px = Vec::new();
        for i in 0..30u32 {
            px.extend_from_slice(&i.to_le_bytes());
        }
        assert_eq!(probe_distinct_colors(&px, 30, 1, AUTO_DISTINCT_CAP), 30);

        // ...and saturating above it.
        let mut px = Vec::new();
        for i in 0..1000u32 {
            px.extend_from_slice(&i.to_le_bytes());
        }
        assert_eq!(
            probe_distinct_colors(&px, 1000, 1, AUTO_DISTINCT_CAP),
            AUTO_DISTINCT_CAP + 1
        );
        // A cap larger than the table is clamped rather than spinning
        // the open-addressed probe forever.
        assert!(probe_distinct_colors(&px, 1000, 1, u32::MAX) < PROBE_TABLE_SLOTS as u32);
    }

    #[test]
    fn lossy_preserves_alpha_exactly_for_shape_clipped_windows() {
        // xeyes/xclock/xlogo are SHAPE-clipped: sync_flush.rs zeroes
        // masked pixels to (0,0,0,0) and the frontend blits
        // source-over. If lossy smeared alpha, a shaped window would
        // grow a rectangular halo. libwebp stores the alpha plane
        // losslessly; assert that rather than trusting it.
        let (w, h) = (64u32, 64u32);
        let mut px = photographic(w, h);
        for y in 0..h {
            for x in 0..w {
                let inside = (x as i32 - 32).pow(2) + (y as i32 - 32).pow(2) < 20 * 20;
                if !inside {
                    let o = ((y * w + x) * 4) as usize;
                    px[o..o + 4].copy_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        let encoded = encode_rgba_lossy(&px, w, h, AUTO_LOSSY_QUALITY);
        assert_eq!(webp_mode(&encoded), "lossy");
        let decoded = webp::Decoder::new(&encoded)
            .decode()
            .expect("lossy webp decodes");
        assert_eq!((decoded.width(), decoded.height()), (w, h));
        assert!(
            decoded.is_alpha(),
            "a shaped window must keep its alpha channel through lossy"
        );
        let worst = px
            .chunks_exact(4)
            .zip(decoded.chunks_exact(4))
            .map(|(a, b)| a[3].abs_diff(b[3]))
            .max()
            .unwrap();
        assert_eq!(worst, 0, "lossy must not perturb the alpha plane");
    }

    #[test]
    fn auto_round_trips_through_a_real_decoder_in_both_modes() {
        for (label, px, w, h, want) in [
            (
                "flat",
                solid(120, 90, [9, 9, 9, 255]),
                120u32,
                90u32,
                "lossless",
            ),
            ("photo", photographic(120, 90), 120, 90, "lossy"),
        ] {
            let encoded = encode_rgba_auto(&px, w, h);
            assert_eq!(webp_mode(&encoded), want, "{label}");
            let decoded = webp::Decoder::new(&encoded)
                .decode()
                .unwrap_or_else(|| panic!("{label} decodes"));
            assert_eq!((decoded.width(), decoded.height()), (w, h), "{label}");
        }
    }
}
