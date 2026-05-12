use tracing::debug;

use super::super::parse_minor;
use super::PictFilter;
use crate::xserver::reply::serialize_var_reply;
use crate::xserver::ClientState;
use x11rb_protocol::protocol::render::{QueryFiltersReply, SetPictureFilterRequest};
use x11rb_protocol::protocol::xproto::Str;
use x11rb_protocol::x11_utils::ByteOrder;

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
    let byte_order = if bo { ByteOrder::Msb } else { ByteOrder::Lsb };
    serialize_var_reply(
        &QueryFiltersReply {
            sequence: seq,
            length: 0,
            aliases: Vec::new(),
            filters: vec![
                Str {
                    name: b"nearest".to_vec(),
                },
                Str {
                    name: b"bilinear".to_vec(),
                },
            ],
        },
        byte_order,
    )
}
