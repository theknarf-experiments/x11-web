// Per-workspace Automerge document plus the sync-protocol state per
// remote peer (the backend, here — only one peer for now). The
// backend is authoritative for the doc's existence and seeds it on
// `OpenWorkspace`; the frontend's first action is to receive the
// initial sync message over the control DataChannel and apply it.
//
// Schema MUST mirror `crates/backend/src/workspace_doc.rs`. The Rust
// side is the canonical shape; this file projects it into TS for
// type-safe access and is the only thing that touches Automerge ops
// directly on the browser.

import * as Automerge from "@automerge/automerge";
import { encodeWorkspaceSync } from "./rtcWire";

/** Doc shape — keep in sync with `WorkspaceDoc` in Rust. */
export interface WorkspaceDoc {
	name: string;
	attached_windows: { [windowId: string]: boolean };
	window_positions: { [windowId: string]: { x: number; y: number } };
	nodes: { [nodeId: string]: OcifNode };
}

/** OCIF-shaped user-drawn node. Internal flat shape; serializes to
 *  `{ id, position: [x,y,z], size: [w,h], data: [{type, ...}] }` on
 *  OCIF export. Keep in sync with `OcifNode` in Rust. */
export interface OcifNode {
	x: number;
	y: number;
	z: number;
	width: number;
	height: number;
	rect?: RectExt;
	arrow?: ArrowExt;
	edge?: EdgeExt;
	/** Inline text content rendered inside the node. Serializes to
	 *  a referenced `text/plain` resource on OCIF export. */
	text?: string;
}

/** `@ocif/rect` — all properties optional per spec. */
export interface RectExt {
	stroke_width?: number;
	stroke_color?: string;
	fill_color?: string;
}

/** `@ocif/arrow` — endpoints in canvas-space coords, plus stroke
 *  styling. The cached start/end coords are always present even
 *  for connected arrows — they're the fallback when an attachment
 *  is later detached. */
export interface ArrowExt {
	start_x: number;
	start_y: number;
	end_x: number;
	end_y: number;
	stroke_width?: number;
	stroke_color?: string;
}

/** `@ocif/edge` — relation between two nodes referenced by id.
 *  Each endpoint is independently optional: just `start` set is a
 *  half-attached arrow (start anchored, end free), and so on.
 *  When both are unset there's no point keeping the field; the
 *  caller drops it. */
export interface EdgeExt {
	start?: string;
	end?: string;
	directed: boolean;
}

interface PerWorkspace {
	doc: Automerge.Doc<WorkspaceDoc>;
	syncState: Automerge.SyncState;
}

/** Lazy per-workspace store. Created on first sync message. */
const docs = new Map<string, PerWorkspace>();

/** Per-workspace listener set. Fired after every applyInbound or
 *  local mutation so React subscribers can re-read the doc. */
const listeners = new Map<string, Set<() => void>>();

/** The active control DataChannel for shipping sync messages. Set
 *  by `useBackendSocket` once the channel opens and cleared on
 *  close. Local mutations (e.g., `setName`) push their generated
 *  sync messages straight onto this channel. */
let controlDc: RTCDataChannel | null = null;

export function setControlChannel(dc: RTCDataChannel | null) {
	controlDc = dc;
}

export function subscribe(
	workspaceId: string,
	listener: () => void,
): () => void {
	let set = listeners.get(workspaceId);
	if (!set) {
		set = new Set();
		listeners.set(workspaceId, set);
	}
	set.add(listener);
	return () => {
		set?.delete(listener);
	};
}

function notify(workspaceId: string) {
	listeners.get(workspaceId)?.forEach((fn) => fn());
}

function ensure(workspaceId: string): PerWorkspace {
	let entry = docs.get(workspaceId);
	if (!entry) {
		entry = {
			doc: Automerge.init<WorkspaceDoc>(),
			syncState: Automerge.initSyncState(),
		};
		docs.set(workspaceId, entry);
	}
	return entry;
}

/** Apply an inbound sync message and return any reply messages the
 *  protocol wants to ship back. Drain to empty so the handshake
 *  converges in as few round-trips as possible. */
export function applyInbound(
	workspaceId: string,
	message: Uint8Array,
): Uint8Array[] {
	const entry = ensure(workspaceId);
	const [newDoc, newState] = Automerge.receiveSyncMessage(
		entry.doc,
		entry.syncState,
		message,
	);
	entry.doc = newDoc;
	entry.syncState = newState;
	notify(workspaceId);
	return drainOutbound(entry);
}

/** Generate any outbound sync messages we have queued for the peer.
 *  Called when the control DC opens (or any time we mutate the doc
 *  locally — slice 2+). */
export function generateOutbound(workspaceId: string): Uint8Array[] {
	return drainOutbound(ensure(workspaceId));
}

function drainOutbound(entry: PerWorkspace): Uint8Array[] {
	const out: Uint8Array[] = [];
	while (true) {
		const [newState, message] = Automerge.generateSyncMessage(
			entry.doc,
			entry.syncState,
		);
		entry.syncState = newState;
		if (!message) break;
		out.push(message);
	}
	return out;
}

