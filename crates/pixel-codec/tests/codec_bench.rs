//! Encode-cost benchmark for the codec selector — the measurements
//! that justify [`x11_web_pixel_codec::AUTO_DISTINCT_CAP`], kept in
//! the repo so they can be re-run rather than trusted.
//!
//! It is `#[ignore]`d: it encodes a few hundred buffers, several of
//! them full-frame lossless on photographic content at ~300 ms each,
//! so it takes minutes and has no place in the default test run.
//!
//! ```text
//! cargo test -p x11-web-pixel-codec --release -- --ignored --nocapture
//! ```
//!
//! **Run it in `--release`.** A dev build leaves the probe and the
//! crop/synthesis code at `-O0` (only `libwebp-sys` is optimised, via
//! the workspace `[profile.dev.package]` override), which inflates
//! the probe column by two orders of magnitude and makes the
//! probe-vs-encode ratio meaningless.
//!
//! The corpus is deliberately mostly *real*: `e2e/tests/*-snapshots/`
//! holds genuine captured X11/GTK windows (xterm, vim, zenity, xeyes,
//! xclock, xlogo, xmessage, firefox, gimp), which is the content this
//! actually has to be right about. The synthetics (solid, gradient,
//! photo, antialiased text) are there to pin the two extremes and the
//! known-adversarial cases.
//!
//! ## Recorded results — macOS arm64, `--release`, webp 0.3.1
//!
//! Produced by this file on 2026-09-03 (`cargo test -p
//! x11-web-pixel-codec --release -- --ignored --nocapture`, 14.4 s,
//! 381 cases: 25 real captures + 4 synthetics, each at full frame
//! plus a 4x4 crop grid). Full frames only below; `LL` = lossless,
//! `LY` = lossy q90, `dist` = probe count (`40+` = early exit).
//!
//! ```text
//! case                                  LL ms  LL bytes |   LY ms  LY bytes | dist  picked
//! firefox-before-input-darwin  921x691 249.27     62974 |   31.65     39708 |  40+  lossy
//! firefox-canvas-darwin        921x691 233.08     52784 |   29.73     25052 |  40+  lossy
//! gimp-canvas-darwin          1046x814  18.69     10228 |   33.28     12670 |   36  lossless
//! zenity-question-darwin       188x120   4.69      2680 |    1.25      1848 |  40+  lossy
//! zenity-canvas-darwin         164x120   2.89      2012 |    1.05      1184 |  40+  lossy
//! vim-after-save-darwin        366x201   2.03       768 |    3.18      4370 |    6  lossless
//! xeyes-canvas-linux           302x202   1.84       734 |    2.84      2662 |    4  lossless
//! xterm-keyboard-linux         366x201   1.49       632 |    2.91      2404 |    4  lossless
//! xterm-canvas-linux           246x136   0.60       398 |    1.41      1052 |    6  lossless
//! xlogo-canvas-darwin          100x100   0.68       436 |    0.77      1618 |    9  lossless
//! synth-photo                 1024x768 240.49    835478 |  110.34    510386 |  40+  lossy
//! synth-gradient              1024x768 183.58      2418 |   34.64      9760 |  40+  lossy
//! synth-aa-text                640x400  18.91     86306 |   28.78    103250 |  40+  lossy
//! synth-solid                 1024x768   3.57        76 |   24.15      1472 |    1  lossless
//! ```
//!
//! Two rows carry the whole argument. **firefox-canvas**: lossy is
//! 7.8x faster *and* 2.1x smaller at the same time, so the old
//! hardcoded-lossless X11/Wayland path was losing on both axes.
//! **synth-solid**: lossy is 6.8x slower and 19x bigger, which is the
//! measurement that disproves the old `encode_rgba_lossy` docstring.
//!
//! Policy comparison, cost = `encode_ms + bytes / 10240`, regret =
//! excess over a per-case oracle:
//!
//! ```text
//! ALL 381 cases                 regret      total   worst encode   p95
//!   always lossless  (was X11+Wayland)  1611.87   2208.13   249.27   19.06
//!   always lossy q90 (was macOS)         156.90    753.16   110.34    3.78
//!   auto (this crate)                     30.02    626.28   110.34    3.58
//!   oracle                                 0.00    596.26   110.34    3.57
//!
//! REAL CAPTURES ONLY, 313 cases
//!   always lossless                      650.64    872.20   249.27    4.69
//!   always lossy q90                      99.82    321.37    33.28    2.94
//!   auto (this crate)                      2.60    224.15    31.65    1.83
//!   oracle                                 0.00    221.55    31.65    1.83
//! ```
//!
//! On real captured windows that is a **250x regret reduction** over
//! the shipped always-lossless policy and a **7.9x cut in worst-case
//! single-encode latency** (249.27 -> 31.65 ms). 10 of 313 real cases
//! are misclassified, worst 1.96x / 1.02 ms — the 200-250 ms tail is
//! gone entirely, and nothing replaces it.
//!
//! Probe cost, worst case over all 381: **3.63%** of the cheaper
//! encode, on a 64x64 xeyes crop. On the cases that matter it is
//! noise: 0.00081 ms against a 29.73 ms encode for full-frame
//! firefox, i.e. 0.003%, because the early exit fires after ~40
//! samples on colour-dense input.
//!
//! Threshold sweep (regret ms, all cases):
//!
//! ```text
//! cap    8   16   24   32   40*   64   96  128   160    192
//! ms  55.1 53.6 50.8 44.8 29.3* 38.3 48.0 56.5 246.2  469.9
//! ```
//!
//! 40 is the minimum and the curve is shallow either side of it, so
//! the constant is a plateau rather than a knife edge. The cliff
//! above 128 is Firefox and the photo falling into always-lossless.
//!
//! These are macOS numbers; a `rust:1-slim` aarch64 container gave
//! identical winner verdicts with times within ~10%, so the constants
//! port to where the Linux sidecars actually run.

