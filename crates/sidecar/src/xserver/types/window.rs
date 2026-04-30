//! Window state, property storage, WM state, and RAII guards.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::pixmap::{SharedGcs, SharedPixmaps};
use super::region::RegionRect;
use super::routing::{EventBroadcaster, SharedWindows};
use crate::framebuffer::Framebuffer;

/// EWMH window type, used for stacking layer and focus/decoration policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowType {
    Desktop,
    Dock,
    Toolbar,
    Menu,
    Utility,
    Splash,
    Dialog,
    DropdownMenu,
    PopupMenu,
    Tooltip,
    Notification,
    Normal,
}

impl WindowType {
    /// Stacking layer for this window type. Higher = on top.
    /// Layer 0: Desktop (always at bottom)
    /// Layer 1: Below (windows with _NET_WM_STATE_BELOW)
    /// Layer 2: Normal, Dialog, Splash, Utility, Toolbar
    /// Layer 3: Above / Dock (panels, taskbars)
    /// Layer 4: Notification, Tooltip, PopupMenu, DropdownMenu, Menu
    pub(crate) fn stacking_layer(self) -> u8 {
        match self {
            WindowType::Desktop => 0,
            WindowType::Normal
            | WindowType::Dialog
            | WindowType::Splash
            | WindowType::Utility
            | WindowType::Toolbar => 2,
            WindowType::Dock => 3,
            WindowType::Menu
            | WindowType::DropdownMenu
            | WindowType::PopupMenu
            | WindowType::Tooltip
            | WindowType::Notification => 4,
        }
    }

    /// Whether this window type should receive input focus by default.
    #[cfg(test)]
    pub(crate) fn accepts_focus(self) -> bool {
        match self {
            WindowType::Normal
            | WindowType::Dialog
            | WindowType::Utility
            | WindowType::Toolbar
            | WindowType::Splash => true,
            WindowType::Desktop
            | WindowType::Dock
            | WindowType::Menu
            | WindowType::DropdownMenu
            | WindowType::PopupMenu
            | WindowType::Tooltip
            | WindowType::Notification => false,
        }
    }

    /// Resolve from _NET_WM_WINDOW_TYPE atom IDs (tries first match in list per EWMH spec).
    pub(crate) fn from_atom_ids(atoms: &[u32]) -> Self {
        for &atom in atoms {
            match atom {
                87 => return WindowType::Desktop,
                86 => return WindowType::Dock,
                82 => return WindowType::Toolbar,
                83 => return WindowType::Menu,
                84 => return WindowType::Utility,
                85 => return WindowType::Splash,
                81 => return WindowType::Dialog,
                88 => return WindowType::DropdownMenu,
                89 => return WindowType::PopupMenu,
                90 => return WindowType::Tooltip,
                91 => return WindowType::Notification,
                80 => return WindowType::Normal,
                _ => {} // unknown type, try next
            }
        }
        WindowType::Normal
    }
}

