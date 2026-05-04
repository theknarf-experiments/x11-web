import { type Collection, createCollection } from "@tanstack/db";
import type {
	MenuItem,
	ProcessInfo,
	SidecarInfo,
	WindowDescriptor,
	WindowWmState,
} from "./types";
import type {
	ArrowExt,
	EdgeExt,
	OcifNode,
	PathExt,
	RectExt,
	TextStyleExt,
	WindowExt,
} from "./workspaceSync";

interface SyncApi<T extends object> {
	begin(options?: { immediate?: boolean }): void;
	write(
		message:
			| { type: "insert" | "update"; value: T }
			| { type: "delete"; key: string },
	): void;
	commit(): void;
	truncate(): void;
	markReady(): void;
}

function makeCollection<T extends object>(opts: {
	id: string;
	getKey: (row: T) => string;
}): {
	collection: Collection<T, string>;
	sync: { current: SyncApi<T> | null };
} {
	const sync: { current: SyncApi<T> | null } = { current: null };
	const collection = createCollection<T, string>({
		id: opts.id,
		getKey: opts.getKey,
		startSync: true,
		sync: {
			rowUpdateMode: "full",
			sync: (params) => {
				sync.current = params as unknown as SyncApi<T>;
				params.markReady();
			},
		},
	});
	return { collection, sync };
}

// ---------- sidecars ----------

const sidecars = makeCollection<SidecarInfo>({
	id: "sidecars",
	getKey: (s) => s.id,
});
export const sidecarsCollection = sidecars.collection;

export function replaceSidecars(rows: SidecarInfo[]) {
	const api = sidecars.sync.current;
	if (!api) return;
	api.begin();
	api.truncate();
	for (const row of rows) {
		api.write({ type: "insert", value: row });
	}
	api.commit();
}

// ---------- processes ----------

export interface ProcessRow extends ProcessInfo {
	sidecar_id: string;
}

const processes = makeCollection<ProcessRow>({
	id: "processes",
	getKey: (p) => `${p.sidecar_id}:${p.pid}`,
});
export const processesCollection = processes.collection;

/** Replace the process list for a single sidecar; leaves other sidecars' rows untouched. */
export function replaceSidecarProcesses(
	sidecarId: string,
	list: ProcessInfo[],
) {
	const api = processes.sync.current;
	if (!api) return;
	const incoming = new Set(list.map((p) => `${sidecarId}:${p.pid}`));
	api.begin();
	for (const [key, row] of processesCollection.state) {
		if (row.sidecar_id === sidecarId && !incoming.has(key)) {
			api.write({ type: "delete", key });
		}
	}
	for (const proc of list) {
		api.write({
			type: "insert",
			value: { ...proc, sidecar_id: sidecarId },
		});
	}
	api.commit();
}

// ---------- windows ----------

/** Per-window sidecar-driven state. Canvas position / z / size for
 *  top-level windows lives on the matching `OcifNode` in the
 *  workspace doc — this row holds everything the X server / sidecar
 *  is authoritative for: title, focus, wm_state, menu, etc. Popups
 *  (`overrideRedirect: true`) keep their `x` / `y` here since the X
 *  server places them and they don't get an OcifNode. */
export interface WindowRow {
	windowId: string;
	sidecarId: string;
	pid: number;
	command: string;
	/** Sidecar-reported geometry. For top-level windows this is the
	 *  X server's idea; the canvas reads w/h from the OcifNode (kept
	 *  in lockstep by the backend on `WindowConfigured`). */
	width: number;
	height: number;
	borderWidth: number;
	borderPixel: number;
	overrideRedirect: boolean;
	resizable: boolean;
	/** Popups only — X server placement. Top-level windows ignore
	 *  these and read position from the matching OcifNode. */
	x: number;
	y: number;
	title: string;
	wmState: WindowWmState;
	color: string;
	focused: boolean;
	menu: MenuItem[];
	/** Local-only memory of the node's pre-maximize geometry, so
	 *  Restore can put the window back where it was. Per-tab; not
	 *  synced via the doc. */
	savedPosition?: { x: number; y: number };
}

