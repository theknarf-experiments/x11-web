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
import { layoutWithLines, prepareWithSegments } from "@chenglou/pretext";
import { encodeWorkspaceSync } from "./rtcWire";

/** Default font family for measuring + rendering text nodes. Must
 *  match the renderer's CSS so measured bounds line up with the
 *  on-screen text. Kept in this module so the helpers and the
 *  view layer agree. */
const DEFAULT_FONT_FAMILY =
	'-apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif';
/** Padding around the measured text, in canvas pixels. Matches
 *  the `padding: 8px 12px` rule on `.text` / `.editor` in
 *  `OcifTextLayer.module.css`. */
const TEXT_PAD_X = 12;
const TEXT_PAD_Y = 8;
/** Minimum width when the text is empty so the textarea has
 *  somewhere to render the caret. */
const EMPTY_TEXT_MIN_WIDTH = 24;

/** Measure rendered text dimensions via pretext (canvas-based,
 *  no DOM reflow). Returns the OUTER node bounds — measured width
 *  + padding. */
export function measureTextNodeBounds(
	text: string,
	fontSizePx: number,
	fontFamily: string = DEFAULT_FONT_FAMILY,
	bold = false,
	italic = false,
): { width: number; height: number } {
	const lineHeight = fontSizePx * 1.3;
	if (text === "") {
		return {
			width: EMPTY_TEXT_MIN_WIDTH + TEXT_PAD_X * 2,
			height: lineHeight + TEXT_PAD_Y * 2,
		};
	}
	const fontParts: string[] = [];
	if (italic) fontParts.push("italic");
	if (bold) fontParts.push("600");
	fontParts.push(`${fontSizePx}px`);
	fontParts.push(fontFamily);
	const font = fontParts.join(" ");
	const prepared = prepareWithSegments(text, font);
	// Huge maxWidth so we only wrap on explicit newlines.
	const result = layoutWithLines(prepared, 1e9, lineHeight);
	const widest =
		result.lines.length === 0
			? 0
			: Math.max(...result.lines.map((l) => l.width));
	return {
		width: Math.ceil(widest) + TEXT_PAD_X * 2,
		height: Math.ceil(result.height) + TEXT_PAD_Y * 2,
	};
}

/** Doc shape — keep in sync with `WorkspaceDoc` in Rust. */
export interface WorkspaceDoc {
	name: string;
	attached_windows: { [windowId: string]: boolean };
	window_positions: { [windowId: string]: { x: number; y: number } };
	nodes: { [nodeId: string]: OcifNode };
	resources: { [resourceId: string]: OcifResource };
}

/** OCIF resource — content referenced by nodes via the
 *  `OcifNode.resource` field. Renderable representations live in
 *  `representations`; today we only emit `text/plain`. */
export interface OcifResource {
	representations: OcifRepresentation[];
}

/** One representation of a resource. Spec requires one of
 *  `content` or `location`; we don't enforce. */
export interface OcifRepresentation {
	mime_type: string;
	content?: string;
	location?: string;
}

/** OCIF-shaped user-drawn node. Internal flat shape; serializes to
 *  `{ id, position: [x,y,z], size: [w,h], data: [{type, ...}] }` on
 *  OCIF export. Mirrors `OcifNode` in Rust, plus a derived `text`
 *  field resolved from the referenced resource at projection time
 *  (a render-side convenience — the doc only stores `resource`). */
export interface OcifNode {
	x: number;
	y: number;
	z: number;
	width: number;
	height: number;
	rect?: RectExt;
	arrow?: ArrowExt;
	edge?: EdgeExt;
	text_style?: TextStyleExt;
	/** Reference to a resource id in `WorkspaceDoc.resources`. The
	 *  renderer pulls the first `text/plain` representation off
	 *  the resource and displays it. */
	resource?: string;
	/** Derived: resolved `text/plain` content from `resource`.
	 *  Computed by `getOcifNodes` — not stored in the doc. */
	text?: string;
}

/** `@ocif/rect` — all properties optional per spec. */
export interface RectExt {
	stroke_width?: number;
	stroke_color?: string;
	fill_color?: string;
}

/** `@ocif/arrow` — endpoints in canvas-space coords, plus stroke
 *  styling and per-end markers. The cached start/end coords are
 *  always present even for connected arrows — they're the
 *  fallback when an attachment is later detached. Markers default
 *  to `"none"` on start and `"arrowhead"` on end when unset. */
export interface ArrowExt {
	start_x: number;
	start_y: number;
	end_x: number;
	end_y: number;
	stroke_width?: number;
	stroke_color?: string;
	start_marker?: "none" | "arrowhead";
	end_marker?: "none" | "arrowhead";
}

/** `@ocif/textstyle` — font and alignment per spec. All fields
 *  optional; renderer applies defaults when missing. */
export interface TextStyleExt {
	font_size_px?: number;
	font_family?: string;
	color?: string;
	align?: "left" | "right" | "center" | "justify";
	bold?: boolean;
	italic?: boolean;
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
			text_style: v.text_style ? { ...v.text_style } : undefined,
			resource: v.resource,
			text: resolveNodeText(entry.doc, v),
		});
	}
	return out;
}

