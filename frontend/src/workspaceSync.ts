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
import { getStroke } from "perfect-freehand";
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

/** Line-height multiplier matching the renderer (`OcifTextLayer`'s
 *  CSS `line-height: 1.3`). */
const LINE_HEIGHT_MULTIPLIER = 1.3;
/** Clamp range for the ternary search in
 *  `solveFontSizeForCornerTarget`. Same range we expose to the UI. */
export const FONT_MIN = 8;
export const FONT_MAX = 200;

/** Append one input sample to a path node's `points` list.
 *
 *  This is the hot path during a freehand stroke: one pointermove
 *  ⇒ one helper call ⇒ one Automerge list-push of three floats.
 *  Per-sample wire delta is constant regardless of how long the
 *  stroke is, and concurrent appends merge correctly via list
 *  semantics. The smoothed SVG path is computed at render time —
 *  the doc only ever stores raw input. */
export function appendOcifNodePathPoint(
	workspaceId: string,
	id: string,
	point: [number, number, number],
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n || !n.path) return;
		n.path.points.push(point[0], point[1], point[2]);
	});
	scheduleFlush(workspaceId);
}

const STROKE_OPTIONS = {
	size: 6,
	thinning: 0.6,
	smoothing: 0.5,
	streamline: 0.5,
	simulatePressure: true,
};

/** Run perfect-freehand on a flat `[x, y, p, x, y, p, ...]` list and
 *  return the smoothed outline polygon in the same coord space as
 *  the inputs. Returns `null` for inputs that can't form a stroke
 *  (< 2 samples, or perfect-freehand collapsed them). */
export function outlineFromPoints(
	points: number[],
): Array<[number, number]> | null {
	if (points.length < 6) return null;
	const inputPoints: Array<[number, number, number]> = [];
	for (let i = 0; i + 2 < points.length; i += 3) {
		inputPoints.push([points[i], points[i + 1], points[i + 2]]);
	}
	if (inputPoints.length < 2) return null;
	const stroke = getStroke(inputPoints, STROKE_OPTIONS) as Array<
		[number, number]
	>;
	return stroke.length < 2 ? null : stroke;
}

/** Smoothed SVG path string for the stroke — the outline polygon
 *  rendered as quadratic Béziers.
 *
 *  Called by `OcifPath` on every render — pure function of
 *  `points`, so memoize on the array reference. */
export function svgPathFromPoints(points: number[]): string | null {
	const stroke = outlineFromPoints(points);
	return stroke ? svgPathFromPolygon(stroke) : null;
}

/** Bounds of the smoothed stroke in input-coord space. Returns
 *  `null` for inputs that can't form a stroke. */
export function pathBoundsFromPoints(
	points: number[],
): { minX: number; minY: number; width: number; height: number } | null {
	const stroke = outlineFromPoints(points);
	if (!stroke) return null;
	let minX = Infinity;
	let minY = Infinity;
	let maxX = -Infinity;
	let maxY = -Infinity;
	for (const [x, y] of stroke) {
		if (x < minX) minX = x;
		if (y < minY) minY = y;
		if (x > maxX) maxX = x;
		if (y > maxY) maxY = y;
	}
	return {
		minX,
		minY,
		width: Math.max(1, maxX - minX),
		height: Math.max(1, maxY - minY),
	};
}

/** Build the SVG `M ... Q ... Z` path that perfect-freehand's
 *  example uses — quadratic-Bézier through midpoints for smooth
 *  rendering of the polygon outline. */
function svgPathFromPolygon(points: Array<[number, number]>): string {
	if (points.length === 0) return "";
	const len = points.length;
	const parts: string[] = [
		`M${points[0][0].toFixed(2)},${points[0][1].toFixed(2)}`,
	];
	parts.push("Q");
	for (let i = 0; i < len; i++) {
		const [x0, y0] = points[i];
		const [x1, y1] = points[(i + 1) % len];
		const mx = (x0 + x1) / 2;
		const my = (y0 + y1) / 2;
		parts.push(
			`${x0.toFixed(2)},${y0.toFixed(2)} ${mx.toFixed(2)},${my.toFixed(2)}`,
		);
	}
	parts.push("Z");
	return parts.join(" ");
}

