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

/** Drop our local doc + sync state for a workspace. Currently
 *  unused; we'll call this if a frontend ever switches workspaces
 *  mid-session. */
export function forget(workspaceId: string) {
	docs.delete(workspaceId);
	listeners.delete(workspaceId);
}
