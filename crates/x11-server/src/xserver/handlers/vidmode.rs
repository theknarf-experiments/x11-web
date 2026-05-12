//! XFree86-VidModeExtension handler (opcode 153).
//!
//! Replies are constructed via x11rb's generator-emitted `SerializeEndian`
//! impls, which produce wire-correct bytes for either LSB or MSB clients
//! directly — no per-reply byteswap callbacks needed.

use tracing::debug;

use super::super::client::ClientState;
use x11rb_protocol::protocol::xf86vidmode::{
    AddModeLineRequest, DeleteModeLineRequest, GetAllModeLinesReply, GetAllModeLinesRequest,
    GetDotClocksReply, GetDotClocksRequest, GetGammaRampReply, GetGammaRampRequest,
    GetGammaRampSizeReply, GetGammaRampSizeRequest, GetGammaReply, GetGammaRequest,
    GetModeLineReply, GetModeLineRequest, GetMonitorReply, GetMonitorRequest, GetViewPortReply,
    GetViewPortRequest, LockModeSwitchRequest, ModModeLineRequest, ModeFlag,
    ModeInfo as WireModeInfo, QueryVersionReply, QueryVersionRequest, SetGammaRampRequest,
    SetGammaRequest, SetViewPortRequest, SwitchModeRequest, SwitchToModeRequest,
    ValidateModeLineReply, ValidateModeLineRequest, ADD_MODE_LINE_REQUEST,
    DELETE_MODE_LINE_REQUEST, GET_ALL_MODE_LINES_REQUEST, GET_DOT_CLOCKS_REQUEST,
    GET_GAMMA_RAMP_REQUEST, GET_GAMMA_RAMP_SIZE_REQUEST, GET_GAMMA_REQUEST, GET_MODE_LINE_REQUEST,
    GET_MONITOR_REQUEST, GET_VIEW_PORT_REQUEST, LOCK_MODE_SWITCH_REQUEST, MOD_MODE_LINE_REQUEST,
    QUERY_VERSION_REQUEST, SET_CLIENT_VERSION_REQUEST, SET_GAMMA_RAMP_REQUEST, SET_GAMMA_REQUEST,
    SET_VIEW_PORT_REQUEST, SWITCH_MODE_REQUEST, SWITCH_TO_MODE_REQUEST, VALIDATE_MODE_LINE_REQUEST,
};
use x11rb_protocol::x11_utils::{ByteOrder, SerializeEndian};

use super::parse_minor;

/// XFree86-VidMode mode information.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VidModeInfo {
    pub(crate) dotclock: u32,
    pub(crate) hdisplay: u16,
    pub(crate) hsyncstart: u16,
    pub(crate) hsyncend: u16,
    pub(crate) htotal: u16,
    pub(crate) vdisplay: u16,
    pub(crate) vsyncstart: u16,
    pub(crate) vsyncend: u16,
    pub(crate) vtotal: u16,
    pub(crate) flags: u32,
}

impl VidModeInfo {
    /// Convert to x11rb's wire-format `ModeInfo` so the wire layout
    /// (48-byte struct with hskew + flags + privsize at exact offsets)
    /// is owned by x11rb's `serialize()` instead of by us.
    fn to_wire(&self) -> WireModeInfo {
        WireModeInfo {
            dotclock: self.dotclock,
            hdisplay: self.hdisplay,
            hsyncstart: self.hsyncstart,
            hsyncend: self.hsyncend,
            htotal: self.htotal,
            hskew: 0,
            vdisplay: self.vdisplay,
            vsyncstart: self.vsyncstart,
            vsyncend: self.vsyncend,
            vtotal: self.vtotal,
            flags: ModeFlag::from(self.flags),
            privsize: 0,
        }
    }

