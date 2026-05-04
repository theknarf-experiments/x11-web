//! Per-workspace Automerge document.
//!
//! Each workspace is a CRDT doc shared between the backend and every
//! frontend bound to that workspace. The schema below is the single
//! source of truth for the doc shape; autosurgeon's `Reconcile` /
//! `Hydrate` derives handle the struct ↔ doc projection so we stay in
//! ordinary Rust types instead of touching Automerge ops directly.
//!
//! Authority boundary: only state that is genuinely *user-collaborative
//! across frontends* lives in here. Sidecar-driven state (window
//! dimensions, focus, X server position for popups) stays in the
//! backend's `window_track` HashMap and continues to flow downstream
//! via `WindowList`. The doc holds the user's choices: which windows
//! they've attached, where they've placed them.
//!
//! The sync protocol is `automerge::sync` — symmetric peer-to-peer
//! sync messages. The backend acts as a peer + persistence keeper;
//! frontends are the other peers. The control DC carries the raw
//! `automerge::sync::Message::encode` bytes.
//!
//! Slice 1b only ships `name`. `attached_windows` and
//! `window_positions` come in slices 2 and 3.

use std::collections::HashMap;

use automerge::sync::{State as SyncState, SyncDoc};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjType, ReadDoc, ROOT};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct WorkspaceDoc {
    /// Display name. Auto-assigned `"Workspace N"` on creation;
    /// editable inline via the menu bar.
    pub name: String,
    /// Window IDs attached to this workspace's canvas. Map (set
    /// semantics) so concurrent attaches converge cleanly. The value
    /// is always `true` and is meaningless — presence is the signal.
    pub attached_windows: HashMap<String, bool>,
    /// Per-window tracked position.
    pub window_positions: HashMap<String, Position>,
    /// User-drawn OCIF-shaped nodes (rectangles, future arrows /
    /// text / paths). Keyed by UUID generated on create. Follows
    /// OCIF v0.7.0 node shape — see <https://canvasprotocol.org>.
    /// Windows aren't migrated into this map yet; once they are,
    /// `attached_windows` and `window_positions` collapse into it.
    pub nodes: HashMap<String, OcifNode>,
    /// OCIF resources — text, images, or any other displayable
    /// content referenced by nodes via the `resource` field.
    /// Keyed by resource id (UUID). The spec stores resources as
    /// a top-level array of `{id, representations, ...}`; we use a
    /// Map keyed on `id` for the same reason `nodes` is mapped:
    /// CRDT-friendly insert/delete semantics.
    pub resources: HashMap<String, OcifResource>,
}

#[derive(Debug, Clone, Copy, Reconcile, Hydrate)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// One OCIF node. Position is `[x, y, z]` per OCIF; we keep it as
/// scalar fields for autosurgeon ergonomics and emit the array at
/// serialization time. `z` drives stacking order (higher renders
/// on top). `width` / `height` are OCIF `size`.
///
/// At most one shape extension (`rect` / `arrow` / future ovals
/// etc.) is set per node. `text` is independent — a rect or arrow
/// can carry an inline text label.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct OcifNode {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub width: f64,
    pub height: f64,
    /// `@ocif/rect` extension.
    pub rect: Option<RectExt>,
    /// `@ocif/path` extension. The path string is in node-local
    /// coords starting at (0, 0), so the rendered SVG sits at the
    /// node's `(x, y)` and fills its `(width, height)`.
    pub path: Option<PathExt>,
    /// `@ocif/arrow` extension. For free-floating arrows the
    /// `start_x/y/end_x/y` are the visual endpoints. For connected
    /// arrows (also carrying `edge`), they're cached but the
    /// renderer recomputes from the connected nodes' bounds.
    pub arrow: Option<ArrowExt>,
    /// `@ocif/edge` extension. Connects two nodes by id —
    /// canonical OCIF "this is a relation" marker. Combined with
    /// `arrow` for visual treatment, this gives a directional
    /// connection that follows the connected boxes when they move
    /// or resize.
    pub edge: Option<EdgeExt>,
    /// `@ocif/textstyle` extension. All fields optional per spec;
    /// renderer falls back to defaults when missing.
    pub text_style: Option<TextStyleExt>,
    /// Reference to a resource in the workspace's `resources` map.
    /// When the resource has a `text/plain` representation, the
    /// renderer displays that text inside the node.
    pub resource: Option<String>,
}

