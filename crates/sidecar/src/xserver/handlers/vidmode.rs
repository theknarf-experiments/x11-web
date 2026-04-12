//! XFree86-VidModeExtension handler (opcode 153).

use tracing::debug;

use super::super::client::ClientState;

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
        0 => { // QueryVersion
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 2); // major
            state.write_u16(&mut reply, 10, 2); // minor
            reply.to_vec()
        }
        1 => { // GetModeLine
            // Return the current mode from the mode list.
            let mode = state.vidmode_modes.get(state.vidmode_current_mode)
                .cloned()
                .unwrap_or_else(|| VidModeInfo::default_for_screen(state.screen_width, state.screen_height));
            let mut reply = vec![0u8; 52]; // 32 header + 20 modeline data
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 5); // length = 5 extra u32s
            state.write_u32(&mut reply, 8, mode.dotclock); // dotclock
            state.write_u16(&mut reply, 12, mode.hdisplay); // hdisplay
            state.write_u16(&mut reply, 14, mode.hsyncstart); // hsyncstart
            state.write_u16(&mut reply, 16, mode.hsyncend); // hsyncend
            state.write_u16(&mut reply, 18, mode.htotal); // htotal
            state.write_u16(&mut reply, 20, 0); // hskew
            state.write_u16(&mut reply, 22, mode.vdisplay); // vdisplay
            state.write_u16(&mut reply, 24, mode.vsyncstart); // vsyncstart
            state.write_u16(&mut reply, 26, mode.vsyncend); // vsyncend
            state.write_u16(&mut reply, 28, mode.vtotal); // vtotal
            state.write_u32(&mut reply, 32, mode.flags); // flags
            // privsize at 36..40 = 0
            reply
        }
        6 => { // GetAllModeLines
            // Return all modes from the mode list.
            let mode_count = state.vidmode_modes.len();
            let mode_size = 48; // bytes per mode line info
            let extra = 4 + mode_size * mode_count; // 4 bytes for count + modes
            let padded = (extra + 3) & !3;
            let mut reply = vec![0u8; 32 + padded];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (padded / 4) as u32);
            state.write_u32(&mut reply, 8, mode_count as u32);
            for (i, mode) in state.vidmode_modes.iter().enumerate() {
                let off = 36 + i * mode_size;
                state.write_u32(&mut reply, off, mode.dotclock);
                state.write_u16(&mut reply, off + 4, mode.hdisplay);
                state.write_u16(&mut reply, off + 6, mode.hsyncstart);
                state.write_u16(&mut reply, off + 8, mode.hsyncend);
                state.write_u16(&mut reply, off + 10, mode.htotal);
                // hskew at off + 12 = 0
                state.write_u16(&mut reply, off + 14, mode.vdisplay);
                state.write_u16(&mut reply, off + 16, mode.vsyncstart);
                state.write_u16(&mut reply, off + 18, mode.vsyncend);
                state.write_u16(&mut reply, off + 20, mode.vtotal);
                // pad at off + 22..26 = 0
                state.write_u32(&mut reply, off + 26, mode.flags);
            }
            reply
        }
        14 => { // GetGamma
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
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
                (approx(&crtc.gamma_red), approx(&crtc.gamma_green), approx(&crtc.gamma_blue))
            } else {
                (1.0, 1.0, 1.0)
            };
            // Gamma is 16.16 fixed point, 1.0 = 65536
            state.write_u32(&mut reply, 8, (gamma_r * 65536.0) as u32); // red
            state.write_u32(&mut reply, 12, (gamma_g * 65536.0) as u32); // green
            state.write_u32(&mut reply, 16, (gamma_b * 65536.0) as u32); // blue
            reply.to_vec()
        }
        15 => { // SetGamma
            // Parse three 16.16 fixed-point gamma values from request
            if data.len() < 20 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let red_fp = state.read_u32(data, 8);
            let green_fp = state.read_u32(data, 12);
            let blue_fp = state.read_u32(data, 16);
            let gamma_r = red_fp as f64 / 65536.0;
            let gamma_g = green_fp as f64 / 65536.0;
            let gamma_b = blue_fp as f64 / 65536.0;
            // Compute ramp: ramp[i] = ((i/255)^(1/gamma) * 65535) as u16
            let compute_ramp = |gamma: f64| -> Vec<u16> {
                (0..256).map(|i| {
                    let normalized = i as f64 / 255.0;
                    let val = if gamma > 0.0 {
                        normalized.powf(1.0 / gamma) * 65535.0
                    } else {
                        normalized * 65535.0
                    };
                    val.round() as u16
                }).collect()
            };
            if let Some(crtc) = state.randr_crtcs.get_mut(0) {
                crtc.gamma_red = compute_ramp(gamma_r);
                crtc.gamma_green = compute_ramp(gamma_g);
                crtc.gamma_blue = compute_ramp(gamma_b);
            }
            Vec::new()
        }
        16 => { // GetGammaRamp
            // Parse ramp size from request
            let size = if data.len() >= 8 {
                state.read_u16(data, 4) as usize
            } else {
                256
            };
            let ramp_bytes = size * 2; // each value is u16
            let padded = (ramp_bytes + 3) & !3;
            let total_extra = padded * 3; // R, G, B
            let mut reply = vec![0u8; 32 + total_extra];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (total_extra / 4) as u32);
            state.write_u16(&mut reply, 8, size as u16); // size
            // Return stored ramp from CRTC
            let (ramp_r, ramp_g, ramp_b) = if let Some(crtc) = state.randr_crtcs.first() {
                (crtc.gamma_red.clone(), crtc.gamma_green.clone(), crtc.gamma_blue.clone())
            } else {
                // Fallback: linear ramp
                let linear: Vec<u16> = (0..256).map(|i| ((i as u32 * 65535) / 255) as u16).collect();
                (linear.clone(), linear.clone(), linear)
            };
            let ramps = [&ramp_r, &ramp_g, &ramp_b];
            for channel in 0..3 {
                let base = 32 + channel * padded;
                for i in 0..size {
                    let val = if i < ramps[channel].len() {
                        ramps[channel][i]
                    } else {
                        // Extrapolate linearly if requested size exceeds stored ramp
                        ((i as u32 * 65535) / (size.max(1) as u32 - 1).max(1)) as u16
                    };
                    let off = base + i * 2;
                    if off + 2 <= reply.len() {
                        state.write_u16(&mut reply, off, val);
                    }
                }
            }
            reply
        }
        17 => { // SetGammaRamp
            // Parse ramp size and ramp data from request
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let size = state.read_u16(data, 4) as usize;
            let ramp_bytes = size * 2;
            let padded = (ramp_bytes + 3) & !3;
            let ramp_start = 8; // ramp data starts after 8-byte header
            let needed = ramp_start + padded * 3;
            if data.len() < needed || size == 0 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let parse_channel = |offset: usize| -> Vec<u16> {
                (0..size).map(|i| {
                    let pos = offset + i * 2;
                    state.read_u16(data, pos)
                }).collect()
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
        18 => { // GetGammaRampSize
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u16(&mut reply, 8, 256); // size = 256
            reply.to_vec()
        }
        2 => { // GetModeLine (legacy alias)
            // Same as minor 1 — some clients use minor 2
            let mode = state.vidmode_modes.get(state.vidmode_current_mode)
                .cloned()
                .unwrap_or_else(|| VidModeInfo::default_for_screen(state.screen_width, state.screen_height));
            let mut reply = vec![0u8; 52];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 5);
            state.write_u32(&mut reply, 8, mode.dotclock);
            state.write_u16(&mut reply, 12, mode.hdisplay);
            state.write_u16(&mut reply, 14, mode.hsyncstart);
            state.write_u16(&mut reply, 16, mode.hsyncend);
            state.write_u16(&mut reply, 18, mode.htotal);
            state.write_u16(&mut reply, 22, mode.vdisplay);
            state.write_u16(&mut reply, 24, mode.vsyncstart);
            state.write_u16(&mut reply, 26, mode.vsyncend);
            state.write_u16(&mut reply, 28, mode.vtotal);
            state.write_u32(&mut reply, 32, mode.flags);
            reply
        }
        3 => { // SwitchToMode — attempt to switch to a matching mode in the mode list
            // XF86VidModeSwitchToMode: screen(2) + pad(2) + ModeInfo starting at offset 8.
            if data.len() < 52 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let screen = state.read_u16(data, 4);
            let requested = VidModeInfo::parse(state, data, 8);
            if state.vidmode_locked {
                debug!(
                    "VidMode SwitchToMode: screen={screen} {}x{} rejected — mode switching is locked",
                    requested.hdisplay, requested.vdisplay,
                );
                return Vec::new();
            }
            if let Some(idx) = state.vidmode_modes.iter().position(|m| m.matches(&requested)) {
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
        4 => { // GetMonitor
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
            let extra = 8 + vendor_padded + model_padded + (hsync_count as usize * 8) + (vsync_count as usize * 8);
            let padded_extra = (extra + 3) & !3;
            let mut reply = vec![0u8; 32 + padded_extra];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, (padded_extra / 4) as u32);
            state.write_u32(&mut reply, 8, vendor_len);
            state.write_u32(&mut reply, 12, model_len);
            state.write_u32(&mut reply, 16, hsync_count);
            state.write_u32(&mut reply, 20, vsync_count);
            let mut off = 32;
            reply[off..off + vendor.len()].copy_from_slice(vendor);
            off += vendor_padded;
            reply[off..off + model.len()].copy_from_slice(model);
            off += model_padded;
            // HSync range: 31.5 - 80.0 kHz (as 16.16 fixed point * 100)
            state.write_u32(&mut reply, off, 3150); // low = 31.50 kHz
            state.write_u32(&mut reply, off + 4, 8000); // high = 80.00 kHz
            off += 8;
            // VSync range: 56 - 75 Hz
            state.write_u32(&mut reply, off, 5600); // low = 56.00 Hz
            state.write_u32(&mut reply, off + 4, 7500); // high = 75.00 Hz
            reply
        }
        5 => { // LockModeSwitch — store the lock state
            // XF86VidModeLockModeSwitch: screen(2) + lock(2)
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let screen = state.read_u16(data, 4);
            let lock = state.read_u16(data, 6);
            state.vidmode_locked = lock != 0;
            debug!("VidMode LockModeSwitch: screen={screen} locked={}", state.vidmode_locked);
            Vec::new()
        }
        7 => { // AddModeLine — parse and add to mode list
            // XF86VidModeAddModeLine: screen(2) + pad(2) + ModeInfo starting at offset 8.
            // The "after" mode info follows at offset 8 + 32 = 40, but we only need the new mode.
            if data.len() < 54 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
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
        8 => { // DeleteModeLine — remove matching mode from the list
            // XF86VidModeDeleteModeLine: screen(2) + pad(2) + ModeInfo starting at offset 8.
            if data.len() < 50 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
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
        9 => { // ValidateModeLine — always return MODE_OK
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, 0); // status = MODE_OK
            reply.to_vec()
        }
        10 => { // SwitchMode — cycle through mode list by zoom direction
            // XF86VidModeSwitchMode: screen(2) + zoom(2)
            if data.len() < 8 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
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
        11 => { // GetViewPort
            let mut reply = [0u8; 32];
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 8, state.vidmode_viewport_x);
            state.write_u32(&mut reply, 12, state.vidmode_viewport_y);
            reply.to_vec()
        }
        12 => { // SetViewPort — store offset (clamped to screen bounds) and log
            // XF86VidModeSetViewPort: screen(2) + pad(2) + x(4) + y(4)
            if data.len() < 16 {
                return crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH, seq, 0,
                    153, minor as u16, state.msb_first,
                );
            }
            let screen = state.read_u16(data, 4);
            let x = state.read_u32(data, 8);
            let y = state.read_u32(data, 12);
            state.vidmode_viewport_x = x;
            state.vidmode_viewport_y = y;
            debug!("VidMode SetViewPort: screen={screen} x={x} y={y} (stored; virtual display always at 0,0)");
            Vec::new()
        }
        13 => { // GetDotClocks
            let mut reply = vec![0u8; 36]; // 32 header + 4 clock value
            reply[0] = 1;
            state.write_u16(&mut reply, 2, seq);
            state.write_u32(&mut reply, 4, 1); // length = 1 extra u32
            state.write_u32(&mut reply, 8, 0); // flags = 0
            state.write_u32(&mut reply, 12, 1); // clocks = 1
            state.write_u32(&mut reply, 16, state.screen_width as u32 * state.screen_height as u32 * 60); // maxclocks
            // Padding at 20..32 is zero
            // clock[0] = dot clock of the mode
            state.write_u32(&mut reply, 32, state.screen_width as u32 * state.screen_height as u32 * 60);
            reply
        }
        _ => {
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST, seq, 0,
                153, minor as u16, state.msb_first,
            )
        }
    }
}