/// Stored X11 property value.
#[derive(Clone, Debug)]
pub(crate) struct PropertyValue {
    pub(crate) prop_type: u32,
    pub(crate) format: u8,
    pub(crate) data: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct WindowState {
    pub(crate) id: u32,
    pub(crate) parent: u32,
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) border_width: u16,
    pub(crate) visual: u32,
    /// Pixel depth derived from the visual (e.g. 24, 32, 16, 8, 4, 0 for InputOnly).
    pub(crate) depth: u8,
    pub(crate) class: u16,
    pub(crate) mapped: bool,
    pub(crate) event_mask: u32,
    pub(crate) do_not_propagate_mask: u32,
    pub(crate) background_pixel: u32,
    /// Background pixmap: None=inherit, Some(0)=None, Some(1)=ParentRelative, Some(pid)=pixmap ID.
    pub(crate) background_pixmap: Option<u32>,
    /// Border pixel color.
    pub(crate) border_pixel: u32,
    /// Border pixmap: None=CopyFromParent, Some(pid)=pixmap ID.
    pub(crate) border_pixmap: Option<u32>,
    pub(crate) override_redirect: bool,
    pub(crate) redirected: bool,
    pub(crate) framebuffer: Framebuffer,
    pub(crate) properties: HashMap<u32, PropertyValue>,
    pub(crate) owner_client_id: String,
    pub(crate) cursor: Option<u32>,
    /// Child window stacking order (bottom to top).
    pub(crate) children_order: Vec<u32>,
    /// Whether this window was retained from a client that disconnected with RetainTemporary.
    pub(crate) retained_temporary: bool,
    /// SHAPE extension: bounding shape (None = rectangular window).
    pub(crate) bounding_shape: Option<Vec<RegionRect>>,
    /// SHAPE extension: clip shape (None = same as bounding).
    pub(crate) clip_shape: Option<Vec<RegionRect>>,
    /// SHAPE extension: input shape (None = same as bounding).
    pub(crate) input_shape: Option<Vec<RegionRect>>,
    /// SHAPE extension: clients subscribed to ShapeNotify events.
    pub(crate) shape_select_clients: Vec<u32>,
    /// Colormap ID assigned to this window (0 = default/root colormap).
    pub(crate) colormap: u32,
    /// Backing store mode (BackingStore::NOT_USEFUL, WHEN_MAPPED, or ALWAYS).
    pub(crate) backing_store: u8,
    /// Backing planes mask: which bit planes to preserve in backing store.
    pub(crate) backing_planes: u32,
    /// Backing pixel: value used for planes not in backing_planes when restoring.
    pub(crate) backing_pixel: u32,
    /// Save-under flag.
    pub(crate) save_under: bool,
    /// Visibility state: 0=Unobscured, 1=PartiallyObscured, 2=FullyObscured.
    pub(crate) visibility: u8,
    /// Saved framebuffer pixels for backing store (when obscured).
    /// Only populated when backing_store != NOT_USEFUL (WhenMapped or Always).
    pub(crate) backing_pixmap: Option<Vec<u8>>,
    /// WM_HINTS initial_state: 1=NormalState, 3=IconicState. Set by ChangeProperty for WM_HINTS.
    pub(crate) wm_hints_initial_state: Option<u32>,
    /// WM_TRANSIENT_FOR: the window ID of the transient-for parent (ICCCM §4.1.2.6).
    pub(crate) transient_for: Option<u32>,
    /// _NET_WM_SYNC_REQUEST_COUNTER: SYNC counter ID for tear-free resizing.
    pub(crate) sync_request_counter: Option<u32>,
    /// Pending sync request value (incremented before each resize, awaited before compositing).
    pub(crate) sync_request_value: u64,
    /// EWMH window type (derived from _NET_WM_WINDOW_TYPE property).
    pub(crate) window_type: WindowType,
    /// _NET_WM_STRUT reserved space: (left, right, top, bottom).
    pub(crate) strut: Option<[u32; 4]>,
    /// WM_HINTS input field (ICCCM §4.1.2.4): whether the window accepts focus.
    /// None = not set (defaults to true per ICCCM), Some(true) = accepts input,
    /// Some(false) = does not accept input (Globally Active or No Input model).
    pub(crate) wm_hints_input: Option<bool>,
    /// WM_HINTS window_group (ICCCM §4.1.2.6): leader window for this group.
    pub(crate) wm_hints_window_group: Option<u32>,
    /// Whether _NET_WM_STATE_MODAL is currently set on this window.
    pub(crate) modal: bool,
    /// Saved geometry before entering fullscreen/maximized state (x, y, width, height).
    /// Restored when leaving the state.
    pub(crate) saved_geometry: Option<(i16, i16, u16, u16)>,
}

impl WindowState {
    /// Returns the effective shape used for rendering clipping.
    /// Prefers clip_shape, falls back to bounding_shape, or None (no clipping).
    pub(crate) fn effective_render_shape(&self) -> Option<&[RegionRect]> {
        self.clip_shape
            .as_deref()
            .or(self.bounding_shape.as_deref())
    }
}

/// Check if a point falls within at least one rectangle of a shape region.
pub(crate) fn point_in_shape(shape: &[RegionRect], x: i16, y: i16) -> bool {
    shape
        .iter()
        .any(|r| x >= r.x && x < r.x + r.width as i16 && y >= r.y && y < r.y + r.height as i16)
}

/// WM_NORMAL_HINTS (size hints) parsed from ICCCM properties.
#[derive(Clone, Debug, Default)]
pub(crate) struct SizeHints {
    pub(crate) min_width: u16,
    pub(crate) min_height: u16,
    pub(crate) max_width: u16,
    pub(crate) max_height: u16,
    pub(crate) width_inc: u16,
    pub(crate) height_inc: u16,
    pub(crate) base_width: u16,
    pub(crate) base_height: u16,
}

