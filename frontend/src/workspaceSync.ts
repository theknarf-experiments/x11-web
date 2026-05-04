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

/** Read-only view of the current hydrated doc. Useful for slice
 *  1b verification (read `name`) and React-side derivations. */
export function snapshot(workspaceId: string): WorkspaceDoc | null {
	const entry = docs.get(workspaceId);
	return entry ? (entry.doc as WorkspaceDoc) : null;
}

/** Drop our local doc + sync state for a workspace. Currently
 *  unused; we'll call this if a frontend ever switches workspaces
 *  mid-session. */
export function forget(workspaceId: string) {
	docs.delete(workspaceId);
}
