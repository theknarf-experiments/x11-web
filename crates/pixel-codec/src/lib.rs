//! Pixel-buffer codec shared between the X11 and macOS sidecars.
//!
//! Encodes captured RGBA frames into the wire format the backend
//! ships over the WebRTC media DataChannel. The frontend decodes via
//! `createImageBitmap`, which natively understands WebP (so the
//! browser does the work, hardware-accelerated where possible).
//!
//! Lossless WebP today; the API is shaped to allow lossy /
//! quality-knob variants later without changing the call sites.

/// Encode an RGBA pixel buffer (`width * height * 4` bytes,
/// row-major, [R, G, B, A] per pixel) into WebP-lossless bytes ready
/// to put into a `DisplayUpdate::PutImage.data` field.
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
/// quality (0.0 – 100.0; 90 is a good UI default — visually
/// identical, ~5-10× faster encode than lossless and smaller
/// payloads). Use [`encode_rgba_lossless`] when fidelity matters
/// (e.g. small text on a high-contrast background).
pub fn encode_rgba_lossy(rgba: &[u8], width: u32, height: u32, quality: f32) -> Vec<u8> {
    let encoder = webp::Encoder::from_rgba(rgba, width, height);
    encoder.encode(quality).to_vec()
}
