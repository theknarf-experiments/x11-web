import { type Collection, createCollection } from "@tanstack/db";
import type {
	MenuItem,
	ProcessInfo,
	SidecarInfo,
	WindowDescriptor,
	WindowWmState,
} from "./types";

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

export interface WindowRow {
	windowId: string;
	sidecarId: string;
	pid: number;
	command: string;
	width: number;
	height: number;
	borderWidth: number;
	borderPixel: number;
	overrideRedirect: boolean;
	resizable: boolean;
	x: number;
	y: number;
	title: string;
	cursor: string;
	wmState: WindowWmState;
	color: string;
	focused: boolean;
	stackingOrder: number;
	menu: MenuItem[];
	savedPosition?: { x: number; y: number };
}

const windows = makeCollection<WindowRow>({
	id: "windows",
	getKey: (w) => w.windowId,
});
export const windowsCollection = windows.collection;

/** Seed values for a window the first time we see it (computed by the caller
 *  so app-level concerns like cascading position and palette can stay out of
 *  the data layer). */
export interface NewWindowSeed {
	x: number;
	y: number;
	color: string;
	cursor: string;
	wmState: WindowWmState;
	title: string;
}

/** Apply an authoritative `WindowList` snapshot. Inserts new rows with seed
 *  values; updates retained rows preserving dynamic UI state (title, focus,
 *  cursor, etc.); deletes rows that disappeared. */
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
	descriptors.forEach((d, i) => {
		const stackingOrder = i + 1;
		const existing = current.get(d.window_id);
		if (existing) {
			// Popup positions come from the X server (override_redirect=true);
			// regular top-level windows track position locally and via the
			// cross-frontend `UpdateWindowPosition` mirror.
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
					stackingOrder,
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
					x: seed.x,
					y: seed.y,
					title: seed.title,
					cursor: seed.cursor,
					wmState: seed.wmState,
					color: seed.color,
					focused: false,
					stackingOrder,
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

/** Bump `stackingOrder` so this window renders on top of everything currently
 *  in the collection. Used by click-to-focus and dock reactivation. Optionally
 *  also unminimizes the window. */
export function raiseWindow(
	windowId: string,
	opts: { unminimize?: boolean } = {},
) {
	const api = windows.sync.current;
	if (!api) return;
	let max = 0;
	for (const row of windowsCollection.state.values()) {
		if (row.stackingOrder > max) max = row.stackingOrder;
	}
	const existing = windowsCollection.state.get(windowId);
	if (!existing) return;
	api.begin();
	api.write({
		type: "update",
		value: {
			...existing,
			stackingOrder: max + 1,
			wmState:
				opts.unminimize && existing.wmState === "minimized"
					? "normal"
					: existing.wmState,
		},
	});
	api.commit();
}

/** Bring all windows belonging to `(sidecarId, pid)` to front and unminimize. */
export function raiseProcess(sidecarId: string, pid: number) {
	const api = windows.sync.current;
	if (!api) return;
	let max = 0;
	for (const row of windowsCollection.state.values()) {
		if (row.stackingOrder > max) max = row.stackingOrder;
	}
	const matches = [...windowsCollection.state.values()].filter(
		(w) => w.sidecarId === sidecarId && w.pid === pid,
	);
	if (matches.length === 0) return;
	api.begin();
	let cursor = max;
	for (const row of matches) {
		cursor += 1;
		api.write({
			type: "update",
			value: {
				...row,
				stackingOrder: cursor,
				wmState: row.wmState === "minimized" ? "normal" : row.wmState,
			},
		});
	}
	api.commit();
}
