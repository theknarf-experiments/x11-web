//! Single-window capture via ScreenCaptureKit.
//!
//! Mirrors cua-driver's `WindowCapture.captureWindow`:
//!
//!   1. `SCShareableContent.getShareableContentWithCompletionHandler:`
//!      to enumerate every window WindowServer knows about.
//!   2. Locate the `SCWindow` whose `windowID` matches the
//!      `CGWindowID` we tracked via `CGWindowListCopyWindowInfo`.
//!   3. `SCContentFilter(desktopIndependentWindow:)` — the filter
//!      kind that captures hidden / occluded / off-Space windows by
//!      reading the desktop-independent compositor surface, instead
//!      of grabbing rectangles off a display.
//!   4. `SCScreenshotManager.captureImageWithFilter:configuration:` —
//!      one-shot capture, returns a CGImage in BGRA.
//!   5. Drain CGImage's data provider, swap channels into RGBA, drop
//!      any per-row padding.
//!
//! Both Apple async entrypoints in step 1 and 4 deliver via ObjC
//! completion blocks; we bridge to Rust async with `tokio::sync::
//! oneshot` and a `Mutex<Option<Sender>>` to satisfy `RcBlock`'s `Fn`
//! constraint (the block is structurally callable many times even
//! though Apple only ever calls it once).

use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};
use tokio::sync::oneshot;

// Pulled in for the deprecated `capture_window_legacy` path that
// uses `CGWindowListCreateImage`. Kept alongside the SCK code so
// callers can pick whichever works on the host's macOS / TCC state.
use core_graphics::geometry::CGRect as LegacyRect;
use core_graphics::window as legacy_win;

#[derive(Debug)]
pub enum CaptureError {
    /// `SCShareableContent.current` failed — almost always the
    /// Screen Recording TCC grant. We surface the NSError description
    /// so the operator sees the actual reason in the log.
    NoContent(String),
    WindowNotFound(u32),
    CaptureFailed(String),
    /// The CGImage came back but we couldn't read its pixel buffer.
    /// Shouldn't happen in practice — defended against because the
    /// Apple SPI returns `Option`s at every step.
    BadImage,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContent(s) => write!(f, "shareable content unavailable: {s}"),
            Self::WindowNotFound(id) => write!(f, "no shareable window with id {id}"),
            Self::CaptureFailed(s) => write!(f, "capture failed: {s}"),
            Self::BadImage => write!(f, "captured image had no pixel data"),
        }
    }
}

impl std::error::Error for CaptureError {}

pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8888, row stride = `width * 4`.
    pub rgba: Vec<u8>,
}

pub async fn capture_window(window_id: u32) -> Result<CapturedFrame, CaptureError> {
    let content = fetch_shareable_content().await?;
    let target = find_window(&content, window_id)?;

    let filter = unsafe {
        SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target)
    };

    // SCK takes points (logical pixels). The window's `frame` is
    // already in points — we let SCK do the scale-up to physical
    // pixels via the display's `backingScaleFactor`. Multiplying
    // here would double-apply the scale on Retina.
    let frame = unsafe { target.frame() };
    let width = frame.size.width.max(1.0) as usize;
    let height = frame.size.height.max(1.0) as usize;

    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(width);
        config.setHeight(height);
        config.setShowsCursor(false);
    }

    let image = capture_image(&filter, &config).await?;
    extract_rgba(&image)
}

async fn fetch_shareable_content() -> Result<Retained<SCShareableContent>, CaptureError> {
    let (tx, rx) = oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let result = if let Some(c) = NonNull::new(content) {
            Ok(unsafe { Retained::retain(c.as_ptr()).unwrap() })
        } else {
            Err(error_message(err))
        };
        if let Some(s) = tx.lock().unwrap().take() {
            let _ = s.send(result);
        }
    });
    unsafe {
        SCShareableContent::getShareableContentWithCompletionHandler(&block);
    }
    rx.await
        .map_err(|_| CaptureError::NoContent("completion handler dropped".into()))?
        .map_err(CaptureError::NoContent)
}

async fn capture_image(
    filter: &SCContentFilter,
    config: &SCStreamConfiguration,
) -> Result<CFRetained<CGImage>, CaptureError> {
    let (tx, rx) = oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let block = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
        let result = if let Some(p) = NonNull::new(image) {
            Ok(unsafe { CFRetained::retain(p) })
        } else {
            Err(error_message(err))
        };
        if let Some(s) = tx.lock().unwrap().take() {
            let _ = s.send(result);
        }
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            filter,
            config,
            Some(&block),
        );
    }
    rx.await
        .map_err(|_| CaptureError::CaptureFailed("completion handler dropped".into()))?
        .map_err(CaptureError::CaptureFailed)
}

