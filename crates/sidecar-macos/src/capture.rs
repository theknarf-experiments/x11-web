//! Single-window capture via ScreenCaptureKit.
//!
//! Two-phase API:
//!
//!   1. [`build_session`] enumerates the desktop's shareable windows
//!      once, finds the target by `CGWindowID`, and constructs a
//!      reusable [`CaptureSession`] (an `SCContentFilter` plus a
//!      pre-built `SCStreamConfiguration`).
//!   2. [`capture_with_session`] reuses the cached session to take a
//!      single screenshot via `SCScreenshotManager.captureImage`.
//!      No per-frame enumeration; just the readback.
//!
//! Skipping the enumeration removes the 50–300 ms / call overhead
//! we'd otherwise pay every frame on a busy desktop.
//!
//! Both entry points are *synchronous* — they block on a
//! `std::sync::mpsc` channel until the ObjC completion block fires,
//! and are intended to run on a dedicated `std::thread` rather than a
//! tokio task. That's because the cached `Retained<SCContentFilter>`
//! is `!Send` — see the unsafe impl on [`CaptureSession`] below.

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

/// A cached capture target — the SC objects needed to take a
/// screenshot of a specific window without re-enumerating the whole
/// desktop. Build once via [`build_session`], reuse across many
/// [`capture_with_session`] calls.
///
/// The session pins to the window's bounds at build time. If the
/// macOS window resizes, captures keep coming back at the original
/// size (downscaled / upscaled to fit). Rebuild the session if the
/// caller knows the bounds changed.
pub struct CaptureSession {
    filter: Retained<SCContentFilter>,
    config: Retained<SCStreamConfiguration>,
}

// SAFETY: ScreenCaptureKit's SCContentFilter and SCStreamConfiguration
// are reference-counted descriptor objects without thread-affine
// state. We construct the session inside a GCD completion-block
// thread, then move it across exactly one boundary (mpsc channel)
// onto the dedicated capture thread, where it stays. We never share
// an instance across threads after that point. Method dispatch on
// these classes is documented to be safe from any thread.
unsafe impl Send for CaptureSession {}

type SessionSender = Arc<Mutex<Option<SyncSender<Result<CaptureSession, String>>>>>;
type FrameSender = Arc<Mutex<Option<SyncSender<Result<CapturedFrame, String>>>>>;

/// Synchronously enumerate shareable windows, find the one with
/// `window_id`, and return a [`CaptureSession`] usable for many
/// subsequent [`capture_with_session`] calls. `max_dim` caps the
/// longer side at capture time (0 = no cap, full window dimensions).
///
/// Blocks the calling thread until the SCK completion block fires —
/// don't call from a tokio task; spawn a dedicated `std::thread`.
pub fn build_session(window_id: u32, max_dim: u32) -> Result<CaptureSession, CaptureError> {
    let (tx, rx) = sync_channel::<Result<CaptureSession, String>>(1);
    let sender: SessionSender = Arc::new(Mutex::new(Some(tx)));
    {
        let outer = build_session_block(window_id, max_dim, sender);
        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&outer);
        }
    }
    rx.recv()
        .map_err(|_| CaptureError::CaptureFailed("session callback dropped".into()))?
        .map_err(classify_error)
}

/// Synchronously capture a frame using a session built earlier.
/// Blocks until SCK's `captureImage` completion fires.
pub fn capture_with_session(session: &CaptureSession) -> Result<CapturedFrame, CaptureError> {
    let (tx, rx) = sync_channel::<Result<CapturedFrame, String>>(1);
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
        .map_err(|_| CaptureError::CaptureFailed("capture callback dropped".into()))?
        .map_err(CaptureError::CaptureFailed)
}

fn classify_error(e: String) -> CaptureError {
    if e.contains("declined") || e.contains("authorized") || e.contains("permission") {
        CaptureError::NoContent(e)
    } else if let Some(rest) = e.strip_prefix("window-not-found:") {
        CaptureError::WindowNotFound(rest.parse().unwrap_or(0))
    } else {
        CaptureError::CaptureFailed(e)
    }
}

fn build_session_block(
    window_id: u32,
    max_dim: u32,
    sender: SessionSender,
) -> RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> {
    RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let send = |r: Result<CaptureSession, String>| {
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
        send(Ok(CaptureSession { filter, config }));
    })
}

fn build_frame_block(sender: FrameSender) -> RcBlock<dyn Fn(*mut CGImage, *mut NSError)> {
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

fn error_message(err: *mut NSError) -> String {
    if err.is_null() {
        return "unknown error (no NSError)".into();
    }
    let err = unsafe { Retained::retain(err).unwrap() };
    err.localizedDescription().to_string()
}