/// `@ocif/rect` — fillColor / strokeColor / strokeWidth. All
/// optional per OCIF; the renderer falls back to defaults when
/// missing.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct RectExt {
    pub stroke_width: Option<f64>,
    pub stroke_color: Option<String>,
    pub fill_color: Option<String>,
}

/// `@ocif/path` — SVG-like path commands plus styling. The
/// freehand-pen pipeline stores the smoothed stroke as a closed
/// filled polygon (SVG path with `Z`), so `fill_color` carries the
/// drawn color and `stroke_width` / `stroke_color` are typically
/// unused. Path coordinates are local to the node — `(0, 0)` is
/// the top-left of the node's bounds.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct PathExt {
    pub path: String,
    pub stroke_width: Option<f64>,
    pub stroke_color: Option<String>,
    pub fill_color: Option<String>,
}

/// `@ocif/arrow` — start / end points (canvas-space coords),
/// strokeColor, strokeWidth, and per-end markers. The cached
/// start/end coords are always present even for connected arrows
/// so we have a fallback position when an attachment is later
/// detached. `start_marker` / `end_marker` are spec values
/// `"none"` or `"arrowhead"`; renderer falls back to "none" on
/// start and "arrowhead" on end when unset.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct ArrowExt {
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub stroke_width: Option<f64>,
    pub stroke_color: Option<String>,
    pub start_marker: Option<String>,
    pub end_marker: Option<String>,
}

/// `@ocif/textstyle` — font / color / alignment / weight. All
/// fields optional per spec; renderer applies sensible defaults
/// (14px sans-serif white centered, no bold/italic) when missing.
/// `align` is one of `"left" | "right" | "center" | "justify"`.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct TextStyleExt {
    pub font_size_px: Option<f64>,
    pub font_family: Option<String>,
    pub color: Option<String>,
    pub align: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

/// One OCIF resource — content that nodes can reference. A node
/// pointing at this resource displays the first representation it
/// can render (today: any `text/plain` representation). Future
/// representations might cover `text/markdown`, images, etc.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct OcifResource {
    pub representations: Vec<OcifRepresentation>,
}

/// One representation of a resource — exactly one of `content`
/// (inline) or `location` (URI to remote bytes) per spec, though
/// we don't enforce. `mime_type` is the IANA MIME type.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct OcifRepresentation {
    pub mime_type: String,
    pub content: Option<String>,
    pub location: Option<String>,
}

/// `@ocif/edge` — relation between two nodes referenced by id.
/// Each endpoint is independently optional: `start = Some(id)`
/// + `end = None` is a half-attached arrow (start anchored to a
/// node, end at the cached `arrow.end_x/y`), and vice versa.
/// `directed = true` renders as start → end; `false` undirected.
/// (OCIF v0.7's `@ocif/edge` requires both endpoints; we relax
/// that for partial-connection UX. On OCIF export, partials would
/// emit a `@ocif/hyperedge` or a custom `@x11web/anchor`
/// extension instead.)
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct EdgeExt {
    pub start: Option<String>,
    pub end: Option<String>,
    pub directed: bool,
}

/// Backend-side state for one workspace doc. Holds the doc itself
/// plus the per-peer sync state — Automerge's sync protocol needs to
/// remember what each peer has acknowledged so it can ship deltas
/// rather than re-sending the whole history every round.
pub struct WorkspaceEntry {
    pub doc: AutoCommit,
    /// `frontend_id → SyncState`. Created lazily on first sync from
    /// that peer.
    pub peer_states: HashMap<String, SyncState>,
}