    /// Create a default mode matching the given screen dimensions.
    pub(crate) fn default_for_screen(width: u16, height: u16) -> Self {
        Self {
            dotclock: 0,
            hdisplay: width,
            hsyncstart: width,
            hsyncend: width,
            htotal: width,
            vdisplay: height,
            vsyncstart: height,
            vsyncend: height,
            vtotal: height,
            flags: 0,
        }
    }

    /// Check if two modes match (same timing parameters, ignoring flags).
    fn matches(&self, other: &VidModeInfo) -> bool {
        self.dotclock == other.dotclock
            && self.hdisplay == other.hdisplay
            && self.hsyncstart == other.hsyncstart
            && self.hsyncend == other.hsyncend
            && self.htotal == other.htotal
            && self.vdisplay == other.vdisplay
            && self.vsyncstart == other.vsyncstart
            && self.vsyncend == other.vsyncend
            && self.vtotal == other.vtotal
    }
}

/// Number of trailing 4-byte words a serialized reply has past its
/// 32-byte header. x11rb's `length` field counts these.
fn trailing_words(serialized_len: usize) -> u32 {
    const HEADER_BYTES: usize = 32;
    const WORD_BYTES: usize = 4;
    debug_assert!(serialized_len >= HEADER_BYTES);
    debug_assert!((serialized_len - HEADER_BYTES).is_multiple_of(WORD_BYTES));
    u32::try_from((serialized_len - HEADER_BYTES) / WORD_BYTES).expect("reply fits in u32 words")
}

/// Serialize an x11rb reply via the generator-emitted `SerializeEndian`
/// impl — bytes come out already wire-correct for `byte_order` so no
/// per-reply swap callback is needed.
///
/// Some replies' fixed-size header is smaller than the X11 32-byte reply
/// minimum (notably `QueryVersionReply` is 12 bytes); we right-pad with
/// zeros to the wire-format minimum so the client doesn't stall waiting
/// for the missing tail.
fn build_reply<R: SerializeEndian>(reply: &R, byte_order: ByteOrder) -> Vec<u8> {
    const REPLY_MIN: usize = 32;
    let mut bytes = Vec::with_capacity(REPLY_MIN);
    reply.serialize_endian_into(&mut bytes, byte_order);
    if bytes.len() < REPLY_MIN {
        bytes.resize(REPLY_MIN, 0);
    }
    bytes
}