/** Find the font size that lands the dragged corner closest to
 *  the cursor. Ternary search over `[FONT_MIN, FONT_MAX]`, asking
 *  pretext for the actual measured bounds at each candidate — no
 *  linear-model assumption, just delegate measurement to pretext.
 *
 *  Distance(F) is unimodal in F: both width and height grow
 *  monotonically with font size, so the dragged corner sweeps
 *  along a (mostly straight) path radiating from the anchor, and
 *  its distance to a fixed cursor point has exactly one minimum.
 *
 *  Cost: ~25 pretext measurements per call to converge to <0.5px
 *  precision; pretext is canvas-backed and microsecond-cheap, so
 *  this is comfortably fast for a 60Hz drag.
 *
 *  Caller is responsible for clamping the result and feeding it
 *  into `setOcifNodeFontSize`. */
export function solveFontSizeForCornerTarget(input: {
	text: string;
	textStyle?: TextStyleExt;
	signX: 1 | -1;
	signY: 1 | -1;
	anchorX: number;
	anchorY: number;
	cursorX: number;
	cursorY: number;
}): number {
	const distanceAtFont = (F: number): number => {
		const b = measureTextNodeBounds(
			input.text,
			F,
			input.textStyle?.font_family,
			input.textStyle?.bold,
			input.textStyle?.italic,
		);
		const cornerX = input.anchorX + input.signX * b.width;
		const cornerY = input.anchorY + input.signY * b.height;
		return Math.hypot(cornerX - input.cursorX, cornerY - input.cursorY);
	};
	let lo = FONT_MIN;
	let hi = FONT_MAX;
	// 32 ternary-search rounds is plenty for [8..200] → <0.01px.
	for (let i = 0; i < 32 && hi - lo > 0.5; i++) {
		const m1 = lo + (hi - lo) / 3;
		const m2 = hi - (hi - lo) / 3;
		if (distanceAtFont(m1) < distanceAtFont(m2)) {
			hi = m2;
		} else {
			lo = m1;
		}
	}
	return (lo + hi) / 2;
}

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
	const lineHeight = fontSizePx * LINE_HEIGHT_MULTIPLIER;
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
	// `whiteSpace: pre-wrap` is the textarea-like mode — pretext
	// preserves `\n` as hard line breaks and keeps spaces visible.
	// The default `normal` mode collapses whitespace, which would
	// merge multi-line text back into a single line.
	const prepared = prepareWithSegments(text, font, {
		whiteSpace: "pre-wrap",
	});
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
	path?: PathExt;
	arrow?: ArrowExt;
	edge?: EdgeExt;
	text_style?: TextStyleExt;
	/** `@x11web/window` ext — present iff this node is a live
	 *  window streamed from a sidecar. Title / focus / wm_state /
	 *  pixels come from the sidecar-driven `WindowList` joined
	 *  by `window_id`; the OcifNode owns position / z / size. */
	window?: WindowExt;
	/** Reference to a resource id in `WorkspaceDoc.resources`. The
	 *  renderer pulls the first representation off the resource
	 *  and displays it. */
	resource?: string;
	/** Derived: resolved text content from the first
	 *  representation on `resource`. Computed by `getOcifNodes` —
	 *  not stored in the doc. */
	text?: string;
	/** Derived: mime type of `resource`'s first representation
	 *  (`text/plain` by default; `text/markdown` for markdown
	 *  notes). Lets the renderer dispatch text-only nodes between
	 *  `OcifText` and `OcifMarkdown` without an extra schema flag.
	 *  Computed by `getOcifNodes` — not stored in the doc. */
	text_mime_type?: string;
}

/** `@x11web/window` — links an OcifNode to a live window streamed
 *  from a sidecar. The node carries canvas-side state (position /
 *  z / size); this ext only carries identity. */
export interface WindowExt {
	window_id: string;
	sidecar_id: string;
}

/** `@ocif/rect` — all properties optional per spec. */
export interface RectExt {
	stroke_width?: number;
	stroke_color?: string;
	fill_color?: string;
}