/// Shared window-manager state.
pub(crate) struct WmState {
    pub(crate) client_id: Option<String>,
    pub(crate) event_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

pub(crate) type SharedWmState = Arc<Mutex<WmState>>;

/// RAII guard that clears the shared WM state when the WM client disconnects.
pub(crate) struct WmCleanupGuard {
    pub(crate) wm_state: SharedWmState,
    pub(crate) client_id: String,
}

impl Drop for WmCleanupGuard {
    fn drop(&mut self) {
        if let Ok(mut wm) = self.wm_state.lock() {
            if wm.client_id.as_deref() == Some(&self.client_id) {
                tracing::info!(
                    "WM client {} disconnected – clearing WM state",
                    self.client_id
                );
                wm.client_id = None;
                wm.event_tx = None;
            }
        }
    }
}

/// Registry of connected client resource bases (for X-Resource QueryClients).
/// Each entry is the resource_id_base assigned to a connected client.
pub(crate) type SharedClientRegistry = Arc<Mutex<Vec<u32>>>;

/// RAII guard that removes this client's resource base from the shared registry on disconnect.
pub(crate) struct ClientRegistryGuard {
    pub(crate) registry: SharedClientRegistry,
    pub(crate) resource_id_base: u32,
}

impl Drop for ClientRegistryGuard {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.retain(|&base| base != self.resource_id_base);
        }
    }
}

/// RAII guard that runs shared-registry cleanup when a client connection task
/// exits. Without this, a client that disconnects via a write error (broken
/// pipe) bypasses the explicit `n == 0` cleanup path inside the request loop —
/// its windows leak into [`SharedWindows`] forever, and any future client that
/// clones the shared map sees the orphaned entries.
///
/// The guard is a fallback: when the explicit `n == 0` cleanup runs to
/// completion, it sets `cleanup_done` to true and the guard becomes a no-op.
/// If the connection task exits any other way (panic, write error, future
/// drop), the guard sweeps the shared registries based on the last known
/// close-down mode.
pub(crate) struct ClientResourcesCleanupGuard {
    pub(crate) shared_windows: SharedWindows,
    pub(crate) shared_pixmaps: SharedPixmaps,
    pub(crate) shared_gcs: SharedGcs,
    pub(crate) event_broadcaster: EventBroadcaster,
    pub(crate) client_id: String,
    /// Last value of `state.close_down_mode` written by the request loop.
    /// 0 = Destroy (default), 1 = RetainPermanent, 2 = RetainTemporary.
    pub(crate) close_down_mode: Arc<AtomicU8>,
    /// Set by the explicit `n == 0` cleanup path so the guard skips on Drop.
    pub(crate) cleanup_done: Arc<AtomicBool>,
}

impl Drop for ClientResourcesCleanupGuard {
    fn drop(&mut self) {
        if self.cleanup_done.load(Ordering::SeqCst) {
            return;
        }
        // Always unsubscribe — broadcaster references the dead channel
        // otherwise, which causes broadcasts to silently drop.
        self.event_broadcaster.unsubscribe_client(&self.client_id);

        // Only Destroy mode tears down the windows in the shared registry.
        // RetainPermanent/RetainTemporary leave them alive on purpose.
        if self.close_down_mode.load(Ordering::SeqCst) != 0 {
            return;
        }
        if let Ok(mut shared) = self.shared_windows.lock() {
            let owned: Vec<u32> = shared
                .iter()
                .filter(|(_, w)| w.owner_client_id == self.client_id)
                .map(|(&wid, _)| wid)
                .collect();
            for &wid in &owned {
                if let Some(parent_id) = shared.get(&wid).map(|w| w.parent) {
                    if let Some(parent) = shared.get_mut(&parent_id) {
                        parent.children_order.retain(|&c| c != wid);
                    }
                }
                shared.remove(&wid);
            }
        }
        if let Ok(mut shared) = self.shared_pixmaps.lock() {
            shared.retain(|_, p| p.owner_client_id != self.client_id);
        }
        // SharedGcs has no per-client owner field, so we conservatively leave
        // GCs alone here. Per-client GCs in state.gcs are already dropped
        // when ClientState drops.
        let _ = &self.shared_gcs;
    }
}
