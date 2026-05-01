//! One-shot, downscaled screenshot path for window thumbnails.
//!
//! Sits alongside `capture.rs` (the live SCStream pipeline). Two
//! reasons to keep it separate:
//!   * Different cadence — thumbnails refresh at ~1 Hz; the live
//!     stream pushes 30 fps.
//!   * Different output — thumbnails feed the spawn-popover picker
//!     for windows the user hasn't attached yet; live frames feed
//!     the canvas for attached windows.
//!
//! Both phases of the API mirror what `capture.rs` had before we
//! switched to SCStream:
//!   1. [`build_session`] enumerates shareable content once, finds
//!      the target window, and constructs a reusable session
//!      (cached `SCContentFilter` + a downscaled `SCStreamConfiguration`).
//!   2. [`capture_with_session`] reuses the cached session to take
//!      a single screenshot via `SCScreenshotManager.captureImage`.
//!
//! The session pins the capture dimensions at build time. If the
//! window resizes substantially the caller should rebuild.

use std::ptr::NonNull;
use std::sync::mpsc::{sync_channel, SyncSender};
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

#[derive(Debug)]
pub enum ScreenshotError {
    NoContent(String),
    WindowNotFound(u32),
    CaptureFailed(String),
    BadImage,
}

impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContent(s) => write!(f, "shareable content unavailable: {s}"),
            Self::WindowNotFound(id) => write!(f, "no shareable window with id {id}"),
            Self::CaptureFailed(s) => write!(f, "screenshot failed: {s}"),
            Self::BadImage => write!(f, "captured image had no pixel data"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

#[derive(Debug, Clone)]
pub struct ScreenshotFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8888, row stride = `width * 4`.
    pub rgba: Vec<u8>,
}

/// Cached screenshot target. Build once via [`build_session`], reuse
/// across many [`capture_with_session`] calls so we skip the
/// 50–300 ms `SCShareableContent` enumeration per call.
pub struct ScreenshotSession {
    filter: Retained<SCContentFilter>,
    config: Retained<SCStreamConfiguration>,
}

// SAFETY: same reasoning as in `capture.rs` — these SCK descriptor
// objects are reference-counted and don't carry thread-affine state.
// We construct on a GCD completion-block thread, ferry across one
// `mpsc` boundary, and keep on a single thread thereafter.
unsafe impl Send for ScreenshotSession {}

type SessionSender = Arc<Mutex<Option<SyncSender<Result<ScreenshotSession, String>>>>>;
type FrameSender = Arc<Mutex<Option<SyncSender<Result<ScreenshotFrame, String>>>>>;

/// Synchronously enumerate shareable windows, find the one with
/// `window_id`, and build a downscaled-screenshot session. `max_dim`
/// caps the longer side at capture time (0 = no cap).
pub fn build_session(window_id: u32, max_dim: u32) -> Result<ScreenshotSession, ScreenshotError> {
    let (tx, rx) = sync_channel::<Result<ScreenshotSession, String>>(1);
    let sender: SessionSender = Arc::new(Mutex::new(Some(tx)));
    {
        let outer = build_session_block(window_id, max_dim, sender);
        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&outer);
        }
    }
    rx.recv()
        .map_err(|_| ScreenshotError::CaptureFailed("session callback dropped".into()))?
        .map_err(classify_error)
}

/// Synchronously capture a single frame using a session built earlier.
pub fn capture_with_session(
    session: &ScreenshotSession,
) -> Result<ScreenshotFrame, ScreenshotError> {
    let (tx, rx) = sync_channel::<Result<ScreenshotFrame, String>>(1);
    let sender: FrameSender = Arc::new(Mutex::new(Some(tx)));
    {
        let inner = build_frame_block(sender);
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &session.filter,
                &session.config,
                Some(&inner),
            );
        }
    }
    rx.recv()
        .map_err(|_| ScreenshotError::CaptureFailed("capture callback dropped".into()))?
        .map_err(ScreenshotError::CaptureFailed)
}

fn classify_error(e: String) -> ScreenshotError {
    if e.contains("declined") || e.contains("authorized") || e.contains("permission") {
        ScreenshotError::NoContent(e)
    } else if let Some(rest) = e.strip_prefix("window-not-found:") {
        ScreenshotError::WindowNotFound(rest.parse().unwrap_or(0))
    } else {
        ScreenshotError::CaptureFailed(e)
    }
}

fn build_session_block(
    window_id: u32,
    max_dim: u32,
    sender: SessionSender,
) -> RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> {
    RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let send = |r: Result<ScreenshotSession, String>| {
            if let Ok(mut g) = sender.lock() {
                if let Some(s) = g.take() {
                    let _ = s.send(r);
                }
            }
        };
        let Some(content_nn) = NonNull::new(content) else {
            send(Err(error_message(err)));
            return;
        };
        let content_ref: &SCShareableContent = unsafe { content_nn.as_ref() };
        let Some(target) = find_window(content_ref, window_id) else {
            send(Err(format!("window-not-found:{window_id}")));
            return;
        };

        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target)
        };
        let frame = unsafe { target.frame() };
        let (width, height) = constrained_size(frame.size.width, frame.size.height, max_dim);
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(width);
            config.setHeight(height);
            config.setShowsCursor(false);
        }
        send(Ok(ScreenshotSession { filter, config }));
    })
}

fn build_frame_block(sender: FrameSender) -> RcBlock<dyn Fn(*mut CGImage, *mut NSError)> {
    RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
        let result = match NonNull::new(image) {
            Some(p) => {
                // SAFETY: We're inside captureImage's completion
                // block; non-null image points to a valid CGImage,
                // and we don't keep it past this scope.
                let img: &CGImage = unsafe { p.as_ref() };
                extract_rgba(img).map_err(|e| format!("{e}"))
            }
            None => Err(error_message(err)),
        };
        if let Ok(mut g) = sender.lock() {
            if let Some(s) = g.take() {
                let _ = s.send(result);
            }
        }
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

fn extract_rgba(image: &CGImage) -> Result<ScreenshotFrame, ScreenshotError> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let bytes_per_row = CGImage::bytes_per_row(Some(image));
    let bits_per_pixel = CGImage::bits_per_pixel(Some(image));
    if width == 0 || height == 0 || bits_per_pixel != 32 {
        return Err(ScreenshotError::BadImage);
    }

    let provider = CGImage::data_provider(Some(image)).ok_or(ScreenshotError::BadImage)?;
    let data: CFRetained<objc2_core_foundation::CFData> =
        CGDataProvider::data(Some(&provider)).ok_or(ScreenshotError::BadImage)?;

    let len = data.length() as usize;
    let ptr = data.byte_ptr();
    if ptr.is_null() || len < bytes_per_row * height {
        return Err(ScreenshotError::BadImage);
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
            rgba.extend_from_slice(row_bytes);
        }
    }

    Ok(ScreenshotFrame {
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