function ship(workspaceId: string, messages: Uint8Array[]) {
	if (!controlDc || controlDc.readyState !== "open") return;
	for (const m of messages) {
		controlDc.send(encodeWorkspaceSync(workspaceId, m));
	}
}

/** Read-only view of the current hydrated doc. */
export function snapshot(workspaceId: string): WorkspaceDoc | null {
	const entry = docs.get(workspaceId);
	return entry ? (entry.doc as WorkspaceDoc) : null;
}

/** Read just the `name` field. Cheap; safe to call inside
 *  `useSyncExternalStore` getSnapshot. */
export function getName(workspaceId: string): string | null {
	return docs.get(workspaceId)?.doc.name ?? null;
}

/** Mutate the workspace's `name`. Generates and ships sync messages
 *  immediately if the control DC is open; otherwise the change sits
 *  in the local doc and is shipped on the next sync round. */
export function setName(workspaceId: string, name: string) {
	const entry = ensure(workspaceId);
	entry.doc = Automerge.change(entry.doc, (d) => {
		d.name = name;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Read the set of window ids currently attached to the workspace.
 *  Returns a fresh `Set` each call — pair with `subscribe` (or the
 *  `useAttachedWindowIds` hook) to know when to re-read. */
export function getAttachedWindowIds(workspaceId: string): Set<string> {
	const entry = docs.get(workspaceId);
	if (!entry) return new Set();
	const map = entry.doc.attached_windows as
		| { [k: string]: boolean }
		| undefined;
	return new Set(Object.keys(map ?? {}));
}

/** Add `windowId` to the workspace's attached set (drag-attach from
 *  the picker). Local doc mutation; backend learns about it via the
 *  next sync round and runs `Start/StopWindowCapture` if needed. */
export function attachWindow(workspaceId: string, windowId: string) {
	const entry = ensure(workspaceId);
	if ((entry.doc.attached_windows ?? {})[windowId]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.attached_windows) d.attached_windows = {};
		d.attached_windows[windowId] = true;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Remove `windowId` from the workspace's attached set. */
export function detachWindow(workspaceId: string, windowId: string) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.attached_windows ?? {})[windowId]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (d.attached_windows) delete d.attached_windows[windowId];
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Read the tracked position for `windowId`, or `null` if no
 *  frontend has placed this window yet (the renderer falls back to
 *  the descriptor's geometry or a cascade seed). */
export function getPosition(
	workspaceId: string,
	windowId: string,
): { x: number; y: number } | null {
	const entry = docs.get(workspaceId);
	if (!entry) return null;
	const map = entry.doc.window_positions as
		| { [k: string]: { x: number; y: number } }
		| undefined;
	const pos = map?.[windowId];
	return pos ? { x: pos.x, y: pos.y } : null;
}

/** Snapshot every tracked position for a workspace. Used by the
 *  frontend's mirror loop to re-sync `WindowRow.x/y` against the
 *  doc on every notify. */
export function getAllPositions(
	workspaceId: string,
): Map<string, { x: number; y: number }> {
	const out = new Map<string, { x: number; y: number }>();
	const entry = docs.get(workspaceId);
	if (!entry) return out;
	const map = entry.doc.window_positions as
		| { [k: string]: { x: number; y: number } }
		| undefined;
	if (!map) return out;
	for (const [k, v] of Object.entries(map)) {
		out.set(k, { x: v.x, y: v.y });
	}
	return out;
}

/** Set or update the tracked position for `windowId`. Called
 *  from drag handlers; the doc syncs to backend + sibling tabs. */
export function setPosition(
	workspaceId: string,
	windowId: string,
	x: number,
	y: number,
) {
	const entry = ensure(workspaceId);
	const existing = (entry.doc.window_positions ?? {})[windowId];
	if (existing && existing.x === x && existing.y === y) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.window_positions) d.window_positions = {};
		d.window_positions[windowId] = { x, y };
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Drop our local doc + sync state for a workspace. Currently
 *  unused; we'll call this if a frontend ever switches workspaces
 *  mid-session. */
export function forget(workspaceId: string) {
	docs.delete(workspaceId);
	listeners.delete(workspaceId);
}

// ---------- OCIF nodes (user-drawn shapes) ----------

/** Snapshot of every user-drawn node in the workspace. Returns a
 *  fresh map per call; pair with `subscribe` (or `useOcifNodes`) to
 *  know when to re-read. */
export function getOcifNodes(workspaceId: string): Map<string, OcifNode> {
	const out = new Map<string, OcifNode>();
	const entry = docs.get(workspaceId);
	if (!entry) return out;
	const nodes = entry.doc.nodes as { [k: string]: OcifNode } | undefined;
	if (!nodes) return out;
	for (const [k, v] of Object.entries(nodes)) {
		out.set(k, {
			x: v.x,
			y: v.y,
			z: v.z,
			width: v.width,
			height: v.height,
			rect: v.rect ? { ...v.rect } : undefined,
			arrow: v.arrow ? { ...v.arrow } : undefined,
			edge: v.edge ? { ...v.edge } : undefined,
			text: v.text,
		});
	}
	return out;
}

/** Insert a new node. Caller picks the id (typically `crypto.randomUUID()`). */
export function insertOcifNode(
	workspaceId: string,
	id: string,
	node: OcifNode,
) {
	const entry = ensure(workspaceId);
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.nodes) d.nodes = {};
		// Always seed `text` (default "") so the Automerge field
		// exists from creation. Adding a brand-new property to a
		// nested object via a later `change` can be flaky depending
		// on how the doc was reconciled — initialising it up front
		// sidesteps that.
		d.nodes[id] = {
			x: node.x,
			y: node.y,
			z: node.z,
			width: node.width,
			height: node.height,
			text: node.text ?? "",
			...(node.rect ? { rect: { ...node.rect } } : {}),
			...(node.arrow ? { arrow: { ...node.arrow } } : {}),
			...(node.edge ? { edge: { ...node.edge } } : {}),
		};
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Update a node's position. Optimistic — caller can patch local
 *  render state separately for snappier feel. */
export function setOcifNodePosition(
	workspaceId: string,
	id: string,
	x: number,
	y: number,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		n.x = x;
		n.y = y;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Update a node's size. */
export function setOcifNodeSize(
	workspaceId: string,
	id: string,
	width: number,
	height: number,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		n.width = width;
		n.height = height;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Update a node's bounds (position + size) in a single mutation —
 *  used by the resize gesture so it only ships one sync message
 *  per pointermove instead of two. */
export function setOcifNodeBounds(
	workspaceId: string,
	id: string,
	x: number,
	y: number,
	width: number,
	height: number,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		n.x = x;
		n.y = y;
		n.width = width;
		n.height = height;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Set a node's inline text content. Pass an empty string to
 *  clear; the field stays present in the doc. */
export function setOcifNodeText(
	workspaceId: string,
	id: string,
	text: string,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		n.text = text;
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Update an arrow node's endpoints in a single mutation. Callers
 *  pass canvas-space coords; the renderer derives the SVG path via
 *  perfect-arrows. We also keep the node's `(x, y, width, height)`
 *  in sync with the arrow's bounding box so future hit-testing /
 *  layout that consults the OCIF size still works. */
export function setOcifArrowEndpoints(
	workspaceId: string,
	id: string,
	startX: number,
	startY: number,
	endX: number,
	endY: number,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n || !n.arrow) return;
		n.arrow.start_x = startX;
		n.arrow.start_y = startY;
		n.arrow.end_x = endX;
		n.arrow.end_y = endY;
		const minX = Math.min(startX, endX);
		const minY = Math.min(startY, endY);
		n.x = minX;
		n.y = minY;
		n.width = Math.abs(endX - startX);
		n.height = Math.abs(endY - startY);
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

export type ArrowAnchor =
	| { kind: "node"; nodeId: string }
	| { kind: "free"; x: number; y: number };

/** Atomic per-endpoint anchor update for an arrow.
 *  - `node` anchor: ensure `n.edge` exists, set
 *    `n.edge.{start|end}` to the node id. The cached
 *    `arrow.{start|end}_x/y` is left alone (it's the fallback
 *    used when this side is later detached).
 *  - `free` anchor: clear `n.edge.{start|end}` (and the whole
 *    `n.edge` if both ends are now unset) and write the canvas
 *    coord onto `arrow.{start|end}_x/y`.
 *
 *  Mutates `n.edge` directly inside the `Automerge.change`
 *  callback rather than capturing into a local — Automerge's
 *  change tracker doesn't always handle proxy-self-assignment
 *  cleanly, so the direct path keeps the mutation crisp. */
export function setOcifArrowAnchor(
	workspaceId: string,
	id: string,
	end: "start" | "end",
	anchor: ArrowAnchor,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n || !n.arrow) return;
		if (anchor.kind === "node") {
			if (!n.edge) n.edge = { directed: true };
			n.edge[end] = anchor.nodeId;
		} else {
			if (n.edge) {
				delete n.edge[end];
				if (
					n.edge.start === undefined &&
					n.edge.end === undefined
				) {
					delete n.edge;
				}
			}
			const xKey = end === "start" ? "start_x" : "end_x";
			const yKey = end === "start" ? "start_y" : "end_y";
			n.arrow[xKey] = anchor.x;
			n.arrow[yKey] = anchor.y;
		}
		// Refresh the node's bounding box from the cached coords.
		const sx = n.arrow.start_x;
		const sy = n.arrow.start_y;
		const ex = n.arrow.end_x;
		const ey = n.arrow.end_y;
		n.x = Math.min(sx, ex);
		n.y = Math.min(sy, ey);
		n.width = Math.abs(ex - sx);
		n.height = Math.abs(ey - sy);
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}


/** Delete a node from the workspace. Also drops any edges that
 *  referenced this node — a dangling edge with a missing endpoint
 *  has nothing to render against. */
export function deleteOcifNode(workspaceId: string, id: string) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.nodes) return;
		delete d.nodes[id];
		for (const [otherId, other] of Object.entries(d.nodes)) {
			const e = (other as OcifNode).edge;
			if (e && (e.start === id || e.end === id)) {
				delete d.nodes[otherId];
			}
		}
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}
