//! XKB device and geometry: GetDeviceInfo, SetDeviceInfo, ListComponents.

use super::super::super::client::ClientState;
use crate::xserver::reply::serialize_var_reply;
use tracing::debug;
use x11rb_protocol::protocol::xkb::{
    DeviceLedInfo, GetDeviceInfoReply, ListComponentsReply, Listing, XIFeature,
};

/// Wire size of one XkbAction record in `SetDeviceInfo` button-action arrays.
const XKB_ACTION_SIZE: usize = 8;

fn listings_for(names: &[&str]) -> Vec<Listing> {
    names
        .iter()
        .map(|n| Listing {
            flags: 0,
            string: n.as_bytes().to_vec(),
        })
        .collect()
}

/// Handle ListComponents (minor opcode 22).
pub(crate) fn handle_list_components(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    // Standard XKB component names — what xkbcomp and libxkbcommon expect.
    let keymaps: &[&str] = &[]; // keymaps are built from the other 5 lists
    let keycodes: &[&str] = &["evdev", "evdev+aliases(qwerty)"];
    let types: &[&str] = &["complete", "default"];
    let compat: &[&str] = &["complete", "default"];
    let symbols: &[&str] = &["pc+us+inet(evdev)", "pc+us"];
    let geometry: &[&str] = &["pc(pc105)", "pc(pc104)"];

    debug!(
        "ListComponents: returning {} keycodes, {} types, {} compat, {} symbols, {} geometry",
        keycodes.len(),
        types.len(),
        compat.len(),
        symbols.len(),
        geometry.len()
    );

    serialize_var_reply(
        &ListComponentsReply {
            device_id: device_id_byte,
            sequence: seq,
            length: 0,
            extra: 0,
            keymaps: listings_for(keymaps),
            keycodes: listings_for(keycodes),
            types: listings_for(types),
            compat_maps: listings_for(compat),
            symbols: listings_for(symbols),
            geometries: listings_for(geometry),
        },
        state.byte_order(),
    )
}

/// Handle GetDeviceInfo (minor opcode 24).
pub(crate) fn handle_get_device_info(
    state: &mut ClientState,
    seq: u16,
    device_id_byte: u8,
) -> Vec<u8> {
    let device_name = b"Virtual core keyboard".to_vec();
    debug!(
        "GetDeviceInfo: returning '{}'",
        std::str::from_utf8(&device_name).unwrap_or("?")
    );

    serialize_var_reply(
        &GetDeviceInfoReply {
            device_id: device_id_byte,
            sequence: seq,
            length: 0,
            present: XIFeature::from(0u16),
            supported: XIFeature::from(0u16),
            unsupported: XIFeature::from(0u16),
            first_btn_wanted: 0,
            n_btns_wanted: 0,
            first_btn_rtrn: 0,
            total_btns: 0,
            has_own_state: true,
            dflt_kbd_fb: 0,
            dflt_led_fb: 0,
            dev_type: 0,
            name: device_name,
            btn_actions: Vec::new(),
            leds: Vec::<DeviceLedInfo>::new(),
        },
        state.byte_order(),
    )
}

/// Handle SetDeviceInfo (minor opcode 25).
pub(crate) fn handle_set_device_info(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() >= 12 {
        let _device_spec = state.read_u16(data, 4);
        let first_btn = data[6];
        let n_btns = data[7];
        let change = state.read_u16(data, 8);
        let _n_led_fbs = state.read_u16(data, 10);

        let mut offset = 12;

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

        if change & 2 != 0 && offset + 4 <= data.len() {
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
