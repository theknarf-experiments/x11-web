//! Per-workspace Automerge document.
//!
//! Each workspace is a CRDT doc shared between the backend and every
//! frontend bound to that workspace. The schema below is the single
//! source of truth for the doc shape; autosurgeon's `Reconcile` /
//! `Hydrate` derives handle the struct ↔ doc projection so we stay in
//! ordinary Rust types instead of touching Automerge ops directly.
//!
//! Authority boundary: the doc holds anything user-collaborative on
//! the canvas — boxes, text, arrows, pen strokes, AND windows. A
//! window is just an `OcifNode` carrying the `@x11web/window`
//! extension; its `(x, y, z, width, height)` are the doc's
//! authoritative geometry. Sidecar-driven state (title, focus,
//! wm_state, menu, render content) lives outside the doc and flows
//! downstream via `WindowList` / `WindowUpdate`. When the X server
//! reports a window resize, the backend mirrors the new
//! `width`/`height` onto every workspace's window-node so peers
//! converge.
//!
//! The sync protocol is `automerge::sync` — symmetric peer-to-peer
//! sync messages. The backend acts as a peer + persistence keeper;
//! frontends are the other peers. The control DC carries the raw
//! `automerge::sync::Message::encode` bytes.

use std::collections::HashMap;

use automerge::sync::{State as SyncState, SyncDoc};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjType, ReadDoc, ScalarValue, Value, ROOT};
#[cfg(test)]
use autosurgeon::hydrate;
use autosurgeon::{reconcile, Hydrate, Reconcile};

#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct WorkspaceDoc {
    /// Display name. Auto-assigned `"Workspace N"` on creation;
    /// editable inline via the menu bar.
    pub name: String,
    /// Every node on the canvas — user-drawn boxes / text / arrows /
    /// pen strokes AND attached windows (carried as `OcifNode`s with
    /// the `@x11web/window` extension). Keyed by UUID; for window
    /// nodes the key IS the underlying `window_id` so edges can
    /// reference them without a separate lookup. Follows OCIF v0.7.0
    /// node shape — see <https://canvasprotocol.org>.
    pub nodes: HashMap<String, OcifNode>,
    /// OCIF resources — text, images, or any other displayable
    /// content referenced by nodes via the `resource` field.
    /// Keyed by resource id (UUID). The spec stores resources as
    /// a top-level array of `{id, representations, ...}`; we use a
    /// Map keyed on `id` for the same reason `nodes` is mapped:
    /// CRDT-friendly insert/delete semantics.
    pub resources: HashMap<String, OcifResource>,
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
    /// `@ocif/path` extension. Stored as a flat list of input
    /// samples in node-local coords; the renderer runs
    /// perfect-freehand on them at draw time. Append-only during a
    /// stroke so concurrent edits merge cleanly via Automerge list
    /// semantics — the wire delta per sample is just three pushed
    /// floats.
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
    /// `@x11web/window` extension. Marks this node as a live
    /// window streamed from a sidecar — node geometry is canonical
    /// for layout, but title / focus / pixels come from the sidecar
    /// (mirrored downstream via `WindowList` / `WindowUpdate`).
    pub window: Option<WindowExt>,
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

/// `@ocif/path` — raw input samples for a freehand stroke. Stored
/// as a flat list of `(x, y, pressure)` triples in node-local
/// coords (origin = the first sampled canvas point). The renderer
/// runs perfect-freehand on these at draw time and emits a closed
/// filled polygon — `fill_color` carries the drawn color, and
/// `stroke_width` / `stroke_color` are typically unused.
///
/// Append-only during a stroke: each pointermove pushes three
/// floats onto `points`. That's the wire delta — concurrent peers
/// merge cleanly via Automerge's list semantics, and the round-
/// trip is bounded regardless of how long the stroke gets.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct PathExt {
    pub points: Vec<f64>,
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

