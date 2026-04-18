//! SECURITY extension handler (opcode 155).

use tracing::debug;

use super::super::client::ClientState;

/// SECURITY (opcode 155)
pub(crate) fn handle_security_request(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    use super::super::client::SecurityAuthorization;
    use crate::xserver::core::require_len;
use crate::xserver::reply::ReplyBuf;

    let minor = data[1];
    match minor {
        0 => {
            // QueryVersion
            ReplyBuf::fixed(seq, state.msb_first)
                .set_u16(8, 1) // major
                .set_u16(10, 0) // minor
                .build()
        }
        1 => {
            // GenerateAuthorization
            if data.len() >= 16 {
                let auth_proto_name_len = state.read_u16(data, 4) as usize;
                let auth_proto_data_len = state.read_u16(data, 6) as usize;
                let value_mask = state.read_u32(data, 8);

                // Parse optional values after the auth proto name + data
                let name_padded = (auth_proto_name_len + 3) & !3;
                let data_padded = (auth_proto_data_len + 3) & !3;
                let values_off = 12 + name_padded + data_padded;

                let mut trust_level: u32 = 0; // trusted by default
                let mut timeout: u32 = 0;
                let mut group: u32 = 0;
                let mut event_mask: u32 = 0;

                let mut voff = values_off;
                if value_mask & 1 != 0 && voff + 4 <= data.len() {
                    timeout = state.read_u32(data, voff);
                    voff += 4;
                }
                if value_mask & 2 != 0 && voff + 4 <= data.len() {
                    trust_level = state.read_u32(data, voff);
                    voff += 4;
                }
                if value_mask & 4 != 0 && voff + 4 <= data.len() {
                    group = state.read_u32(data, voff);
                    voff += 4;
                }
                if value_mask & 8 != 0 && voff + 4 <= data.len() {
                    event_mask = state.read_u32(data, voff);
                }

                // Generate a unique auth ID using UUID to avoid collisions
                let auth_id = uuid::Uuid::new_v4().as_u128() as u32;

                state.security_authorizations.insert(
                    auth_id,
                    SecurityAuthorization {
                        auth_id,
                        trust_level,
                        timeout,
                        group,
                        event_mask,
                    },
                );

                debug!("SECURITY GenerateAuthorization: auth_id={auth_id} trust={trust_level}");

                // Generate auth data (MIT-MAGIC-COOKIE-1 style: 16 random bytes)
                let auth_data: Vec<u8> = uuid::Uuid::new_v4().as_bytes().to_vec();

                // Register the token in the shared security map for cross-connection validation
                let mut token_key = [0u8; 16];
                token_key.copy_from_slice(&auth_data[..16]);
                if let Ok(mut tokens) = state.shared_security_tokens.lock() {
                    tokens.insert(
                        token_key,
                        crate::xserver::types::SecurityTokenInfo {
                            auth_id,
                            trust_level,
                            timeout,
                            group,
                            created_at: std::time::Instant::now(),
                        },
                    );
                }

                let auth_data_len = auth_data.len() as u32;
                let extra_words = auth_data_len.div_ceil(4);
                let mut reply = ReplyBuf::with_extra(seq, (extra_words * 4) as usize, state.msb_first)
                    .set_u32(8, auth_id)
                    .set_u16(12, auth_data_len as u16);
                reply.buf_mut()[16..16 + auth_data.len()].copy_from_slice(&auth_data);
                reply.build()
            } else {
                crate::xserver::core::build_error_bo(
                    crate::xserver::core::BAD_LENGTH,
                    seq,
                    0,
                    155,
                    minor as u16,
                    state.msb_first,
                )
            }
        }
        2 => {
            // RevokeAuthorization
            require_len!(data, 8, seq, 155, minor as u16, state.msb_first);
            let auth_id = state.read_u32(data, 4);
            state.security_authorizations.remove(&auth_id);
            state.recycle_xid(auth_id);
            // Remove from shared token map
            if let Ok(mut tokens) = state.shared_security_tokens.lock() {
                tokens.retain(|_, info| info.auth_id != auth_id);
            }
            debug!("SECURITY RevokeAuthorization: auth_id={auth_id}");
            Vec::new()
        }
        _ => {
            debug!("SECURITY: unhandled minor opcode {minor}");
            crate::xserver::core::build_error_bo(
                crate::xserver::core::BAD_REQUEST,
                seq,
                minor as u32,
                155,
                minor as u16,
                state.msb_first,
            )
        }
    }
}