/// XFree86-VidModeExtension (opcode 153)
pub(crate) fn handle_vidmode_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    let vidmode_err = |code: u8, bad_value: u32| {
        crate::xserver::core::build_error(code, seq, bad_value, 153, minor as u16)
    };
    match minor {
        QUERY_VERSION_REQUEST => {
            let _req = parse_minor!(QueryVersionRequest, data, state, seq, 153, minor);
            let reply = QueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: 2,
                minor_version: 2,
            };
            build_reply(&reply, state.byte_order())
        }
        GET_MODE_LINE_REQUEST => {
            let _req = parse_minor!(GetModeLineRequest, data, state, seq, 153, minor);
            let mode = current_mode(state);
            let reply = GetModeLineReply {
                sequence: seq,
                length: 0,
                dotclock: mode.dotclock,
                hdisplay: mode.hdisplay,
                hsyncstart: mode.hsyncstart,
                hsyncend: mode.hsyncend,
                htotal: mode.htotal,
                hskew: 0,
                vdisplay: mode.vdisplay,
                vsyncstart: mode.vsyncstart,
                vsyncend: mode.vsyncend,
                vtotal: mode.vtotal,
                flags: ModeFlag::from(mode.flags),
                private: Vec::new(),
            };
            build_var_reply(&reply, state.byte_order())
        }
        MOD_MODE_LINE_REQUEST => {
            // Modify a mode in the mode list. Treated as a no-op:
            // we accept the request to avoid BadRequest from clients
            // that probe ModModeLine but don't fail the connection.
            let _req: ModModeLineRequest<'_> =
                parse_minor!(ModModeLineRequest, data, state, seq, 153, minor);
            Vec::new()
        }
        SWITCH_MODE_REQUEST => {
            // Cycle through mode list by zoom direction.
            let req = parse_minor!(SwitchModeRequest, data, state, seq, 153, minor);
            let screen = req.screen;
            let zoom = req.zoom as i16;
            if state.vidmode_locked {
                debug!("VidMode SwitchMode: screen={screen} zoom={zoom} rejected — mode switching is locked");
                return Vec::new();
            }
            if !state.vidmode_modes.is_empty() {
                let len = state.vidmode_modes.len();
                if zoom > 0 {
                    state.vidmode_current_mode = (state.vidmode_current_mode + 1) % len;
                } else if zoom < 0 {
                    state.vidmode_current_mode = (state.vidmode_current_mode + len - 1) % len;
                }
                let mode = &state.vidmode_modes[state.vidmode_current_mode];
                debug!(
                    "VidMode SwitchMode: screen={screen} zoom={zoom} -> mode {} ({}x{})",
                    state.vidmode_current_mode, mode.hdisplay, mode.vdisplay,
                );
            }
            Vec::new()
        }
        GET_MONITOR_REQUEST => {
            // Return a single monitor with vendor/model strings.
            let _req = parse_minor!(GetMonitorRequest, data, state, seq, 153, minor);
            let vendor = b"x11web".to_vec();
            let model = b"virtual".to_vec();
            // Pad the vendor string out to a 4-byte boundary; x11rb's
            // serializer asserts that `alignment_pad.len()` matches
            // `align_to_4(vendor_len) - vendor_len`.
            let alignment_pad =
                vec![0u8; crate::xserver::core::align_to_4(vendor.len()) - vendor.len()];
            // Syncrange is u32: low Hz in lower 16 bits, high in upper 16.
            let pack = |low: u16, high: u16| -> u32 { u32::from(low) | (u32::from(high) << 16) };
            let reply = GetMonitorReply {
                sequence: seq,
                length: 0,
                hsync: vec![pack(31, 80)],
                vsync: vec![pack(56, 75)],
                vendor,
                alignment_pad,
                model,
            };
            build_var_reply(&reply, state.byte_order())
        }
        LOCK_MODE_SWITCH_REQUEST => {
            let req = parse_minor!(LockModeSwitchRequest, data, state, seq, 153, minor);
            let screen = req.screen;
            let lock = req.lock;
            state.vidmode_locked = lock != 0;
            debug!(
                "VidMode LockModeSwitch: screen={screen} locked={}",
                state.vidmode_locked
            );
            Vec::new()
        }
        GET_ALL_MODE_LINES_REQUEST => {
            let _req = parse_minor!(GetAllModeLinesRequest, data, state, seq, 153, minor);
            let modeinfo: Vec<WireModeInfo> = state
                .vidmode_modes
                .iter()
                .map(VidModeInfo::to_wire)
                .collect();
            let reply = GetAllModeLinesReply {
                sequence: seq,
                length: 0,
                modeinfo,
            };
            build_var_reply(&reply, state.byte_order())
        }
        ADD_MODE_LINE_REQUEST => {
            let req = parse_minor!(AddModeLineRequest, data, state, seq, 153, minor);
            let new_mode = mode_from_add_modeline(&req);
            debug!(
                "VidMode AddModeLine: screen={} {}x{} dotclock={}",
                req.screen, new_mode.hdisplay, new_mode.vdisplay, new_mode.dotclock,
            );
            if !state.vidmode_modes.iter().any(|m| m.matches(&new_mode)) {
                state.vidmode_modes.push(new_mode);
            }
            Vec::new()
        }
        DELETE_MODE_LINE_REQUEST => {
            let req = parse_minor!(DeleteModeLineRequest, data, state, seq, 153, minor);
            let target = mode_from_delete_modeline(&req);
            debug!(
                "VidMode DeleteModeLine: screen={} {}x{} dotclock={}",
                req.screen, target.hdisplay, target.vdisplay, target.dotclock,
            );
            if let Some(idx) = state.vidmode_modes.iter().position(|m| m.matches(&target)) {
                if state.vidmode_modes.len() > 1 {
                    state.vidmode_modes.remove(idx);
                    if state.vidmode_current_mode >= state.vidmode_modes.len() {
                        state.vidmode_current_mode = 0;
                    } else if state.vidmode_current_mode > idx {
                        state.vidmode_current_mode -= 1;
                    }
                } else {
                    debug!("VidMode DeleteModeLine: refusing to delete last mode");
                }
            }
            Vec::new()
        }
        VALIDATE_MODE_LINE_REQUEST => {
            let _req = parse_minor!(ValidateModeLineRequest, data, state, seq, 153, minor);
            let reply = ValidateModeLineReply {
                sequence: seq,
                length: 0,
                status: 0, // MODE_OK
            };
            build_reply(&reply, state.byte_order())
        }
        SWITCH_TO_MODE_REQUEST => {
            let req = parse_minor!(SwitchToModeRequest, data, state, seq, 153, minor);
            let requested = mode_from_switch_to_mode(&req);
            if state.vidmode_locked {
                debug!(
                    "VidMode SwitchToMode: screen={} {}x{} rejected — mode switching is locked",
                    req.screen, requested.hdisplay, requested.vdisplay,
                );
                return Vec::new();
            }
            if let Some(idx) = state
                .vidmode_modes
                .iter()
                .position(|m| m.matches(&requested))
            {
                state.vidmode_current_mode = idx;
                debug!(
                    "VidMode SwitchToMode: screen={} switched to mode {idx} ({}x{}, dotclock={})",
                    req.screen, requested.hdisplay, requested.vdisplay, requested.dotclock,
                );
            } else {
                debug!(
                    "VidMode SwitchToMode: screen={} {}x{} not found in mode list (no change)",
                    req.screen, requested.hdisplay, requested.vdisplay,
                );
            }
            Vec::new()
        }
        GET_VIEW_PORT_REQUEST => {
            let _req = parse_minor!(GetViewPortRequest, data, state, seq, 153, minor);
            let reply = GetViewPortReply {
                sequence: seq,
                length: 0,
                x: state.vidmode_viewport_x,
                y: state.vidmode_viewport_y,
            };
            build_reply(&reply, state.byte_order())
        }
        SET_VIEW_PORT_REQUEST => {
            let req = parse_minor!(SetViewPortRequest, data, state, seq, 153, minor);
            state.vidmode_viewport_x = req.x;
            state.vidmode_viewport_y = req.y;
            debug!(
                "VidMode SetViewPort: screen={} x={} y={} (stored; virtual display always at 0,0)",
                req.screen, req.x, req.y,
            );
            Vec::new()
        }
        GET_DOT_CLOCKS_REQUEST => {
            let _req = parse_minor!(GetDotClocksRequest, data, state, seq, 153, minor);
            let dotclock = u32::from(state.screen_width) * u32::from(state.screen_height) * 60;
            let reply = GetDotClocksReply {
                sequence: seq,
                length: 0,
                flags: 0u32.into(), // bit 0 clear → continuous clock list of length `clocks`
                clocks: 1,
                maxclocks: dotclock,
                clock: vec![dotclock],
            };
            build_var_reply(&reply, state.byte_order())
        }
        SET_CLIENT_VERSION_REQUEST => {
            // Xxf86vm calls this on every connection to negotiate
            // the protocol version. We don't track per-client
            // versions; just acknowledge with no reply.
            Vec::new()
        }
        SET_GAMMA_REQUEST => {
            let req = parse_minor!(SetGammaRequest, data, state, seq, 153, minor);
            // 16.16 fixed point: 1.0 == 65536.
            let gamma = |fp: u32| -> f64 { fp as f64 / 65536.0 };
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = compute_gamma_ramp(gamma(req.red));
                crtc.gamma_green = compute_gamma_ramp(gamma(req.green));
                crtc.gamma_blue = compute_gamma_ramp(gamma(req.blue));
            }
            Vec::new()
        }
        GET_GAMMA_REQUEST => {
            let _req = parse_minor!(GetGammaRequest, data, state, seq, 153, minor);
            let (gamma_r, gamma_g, gamma_b) = approx_gamma_from_state(state);
            let to_fp = |g: f64| -> u32 { (g * 65536.0) as u32 };
            let reply = GetGammaReply {
                sequence: seq,
                length: 0,
                red: to_fp(gamma_r),
                green: to_fp(gamma_g),
                blue: to_fp(gamma_b),
            };
            build_reply(&reply, state.byte_order())
        }
        GET_GAMMA_RAMP_REQUEST => {
            let req = parse_minor!(GetGammaRampRequest, data, state, seq, 153, minor);
            let size = req.size;
            // x11rb's GetGammaRampReply requires red/green/blue lengths to
            // each equal `(size + 1) & !1` u16s (the spec aligns to a
            // u32 boundary by padding odd sizes with one trailing u16).
            let aligned_size = ((u32::from(size) + 1) & !1) as usize;
            let (red, green, blue) = sample_gamma_ramps(state, size, aligned_size);
            let reply = GetGammaRampReply {
                sequence: seq,
                length: 0,
                size,
                red,
                green,
                blue,
            };
            build_var_reply(&reply, state.byte_order())
        }
        SET_GAMMA_RAMP_REQUEST => {
            let req = parse_minor!(SetGammaRampRequest, data, state, seq, 153, minor);
            let size = req.size as usize;
            if size == 0 {
                return vidmode_err(crate::xserver::core::LENGTH_ERROR, 0);
            }
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = req.red.iter().take(size).copied().collect();
                crtc.gamma_green = req.green.iter().take(size).copied().collect();
                crtc.gamma_blue = req.blue.iter().take(size).copied().collect();
            }
            Vec::new()
        }
        GET_GAMMA_RAMP_SIZE_REQUEST => {
            let _req = parse_minor!(GetGammaRampSizeRequest, data, state, seq, 153, minor);
            let reply = GetGammaRampSizeReply {
                sequence: seq,
                length: 0,
                size: 256,
            };
            build_reply(&reply, state.byte_order())
        }
        _ => vidmode_err(crate::xserver::core::REQUEST_ERROR, 0),
    }
}

