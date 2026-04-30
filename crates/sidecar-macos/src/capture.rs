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
//! The chain is structured as **nested ObjC completion handlers** —
//! `getShareableContent`'s callback synchronously kicks off
//! `captureImage`'s callback, which extracts pixels and sends the
//! resulting `CapturedFrame` through a single `oneshot`. This shape
//! keeps every ObjC type (SCShareableContent, SCWindow,
//! SCContentFilter, CGImage) inside the ObjC-owned thread context,
//! so the only thing crossing Rust's `.await` boundary is
//! `oneshot::Receiver<Result<CapturedFrame, String>>` — which IS
//! `Send`, allowing `capture_window` to be called from
//! `tokio::spawn`'d tasks.

use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};
use tokio::sync::oneshot;

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

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8888, row stride = `width * 4`.
    pub rgba: Vec<u8>,
}

/// One-shot completion delivered by the nested ObjC blocks.
type FrameSender = oneshot::Sender<Result<CapturedFrame, String>>;
type SharedSender = Arc<Mutex<Option<FrameSender>>>;

/// Capture `window_id` and return its pixels as packed RGBA.
///
/// `max_dim`, if non-zero, caps the longer side of the captured
/// image (in points) — SCK does the downscale internally during
/// capture, so neither the wire payload nor our extract loop pay
/// for the extra pixels. A typical desktop window at 1600×1000
/// emits ~6.4 MB of RGBA at full size; capping to `max_dim = 800`
/// brings that to ~1.6 MB which JSON-over-WebSocket can carry at
/// a couple Hz without choking.
///
/// Send-friendly: holds only `oneshot::Receiver` across `.await`,
/// never ObjC types.
pub async fn capture_window(window_id: u32, max_dim: u32) -> Result<CapturedFrame, CaptureError> {
    let (tx, rx) = oneshot::channel::<Result<CapturedFrame, String>>();
    let sender: SharedSender = Arc::new(Mutex::new(Some(tx)));

    // Tight scope around the !Send `RcBlock` so Rust's drop-tracker
    // can prove it doesn't outlive the SCK call; without this the
    // compiler keeps it alive across the `.await` and the future
    // becomes non-`Send`, blocking `tokio::spawn`. SCK calls
    // `Block_copy` internally, so it owns its own retain — dropping
    // our `RcBlock` here is safe.
    {
        let outer = build_outer_block(window_id, max_dim, sender);
        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&outer);
        }
    }

    rx.await
        .map_err(|_| CaptureError::CaptureFailed("completion handler dropped".into()))?
        .map_err(|e| {
            // Heuristic: TCC failures are routed through `NoContent`
            // so callers can offer the right log message.
            if e.contains("declined") || e.contains("authorized") || e.contains("permission") {
                CaptureError::NoContent(e)
            } else if let Some(rest) = e.strip_prefix("window-not-found:") {
                CaptureError::WindowNotFound(rest.parse().unwrap_or(0))
            } else {
                CaptureError::CaptureFailed(e)
            }
        })
}

fn build_outer_block(
    window_id: u32,
    max_dim: u32,
    sender: SharedSender,
) -> RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> {
    RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let Some(content_nn) = NonNull::new(content) else {
            send_err(&sender, error_message(err));
            return;
        };
        // SAFETY: `getShareableContent`'s contract: when err is null,
        // content is a valid SCShareableContent. We're inside that
        // callback path.
        let content_ref: &SCShareableContent = unsafe { content_nn.as_ref() };

        let target = match find_window(content_ref, window_id) {
            Some(w) => w,
            None => {
                send_err(&sender, format!("window-not-found:{window_id}"));
                return;
            }
        };

        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target)
        };
        // SCK takes points (logical pixels). The window's `frame` is
        // already in points — SCK does the scale-up to physical
        // pixels internally via the display's backing scale.
        let frame = unsafe { target.frame() };
        let (width, height) = constrained_size(frame.size.width, frame.size.height, max_dim);
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(width);
            config.setHeight(height);
            config.setShowsCursor(false);
        }

        // Nested completion: extract bytes inside ObjC, ship Vec<u8>
        // out — never a !Send ObjC type — through the same sender.
        let inner = build_inner_block(sender.clone());
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&inner),
            );
        }
        // inner drops here; SCK's Block_copy keeps it alive.
    })
}

fn build_inner_block(sender: SharedSender) -> RcBlock<dyn Fn(*mut CGImage, *mut NSError)> {
    RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
        let result = match NonNull::new(image) {
            Some(p) => {
                // SAFETY: We're in the captureImage completion
                // callback; when image is non-null, it points to a
                // valid CGImage. We don't keep it past this scope.
                let img: &CGImage = unsafe { p.as_ref() };
                extract_rgba(img).map_err(|e| format!("{e}"))
            }
            None => Err(error_message(err)),
        };
        send_result(&sender, result);
    })
}

fn constrained_size(width: f64, height: f64, max_dim: u32) -> (usize, usize) {
    let w = width.max(1.0);
    let h = height.max(1.0);
    if max_dim == 0 {
        return (w as usize, h as usize);
    }
    let cap = max_dim as f64;
    let larger = w.max(h);
    if larger <= cap {
        return (w as usize, h as usize);
    }
    let scale = cap / larger;
    ((w * scale).max(1.0) as usize, (h * scale).max(1.0) as usize)
}

fn find_window(
    content: &SCShareableContent,
    window_id: u32,
) -> Option<Retained<objc2_screen_capture_kit::SCWindow>> {
    let windows = unsafe { content.windows() };
    for i in 0..windows.count() {
        let w = windows.objectAtIndex(i);
        if unsafe { w.windowID() } == window_id {
            return Some(w);
        }
    }
    None
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
    let data: CFRetained<objc2_core_foundation::CFData> =
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
            CGImageAlphaInfo::PremultipliedFirst
                | CGImageAlphaInfo::First
                | CGImageAlphaInfo::NoneSkipFirst,
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
            // Defensive: cua-driver assumes BGRA, but on non-Retina
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

fn send_err(sender: &SharedSender, msg: String) {
    send_result(sender, Err(msg));
}

fn send_result(sender: &SharedSender, result: Result<CapturedFrame, String>) {
    if let Ok(mut guard) = sender.lock() {
        if let Some(s) = guard.take() {
            let _ = s.send(result);
        }
    }
}

fn error_message(err: *mut NSError) -> String {
    if err.is_null() {
        return "unknown error (no NSError)".into();
    }
    let err = unsafe { Retained::retain(err).unwrap() };
    err.localizedDescription().to_string()
}