/** `@ocif/path` — raw freehand input samples. `points` is a flat
 *  list of `[x, y, pressure, x, y, pressure, ...]` triples in
 *  node-local coords (origin = the first sampled canvas point).
 *  The renderer runs perfect-freehand on these at draw time;
 *  we never serialize the smoothed SVG path into the doc. */
export interface PathExt {
	points: number[];
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
	for (const fn of listeners.get(workspaceId) ?? []) {
		fn();
	}
}

/** Notify React subscribers synchronously; batch outbound sync
 *  messages onto the next animation frame.
 *
 *  Synchronous notify is essential for controlled `<textarea>`s:
 *  the doc updates inside `onChange`, and we need React to see the
 *  new value in the same tick so the controlled `value` prop
 *  matches the DOM. Otherwise an unrelated re-render between the
 *  keystroke and the next RAF would reconcile the textarea back
 *  to its stale prop value and reset the cursor to the end.
 *
 *  Outbound `ship` (Automerge sync messages over the control DC)
 *  stays RAF-batched. That's where the bandwidth win lives — a
 *  high-frequency drag (pen draw, text edit) sends one bundled
 *  sync message per RAF tick instead of one per keystroke.
 *  Pattern modeled on Automerge Repo's `DocSynchronizer`, which
 *  throttles its `#syncWithPeers` to 100 ms; RAF (~16 ms) is fine
 *  for our local-network case. */
const dirtyShipWorkspaces = new Set<string>();
let rafScheduled = false;

function scheduleFlush(workspaceId: string) {
	notify(workspaceId);
	dirtyShipWorkspaces.add(workspaceId);
	if (rafScheduled) return;
	rafScheduled = true;
	requestAnimationFrame(flushShip);
}

