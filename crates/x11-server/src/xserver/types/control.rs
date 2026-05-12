//! Access control, security tokens, and RECORD shared state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Shared access control state (server-wide, used for host-based access control).
pub(crate) type SharedAccessControl = Arc<Mutex<AccessControlState>>;

/// Server-wide access control settings.
pub(crate) struct AccessControlState {
    /// Whether access control is enabled.
    pub(crate) enabled: bool,
    /// List of allowed hosts.
    pub(crate) hosts: Vec<super::super::client::types::AccessHost>,
}

impl AccessControlState {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            hosts: Vec::new(),
        }
    }

    /// Check if a TCP peer address is allowed to connect.
    /// Returns true if the connection should be accepted.
    pub(crate) fn check_tcp_address(&self, addr: &std::net::SocketAddr) -> bool {
        if !self.enabled {
            return true; // Access control disabled — accept all
        }

        let ip = addr.ip();
        // Check if the peer's IP matches any entry in the hosts list
        for host in &self.hosts {
            match host.family {
                0 => {
                    // Internet (IPv4)
                    if host.address.len() == 4 {
                        if let std::net::IpAddr::V4(v4) = ip {
                            if v4.octets()
                                == <[u8; 4]>::try_from(host.address.as_slice()).unwrap_or([0; 4])
                            {
                                return true;
                            }
                        }
                    }
                }
                6 => {
                    // InternetV6
                    if host.address.len() == 16 {
                        if let std::net::IpAddr::V6(v6) = ip {
                            if v6.octets()
                                == <[u8; 16]>::try_from(host.address.as_slice()).unwrap_or([0; 16])
                            {
                                return true;
                            }
                        }
                    }
                }
                5 => {
                    // ServerInterpreted — check for "localuser" or "localgroup" patterns
                    // Accept ServerInterpreted entries as wildcards for now
                    return true;
                }
                254 => {
                    // Local — always matches local connections
                    return true;
                }
                _ => {} // DECnet, Chaos, etc. — not matched
            }
        }

        // Check for localhost
        match ip {
            std::net::IpAddr::V4(v4) if v4.is_loopback() => true,
            std::net::IpAddr::V6(v6) if v6.is_loopback() => true,
            _ => false, // Not in hosts list and not localhost
        }
    }
}

/// Shared SECURITY authorizations for cross-connection token validation.
/// Key: auth token (16 bytes), Value: (auth_id, trust_level, timeout, group, created_at).
pub(crate) type SharedSecurityTokens = Arc<Mutex<HashMap<[u8; 16], SecurityTokenInfo>>>;

/// Info for a SECURITY-generated authorization token.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct SecurityTokenInfo {
    pub(crate) auth_id: u32,
    pub(crate) trust_level: u32, // 0 = trusted, 1 = untrusted
    pub(crate) timeout: u32,     // seconds until expiry (0 = no timeout)
    pub(crate) group: u32,
    pub(crate) created_at: std::time::Instant,
}

impl SecurityTokenInfo {
    /// Check if this token has expired.
    pub(crate) fn is_expired(&self) -> bool {
        self.timeout > 0 && self.created_at.elapsed().as_secs() >= self.timeout as u64
    }
}

/// Shared RECORD contexts for cross-connection protocol interception.
/// Key: context_id, Value: SharedRecordEntry with context state and event channel.
pub(crate) type SharedRecordContexts = Arc<Mutex<HashMap<u32, SharedRecordEntry>>>;

/// Entry in the shared RECORD registry.
#[allow(dead_code)]
pub(crate) struct SharedRecordEntry {
    pub(crate) recording_client_id: String,
    pub(crate) recording_resource_base: u32,
    pub(crate) context: super::super::handlers::record::RecordContext,
    pub(crate) event_tx: mpsc::UnboundedSender<Vec<u8>>,
}