/// `@x11web/window` — identity of a live window streamed from a
/// sidecar. The hosting `OcifNode` carries the canvas-side state
/// (position / z / size); this extension links it to the
/// underlying X11 / macOS window so the renderer can pull pixels +
/// title / wm_state / menu / focus from the sidecar-driven
/// `WindowList`. Custom extension since OCIF doesn't yet have a
/// "live media" node concept.
#[derive(Debug, Clone, Default, Reconcile, Hydrate)]
pub struct WindowExt {
    pub window_id: String,
    pub sidecar_id: String,
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
            nodes: HashMap::new(),
            resources: HashMap::new(),
        };
        reconcile(&mut doc, &seed).expect("reconcile fresh workspace doc");
        Self {
            doc,
            peer_states: HashMap::new(),
        }
    }

    /// Hydrate the doc into a Rust struct. Tests-only: real code
    /// paths use targeted ops so that frontend-created nodes (which
    /// omit `Null`s for absent `Option` fields) don't crash the
    /// hydrator.
    #[cfg(test)]
    pub fn snapshot(&self) -> WorkspaceDoc {
        hydrate(&self.doc).expect("workspace doc hydrate")
    }

    /// Generate the next outbound sync message for `peer_id`, if
    /// any. Returns `None` when the peer is fully caught up.
    pub fn generate_sync(&mut self, peer_id: &str) -> Option<Vec<u8>> {
        let state = self.peer_states.entry(peer_id.to_string()).or_default();
        self.doc
            .sync()
            .generate_sync_message(state)
            .map(|m| m.encode())
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

    /// Insert a window-node — an `OcifNode` with the
    /// `@x11web/window` extension. The node id IS the underlying
    /// `window_id` so edges can reference it directly. Returns
    /// `true` if a new node was created (false on duplicate, which
    /// is idempotent — the caller can safely retry).
    ///
    /// Uses targeted Automerge ops rather than `hydrate` +
    /// `reconcile`. Two reasons:
    ///   1. The frontend writes nodes via `Automerge.change` without
    ///      emitting explicit `Null`s for absent `Option<T>` fields,
    ///      and autosurgeon's `Hydrate<Option<T>>` rejects absent
    ///      keys. So any backend code path that hydrated the whole
    ///      doc would panic the moment a frontend-created node
    ///      landed.
    ///   2. Even setting (1) aside, `reconcile` is `O(doc size)`
    ///      and runs on every X11 spawn / drag-attach.
    ///
    /// We still write each `Option<T>` field as an explicit `Null`
    /// scalar so any consumer that *does* hydrate (e.g. tests) sees
    /// a fully-shaped node.
    pub fn attach_window_node(
        &mut self,
        window_id: &str,
        sidecar_id: &str,
        x: f64,
        y: f64,
        z: f64,
        width: f64,
        height: f64,
    ) -> bool {
        let Some(nodes) = ensure_nodes_map(&mut self.doc) else {
            return false;
        };
        if self.doc.get(&nodes, window_id).ok().flatten().is_some() {
            return false;
        }
        let Ok(node) = self.doc.put_object(&nodes, window_id, ObjType::Map) else {
            return false;
        };
        let _ = self.doc.put(&node, "x", x);
        let _ = self.doc.put(&node, "y", y);
        let _ = self.doc.put(&node, "z", z);
        let _ = self.doc.put(&node, "width", width);
        let _ = self.doc.put(&node, "height", height);
        // Explicit Null for every Option<T> field on `OcifNode` so a
        // future hydrate (e.g. in tests) doesn't choke on missing
        // keys.
        for key in ["rect", "path", "arrow", "edge", "text_style", "resource"] {
            let _ = self.doc.put(&node, key, ScalarValue::Null);
        }
        let Ok(window) = self.doc.put_object(&node, "window", ObjType::Map) else {
            return false;
        };
        let _ = self.doc.put(&window, "window_id", window_id);
        let _ = self.doc.put(&window, "sidecar_id", sidecar_id);
        true
    }

    /// Remove the window-node for `window_id`. Returns `true` if
    /// the node was present. Sweeps edges that referenced the
    /// disappearing window so they don't dangle.
    pub fn detach_window_node(&mut self, window_id: &str) -> bool {
        let Some(nodes) = get_map(&self.doc, &ROOT, "nodes") else {
            return false;
        };
        if self.doc.get(&nodes, window_id).ok().flatten().is_none() {
            return false;
        }
        if self.doc.delete(&nodes, window_id).is_err() {
            return false;
        }
        // Drop any node whose `edge` references the removed id.
        // Two-pass: collect ids first since we can't mutate while
        // iterating Automerge keys.
        let to_delete: Vec<String> = self
            .doc
            .keys(&nodes)
            .filter(|other| {
                let Some(edge) =
                    get_map(&self.doc, &nodes, other).and_then(|n| get_map(&self.doc, &n, "edge"))
                else {
                    return false;
                };
                let start = read_string(&self.doc, &edge, "start");
                let end = read_string(&self.doc, &edge, "end");
                start.as_deref() == Some(window_id) || end.as_deref() == Some(window_id)
            })
            .collect();
        for other in to_delete {
            let _ = self.doc.delete(&nodes, &other);
        }
        true
    }

    /// Mirror sidecar-reported position onto the window-node.
    /// Only used for the *pre-map* `WindowConfigured` that the X
    /// server emits when honoring `WM_NORMAL_HINTS` USPosition /
    /// PPosition — the auto-attach used the (0, 0) from
    /// `WindowCreated` so the cascade fallback kicked in; this lets
    /// the WM-style position rewrite the node before the user sees
    /// it. Returns `true` if a node existed and x/y actually
    /// changed.
    pub fn set_window_node_position(&mut self, window_id: &str, x: f64, y: f64) -> bool {
        let Some(nodes) = get_map(&self.doc, &ROOT, "nodes") else {
            return false;
        };
        let Some(node) = get_map(&self.doc, &nodes, window_id) else {
            return false;
        };
        if get_map(&self.doc, &node, "window").is_none() {
            return false;
        }
        let cur_x = read_f64(&self.doc, &node, "x");
        let cur_y = read_f64(&self.doc, &node, "y");
        if cur_x == Some(x) && cur_y == Some(y) {
            return false;
        }
        let _ = self.doc.put(&node, "x", x);
        let _ = self.doc.put(&node, "y", y);
        true
    }

    /// Mirror sidecar-reported geometry onto the window-node.
    /// Hot path — called on every `WindowConfigured` from the
    /// sidecar. Targeted writes only (no full-doc reconcile).
    /// Returns `true` if a node existed and the size actually
    /// changed.
    pub fn set_window_node_size(&mut self, window_id: &str, width: f64, height: f64) -> bool {
        let Some(nodes) = get_map(&self.doc, &ROOT, "nodes") else {
            return false;
        };
        let Some(node) = get_map(&self.doc, &nodes, window_id) else {
            return false;
        };
        if get_map(&self.doc, &node, "window").is_none() {
            return false;
        }
        let cur_w = read_f64(&self.doc, &node, "width");
        let cur_h = read_f64(&self.doc, &node, "height");
        if cur_w == Some(width) && cur_h == Some(height) {
            return false;
        }
        let _ = self.doc.put(&node, "width", width);
        let _ = self.doc.put(&node, "height", height);
        true
    }

    /// Count of nodes + highest `z` in the doc. Computed via
    /// targeted ops so we never have to `hydrate` (which would
    /// reject frontend-created nodes that omit `Null`s for absent
    /// `Option` fields). Used by `backend_attach_window` to seed a
    /// cascade position + stacking order for new window-nodes.
    pub fn node_stats(&self) -> (usize, f64) {
        let Some(nodes) = get_map(&self.doc, &ROOT, "nodes") else {
            return (0, 0.0);
        };
        let mut count = 0usize;
        let mut max_z = 0.0_f64;
        for key in self.doc.keys(&nodes) {
            count += 1;
            if let Some(node) = get_map(&self.doc, &nodes, &key) {
                if let Some(z) = read_f64(&self.doc, &node, "z") {
                    if z > max_z {
                        max_z = z;
                    }
                }
            }
        }
        (count, max_z)
    }

    /// Iterate the window-node ids in this workspace. Used by
    /// `reconcile_streaming_after_change` to compute the global
    /// refcount across every workspace.
    pub fn window_node_ids(&self) -> Vec<String> {
        let Some(nodes) = get_map(&self.doc, &ROOT, "nodes") else {
            return Vec::new();
        };
        self.doc
            .keys(&nodes)
            .filter(|id| {
                get_map(&self.doc, &nodes, id)
                    .and_then(|n| get_map(&self.doc, &n, "window"))
                    .is_some()
            })
            .collect()
    }
}

