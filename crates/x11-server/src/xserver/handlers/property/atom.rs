//! Atom operations — InternAtom (opcode 16), GetAtomName (opcode 17).

use super::*;
use crate::xserver::reply::{serialize_reply, serialize_var_reply};
use x11rb_protocol::protocol::xproto::{
    GetAtomNameReply, GetAtomNameRequest, InternAtomReply, InternAtomRequest,
};

// ---------------------------------------------------------------------------
// Opcode 16: InternAtom
// ---------------------------------------------------------------------------

pub(crate) fn handle_intern_atom(state: &mut ClientState, req: &InternAtomRequest) -> Vec<u8> {
    let seq = state.sequence;
    let name = String::from_utf8_lossy(&req.name).to_string();
    let atom = state.intern_atom(&name, req.only_if_exists);

    serialize_reply(
        &InternAtomReply {
            sequence: seq,
            length: 0,
            atom,
        },
        state.byte_order(),
    )
}

// ---------------------------------------------------------------------------
// Opcode 17: GetAtomName
// ---------------------------------------------------------------------------

pub(crate) fn handle_get_atom_name(state: &mut ClientState, req: &GetAtomNameRequest) -> Vec<u8> {
    let seq = state.sequence;
    let atom = req.atom;

    let Some(name) = state.get_atom_name(atom) else {
        return build_error(ATOM_ERROR, seq, atom, 17, 0);
    };

    serialize_var_reply(
        &GetAtomNameReply {
            sequence: seq,
            length: 0,
            name: name.into_bytes(),
        },
        state.byte_order(),
    )
}
