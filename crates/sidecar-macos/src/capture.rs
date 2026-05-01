//! Single-window capture via ScreenCaptureKit's `SCStream`.
//!
//! Two-phase API:
//!
//!   1. [`build_session`] enumerates shareable windows once, finds the
//!      target by `CGWindowID`, builds an `SCStream` configured to
//!      deliver up to `fps` BGRA frames per second, and returns a
//!      [`CaptureSession`] that owns the running stream.
//!   2. [`recv_frame_timeout`] blocks the calling thread until SCK
//!      pushes the next frame onto the session's queue (or the
//!      timeout elapses).
//!
//! Pull vs push: the previous implementation called
//! `SCScreenshotManager.captureImage` per frame, which paid 100–300 ms
//! of internal setup on every call. With `SCStream` the framework
//! pushes us frames as the WindowServer composites them — capture
//! latency drops to a single-digit-ms IOSurface readback.
//!
//! All ObjC interop is via `objc2`. The custom [`StreamHandler`]
//! class (defined via [`define_class!`]) implements both
//! `SCStreamOutput` (frame delivery) and `SCStreamDelegate` (stream
//! lifecycle). It runs on a dedicated serial `DispatchQueue` so frame
//! callbacks are serialised; the Mutex around the sender is just to
//! satisfy `Sync`.
//!
//! `CaptureSession` is constructed from an SCK completion block (a
//! GCD-managed thread) and then ferried via `std::sync::mpsc` to the
//! caller. The contained `Retained<...>` ObjC objects aren't
//! auto-`Send`, but the underlying ScreenCaptureKit / dispatch types
//! are documented as thread-safe — see the `unsafe impl Send` below.

use std::ptr::NonNull;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, AllocAnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVImageBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType,
};

#[derive(Debug)]
pub enum CaptureError {
    /// `SCShareableContent.current` failed — almost always the
    /// Screen Recording TCC grant. We surface the NSError description
    /// so the operator sees the actual reason in the log.
    NoContent(String),
    WindowNotFound(u32),
    CaptureFailed(String),
    /// Stream's `didStopWithError` fired or the producer end of the
    /// frame channel was dropped — caller should rebuild the session.
    Disconnected,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContent(s) => write!(f, "shareable content unavailable: {s}"),
            Self::WindowNotFound(id) => write!(f, "no shareable window with id {id}"),
            Self::CaptureFailed(s) => write!(f, "capture failed: {s}"),
            Self::Disconnected => write!(f, "capture stream stopped"),
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

/// `kCVPixelFormatType_32BGRA = 'BGRA' = 0x42475241`. SCStream's
/// default pixel format and what we explicitly request below.
const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

/// `kCVPixelBufferLock_ReadOnly`. Hint to CoreVideo that we won't
/// modify the buffer; lets it skip cache-invalidation work.
const LOCK_FLAGS_READ_ONLY: CVPixelBufferLockFlags = CVPixelBufferLockFlags(1);

type FrameSender = SyncSender<CapturedFrame>;
type FrameReceiver = Receiver<CapturedFrame>;

#[derive(Default)]
pub(crate) struct StreamHandlerIvars {
    /// `try_send` here from the SCK callback; latest-frame-wins by
    /// keeping the channel bounded at 1 and dropping on full.
    sender: Mutex<Option<FrameSender>>,
}

define_class!(
    /// Bridge between `SCStream`'s ObjC delegate/output protocols and
    /// our Rust frame channel.
    #[unsafe(super(NSObject))]
    #[ivars = StreamHandlerIvars]
    pub(crate) struct StreamHandler;

    impl StreamHandler {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            let frame = match extract_rgba(sample_buffer) {
                Some(f) => f,
                None => return,
            };
            // Latest-frame-wins: try to push; on a full channel drop
            // this frame on the floor (consumer is still encoding the
            // previous one). The bounded(1) sync_channel guarantees
            // we never queue more than one frame ahead.
            if let Ok(g) = self.ivars().sender.lock() {
                if let Some(s) = g.as_ref() {
                    match s.try_send(frame) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => {}
                    }
                }
            }
        }

        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            tracing::warn!(
                "SCStream stopped: {}",
                error.localizedDescription()
            );
            // Drop the sender so recv_frame_timeout returns
            // Disconnected on the next call.
            if let Ok(mut g) = self.ivars().sender.lock() {
                let _ = g.take();
            }
        }
    }

    unsafe impl NSObjectProtocol for StreamHandler {}
    unsafe impl SCStreamOutput for StreamHandler {}
    unsafe impl SCStreamDelegate for StreamHandler {}
);

