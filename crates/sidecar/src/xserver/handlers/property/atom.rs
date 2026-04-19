//! Atom operations — InternAtom (opcode 16), GetAtomName (opcode 17).

use super::*;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;

// ---------------------------------------------------------------------------
// Opcode 16: InternAtom
// ---------------------------------------------------------------------------

pub(crate) fn handle_intern_atom(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 16);
    use x11rb_protocol::protocol::xproto::InternAtomRequest;
    let req = match InternAtomRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 16, 0),
    };
    let only_if_exists = req.only_if_exists;
    let name = String::from_utf8_lossy(&req.name).to_string();

    let atom = state.intern_atom(&name, only_if_exists);

    ReplyBuf::fixed(seq, state.msb_first)
        .set_u32(8, atom)
        .build()
}

// ---------------------------------------------------------------------------
// Opcode 17: GetAtomName
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_atom_name(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    require_len!(data, 8, seq, 17);
    use x11rb_protocol::protocol::xproto::GetAtomNameRequest;
    let req = match GetAtomNameRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error(LENGTH_ERROR, seq, 0, 17, 0),
    };
    let atom = req.atom;

    // BadAtom (error code 5) for unknown atoms
    let Some(name) = state.get_atom_name(atom) else {
        return build_error(ATOM_ERROR, seq, atom, 17, 0);
    };
    let name_bytes = name.as_bytes();
    let padded_len = (name_bytes.len() + 3) & !3;

    let mut reply = ReplyBuf::with_extra(seq, padded_len, state.msb_first)
        .set_u16(8, name_bytes.len() as u16);
    reply.buf_mut()[32..32 + name_bytes.len()].copy_from_slice(name_bytes);

    reply.build()
}
