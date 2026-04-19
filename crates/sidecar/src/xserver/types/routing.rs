//! Routing infrastructure: WindowRouter, EventRouter, EventBroadcaster.
//!
//! These types handle message delivery between the frontend, backend, and
//! individual X11 client connections.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use x11_web_protocol::{DisplayUpdate, InputEvent};
use x11rb_protocol::protocol::xproto::EventMask;
/// A display update tagged with the client_id that produced it.
pub type TaggedDisplayUpdate = (String, DisplayUpdate);

/// Shared screen size sender/receiver pair for dynamic RandR resize.
pub type ScreenSizeTx = watch::Sender<(u16, u16)>;
pub type ScreenSizeRx = watch::Receiver<(u16, u16)>;

/// Shared server grab lock. When Some(client_id), that client holds the server
/// grab and all other clients must wait before processing requests.
/// Uses std::sync::Mutex (not tokio) so it can be locked from synchronous
/// handler code (GrabServer/UngrabServer) without `try_lock()` spin loops.
/// The Notify is signaled when the grab is released so waiters wake immediately.
pub(crate) type ServerGrabLock = Arc<(Mutex<Option<String>>, tokio::sync::Notify)>;

/// Shared window registry, keyed by window ID.
/// All connections share a single window namespace, as required by X11.
pub(crate) type SharedWindows = Arc<Mutex<HashMap<u32, super::WindowState>>>;

/// Message sent to a specific X11 connection via the window router.
pub(crate) enum WindowMessage {
    Input(InputEvent),
    Resize(u16, u16),
}

/// Routes messages from the frontend to the correct X11 connection.
/// Maps window UUID → (sender, x11_window_id).
#[derive(Clone)]
pub struct WindowRouter {
    routes: Arc<Mutex<HashMap<String, WindowRoute>>>,
}

struct WindowRoute {
    tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    x11_window_id: u32,
}

impl WindowRouter {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn register(
        &self,
        uuid: &str,
        x11_wid: u32,
        tx: &mpsc::UnboundedSender<(u32, WindowMessage)>,
    ) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(
                uuid.to_string(),
                WindowRoute {
                    tx: tx.clone(),
                    x11_window_id: x11_wid,
                },
            );
        }
    }

    pub(crate) fn unregister_all(&self, uuids: &[String]) {
        if let Ok(mut routes) = self.routes.lock() {
            for uuid in uuids {
                routes.remove(uuid);
            }
        }
    }

    pub fn send_input(&self, window_uuid: &str, event: InputEvent) -> bool {
        if let Ok(routes) = self.routes.lock() {
            if let Some(route) = routes.get(window_uuid) {
                let _ = route
                    .tx
                    .send((route.x11_window_id, WindowMessage::Input(event)));
                return true;
            }
        }
        false
    }

    pub fn send_resize(&self, window_uuid: &str, width: u16, height: u16) -> bool {
        if let Ok(routes) = self.routes.lock() {
            if let Some(route) = routes.get(window_uuid) {
                let _ = route
                    .tx
                    .send((route.x11_window_id, WindowMessage::Resize(width, height)));
                return true;
            }
        }
        false
    }
}

/// Routes raw X11 events to the correct connection by X11 window ID.
/// Used for cross-connection event delivery (e.g., XDND, selections).
#[derive(Clone)]
pub(crate) struct EventRouter {
    /// Maps X11 window ID → event sender for the owning connection.
    routes: Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<Vec<u8>>>>>,
}

impl EventRouter {
    pub(crate) fn new() -> Self {
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a connection's event sender for a window.
    pub(crate) fn register(&self, x11_wid: u32, tx: &mpsc::UnboundedSender<Vec<u8>>) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(x11_wid, tx.clone());
        }
    }

    /// Unregister all windows for a set of window IDs.
    pub(crate) fn unregister(&self, wids: &[u32]) {
        if let Ok(mut routes) = self.routes.lock() {
            for wid in wids {
                routes.remove(wid);
            }
        }
    }

    /// Send a raw event to the connection that owns the given window.
    /// Returns true if the event was sent, false if no route was found.
    pub(crate) fn send_event(&self, x11_wid: u32, event: Vec<u8>) -> bool {
        if let Ok(routes) = self.routes.lock() {
            if let Some(tx) = routes.get(&x11_wid) {
                let _ = tx.send(event);
                return true;
            }
        }
        false
    }
}