// ---------------------------------------------------------------------------
// Mode-list helpers
// ---------------------------------------------------------------------------

fn current_mode(state: &ClientState) -> VidModeInfo {
    state
        .vidmode_modes
        .get(state.vidmode_current_mode)
        .cloned()
        .unwrap_or_else(|| VidModeInfo::default_for_screen(state.screen_width, state.screen_height))
}

fn mode_from_add_modeline(req: &AddModeLineRequest) -> VidModeInfo {
    VidModeInfo {
        dotclock: req.dotclock,
        hdisplay: req.hdisplay,
        hsyncstart: req.hsyncstart,
        hsyncend: req.hsyncend,
        htotal: req.htotal,
        vdisplay: req.vdisplay,
        vsyncstart: req.vsyncstart,
        vsyncend: req.vsyncend,
        vtotal: req.vtotal,
        flags: u32::from(req.flags),
    }
}

fn mode_from_delete_modeline(req: &DeleteModeLineRequest) -> VidModeInfo {
    VidModeInfo {
        dotclock: req.dotclock,
        hdisplay: req.hdisplay,
        hsyncstart: req.hsyncstart,
        hsyncend: req.hsyncend,
        htotal: req.htotal,
        vdisplay: req.vdisplay,
        vsyncstart: req.vsyncstart,
        vsyncend: req.vsyncend,
        vtotal: req.vtotal,
        flags: u32::from(req.flags),
    }
}