impl StreamHandler {
    fn new(sender: FrameSender) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamHandlerIvars {
            sender: Mutex::new(Some(sender)),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// A running per-window capture. The contained `SCStream` continues
/// pushing frames into `frame_rx` until this struct is dropped, at
/// which point we tell SCK to stop the stream (fire-and-forget).
pub struct CaptureSession {
    stream: Retained<SCStream>,
    /// Holding a reference keeps SCK's retained pointer balanced and
    /// gives us a Drop hook on the channel sender (via the ivar) when
    /// the session is torn down.
    _handler: Retained<StreamHandler>,
    /// The serial dispatch queue SCK invokes our output callback on.
    /// Held here so it lives as long as the stream does.
    _queue: DispatchRetained<DispatchQueue>,
    frame_rx: FrameReceiver,
}

// SAFETY: SCStream / StreamHandler / DispatchQueue / Receiver are all
// thread-safe in their underlying APIs — GCD queues are documented as
// safe to access from any thread, ObjC dispatch on these classes
// likewise, and `std::sync::mpsc::Receiver<T>: Send` when `T: Send`.
// We construct the session on a GCD completion-block thread, ferry it
// across one mpsc boundary onto the dedicated capture thread, and
// keep it there.
unsafe impl Send for CaptureSession {}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        // Fire-and-forget stop: SCK may take tens of ms to fully tear
        // down the IOSurface pipeline, but we don't want to block the
        // enumerator on it. The completion handler retains itself
        // until SCK fires it.
        let block = RcBlock::new(|err: *mut NSError| {
            if !err.is_null() {
                let err_msg =
                    unsafe { Retained::retain(err).map(|e| e.localizedDescription().to_string()) };
                tracing::debug!("SCStream stop completion: {:?}", err_msg);
            }
        });
        unsafe {
            self.stream.stopCaptureWithCompletionHandler(Some(&block));
        }
    }
}

type SessionSender = Arc<Mutex<Option<SyncSender<Result<CaptureSession, String>>>>>;

/// Start an `SCStream` for the window with `window_id` at up to `fps`
/// frames/sec. Blocks the calling thread until SCK has both
/// enumerated content *and* started the stream — don't call from a
/// tokio task; spawn a dedicated `std::thread`.
pub fn build_session(window_id: u32, fps: u32) -> Result<CaptureSession, CaptureError> {
    let (tx, rx) = sync_channel::<Result<CaptureSession, String>>(1);
    let sender: SessionSender = Arc::new(Mutex::new(Some(tx)));
    {
        let outer = build_session_block(window_id, fps, sender);
        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&outer);
        }
    }
    rx.recv()
        .map_err(|_| CaptureError::CaptureFailed("session callback dropped".into()))?
        .map_err(classify_error)
}

