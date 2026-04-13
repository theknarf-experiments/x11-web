//! DRI3 device/modifier operations: GetSupportedModifiers, SetDRMDeviceInUse.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::DRI3_MAJOR_OPCODE;

// -----------------------------------------------------------------
// 6: GetSupportedModifiers (DRI3 1.2)
// -----------------------------------------------------------------
pub(crate) fn handle_get_supported_modifiers(
    _state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
    bo: bool,
) -> Vec<u8> {
    if data.len() < 12 {
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
    }
    debug!("DRI3 GetSupportedModifiers");

    // Return DRM_FORMAT_MOD_LINEAR (0) and DRM_FORMAT_MOD_INVALID
    // (0x00ffffffffffffff) as supported modifiers.
    // Window modifiers = what this "compositor" supports for scanout.
    // Screen modifiers = what the GPU/renderer supports for rendering.
    const DRM_FORMAT_MOD_LINEAR: u64 = 0;
    const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

    let num_window_modifiers: u32 = 1; // LINEAR only for window/scanout
    let num_screen_modifiers: u32 = 2; // LINEAR + INVALID for rendering

    // Extra data: window modifiers (1 * 8 bytes) + screen modifiers (2 * 8 bytes) = 24 bytes
    // 24 / 4 = 6 words
    let extra_bytes = ((num_window_modifiers + num_screen_modifiers) as usize) * 8;
    let extra_words = extra_bytes / 4;
    let mut reply = vec![0u8; 32 + extra_bytes];
    reply[0] = 1; // Reply
    write_u16_bo(&mut reply, 2, seq, bo);
    write_u32_bo(&mut reply, 4, extra_words as u32, bo); // length
    write_u32_bo(&mut reply, 8, num_window_modifiers, bo);
    write_u32_bo(&mut reply, 12, num_screen_modifiers, bo);

    // Window modifiers (u64 each), starting at offset 32
    let mut off = 32;
    // LINEAR
    write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_LINEAR & 0xFFFF_FFFF) as u32, bo);
    write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_LINEAR >> 32) as u32, bo);
    off += 8;

    // Screen modifiers (u64 each)
    // LINEAR
    write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_LINEAR & 0xFFFF_FFFF) as u32, bo);
    write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_LINEAR >> 32) as u32, bo);
    off += 8;
    // INVALID (used by Mesa as a fallback/any-modifier sentinel)
    write_u32_bo(&mut reply, off, (DRM_FORMAT_MOD_INVALID & 0xFFFF_FFFF) as u32, bo);
    write_u32_bo(&mut reply, off + 4, (DRM_FORMAT_MOD_INVALID >> 32) as u32, bo);

    reply
}

// -----------------------------------------------------------------
// 9: SetDRMDeviceInUse (DRI3 1.4, void request)
// -----------------------------------------------------------------
pub(crate) fn handle_set_drm_device_in_use(
    state: &mut ClientState,
    data: &[u8],
    seq: u16,
    minor: u8,
    bo: bool,
) -> Vec<u8> {
    // Request: window(4), drmMajor(4), drmMinor(4)
    if data.len() < 16 {
        return build_error_bo(BAD_LENGTH, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo);
    }

    let _window = read_u32_bo(data, 4, bo);
    let drm_major = read_u32_bo(data, 8, bo);
    let drm_minor = read_u32_bo(data, 12, bo);

    debug!("DRI3 SetDRMDeviceInUse: window={_window:#x} drm_device={drm_major}:{drm_minor}");

    // Track the DRM device this client is using.
    state.dri3_drm_device = Some((drm_major, drm_minor));

    Vec::new()
}