fn mode_from_switch_to_mode(req: &SwitchToModeRequest<'_>) -> VidModeInfo {
    VidModeInfo {
        dotclock: req.dotclock,
        hdisplay: req.hdisplay,
        hsyncstart: req.hsyncstart,
        hsyncend: req.hsyncend,
        htotal: req.htotal,
        vdisplay: req.vdisplay,
        vsyncstart: req.vsyncstart,
        vsyncend: req.vsyncend,
        vtotal: req.vtotal,
        flags: u32::from(req.flags),
    }
}

// ---------------------------------------------------------------------------
// Gamma helpers
// ---------------------------------------------------------------------------

fn compute_gamma_ramp(gamma: f64) -> Vec<u16> {
    (0..256)
        .map(|i| {
            let normalized = i as f64 / 255.0;
            let val = if gamma > 0.0 {
                normalized.powf(1.0 / gamma) * 65535.0
            } else {
                normalized * 65535.0
            };
            val.round() as u16
        })
        .collect()
}

fn approx_gamma_from_state(state: &ClientState) -> (f64, f64, f64) {
    // gamma ≈ log(128/255) / log(ramp[128]/65535)
    let approx = |ramp: &[u16]| -> f64 {
        if ramp.len() > 128 && ramp[128] > 0 {
            let mid_val = ramp[128] as f64 / 65535.0;
            let mid_pos: f64 = 128.0 / 255.0;
            if mid_val > 0.0 && mid_val < 1.0 {
                mid_pos.ln() / mid_val.ln()
            } else {
                1.0
            }
        } else {
            1.0
        }
    };
    if let Some(crtc) = state.randr_crtcs.first() {
        (
            approx(&crtc.gamma_red),
            approx(&crtc.gamma_green),
            approx(&crtc.gamma_blue),
        )
    } else {
        (1.0, 1.0, 1.0)
    }
}