/// Cross-connection event subscription entry.
/// Each entry represents a client that wants events on a specific window.
struct EventSubscription {
    /// Client identifier (to avoid sending events back to the source).
    client_id: String,
    /// Event mask bits this client has selected on the window.
    event_mask: u32,
    /// Channel to deliver events to this client.
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

/// Global client entry for broadcasting events to ALL clients (e.g., MappingNotify).
struct GlobalClient {
    client_id: String,
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

/// Cross-connection event broadcast system.
/// Per X11 spec, any client can select events on any window, and the server
/// must deliver matching events to all selecting clients. This structure
/// maintains per-window subscriber lists with event mask filtering.
#[derive(Clone)]
pub(crate) struct EventBroadcaster {
    /// Maps window ID → list of subscriptions from different clients.
    subscriptions: Arc<Mutex<HashMap<u32, Vec<EventSubscription>>>>,
    /// All registered clients for global broadcasts (e.g., MappingNotify).
    all_clients: Arc<Mutex<Vec<GlobalClient>>>,
}

impl EventBroadcaster {
    pub(crate) fn new() -> Self {
        Self {
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            all_clients: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a client for global broadcasts (call once per connection).
    pub(crate) fn register_client(&self, client_id: &str, tx: &mpsc::UnboundedSender<Vec<u8>>) {
        if let Ok(mut clients) = self.all_clients.lock() {
            // Update if exists, otherwise add
            if let Some(existing) = clients.iter_mut().find(|c| c.client_id == client_id) {
                existing.tx = tx.clone();
            } else {
                clients.push(GlobalClient {
                    client_id: client_id.to_string(),
                    tx: tx.clone(),
                });
            }
        }
    }

    /// Unregister a client from global broadcasts (call on disconnect).
    pub(crate) fn unregister_client(&self, client_id: &str) {
        if let Ok(mut clients) = self.all_clients.lock() {
            clients.retain(|c| c.client_id != client_id);
        }
    }

    /// Broadcast an event to all other connected clients unconditionally.
    /// Used for MappingNotify which per X11 spec must reach every client.
    /// The source client is excluded here because it receives the event
    /// via pending_events (pushed by the caller before invoking broadcast).
    pub(crate) fn broadcast_global(&self, event: &[u8], source_client_id: &str) -> usize {
        let mut delivered = 0;
        if let Ok(clients) = self.all_clients.lock() {
            for client in clients.iter() {
                if client.client_id != source_client_id {
                    let _ = client.tx.send(event.to_vec());
                    delivered += 1;
                }
            }
        }
        delivered
    }

    /// Subscribe a client to events on a window with the given event mask.
    /// If the client already has a subscription on this window, the mask is updated.
    pub(crate) fn subscribe(
        &self,
        window_id: u32,
        client_id: &str,
        event_mask: u32,
        tx: &mpsc::UnboundedSender<Vec<u8>>,
    ) {
        if let Ok(mut subs) = self.subscriptions.lock() {
            let list = subs.entry(window_id).or_default();
            // Update existing subscription or add new
            if let Some(existing) = list.iter_mut().find(|s| s.client_id == client_id) {
                existing.event_mask = event_mask;
                existing.tx = tx.clone();
            } else if event_mask != 0 {
                list.push(EventSubscription {
                    client_id: client_id.to_string(),
                    event_mask,
                    tx: tx.clone(),
                });
            }
        }
    }

    /// Unsubscribe a client from all windows and global broadcasts.
    pub(crate) fn unsubscribe_client(&self, client_id: &str) {
        if let Ok(mut subs) = self.subscriptions.lock() {
            for list in subs.values_mut() {
                list.retain(|s| s.client_id != client_id);
            }
            subs.retain(|_, list| !list.is_empty());
        }
        self.unregister_client(client_id);
    }

    /// Unsubscribe a client from a specific window.
    #[allow(dead_code)]
    pub(crate) fn unsubscribe_window(&self, window_id: u32, client_id: &str) {
        if let Ok(mut subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get_mut(&window_id) {
                list.retain(|s| s.client_id != client_id);
            }
        }
    }

    /// Broadcast an event to all clients that have selected the matching
    /// event mask on the given window, EXCEPT the source client.
    /// Returns the number of clients the event was delivered to.
    pub(crate) fn broadcast(
        &self,
        window_id: u32,
        event_mask_bit: u32,
        event: &[u8],
        source_client_id: &str,
    ) -> usize {
        let mut delivered = 0;
        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                for sub in list {
                    if sub.client_id != source_client_id && (sub.event_mask & event_mask_bit) != 0 {
                        let _ = sub.tx.send(event.to_vec());
                        delivered += 1;
                    }
                }
            }
        }
        delivered
    }