impl WorkspaceEntry {
    /// Create a fresh empty doc seeded with the given name.
    pub fn new(name: &str) -> Self {
        let mut doc = AutoCommit::new();
        let seed = WorkspaceDoc {
            name: name.to_string(),
            attached_windows: HashMap::<String, bool>::new(),
            window_positions: HashMap::new(),
            nodes: HashMap::new(),
            resources: HashMap::new(),
        };
        reconcile(&mut doc, &seed).expect("reconcile fresh workspace doc");
        Self {
            doc,
            peer_states: HashMap::new(),
        }
    }

    /// Hydrate the doc into a Rust struct. Cheap, but not free —
    /// callers should batch reads rather than hydrating per-field.
    /// Used in slices 2+; kept here so the doc's `attached_windows` /
    /// `window_positions` are reachable from sync apply paths once we
    /// migrate them off `AppState`.
    #[allow(dead_code)]
    pub fn snapshot(&self) -> WorkspaceDoc {
        hydrate(&self.doc).expect("workspace doc hydrate")
    }

    /// Generate the next outbound sync message for `peer_id`, if
    /// any. Returns `None` when the peer is fully caught up.
    pub fn generate_sync(&mut self, peer_id: &str) -> Option<Vec<u8>> {
        let state = self.peer_states.entry(peer_id.to_string()).or_default();
        self.doc.sync().generate_sync_message(state).map(|m| m.encode())
    }

    /// Apply an inbound sync message from `peer_id`. The doc may
    /// change as a result; the caller should follow up with a
    /// `generate_sync` round to send any reply.
    pub fn receive_sync(&mut self, peer_id: &str, bytes: &[u8]) -> Result<(), String> {
        let message = automerge::sync::Message::decode(bytes)
            .map_err(|e| format!("decode sync message: {e}"))?;
        let state = self.peer_states.entry(peer_id.to_string()).or_default();
        self.doc
            .sync()
            .receive_sync_message(state, message)
            .map_err(|e| format!("receive sync message: {e}"))
    }

    /// Drop a peer's sync state (used on frontend disconnect to
    /// avoid unbounded growth).
    pub fn forget_peer(&mut self, peer_id: &str) {
        self.peer_states.remove(peer_id);
    }

    /// Peers we've ever exchanged a sync message with for this
    /// workspace. Caller fans out backend-side mutations to all of
    /// them — newly-connected peers start their first sync via the
    /// OpenWorkspace handler instead.
    pub fn peers(&self) -> Vec<String> {
        self.peer_states.keys().cloned().collect()
    }

    /// Add `window_id` to `attached_windows` (the doc's set). Returns
    /// `true` if the set actually changed. Used by backend-side
    /// mutations (X11 auto-attach on `WindowCreated`); frontend-side
    /// attaches arrive as inbound sync messages and never call this.
    pub fn attach(&mut self, window_id: &str) -> bool {
        let attached = match self.doc.get(ROOT, "attached_windows").ok().flatten() {
            Some((automerge::Value::Object(ObjType::Map), id)) => id,
            _ => match self.doc.put_object(ROOT, "attached_windows", ObjType::Map) {
                Ok(id) => id,
                Err(_) => return false,
            },
        };
        if self.doc.get(&attached, window_id).ok().flatten().is_some() {
            return false;
        }
        self.doc.put(&attached, window_id, true).is_ok()
    }

    /// Remove `window_id` from `attached_windows`. Returns `true` if
    /// the entry was present (and is now gone). Used on
    /// `WindowDestroyed` cleanup.
    pub fn detach(&mut self, window_id: &str) -> bool {
        let attached = match self.doc.get(ROOT, "attached_windows").ok().flatten() {
            Some((automerge::Value::Object(ObjType::Map), id)) => id,
            _ => return false,
        };
        if self.doc.get(&attached, window_id).ok().flatten().is_none() {
            return false;
        }
        self.doc.delete(&attached, window_id).is_ok()
    }