function flushShip() {
	rafScheduled = false;
	const ids = [...dirtyShipWorkspaces];
	dirtyShipWorkspaces.clear();
	for (const id of ids) {
		const entry = docs.get(id);
		if (!entry) continue;
		ship(id, drainOutbound(entry));
	}
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

/** Apply an inbound sync message. Updates the doc + per-peer
 *  sync state synchronously, notifies React subscribers in the
 *  same tick, and queues any reply messages for shipping on the
 *  next animation frame. The caller doesn't need to forward
 *  replies — the scheduler does it. */
export function applyInbound(workspaceId: string, message: Uint8Array): void {
	const entry = ensure(workspaceId);
	const [newDoc, newState] = Automerge.receiveSyncMessage(
		entry.doc,
		entry.syncState,
		message,
	);
	entry.doc = newDoc;
	entry.syncState = newState;
	scheduleFlush(workspaceId);
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
		// `RTCDataChannel.send` is overloaded across the Uint8Array
		// generic variants; the encoder hands back a fresh
		// `ArrayBuffer`-backed Uint8Array so this is safe.
		const buf = encodeWorkspaceSync(workspaceId, m) as Uint8Array<ArrayBuffer>;
		controlDc.send(buf);
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
	scheduleFlush(workspaceId);
}

/** Insert a window-node — the canvas-side handle for a live window
 *  streamed from a sidecar. Node id IS the underlying `windowId` so
 *  edges can target it directly. Used by drag-attach from the dock
 *  picker; backend-driven attaches arrive via inbound sync. */
export function attachWindowNode(
	workspaceId: string,
	windowId: string,
	sidecarId: string,
	x: number,
	y: number,
	width: number,
	height: number,
) {
	const entry = ensure(workspaceId);
	if ((entry.doc.nodes ?? {})[windowId]) return;
	let maxZ = 0;
	for (const n of Object.values(entry.doc.nodes ?? {})) {
		if (n.z > maxZ) maxZ = n.z;
	}
	insertOcifNode(workspaceId, windowId, {
		x,
		y,
		z: maxZ + 1,
		width,
		height,
		window: { window_id: windowId, sidecar_id: sidecarId },
	});
}

/** Remove the window-node for `windowId` (drag-off-canvas, or any
 *  other user-driven detach). Cascades through `deleteOcifNode`'s
 *  edge-cleanup logic so edges that referenced the window are
 *  dropped rather than left dangling. */
export function detachWindowNode(workspaceId: string, windowId: string) {
	deleteOcifNode(workspaceId, windowId);
}

/** Window ids currently rendered on this workspace's canvas — every
 *  node id whose `OcifNode` carries the `@x11web/window` extension.
 *  Returns a fresh `Set` per call; pair with `subscribe` to know
 *  when to re-read. */
export function getWindowNodeIds(workspaceId: string): Set<string> {
	const out = new Set<string>();
	const entry = docs.get(workspaceId);
	if (!entry) return out;
	for (const [id, node] of Object.entries(entry.doc.nodes ?? {})) {
		if (node.window) out.add(id);
	}
	return out;
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
			// `points` is an Automerge list proxy; copy to a plain
			// array so consumers can `useMemo` on it and React
			// equality checks behave predictably.
			path: v.path
				? {
						points: v.path.points ? Array.from(v.path.points) : [],
						stroke_width: v.path.stroke_width,
						stroke_color: v.path.stroke_color,
						fill_color: v.path.fill_color,
					}
				: undefined,
			arrow: v.arrow ? { ...v.arrow } : undefined,
			edge: v.edge ? { ...v.edge } : undefined,
			text_style: v.text_style ? { ...v.text_style } : undefined,
			window: v.window ? { ...v.window } : undefined,
			resource: v.resource,
			text: resolveNodeText(entry.doc, v),
			text_mime_type: resolveNodeMimeType(entry.doc, v),
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
	const mimeType = node.text_mime_type ?? "text/plain";
	// Plain-text-only nodes auto-fit their bounds to the measured
	// text; boxes / arrows / paths / windows / markdown notes keep
	// the caller's geometry (markdown notes are user-resizable and
	// scroll their content).
	const isAutoFitText =
		!node.rect &&
		!node.arrow &&
		!node.path &&
		!node.window &&
		mimeType === "text/plain";
	const measured = isAutoFitText
		? measureTextNodeBounds(
				node.text ?? "",
				node.text_style?.font_size_px ?? 14,
				node.text_style?.font_family,
				node.text_style?.bold,
				node.text_style?.italic,
			)
		: null;
	// Window nodes don't carry text content; skip the resource
	// allocation so we don't leak phantom text/plain resources for
	// every attached window.
	const allocateResource = !node.window;
	entry.doc = Automerge.change(entry.doc, (d) => {
		if (!d.nodes) d.nodes = {};
		if (!d.resources) d.resources = {};
		const resourceId = allocateResource
			? (node.resource ?? crypto.randomUUID())
			: undefined;
		if (resourceId && !d.resources[resourceId]) {
			// Seed the representation with empty content, then splice
			// the seed text in. This makes `content` a text CRDT from
			// the first character — concurrent edits in two tabs merge
			// at character level instead of last-writer-winning the
			// whole string.
			d.resources[resourceId] = {
				representations: [{ mime_type: mimeType, content: "" }],
			};
			const seed = node.text ?? "";
			if (seed.length > 0) {
				Automerge.splice(
					d as Automerge.Doc<WorkspaceDoc>,
					["resources", resourceId, "representations", 0, "content"],
					0,
					0,
					seed,
				);
			}
		}
		d.nodes[id] = {
			x: node.x,
			y: node.y,
			z: node.z,
			width: measured?.width ?? node.width,
			height: measured?.height ?? node.height,
			...(resourceId ? { resource: resourceId } : {}),
			...(node.rect ? { rect: { ...node.rect } } : {}),
			...(node.path
				? {
						// Build the PathExt with only defined fields —
						// Automerge throws on `undefined` assignments to
						// optional scalars.
						path: {
							points: node.path.points ? [...node.path.points] : [],
							...(node.path.stroke_width !== undefined
								? { stroke_width: node.path.stroke_width }
								: {}),
							...(node.path.stroke_color !== undefined
								? { stroke_color: node.path.stroke_color }
								: {}),
							...(node.path.fill_color !== undefined
								? { fill_color: node.path.fill_color }
								: {}),
						},
					}
				: {}),
			...(node.arrow ? { arrow: { ...node.arrow } } : {}),
			...(node.edge ? { edge: { ...node.edge } } : {}),
			...(node.text_style ? { text_style: { ...node.text_style } } : {}),
			...(node.window ? { window: { ...node.window } } : {}),
		};
	});
	scheduleFlush(workspaceId);
}

/** Bump a node's `z` so it renders on top of every other node in
 *  the workspace. Used by click-to-focus / dock reactivation for
 *  window-nodes; also handy for "raise this shape" gestures on
 *  boxes / text in the future. Collaborative — every peer sees
 *  the new stacking order. */
export function raiseOcifNode(workspaceId: string, id: string) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	let maxZ = 0;
	for (const n of Object.values(entry.doc.nodes ?? {})) {
		if (n.z > maxZ) maxZ = n.z;
	}
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		n.z = maxZ + 1;
	});
	scheduleFlush(workspaceId);
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
	scheduleFlush(workspaceId);
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
	scheduleFlush(workspaceId);
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
	scheduleFlush(workspaceId);
}

