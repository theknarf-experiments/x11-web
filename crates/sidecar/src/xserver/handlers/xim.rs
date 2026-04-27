//! Built-in XIM (X Input Method) server.
//!
//! X11 toolkit apps (GTK, Qt) expect an XIM server to be available for text
//! input. This module implements a minimal XIM server that acts as a
//! passthrough input method -- it accepts connections, creates input contexts,
//! and forwards key events back as committed strings.
//!
//! The XIM transport uses X11 client messages (`_XIM_XCONNECT`, `_XIM_PROTOCOL`)
//! and window properties for messages that exceed the 20-byte client message
//! data limit.

use std::collections::HashMap;
use tracing::debug;

use super::*;
use crate::compose::{ComposeResult, ComposeState};
use crate::xserver::event::{serialize_event, serialize_event_with_layout};
use x11rb_protocol::protocol::xproto::ClientMessageEvent;

/// Wire-field layout for `ClientMessageEvent` with format=8 — only header is
/// byteswapped; the 20-byte data payload is opaque bytes.
const CLIENT_MESSAGE_FORMAT8_LAYOUT: &[(usize, usize)] = &[
    (2, 2), // sequence
    (4, 4), // window
    (8, 4), // type
];

// ---------------------------------------------------------------------------
// XIM protocol major opcodes
// ---------------------------------------------------------------------------

const XIM_CONNECT: u8 = 1;
const XIM_CONNECT_REPLY: u8 = 2;
const XIM_OPEN: u8 = 30;
const XIM_OPEN_REPLY: u8 = 31;
const XIM_CLOSE: u8 = 32;
const XIM_CLOSE_REPLY: u8 = 33;
const XIM_QUERY_EXTENSION: u8 = 40;
const XIM_QUERY_EXTENSION_REPLY: u8 = 41;
const XIM_ENCODING_NEGOTIATION: u8 = 50;
const XIM_ENCODING_NEGOTIATION_REPLY: u8 = 51;
const XIM_SET_EVENT_MASK: u8 = 37;
const XIM_CREATE_IC: u8 = 56;
const XIM_CREATE_IC_REPLY: u8 = 57;
const XIM_DESTROY_IC: u8 = 58;
const XIM_DESTROY_IC_REPLY: u8 = 59;
const XIM_SET_IC_VALUES: u8 = 60;
const XIM_SET_IC_VALUES_REPLY: u8 = 61;
const XIM_GET_IC_VALUES: u8 = 62;
const XIM_GET_IC_VALUES_REPLY: u8 = 63;
const XIM_TRIGGER_NOTIFY: u8 = 35;
#[allow(dead_code)]
const XIM_TRIGGER_NOTIFY_REPLY: u8 = 36;
const XIM_SET_IC_FOCUS: u8 = 68;
const XIM_UNSET_IC_FOCUS: u8 = 69;
const XIM_FORWARD_EVENT: u8 = 82;
const XIM_COMMIT: u8 = 83;
const XIM_RESET_IC: u8 = 64;
const XIM_RESET_IC_REPLY: u8 = 65;
const XIM_SYNC: u8 = 38;
const XIM_SYNC_REPLY: u8 = 39;
const XIM_GEOMETRY: u8 = 66;
const XIM_STR_CONVERSION: u8 = 67;

// XIM input style flags
const XIM_PREEDIT_CALLBACKS: u32 = 0x0002;
const XIM_PREEDIT_POSITION: u32 = 0x0004;
const XIM_PREEDIT_NOTHING: u32 = 0x0008;
#[allow(dead_code)]
const XIM_STATUS_CALLBACKS: u32 = 0x0020;
const XIM_STATUS_NOTHING: u32 = 0x0400;

// XIM preedit callback opcodes
const XIM_PREEDIT_START: u8 = 70;
const XIM_PREEDIT_DRAW: u8 = 72;
#[allow(dead_code)]
const XIM_PREEDIT_CARET: u8 = 73;
const XIM_PREEDIT_DONE: u8 = 74;
const XIM_PREEDIT_START_REPLY: u8 = 71;

// XIM IC attribute IDs (well-known)
const XN_INPUT_STYLE: u16 = 0;
const XN_CLIENT_WINDOW: u16 = 1;
const XN_FOCUS_WINDOW: u16 = 2;
const XN_PREEDIT_ATTRIBUTES: u16 = 3;
#[allow(dead_code)]
const XN_STATUS_ATTRIBUTES: u16 = 4;
const XN_SPOT_LOCATION: u16 = 5;

// ---------------------------------------------------------------------------
// XIM server state
// ---------------------------------------------------------------------------

/// Built-in XIM server state, embedded in each connection's ClientState.
pub(crate) struct XimServer {
    /// XIM server window ID.
    pub(crate) window: u32,
    /// Next input method ID.
    next_im_id: u16,
    /// Next input context ID.
    next_ic_id: u16,
    /// Active input methods (im_id -> XimConnection).
    connections: HashMap<u16, XimConnection>,
    /// Counter for generating unique property atoms for large XIM transport messages.
    xim_transport_counter: u32,
    /// Compose state for dead key / multi-key sequences.
    compose: ComposeState,
}

struct XimConnection {
    client_window: u32,
    /// Input contexts for this connection.
    contexts: HashMap<u16, XimInputContext>,
}

struct XimInputContext {
    input_style: u32,
    client_window: u32,
    focus_window: u32,
    preedit_active: bool,
    spot_x: i16,
    spot_y: i16,
}