/// Wait up to `timeout` for the stream's next frame. Returns
/// `Ok(Some(frame))` on data, `Ok(None)` on timeout, or
/// `Err(Disconnected)` when the stream has shut down.
pub fn recv_frame_timeout(
    session: &CaptureSession,
    timeout: Duration,
) -> Result<Option<CapturedFrame>, CaptureError> {
    match session.frame_rx.recv_timeout(timeout) {
        Ok(frame) => Ok(Some(frame)),
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => Err(CaptureError::Disconnected),
    }
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

fn send_session_result(sender: &SessionSender, result: Result<CaptureSession, String>) {
    if let Ok(mut g) = sender.lock() {
        if let Some(s) = g.take() {
            let _ = s.send(result);
        }
    }
}

fn build_session_block(
    window_id: u32,
    fps: u32,
    sender: SessionSender,
) -> RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> {
    RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let Some(content_nn) = NonNull::new(content) else {
            send_session_result(&sender, Err(error_message(err)));
            return;
        };
        // SAFETY: The completion handler hands us a non-null
        // SCShareableContent on success; we hold the borrow only for
        // the duration of this block.
        let content_ref: &SCShareableContent = unsafe { content_nn.as_ref() };
        let Some(target) = find_window(content_ref, window_id) else {
            send_session_result(&sender, Err(format!("window-not-found:{window_id}")));
            return;
        };

        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target)
        };
        let frame = unsafe { target.frame() };
        let width = (frame.size.width.max(1.0)) as usize;
        let height = (frame.size.height.max(1.0)) as usize;

        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(width);
            config.setHeight(height);
            config.setShowsCursor(false);
            config.setPixelFormat(PIXEL_FORMAT_BGRA);
            // Cap delivery rate at `fps`. SCK will dedup unchanged
            // frames internally and skip delivery (so we won't see
            // wasted callbacks while a window is idle).
            let interval = CMTime::new(1, fps as i32);
            config.setMinimumFrameInterval(interval);
            // Keep the queue shallow — when our consumer falls
            // behind, we'd rather drop the old frame than buffer.
            config.setQueueDepth(3);
        }

        // Bounded(1): producer (SCK callback) drops on full so the
        // consumer always sees the freshest frame.
        let (frame_tx, frame_rx) = sync_channel::<CapturedFrame>(1);
        let handler = StreamHandler::new(frame_tx);
        let queue = DispatchQueue::new(
            "com.x11web.sidecar-macos.capture",
            DispatchQueueAttr::SERIAL,
        );

        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                Some(ProtocolObject::from_ref(&*handler)),
            )
        };
        if let Err(e) = unsafe {
            stream.addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*handler),
                SCStreamOutputType::Screen,
                Some(&queue),
            )
        } {
            send_session_result(
                &sender,
                Err(format!("addStreamOutput: {}", e.localizedDescription())),
            );
            return;
        }

        // Move the partially-assembled session into an Arc<Mutex<...>>
        // so the start completion can `take()` it once. RcBlock
        // closures must be `Fn`, so we can't move directly.
        let pending = Arc::new(Mutex::new(Some(CaptureSession {
            stream: stream.clone(),
            _handler: handler,
            _queue: queue,
            frame_rx,
        })));
        let pending_for_block = pending.clone();
        let sender_for_block = sender.clone();
        let start_block = RcBlock::new(move |start_err: *mut NSError| {
            let result = if !start_err.is_null() {
                Err(format!("startCapture: {}", error_message(start_err)))
            } else {
                pending_for_block
                    .lock()
                    .ok()
                    .and_then(|mut g| g.take())
                    .map(Ok)
                    .unwrap_or_else(|| Err("session already delivered".into()))
            };
            send_session_result(&sender_for_block, result);
        });
        unsafe {
            stream.startCaptureWithCompletionHandler(Some(&start_block));
        }
    })
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

/// Pull RGBA bytes out of the CMSampleBuffer. Returns `None` for
/// frames we can't or shouldn't process (idle redeliveries with no
/// new pixels, unexpected pixel formats, locking failures).
fn extract_rgba(sample_buffer: &CMSampleBuffer) -> Option<CapturedFrame> {
    let image_buffer = unsafe { sample_buffer.image_buffer() }?;
    let pixel_buffer: &CVImageBuffer = &image_buffer;
    let format = CVPixelBufferGetPixelFormatType(pixel_buffer);
    if format != PIXEL_FORMAT_BGRA {
        tracing::warn!("unexpected SCStream pixel format: {:#x}", format);
        return None;
    }

    let lock = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, LOCK_FLAGS_READ_ONLY) };
    if lock != 0 {
        return None;
    }
    let result = read_locked_pixels(pixel_buffer);
    unsafe {
        CVPixelBufferUnlockBaseAddress(pixel_buffer, LOCK_FLAGS_READ_ONLY);
    }
    result
}

fn read_locked_pixels(pixel_buffer: &CVImageBuffer) -> Option<CapturedFrame> {
    let width = CVPixelBufferGetWidth(pixel_buffer);
    let height = CVPixelBufferGetHeight(pixel_buffer);
    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    let base = CVPixelBufferGetBaseAddress(pixel_buffer);
    if base.is_null() || width == 0 || height == 0 || bytes_per_row < width * 4 {
        return None;
    }
    // SAFETY: We hold the read lock for the duration of this slice's
    // lifetime; CVPixelBufferUnlockBaseAddress runs after we return.
    let raw = unsafe { std::slice::from_raw_parts(base as *const u8, bytes_per_row * height) };

    // BGRA → packed RGBA, dropping any row stride padding.
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = row * bytes_per_row;
        let row_bytes = &raw[start..start + width * 4];
        for px in row_bytes.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }

    Some(CapturedFrame {
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