/** Set a node's text content. Routes through the OCIF resource
 *  model: a `text/plain` representation lives on a resource that
 *  the node references via `resource`. Creates the resource the
 *  first time the node has text; updates the existing
 *  representation thereafter.
 *
 *  Text-only nodes (no `rect` / `arrow`) auto-resize to fit the
 *  measured text; boxes keep their user-set bounds. */
export function setOcifNodeText(workspaceId: string, id: string, text: string) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		writeText(d, n, text);
		if (!n.rect && !n.arrow && isPlainTextNode(d, n)) {
			const bounds = boundsForNode(n, text);
			n.width = bounds.width;
			n.height = bounds.height;
		}
	});
	scheduleFlush(workspaceId);
}

/** Set a text-only node's font size (in `@ocif/textstyle.font_size_px`).
 *  Re-measures and updates bounds so the rendered text fits.
 *
 *  `anchor` (optional) keeps a specific corner of the node fixed
 *  in canvas space across the resize. `gripX` / `gripY` are the
 *  signs of the GRABBED corner (the one moving with the cursor):
 *  the anchor is the corner with opposite signs, which we want to
 *  pin at `(anchor.x, anchor.y)`. When omitted, the node's
 *  top-left (`x, y`) is left untouched. */
export function setOcifNodeFontSize(
	workspaceId: string,
	id: string,
	fontSizePx: number,
	anchor?: {
		x: number;
		y: number;
		gripX: 1 | -1;
		gripY: 1 | -1;
	},
) {
	const entry = ensure(workspaceId);
	if (!(entry.doc.nodes ?? {})[id]) return;
	entry.doc = Automerge.change(entry.doc, (d) => {
		const n = d.nodes?.[id];
		if (!n) return;
		if (!n.text_style) n.text_style = {};
		n.text_style.font_size_px = fontSizePx;
		if (!n.rect && !n.arrow && isPlainTextNode(d, n)) {
			const text = readText(d, n);
			const bounds = boundsForNode(n, text);
			n.width = bounds.width;
			n.height = bounds.height;
			if (anchor) {
				// Pin the corner OPPOSITE the grabbed one. If the
				// user grabbed the east edge (gripX > 0) the west
				// edge stays at anchor.x; vice versa for west grip.
				n.x = anchor.gripX > 0 ? anchor.x : anchor.x - bounds.width;
				n.y = anchor.gripY > 0 ? anchor.y : anchor.y - bounds.height;
			}
		}
	});
	scheduleFlush(workspaceId);
}

/** Helper — write a `text/plain` representation through the
 *  resource layer. Mutates `d` directly inside an
 *  `Automerge.change` callback.
 *
 *  Updates to an existing representation route through
 *  `Automerge.updateText`: it diffs the current value against
 *  `text` and emits character-level splice ops, so concurrent
 *  edits in two tabs merge instead of last-writer-winning the
 *  whole string. New representations seed `content: ""` and
 *  splice `text` in afterwards so the field starts as a text
 *  CRDT from the first character.
 *
 *  `d` is the mutable `Doc` proxy supplied by `Automerge.change`,
 *  not a hydrated `WorkspaceDoc` value — the type annotation is
 *  loose here because `Automerge.updateText` / `Automerge.splice`
 *  expect the proxy and not a typed view. */