/// Get-or-create the `nodes` map at the doc root. Returns `None`
/// only if the root's `nodes` slot already holds a non-map value
/// (which would be a schema bug).
fn ensure_nodes_map(doc: &mut AutoCommit) -> Option<automerge::ObjId> {
    if let Some(id) = get_map(doc, &ROOT, "nodes") {
        return Some(id);
    }
    doc.put_object(ROOT, "nodes", ObjType::Map).ok()
}

/// Read a child Map's ObjId by name, or `None` if the slot is
/// absent or holds a non-map value. Used by the targeted-mutation
/// helpers above to avoid touching `hydrate` / `reconcile` on the
/// hot path.
fn get_map(doc: &AutoCommit, parent: &automerge::ObjId, key: &str) -> Option<automerge::ObjId> {
    match doc.get(parent, key).ok().flatten() {
        Some((Value::Object(ObjType::Map), id)) => Some(id),
        _ => None,
    }
}

fn read_f64(doc: &AutoCommit, parent: &automerge::ObjId, key: &str) -> Option<f64> {
    match doc.get(parent, key).ok().flatten()? {
        (Value::Scalar(scalar), _) => match scalar.into_owned() {
            ScalarValue::F64(v) => Some(v),
            ScalarValue::Int(v) => Some(v as f64),
            ScalarValue::Uint(v) => Some(v as f64),
            _ => None,
        },
        _ => None,
    }
}

