import { type Dispatch, type SetStateAction, useCallback, useRef } from "react";
import {
	patchWindow,
	setFocusedWindow,
	unminimizeWindow,
	windowsCollection,
	windowsForProcess,
} from "./db";
import type { FocusPolicy, InputEvent } from "./types";
import type { OcifNode } from "./workspaceSync";
import { raiseOcifNode, setOcifNodePosition } from "./workspaceSync";

let requestCounter = 0;
function nextRequestId(): string {
	requestCounter += 1;
	return `wnd-act-${requestCounter}`;
}

interface UseWindowActionsArgs {
	activeWorkspaceId: string | null;
	ocifNodes: Map<string, OcifNode>;
	focusPolicy: FocusPolicy;
	// biome-ignore lint/suspicious/noExplicitAny: protocol union lives in the host
	send: (message: any) => void;
	setClosedWindowIds: Dispatch<SetStateAction<Set<string>>>;
}

/**
 * The collection of small window-lifecycle callbacks that
 * `WindowFrame` and the dock both consume. Each is a thin glue
 * between local mutations (`patchWindow`, `setOcifNodePosition`,
 * `raiseOcifNode`, …) and a corresponding `send(...)` to the
 * backend. Returned in a stable bag so the host can spread them
 * straight onto the `WindowFrame` props.
 */
export function useWindowActions({
	activeWorkspaceId,
	ocifNodes,
	focusPolicy,
	send,
	setClosedWindowIds,
}: UseWindowActionsArgs) {
	/** Top-level windows live as `OcifNode`s in the workspace doc —
	 *  moving one is just `setOcifNodePosition`. Pop-ups
	 *  (overrideRedirect) don't get a node; their X-server placement
	 *  is authoritative and they ignore drags. */
	const handleMove = useCallback(
		(windowId: string, x: number, y: number) => {
			if (!activeWorkspaceId) return;
			const win = windowsCollection.state.get(windowId);
			if (win?.overrideRedirect) return;
			setOcifNodePosition(activeWorkspaceId, windowId, x, y);
		},
		[activeWorkspaceId],
	);

	/** Debounced resize — coalesce a stream of drag-resize updates
	 *  into a single `ResizeWindow` send so we don't flood the
	 *  backend / the X server. */
	const resizeTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
	const handleResize = useCallback(
		(windowId: string, sidecarId: string, width: number, height: number) => {
			clearTimeout(resizeTimerRef.current);
			resizeTimerRef.current = setTimeout(() => {
				send({
					type: "ResizeWindow",
					sidecar_id: sidecarId,
					window_id: windowId,
					width,
					height,
				});
			}, 100);
		},
		[send],
	);

	/** Click-to-focus path: raise the node so it z-orders on top,
	 *  unminimize if it was hidden, and set local focus state. The
	 *  X server forwards its own `Focused` shortly after, but the
	 *  macOS sidecar doesn't, so the local set is needed there. */
	const handleFocus = useCallback(
		(windowId: string) => {
			if (activeWorkspaceId) {
				raiseOcifNode(activeWorkspaceId, windowId);
				unminimizeWindow(windowId);
			}
			setFocusedWindow(windowId);
		},
		[activeWorkspaceId],
	);

	/** Forward a per-window `InputEvent` to the backend. The
	 *  `WindowFrame` produces these; we just envelope and send. */
	const handleInput = useCallback(
		(windowId: string, sidecarId: string, event: InputEvent) => {
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event,
			});
		},
		[send],
	);

	/** Kill a process and locally hide all of its windows until the
	 *  backend's next `WindowList` drops them. */
	const handleCloseProcess = useCallback(
		(sidecarId: string, pid: number) => {
			const wids = windowsForProcess(sidecarId, pid);
			if (wids.length > 0) {
				setClosedWindowIds((prev) => new Set([...prev, ...wids]));
			}
			send({
				type: "KillProcess",
				request_id: nextRequestId(),
				sidecar_id: sidecarId,
				pid,
			});
		},
		[send, setClosedWindowIds],
	);

	/** Minimize a window (hide it, show in dock). */
	const handleMinimize = useCallback(
		(windowId: string, sidecarId: string) => {
			patchWindow(windowId, { wmState: "minimized" });
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "minimized" },
			});
		},
		[send],
	);

	/** Maximize a window (expand to fill viewport). Stash the
	 *  pre-maximize node position so Restore can put it back. */
	const handleMaximize = useCallback(
		(windowId: string, sidecarId: string) => {
			const node = ocifNodes.get(windowId);
			patchWindow(windowId, {
				wmState: "maximized",
				...(node ? { savedPosition: { x: node.x, y: node.y } } : {}),
			});
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "maximized" },
			});
		},
		[send, ocifNodes],
	);

	/** Close a window gracefully via ICCCM WM_DELETE_WINDOW. */
	const handleCloseWindow = useCallback(
		(windowId: string, sidecarId: string) => {
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "close" },
			});
		},
		[send],
	);

	/** Restore a window from maximized/fullscreen/minimized to
	 *  normal. Pre-maximize node position is in
	 *  `WindowRow.savedPosition` (per-tab local memory stashed at
	 *  maximize time). */
	const handleRestore = useCallback(
		(windowId: string, sidecarId: string) => {
			if (!activeWorkspaceId) return;
			const win = windowsCollection.state.get(windowId);
			patchWindow(windowId, { wmState: "normal" });
			if (win?.savedPosition) {
				setOcifNodePosition(
					activeWorkspaceId,
					windowId,
					win.savedPosition.x,
					win.savedPosition.y,
				);
			}
			send({
				type: "InputEvent",
				sidecar_id: sidecarId,
				window_id: windowId,
				event: { kind: "WindowManage", action: "normal" },
			});
		},
		[activeWorkspaceId, send],
	);

	/** Focus follows mouse: focus a window on mouse enter — gated by
	 *  the user's focus policy setting. */
	const handleMouseEnterWindow = useCallback(
		(windowId: string) => {
			if (focusPolicy !== "focus-follows-mouse") return;
			if (activeWorkspaceId) raiseOcifNode(activeWorkspaceId, windowId);
			setFocusedWindow(windowId);
		},
		[focusPolicy, activeWorkspaceId],
	);

	return {
		handleMove,
		handleResize,
		handleFocus,
		handleInput,
		handleCloseProcess,
		handleMinimize,
		handleMaximize,
		handleCloseWindow,
		handleRestore,
		handleMouseEnterWindow,
	};
}