use std::path::{Path, PathBuf};
use std::time::Instant;

use x11_web_pixel_codec::{
    encode_rgba_lossless, encode_rgba_lossy, probe_distinct_colors, select_codec, Codec,
    AUTO_DISTINCT_CAP, AUTO_LOSSY_QUALITY,
};

/// Bytes-per-millisecond the payload is charged at when comparing a
/// codec's time against its size. 10240 B/ms is 10 MB/s, a plausible
/// DataChannel. The ranking of the three policies is unchanged at
/// 2560 B/ms and at infinity (pure latency), so nothing here depends
/// on guessing this number.
const BYTES_PER_MS: f64 = 10240.0;

struct Image {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// Real captured window content vs. a synthetic stressor. Kept
    /// separate in the summary because the synthetics are adversarial
    /// by construction and would otherwise flatter always-lossy.
    real: bool,
}

impl Image {
    fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Image> {
        if x + w > self.width || y + h > self.height {
            return None;
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let o = (((y + row) * self.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[o..o + (w * 4) as usize]);
        }
        Some(Image {
            name: format!("{} [{}x{}@{},{}]", self.name, w, h, x, y),
            width: w,
            height: h,
            rgba,
            real: self.real,
        })
    }
}

/// Min-of-N wall time in milliseconds. Min rather than mean: we want
/// the encoder's cost, not the machine's scheduling noise.
fn time_ms(reps: u32, mut f: impl FnMut()) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    best
}

fn snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/tests/x11-web.spec.ts-snapshots")
}

fn load_real_corpus() -> Vec<Image> {
    let dir = snapshot_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!(
            "!! no snapshot corpus at {}; synthetics only",
            dir.display()
        );
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();

    paths
        .iter()
        .filter_map(|p| {
            let img = image::open(p).ok()?.to_rgba8();
            Some(Image {
                name: p.file_stem()?.to_string_lossy().into_owned(),
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
                real: true,
            })
        })
        .collect()
}

fn synth(name: &str, w: u32, h: u32, mut px: impl FnMut(u32, u32) -> [u8; 4]) -> Image {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&px(x, y));
        }
    }
    Image {
        name: name.to_string(),
        width: w,
        height: h,
        rgba,
        real: false,
    }
}

