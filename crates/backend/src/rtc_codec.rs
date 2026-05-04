//! Encoders for the WebRTC DataChannel binary protocol.
//!
//! Two message types: `PutImage` (live pixel rectangles, see the
//! sidecar) and `WindowThumbnail` (low-rate downscaled previews used
//! by the spawn-popover picker). Both pre-size the message builder
//! so the resulting stream is always single-segment — the frontend's
//! hand-rolled decoder doesn't follow far pointers.

use capnp::message::HeapAllocator;
use x11_web_rtc_wire::wire_capnp;

/// Build a `Frame::PutImage` capnp message and serialise to a single
/// Vec ready for `RTCDataChannel.send`.
pub fn encode_put_image(
    window_id: &str,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    data: &[u8],
) -> Vec<u8> {
    // Reserve enough words in the first segment to fit the message
    // in one go — overhead (~80 bytes) plus the data payload plus
    // the windowId text, rounded up to whole words and a small
    // margin. Keeps the resulting stream single-segment so the
    // frontend can decode without far-pointer handling.
    let words_needed = (data.len() + window_id.len() + 128).div_ceil(8);
    let allocator = HeapAllocator::new().first_segment_words(words_needed as u32);
    let mut message = capnp::message::Builder::new(allocator);
    {
        let frame = message.init_root::<wire_capnp::frame::Builder>();
        let mut put_image = frame.init_put_image();
        put_image.set_window_id(window_id);
        put_image.set_x(x);
        put_image.set_y(y);
        put_image.set_width(width);
        put_image.set_height(height);
        put_image.set_data(data);
    }
    let mut buf = Vec::new();
    capnp::serialize::write_message(&mut buf, &message)
        .expect("capnp serialise to Vec is infallible");
    buf
}

/// Build a `Frame::WindowThumbnail` capnp message and serialise to a
/// single Vec ready for `RTCDataChannel.send`. Mirrors
/// [`encode_put_image`] minus the (x, y) offset — thumbnails always
/// represent the full window.
pub fn encode_window_thumbnail(window_id: &str, width: u16, height: u16, data: &[u8]) -> Vec<u8> {
    let words_needed = (data.len() + window_id.len() + 128).div_ceil(8);
    let allocator = HeapAllocator::new().first_segment_words(words_needed as u32);
    let mut message = capnp::message::Builder::new(allocator);
    {
        let frame = message.init_root::<wire_capnp::frame::Builder>();
        let mut t = frame.init_window_thumbnail();
        t.set_window_id(window_id);
        t.set_width(width);
        t.set_height(height);
        t.set_data(data);
    }
    let mut buf = Vec::new();
    capnp::serialize::write_message(&mut buf, &message)
        .expect("capnp serialise to Vec is infallible");
    buf
}

/// Build a `Frame::WorkspaceSync` capnp message. Carried over the
/// control DataChannel (ordered+reliable). `message` is whatever the
/// caller wants to ship — at the Automerge layer that's the raw
/// `sync::Message::encode` output; this codec doesn't care.
pub fn encode_workspace_sync(workspace_id: &str, message: &[u8]) -> Vec<u8> {
    let words_needed = (message.len() + workspace_id.len() + 128).div_ceil(8);
    let allocator = HeapAllocator::new().first_segment_words(words_needed as u32);
    let mut builder = capnp::message::Builder::new(allocator);
    {
        let frame = builder.init_root::<wire_capnp::frame::Builder>();
        let mut sync = frame.init_workspace_sync();
        sync.set_workspace_id(workspace_id);
        sync.set_message(message);
    }
    let mut buf = Vec::new();
    capnp::serialize::write_message(&mut buf, &builder)
        .expect("capnp serialise to Vec is infallible");
    buf
}

/// Decode a `Frame::WorkspaceSync` carried inbound on the control
/// channel. Returns `(workspace_id, message)` on success.
pub fn decode_workspace_sync(buf: &[u8]) -> Option<(String, Vec<u8>)> {
    let reader = capnp::serialize::read_message(buf, capnp::message::ReaderOptions::new()).ok()?;
    let frame: wire_capnp::frame::Reader = reader.get_root().ok()?;
    let sync = match frame.which().ok()? {
        wire_capnp::frame::Which::WorkspaceSync(s) => s.ok()?,
        _ => return None,
    };
    let workspace_id = sync.get_workspace_id().ok()?.to_str().ok()?.to_string();
    let message = sync.get_message().ok()?.to_vec();
    Some((workspace_id, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_sync_roundtrip() {
        let bytes = encode_workspace_sync("hello-test", b"hello-from-backend");
        let (wid, msg) = decode_workspace_sync(&bytes).expect("decode");
        assert_eq!(wid, "hello-test");
        assert_eq!(&msg[..], b"hello-from-backend");
    }

    #[test]
    fn workspace_sync_empty_message() {
        // Edge case: zero-byte message. Sync handshake start can
        // legitimately be empty (Automerge sometimes signals "I have
        // nothing to send" with a tiny / empty message).
        let bytes = encode_workspace_sync("ws-1", b"");
        let (wid, msg) = decode_workspace_sync(&bytes).expect("decode");
        assert_eq!(wid, "ws-1");
        assert_eq!(msg.len(), 0);
    }
}
