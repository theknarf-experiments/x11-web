//! Type-safe GLX reply builders.
//!
//! These encode the wire format rules for `xGLXSingleReply` so that callers
//! can't accidentally put data at the wrong offset or leave extra data that
//! triggers XCB's `xcb_xlib_extra_reply_data_left` assertion.
//!
//! # Wire layout (xGLXSingleReply)
//!
//! ```text
//!   [0]      type = 1 (X_Reply)
//!   [1]      unused
//!   [2..4]   sequence number (LE)
//!   [4..8]   length — extra 4-byte words beyond the 32-byte header
//!   [8..12]  retval — scalar return value
//!   [12..16] size   — element count for variable-length data
//!   [16..32] padding
//!   [32..]   variable-length data (only if length > 0)
//! ```
//!
//! # Mesa's indirect GL rules
//!
//! Mesa's `__GLX_SINGLE_GET_SIZE(compsize)` reads `reply.size`.
//! - **size == 0**: no data at all.
//! - **size == 1**: single value lives in `retval` (bytes 8–11).
//!   Mesa does NOT call `_XRead`. `length` MUST be 0.
//! - **size > 1**: `size` elements in extra data (bytes 32+).
//!   Mesa calls `_XRead(dpy, buf, size * elem_size)`.
//!   `length` = padded byte count / 4.

/// GLX single-command reply.
///
/// Each variant maps to exactly one valid wire encoding.
/// You cannot accidentally mix up retval and size.
pub(crate) enum GlxReply {
    /// No data — for void-ish replies or empty results.
    /// Wire: retval=0, size=0, length=0.
    Empty,

    /// Scalar return (GetError, IsEnabled, IsList, IsTexture, GenLists).
    /// The value goes in retval[8..12]. size=0, length=0.
    Scalar(u32),

    /// Single 4-byte element (GetIntegerv with N=1, GetFloatv with N=1, etc.).
    /// The value goes in retval[8..12]. size=1, length=0.
    /// Mesa reads `retval` when size==1, so NO extra data.
    SingleValue([u8; 4]),

    /// Multiple elements (GetIntegerv with N>1, GetLightfv, etc.).
    /// Element count in size[12..16], data at [32..], length=padded/4.
    Values {
        count: u32,
        data: Vec<u8>,
    },

    /// Byte string (GetString, GetPolygonStipple, GetTexImage).
    /// Byte count in size[12..16], data at [32..], length=padded/4.
    Bytes {
        count: u32,
        data: Vec<u8>,
    },
}

impl GlxReply {
    /// Serialize to wire bytes.
    pub(crate) fn encode(self, seq: u16) -> Vec<u8> {
        match self {
            GlxReply::Empty => {
                let mut buf = [0u8; 32];
                buf[0] = 1;
                buf[2..4].copy_from_slice(&seq.to_le_bytes());
                buf.to_vec()
            }

            GlxReply::Scalar(value) => {
                let mut buf = [0u8; 32];
                buf[0] = 1;
                buf[2..4].copy_from_slice(&seq.to_le_bytes());
                buf[8..12].copy_from_slice(&value.to_le_bytes()); // retval
                buf.to_vec()
            }

            GlxReply::SingleValue(bytes) => {
                let mut buf = [0u8; 32];
                buf[0] = 1;
                buf[2..4].copy_from_slice(&seq.to_le_bytes());
                buf[8..8 + bytes.len()].copy_from_slice(&bytes); // retval
                buf[12..16].copy_from_slice(&1u32.to_le_bytes()); // size = 1
                // length = 0 — NO extra data
                buf.to_vec()
            }

            GlxReply::Values { count, data } | GlxReply::Bytes { count, data } => {
                let padded = (data.len() + 3) & !3;
                let extra_words = padded / 4;
                let mut buf = vec![0u8; 32 + padded];
                buf[0] = 1;
                buf[2..4].copy_from_slice(&seq.to_le_bytes());
                buf[4..8].copy_from_slice(&(extra_words as u32).to_le_bytes()); // length
                buf[12..16].copy_from_slice(&count.to_le_bytes()); // size
                if !data.is_empty() {
                    buf[32..32 + data.len()].copy_from_slice(&data);
                }
                buf
            }
        }
    }

    // -- Convenience constructors --

    /// Build reply from a slice of i32 values.
    /// Automatically picks SingleValue (N==1) or Values (N>1).
    pub(crate) fn from_i32s(values: &[i32]) -> Self {
        if values.len() == 1 {
            Self::SingleValue(values[0].to_le_bytes())
        } else {
            let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            Self::Values { count: values.len() as u32, data }
        }
    }

    /// Build reply from a slice of u32 values.
    pub(crate) fn from_u32s(values: &[u32]) -> Self {
        if values.len() == 1 {
            Self::SingleValue(values[0].to_le_bytes())
        } else {
            let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            Self::Values { count: values.len() as u32, data }
        }
    }

    /// Build reply from a slice of f32 values.
    pub(crate) fn from_f32s(values: &[f32]) -> Self {
        if values.len() == 1 {
            Self::SingleValue(values[0].to_le_bytes())
        } else {
            let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            Self::Values { count: values.len() as u32, data }
        }
    }

    /// Build reply from a slice of f64 values.
    pub(crate) fn from_f64s(values: &[f64]) -> Self {
        // f64 is 8 bytes — never fits in SingleValue (max 4 bytes)
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        Self::Values { count: values.len() as u32, data }
    }

    /// Build reply from a slice of bool values (as u8).
    pub(crate) fn from_bools(values: &[u8]) -> Self {
        if values.len() == 1 {
            Self::SingleValue([values[0], 0, 0, 0])
        } else {
            Self::Values { count: values.len() as u32, data: values.to_vec() }
        }
    }

    /// Build reply from a byte string (GetString, GetPolygonStipple, GetTexImage).
    /// `byte_count` is the number of significant bytes (Mesa reads this many).
    pub(crate) fn from_bytes(byte_count: u32, data: Vec<u8>) -> Self {
        Self::Bytes { count: byte_count, data }
    }
}

/// GLX query string reply builder.
///
/// Used by QueryExtensionsString (minor 18) and QueryServerString (minor 19).
/// Both have the same layout: pad at [8..12], n at [12..16], string at [32..].
/// The string MUST include the null terminator in `n` (Xorg convention).
pub(crate) fn build_glx_string_reply(seq: u16, string: &[u8]) -> Vec<u8> {
    // Include null terminator — Mesa allocates exactly n bytes without adding '\0'.
    let n = (string.len() + 1) as u32;
    let padded = ((n as usize) + 3) & !3;
    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    // [8..12] = pad, leave as zero
    reply[12..16].copy_from_slice(&n.to_le_bytes());
    if !string.is_empty() {
        reply[32..32 + string.len()].copy_from_slice(string);
    }
    // Null terminator at [32 + string.len()] is already 0 from vec![0u8; ...]
    reply
}
