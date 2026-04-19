//! XFree86-VidModeExtension handler (opcode 153).

use tracing::debug;

use super::super::client::ClientState;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

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

    /// Parse a VidModeInfo from raw X11 request data at the given offset.
    /// The ModeInfo struct layout: dotclock(4) + hdisplay(2) + hsyncstart(2)
    /// + hsyncend(2) + htotal(2) + hskew(2) + vdisplay(2) + vsyncstart(2)
    /// + vsyncend(2) + vtotal(2) + pad(4) + flags(4) = 32 bytes minimum.
    fn parse(state: &ClientState, data: &[u8], offset: usize) -> Self {
        Self {
            dotclock: state.read_u32(data, offset),
            hdisplay: state.read_u16(data, offset + 4),
            hsyncstart: state.read_u16(data, offset + 6),
            hsyncend: state.read_u16(data, offset + 8),
            htotal: state.read_u16(data, offset + 10),
            // hskew at offset + 12, skip
            vdisplay: state.read_u16(data, offset + 14),
            vsyncstart: state.read_u16(data, offset + 16),
            vsyncend: state.read_u16(data, offset + 18),
            vtotal: state.read_u16(data, offset + 20),
            // pad at offset + 22..26
            flags: state.read_u32(data, offset + 26),
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

/// XFree86-VidModeExtension (opcode 153)
pub(crate) fn handle_vidmode_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 2) // major
                .set_u16(10, 2) // minor
                .build()
        }
        1 => {
            // GetModeLine
            // Return the current mode from the mode list.
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
            require_len!(data, 20, seq, 153, minor as u16, state.msb_first);
            let red_fp = state.read_u32(data, 8);
            let green_fp = state.read_u32(data, 12);
            let blue_fp = state.read_u32(data, 16);
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
            // Parse ramp size from request
            let size = if data.len() >= 8 {
                state.read_u16(data, 4) as usize
            } else {
                256
            };
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
            // Parse ramp size and ramp data from request
            require_len!(data, 8, seq, 153, minor as u16, state.msb_first);
            let size = state.read_u16(data, 4) as usize;
            let ramp_bytes = size * 2;
            let padded = (ramp_bytes + 3) & !3;
            let ramp_start = 8; // ramp data starts after 8-byte header
            let needed = ramp_start + padded * 3;
            if data.len() < needed || size == 0 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::LENGTH_ERROR,
                    seq,
                    0,
                    153,
                    minor as u16,
                    state.msb_first,
                );
            }
            let parse_channel = |offset: usize| -> Vec<u16> {
                (0..size)
                    .map(|i| {
                        let pos = offset + i * 2;
                        state.read_u16(data, pos)
                    })
                    .collect()
            };
            let red = parse_channel(ramp_start);
            let green = parse_channel(ramp_start + padded);
            let blue = parse_channel(ramp_start + padded * 2);
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = red;
                crtc.gamma_green = green;
                crtc.gamma_blue = blue;
            }
            Vec::new()
        }
        18 => {
            // GetGammaRampSize
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
            // XF86VidModeSwitchToMode: screen(2) + pad(2) + ModeInfo starting at offset 8.
            require_len!(data, 52, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let requested = VidModeInfo::parse(state, data, 8);
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
            // XF86VidModeLockModeSwitch: screen(2) + lock(2)
            require_len!(data, 8, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let lock = state.read_u16(data, 6);
            state.vidmode_locked = lock != 0;
            debug!(
                "VidMode LockModeSwitch: screen={screen} locked={}",
                state.vidmode_locked
            );
            Vec::new()
        }
        7 => {
            // AddModeLine — parse and add to mode list
            // XF86VidModeAddModeLine: screen(2) + pad(2) + ModeInfo starting at offset 8.
            // The "after" mode info follows at offset 8 + 32 = 40, but we only need the new mode.
            require_len!(data, 54, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let new_mode = VidModeInfo::parse(state, data, 8);
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
            // XF86VidModeDeleteModeLine: screen(2) + pad(2) + ModeInfo starting at offset 8.
            require_len!(data, 50, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let target = VidModeInfo::parse(state, data, 8);
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
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, 0) // status = MODE_OK
                .build()
        }
        10 => {
            // SwitchMode — cycle through mode list by zoom direction
            // XF86VidModeSwitchMode: screen(2) + zoom(2)
            require_len!(data, 8, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let zoom = state.read_i16(data, 6);
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
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u32(8, state.vidmode_viewport_x)
                .set_u32(12, state.vidmode_viewport_y)
                .build()
        }
        12 => {
            // SetViewPort — store offset (clamped to screen bounds) and log
            // XF86VidModeSetViewPort: screen(2) + pad(2) + x(4) + y(4)
            require_len!(data, 16, seq, 153, minor as u16, state.msb_first);
            let screen = state.read_u16(data, 4);
            let x = state.read_u32(data, 8);
            let y = state.read_u32(data, 12);
            state.vidmode_viewport_x = x;
            state.vidmode_viewport_y = y;
            debug!("VidMode SetViewPort: screen={screen} x={x} y={y} (stored; virtual display always at 0,0)");
            Vec::new()
        }
        13 => {
            // GetDotClocks
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