fn synthetic_corpus() -> Vec<Image> {
    let mut rng = 0x2545_f491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    vec![
        // The case that disproves the old "lossy is always cheaper"
        // docstring.
        synth("synth-solid", 1024, 768, |_, _| [0x2e, 0x34, 0x40, 0xff]),
        // Adversarial: lossless is 24x smaller but 5x slower.
        synth("synth-gradient", 1024, 768, |x, y| {
            [(x / 4) as u8, (y / 3) as u8, ((x + y) / 7) as u8, 0xff]
        }),
        // Photographic: smooth + per-pixel jitter, colour-dense.
        synth("synth-photo", 1024, 768, move |x, y| {
            let n = (next() >> 26) as u8;
            [
                ((x / 4) as u8).wrapping_add(n),
                ((y / 3) as u8).wrapping_add(n),
                (((x ^ y) / 2) as u8).wrapping_add(n),
                0xff,
            ]
        }),
        // Antialiased light-on-dark text: the selector's known weak
        // spot (it picks lossy where lossless narrowly wins), kept in
        // the corpus so a future change cannot make it worse
        // unnoticed.
        synth("synth-aa-text", 640, 400, move |x, y| {
            let stroke = (x % 7) < 3 && (y % 12) < 8;
            if stroke {
                let cov = (next() >> 24) as u8 / 2 + 128;
                [cov, cov / 2 + 96, 255 - cov / 3, 0xff]
            } else {
                [0x18, 0x18, 0x1c, 0xff]
            }
        }),
    ]
}

/// Everything measured for one buffer.
struct Row {
    name: String,
    w: u32,
    h: u32,
    real: bool,
    distinct: u32,
    probe_ms: f64,
    ll_ms: f64,
    ll_bytes: usize,
    ly_ms: f64,
    ly_bytes: usize,
    picked: Codec,
}

impl Row {
    fn cost(ms: f64, bytes: usize) -> f64 {
        ms + bytes as f64 / BYTES_PER_MS
    }
    fn ll_cost(&self) -> f64 {
        Self::cost(self.ll_ms, self.ll_bytes)
    }
    fn ly_cost(&self) -> f64 {
        Self::cost(self.ly_ms, self.ly_bytes)
    }
    fn oracle_cost(&self) -> f64 {
        self.ll_cost().min(self.ly_cost())
    }
    /// Cost of the auto policy, including the probe it had to run.
    fn auto_cost(&self) -> f64 {
        self.probe_ms
            + match self.picked {
                Codec::Lossless => self.ll_cost(),
                Codec::Lossy(_) => self.ly_cost(),
            }
    }
    fn auto_encode_ms(&self) -> f64 {
        match self.picked {
            Codec::Lossless => self.ll_ms,
            Codec::Lossy(_) => self.ly_ms,
        }
    }
}