const windows = makeCollection<WindowRow>({
	id: "windows",
	getKey: (w) => w.windowId,
});
export const windowsCollection = windows.collection;

/** Seed values for a window the first time we see it (computed by the caller
 *  so app-level concerns like palette and title fallback can stay out of the
 *  data layer). */
export interface NewWindowSeed {
	color: string;
	wmState: WindowWmState;
	title: string;
}

/** Apply an authoritative `WindowList` snapshot. Inserts new rows with seed
 *  values; updates retained rows preserving dynamic UI state (title, focus,
 *  menu, etc.); deletes rows that disappeared. Position / z / size for
 *  top-level windows lives on the matching `OcifNode` in the workspace doc;
 *  this row only carries sidecar-driven state. Popups (override_redirect)
 *  keep `(x, y)` here since the X server places them. */
export function applyWindowList(
	descriptors: WindowDescriptor[],
	seedNew: (descriptor: WindowDescriptor) => NewWindowSeed,
) {
	const api = windows.sync.current;
	if (!api) return;
	const current = windowsCollection.state;
	const incoming = new Set(descriptors.map((d) => d.window_id));
	api.begin();
	for (const wid of current.keys()) {
		if (!incoming.has(wid)) {
			api.write({ type: "delete", key: wid });
		}
	}
	descriptors.forEach((d) => {
		const existing = current.get(d.window_id);
		if (existing) {
			const x = d.override_redirect ? d.x : existing.x;
			const y = d.override_redirect ? d.y : existing.y;
			api.write({
				type: "update",
				value: {
					...existing,
					sidecarId: d.sidecar_id,
					pid: d.pid,
					command: d.command,
					width: d.width,
					height: d.height,
					borderWidth: d.border_width,
					borderPixel: d.border_pixel,
					overrideRedirect: d.override_redirect,
					resizable: d.resizable,
					x,
					y,
				},
			});
		} else {
			const seed = seedNew(d);
			api.write({
				type: "insert",
				value: {
					windowId: d.window_id,
					sidecarId: d.sidecar_id,
					pid: d.pid,
					command: d.command,
					width: d.width,
					height: d.height,
					borderWidth: d.border_width,
					borderPixel: d.border_pixel,
					overrideRedirect: d.override_redirect,
					resizable: d.resizable,
					x: d.x,
					y: d.y,
					title: seed.title,
					wmState: seed.wmState,
					color: seed.color,
					focused: false,
					menu: [],
				},
			});
		}
	});
	api.commit();
}

/** Targeted partial update of a single window row. Silently skips windows
 *  that aren't in the collection yet (deltas can race ahead of WindowList). */
export function patchWindow(windowId: string, patch: Partial<WindowRow>) {
	const api = windows.sync.current;
	if (!api) return;
	const existing = windowsCollection.state.get(windowId);
	if (!existing) return;
	api.begin();
	api.write({ type: "update", value: { ...existing, ...patch } });
	api.commit();
}

/** Set the singular focused window; clears `focused=true` on the previously
 *  focused row. Pass `null` to clear focus. */
export function setFocusedWindow(windowId: string | null) {
	const api = windows.sync.current;
	if (!api) return;
	api.begin();
	for (const [wid, row] of windowsCollection.state) {
		const shouldFocus = wid === windowId;
		if (row.focused !== shouldFocus) {
			api.write({ type: "update", value: { ...row, focused: shouldFocus } });
		}
	}
	api.commit();
}

/** Unminimize a window (transition `wmState: "minimized" → "normal"`)
 *  if it's currently minimized. Stacking order — the "raise to top"
 *  effect — lives on the matching `OcifNode.z` and is bumped via
 *  `raiseOcifNode` in `workspaceSync`. */