fn find_window(
    content: &SCShareableContent,
    window_id: u32,
) -> Result<Retained<SCWindow>, CaptureError> {
    let windows = unsafe { content.windows() };
    for i in 0..windows.count() {
        let w = windows.objectAtIndex(i);
        if unsafe { w.windowID() } == window_id {
            return Ok(w);
        }
    }
    Err(CaptureError::WindowNotFound(window_id))
}

fn extract_rgba(image: &CGImage) -> Result<CapturedFrame, CaptureError> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let bytes_per_row = CGImage::bytes_per_row(Some(image));
    let bits_per_pixel = CGImage::bits_per_pixel(Some(image));
    if width == 0 || height == 0 || bits_per_pixel != 32 {
        return Err(CaptureError::BadImage);
    }

    let provider = CGImage::data_provider(Some(image)).ok_or(CaptureError::BadImage)?;
    let data: CFRetained<CFData> =
        CGDataProvider::data(Some(&provider)).ok_or(CaptureError::BadImage)?;

    let len = data.length() as usize;
    let ptr = data.byte_ptr();
    if ptr.is_null() || len < bytes_per_row * height {
        return Err(CaptureError::BadImage);
    }
    let raw = unsafe { std::slice::from_raw_parts(ptr, len) };

    // SCK delivers BGRA premultiplied (host-endian little-endian =
    // byte order [B, G, R, A]). Convert to packed RGBA, dropping any
    // row stride padding.
    let mut rgba = Vec::with_capacity(width * height * 4);
    let alpha = CGImage::alpha_info(Some(image));
    let byte_order = CGImage::byte_order_info(Some(image));
    let is_bgra = matches!(
        (alpha, byte_order),
        (
            CGImageAlphaInfo::PremultipliedFirst | CGImageAlphaInfo::First | CGImageAlphaInfo::NoneSkipFirst,
            CGImageByteOrderInfo::Order32Little,
        )
    );

    for row in 0..height {
        let start = row * bytes_per_row;
        let row_bytes = &raw[start..start + width * 4];
        if is_bgra {
            for px in row_bytes.chunks_exact(4) {
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        } else {
            // Trust the source — already RGBA-shaped. Defensive
            // because cua-driver assumes BGRA, but on non-Retina
            // / non-SDR captures the layout can shift.
            rgba.extend_from_slice(row_bytes);
        }
    }

    Ok(CapturedFrame {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// Synchronous fallback capture using `CGWindowListCreateImage`. The
/// API is deprecated as of macOS 14 — Apple's intent is for everything
/// to flow through ScreenCaptureKit + Screen Recording TCC — but the
/// shim still ships, and on systems where it works it sidesteps the
/// async-block dance entirely. Useful for verifying the rest of the
/// pipeline (encode → wire → frontend canvas) without first solving
/// TCC for the SCK path.
///
/// Caveats per Apple's deprecation notice:
///   - Returns blank/clipped images for occluded windows on macOS 14+
///   - Future macOS versions are expected to remove it entirely
///   - Doesn't capture off-Space windows reliably
///
/// All of which is why cua uses ScreenCaptureKit. We keep this around
/// as a smoke-test affordance.
pub fn capture_window_legacy(window_id: u32) -> Result<CapturedFrame, CaptureError> {
    let opts = legacy_win::kCGWindowListOptionIncludingWindow
        | legacy_win::kCGWindowListExcludeDesktopElements;
    let image_opts = legacy_win::kCGWindowImageBoundsIgnoreFraming
        | legacy_win::kCGWindowImageBestResolution;
    // Passing `CGRectNull` (all zeros sentinel) requests the window's
    // own bounds. core-graphics 0.24 doesn't export the constant, so
    // we hand-construct the equivalent: width/height = infinity.
    let null_rect = LegacyRect::new(
        &core_graphics::geometry::CGPoint::new(f64::INFINITY, f64::INFINITY),
        &core_graphics::geometry::CGSize::new(0.0, 0.0),
    );
    let img = legacy_win::create_image(null_rect, opts, window_id, image_opts)
        .ok_or_else(|| CaptureError::CaptureFailed(
            "CGWindowListCreateImage returned null".into(),
        ))?;

    let width = img.width();
    let height = img.height();
    let bytes_per_row = img.bytes_per_row();
    let bits_per_pixel = img.bits_per_pixel();
    if width == 0 || height == 0 || bits_per_pixel != 32 {
        return Err(CaptureError::BadImage);
    }
    let data = img.data();
    let raw: &[u8] = data.bytes();
    if raw.len() < bytes_per_row * height {
        return Err(CaptureError::BadImage);
    }

    // CGWindowListCreateImage returns BGRA premultiplied, same as SCK
    // SDR captures.
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = row * bytes_per_row;
        let row_bytes = &raw[start..start + width * 4];
        for px in row_bytes.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }

    Ok(CapturedFrame {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

fn error_message(err: *mut NSError) -> String {
    if err.is_null() {
        return "unknown error (no NSError)".into();
    }
    let err = unsafe { Retained::retain(err).unwrap() };
    err.localizedDescription().to_string()
}
