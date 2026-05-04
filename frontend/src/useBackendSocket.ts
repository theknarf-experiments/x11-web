import {
	useCallback,
	useEffect,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import {
	applyWindowList,
	type NewWindowSeed,
	patchWindow,
	replaceSidecarProcesses,
	replaceSidecars,
	setFocusedWindow,
	windowsCollection,
} from "./db";
import { Reassembler } from "./rtcReassembler";
import { decodeFrame, encodeWorkspaceSync } from "./rtcWire";
import {
	applyInbound,
	getName as getWorkspaceName,
	setControlChannel,
	setName as setWorkspaceName,
	subscribe as subscribeWorkspace,
} from "./workspaceSync";
import type {
	BackendToFrontend,
	FrontendToBackend,
	WindowDescriptor,
	WindowUpdate,
	Workspace,
} from "./types";
import { colorForWindowId } from "./windowColors";

// Resolve order: ?ws=... query param > VITE_WS_URL build-time env > default.
// The query-param branch lets parallel e2e workers share a single built
// bundle but route each browser page to its own backend.
const WS_URL = (() => {
	if (typeof window !== "undefined") {
		const fromQuery = new URLSearchParams(window.location.search).get("ws");
		if (fromQuery) return fromQuery;
	}
	return import.meta.env.VITE_WS_URL || "ws://localhost:3001/ws/frontend";
})();

export type WindowUpdateCallback = (update: WindowUpdate) => void;
export type BellCallback = (percent: number) => void;
/** Raw bytes from the WebRTC DataChannel — caller decodes (Cap'n Proto). */
export type DataChannelMessageCallback = (data: Uint8Array) => void;

/**
 * Per-event diagnostic surfaced from the backend / WebSocket layer.
 * Rendered by `<DiagnosticsPanel>` so the user has *some* visibility
 * into errors that previously vanished into the void (sidecar errors,
 * socket errors).
 */
export interface Diagnostic {
	id: string;
	level: "info" | "warn" | "error";
	source: "ws" | "command" | "sidecar";
	message: string;
	timestamp: number;
}

const MAX_DIAGNOSTICS = 100;
let diagnosticCounter = 0;
function nextDiagnosticId() {
	return `diag-${++diagnosticCounter}-${Date.now()}`;
}

let spawnCounter = 0;

function seedForDescriptor(
	d: WindowDescriptor,
	send: (msg: FrontendToBackend) => void,
): NewWindowSeed {
	let x: number;
	let y: number;
	if (d.placed || d.override_redirect) {
		// Backend gave us an authoritative position (X11 server for popups,
		// cross-frontend tracked for top-level windows).
		x = d.x;
		y = d.y;
	} else {
		// First time *any* frontend has seen this top-level window — pick a
		// cascading position and broadcast it so other tabs converge.
		const idx = spawnCounter++;
		const offset = idx * 30;
		x = window.innerWidth / 4 + offset;
		y = window.innerHeight / 4 + offset;
		send({
			type: "UpdateWindowPosition",
			window_id: d.window_id,
			x,
			y,
		});
	}
	return {
		x,
		y,
		color: d.override_redirect ? "transparent" : colorForWindowId(d.window_id),
		title: d.command || `PID ${d.pid}`,
		cursor: "default",
		wmState: "normal",
	};
}

/** Apply a `WindowUpdate` to the windows collection where it represents a
 *  persistent UI-state change. Returns true if the hook handled the update
 *  (the App can still observe it via the `onWindowUpdate` callback for
 *  side-effect kinds like `PutImage` and animated/bitmap cursors). */
function applyWindowUpdate(update: WindowUpdate) {
	switch (update.kind) {
		case "TitleChanged":
			patchWindow(update.window_id, { title: update.title });
			break;
		case "CursorChanged":
			patchWindow(update.window_id, { cursor: update.cursor });
			break;
		case "Focused":
			setFocusedWindow(update.window_id);
			break;
		case "PositionChanged":
			patchWindow(update.window_id, { x: update.x, y: update.y });
			break;
		case "MenuStructure":
			patchWindow(update.window_id, { menu: update.menu });
			break;
		case "StateChanged":
			if (update.state === "maximized" || update.state === "fullscreen") {
				// Save the current position so Restore can put the window
				// back where it was before the WM transition.
				const existing = windowsCollection.state.get(update.window_id);
				if (existing) {
					patchWindow(update.window_id, {
						wmState: update.state,
						savedPosition: { x: existing.x, y: existing.y },
					});
				}
			} else {
				patchWindow(update.window_id, { wmState: update.state });
			}
			break;
		// PutImage / CursorBitmap / CursorAnimated are handled by App.tsx
		// (renderers and async cursor decoding live there).
	}
}

export function useBackendSocket() {
	const wsRef = useRef<WebSocket | null>(null);
	const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
	const disposed = useRef(false);
	const windowUpdateCallbackRef = useRef<WindowUpdateCallback | null>(null);
	const bellCallbackRef = useRef<BellCallback | null>(null);
	const dcMessageCallbackRef = useRef<DataChannelMessageCallback | null>(null);
	const peerConnectionRef = useRef<RTCPeerConnection | null>(null);
	const dataChannelRef = useRef<RTCDataChannel | null>(null);
	const controlChannelRef = useRef<RTCDataChannel | null>(null);
	const sendRef = useRef<(msg: FrontendToBackend) => void>(() => {});
	const [connected, setConnected] = useState(false);
	const [activeWorkspace, setActiveWorkspace] = useState<Workspace | null>(null);
	/// Window IDs attached to `activeWorkspace`'s canvas. The backend
	/// pushes a fresh snapshot via `AttachedWindows` whenever the set
	/// changes; we ignore snapshots for other workspaces.
	const [attachedWindowIds, setAttachedWindowIds] = useState<Set<string>>(
		() => new Set(),
	);
	const activeWorkspaceIdRef = useRef<string | null>(null);
	const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);

	const pushDiagnostic = useCallback(
		(d: Omit<Diagnostic, "id" | "timestamp">) => {
			setDiagnostics((prev) => {
				const next = [
					...prev,
					{ ...d, id: nextDiagnosticId(), timestamp: Date.now() },
				];
				// Keep only the most recent N entries.
				return next.length > MAX_DIAGNOSTICS
					? next.slice(next.length - MAX_DIAGNOSTICS)
					: next;
			});
		},
		[],
	);

	const dismissDiagnostic = useCallback((id: string) => {
		setDiagnostics((prev) => prev.filter((d) => d.id !== id));
	}, []);

	const clearDiagnostics = useCallback(() => {
		setDiagnostics([]);
	}, []);

	useEffect(() => {
		disposed.current = false;

		function startRtc() {
			if (peerConnectionRef.current) return;
			// Host-only ICE config. STUN/TURN can be added once we
			// deploy beyond LAN.
			const pc = new RTCPeerConnection({ iceServers: [] });
			peerConnectionRef.current = pc;

			pc.onicecandidate = (e) => {
				if (e.candidate) {
					sendRef.current({
						type: "RtcIceCandidate",
						candidate: e.candidate.candidate,
						sdp_mid: e.candidate.sdpMid,
						sdp_mline_index: e.candidate.sdpMLineIndex,
					});
				}
			};

			// Unordered + unreliable: pixel frames are independent
			// snapshots. Drop in transit is fine — the next frame
			// supersedes — and out-of-order arrival is handled by the
			// reassembler keying off the chunk header's `msg_id`.
			const dc = pc.createDataChannel("putimage", {
				ordered: false,
				maxRetransmits: 0,
			});
			dataChannelRef.current = dc;
			dc.binaryType = "arraybuffer";
			const reassembler = new Reassembler();
			dc.onopen = () =>
				pushDiagnostic({
					level: "info",
					source: "ws",
					message: "DataChannel open",
				});
			dc.onclose = () =>
				pushDiagnostic({
					level: "warn",
					source: "ws",
					message: "DataChannel closed",
				});
			dc.onmessage = (e) => {
				const chunk = new Uint8Array(e.data as ArrayBuffer);
				const payload = reassembler.onChunk(chunk);
				if (payload) dcMessageCallbackRef.current?.(payload);
			};

			// Ordered + reliable: workspace Automerge sync messages
			// and any other loss-intolerant control traffic. No
			// chunking — messages stay under the SCTP single-message
			// limit (sync messages are a few hundred bytes to a few
			// KB at our scale).
			const controlDc = pc.createDataChannel("control");
			controlChannelRef.current = controlDc;
			controlDc.binaryType = "arraybuffer";
			controlDc.onopen = () => {
				setControlChannel(controlDc);
				pushDiagnostic({
					level: "info",
					source: "ws",
					message: "control DC open",
				});
			};
			controlDc.onclose = () => {
				setControlChannel(null);
				pushDiagnostic({
					level: "warn",
					source: "ws",
					message: "control DC closed",
				});
			};
			controlDc.onmessage = (e) => {
				const bytes = new Uint8Array(e.data as ArrayBuffer);
				const frame = decodeFrame(bytes);
				if (frame?.kind !== "workspaceSync") return;
				const replies = applyInbound(frame.workspaceId, frame.message);
				for (const reply of replies) {
					controlDc.send(encodeWorkspaceSync(frame.workspaceId, reply));
				}
			};

			pc.createOffer()
				.then((offer) => pc.setLocalDescription(offer).then(() => offer))
				.then((offer) => {
					sendRef.current({ type: "RtcOffer", sdp: offer.sdp ?? "" });
				})
				.catch((err) => {
					pushDiagnostic({
						level: "error",
						source: "ws",
						message: `RTC offer failed: ${err}`,
					});
				});
		}

		function connect() {
			if (disposed.current) return;

			const ws = new WebSocket(WS_URL);
			wsRef.current = ws;

			ws.onopen = () => {
				setConnected(true);
				// Kick off the WebRTC handshake once the WS is up. The
				// WS carries SDP + ICE; once the DC opens, high-volume
				// traffic (currently just `PutImage`) moves there.
				startRtc();
				// Bind this session to a workspace. URL hash drives
				// the choice — empty hash = ask backend for a fresh
				// one. The `Workspace` reply will set the hash if the
				// backend assigned a different id (e.g. our hash was
				// stale across a restart).
				const requestedId =
					window.location.hash.replace(/^#/, "") || null;
				sendRef.current({ type: "OpenWorkspace", id: requestedId });
			};

			ws.onerror = () => {
				pushDiagnostic({
					level: "error",
					source: "ws",
					message: `WebSocket error connecting to ${WS_URL}`,
				});
			};

			ws.onclose = (event) => {
				setConnected(false);
				setActiveWorkspace(null);
				activeWorkspaceIdRef.current = null;
				setAttachedWindowIds(new Set());
				if (!disposed.current) {
					pushDiagnostic({
						level: "warn",
						source: "ws",
						message: `WebSocket closed (code ${event.code}); reconnecting in 3s`,
					});
					reconnectTimer.current = setTimeout(connect, 3000);
				}
			};

			ws.onmessage = (event) => {
				const msg: BackendToFrontend = JSON.parse(event.data);

				switch (msg.type) {
					case "SidecarList":
						replaceSidecars(msg.sidecars);
						break;
					case "Workspace": {
						setActiveWorkspace(msg.workspace);
						activeWorkspaceIdRef.current = msg.workspace.id;
						const currentHash = window.location.hash.replace(/^#/, "");
						if (currentHash !== msg.workspace.id) {
							window.history.replaceState(
								null,
								"",
								`${window.location.pathname}${window.location.search}#${msg.workspace.id}`,
							);
						}
						break;
					}
					case "AttachedWindows": {
						// Only act on snapshots for our bound workspace —
						// the backend broadcasts to all frontends rather
						// than per-recipient filtering.
						if (msg.workspace_id !== activeWorkspaceIdRef.current) {
							break;
						}
						setAttachedWindowIds(new Set(msg.window_ids));
						break;
					}
					case "ProcessList":
						replaceSidecarProcesses(msg.sidecar_id, msg.processes);
						break;
					case "WindowList":
						applyWindowList(msg.windows, (d) =>
							seedForDescriptor(d, sendRef.current),
						);
						break;
					case "WindowUpdate":
						applyWindowUpdate(msg.update);
						windowUpdateCallbackRef.current?.(msg.update);
						break;
					case "Bell":
						bellCallbackRef.current?.(msg.percent);
						break;
					case "CommandResult":
						pushDiagnostic({
							level: msg.success ? "info" : "error",
							source: "command",
							message: msg.message || (msg.success ? "OK" : "command failed"),
						});
						break;
					case "RtcAnswer": {
						const pc = peerConnectionRef.current;
						if (pc) {
							pc.setRemoteDescription({
								type: "answer",
								sdp: msg.sdp,
							}).catch((err) => {
								pushDiagnostic({
									level: "error",
									source: "ws",
									message: `setRemoteDescription failed: ${err}`,
								});
							});
						}
						break;
					}
					case "RtcIceCandidate": {
						const pc = peerConnectionRef.current;
						if (pc && msg.candidate) {
							pc.addIceCandidate({
								candidate: msg.candidate,
								sdpMid: msg.sdp_mid ?? undefined,
								sdpMLineIndex: msg.sdp_mline_index ?? undefined,
							}).catch((err) => {
								pushDiagnostic({
									level: "warn",
									source: "ws",
									message: `addIceCandidate failed: ${err}`,
								});
							});
						}
						break;
					}
				}
			};
		}

		connect();

		return () => {
			disposed.current = true;
			clearTimeout(reconnectTimer.current);
			const ws = wsRef.current;
			if (ws) {
				if (ws.readyState === WebSocket.OPEN) {
					ws.close();
				} else {
					ws.onopen = ws.onclose = ws.onerror = ws.onmessage = null;
				}
			}
			dataChannelRef.current?.close();
			dataChannelRef.current = null;
			controlChannelRef.current?.close();
			controlChannelRef.current = null;
			peerConnectionRef.current?.close();
			peerConnectionRef.current = null;
		};
		// `pushDiagnostic` is a useCallback with [] deps and is therefore
		// stable for the lifetime of the component, so this effect still
		// runs only once on mount.
	}, [pushDiagnostic]);

	const send = useCallback((msg: FrontendToBackend) => {
		if (wsRef.current?.readyState === WebSocket.OPEN) {
			wsRef.current.send(JSON.stringify(msg));
		}
	}, []);
	sendRef.current = send;

	const onWindowUpdate = useCallback((cb: WindowUpdateCallback | null) => {
		windowUpdateCallbackRef.current = cb;
	}, []);

	const onBell = useCallback((cb: BellCallback | null) => {
		bellCallbackRef.current = cb;
	}, []);

	const onDataChannelMessage = useCallback(
		(cb: DataChannelMessageCallback | null) => {
			dcMessageCallbackRef.current = cb;
		},
		[],
	);

	return {
		connected,
		activeWorkspace,
		attachedWindowIds,
		send,
		onWindowUpdate,
		onBell,
		onDataChannelMessage,
		setWorkspaceName,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	};
}

/** Reactive read of the workspace's `name` field from the local
 *  Automerge doc. Returns `null` until the initial sync arrives.
 *  Re-renders automatically on remote changes (other tabs renaming
 *  the same workspace) and on local `setName` calls. */
export function useWorkspaceName(workspaceId: string | null): string | null {
	const subscribe = useCallback(
		(listener: () => void) => {
			if (!workspaceId) return () => {};
			return subscribeWorkspace(workspaceId, listener);
		},
		[workspaceId],
	);
	const getSnapshot = useCallback(
		() => (workspaceId ? getWorkspaceName(workspaceId) : null),
		[workspaceId],
	);
	return useSyncExternalStore(subscribe, getSnapshot);
}
