use tracing::debug;

use crate::xserver::ClientState;
use crate::xserver::core::{read_u16_bo, read_u32_bo, write_u16_bo, write_u32_bo};
use super::{pad4, PictFilter};
use crate::xserver::core::require_len;

/// SetPictureFilter (RENDER minor opcode 30).
/// Sets the filter on a picture (nearest, bilinear, etc.).
pub(crate) fn handle_set_picture_filter(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let bo = state.msb_first;
    require_len!(data, 12, seq, 139, data[1] as u16, bo);
    let pic_id = read_u32_bo(data, 4, bo);
    let name_len = read_u16_bo(data, 8, bo) as usize;
    let filter = if data.len() >= 10 + name_len {
        let filter_name = std::str::from_utf8(&data[10..10 + name_len]).unwrap_or("nearest");
        debug!("Render SetPictureFilter: pic={pic_id:#x} filter={filter_name}");
        match filter_name {
            "bilinear" | "best" | "good" => PictFilter::Bilinear,
            _ => PictFilter::Nearest,
        }
    } else {
        PictFilter::Nearest
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
    let total = 32 + extra;

    let mut reply = vec![0u8; total];
    reply[0] = 1; // Reply
    write_u16_bo(&mut reply, 2, seq, bo);
    write_u32_bo(&mut reply, 4, (extra / 4) as u32, bo);
    write_u32_bo(&mut reply, 8, num_aliases, bo);
    write_u32_bo(&mut reply, 12, num_filters, bo);

    // Filter 1: "nearest"
    let mut off = 32;
    reply[off] = filter1.len() as u8;
    off += 1;
    reply[off..off + filter1.len()].copy_from_slice(filter1);
    off = 32 + pad4(filter1_bytes);

    // Filter 2: "bilinear"
    reply[off] = filter2.len() as u8;
    off += 1;
    reply[off..off + filter2.len()].copy_from_slice(filter2);

    reply
}