    /// Check whether another client already holds SubstructureRedirectMask or
    /// ResizeRedirectMask on the given window.
    ///
    /// Per X11 spec Section 12.3 these masks may only be selected by ONE client
    /// per window.  If a different client already holds any of the redirect bits
    /// being requested, this function returns `Some(conflicting_bits)`.
    /// Returns `None` when there is no conflict (either no redirects are being
    /// requested, or the only existing holder is the requesting client itself).
    pub(crate) fn check_redirect_conflict(
        &self,
        window_id: u32,
        new_mask: u32,
        client_id: &str,
    ) -> Option<u32> {
        // Bit positions for the two exclusive redirect masks
        let redirect_bits = u32::from(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::RESIZE_REDIRECT);
        let requested_redirects = new_mask & redirect_bits;
        if requested_redirects == 0 {
            return None; // No redirect masks requested, no conflict possible
        }

        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                for sub in list {
                    if sub.client_id != client_id && (sub.event_mask & requested_redirects) != 0 {
                        return Some(requested_redirects & sub.event_mask); // Conflict!
                    }
                }
            }
        }
        None
    }

    /// Check if any client OTHER than `exclude_client` has subscribed to the
    /// given event mask bit on the specified window.
    pub(crate) fn has_mask_subscriber(
        &self,
        window_id: u32,
        event_mask_bit: u32,
        exclude_client: &str,
    ) -> bool {
        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                for sub in list {
                    if sub.client_id != exclude_client && (sub.event_mask & event_mask_bit) != 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Get the event mask a specific client has subscribed to on a window.
    #[allow(dead_code)]
    pub(crate) fn client_event_mask(&self, window_id: u32, client_id: &str) -> u32 {
        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                for sub in list {
                    if sub.client_id == client_id {
                        return sub.event_mask;
                    }
                }
            }
        }
        0
    }

    /// Get the union of all clients' event masks on a window.
    pub(crate) fn all_event_masks(&self, window_id: u32) -> u32 {
        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                return list.iter().fold(0u32, |acc, sub| acc | sub.event_mask);
            }
        }
        0
    }

    /// Broadcast an event to ALL clients on the given window (e.g., MappingNotify).
    /// Does not filter by event mask or source client.
    #[allow(dead_code)]
    pub(crate) fn broadcast_all(&self, window_id: u32, event: &[u8]) -> usize {
        let mut delivered = 0;
        if let Ok(subs) = self.subscriptions.lock() {
            if let Some(list) = subs.get(&window_id) {
                for sub in list {
                    let _ = sub.tx.send(event.to_vec());
                    delivered += 1;
                }
            }
        }
        delivered
    }
}