    /// Iterate the currently-attached window ids. Used by
    /// `reconcile_streaming_after_change` to compute the global
    /// refcount across every workspace.
    pub fn attached_window_ids(&self) -> Vec<String> {
        match self.doc.get(ROOT, "attached_windows").ok().flatten() {
            Some((automerge::Value::Object(ObjType::Map), id)) => {
                self.doc.keys(&id).collect()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_detach_idempotent_and_visible_to_peers() {
        let mut backend = WorkspaceEntry::new("Workspace 1");
        assert!(backend.attach("win-a"));
        // Second attach is a no-op.
        assert!(!backend.attach("win-a"));
        assert!(backend.attach("win-b"));

        // Sync to a peer and verify both windows arrive.
        let mut peer_doc = AutoCommit::new();
        let mut peer_state = SyncState::new();
        for _ in 0..32 {
            let to_peer = backend.generate_sync("p");
            let from_peer = peer_doc
                .sync()
                .generate_sync_message(&mut peer_state)
                .map(|m| m.encode());
            if to_peer.is_none() && from_peer.is_none() {
                break;
            }
            if let Some(b) = to_peer {
                let m = automerge::sync::Message::decode(&b).unwrap();
                peer_doc
                    .sync()
                    .receive_sync_message(&mut peer_state, m)
                    .unwrap();
            }
            if let Some(b) = from_peer {
                backend.receive_sync("p", &b).unwrap();
            }
        }
        let view: WorkspaceDoc = hydrate(&peer_doc).unwrap();
        assert!(view.attached_windows.contains_key("win-a"));
        assert!(view.attached_windows.contains_key("win-b"));

        // Detach + sync — windows disappear.
        assert!(backend.detach("win-a"));
        assert!(!backend.detach("win-a")); // idempotent
        for _ in 0..32 {
            let to_peer = backend.generate_sync("p");
            let from_peer = peer_doc
                .sync()
                .generate_sync_message(&mut peer_state)
                .map(|m| m.encode());
            if to_peer.is_none() && from_peer.is_none() {
                break;
            }
            if let Some(b) = to_peer {
                let m = automerge::sync::Message::decode(&b).unwrap();
                peer_doc
                    .sync()
                    .receive_sync_message(&mut peer_state, m)
                    .unwrap();
            }
            if let Some(b) = from_peer {
                backend.receive_sync("p", &b).unwrap();
            }
        }
        let view2: WorkspaceDoc = hydrate(&peer_doc).unwrap();
        assert!(!view2.attached_windows.contains_key("win-a"));
        assert!(view2.attached_windows.contains_key("win-b"));
    }

    #[test]
    fn two_peers_converge_on_initial_state() {
        // Backend creates a doc with a name; "frontend" is just
        // another AutoCommit on the other side. Run the sync
        // protocol to completion (both sides return None) and
        // verify both have the same hydrated state.
        let mut backend = WorkspaceEntry::new("Workspace 1");
        let mut frontend_doc = AutoCommit::new();
        let mut frontend_state = SyncState::new();
        let peer = "test-peer";

        // Bounded loop — should converge in <10 rounds.
        for _ in 0..32 {
            let to_frontend = backend.generate_sync(peer);
            let from_frontend = frontend_doc
                .sync()
                .generate_sync_message(&mut frontend_state)
                .map(|m| m.encode());

            if to_frontend.is_none() && from_frontend.is_none() {
                break;
            }

            if let Some(bytes) = to_frontend {
                let msg = automerge::sync::Message::decode(&bytes).unwrap();
                frontend_doc
                    .sync()
                    .receive_sync_message(&mut frontend_state, msg)
                    .unwrap();
            }
            if let Some(bytes) = from_frontend {
                backend.receive_sync(peer, &bytes).unwrap();
            }
        }

        let backend_view = backend.snapshot();
        let frontend_view: WorkspaceDoc = hydrate(&frontend_doc).unwrap();
        assert_eq!(backend_view.name, "Workspace 1");
        assert_eq!(frontend_view.name, "Workspace 1");
    }
}
