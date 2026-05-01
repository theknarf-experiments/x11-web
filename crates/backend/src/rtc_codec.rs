//! Encoders for the WebRTC DataChannel binary protocol.
//!
//! One message type for now: `PutImage`. The pixel `data` field is
//! whatever the sidecar produced — currently deflate-compressed RGBA;
//! the frontend inflates with pako after capnp decode.
//!
//! The encoder pre-sizes the message builder to fit the largest
//! plausible single PutImage in one segment so the resulting stream
//! is always single-segment. The frontend's hand-rolled decoder
//! relies on that — it doesn't follow far pointers.

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
