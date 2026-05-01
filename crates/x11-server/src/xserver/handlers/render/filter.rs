use tracing::debug;

use super::super::parse_minor;
use super::{pad4, PictFilter};
use crate::xserver::reply::ReplyBuf;
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::SetPictureFilterRequest;

/// SetPictureFilter (RENDER minor opcode 30).
/// Sets the filter on a picture (nearest, bilinear, etc.).
pub(crate) fn handle_set_picture_filter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let req = parse_minor!(
        SetPictureFilterRequest,
        data,
        state,
        seq,
        139,
        data[1] as u16
    );

    let pic_id = req.picture;
    let filter_name = std::str::from_utf8(&req.filter).unwrap_or("nearest");
    debug!("Render SetPictureFilter: pic={pic_id:#x} filter={filter_name}");

    let filter = match filter_name {
        "bilinear" | "best" | "good" => PictFilter::Bilinear,
        _ => PictFilter::Nearest,
    };

    if let Some(pic) = state.render.pictures.get_mut(&pic_id) {
        pic.filter = filter;
    }
    Vec::new()
}

pub(crate) fn handle_query_filters(seq: u16, bo: bool) -> Vec<u8> {
    // Return ["nearest", "bilinear"]
    let filter1 = b"nearest";
    let filter2 = b"bilinear";

    // Each alias is 2 bytes (CARD16), num_aliases first
    // Each filter is: 1-byte length + name bytes, padded to 4 bytes
    let num_aliases: u32 = 0;
    let num_filters: u32 = 2;

    let aliases_bytes = 0usize;
    let filter1_bytes = 1 + filter1.len(); // length byte + name
    let filter2_bytes = 1 + filter2.len();
    let filters_bytes = pad4(filter1_bytes) + pad4(filter2_bytes);
    let extra = aliases_bytes + filters_bytes;

    let mut reply = ReplyBuf::with_extra(seq, extra, bo)
        .set_u32(8, num_aliases)
        .set_u32(12, num_filters);

    // Filter 1: "nearest"
    let mut off = 32;
    reply.buf_mut()[off] = filter1.len() as u8;
    off += 1;
    reply.buf_mut()[off..off + filter1.len()].copy_from_slice(filter1);
    off = 32 + pad4(filter1_bytes);

    // Filter 2: "bilinear"
    reply.buf_mut()[off] = filter2.len() as u8;
    off += 1;
    reply.buf_mut()[off..off + filter2.len()].copy_from_slice(filter2);

    reply.build()
}
