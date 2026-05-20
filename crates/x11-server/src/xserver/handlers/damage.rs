//! DAMAGE extension handler.

use super::parse_minor;
use tracing::{debug, info};

use super::super::client::ClientState;
use super::super::types::DamageInfo;
use crate::xserver::reply::serialize_reply;
use x11rb_protocol::protocol::damage::{
    AddRequest as DamageAddRequest, CreateRequest as DamageCreateRequest,
    DestroyRequest as DamageDestroyRequest, QueryVersionReply as DamageQueryVersionReply,
    SubtractRequest as DamageSubtractRequest, ADD_REQUEST as DAMAGE_ADD_REQUEST,
    CREATE_REQUEST as DAMAGE_CREATE_REQUEST, DESTROY_REQUEST as DAMAGE_DESTROY_REQUEST,
    QUERY_VERSION_REQUEST as DAMAGE_QUERY_VERSION_REQUEST,
    SUBTRACT_REQUEST as DAMAGE_SUBTRACT_REQUEST,
};

pub(crate) fn handle_damage_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let minor = data[1];
    debug!("DAMAGE minor opcode: {minor}");

    match minor {
        DAMAGE_QUERY_VERSION_REQUEST => serialize_reply(
            &DamageQueryVersionReply {
                sequence: seq,
                length: 0,
                major_version: 1,
                minor_version: 1,
            },
            state.byte_order(),
        ),
        DAMAGE_CREATE_REQUEST => {
            let req = parse_minor!(DamageCreateRequest, data, state, seq, 143, minor as u16);
            let damage_id = req.damage;
            let drawable = req.drawable;
            let level = u8::from(req.level);
            info!("DAMAGE Create: id={damage_id:#x} drawable={drawable:#x} level={level}");
            state.damage_regions.insert(
                damage_id,
                DamageInfo {
                    drawable,
                    level,
                    accumulated: super::super::types::XFixesRegion::new(),
                },
            );
            Vec::new()
        }
        DAMAGE_DESTROY_REQUEST => {
            let req = parse_minor!(DamageDestroyRequest, data, state, seq, 143, minor as u16);
            let damage_id = req.damage;
            debug!("DAMAGE Destroy: id={damage_id:#x}");
            state.damage_regions.remove(&damage_id);
            state.recycle_xid(damage_id);
            Vec::new()
        }
        DAMAGE_SUBTRACT_REQUEST => {
            let req = parse_minor!(DamageSubtractRequest, data, state, seq, 143, minor as u16);
            let damage_id = req.damage;
            let repair = req.repair;
            let parts = req.parts;
            debug!("DAMAGE Subtract: id={damage_id:#x} repair={repair:#x} parts={parts:#x}");

            // Get the accumulated damage for this damage object.
            let accumulated = state
                .damage_regions
                .get(&damage_id)
                .map(|d| d.accumulated.clone())
                .unwrap_or_else(super::super::types::XFixesRegion::new);

            let remainder = if repair == 0 {
                // repair=None: subtract everything (acknowledge all damage).
                super::super::types::XFixesRegion::new()
            } else if let Some(repair_region) = state.xfixes_regions.get(&repair) {
                // Subtract the repair region from accumulated damage.
                accumulated.subtract(repair_region)
            } else {
                // Repair region doesn't exist — treat as empty (acknowledge nothing).
                accumulated.clone()
            };

            // Store the remainder in the parts region (if not None).
            if parts != 0 {
                state.xfixes_regions.insert(parts, remainder.clone());
            }

            // Update the accumulated damage to the remainder.
            if let Some(dmg) = state.damage_regions.get_mut(&damage_id) {
                dmg.accumulated = remainder;
            }

            Vec::new()
        }
        DAMAGE_ADD_REQUEST => {
            // Manually add damage to a drawable.
            let req = parse_minor!(DamageAddRequest, data, state, seq, 143, minor as u16);
            let drawable = req.drawable;
            let region = req.region;
            debug!("DAMAGE Add: drawable={drawable:#x} region={region:#x}");
            // Get region extents and notify damage
            if let Some(reg) = state.xfixes_regions.get(&region) {
                let ext = reg.extents();
                state.notify_damage(drawable, ext.x, ext.y, ext.width, ext.height);
            }
            Vec::new()
        }
        _ => {
            debug!("Unhandled DAMAGE minor opcode: {minor}");
            crate::xserver::core::build_error(
                crate::xserver::core::REQUEST_ERROR,
                seq,
                minor as u32,
                143,
                minor as u16,
            )
        }
    }
}