function writeText(d: WorkspaceDoc, n: OcifNode, text: string): void {
	if (!d.resources) d.resources = {};
	let resourceId = n.resource;
	if (!resourceId) {
		resourceId = crypto.randomUUID();
		n.resource = resourceId;
		d.resources[resourceId] = {
			representations: [{ mime_type: "text/plain", content: "" }],
		};
		Automerge.splice(
			d as Automerge.Doc<WorkspaceDoc>,
			["resources", resourceId, "representations", 0, "content"],
			0,
			0,
			text,
		);
		return;
	}
	const r = d.resources[resourceId];
	if (!r) {
		d.resources[resourceId] = {
			representations: [{ mime_type: "text/plain", content: "" }],
		};
		Automerge.splice(
			d as Automerge.Doc<WorkspaceDoc>,
			["resources", resourceId, "representations", 0, "content"],
			0,
			0,
			text,
		);
		return;
	}
	// Each resource carries one representation; update its content
	// regardless of mime type so the same code path handles
	// `text/plain` and `text/markdown` (and any future mime types).
	if (r.representations.length > 0) {
		Automerge.updateText(
			d as Automerge.Doc<WorkspaceDoc>,
			["resources", resourceId, "representations", 0, "content"],
			text,
		);
	} else {
		r.representations.push({ mime_type: "text/plain", content: "" });
		Automerge.splice(
			d as Automerge.Doc<WorkspaceDoc>,
			["resources", resourceId, "representations", 0, "content"],
			0,
			0,
			text,
		);
	}
}

/** Helper — read the active representation's content for a node
 *  from the shared resources map. Empty string when missing. */
function readText(d: WorkspaceDoc, n: OcifNode): string {
	if (!n.resource) return "";
	const r = d.resources?.[n.resource];
	const rep = r?.representations?.[0];
	return rep?.content ?? "";
}

/** Helper — true when `n` carries plain text content (vs. markdown
 *  or another mime type). Used to gate auto-fit-to-text behavior:
 *  plain text nodes auto-resize so the bounds always hug the text;
 *  markdown notes have caller-set bounds and an internal scrollbar
 *  when content overflows. */
function isPlainTextNode(d: WorkspaceDoc, n: OcifNode): boolean {
	if (!n.resource) return true;
	const r = d.resources?.[n.resource];
	const rep = r?.representations?.[0];
	return (rep?.mime_type ?? "text/plain") === "text/plain";
}

/** Helper — measure node bounds from current text + textstyle. */
function boundsForNode(
	n: OcifNode,
	text: string,
): { width: number; height: number } {
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
 *  Returns the first representation's content (each resource carries
 *  one representation today — `text/plain` for plain text nodes,
 *  `text/markdown` for markdown notes). Empty string when there's
 *  no resource or the resource is empty. */
export function resolveNodeText(
	doc: Automerge.Doc<WorkspaceDoc>,
	node: OcifNode,
): string {
	if (!node.resource) return "";
	const r = doc.resources?.[node.resource];
	const rep = r?.representations?.[0];
	return rep?.content ?? "";
}

/** Mime type of a node's referenced resource (the first
 *  representation). `"text/plain"` when the resource is missing
 *  or doesn't declare a mime type — same default the spec
 *  applies. Lets the renderer pick `OcifText` vs `OcifMarkdown`
 *  for text-only nodes. */
export function resolveNodeMimeType(
	doc: Automerge.Doc<WorkspaceDoc>,
	node: OcifNode,
): string {
	if (!node.resource) return "text/plain";
	const r = doc.resources?.[node.resource];
	const rep = r?.representations?.[0];
	return rep?.mime_type ?? "text/plain";
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
	scheduleFlush(workspaceId);
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
				if (n.edge.start === undefined && n.edge.end === undefined) {
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
	scheduleFlush(workspaceId);
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
	scheduleFlush(workspaceId);
}