export function unminimizeWindow(windowId: string) {
	const existing = windowsCollection.state.get(windowId);
	if (!existing || existing.wmState !== "minimized") return;
	patchWindow(windowId, { wmState: "normal" });
}

/** Find every `(sidecarId, pid)` window's id — used by the dock to
 *  bring all of an app's windows to front in one gesture. */
export function windowsForProcess(sidecarId: string, pid: number): string[] {
	return [...windowsCollection.state.values()]
		.filter((w) => w.sidecarId === sidecarId && w.pid === pid)
		.map((w) => w.windowId);
}

// ---------- OCIF nodes (user-drawn shapes, mirror of doc state) ----------

/** A single OCIF node row, projected from the per-workspace
 *  Automerge doc. Source of truth is the doc; this collection is a
 *  one-way reactive mirror. Mutations go through `workspaceSync.*`
 *  helpers — never mutate this collection directly.
 *
 *  Shape extensions (`rect` / `arrow` / future ovals) are kept as
 *  nested objects rather than flattened — fewer fields to maintain
 *  per shape and the row becomes near-isomorphic to `OcifNode`. */
export interface OcifNodeRow {
	id: string;
	workspaceId: string;
	nodeId: string;
	x: number;
	y: number;
	z: number;
	width: number;
	height: number;
	/** Resolved text content from the referenced resource — view
	 *  copy. Source of truth is `WorkspaceDoc.resources[node.resource]
	 *  .representations[mime=text/plain].content`. */
	text: string;
	rect?: RectExt;
	path?: PathExt;
	arrow?: ArrowExt;
	edge?: EdgeExt;
	textStyle?: TextStyleExt;
	window?: WindowExt;
	resourceId?: string;
	/** Mime type of the resource's first representation — drives
	 *  text-only-node dispatch in the renderer (`text/plain` →
	 *  `OcifText`, `text/markdown` → `OcifMarkdown`). */
	textMimeType?: string;
}

const ocifNodes = makeCollection<OcifNodeRow>({
	id: "ocif-nodes",
	getKey: (r) => r.id,
});
export const ocifNodesCollection = ocifNodes.collection;

/** Replace the OCIF nodes for one workspace with a fresh snapshot.
 *  Other workspaces' rows are untouched. Called after every
 *  Automerge mutation / inbound sync that affects this workspace's
 *  doc. Uses `insert` vs `update` correctly — `insert` on an
 *  existing key is a no-op for our collection sync, so movement /
 *  resize updates need `update`. */
export function applyOcifNodesSnapshot(
	workspaceId: string,
	snap: Map<string, OcifNode>,
) {
	const api = ocifNodes.sync.current;
	if (!api) return;
	const current = ocifNodesCollection.state;
	const incomingKeys = new Set(
		[...snap.keys()].map((k) => `${workspaceId}:${k}`),
	);
	api.begin();
	for (const [key, row] of current) {
		if (row.workspaceId === workspaceId && !incomingKeys.has(key)) {
			api.write({ type: "delete", key });
		}
	}
	for (const [nodeId, node] of snap) {
		const id = `${workspaceId}:${nodeId}`;
		const value: OcifNodeRow = {
			id,
			workspaceId,
			nodeId,
			x: node.x,
			y: node.y,
			z: node.z,
			width: node.width,
			height: node.height,
			text: node.text ?? "",
			rect: node.rect ? { ...node.rect } : undefined,
			path: node.path ? { ...node.path } : undefined,
			arrow: node.arrow ? { ...node.arrow } : undefined,
			edge: node.edge ? { ...node.edge } : undefined,
			textStyle: node.text_style ? { ...node.text_style } : undefined,
			window: node.window ? { ...node.window } : undefined,
			resourceId: node.resource,
			textMimeType: node.text_mime_type,
		};
		api.write({
			type: current.has(id) ? "update" : "insert",
			value,
		});
	}
	api.commit();
}
