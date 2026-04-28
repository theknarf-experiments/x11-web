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
    /// Serialize to wire bytes. GLX always writes LE on the wire (it's a
    /// pre-byteswap-aware protocol; clients adapt themselves).
    pub(crate) fn encode(self, seq: u16) -> Vec<u8> {
        use crate::xserver::reply::ReplyBuf;
        match self {
            GlxReply::Empty => ReplyBuf::fixed(seq, false).build(),

            GlxReply::Scalar(value) => ReplyBuf::fixed(seq, false)
                .set_u32(8, value)
                .build(),

            GlxReply::SingleValue(bytes) => ReplyBuf::fixed(seq, false)
                .set_bytes(8, &bytes)
                .set_u32(12, 1) // size = 1
                .build(),

            GlxReply::Values { count, data } | GlxReply::Bytes { count, data } => {
                let padded = (data.len() + 3) & !3;
                let mut buf = ReplyBuf::with_extra(seq, padded, false)
                    .set_u32(12, count);
                buf.buf_mut()[32..32 + data.len()].copy_from_slice(&data);
                buf.build()
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

// ---------------------------------------------------------------------------
// GLX management reply builders (non-single-command replies)
// ---------------------------------------------------------------------------

/// MakeCurrent / MakeContextCurrent reply: contextTag in retval[8..12].
pub(crate) fn make_current_reply(seq: u16, context_tag: u32) -> Vec<u8> {
    crate::xserver::reply::ReplyBuf::fixed(seq, false)
        .set_u32(8, context_tag)
        .build()
}

/// IsDirect reply: is_direct at byte[1] (not byte[8]!).
pub(crate) fn is_direct_reply(seq: u16, is_direct: bool) -> Vec<u8> {
    crate::xserver::reply::ReplyBuf::fixed(seq, false)
        .set_data_byte(if is_direct { 1 } else { 0 })
        .build()
}

/// QueryVersion reply: major at [8..12], minor at [12..16].
pub(crate) fn query_version_reply(seq: u16, major: u32, minor: u32) -> Vec<u8> {
    crate::xserver::reply::ReplyBuf::fixed(seq, false)
        .set_u32(8, major)
        .set_u32(12, minor)
        .build()
}

/// GetDrawableAttributes / QueryContext reply.
/// numAttribs at [8..12], key-value pairs at [32..].
pub(crate) fn attrib_pairs_reply(seq: u16, pairs: &[(u32, u32)]) -> Vec<u8> {
    let extra_bytes = pairs.len() * 8;
    let mut buf = crate::xserver::reply::ReplyBuf::with_extra(seq, extra_bytes, false)
        .set_u32(8, pairs.len() as u32);
    for (i, &(key, val)) in pairs.iter().enumerate() {
        buf = buf.set_u32(32 + i * 8, key).set_u32(32 + i * 8 + 4, val);
    }
    buf.build()
}

/// AreTexturesResident reply.
/// all_resident in retval[8..12], per-texture residences in extra data.
pub(crate) fn are_textures_resident_reply(seq: u16, all_resident: bool, residences: &[u8]) -> Vec<u8> {
    let extra_padded = residences.len().div_ceil(4) * 4;
    let mut buf = crate::xserver::reply::ReplyBuf::with_extra(seq, extra_padded, false)
        .set_u32(8, if all_resident { 1 } else { 0 });
    buf.buf_mut()[32..32 + residences.len()].copy_from_slice(residences);
    buf.build()
}

/// GLX query string reply builder.
///
/// Used by QueryExtensionsString (minor 18) and QueryServerString (minor 19).
/// Both have the same layout: pad at [8..12], n at [12..16], string at [32..].
/// The string MUST include the null terminator in `n` (Xorg convention).
pub(crate) fn build_glx_string_reply(seq: u16, string: &[u8]) -> Vec<u8> {
    // Include null terminator — Mesa allocates exactly n bytes without adding '\0'.
    let n = string.len() + 1;
    let padded = (n + 3) & !3;
    let mut reply = crate::xserver::reply::ReplyBuf::with_extra(seq, padded, false)
        .set_u32(12, n as u32);
    reply.buf_mut()[32..32 + string.len()].copy_from_slice(string);
    // Null terminator at [32 + string.len()] is already 0 from ReplyBuf init.
    reply.build()
}