fn sample_gamma_ramps(
    state: &ClientState,
    size: u16,
    aligned_size: usize,
) -> (Vec<u16>, Vec<u16>, Vec<u16>) {
    let sample = |stored: &[u16]| -> Vec<u16> {
        (0..aligned_size)
            .map(|i| {
                if i < usize::from(size) {
                    stored
                        .get(i)
                        .copied()
                        // Linear extrapolation if request exceeds stored ramp.
                        .unwrap_or_else(|| {
                            ((i as u32 * 65535) / (usize::from(size).max(1) as u32 - 1).max(1))
                                as u16
                        })
                } else {
                    // Trailing alignment u16 (only present when size is odd).
                    0
                }
            })
            .collect()
    };
    if let Some(crtc) = state.randr_crtcs.first() {
        (
            sample(&crtc.gamma_red),
            sample(&crtc.gamma_green),
            sample(&crtc.gamma_blue),
        )
    } else {
        let linear: Vec<u16> = (0..256)
            .map(|i| ((i as u32 * 65535) / 255) as u16)
            .collect();
        (sample(&linear), sample(&linear), sample(&linear))
    }
}

// ---------------------------------------------------------------------------
// Reply builders for variable-length replies
// ---------------------------------------------------------------------------

fn build_var_reply<R: SerializeEndian>(reply: &R, byte_order: ByteOrder) -> Vec<u8> {
    let mut bytes = Vec::new();
    reply.serialize_endian_into(&mut bytes, byte_order);
    fix_length(&mut bytes, byte_order);
    bytes
}

/// Overwrite the reply's `length` field (bytes 4..8) with the trailing
/// word count derived from the actual buffer. x11rb's `Serialize`
/// emits whatever value the caller put on the struct, but we always
/// build with `length: 0` and let the buffer length be the source of
/// truth — that way we can't drift.
fn fix_length(bytes: &mut Vec<u8>, byte_order: ByteOrder) {
    let length = trailing_words(bytes.len());
    let length_bytes = match byte_order {
        ByteOrder::Lsb => length.to_le_bytes(),
        ByteOrder::Msb => length.to_be_bytes(),
    };
    bytes[4..8].copy_from_slice(&length_bytes);
}