/** Insert a new node. Caller picks the id (typically
 *  `crypto.randomUUID()`). Pass `node.text` to seed text content
 *  — we'll create a backing resource and hook up
 *  `node.resource`. The view-only `text` field on `OcifNode` is
 *  consumed here and translated into the spec-shaped resource.
 *
 *  For text-only nodes (no `rect` / `arrow`), the caller's
 *  `width` / `height` are ignored and replaced with measured
 *  bounds from the actual text + textstyle — keeps the node
 *  bounds tight from creation. */
export function insertOcifNode(
	workspaceId: string,
	id: string,
	node: OcifNode,
) {
	const entry = ensure(workspaceId);
	const isTextOnly = !node.rect && !node.arrow;
	const measured = isTextOnly
		? measureTextNodeBounds(
				node.text ?? "",
				node.text_style?.font_size_px ?? 14,
				node.text_style?.font_family,
				node.text_style?.bold,
				node.text_style?.italic,
			)
		: null;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.nodes) d.nodes = {};
		if (!d.resources) d.resources = {};
		// Allocate a resource up front so subsequent text edits
		// update the existing representation rather than creating
		// one lazily.
		const resourceId = node.resource ?? crypto.randomUUID();
		if (!d.resources[resourceId]) {
			d.resources[resourceId] = {
				representations: [
					{ mime_type: "text/plain", content: node.text ?? "" },
				],
			};
		}
		d.nodes[id] = {
			x: node.x,
			y: node.y,
			z: node.z,
			width: measured?.width ?? node.width,
			height: measured?.height ?? node.height,
			resource: resourceId,
			...(node.rect ? { rect: { ...node.rect } } : {}),
			...(node.arrow ? { arrow: { ...node.arrow } } : {}),
			...(node.edge ? { edge: { ...node.edge } } : {}),
			...(node.text_style ? { text_style: { ...node.text_style } } : {}),
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

/** Set a node's text content. Routes through the OCIF resource
 *  model: a `text/plain` representation lives on a resource that
 *  the node references via `resource`. Creates the resource the
 *  first time the node has text; updates the existing
 *  representation thereafter.
 *
 *  Text-only nodes (no `rect` / `arrow`) auto-resize to fit the
 *  measured text; boxes keep their user-set bounds. */
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
		writeText(d, n, text);
		if (!n.rect && !n.arrow) {
			const bounds = boundsForNode(n, text);
			n.width = bounds.width;
			n.height = bounds.height;
		}
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Set a text-only node's font size (in `@ocif/textstyle.font_size_px`).
 *  Re-measures and updates bounds so the rendered text fits. */
export function setOcifNodeFontSize(
	workspaceId: string,
	id: string,
	fontSizePx: number,
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		if (!n.text_style) n.text_style = {};
		n.text_style.font_size_px = fontSizePx;
		if (!n.rect && !n.arrow) {
			const text = readText(d, n);
			const bounds = boundsForNode(n, text);
			n.width = bounds.width;
			n.height = bounds.height;
		}
	});
	notify(workspaceId);
	ship(workspaceId, drainOutbound(entry));
}

/** Helper — write a `text/plain` representation through the
 *  resource layer. Mutates `d` directly inside an
 *  `Automerge.change` callback. */
function writeText(
	d: WorkspaceDoc,
	n: OcifNode,
	text: string,
): void {
	if (!d.resources) d.resources = {};
	let resourceId = n.resource;
	if (!resourceId) {
		resourceId = crypto.randomUUID();
		n.resource = resourceId;
		d.resources[resourceId] = {
			representations: [{ mime_type: "text/plain", content: text }],
		};
		return;
	}
	const r = d.resources[resourceId];
	if (!r) {
		d.resources[resourceId] = {
			representations: [{ mime_type: "text/plain", content: text }],
		};
		return;
	}
	const idx = r.representations.findIndex(
		(rep) => rep.mime_type === "text/plain",
	);
	if (idx >= 0) {
		r.representations[idx].content = text;
	} else {
		r.representations.push({ mime_type: "text/plain", content: text });
	}
}

/** Helper — read the `text/plain` content for a node from the
 *  shared resources map. Empty string when missing. */
function readText(d: WorkspaceDoc, n: OcifNode): string {
	if (!n.resource) return "";
	const r = d.resources?.[n.resource];
	if (!r) return "";
	const rep = r.representations.find(
		(rep) => rep.mime_type === "text/plain",
	);
	return rep?.content ?? "";
}

/** Helper — measure node bounds from current text + textstyle. */
function boundsForNode(n: OcifNode, text: string): { width: number; height: number } {
	const ts = n.text_style;
	return measureTextNodeBounds(
		text,
		ts?.font_size_px ?? 14,
		ts?.font_family ?? DEFAULT_FONT_FAMILY,
		ts?.bold ?? false,
		ts?.italic ?? false,
	);
}

/** Resolve a node's text content from its referenced resource.
 *  Returns an empty string if the node has no resource or the
 *  resource has no `text/plain` representation. */
export function resolveNodeText(
	doc: Automerge.Doc<WorkspaceDoc>,
	node: OcifNode,
): string {
	if (!node.resource) return "";
	const r = doc.resources?.[node.resource];
	if (!r) return "";
	const rep = r.representations.find((rep) => rep.mime_type === "text/plain");
	return rep?.content ?? "";
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
