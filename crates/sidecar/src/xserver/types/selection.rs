//! Selection and clipboard types for cross-connection clipboard operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;

/// Shared selection ownership across all connections.
/// Maps selection atom → (owner window, event_tx of the owning connection).
pub(crate) type SharedSelections = Arc<Mutex<HashMap<u32, SelectionEntry>>>;

/// Entry for a shared selection.
pub(crate) struct SelectionEntry {
    pub(crate) owner: u32,
    pub(crate) event_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Timestamp when this selection ownership was acquired.
    pub(crate) timestamp: u32,
}

/// Events emitted by the selection subsystem for the clipboard bridge.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ClipboardEvent {
    /// Selection ownership changed.
    OwnerChanged { selection: String, owner: u32 },
    /// Selection data is available (response to a clipboard read request).
    Data { selection: String, mime_type: String, data: Vec<u8> },
}

/// Server-side clipboard data set by the backend (for pasting from browser into X11 apps).
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct ServerClipboardData {
    pub(crate) mime_type: String,
    pub(crate) data: Vec<u8>,
}

/// Shared clipboard state for server-owned selections (browser → X11).
pub(crate) type SharedClipboard = Arc<Mutex<HashMap<String, ServerClipboardData>>>;

/// Proxy window ID used by the server for clipboard operations.
/// This is a well-known window ID that never conflicts with client resource IDs
/// (client IDs start at base = (conn_index+1) << 22, so 0x10 is safe).
#[allow(dead_code)]
pub(crate) const CLIPBOARD_PROXY_WINDOW: u32 = 0x00000010;

/// Window ID used by the server's clipboard manager for persistence.
/// When a CLIPBOARD owner disconnects, the server takes ownership using this
/// window and serves the saved data to future requestors.
pub(crate) const CLIPBOARD_MANAGER_WINDOW: u32 = 0x00000014;

/// Window ID used as the system tray manager (_NET_SYSTEM_TRAY_S0 owner).
pub(crate) const SYSTEM_TRAY_WINDOW: u32 = 0x00000016;

/// Saved clipboard entry for persistence across client disconnects.
#[derive(Clone)]
pub(crate) struct PersistentClipboardEntry {
    /// The selection data keyed by target atom (e.g., UTF8_STRING → data bytes).
    pub(crate) targets: HashMap<u32, Vec<u8>>,
    /// Timestamp when the data was saved.
    pub(crate) timestamp: u32,
}

/// Shared persistent clipboard state: maps selection atom → saved data.
/// Used by the server's built-in clipboard manager to preserve clipboard
/// contents when the owning client disconnects.
pub(crate) type PersistentClipboard = Arc<Mutex<HashMap<u32, PersistentClipboardEntry>>>;

/// State for an in-progress INCR selection transfer.
pub(crate) struct IncrTransfer {
    pub(crate) requestor: u32,
    pub(crate) property: u32,
    /// Selection atom this transfer belongs to (needed for multi-selection disambiguation).
    #[allow(dead_code)]
    pub(crate) selection: u32,
    pub(crate) target: u32,
    pub(crate) data: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) chunk_size: usize,
    /// Timestamp of the last activity (creation or chunk send/receive).
    /// Used to time out stale transfers per X11 spec.
    pub(crate) last_activity: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incr_transfer_chunk_state() {
        let transfer = IncrTransfer {
            requestor: 100,
            property: 200,
            selection: 300,
            target: 400,
            data: vec![0u8; 200_000], // 200KB
            offset: 0,
            chunk_size: 65536,
            last_activity: Instant::now(),
        };
        // First chunk should be chunk_size bytes
        let remaining = transfer.data.len() - transfer.offset;
        let chunk = remaining.min(transfer.chunk_size);
        assert_eq!(chunk, 65536);
        // After 3 chunks
        let offset_after_3 = 65536 * 3;
        let remaining = transfer.data.len() - offset_after_3;
        assert_eq!(remaining, 200_000 - 65536 * 3);
        // Last chunk is smaller
        let last_chunk = remaining.min(65536);
        assert!(last_chunk < 65536);
        assert_eq!(last_chunk, 200_000 - 65536 * 3);
    }

    #[test]
    fn incr_transfer_completes_when_offset_equals_len() {
        let transfer = IncrTransfer {
            requestor: 1,
            property: 2,
            selection: 3,
            target: 4,
            data: vec![42u8; 100],
            offset: 100, // All data sent
            chunk_size: 50,
            last_activity: Instant::now(),
        };
        let remaining = transfer.data.len() - transfer.offset;
        assert_eq!(remaining, 0);
    }

    #[test]
    fn persistent_clipboard_entry_multiple_targets() {
        let mut entry = PersistentClipboardEntry {
            targets: HashMap::new(),
            timestamp: 12345,
        };
        entry.targets.insert(100, b"hello".to_vec()); // UTF8_STRING
        entry.targets.insert(101, b"hello".to_vec()); // STRING
        assert_eq!(entry.targets.len(), 2);
        assert!(entry.targets.contains_key(&100));
        assert!(entry.targets.contains_key(&101));
    }

    #[test]
    fn incr_threshold_is_reasonable() {
        // The INCR threshold should be large enough to avoid unnecessary
        // incremental transfers for typical clipboard data.
        // Mirrors the constant in handlers/property.rs.
        const INCR_THRESHOLD: usize = 65536;
        assert!(INCR_THRESHOLD >= 4096, "INCR threshold should be at least 4KB");
        assert!(INCR_THRESHOLD <= 1_048_576, "INCR threshold should be at most 1MB");
    }
}