fn read_string(doc: &AutoCommit, parent: &automerge::ObjId, key: &str) -> Option<String> {
    match doc.get(parent, key).ok().flatten()? {
        (Value::Scalar(scalar), _) => match scalar.into_owned() {
            ScalarValue::Str(s) => Some(s.into()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_sync(
        backend: &mut WorkspaceEntry,
        peer_doc: &mut AutoCommit,
        peer_state: &mut SyncState,
    ) {
        for _ in 0..32 {
            let to_peer = backend.generate_sync("p");
            let from_peer = peer_doc
                .sync()
                .generate_sync_message(peer_state)
                .map(|m| m.encode());
            if to_peer.is_none() && from_peer.is_none() {
                break;
            }
            if let Some(b) = to_peer {
                let m = automerge::sync::Message::decode(&b).unwrap();
                peer_doc.sync().receive_sync_message(peer_state, m).unwrap();
            }
            if let Some(b) = from_peer {
                backend.receive_sync("p", &b).unwrap();
            }
        }
    }

    #[test]
    fn attach_detach_window_node_idempotent_and_visible_to_peers() {
        let mut backend = WorkspaceEntry::new("Workspace 1");
        assert!(backend.attach_window_node("win-a", "sc-1", 10.0, 20.0, 1.0, 100.0, 80.0));
        // Second attach is a no-op (idempotent).
        assert!(!backend.attach_window_node("win-a", "sc-1", 10.0, 20.0, 1.0, 100.0, 80.0));
        assert!(backend.attach_window_node("win-b", "sc-1", 30.0, 40.0, 2.0, 100.0, 80.0));

        // Sync to a peer and verify both window nodes arrive.
        let mut peer_doc = AutoCommit::new();
        let mut peer_state = SyncState::new();
        drain_sync(&mut backend, &mut peer_doc, &mut peer_state);
        let view: WorkspaceDoc = hydrate(&peer_doc).unwrap();
        let node_a = view.nodes.get("win-a").expect("win-a present");
        assert_eq!(node_a.window.as_ref().unwrap().window_id, "win-a");
        assert_eq!(node_a.window.as_ref().unwrap().sidecar_id, "sc-1");
        assert_eq!(node_a.x, 10.0);
        assert!(view.nodes.contains_key("win-b"));

        // Sidecar resize → mirrored onto the node.
        assert!(backend.set_window_node_size("win-a", 200.0, 150.0));
        // No-op if size unchanged.
        assert!(!backend.set_window_node_size("win-a", 200.0, 150.0));

        // Detach + sync — node disappears.
        assert!(backend.detach_window_node("win-a"));
        assert!(!backend.detach_window_node("win-a")); // idempotent
        drain_sync(&mut backend, &mut peer_doc, &mut peer_state);
        let view2: WorkspaceDoc = hydrate(&peer_doc).unwrap();
        assert!(!view2.nodes.contains_key("win-a"));
        let node_b = view2.nodes.get("win-b").unwrap();
        assert_eq!(node_b.width, 100.0);
        assert_eq!(node_b.height, 80.0);
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
