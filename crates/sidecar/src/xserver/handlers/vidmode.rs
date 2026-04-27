//! XFree86-VidModeExtension handler (opcode 153).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;
use x11rb_protocol::protocol::xf86vidmode::{
    AddModeLineRequest, DeleteModeLineRequest, GetAllModeLinesRequest, GetDotClocksRequest,
    GetGammaRampRequest, GetGammaRampSizeRequest, GetGammaRequest, GetModeLineRequest,
    GetMonitorRequest, GetViewPortRequest, LockModeSwitchRequest, QueryVersionRequest,
    SetGammaRampRequest, SetGammaRequest, SetViewPortRequest, SwitchModeRequest,
    SwitchToModeRequest, ValidateModeLineRequest,
};
use x11rb_protocol::x11_utils::RequestHeader;

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

/// Build a RequestHeader with a custom minor opcode (for cases where the
/// current code's minor numbering differs from x11rb's constants).
#[inline]
fn vidmode_header(data: &[u8], minor_override: u8) -> RequestHeader {
    RequestHeader {
        major_opcode: data[0],
        minor_opcode: minor_override,
        remaining_length: 0,
    }
}

/// XFree86-VidModeExtension (opcode 153)
pub(crate) fn handle_vidmode_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            let _req = parse_minor!(QueryVersionRequest, data, state, seq, 153, minor);
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 2) // major
                .set_u16(10, 2) // minor
                .build()
        }
        1 => {
            // GetModeLine — return the current mode from the mode list.
            let _req = parse_minor!(GetModeLineRequest, data, state, seq, 153, minor);
            let mode = state
                .vidmode_modes
                .get(state.vidmode_current_mode)
                .cloned()
                .unwrap_or_else(|| {
                    VidModeInfo::default_for_screen(state.screen_width, state.screen_height)
                });
            ReplyBuf::with_extra(seq, 20, state.msb_first) // 32 header + 20 modeline data
                .set_u32(8, mode.dotclock) // dotclock
                .set_u16(12, mode.hdisplay) // hdisplay
                .set_u16(14, mode.hsyncstart) // hsyncstart
                .set_u16(16, mode.hsyncend) // hsyncend
                .set_u16(18, mode.htotal) // htotal
                .set_u16(20, 0) // hskew
                .set_u16(22, mode.vdisplay) // vdisplay
                .set_u16(24, mode.vsyncstart) // vsyncstart
                .set_u16(26, mode.vsyncend) // vsyncend
                .set_u16(28, mode.vtotal) // vtotal
                .set_u32(32, mode.flags) // flags
                // privsize at 36..40 = 0
                .build()
        }
        6 => {
            // GetAllModeLines
            // Return all modes from the mode list.
            let _req = parse_minor!(GetAllModeLinesRequest, data, state, seq, 153, minor);
            let mode_count = state.vidmode_modes.len();
            let mode_size = 48; // bytes per mode line info
            let extra = 4 + mode_size * mode_count; // 4 bytes for count + modes
            let padded = (extra + 3) & !3;
            let mut reply = ReplyBuf::with_extra(seq, padded, state.msb_first)
                .set_u32(8, mode_count as u32);
            for (i, mode) in state.vidmode_modes.iter().enumerate() {
                let off = 36 + i * mode_size;
                reply = reply
                    .set_u32(off, mode.dotclock)
                    .set_u16(off + 4, mode.hdisplay)
                    .set_u16(off + 6, mode.hsyncstart)
                    .set_u16(off + 8, mode.hsyncend)
                    .set_u16(off + 10, mode.htotal)
                    // hskew at off + 12 = 0
                    .set_u16(off + 14, mode.vdisplay)
                    .set_u16(off + 16, mode.vsyncstart)
                    .set_u16(off + 18, mode.vsyncend)
                    .set_u16(off + 20, mode.vtotal)
                    // pad at off + 22..26 = 0
                    .set_u32(off + 26, mode.flags);
            }
            reply.build()
        }
        14 => {
            // GetGamma
            // x11rb uses minor opcode 16 for GetGamma; override header.
            let _req = parse_minor!(GetGammaRequest, data, state, seq, 153, minor, vidmode_header(data, 16));
            // Approximate gamma from stored ramp midpoint:
            // gamma = log(ramp[128]/65535) / log(128/255)
            let (gamma_r, gamma_g, gamma_b) = if let Some(crtc) = state.randr_crtcs.first() {
                let approx = |ramp: &[u16]| -> f64 {
                    if ramp.len() > 128 && ramp[128] > 0 {
                        let mid_val: f64 = ramp[128] as f64 / 65535.0;
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
                (
                    approx(&crtc.gamma_red),
                    approx(&crtc.gamma_green),
                    approx(&crtc.gamma_blue),
                )
            } else {
                (1.0, 1.0, 1.0)
            };
            // Gamma is 16.16 fixed point, 1.0 = 65536
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, (gamma_r * 65536.0) as u32) // red
                .set_u32(12, (gamma_g * 65536.0) as u32) // green
                .set_u32(16, (gamma_b * 65536.0) as u32) // blue
                .build()
        }
        15 => {
            // SetGamma
            // Parse three 16.16 fixed-point gamma values from request
            let req = parse_minor!(SetGammaRequest, data, state, seq, 153, minor);
            let red_fp = req.red;
            let green_fp = req.green;
            let blue_fp = req.blue;
            let gamma_r = red_fp as f64 / 65536.0;
            let gamma_g = green_fp as f64 / 65536.0;
            let gamma_b = blue_fp as f64 / 65536.0;
            // Compute ramp: ramp[i] = ((i/255)^(1/gamma) * 65535) as u16
            let compute_ramp = |gamma: f64| -> Vec<u16> {
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
            };
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = compute_ramp(gamma_r);
                crtc.gamma_green = compute_ramp(gamma_g);
                crtc.gamma_blue = compute_ramp(gamma_b);
            }
            Vec::new()
        }
        16 => {
            // GetGammaRamp
            // x11rb uses minor opcode 17 for GetGammaRamp; override header.
            let req = match GetGammaRampRequest::try_parse_request(
                vidmode_header(data, 17),
                &data[4..],
            ) {
                Ok(r) => r,
                Err(_) => {
                    // Fallback to default size if parsing fails
                    GetGammaRampRequest {
                        screen: 0,
                        size: 256,
                    }
                }
            };
            let size = req.size as usize;
            let ramp_bytes = size * 2; // each value is u16
            let padded = (ramp_bytes + 3) & !3;
            let total_extra = padded * 3; // R, G, B
            let mut reply = ReplyBuf::with_extra(seq, total_extra, state.msb_first)
                .set_u16(8, size as u16); // size
            // Return stored ramp from CRTC, referencing directly to avoid clones
            let linear_ramp: Vec<u16>;
            let ramps: [&[u16]; 3] = if let Some(crtc) = state.randr_crtcs.first() {
                [&crtc.gamma_red, &crtc.gamma_green, &crtc.gamma_blue]
            } else {
                linear_ramp = (0..256)
                    .map(|i| ((i as u32 * 65535) / 255) as u16)
                    .collect();
                [&linear_ramp, &linear_ramp, &linear_ramp]
            };
            {
                let buf = reply.buf_mut();
                for (channel, ramp) in ramps.iter().enumerate() {
                    let base = 32 + channel * padded;
                    for i in 0..size {
                        let val = if i < ramp.len() {
                            ramp[i]
                        } else {
                            // Extrapolate linearly if requested size exceeds stored ramp
                            ((i as u32 * 65535) / (size.max(1) as u32 - 1).max(1)) as u16
                        };
                        let off = base + i * 2;
                        if off + 2 <= buf.len() {
                            let bytes = if state.msb_first {
                                val.to_be_bytes()
                            } else {
                                val.to_le_bytes()
                            };
                            buf[off..off + 2].copy_from_slice(&bytes);
                        }
                    }
                }
            }
            reply.build()
        }
        17 => {
            // SetGammaRamp
            // x11rb uses minor opcode 18 for SetGammaRamp; override header.
            let req = parse_minor!(SetGammaRampRequest, data, state, seq, 153, minor, vidmode_header(data, 18));
            let size = req.size as usize;
            if size == 0 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::LENGTH_ERROR,
                    seq,
                    0,
                    153,
                    minor as u16,
                    state.msb_first,
                );
            }
            let red: Vec<u16> = req.red.iter().take(size).copied().collect();
            let green: Vec<u16> = req.green.iter().take(size).copied().collect();
            let blue: Vec<u16> = req.blue.iter().take(size).copied().collect();
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = red;
                crtc.gamma_green = green;
                crtc.gamma_blue = blue;
            }
            Vec::new()
        }
        18 => {
            // GetGammaRampSize
            // x11rb uses minor opcode 19 for GetGammaRampSize; override header.
            let _req = parse_minor!(GetGammaRampSizeRequest, data, state, seq, 153, minor, vidmode_header(data, 19));
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 256) // size = 256
                .build()
        }
        2 => {
            // GetModeLine (legacy alias)
            // Same as minor 1 — some clients use minor 2
            let mode = state
                .vidmode_modes
                .get(state.vidmode_current_mode)
                .cloned()
                .unwrap_or_else(|| {
                    VidModeInfo::default_for_screen(state.screen_width, state.screen_height)
                });
            ReplyBuf::with_extra(seq, 20, state.msb_first)
                .set_u32(8, mode.dotclock)
                .set_u16(12, mode.hdisplay)
                .set_u16(14, mode.hsyncstart)
                .set_u16(16, mode.hsyncend)
                .set_u16(18, mode.htotal)
                .set_u16(22, mode.vdisplay)
                .set_u16(24, mode.vsyncstart)
                .set_u16(26, mode.vsyncend)
                .set_u16(28, mode.vtotal)
                .set_u32(32, mode.flags)
                .build()
        }
        3 => {
            // SwitchToMode — attempt to switch to a matching mode in the mode list
            // x11rb uses minor opcode 10 for SwitchToMode; override header.
            let req = parse_minor!(SwitchToModeRequest, data, state, seq, 153, minor, vidmode_header(data, 10));
            let screen = req.screen;
            let requested = VidModeInfo {
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
            };
            if state.vidmode_locked {
                debug!(
                    "VidMode SwitchToMode: screen={screen} {}x{} rejected — mode switching is locked",
                    requested.hdisplay, requested.vdisplay,
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
                    "VidMode SwitchToMode: screen={screen} switched to mode {idx} ({}x{}, dotclock={})",
                    requested.hdisplay, requested.vdisplay, requested.dotclock,
                );
            } else {
                debug!(
                    "VidMode SwitchToMode: screen={screen} {}x{} not found in mode list (no change)",
                    requested.hdisplay, requested.vdisplay,
                );
            }
            Vec::new()
        }
        4 => {
            // GetMonitor
            // Return a single monitor with vendor/model strings
            let _req = parse_minor!(GetMonitorRequest, data, state, seq, 153, minor);
            let vendor = b"x11web";
            let model = b"virtual";
            let vendor_len = vendor.len() as u32;
            let model_len = model.len() as u32;
            let vendor_padded = ((vendor_len as usize) + 3) & !3;
            let model_padded = ((model_len as usize) + 3) & !3;
            let hsync_count: u32 = 1;
            let vsync_count: u32 = 1;
            // hsync ranges (2 u32 each: low, high) + vsync ranges
            let extra = 8
                + vendor_padded
                + model_padded
                + (hsync_count as usize * 8)
                + (vsync_count as usize * 8);
            let padded_extra = (extra + 3) & !3;
            let mut off = 32;
            let reply = ReplyBuf::with_extra(seq, padded_extra, state.msb_first)
                .set_u32(8, vendor_len)
                .set_u32(12, model_len)
                .set_u32(16, hsync_count)
                .set_u32(20, vsync_count)
                .set_bytes(off, vendor);
            off += vendor_padded;
            let reply = reply.set_bytes(off, model);
            off += model_padded;
            // HSync range: 31.5 - 80.0 kHz (as 16.16 fixed point * 100)
            let reply = reply
                .set_u32(off, 3150) // low = 31.50 kHz
                .set_u32(off + 4, 8000); // high = 80.00 kHz
            off += 8;
            // VSync range: 56 - 75 Hz
            reply
                .set_u32(off, 5600) // low = 56.00 Hz
                .set_u32(off + 4, 7500) // high = 75.00 Hz
                .build()
        }
        5 => {
            // LockModeSwitch — store the lock state
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
        7 => {
            // AddModeLine — parse and add to mode list
            let req = parse_minor!(AddModeLineRequest, data, state, seq, 153, minor);
            let screen = req.screen;
            let new_mode = VidModeInfo {
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
            };
            debug!(
                "VidMode AddModeLine: screen={screen} {}x{} dotclock={}",
                new_mode.hdisplay, new_mode.vdisplay, new_mode.dotclock,
            );
            // Only add if not already present.
            if !state.vidmode_modes.iter().any(|m| m.matches(&new_mode)) {
                state.vidmode_modes.push(new_mode);
            }
            Vec::new()
        }
        8 => {
            // DeleteModeLine — remove matching mode from the list
            let req = parse_minor!(DeleteModeLineRequest, data, state, seq, 153, minor);
            let screen = req.screen;
            let target = VidModeInfo {
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
            };
            debug!(
                "VidMode DeleteModeLine: screen={screen} {}x{} dotclock={}",
                target.hdisplay, target.vdisplay, target.dotclock,
            );
            if let Some(idx) = state.vidmode_modes.iter().position(|m| m.matches(&target)) {
                // Don't allow deleting the last mode.
                if state.vidmode_modes.len() > 1 {
                    state.vidmode_modes.remove(idx);
                    // Adjust current_mode index if needed.
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
        9 => {
            // ValidateModeLine — always return MODE_OK
            let _req = parse_minor!(ValidateModeLineRequest, data, state, seq, 153, minor);
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, 0) // status = MODE_OK
                .build()
        }
        10 => {
            // SwitchMode — cycle through mode list by zoom direction
            // x11rb uses minor opcode 3 for SwitchMode; override header.
            let req = parse_minor!(SwitchModeRequest, data, state, seq, 153, minor, vidmode_header(data, 3));
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
        11 => {
            // GetViewPort
            let _req = parse_minor!(GetViewPortRequest, data, state, seq, 153, minor);
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, state.vidmode_viewport_x)
                .set_u32(12, state.vidmode_viewport_y)
                .build()
        }
        12 => {
            // SetViewPort — store offset (clamped to screen bounds) and log
            let req = parse_minor!(SetViewPortRequest, data, state, seq, 153, minor);
            let screen = req.screen;
            let x = req.x;
            let y = req.y;
            state.vidmode_viewport_x = x;
            state.vidmode_viewport_y = y;
            debug!("VidMode SetViewPort: screen={screen} x={x} y={y} (stored; virtual display always at 0,0)");
            Vec::new()
        }
        13 => {
            // GetDotClocks
            let _req = parse_minor!(GetDotClocksRequest, data, state, seq, 153, minor);
            let dotclock = state.screen_width as u32 * state.screen_height as u32 * 60;
            ReplyBuf::with_extra(seq, 4, state.msb_first) // 32 header + 4 clock value
                .set_u32(8, 0) // flags = 0
                .set_u32(12, 1) // clocks = 1
                .set_u32(16, dotclock) // maxclocks
                // Padding at 20..32 is zero
                // clock[0] = dot clock of the mode
                .set_u32(32, dotclock)
                .build()
        }
        _ => crate::xserver::core::build_error_bo(
            crate::xserver::core::REQUEST_ERROR,
            seq,
            0,
            153,
            minor as u16,
            state.msb_first,
        ),
    }
}
