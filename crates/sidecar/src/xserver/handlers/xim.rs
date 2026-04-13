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

    let mut reply = [0u8; 32];
    reply[0] = CLIENT_MESSAGE_EVENT | 0x80; // synthetic
    reply[1] = 32; // format
    state.write_u16(&mut reply, 2, state.sequence);
    state.write_u32(&mut reply, 4, client_comm_window);
    state.write_u32(&mut reply, 8, xim_xconnect_atom);
    state.write_u32(&mut reply, 12, state.xim.window); // server comm window
    state.write_u32(&mut reply, 16, 0); // major transport version
    state.write_u32(&mut reply, 20, 0); // minor transport version
    state.write_u32(&mut reply, 24, 20); // divide size

    // Route the reply to the client's communication window.
    if !state.event_router.send_event(client_comm_window, reply.to_vec()) {
        // Client window is on this connection -- deliver locally.
        state.pending_events.push(reply.to_vec());
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
        let mut cm = [0u8; 32];
        cm[0] = CLIENT_MESSAGE_EVENT | 0x80;
        cm[1] = 8; // format 8
        state.write_u16(&mut cm, 2, state.sequence);
        state.write_u32(&mut cm, 4, client_window);
        state.write_u32(&mut cm, 8, xim_protocol_atom);
        let copy_len = reply_data.len().min(20);
        cm[12..12 + copy_len].copy_from_slice(&reply_data[..copy_len]);

        if !state.event_router.send_event(client_window, cm.to_vec()) {
            state.pending_events.push(cm.to_vec());
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
        let mut cm = [0u8; 32];
        cm[0] = CLIENT_MESSAGE_EVENT | 0x80;
        cm[1] = 32; // format 32
        state.write_u16(&mut cm, 2, state.sequence);
        state.write_u32(&mut cm, 4, client_window);
        state.write_u32(&mut cm, 8, xim_protocol_atom);
        state.write_u32(&mut cm, 12, reply_data.len() as u32); // data.l[0] = length
        state.write_u32(&mut cm, 16, prop_atom);               // data.l[1] = property atom

        if !state.event_router.send_event(client_window, cm.to_vec()) {
            state.pending_events.push(cm.to_vec());
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
        0,    // minor
        1, 0, // length = 1 (4 bytes)
        1, 0, // server major version = 1
        0, 0, // server minor version = 0
    ];

    // We don't have an im_id yet (that comes with XIM_OPEN), so send to
    // all pending events for the current client.
    let xim_protocol_atom = state.intern_atom("_XIM_PROTOCOL", false);
    let mut cm = [0u8; 32];
    cm[0] = CLIENT_MESSAGE_EVENT | 0x80;
    cm[1] = 8;
    state.write_u16(&mut cm, 2, state.sequence);
    // We need to figure out the client window. For XIM_CONNECT, we don't
    // have a registered connection yet. The client sent the message to our
    // XIM window, and we need to reply to *their* window. The client window
    // was communicated in the _XIM_XCONNECT handshake.
    // For now, push to pending_events (the client is on this connection).
    state.write_u32(&mut cm, 4, 0); // will be overwritten below
    state.write_u32(&mut cm, 8, xim_protocol_atom);
    cm[12..12 + reply.len()].copy_from_slice(&reply);

    // Push directly to pending events -- the reply goes to the same connection.
    state.pending_events.push(cm.to_vec());

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
        parse_ic_attributes(attr_data, &mut input_style, &mut client_window, &mut focus_window, &mut spot_x, &mut spot_y);
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
        parse_ic_attributes(attr_data, &mut input_style, &mut client_window, &mut focus_window, &mut spot_x, &mut spot_y);

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
/// and send it back via XIM_COMMIT.
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
    let (normal_keysym, shifted_keysym) = super::keycode_to_keysym(keycode);
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

    // Skip modifier keys, function keys, and non-printable keysyms.
    if keysym >= 0xff00 {
        return Vec::new();
    }

    // Convert keysym to UTF-8 string.
    let text = keysym_to_string(keysym);
    if text.is_empty() {
        return Vec::new();
    }

    // Send XIM_COMMIT with the committed string.
    send_xim_commit(state, im_id, ic_id, &text);
    Vec::new()
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

    String::new()
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
