//! DRI3 device/modifier operations: GetSupportedModifiers, SetDRMDeviceInUse.

use tracing::debug;

use super::super::super::client::ClientState;
use super::super::super::core::*;
use super::DRI3_MAJOR_OPCODE;
use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;
use crate::xserver::request::request_header;

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
    require_len!(data, 12, seq, DRI3_MAJOR_OPCODE, minor as u16, bo);
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
    ReplyBuf::with_extra(seq, extra_bytes, bo)
        .set_u32(8, num_window_modifiers)
        .set_u32(12, num_screen_modifiers)
        // Window modifiers (u64 each), starting at offset 32
        .set_u64(32, DRM_FORMAT_MOD_LINEAR)
        // Screen modifiers (u64 each)
        .set_u64(40, DRM_FORMAT_MOD_LINEAR)
        .set_u64(48, DRM_FORMAT_MOD_INVALID) // fallback/any-modifier sentinel
        .build()
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
    require_len!(data, 16, seq, DRI3_MAJOR_OPCODE, minor as u16, bo);

    use x11rb_protocol::protocol::dri3::SetDRMDeviceInUseRequest;
    let req = match SetDRMDeviceInUseRequest::try_parse_request(request_header(data), &data[4..]) {
        Ok(r) => r,
        Err(_) => return build_error_bo(LENGTH_ERROR, seq, 0, DRI3_MAJOR_OPCODE, minor as u16, bo),
    };
    let _window = req.window;
    let drm_major = req.drm_major;
    let drm_minor = req.drm_minor;

    debug!("DRI3 SetDRMDeviceInUse: window={_window:#x} drm_device={drm_major}:{drm_minor}");

    // Track the DRM device this client is using.
    state.dri3_drm_device = Some((drm_major, drm_minor));

    Vec::new()
}