fn measure(img: &Image, reps: u32) -> Row {
    let (px, w, h) = (&img.rgba, img.width, img.height);
    let distinct = probe_distinct_colors(px, w, h, AUTO_DISTINCT_CAP);
    // The probe is sub-microsecond, so time many reps and divide
    // rather than trying to resolve one.
    let probe_ms = time_ms(reps, || {
        for _ in 0..100 {
            std::hint::black_box(probe_distinct_colors(
                std::hint::black_box(px),
                w,
                h,
                AUTO_DISTINCT_CAP,
            ));
        }
    }) / 100.0;

    let mut ll_bytes = 0;
    let ll_ms = time_ms(reps, || {
        ll_bytes = encode_rgba_lossless(px, w, h).len();
    });
    let mut ly_bytes = 0;
    let ly_ms = time_ms(reps, || {
        ly_bytes = encode_rgba_lossy(px, w, h, AUTO_LOSSY_QUALITY).len();
    });

    Row {
        name: img.name.clone(),
        w,
        h,
        real: img.real,
        distinct,
        probe_ms,
        ll_ms,
        ll_bytes,
        ly_ms,
        ly_bytes,
        picked: select_codec(px, w, h),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn summarise(label: &str, rows: &[&Row]) {
    if rows.is_empty() {
        return;
    }
    let report = |policy: &str, cost: &dyn Fn(&Row) -> f64, ms: &dyn Fn(&Row) -> f64| {
        let total: f64 = rows.iter().map(|r| cost(r)).sum();
        let oracle: f64 = rows.iter().map(|r| r.oracle_cost()).sum();
        let mut times: Vec<f64> = rows.iter().map(|r| ms(r)).collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  {policy:<28} regret {:>9.2} ms  total {:>9.2} ms  worst encode {:>8.2} ms  p95 {:>7.2} ms",
            total - oracle,
            total,
            times.last().copied().unwrap_or(0.0),
            percentile(&times, 0.95),
        );
    };
    println!("\n== {label} ({} cases) ==", rows.len());
    report("always lossless", &|r| r.ll_cost(), &|r| r.ll_ms);
    report("always lossy q90", &|r| r.ly_cost(), &|r| r.ly_ms);
    report(
        "auto (this crate)",
        &|r| r.auto_cost(),
        &Row::auto_encode_ms,
    );
    report("oracle", &Row::oracle_cost, &|r| {
        if r.ll_cost() <= r.ly_cost() {
            r.ll_ms
        } else {
            r.ly_ms
        }
    });

    let wrong: Vec<&&Row> = rows
        .iter()
        .filter(|r| (r.auto_cost() - r.probe_ms - r.oracle_cost()).abs() > 1e-9)
        .collect();
    let worst_ratio = wrong
        .iter()
        .map(|r| (r.auto_cost() - r.probe_ms) / r.oracle_cost())
        .fold(1.0f64, f64::max);
    let worst_abs = wrong
        .iter()
        .map(|r| r.auto_cost() - r.probe_ms - r.oracle_cost())
        .fold(0.0f64, f64::max);
    println!(
        "  misclassified {}/{}; worst ratio {worst_ratio:.2}x; worst absolute excess {worst_abs:.2} ms",
        wrong.len(),
        rows.len()
    );
}

#[test]
#[ignore = "minutes-long encode benchmark; run with --release -- --ignored --nocapture"]
fn codec_selection_benchmark() {
    let mut sources = load_real_corpus();
    let n_real = sources.len();
    sources.extend(synthetic_corpus());
    assert!(
        n_real > 0,
        "expected real captures in {}",
        snapshot_dir().display()
    );
    println!(
        "corpus: {n_real} real captures + {} synthetics",
        sources.len() - n_real
    );

    // Unbiased crop grid: fixed fractional positions, not
    // hand-picked "busy" patches. Small rects dominate X11 damage
    // traffic by count, so they have to be in the sample.
    const CROPS: [(u32, u32); 4] = [(320, 240), (200, 100), (64, 64), (40, 20)];
    const POSITIONS: [(f64, f64); 4] = [(0.0, 0.0), (0.5, 0.15), (0.15, 0.5), (0.6, 0.6)];

    // Materialise every buffer once and keep it: the threshold sweep
    // at the end re-probes these without re-encoding.
    let mut cases: Vec<Image> = Vec::new();
    for src in sources {
        for (cw, ch) in CROPS {
            for (fx, fy) in POSITIONS {
                let x = ((src.width.saturating_sub(cw)) as f64 * fx) as u32;
                let y = ((src.height.saturating_sub(ch)) as f64 * fy) as u32;
                if let Some(c) = src.crop(x, y, cw, ch) {
                    cases.push(c);
                }
            }
        }
        cases.push(src);
    }

    let rows: Vec<Row> = cases
        .iter()
        .map(|c| {
            let big = c.width as u64 * c.height as u64 > 250_000;
            measure(c, if big { 3 } else { 7 })
        })
        .collect();

    println!(
        "\n{:<44} {:>10} {:>9} | {:>10} {:>9} | {:>5} {:>10} {:>9}",
        "case", "LL ms", "LL bytes", "LY ms", "LY bytes", "dist", "probe ms", "picked"
    );
    for r in &rows {
        // Only print full frames; the crop rows would be hundreds of
        // lines of noise, and they are all folded into the summary.
        if r.name.contains('[') {
            continue;
        }
        println!(
            "{:<44} {:>10.2} {:>9} | {:>10.2} {:>9} | {:>5} {:>10.5} {:>9}",
            format!("{} {}x{}", r.name, r.w, r.h),
            r.ll_ms,
            r.ll_bytes,
            r.ly_ms,
            r.ly_bytes,
            if r.distinct > AUTO_DISTINCT_CAP {
                format!("{}+", AUTO_DISTINCT_CAP)
            } else {
                r.distinct.to_string()
            },
            r.probe_ms,
            match r.picked {
                Codec::Lossless => "lossless",
                Codec::Lossy(_) => "lossy",
            }
        );
    }

    // Probe overhead as a fraction of the encode it gates — the
    // number that decides whether a probe is affordable at all.
    let worst_probe = rows
        .iter()
        .map(|r| (r.probe_ms / r.ll_ms.min(r.ly_ms) * 100.0, r.name.as_str()))
        .fold((0.0f64, ""), |a, b| if b.0 > a.0 { b } else { a });
    println!(
        "\nprobe cost vs cheapest encode: worst {:.2}% ({})",
        worst_probe.0, worst_probe.1
    );

    summarise("ALL", &rows.iter().collect::<Vec<_>>());
    summarise(
        "REAL CAPTURES ONLY",
        &rows.iter().filter(|r| r.real).collect::<Vec<_>>(),
    );

    // Threshold sweep: re-probe at each cap and replay the already
    // measured encode costs, so this is nearly free.
    println!("\n== threshold sweep (regret ms, all cases) ==");
    for cap in [8u32, 16, 24, 32, 40, 64, 96, 128, 160, 192] {
        let regret: f64 = rows
            .iter()
            .zip(&cases)
            .map(|(r, c)| {
                let d = probe_distinct_colors(&c.rgba, c.width, c.height, cap);
                let chosen = if d <= cap { r.ll_cost() } else { r.ly_cost() };
                chosen - r.oracle_cost()
            })
            .sum();
        println!(
            "  cap {cap:>4} -> {regret:>9.2} ms{}",
            if cap == AUTO_DISTINCT_CAP {
                "   <-- shipped"
            } else {
                ""
            }
        );
    }
}

/// Guard on the e2e suite's tightest screenshot comparisons.
///
/// `e2e/tests/x11-web.spec.ts` compares the decoded canvas against
/// these baselines byte-for-byte-ish: xeyes at `maxDiffPixelRatio`
/// 0.01, xterm and vim at 0.05. Those margins hold trivially today
/// because the selector sends all of that content down the *lossless*
/// path, so the comparison is bit-exact rather than merely close.
///
/// That is a property of the threshold, not a law, so pin it here:
/// this fails in half a second if someone lowers
/// `AUTO_DISTINCT_CAP`, long before it shows up as a 20-minute e2e
/// run with a mystery pixel diff. Unlike the benchmark above it does
/// no encoding, so it stays in the default test run.
///
/// Firefox is deliberately *not* in this list. `firefox-canvas`
/// probes past the cap and does encode lossy — that is the whole
/// point of the change — and its test tolerates it at a 0.1 ratio.
#[test]
fn e2e_screenshot_baselines_stay_on_the_lossless_path() {
    const TIGHTLY_COMPARED: [&str; 6] = [
        "xeyes-canvas",
        "xeyes-looking-center",
        "xeyes-looking-top-right",
        "xterm-canvas",
        "xterm-keyboard",
        "vim-after-save",
    ];

    let corpus = load_real_corpus();
    assert!(
        !corpus.is_empty(),
        "expected the e2e snapshot corpus at {}",
        snapshot_dir().display()
    );

    let mut checked = 0;
    for img in &corpus {
        if !TIGHTLY_COMPARED
            .iter()
            .any(|p| img.name.starts_with(&format!("{p}-")))
        {
            continue;
        }
        checked += 1;
        let distinct = probe_distinct_colors(&img.rgba, img.width, img.height, AUTO_DISTINCT_CAP);
        assert_eq!(
            select_codec(&img.rgba, img.width, img.height),
            Codec::Lossless,
            "{} ({}x{}, {} distinct colours) would encode lossy; the e2e \
             screenshot comparison for it expects bit-exact pixels",
            img.name,
            img.width,
            img.height,
            distinct,
        );
    }
    assert!(
        checked >= TIGHTLY_COMPARED.len(),
        "only matched {checked} baselines; did a snapshot get renamed?"
    );
}