impl XimServer {
    pub(crate) fn new(window: u32) -> Self {
        Self {
            window,
            next_im_id: 1,
            next_ic_id: 1,
            connections: HashMap::new(),
            xim_transport_counter: 0,
            compose: ComposeState::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// XIM_XCONNECT handling
// ---------------------------------------------------------------------------

/// Handle an `_XIM_XCONNECT` client message sent to the XIM server window.
/// Returns a client message event to send back to the requesting client.
pub(crate) fn handle_xim_xconnect(state: &mut ClientState, event: &[u8]) -> Vec<u8> {
    // ClientMessage event layout:
    //   [0]    = event type (33 | 0x80)
    //   [1]    = format (32)
    //   [4-7]  = window (our XIM window)
    //   [8-11] = type atom (_XIM_XCONNECT)
    //   [12-15] = client comm window (data.l[0])
    //   [16-19] = major transport version (data.l[1])
    //   [20-23] = minor transport version (data.l[2])

    if event.len() < 28 {
        return Vec::new();
    }

    let client_comm_window = state.read_u32(event, 12);
    debug!(
        "XIM: _XIM_XCONNECT from client window {:#x}",
        client_comm_window
    );

    // Build the _XIM_XCONNECT reply client message.
    // data.l[0] = server comm window (our XIM window)
    // data.l[1] = major transport version (0 = only-CM transport)
    // data.l[2] = minor transport version (0)
    // data.l[3] = divide size (max client message data = 20 bytes)
    let xim_xconnect_atom = state.intern_atom("_XIM_XCONNECT", false);

    let reply = serialize_event(&ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT | 0x80, // synthetic
        format: 32,
        sequence: state.sequence,
        window: client_comm_window,
        type_: xim_xconnect_atom,
        // server comm window, major xport, minor xport, divide size, pad
        data: [state.xim.window, 0, 0, 20, 0].into(),
    }, state.msb_first);

    if !state.event_router.send_event(client_comm_window, reply.clone()) {
        state.pending_events.push(reply);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// XIM_PROTOCOL handling
// ---------------------------------------------------------------------------

/// Handle an `_XIM_PROTOCOL` client message containing an XIM protocol message.
/// The protocol data is packed into the 20-byte client message data area.
pub(crate) fn handle_xim_protocol(state: &mut ClientState, event: &[u8]) -> Vec<u8> {
    // ClientMessage data area starts at byte 12, format is 8 (byte-packed).
    // XIM protocol header: major_opcode (1 byte), minor_opcode (1 byte),
    // length (2 bytes, in 4-byte units).
    if event.len() < 16 {
        return Vec::new();
    }

    let format = event[1] & 0x7F;

    // XIM protocol data is in the client message data area (bytes 12-31).
    let xim_data = if format == 8 {
        // Format 8: raw bytes
        &event[12..]
    } else {
        // Format 32: data is in 32-bit words at bytes 12-31
        &event[12..]
    };

    if xim_data.len() < 4 {
        return Vec::new();
    }

    let major_opcode = xim_data[0];
    let _minor_opcode = xim_data[1];
    // Length is always little-endian in XIM protocol (XIM uses its own byte order)
    let _length = u16::from_le_bytes([xim_data[2], xim_data[3]]);

    debug!(
        "XIM: protocol message major={} minor={}",
        major_opcode, _minor_opcode
    );

    match major_opcode {
        XIM_CONNECT => handle_xim_connect(state, xim_data),
        XIM_OPEN => handle_xim_open(state, xim_data),
        XIM_CLOSE => handle_xim_close(state, xim_data),
        XIM_QUERY_EXTENSION => handle_xim_query_extension(state, xim_data),
        XIM_ENCODING_NEGOTIATION => handle_xim_encoding_negotiation(state, xim_data),
        XIM_CREATE_IC => handle_xim_create_ic(state, xim_data),
        XIM_DESTROY_IC => handle_xim_destroy_ic(state, xim_data),
        XIM_SET_IC_VALUES => handle_xim_set_ic_values(state, xim_data),
        XIM_GET_IC_VALUES => handle_xim_get_ic_values(state, xim_data),
        XIM_SET_IC_FOCUS => handle_xim_set_ic_focus(state, xim_data),
        XIM_UNSET_IC_FOCUS => handle_xim_unset_ic_focus(state, xim_data),
        XIM_RESET_IC => handle_xim_reset_ic(state, xim_data),
        XIM_FORWARD_EVENT => handle_xim_forward_event(state, xim_data),
        XIM_PREEDIT_START_REPLY => handle_xim_preedit_start_reply(state, xim_data),
        XIM_TRIGGER_NOTIFY => handle_xim_trigger_notify(state, xim_data),
        XIM_SYNC => handle_xim_sync(state, xim_data),
        XIM_SYNC_REPLY => {
            // Client acknowledges a sync request — no action needed.
            debug!("XIM: SYNC_REPLY");
            Vec::new()
        }
        XIM_GEOMETRY => {
            // Client notifies IM of geometry change — no action for passthrough IM.
            debug!("XIM: GEOMETRY notification");
            Vec::new()
        }
        XIM_STR_CONVERSION => {
            // String conversion reply from client — not used in passthrough mode.
            debug!("XIM: STR_CONVERSION");
            Vec::new()
        }
        _ => {
            debug!("XIM: unhandled major opcode {}", major_opcode);
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Individual XIM message handlers
// ---------------------------------------------------------------------------

/// Build an XIM protocol reply and send it as a `_XIM_PROTOCOL` client message.
/// `reply_data` is the raw XIM protocol message (including header).
fn send_xim_reply(state: &mut ClientState, im_id: u16, reply_data: &[u8]) {
    // Find the client window for this IM connection.
    let client_window = state
        .xim
        .connections
        .get(&im_id)
        .map(|c| c.client_window)
        .unwrap_or(0);

    if client_window == 0 {
        debug!("XIM: no client window for im_id {}, dropping reply", im_id);
        return;
    }

    send_xim_reply_to(state, client_window, reply_data);
}

/// Send an XIM reply to a specific client window.
fn send_xim_reply_to(state: &mut ClientState, client_window: u32, reply_data: &[u8]) {
    let xim_protocol_atom = state.intern_atom("_XIM_PROTOCOL", false);

    if reply_data.len() <= 20 {
        // Fits in a single client message (format 8).
        let mut data = [0u8; 20];
        data[..reply_data.len()].copy_from_slice(reply_data);
        let cm = serialize_event_with_layout(
            &ClientMessageEvent {
                response_type: CLIENT_MESSAGE_EVENT | 0x80,
                format: 8,
                sequence: state.sequence,
                window: client_window,
                type_: xim_protocol_atom,
                data: data.into(),
            },
            state.msb_first,
            CLIENT_MESSAGE_FORMAT8_LAYOUT,
        );
        if !state.event_router.send_event(client_window, cm.clone()) {
            state.pending_events.push(cm);
        }
    } else {
        // Large message: use the property-based XIM transport.
        // 1. Create a unique property atom for this message.
        // 2. Set the full reply data as a property on the XIM server window.
        // 3. Send a format=32 ClientMessage to the client with:
        //    data.l[0] = length of the property data
        //    data.l[1] = property atom
        let counter = state.xim.xim_transport_counter;
        state.xim.xim_transport_counter = counter.wrapping_add(1);
        let prop_name = format!("_server_XIM_{}", counter);
        let prop_atom = state.intern_atom(&prop_name, false);
        let xim_window = state.xim.window;

        // Store the reply data as a property on the XIM server window.
        if let Some(win) = state.windows.get_mut(&xim_window) {
            win.properties.insert(
                prop_atom,
                PropertyValue {
                    prop_type: 31, // STRING
                    format: 8,
                    data: reply_data.to_vec(),
                },
            );
        }

        // Send a format=32 ClientMessage pointing to the property.
        let cm = serialize_event(
            &ClientMessageEvent {
                response_type: CLIENT_MESSAGE_EVENT | 0x80,
                format: 32,
                sequence: state.sequence,
                window: client_window,
                type_: xim_protocol_atom,
                data: [reply_data.len() as u32, prop_atom, 0, 0, 0].into(),
            },
            state.msb_first,
        );
        if !state.event_router.send_event(client_window, cm.clone()) {
            state.pending_events.push(cm);
        }
    }
}

/// XIM_CONNECT (1): Client requests connection to the IM server.
fn handle_xim_connect(state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    // XIM_CONNECT payload (after header):
    //   byte-order (1 byte), unused (1 byte),
    //   client-major-version (2 bytes), client-minor-version (2 bytes),
    //   number of auth protocols (2 bytes), auth protocol list...
    //
    // We accept unconditionally and reply with XIM_CONNECT_REPLY.

    // XIM_CONNECT_REPLY:
    //   major-opcode = 2, minor = 0, length = 1 (4 bytes of data)
    //   server-major-version (2 bytes), server-minor-version (2 bytes)
    let reply = [
        XIM_CONNECT_REPLY,
        0, // minor
        1,
        0, // length = 1 (4 bytes)
        1,
        0, // server major version = 1
        0,
        0, // server minor version = 0
    ];

    // We don't have an im_id yet (that comes with XIM_OPEN), so send to
    // all pending events for the current client.
    // We need to figure out the client window. For XIM_CONNECT, we don't
    // have a registered connection yet. The client sent the message to our
    // XIM window, and we need to reply to *their* window. The client window
    // was communicated in the _XIM_XCONNECT handshake.
    // For now, push to pending_events (the client is on this connection).
    let xim_protocol_atom = state.intern_atom("_XIM_PROTOCOL", false);
    let mut data = [0u8; 20];
    data[..reply.len()].copy_from_slice(&reply);
    let cm = serialize_event_with_layout(
        &ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT | 0x80,
            format: 8,
            sequence: state.sequence,
            window: 0, // overwritten by event router
            type_: xim_protocol_atom,
            data: data.into(),
        },
        state.msb_first,
        CLIENT_MESSAGE_FORMAT8_LAYOUT,
    );

    // Push directly to pending events -- the reply goes to the same connection.
    state.pending_events.push(cm);

    Vec::new()
}

/// XIM_OPEN (30): Client opens an input method.
fn handle_xim_open(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // XIM_OPEN payload (after 4-byte header):
    //   locale name length (2 bytes), locale name (variable), pad
    //
    // We assign a new im_id and respond with XIM_OPEN_REPLY.

    let im_id = state.xim.next_im_id;
    state.xim.next_im_id += 1;

    // We need a client_window, which should have been set during _XIM_XCONNECT.
    // Since we may not have stored it yet, use 0 and rely on local delivery.
    // In practice, the client that sent XIM_OPEN is the same connection.
    let client_window = if data.len() >= 8 {
        // Some implementations pack the client window in the open message
        0u32
    } else {
        0u32
    };

    state.xim.connections.insert(
        im_id,
        XimConnection {
            client_window,
            contexts: HashMap::new(),
        },
    );

    // XIM_OPEN_REPLY:
    //   major = 31, minor = 0
    //   length = 3 (12 bytes of data after header)
    //   im_id (2 bytes), pad (2 bytes)
    //   number of IM attributes (2 bytes) = 1 (InputStyle)
    //   [attr: id(2) + type(2) + name_len(2) + name + pad]
    //   number of IC attributes (2 bytes) = 3
    //   [attr entries for InputStyle, ClientWindow, FocusWindow]

    // Build the IM attribute list (just InputStyle)
    let im_attr_input_style = build_xim_attr(0, 0x0001, b"inputStyle");
    // Build the IC attribute list
    let ic_attr_0 = build_xim_attr(XN_INPUT_STYLE, 0x0001, b"inputStyle");
    let ic_attr_1 = build_xim_attr(XN_CLIENT_WINDOW, 0x0004, b"clientWindow");
    let ic_attr_2 = build_xim_attr(XN_FOCUS_WINDOW, 0x0004, b"focusWindow");
    let ic_attr_3 = build_xim_attr(XN_PREEDIT_ATTRIBUTES, 0x8001, b"preeditAttributes");
    let ic_attr_4 = build_xim_attr(XN_SPOT_LOCATION, 0x0003, b"spotLocation");

    let mut reply_body = Vec::new();
    // im_id (2 bytes LE)
    reply_body.extend_from_slice(&im_id.to_le_bytes());
    // pad (2 bytes)
    reply_body.extend_from_slice(&[0, 0]);
    // number of IM attributes (2 bytes LE)
    reply_body.extend_from_slice(&1u16.to_le_bytes());
    // pad (2 bytes)
    reply_body.extend_from_slice(&[0, 0]);
    reply_body.extend_from_slice(&im_attr_input_style);
    // number of IC attributes (2 bytes LE)
    reply_body.extend_from_slice(&5u16.to_le_bytes());
    // pad (2 bytes)
    reply_body.extend_from_slice(&[0, 0]);
    reply_body.extend_from_slice(&ic_attr_0);
    reply_body.extend_from_slice(&ic_attr_1);
    reply_body.extend_from_slice(&ic_attr_2);
    reply_body.extend_from_slice(&ic_attr_3);
    reply_body.extend_from_slice(&ic_attr_4);

    let length_words = reply_body.len().div_ceil(4) as u16;
    let mut reply = Vec::with_capacity(4 + reply_body.len());
    reply.push(XIM_OPEN_REPLY);
    reply.push(0);
    reply.extend_from_slice(&length_words.to_le_bytes());
    reply.extend_from_slice(&reply_body);

    // Send XIM_SET_EVENT_MASK to tell the client which events we want forwarded.
    // We want KeyPress (bit 0) and KeyRelease (bit 1) forwarded.
    send_xim_set_event_mask(state, im_id, 0);

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// Send XIM_SET_EVENT_MASK to the client, requesting key events be forwarded.
fn send_xim_set_event_mask(state: &mut ClientState, im_id: u16, ic_id: u16) {
    // XIM_SET_EVENT_MASK:
    //   major = 37, minor = 0, length = 3 (12 bytes)
    //   im_id (2), ic_id (2),
    //   forward_event_mask (4) = KeyPressMask | KeyReleaseMask
    //   synchronous_event_mask (4) = 0
    let mut msg = Vec::with_capacity(16);
    msg.push(XIM_SET_EVENT_MASK);
    msg.push(0);
    msg.extend_from_slice(&3u16.to_le_bytes()); // length in 4-byte units
    msg.extend_from_slice(&im_id.to_le_bytes());
    msg.extend_from_slice(&ic_id.to_le_bytes());
    msg.extend_from_slice(&0x0000_0003u32.to_le_bytes()); // KeyPress | KeyRelease
    msg.extend_from_slice(&0x0000_0000u32.to_le_bytes()); // no sync events

    send_xim_reply(state, im_id, &msg);
}

/// Build an XIM attribute descriptor.
/// Format: attribute_id(2) + type(2) + name_length(2) + name + padding.
fn build_xim_attr(id: u16, attr_type: u16, name: &[u8]) -> Vec<u8> {
    let padded_name_len = (name.len() + 3) & !3;
    let mut buf = Vec::with_capacity(6 + padded_name_len);
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(&attr_type.to_le_bytes());
    buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
    buf.extend_from_slice(name);
    // Pad to 4-byte boundary
    buf.resize(buf.len() + padded_name_len - name.len(), 0);
    buf
}

/// XIM_CLOSE (32): Client closes an input method.
fn handle_xim_close(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let im_id = if data.len() >= 6 {
        u16::from_le_bytes([data[4], data[5]])
    } else {
        return Vec::new();
    };

    debug!("XIM: CLOSE im_id={}", im_id);
    state.xim.connections.remove(&im_id);

    // XIM_CLOSE_REPLY: major=33, minor=0, length=1
    //   im_id (2), pad (2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_CLOSE_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes());
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&[0, 0]);

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_QUERY_EXTENSION (40): Client queries supported XIM extensions.
fn handle_xim_query_extension(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let im_id = if data.len() >= 6 {
        u16::from_le_bytes([data[4], data[5]])
    } else {
        return Vec::new();
    };

    debug!("XIM: QUERY_EXTENSION im_id={}", im_id);

    // XIM_QUERY_EXTENSION_REPLY: major=41, minor=0
    //   im_id (2), pad (2)
    //   number of extensions (2) = 0, pad (2)
    let mut reply = Vec::with_capacity(12);
    reply.push(XIM_QUERY_EXTENSION_REPLY);
    reply.push(0);
    reply.extend_from_slice(&2u16.to_le_bytes()); // length = 2 (8 bytes)
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&[0, 0]); // pad
    reply.extend_from_slice(&0u16.to_le_bytes()); // 0 extensions
    reply.extend_from_slice(&[0, 0]); // pad

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_ENCODING_NEGOTIATION (50): Client negotiates encoding.
fn handle_xim_encoding_negotiation(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let im_id = if data.len() >= 6 {
        u16::from_le_bytes([data[4], data[5]])
    } else {
        return Vec::new();
    };

    debug!("XIM: ENCODING_NEGOTIATION im_id={}", im_id);

    // XIM_ENCODING_NEGOTIATION_REPLY: major=51, minor=0
    //   im_id (2), category (2) = 0 (name), encoding_index (2) = 0, pad (2)
    let mut reply = Vec::with_capacity(12);
    reply.push(XIM_ENCODING_NEGOTIATION_REPLY);
    reply.push(0);
    reply.extend_from_slice(&2u16.to_le_bytes()); // length = 2 (8 bytes)
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&0u16.to_le_bytes()); // category = name
    reply.extend_from_slice(&0u16.to_le_bytes()); // encoding index = 0 (first offered)
    reply.extend_from_slice(&[0, 0]); // pad

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_CREATE_IC (56): Client creates an input context.
fn handle_xim_create_ic(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let im_id = if data.len() >= 6 {
        u16::from_le_bytes([data[4], data[5]])
    } else {
        return Vec::new();
    };

    let ic_id = state.xim.next_ic_id;
    state.xim.next_ic_id += 1;

    // Parse IC attribute values from the request
    let mut input_style = XIM_PREEDIT_NOTHING | XIM_STATUS_NOTHING;
    let mut client_window = 0u32;
    let mut focus_window = 0u32;
    let mut spot_x: i16 = 0;
    let mut spot_y: i16 = 0;

    // IC attributes start after im_id(2) + byte-length-of-attr-list(2)
    if data.len() >= 8 {
        let attr_len = u16::from_le_bytes([data[6], data[7]]) as usize;
        let attr_data = &data[8..data.len().min(8 + attr_len)];
        parse_ic_attributes(
            attr_data,
            &mut input_style,
            &mut client_window,
            &mut focus_window,
            &mut spot_x,
            &mut spot_y,
        );
    }

    if focus_window == 0 {
        focus_window = client_window;
    }

    debug!(
        "XIM: CREATE_IC im_id={} ic_id={} style={:#x} client_win={:#x} focus_win={:#x}",
        im_id, ic_id, input_style, client_window, focus_window
    );

    // Update the connection's client_window if we got one from the IC creation.
    if client_window != 0 {
        if let Some(conn) = state.xim.connections.get_mut(&im_id) {
            if conn.client_window == 0 {
                conn.client_window = client_window;
            }
            conn.contexts.insert(
                ic_id,
                XimInputContext {
                    input_style,
                    client_window,
                    focus_window,
                    preedit_active: false,
                    spot_x,
                    spot_y,
                },
            );
        }
    }

    // XIM_CREATE_IC_REPLY: major=57, minor=0, length=1
    //   im_id (2), ic_id (2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_CREATE_IC_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes()); // length = 1 (4 bytes)
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// Parse IC attributes from a byte buffer, including preedit sub-attributes.
fn parse_ic_attributes(
    data: &[u8],
    input_style: &mut u32,
    client_window: &mut u32,
    focus_window: &mut u32,
    spot_x: &mut i16,
    spot_y: &mut i16,
) {
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let attr_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + attr_len > data.len() {
            break;
        }

        match attr_id {
            XN_INPUT_STYLE if attr_len >= 4 => {
                *input_style = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
            }
            XN_CLIENT_WINDOW if attr_len >= 4 => {
                *client_window = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
            }
            XN_FOCUS_WINDOW if attr_len >= 4 => {
                *focus_window = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
            }
            XN_PREEDIT_ATTRIBUTES => {
                // Nested sub-attributes (e.g. spotLocation)
                parse_preedit_sub_attributes(&data[offset..offset + attr_len], spot_x, spot_y);
            }
            XN_SPOT_LOCATION if attr_len >= 4 => {
                *spot_x = i16::from_le_bytes([data[offset], data[offset + 1]]);
                *spot_y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            }
            _ => {}
        }

        // Advance past the value, padded to 4 bytes
        offset += (attr_len + 3) & !3;
    }
}

/// Parse preedit nested sub-attributes (inside XN_PREEDIT_ATTRIBUTES).
fn parse_preedit_sub_attributes(data: &[u8], spot_x: &mut i16, spot_y: &mut i16) {
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let attr_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + attr_len > data.len() {
            break;
        }

        match attr_id {
            XN_SPOT_LOCATION if attr_len >= 4 => {
                *spot_x = i16::from_le_bytes([data[offset], data[offset + 1]]);
                *spot_y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            }
            _ => {}
        }

        offset += (attr_len + 3) & !3;
    }
}

/// XIM_DESTROY_IC (58): Client destroys an input context.
fn handle_xim_destroy_ic(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);

    debug!("XIM: DESTROY_IC im_id={} ic_id={}", im_id, ic_id);

    if let Some(conn) = state.xim.connections.get_mut(&im_id) {
        conn.contexts.remove(&ic_id);
    }

    // XIM_DESTROY_IC_REPLY: major=59, minor=0, length=1
    //   im_id (2), ic_id (2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_DESTROY_IC_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes());
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_SET_IC_VALUES (60): Client sets IC attribute values.
fn handle_xim_set_ic_values(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);

    debug!("XIM: SET_IC_VALUES im_id={} ic_id={}", im_id, ic_id);

    // Parse and apply attribute values
    if data.len() >= 12 {
        let _byte_len = u16::from_le_bytes([data[8], data[9]]);
        let attr_data = &data[10..];

        let mut input_style = 0u32;
        let mut client_window = 0u32;
        let mut focus_window = 0u32;
        let mut spot_x: i16 = 0;
        let mut spot_y: i16 = 0;
        parse_ic_attributes(
            attr_data,
            &mut input_style,
            &mut client_window,
            &mut focus_window,
            &mut spot_x,
            &mut spot_y,
        );

        if let Some(conn) = state.xim.connections.get_mut(&im_id) {
            if let Some(ic) = conn.contexts.get_mut(&ic_id) {
                if input_style != 0 {
                    ic.input_style = input_style;
                }
                if client_window != 0 {
                    ic.client_window = client_window;
                }
                if focus_window != 0 {
                    ic.focus_window = focus_window;
                }
                if spot_x != 0 || spot_y != 0 {
                    ic.spot_x = spot_x;
                    ic.spot_y = spot_y;
                }
            }
        }
    }

    // XIM_SET_IC_VALUES_REPLY: major=61, minor=0, length=1
    //   im_id (2), ic_id (2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_SET_IC_VALUES_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes());
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_GET_IC_VALUES (62): Client queries IC attribute values.
fn handle_xim_get_ic_values(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);

    debug!("XIM: GET_IC_VALUES im_id={} ic_id={}", im_id, ic_id);

    // Build attribute values from stored IC state
    let mut ic_attrs = Vec::new();
    if let Some(conn) = state.xim.connections.get(&im_id) {
        if let Some(ic) = conn.contexts.get(&ic_id) {
            // InputStyle
            ic_attrs.extend_from_slice(&XN_INPUT_STYLE.to_le_bytes());
            ic_attrs.extend_from_slice(&4u16.to_le_bytes());
            ic_attrs.extend_from_slice(&ic.input_style.to_le_bytes());
            // ClientWindow
            ic_attrs.extend_from_slice(&XN_CLIENT_WINDOW.to_le_bytes());
            ic_attrs.extend_from_slice(&4u16.to_le_bytes());
            ic_attrs.extend_from_slice(&ic.client_window.to_le_bytes());
            // FocusWindow
            ic_attrs.extend_from_slice(&XN_FOCUS_WINDOW.to_le_bytes());
            ic_attrs.extend_from_slice(&4u16.to_le_bytes());
            ic_attrs.extend_from_slice(&ic.focus_window.to_le_bytes());
        }
    }

    // XIM_GET_IC_VALUES_REPLY: major=63, minor=0
    //   im_id (2), ic_id (2), byte_length (2), pad (2), ic_attrs...
    let mut reply_body = Vec::new();
    reply_body.extend_from_slice(&im_id.to_le_bytes());
    reply_body.extend_from_slice(&ic_id.to_le_bytes());
    reply_body.extend_from_slice(&(ic_attrs.len() as u16).to_le_bytes());
    reply_body.extend_from_slice(&[0, 0]); // pad
    reply_body.extend_from_slice(&ic_attrs);

    let length_words = reply_body.len().div_ceil(4) as u16;
    let mut reply = Vec::with_capacity(4 + reply_body.len());
    reply.push(XIM_GET_IC_VALUES_REPLY);
    reply.push(0);
    reply.extend_from_slice(&length_words.to_le_bytes());
    reply.extend_from_slice(&reply_body);

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_SET_IC_FOCUS (68): Client sets focus to an IC.
fn handle_xim_set_ic_focus(_state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let im_id = u16::from_le_bytes([data[4], data[5]]);
        let ic_id = u16::from_le_bytes([data[6], data[7]]);
        debug!("XIM: SET_IC_FOCUS im_id={} ic_id={}", im_id, ic_id);
    }
    // No reply required for SET_IC_FOCUS.
    Vec::new()
}

/// XIM_UNSET_IC_FOCUS (69): Client unsets focus from an IC.
fn handle_xim_unset_ic_focus(_state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let im_id = u16::from_le_bytes([data[4], data[5]]);
        let ic_id = u16::from_le_bytes([data[6], data[7]]);
        debug!("XIM: UNSET_IC_FOCUS im_id={} ic_id={}", im_id, ic_id);
    }
    // No reply required for UNSET_IC_FOCUS.
    Vec::new()
}

/// XIM_FORWARD_EVENT (82): Client forwards a key event to the IM.
///
/// For our passthrough IM, we convert the key event to a committed string
/// and send it back via XIM_COMMIT. Dead keys and multi-key compose
/// sequences are handled through the ComposeState.
fn handle_xim_forward_event(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // XIM_FORWARD_EVENT payload (after 4-byte header):
    //   im_id (2), ic_id (2), flag (2), serial (2),
    //   xEvent (32 bytes -- a KeyPress/KeyRelease event)
    if data.len() < 12 {
        return Vec::new();
    }

    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);
    let _flag = u16::from_le_bytes([data[8], data[9]]);
    let _serial = u16::from_le_bytes([data[10], data[11]]);

    debug!("XIM: FORWARD_EVENT im_id={} ic_id={}", im_id, ic_id);

    // The xEvent starts at offset 12 and is 32 bytes (X11 wire format).
    if data.len() < 44 {
        return Vec::new();
    }

    let x_event = &data[12..44];
    let event_type = x_event[0] & 0x7F;

    // Only process KeyPress events (type 2)
    if event_type != 2 {
        return Vec::new();
    }

    // Extract the keycode from the KeyPress event.
    // KeyPress wire format: detail (keycode) is at byte 1.
    let keycode = x_event[1];
    // State (modifier mask) is at bytes 28-29 in the wire event.
    let modifier_state = u16::from_le_bytes([x_event[28], x_event[29]]);

    // Look up the keysym from the keycode.
    let (normal_keysym, shifted_keysym) = super::resolve_keysym(keycode, &state.custom_keymap);
    let shift_pressed = modifier_state & 0x0001 != 0;
    let caps_lock = modifier_state & 0x0002 != 0;

    let keysym = if (0x61..=0x7a).contains(&normal_keysym) {
        // Letter key: apply shift and caps lock
        if shift_pressed ^ caps_lock {
            shifted_keysym
        } else {
            normal_keysym
        }
    } else if shift_pressed {
        shifted_keysym
    } else {
        normal_keysym
    };

    if keysym == 0 {
        return Vec::new();
    }

    // Skip pure modifier keys (Shift, Control, Alt, Super, etc.)
    // but allow function keys and other special keys through the compose state.
    if is_modifier_keysym(keysym) {
        return Vec::new();
    }

    // Process through compose state for dead key / multi-key sequences.
    match state.xim.compose.process(keysym) {
        ComposeResult::Pass(ks) => {
            // Not composing -- handle normally.
            // Function keys and non-printable keysyms are forwarded back
            // to the client as key events (not committed as text).
            if ks >= 0xff00 {
                // Forward the original X11 event back to the client untouched.
                forward_key_event_to_client(state, im_id, ic_id, data);
                return Vec::new();
            }

            let text = keysym_to_string(ks);
            if text.is_empty() {
                return Vec::new();
            }
            send_xim_commit(state, im_id, ic_id, &text);
        }
        ComposeResult::Consumed => {
            // Key consumed by compose sequence -- start preedit if the IC
            // supports it and show the compose indicator.
            let supports_preedit = state
                .xim
                .connections
                .get(&im_id)
                .and_then(|c| c.contexts.get(&ic_id))
                .map(|ic| ic.input_style & (XIM_PREEDIT_CALLBACKS | XIM_PREEDIT_POSITION) != 0)
                .unwrap_or(false);

            if supports_preedit {
                let is_active = state
                    .xim
                    .connections
                    .get(&im_id)
                    .and_then(|c| c.contexts.get(&ic_id))
                    .map(|ic| ic.preedit_active)
                    .unwrap_or(false);
                if !is_active {
                    send_xim_preedit_start(state, im_id, ic_id);
                }
                // Show a compose indicator (middle dot) while composing.
                send_xim_preedit_draw(state, im_id, ic_id, "\u{00b7}", 1);
            }
        }
        ComposeResult::Composed(text) => {
            // Compose sequence complete -- end preedit and commit text.
            let was_active = state
                .xim
                .connections
                .get(&im_id)
                .and_then(|c| c.contexts.get(&ic_id))
                .map(|ic| ic.preedit_active)
                .unwrap_or(false);
            if was_active {
                send_xim_preedit_done(state, im_id, ic_id);
            }
            send_xim_commit(state, im_id, ic_id, &text);
        }
        ComposeResult::Cancelled(keysyms) => {
            // Compose failed -- end preedit and replay the keysyms as text.
            let was_active = state
                .xim
                .connections
                .get(&im_id)
                .and_then(|c| c.contexts.get(&ic_id))
                .map(|ic| ic.preedit_active)
                .unwrap_or(false);
            if was_active {
                send_xim_preedit_done(state, im_id, ic_id);
            }
            for ks in keysyms {
                let text = keysym_to_string(ks);
                if !text.is_empty() {
                    send_xim_commit(state, im_id, ic_id, &text);
                }
            }
        }
    }

    Vec::new()
}

/// Forward a key event back to the client (for function keys, arrows, etc.).
fn forward_key_event_to_client(state: &mut ClientState, im_id: u16, ic_id: u16, data: &[u8]) {
    // XIM_FORWARD_EVENT reply: send the same event back with the
    // "forwarded from IM" flag cleared so the client processes it.
    if data.len() < 44 {
        return;
    }

    let mut reply_body = Vec::new();
    reply_body.extend_from_slice(&im_id.to_le_bytes());
    reply_body.extend_from_slice(&ic_id.to_le_bytes());
    // flag = 0 (not synchronous, from IM to client)
    reply_body.extend_from_slice(&0u16.to_le_bytes());
    // serial number
    reply_body.extend_from_slice(&data[10..12]);
    // The original X11 event
    reply_body.extend_from_slice(&data[12..44]);

    let length_words = reply_body.len().div_ceil(4) as u16;
    let mut msg = Vec::with_capacity(4 + reply_body.len());
    msg.push(XIM_FORWARD_EVENT);
    msg.push(0);
    msg.extend_from_slice(&length_words.to_le_bytes());
    msg.extend_from_slice(&reply_body);

    send_xim_reply(state, im_id, &msg);
}

/// Check if a keysym is a pure modifier key (Shift, Control, etc.)
/// that should not be processed as text input.
fn is_modifier_keysym(keysym: u32) -> bool {
    matches!(
        keysym,
        0xffe1  // Shift_L
        | 0xffe2  // Shift_R
        | 0xffe3  // Control_L
        | 0xffe4  // Control_R
        | 0xffe5  // Caps_Lock
        | 0xffe6  // Shift_Lock
        | 0xffe7  // Meta_L
        | 0xffe8  // Meta_R
        | 0xffe9  // Alt_L
        | 0xffea  // Alt_R
        | 0xffeb  // Super_L
        | 0xffec  // Super_R
        | 0xffed  // Hyper_L
        | 0xffee  // Hyper_R
        | 0xfe03  // ISO_Level3_Shift (AltGr)
        | 0xfe11  // ISO_Level5_Shift
    )
}

/// Send an XIM_COMMIT message with a committed UTF-8 string.
fn send_xim_commit(state: &mut ClientState, im_id: u16, ic_id: u16, text: &str) {
    let text_bytes = text.as_bytes();

    // XIM_COMMIT: major=83, minor=0
    //   im_id (2), ic_id (2),
    //   flag (2) = 0x0002 (XLookupChars -- string only, no keysym),
    //   byte_length_of_committed_string (2),
    //   committed_string (variable), pad
    let flag: u16 = 0x0002; // XLookupChars
    let padded_len = (text_bytes.len() + 3) & !3;

    let mut body = Vec::new();
    body.extend_from_slice(&im_id.to_le_bytes());
    body.extend_from_slice(&ic_id.to_le_bytes());
    body.extend_from_slice(&flag.to_le_bytes());
    body.extend_from_slice(&(text_bytes.len() as u16).to_le_bytes());
    body.extend_from_slice(text_bytes);
    // Pad to 4-byte boundary
    body.resize(body.len() + padded_len - text_bytes.len(), 0);

    let length_words = body.len().div_ceil(4) as u16;
    let mut msg = Vec::with_capacity(4 + body.len());
    msg.push(XIM_COMMIT);
    msg.push(0);
    msg.extend_from_slice(&length_words.to_le_bytes());
    msg.extend_from_slice(&body);

    send_xim_reply(state, im_id, &msg);
}

/// XIM_RESET_IC (64): Client resets an input context. If there is any
/// in-progress preedit text, it should be returned and the preedit ended.
fn handle_xim_reset_ic(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);

    debug!("XIM: RESET_IC im_id={} ic_id={}", im_id, ic_id);

    // End any active preedit session.
    let was_active = state
        .xim
        .connections
        .get(&im_id)
        .and_then(|c| c.contexts.get(&ic_id))
        .map(|ic| ic.preedit_active)
        .unwrap_or(false);

    if was_active {
        send_xim_preedit_done(state, im_id, ic_id);
    }

    // XIM_RESET_IC_REPLY: major=65, minor=0
    //   im_id(2), ic_id(2),
    //   byte_length_of_committed_string(2) = 0,
    //   committed_string = empty, pad
    let mut reply_body = Vec::new();
    reply_body.extend_from_slice(&im_id.to_le_bytes());
    reply_body.extend_from_slice(&ic_id.to_le_bytes());
    reply_body.extend_from_slice(&0u16.to_le_bytes()); // no committed string
    reply_body.extend_from_slice(&[0, 0]); // pad

    let length_words = reply_body.len().div_ceil(4) as u16;
    let mut reply = Vec::with_capacity(4 + reply_body.len());
    reply.push(XIM_RESET_IC_REPLY);
    reply.push(0);
    reply.extend_from_slice(&length_words.to_le_bytes());
    reply.extend_from_slice(&reply_body);

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_TRIGGER_NOTIFY (35): Client notifies IM of trigger key activation.
/// Reply with XIM_TRIGGER_NOTIFY_REPLY to accept the on/off switch.
fn handle_xim_trigger_notify(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);
    let flag = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    debug!(
        "XIM: TRIGGER_NOTIFY im_id={} ic_id={} flag={}",
        im_id, ic_id, flag
    );

    // XIM_TRIGGER_NOTIFY_REPLY: major=36, minor=0, length=1
    //   im_id(2), ic_id(2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_TRIGGER_NOTIFY_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes());
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// XIM_SYNC (38): Server asks client to sync. Reply immediately with
/// XIM_SYNC_REPLY since our passthrough IM doesn't need synchronization.
fn handle_xim_sync(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }
    let im_id = u16::from_le_bytes([data[4], data[5]]);
    let ic_id = u16::from_le_bytes([data[6], data[7]]);

    debug!("XIM: SYNC im_id={} ic_id={}", im_id, ic_id);

    // XIM_SYNC_REPLY: major=39, minor=0, length=1
    //   im_id(2), ic_id(2)
    let mut reply = Vec::with_capacity(8);
    reply.push(XIM_SYNC_REPLY);
    reply.push(0);
    reply.extend_from_slice(&1u16.to_le_bytes());
    reply.extend_from_slice(&im_id.to_le_bytes());
    reply.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &reply);
    Vec::new()
}

/// Send XIM_PREEDIT_CARET to notify the client of a caret position change.
#[allow(dead_code)]
pub(crate) fn send_xim_preedit_caret(
    state: &mut ClientState,
    im_id: u16,
    ic_id: u16,
    position: i32,
    direction: u32,
    style: u32,
) {
    // XIM_PREEDIT_CARET: major=73, minor=0
    //   im_id(2), ic_id(2),
    //   position(4), direction(4), style(4)
    let mut body = Vec::new();
    body.extend_from_slice(&im_id.to_le_bytes());
    body.extend_from_slice(&ic_id.to_le_bytes());
    body.extend_from_slice(&position.to_le_bytes());
    body.extend_from_slice(&direction.to_le_bytes());
    body.extend_from_slice(&style.to_le_bytes());

    let length_words = body.len().div_ceil(4) as u16;
    let mut msg = Vec::with_capacity(4 + body.len());
    msg.push(XIM_PREEDIT_CARET);
    msg.push(0);
    msg.extend_from_slice(&length_words.to_le_bytes());
    msg.extend_from_slice(&body);

    send_xim_reply(state, im_id, &msg);
}

/// Convert a keysym to a UTF-8 string.
///
/// Handles Latin-1, Latin-2 through Latin-9, Greek, Cyrillic, Armenian,
/// Georgian, currency, Thai, Korean, and the Unicode keysym range.
fn keysym_to_string(keysym: u32) -> String {
    // Latin-1 range (0x0020-0x00ff): direct Unicode mapping
    if (0x0020..=0x007e).contains(&keysym) || (0x00a0..=0x00ff).contains(&keysym) {
        if let Some(c) = char::from_u32(keysym) {
            return c.to_string();
        }
    }

    // Unicode keysym range: 0x01000000 + Unicode codepoint
    if keysym >= 0x0100_0000 {
        let codepoint = keysym - 0x0100_0000;
        if let Some(c) = char::from_u32(codepoint) {
            return c.to_string();
        }
    }

    // Latin-2 through Latin-9, Greek, Cyrillic, and other legacy keysym ranges.
    // These map keysyms to Unicode codepoints via a lookup table.
    if let Some(cp) = legacy_keysym_to_unicode(keysym) {
        if let Some(c) = char::from_u32(cp) {
            return c.to_string();
        }
    }

    String::new()
}

/// Map legacy (non-Latin-1) X11 keysyms to Unicode codepoints.
/// Covers the most commonly used keysyms from the X11 keysym tables.
fn legacy_keysym_to_unicode(keysym: u32) -> Option<u32> {
    // Latin-2 (0x01a1-0x01ff)
    // Latin-3 (0x02a1-0x02ff)
    // Latin-4 (0x03a1-0x03ff)
    // Greek   (0x07a1-0x07f9)
    // Cyrillic (0x06a1-0x06ff)
    // Thai    (0x0da1-0x0dff)
    // Korean  (0x0ea0-0x0eff)
    // Currency (0x20a0-0x20cf)

    // A selection of the most important mappings. For a complete table,
    // see the X11 keysymdef.h file.
    match keysym {
        // Latin-2 common (Polish, Czech, Hungarian, etc.)
        0x01a1 => Some(0x0104), // Aogonek
        0x01a6 => Some(0x015a), // Sacute
        0x01a9 => Some(0x0160), // Scaron
        0x01ab => Some(0x0164), // Tcaron
        0x01ac => Some(0x0179), // Zacute
        0x01ae => Some(0x017d), // Zcaron
        0x01af => Some(0x017b), // Zabovedot
        0x01b1 => Some(0x0105), // aogonek
        0x01b6 => Some(0x015b), // sacute
        0x01b9 => Some(0x0161), // scaron
        0x01bb => Some(0x0165), // tcaron
        0x01bc => Some(0x017a), // zacute
        0x01be => Some(0x017e), // zcaron
        0x01bf => Some(0x017c), // zabovedot
        0x01c0 => Some(0x0154), // Racute
        0x01c3 => Some(0x0102), // Abreve
        0x01c5 => Some(0x0139), // Lacute
        0x01c6 => Some(0x0106), // Cacute
        0x01c8 => Some(0x010c), // Ccaron
        0x01ca => Some(0x0118), // Eogonek
        0x01cc => Some(0x011a), // Ecaron
        0x01cf => Some(0x010e), // Dcaron
        0x01d0 => Some(0x0110), // Dstroke
        0x01d1 => Some(0x0143), // Nacute
        0x01d2 => Some(0x0147), // Ncaron
        0x01d5 => Some(0x0150), // Odoubleacute
        0x01d8 => Some(0x0158), // Rcaron
        0x01d9 => Some(0x016e), // Uring
        0x01db => Some(0x0170), // Udoubleacute
        0x01de => Some(0x0162), // Tcedilla
        0x01e0 => Some(0x0155), // racute
        0x01e3 => Some(0x0103), // abreve
        0x01e5 => Some(0x013a), // lacute
        0x01e6 => Some(0x0107), // cacute
        0x01e8 => Some(0x010d), // ccaron
        0x01ea => Some(0x0119), // eogonek
        0x01ec => Some(0x011b), // ecaron
        0x01ef => Some(0x010f), // dcaron
        0x01f0 => Some(0x0111), // dstroke
        0x01f1 => Some(0x0144), // nacute
        0x01f2 => Some(0x0148), // ncaron
        0x01f5 => Some(0x0151), // odoubleacute
        0x01f8 => Some(0x0159), // rcaron
        0x01f9 => Some(0x016f), // uring
        0x01fb => Some(0x0171), // udoubleacute
        0x01fe => Some(0x0163), // tcedilla

        // Greek (0x07a1-0x07f9) -> Unicode Greek block (0x0384-0x03ce)
        0x07a1 => Some(0x0386), // Greek_ALPHAaccent
        0x07a2 => Some(0x0388), // Greek_EPSILONaccent
        0x07a3 => Some(0x0389), // Greek_ETAaccent
        0x07a4 => Some(0x038a), // Greek_IOTAaccent
        0x07a5 => Some(0x03aa), // Greek_IOTAdiaeresis
        0x07a7 => Some(0x038c), // Greek_OMICRONaccent
        0x07a8 => Some(0x038e), // Greek_UPSILONaccent
        0x07a9 => Some(0x03ab), // Greek_UPSILONdieresis
        0x07ab => Some(0x038f), // Greek_OMEGAaccent
        0x07ae => Some(0x0385), // Greek_accentdieresis
        0x07af => Some(0x2015), // Greek_horizbar
        0x07b1 => Some(0x03b1), // Greek_alpha
        0x07b2 => Some(0x03b2), // Greek_beta
        0x07b3 => Some(0x03b3), // Greek_gamma
        0x07b4 => Some(0x03b4), // Greek_delta
        0x07b5 => Some(0x03b5), // Greek_epsilon
        0x07b6 => Some(0x03b6), // Greek_zeta
        0x07b7 => Some(0x03b7), // Greek_eta
        0x07b8 => Some(0x03b8), // Greek_theta
        0x07b9 => Some(0x03b9), // Greek_iota
        0x07ba => Some(0x03ba), // Greek_kappa
        0x07bb => Some(0x03bb), // Greek_lambda
        0x07bc => Some(0x03bc), // Greek_mu
        0x07bd => Some(0x03bd), // Greek_nu
        0x07be => Some(0x03be), // Greek_xi
        0x07bf => Some(0x03bf), // Greek_omicron
        0x07c0 => Some(0x03c0), // Greek_pi
        0x07c1 => Some(0x03c1), // Greek_rho
        0x07c2 => Some(0x03c3), // Greek_sigma
        0x07c3 => Some(0x03c2), // Greek_finalsmallsigma
        0x07c4 => Some(0x03c4), // Greek_tau
        0x07c5 => Some(0x03c5), // Greek_upsilon
        0x07c6 => Some(0x03c6), // Greek_phi
        0x07c7 => Some(0x03c7), // Greek_chi
        0x07c8 => Some(0x03c8), // Greek_psi
        0x07c9 => Some(0x03c9), // Greek_omega
        0x07d1 => Some(0x0391), // Greek_ALPHA
        0x07d2 => Some(0x0392), // Greek_BETA
        0x07d3 => Some(0x0393), // Greek_GAMMA
        0x07d4 => Some(0x0394), // Greek_DELTA
        0x07d5 => Some(0x0395), // Greek_EPSILON
        0x07d6 => Some(0x0396), // Greek_ZETA
        0x07d7 => Some(0x0397), // Greek_ETA
        0x07d8 => Some(0x0398), // Greek_THETA
        0x07d9 => Some(0x0399), // Greek_IOTA
        0x07da => Some(0x039a), // Greek_KAPPA
        0x07db => Some(0x039b), // Greek_LAMBDA
        0x07dc => Some(0x039c), // Greek_MU
        0x07dd => Some(0x039d), // Greek_NU
        0x07de => Some(0x039e), // Greek_XI
        0x07df => Some(0x039f), // Greek_OMICRON
        0x07e0 => Some(0x03a0), // Greek_PI
        0x07e1 => Some(0x03a1), // Greek_RHO
        0x07e2 => Some(0x03a3), // Greek_SIGMA
        0x07e4 => Some(0x03a4), // Greek_TAU
        0x07e5 => Some(0x03a5), // Greek_UPSILON
        0x07e6 => Some(0x03a6), // Greek_PHI
        0x07e7 => Some(0x03a7), // Greek_CHI
        0x07e8 => Some(0x03a8), // Greek_PSI
        0x07e9 => Some(0x03a9), // Greek_OMEGA

        // Cyrillic (0x06a1-0x06ff) -> Unicode Cyrillic block
        0x06a1 => Some(0x0452), // Serbian_dje
        0x06a2 => Some(0x0453), // Macedonia_gje
        0x06a3 => Some(0x0451), // Cyrillic_io
        0x06a4 => Some(0x0454), // Ukrainian_ie
        0x06a5 => Some(0x0455), // Macedonia_dse
        0x06a6 => Some(0x0456), // Ukrainian_i
        0x06a7 => Some(0x0457), // Ukrainian_yi
        0x06a8 => Some(0x0458), // Cyrillic_je
        0x06a9 => Some(0x0459), // Cyrillic_lje
        0x06aa => Some(0x045a), // Cyrillic_nje
        0x06ab => Some(0x045b), // Serbian_tshe
        0x06ac => Some(0x045c), // Macedonia_kje
        0x06ae => Some(0x045e), // Byelorussian_shortu
        0x06af => Some(0x045f), // Cyrillic_dzhe
        0x06b0 => Some(0x2116), // numerosign
        0x06b1 => Some(0x0402), // Serbian_DJE
        0x06b2 => Some(0x0403), // Macedonia_GJE
        0x06b3 => Some(0x0401), // Cyrillic_IO
        0x06b4 => Some(0x0404), // Ukrainian_IE
        0x06b5 => Some(0x0405), // Macedonia_DSE
        0x06b6 => Some(0x0406), // Ukrainian_I
        0x06b7 => Some(0x0407), // Ukrainian_YI
        0x06b8 => Some(0x0408), // Cyrillic_JE
        0x06b9 => Some(0x0409), // Cyrillic_LJE
        0x06ba => Some(0x040a), // Cyrillic_NJE
        0x06bb => Some(0x040b), // Serbian_TSHE
        0x06bc => Some(0x040c), // Macedonia_KJE
        0x06be => Some(0x040e), // Byelorussian_SHORTU
        0x06bf => Some(0x040f), // Cyrillic_DZHE
        0x06c0 => Some(0x044e), // Cyrillic_yu
        0x06c1 => Some(0x0430), // Cyrillic_a
        0x06c2 => Some(0x0431), // Cyrillic_be
        0x06c3 => Some(0x0446), // Cyrillic_tse
        0x06c4 => Some(0x0434), // Cyrillic_de
        0x06c5 => Some(0x0435), // Cyrillic_ie
        0x06c6 => Some(0x0444), // Cyrillic_ef
        0x06c7 => Some(0x0433), // Cyrillic_ghe
        0x06c8 => Some(0x0445), // Cyrillic_ha
        0x06c9 => Some(0x0438), // Cyrillic_i
        0x06ca => Some(0x0439), // Cyrillic_shorti
        0x06cb => Some(0x043a), // Cyrillic_ka
        0x06cc => Some(0x043b), // Cyrillic_el
        0x06cd => Some(0x043c), // Cyrillic_em
        0x06ce => Some(0x043d), // Cyrillic_en
        0x06cf => Some(0x043e), // Cyrillic_o
        0x06d0 => Some(0x043f), // Cyrillic_pe
        0x06d1 => Some(0x044f), // Cyrillic_ya
        0x06d2 => Some(0x0440), // Cyrillic_er
        0x06d3 => Some(0x0441), // Cyrillic_es
        0x06d4 => Some(0x0442), // Cyrillic_te
        0x06d5 => Some(0x0443), // Cyrillic_u
        0x06d6 => Some(0x0436), // Cyrillic_zhe
        0x06d7 => Some(0x0432), // Cyrillic_ve
        0x06d8 => Some(0x044c), // Cyrillic_softsign
        0x06d9 => Some(0x044b), // Cyrillic_yeru
        0x06da => Some(0x0437), // Cyrillic_ze
        0x06db => Some(0x0448), // Cyrillic_sha
        0x06dc => Some(0x044d), // Cyrillic_e
        0x06dd => Some(0x0449), // Cyrillic_shcha
        0x06de => Some(0x0447), // Cyrillic_che
        0x06df => Some(0x044a), // Cyrillic_hardsign
        0x06e0 => Some(0x042e), // Cyrillic_YU
        0x06e1 => Some(0x0410), // Cyrillic_A
        0x06e2 => Some(0x0411), // Cyrillic_BE
        0x06e3 => Some(0x0426), // Cyrillic_TSE
        0x06e4 => Some(0x0414), // Cyrillic_DE
        0x06e5 => Some(0x0415), // Cyrillic_IE
        0x06e6 => Some(0x0424), // Cyrillic_EF
        0x06e7 => Some(0x0413), // Cyrillic_GHE
        0x06e8 => Some(0x0425), // Cyrillic_HA
        0x06e9 => Some(0x0418), // Cyrillic_I
        0x06ea => Some(0x0419), // Cyrillic_SHORTI
        0x06eb => Some(0x041a), // Cyrillic_KA
        0x06ec => Some(0x041b), // Cyrillic_EL
        0x06ed => Some(0x041c), // Cyrillic_EM
        0x06ee => Some(0x041d), // Cyrillic_EN
        0x06ef => Some(0x041e), // Cyrillic_O
        0x06f0 => Some(0x041f), // Cyrillic_PE
        0x06f1 => Some(0x042f), // Cyrillic_YA
        0x06f2 => Some(0x0420), // Cyrillic_ER
        0x06f3 => Some(0x0421), // Cyrillic_ES
        0x06f4 => Some(0x0422), // Cyrillic_TE
        0x06f5 => Some(0x0423), // Cyrillic_U
        0x06f6 => Some(0x0416), // Cyrillic_ZHE
        0x06f7 => Some(0x0412), // Cyrillic_VE
        0x06f8 => Some(0x042c), // Cyrillic_SOFTSIGN
        0x06f9 => Some(0x042b), // Cyrillic_YERU
        0x06fa => Some(0x0417), // Cyrillic_ZE
        0x06fb => Some(0x0428), // Cyrillic_SHA
        0x06fc => Some(0x042d), // Cyrillic_E
        0x06fd => Some(0x0429), // Cyrillic_SHCHA
        0x06fe => Some(0x0427), // Cyrillic_CHE
        0x06ff => Some(0x042a), // Cyrillic_HARDSIGN

        // Thai (0x0da1-0x0dff) -> Unicode Thai block (0x0e01-0x0e5f)
        k @ 0x0da1..=0x0dff => Some(k - 0x0da1 + 0x0e01),

        // Currency symbols
        0x20a0 => Some(0x20a0), // EcuSign
        0x20a1 => Some(0x20a1), // ColonSign
        0x20a2 => Some(0x20a2), // CruzeiroSign
        0x20a3 => Some(0x20a3), // FFrancSign
        0x20a4 => Some(0x20a4), // LiraSign
        0x20a5 => Some(0x20a5), // MillSign
        0x20a6 => Some(0x20a6), // NairaSign
        0x20a7 => Some(0x20a7), // PesetaSign
        0x20a8 => Some(0x20a8), // RupeeSign
        0x20a9 => Some(0x20a9), // WonSign
        0x20aa => Some(0x20aa), // NewSheqelSign
        0x20ab => Some(0x20ab), // DongSign
        0x20ac => Some(0x20ac), // EuroSign

        // Special typographic
        0x0ad0 => Some(0x2014), // emdash
        0x0ad1 => Some(0x2013), // endash
        0x0aae => Some(0x2026), // ellipsis
        0x0aa5 => Some(0x2022), // enfilledcircbullet
        0x0ad2 => Some(0x2018), // leftsinglequotemark
        0x0ad3 => Some(0x2019), // rightsinglequotemark
        0x0ad4 => Some(0x201c), // leftdoublequotemark
        0x0ad5 => Some(0x201d), // rightdoublequotemark
        0x0af1 => Some(0x2010), // hyphen
        0x0ab8 => Some(0x2003), // emspace
        0x0abb => Some(0x2002), // enspace
        0x0ac9 => Some(0x2122), // trademark

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Preedit callback support
// ---------------------------------------------------------------------------

/// XIM_PREEDIT_START_REPLY (71): Client acknowledges preedit start.
fn handle_xim_preedit_start_reply(_state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 8 {
        let im_id = u16::from_le_bytes([data[4], data[5]]);
        let ic_id = u16::from_le_bytes([data[6], data[7]]);
        // data[8..12] contains the return value (max preedit length), but we
        // don't need to act on it for basic preedit callback support.
        debug!("XIM: PREEDIT_START_REPLY im_id={} ic_id={}", im_id, ic_id);
    }
    Vec::new()
}

/// Start preedit composition for an IC.
fn send_xim_preedit_start(state: &mut ClientState, im_id: u16, ic_id: u16) {
    // XIM_PREEDIT_START: major=70, minor=0, length=1
    //   im_id(2), ic_id(2)
    let mut msg = Vec::with_capacity(8);
    msg.push(XIM_PREEDIT_START);
    msg.push(0);
    msg.extend_from_slice(&1u16.to_le_bytes()); // length = 1 (4 bytes)
    msg.extend_from_slice(&im_id.to_le_bytes());
    msg.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &msg);

    // Mark the IC as having an active preedit session.
    if let Some(conn) = state.xim.connections.get_mut(&im_id) {
        if let Some(ic) = conn.contexts.get_mut(&ic_id) {
            ic.preedit_active = true;
        }
    }
}

/// Update preedit string for an IC.
fn send_xim_preedit_draw(state: &mut ClientState, im_id: u16, ic_id: u16, text: &str, caret: i32) {
    let text_bytes = text.as_bytes();
    let padded_text_len = (text_bytes.len() + 3) & !3;

    // XIM_PREEDIT_DRAW: major=72, minor=0, length=variable
    //   im_id(2), ic_id(2),
    //   caret(4) - cursor position in preedit string
    //   chg_first(4) - first changed character
    //   chg_length(4) - number of characters changed
    //   status(4) - 0=normal, 1=highlighted, 2=reverse
    //   string_length(2), string_data(variable), pad
    //   (no feedback array for basic support)
    let chg_first: i32 = 0;
    let chg_length: i32 = text.chars().count() as i32;
    let status: u32 = 0; // normal

    let mut body = Vec::new();
    body.extend_from_slice(&im_id.to_le_bytes());
    body.extend_from_slice(&ic_id.to_le_bytes());
    body.extend_from_slice(&caret.to_le_bytes());
    body.extend_from_slice(&chg_first.to_le_bytes());
    body.extend_from_slice(&chg_length.to_le_bytes());
    body.extend_from_slice(&status.to_le_bytes());
    body.extend_from_slice(&(text_bytes.len() as u16).to_le_bytes());
    body.extend_from_slice(text_bytes);
    // Pad string to 4-byte boundary
    body.resize(body.len() + padded_text_len - text_bytes.len(), 0);
    // No feedback array: feedback_length = 0
    body.extend_from_slice(&0u16.to_le_bytes());
    // Pad to 4-byte boundary if needed
    if body.len() % 4 != 0 {
        let pad = 4 - (body.len() % 4);
        body.resize(body.len() + pad, 0);
    }

    let length_words = body.len().div_ceil(4) as u16;
    let mut msg = Vec::with_capacity(4 + body.len());
    msg.push(XIM_PREEDIT_DRAW);
    msg.push(0);
    msg.extend_from_slice(&length_words.to_le_bytes());
    msg.extend_from_slice(&body);

    send_xim_reply(state, im_id, &msg);
}

/// End preedit composition for an IC.
fn send_xim_preedit_done(state: &mut ClientState, im_id: u16, ic_id: u16) {
    // XIM_PREEDIT_DONE: major=74, minor=0, length=1
    //   im_id(2), ic_id(2)
    let mut msg = Vec::with_capacity(8);
    msg.push(XIM_PREEDIT_DONE);
    msg.push(0);
    msg.extend_from_slice(&1u16.to_le_bytes()); // length = 1 (4 bytes)
    msg.extend_from_slice(&im_id.to_le_bytes());
    msg.extend_from_slice(&ic_id.to_le_bytes());

    send_xim_reply(state, im_id, &msg);

    // Mark the IC as no longer having an active preedit session.
    if let Some(conn) = state.xim.connections.get_mut(&im_id) {
        if let Some(ic) = conn.contexts.get_mut(&ic_id) {
            ic.preedit_active = false;
        }
    }
}

/// Find the focused input context. Returns (im_id, ic_id) of the first IC
/// that uses a preedit-capable input style, or falls back to any IC.
fn find_focused_ic(state: &ClientState) -> Option<(u16, u16)> {
    // Prefer an IC with PREEDIT_CALLBACKS or PREEDIT_POSITION style.
    let mut fallback: Option<(u16, u16)> = None;
    for (&im_id, conn) in &state.xim.connections {
        for (&ic_id, ic) in &conn.contexts {
            if ic.input_style & (XIM_PREEDIT_CALLBACKS | XIM_PREEDIT_POSITION) != 0 {
                return Some((im_id, ic_id));
            }
            if fallback.is_none() {
                fallback = Some((im_id, ic_id));
            }
        }
    }
    fallback
}

/// Handle a composition event from the frontend.
/// `phase`: "start", "update", "end"
/// `text`: the preedit/committed text
pub(crate) fn handle_composition_event(state: &mut ClientState, phase: &str, text: &str) {
    let (im_id, ic_id) = match find_focused_ic(state) {
        Some(ids) => ids,
        None => {
            debug!("XIM: composition event but no active IC, dropping");
            return;
        }
    };

    debug!(
        "XIM: composition event phase={} text={:?} im_id={} ic_id={}",
        phase, text, im_id, ic_id
    );

    match phase {
        "start" => {
            send_xim_preedit_start(state, im_id, ic_id);
        }
        "update" => {
            let caret = text.chars().count() as i32;
            send_xim_preedit_draw(state, im_id, ic_id, text, caret);
        }
        "end" => {
            // Commit the final text, then end preedit.
            if !text.is_empty() {
                send_xim_commit(state, im_id, ic_id, text);
            }
            send_xim_preedit_done(state, im_id, ic_id);
        }
        _ => {
            debug!("XIM: unknown composition phase {:?}", phase);
        }
    }
}

/// Check if a ClientMessage event is an XIM message targeting our server window
/// and handle it. Returns true if the message was handled.
pub(crate) fn maybe_handle_xim_message(state: &mut ClientState, event: &[u8]) -> bool {
    if event.len() < 32 {
        return false;
    }

    let event_type = event[0] & 0x7F;
    if event_type != CLIENT_MESSAGE_EVENT {
        return false;
    }

    let target_window = state.read_u32(event, 4);
    let msg_type = state.read_u32(event, 8);

    let xim_xconnect_atom = state.intern_atom("_XIM_XCONNECT", true);
    let xim_protocol_atom = state.intern_atom("_XIM_PROTOCOL", true);

    // Only handle messages directed at the XIM window.
    if target_window != state.xim.window {
        return false;
    }

    if xim_xconnect_atom != 0 && msg_type == xim_xconnect_atom {
        handle_xim_xconnect(state, event);
        return true;
    }

    if xim_protocol_atom != 0 && msg_type == xim_protocol_atom {
        handle_xim_protocol(state, event);
        return true;
    }

    false
}
