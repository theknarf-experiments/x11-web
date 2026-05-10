//! XKB device and geometry: GetDeviceInfo, SetDeviceInfo, ListComponents.

use super::super::super::client::ClientState;
use crate::xserver::reply::ReplyBuf;
use tracing::debug;

/// GetDeviceInfo reply layout (after the 32-byte reply header).
mod get_device_info_layout {
    /// hasOwnState (u8) — 1 if the device has its own state.
    pub(super) const HAS_OWN_STATE: usize = 21;
    /// nameLen (u16) — length of the device-name string.
    pub(super) const NAME_LEN: usize = 30;
    /// First byte of the name string (32-byte reply header + 24-byte fixed body).
    pub(super) const NAME_DATA: usize = 56;
}

/// Wire size of one XkbAction record in `SetDeviceInfo` button-action arrays.
const XKB_ACTION_SIZE: usize = 8;

/// Handle ListComponents (minor opcode 22).
pub(crate) fn handle_list_components(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    // ListComponents: return real XKB component database names.
    //
    // Reply body consists of 6 counted lists (16-bit count + entries).
    // Each entry is: 2-byte flags + 1-byte name_length + name bytes.
    // Lists: keymaps, keycodes, types, compat, symbols, geometry.
    //
    // We report the standard XKB components that xkbcomp and
    // libxkbcommon expect from a well-configured X server.

    // Standard XKB component names
    let keymaps: &[&str] = &[]; // keymaps are built from the other 5 lists
    let keycodes: &[&str] = &["evdev", "evdev+aliases(qwerty)"];
    let types: &[&str] = &["complete", "default"];
    let compat: &[&str] = &["complete", "default"];
    let symbols: &[&str] = &["pc+us+inet(evdev)", "pc+us"];
    let geometry: &[&str] = &["pc(pc105)", "pc(pc104)"];

    let lists: [&[&str]; 6] = [keymaps, keycodes, types, compat, symbols, geometry];

    // Compute body size: for each list, 2 bytes count + entries.
    // Each entry: 2 bytes flags + 1 byte name_len + name bytes.
    let mut body_size = 0usize;
    for list in &lists {
        body_size += 2; // count
        for name in *list {
            body_size += 2 + 1 + name.len(); // flags + name_len + name
        }
    }
    // Pad to 4-byte boundary
    let padded = crate::xserver::core::align_to_4(body_size);

    let mut reply =
        ReplyBuf::with_extra(seq, padded, state.msb_first).set_data_byte(device_id_byte);

    // Encode header fields: nKeymaps..nGeometries at bytes 8-19
    for (i, list) in lists.iter().enumerate() {
        reply = reply.set_u16(8 + i * 2, list.len() as u16);
    }

    // Encode list entries starting at byte 32
    let mut off = 32;
    for list in &lists {
        for name in *list {
            // flags = 0 (no LC_* flags — plain component listing)
            reply = reply.set_u16(off, 0);
            off += 2;
            // name length (1 byte)
            reply.buf_mut()[off] = name.len() as u8;
            off += 1;
            // name bytes
            reply.buf_mut()[off..off + name.len()].copy_from_slice(name.as_bytes());
            off += name.len();
        }
    }

    debug!(
        "ListComponents: returning {} keycodes, {} types, {} compat, {} symbols, {} geometry",
        keycodes.len(),
        types.len(),
        compat.len(),
        symbols.len(),
        geometry.len()
    );
    reply.build()
}

/// Handle GetDeviceInfo (minor opcode 24).
pub(crate) fn handle_get_device_info(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    // GetDeviceInfo: return minimal device info with name.
    // Wire request: 4-5=device_spec, 6-7=wanted, 8=all_buttons,
    //               9=first_button, 10=num_buttons, 11-12=led_class, 13-14=led_id
    let device_name = b"Virtual core keyboard";
    let name_len = device_name.len();
    let name_pad = (4 - (name_len % 4)) % 4;
    let body_len = 24 + name_len + name_pad; // fixed fields + name
    let mut reply =
        ReplyBuf::with_extra(seq, body_len, state.msb_first).set_data_byte(device_id_byte);
    // Byte 8-9: present (what we return): 0 = nothing extra
    reply = reply.set_u16(8, 0);
    // Byte 10: supported (bitmask of supported features)
    reply = reply.set_u16(10, 0);
    // Byte 12-13: unsupported (0)
    // Byte 14-15: nDeviceLedFBs (0)
    // Byte 16: firstBtnWanted (0)
    // Byte 17: nBtnsWanted (0)
    // Byte 18: firstBtnRtrn (0)
    // Byte 19: nBtnsRtrn (0)
    // Byte 20: totalBtns (0)
    // hasOwnState = 1
    reply.buf_mut()[get_device_info_layout::HAS_OWN_STATE] = 1;
    // Byte 22-23: dfltKbdFB (0)
    // Byte 24-25: dfltLedFB (0)
    // Byte 26-27: nDeviceLedFBs (already 0)
    // Byte 28-29: devType (0)
    reply = reply.set_u16(get_device_info_layout::NAME_LEN, name_len as u16);
    // Name follows the 24-byte fixed body at offset 56.
    reply.buf_mut()
        [get_device_info_layout::NAME_DATA..get_device_info_layout::NAME_DATA + name_len]
        .copy_from_slice(device_name);
    debug!(
        "GetDeviceInfo: returning '{}'",
        std::str::from_utf8(device_name).unwrap_or("?")
    );
    reply.build()
}

/// Handle SetDeviceInfo (minor opcode 25).
pub(crate) fn handle_set_device_info(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    // SetDeviceInfo: parse and store device button actions and LED info.
    // Wire format:
    //   4-5: deviceSpec
    //   6: firstBtn
    //   7: nBtns
    //   8-9: change (bitmask: 1=ButtonActions, 2=Leds)
    //   10-11: nDeviceLedFBs
    // Followed by: button actions (nBtns * 8 bytes) if change & 1,
    //              then LED feedback info if change & 2.
    if data.len() >= 12 {
        let _device_spec = state.read_u16(data, 4);
        let first_btn = data[6];
        let n_btns = data[7];
        let change = state.read_u16(data, 8);
        let _n_led_fbs = state.read_u16(data, 10);

        let mut offset = 12;

        // Parse button actions (each action is 8 bytes)
        if change & 1 != 0 {
            for i in 0..n_btns {
                if offset + XKB_ACTION_SIZE > data.len() {
                    break;
                }
                let btn_idx = first_btn + i;
                let action: [u8; XKB_ACTION_SIZE] = data[offset..offset + XKB_ACTION_SIZE]
                    .try_into()
                    .expect("checked length above");
                let action_type = action[0];
                state.xkb_button_actions.insert(btn_idx, action);
                debug!(
                    "SetDeviceInfo: button {} action type {}",
                    btn_idx, action_type
                );
                offset += XKB_ACTION_SIZE;
            }
        }

        // Parse LED feedback info if present
        if change & 2 != 0 && offset + 4 <= data.len() {
            // LED feedback entries: led_class(2), led_id(2), then
            // names_present(4), maps_present(4), ...
            // Store as opaque blobs for GetDeviceInfo to echo back.
            let remaining = &data[offset..];
            state.xkb_device_led_info = remaining.to_vec();
            debug!(
                "SetDeviceInfo: stored {} bytes of LED info",
                remaining.len()
            );
        }
    }
    Vec::new()
}
