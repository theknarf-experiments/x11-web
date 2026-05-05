import { useLiveQuery } from "@tanstack/react-db";
import { Dock, DOCK_WINDOW_DRAG_MIME, type DockProcess } from "@x11-web/components";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppContextMenu, getAppContextMenuItems } from "./AppContextMenu";
import { ClientRenderer } from "./ClientRenderer";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import {
	applyOcifNodesSnapshot,
	ocifNodesCollection,
	patchWindow,
	processesCollection,
	setFocusedWindow,
	sidecarsCollection,
	unminimizeWindow,
	windowsCollection,
	windowsForProcess,
} from "./db";
import { useHotkey } from "@tanstack/react-hotkeys";
import {
	CanvasToolbar,
	type CanvasTool,
	TOOL_HOTKEYS,
} from "./CanvasToolbar";
import { GlobalMenuBar } from "./GlobalMenuBar";
import { InfiniteCanvas } from "./InfiniteCanvas";
import { OcifArrow } from "./OcifArrow";
import { OcifBox, type ResizeHandle } from "./OcifBox";
import { OcifMarkdown } from "./OcifMarkdown";
import { OcifPath } from "./OcifPath";
import { OcifText, type TextCorner } from "./OcifText";
import { decodeFrame } from "./rtcWire";
import { SettingsPanel } from "./SettingsPanel";
import type { FocusPolicy, InputEvent, MenuAction } from "./types";
import {
	useAttachedWindowIds,
	useBackendSocket,
	useWorkspaceName,
} from "./useBackendSocket";
import { WindowFrame } from "./WindowFrame";
import {
	appendOcifNodePathPoint,
	deleteOcifNode,
	FONT_MAX,
	FONT_MIN,
	getOcifNodes,
	insertOcifNode,
	type OcifNode,
	raiseOcifNode,
	setOcifArrowAnchor,
	setOcifArrowEndpoints,
	setOcifNodeBounds,
	setOcifNodeFontSize,
	setOcifNodePosition,
	setOcifNodeText,
	solveFontSizeForCornerTarget,
	subscribe as subscribeWorkspace,
} from "./workspaceSync";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

function App() {
	const {
		connected,
		activeWorkspace,
		send,
		onBell,
		onDataChannelMessage,
		setWorkspaceName,
		attachWindowToWorkspace,
		diagnostics,
		dismissDiagnostic,
		clearDiagnostics,
	} = useBackendSocket();
	const workspaceName = useWorkspaceName(activeWorkspace?.id ?? null);
	const attachedWindowIds = useAttachedWindowIds(activeWorkspace?.id ?? null);

	// Mirror the per-workspace Automerge doc's OCIF nodes into the
	// `ocifNodesCollection`. The doc is the source of truth — this
	// effect just projects it for `useLiveQuery` to read. Mutations
	// route through `workspaceSync.*` helpers, which trigger the
	// subscription that re-runs `apply`.
	useEffect(() => {
		if (!activeWorkspace) return;
		const apply = () => {
			applyOcifNodesSnapshot(
				activeWorkspace.id,
				getOcifNodes(activeWorkspace.id),
			);
		};
		apply();
		return subscribeWorkspace(activeWorkspace.id, apply);
	}, [activeWorkspace]);

	// Read OCIF nodes for the active workspace. Source of truth is
	// the Automerge doc, projected into `ocifNodesCollection` by the
	// effect above.
	const { data: ocifNodeRows = [] } = useLiveQuery((q) =>
		q.from({ n: ocifNodesCollection }).select(({ n }) => n),
	);
	const activeWorkspaceId = activeWorkspace?.id ?? null;
	const ocifNodes = useMemo(() => {
		const out = new Map<string, OcifNode>();
		for (const row of ocifNodeRows) {
			if (row.workspaceId !== activeWorkspaceId) continue;
			out.set(row.nodeId, {
				x: row.x,
				y: row.y,
				z: row.z,
				width: row.width,
				height: row.height,
				text: row.text,
				rect: row.rect,
				path: row.path,
				arrow: row.arrow,
				edge: row.edge,
				text_style: row.textStyle,
				window: row.window,
				resource: row.resourceId,
				text_mime_type: row.textMimeType,
			});
		}
		return out;
	}, [ocifNodeRows, activeWorkspaceId]);

	const { data: processes = [] } = useLiveQuery((q) =>
		q.from({ p: processesCollection }).select(({ p }) => p),
	);
	const { data: sidecars = [] } = useLiveQuery((q) =>
		q.from({ s: sidecarsCollection }).select(({ s }) => s),
	);
	// DOM order matches collection insertion order; visual stacking comes
	// from `OcifNode.z` via CSS z-index. Reordering the array would flip
	// the DOM whenever a window is raised, which breaks any hit-testing that
	// uses DOM-position locators (e.g. Playwright's `nth(...)`).
	const { data: windows = [] } = useLiveQuery((q) =>
		q.from({ w: windowsCollection }).select(({ w }) => w),
	);

	/** One renderer per top-level X11 window (keyed by window_id). */
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	/** Windows the user has locally closed (KillProcess in flight); filtered
	 *  out of the visible set until the backend's next WindowList drops them. */
	const [closedWindowIds, setClosedWindowIds] = useState<Set<string>>(
		() => new Set(),
	);
	/** Focus policy setting. */
	const [focusPolicy, setFocusPolicy] = useState<FocusPolicy>("click-to-focus");

	/** Active canvas tool — "pointer" (default), "box", or "arrow".
	 *  Hotkeys (V / B / A) registered below. */
	const [tool, setTool] = useState<CanvasTool>("pointer");
	useHotkey(TOOL_HOTKEYS.pointer, () => setTool("pointer"));
	useHotkey(TOOL_HOTKEYS.box, () => setTool("box"));
	useHotkey(TOOL_HOTKEYS.arrow, () => setTool("arrow"));
	useHotkey(TOOL_HOTKEYS.text, () => setTool("text"));
	useHotkey(TOOL_HOTKEYS.pen, () => setTool("pen"));
	useHotkey(TOOL_HOTKEYS.markdown, () => setTool("markdown"));
	/** Local preview while the user is drag-creating a shape.
	 *  Not synced — peers see the shape only on commit (pointerup).
	 *  For arrows, `startNodeId` / `endNodeId` are the boxes the
	 *  gesture began / currently hovers over (or null for empty
	 *  canvas); on release a connected `@ocif/edge` is created
	 *  when both ends are over boxes. */
	const [drawing, setDrawing] = useState<
		| {
				kind: "box";
				startX: number;
				startY: number;
				x: number;
				y: number;
				w: number;
				h: number;
		  }
		| {
				kind: "arrow";
				startX: number;
				startY: number;
				endX: number;
				endY: number;
				startNodeId: string | null;
				endNodeId: string | null;
		  }
		| {
				kind: "pen";
				/** Raw input points in canvas coords. Pressure (third
				 *  component) is captured from the pointer event when
				 *  available; perfect-freehand uses it to vary stroke
				 *  width along the path. */
				points: Array<[number, number, number]>;
				/** OCIF node id once it's been inserted — set on the
				 *  first pointermove with ≥ 2 points so peers see the
				 *  stroke grow live. `null` until then. */
				nodeId: string | null;
		  }
		| null
	>(null);
	/** Mirror of `drawing` for ref-style reads inside event
	 *  handlers — lets `onMove` access the latest state without
	 *  triggering an effect re-run / listener re-bind on every
	 *  pointermove. */
	const drawingRef = useRef(drawing);
	drawingRef.current = drawing;
	/** Currently-selected OCIF node id (or null). Selection is
	 *  per-tab — different users have different selections. */
	const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
	/** Currently-text-editing OCIF node id (or null). Per-tab. */
	const [editingNodeId, setEditingNodeId] = useState<string | null>(null);
	/** Live drag state for an arrow endpoint or arrow-create
	 *  gesture. Drives two visual hints:
	 *    - the endpoint being dragged renders in a "dragging" color
	 *    - the box currently under the cursor (if a valid drop
	 *      target) shows an attach-preview outline
	 *  Per-tab; the doc carries the actual anchored state, this is
	 *  purely UI feedback during the gesture. */
	const [arrowDrag, setArrowDrag] = useState<{
		arrowId: string;
		end: "start" | "end";
		dropTargetNodeId: string | null;
	} | null>(null);
	/** Helper to translate page-coords (from window-level pointermove
	 *  events) into canvas-space, exposed by `InfiniteCanvas`. */
	const pageToCanvasRef = useRef<
		((cx: number, cy: number) => { x: number; y: number }) | null
	>(null);
	/** Highest `z` we've assigned to any node in this session. New
	 *  boxes get `maxZ + 1` so they land on top. */
	const maxNodeZRef = useRef(0);

	// Reap renderers and locally-closed entries for windows the
	// backend has dropped. Renderer creation happens lazily during
	// render so a window appears as soon as it lands in the list.
	useEffect(() => {
		const live = new Set(windows.map((w) => w.windowId));
		for (const wid of [...renderersRef.current.keys()]) {
			if (!live.has(wid)) renderersRef.current.delete(wid);
		}
		setClosedWindowIds((prev) => {
			let changed = false;
			const next = new Set(prev);
			for (const wid of prev) {
				if (!live.has(wid)) {
					next.delete(wid);
					changed = true;
				}
			}
			return changed ? next : prev;
		});
	}, [windows]);

	// Per-window thumbnail object URLs, keyed by window_id.
	// Driven by `Frame::WindowThumbnail` arrivals over the DC; the
	// previous URL is revoked when superseded so we don't leak.
	const [thumbnails, setThumbnails] = useState<Map<string, string>>(
		() => new Map(),
	);

	// Frame messages over the WebRTC DataChannel: decode the
	// Cap'n Proto envelope and dispatch on variant. PutImage routes
	// to the per-window renderer; WindowThumbnail updates the
	// thumbnail map (consumed by the dock's spawn popover).
	useEffect(() => {
		onDataChannelMessage((bytes) => {
			const msg = decodeFrame(bytes);
			if (!msg) return;
			if (msg.kind === "putImage") {
				const renderers = renderersRef.current;
				let r = renderers.get(msg.windowId);
				if (!r) {
					r = new ClientRenderer(msg.width || 1, msg.height || 1);
					renderers.set(msg.windowId, r);
				}
				r.pushPutImage(msg.x, msg.y, msg.data);
				return;
			}
			if (msg.kind === "thumbnail") {
				// Copy the bytes into a fresh ArrayBuffer because the
				// Uint8Array we got from the decoder is a view into
				// the reassembler's buffer, which gets reused.
				const copy = new Uint8Array(msg.data);
				const url = URL.createObjectURL(
					new Blob([copy], { type: "image/webp" }),
				);
				setThumbnails((prev) => {
					const next = new Map(prev);
					const old = next.get(msg.windowId);
					if (old) URL.revokeObjectURL(old);
					next.set(msg.windowId, url);
					return next;
				});
			}
		});
		return () => onDataChannelMessage(null);
	}, [onDataChannelMessage]);

	// Top-level Bell event — play an audible/visual notification.
	useEffect(() => {
		onBell((percent) => {
			try {
				const ctx = new AudioContext();
				const osc = ctx.createOscillator();
				const gain = ctx.createGain();
				osc.connect(gain);
				gain.connect(ctx.destination);
				osc.frequency.value = 800;
				gain.gain.value = Math.max(0.01, percent / 100);
				osc.start();
				osc.stop(ctx.currentTime + 0.1);
			} catch {
				document.body.style.backgroundColor = "#fff";
				setTimeout(() => {
					document.body.style.backgroundColor = "";
				}, 100);
			}
		});
		return () => onBell(null);
	}, [onBell]);

	function handleSpawn(sidecarId: string, command: string, args: string[]) {
		if (!activeWorkspace) return;
		send({
			type: "SpawnProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			workspace_id: activeWorkspace.id,
			command,
			args,
		});
	}

	// Track the highest `z` we've seen so newly-drawn boxes land
	// on top of existing ones. Reads on every doc change.
	useEffect(() => {
		let max = maxNodeZRef.current;
		for (const node of ocifNodes.values()) {
			if (node.z > max) max = node.z;
		}
		maxNodeZRef.current = max;
	}, [ocifNodes]);

	/** Pointer-down on empty canvas. In "box" or "arrow" mode this
	 *  starts a drag-create gesture; in "pointer" mode it deselects
	 *  any selected node. We deliberately don't touch
	 *  `editingNodeId` here — the textarea's `onBlur` is the
	 *  canonical exit signal for edit mode, and clearing the state
	 *  synchronously here would unmount the textarea before its
	 *  blur event can fire, losing the in-progress text. */
	const handleCanvasPointerDown = useCallback(
		(point: { x: number; y: number }, e: React.PointerEvent) => {
			if (!activeWorkspace) return;
			if (tool === "box") {
				e.preventDefault();
				setSelectedNodeId(null);
				setDrawing({
					kind: "box",
					startX: point.x,
					startY: point.y,
					x: point.x,
					y: point.y,
					w: 0,
					h: 0,
				});
				return;
			}
			if (tool === "arrow") {
				e.preventDefault();
				setSelectedNodeId(null);
				// If the gesture starts on top of an attachable
				// node (box / text / window), capture its id so we
				// can build a connected `@ocif/edge` on release.
				// `data-ocif-attachable` is the unified marker;
				// arrows / paths / popups deliberately don't carry
				// it (you can't anchor an edge to another edge).
				const targetEl = e.target as Element | null;
				const startEl = targetEl?.closest<HTMLElement>(
					"[data-ocif-attachable]",
				);
				const startNodeId = startEl?.dataset.ocifAttachable ?? null;
				setDrawing({
					kind: "arrow",
					startX: point.x,
					startY: point.y,
					endX: point.x,
					endY: point.y,
					startNodeId,
					endNodeId: startNodeId,
				});
				return;
			}
			if (tool === "pen") {
				e.preventDefault();
				setSelectedNodeId(null);
				setDrawing({
					kind: "pen",
					points: [[point.x, point.y, e.pressure || 0.5]],
					nodeId: null,
				});
				return;
			}
			if (tool === "text") {
				e.preventDefault();
				// Click-to-place: drop a text-only node at the
				// cursor and immediately enter edit mode. Empty
				// text nodes are auto-deleted on blur (see
				// `handleExitEdit`) so a stray click leaves no
				// orphan in the doc.
				const id = crypto.randomUUID();
				const z = maxNodeZRef.current + 1;
				maxNodeZRef.current = z;
				const TEXT_W = 200;
				const TEXT_H = 32;
				insertOcifNode(activeWorkspace.id, id, {
					x: point.x - TEXT_W / 2,
					y: point.y - TEXT_H / 2,
					z,
					width: TEXT_W,
					height: TEXT_H,
					text: "",
				});
				setSelectedNodeId(id);
				setEditingNodeId(id);
				// One-shot: drop back to pointer mode after a
				// create. Matches the convention in design tools
				// (Figma / tldraw): pick a tool, place one shape,
				// resume editing.
				setTool("pointer");
				return;
			}
			if (tool === "markdown") {
				e.preventDefault();
				// Click-to-place a markdown note at a fixed default
				// size — the user resizes via corner handles, the
				// content scrolls when it overflows. The editor is
				// always live; no separate edit mode.
				const id = crypto.randomUUID();
				const z = maxNodeZRef.current + 1;
				maxNodeZRef.current = z;
				const MD_W = 280;
				const MD_H = 200;
				insertOcifNode(activeWorkspace.id, id, {
					x: point.x - MD_W / 2,
					y: point.y - MD_H / 2,
					z,
					width: MD_W,
					height: MD_H,
					text: "",
					text_mime_type: "text/markdown",
				});
				setSelectedNodeId(id);
				setTool("pointer");
				return;
			}
			setSelectedNodeId(null);
		},
		[tool, activeWorkspace],
	);

	// Drive the draw preview from window-level pointer events so
	// the gesture survives if the cursor leaves the canvas mid-
	// drag. Commits on release. Side effects (the doc insert) live
	// OUTSIDE the state updater — React Strict Mode dev-double-
	// invokes setter callbacks, which would otherwise create two
	// nodes. The captured `drawing` is up to date because this
	// effect re-runs on every pointermove.
	useEffect(() => {
		if (!drawing || !activeWorkspace) return;
		const onMove = (ev: PointerEvent) => {
			const toCanvas = pageToCanvasRef.current;
			if (!toCanvas) return;
			const p = toCanvas(ev.clientX, ev.clientY);
			// Pen: side effects (insert / append) happen outside
			// the state updater so React Strict Mode's dev-double-
			// invoke can't fire them twice.
			//
			// Wire shape: the node is anchored at the FIRST sampled
			// canvas point (`node.x/y`); each subsequent sample is
			// stored as an `(lx, ly, p)` triple offset from that
			// anchor. Inserting the node seeds `points` with all
			// samples collected so far (≥ 2, including the
			// pointerdown sample); every later pointermove pushes
			// just one triple via `appendOcifNodePathPoint`. That's
			// the wire delta — three Automerge list-pushes per
			// frame, regardless of stroke length.
			const cur = drawingRef.current;
			if (cur?.kind === "pen") {
				const sample: [number, number, number] = [
					p.x,
					p.y,
					ev.pressure || 0.5,
				];
				const newPoints = [...cur.points, sample];
				let nodeId = cur.nodeId;
				if (!nodeId && newPoints.length >= 2) {
					const [fx, fy] = cur.points[0];
					nodeId = crypto.randomUUID();
					const z = maxNodeZRef.current + 1;
					maxNodeZRef.current = z;
					const flat: number[] = [];
					for (const [px, py, pp] of newPoints) {
						flat.push(px - fx, py - fy, pp);
					}
					insertOcifNode(activeWorkspace.id, nodeId, {
						x: fx,
						y: fy,
						z,
						width: 0,
						height: 0,
						path: { points: flat, fill_color: "#ffffff" },
					});
				} else if (nodeId) {
					const [fx, fy] = cur.points[0];
					appendOcifNodePathPoint(activeWorkspace.id, nodeId, [
						p.x - fx,
						p.y - fy,
						ev.pressure || 0.5,
					]);
				}
				setDrawing({
					kind: "pen",
					points: newPoints,
					nodeId,
				});
				return;
			}
			// Hit-test for arrow snapping: walk up from the element
			// under the cursor to the nearest attachable marker.
			const overEl = document.elementFromPoint(ev.clientX, ev.clientY);
			const overAttachable = overEl?.closest<HTMLElement>(
				"[data-ocif-attachable]",
			);
			const overNodeId = overAttachable?.dataset.ocifAttachable ?? null;
			setDrawing((d) => {
				if (!d) return d;
				if (d.kind === "box") {
					return {
						...d,
						x: Math.min(d.startX, p.x),
						y: Math.min(d.startY, p.y),
						w: Math.abs(p.x - d.startX),
						h: Math.abs(p.y - d.startY),
					};
				}
				return { ...d, endX: p.x, endY: p.y, endNodeId: overNodeId };
			});
		};
		const onUp = () => {
			if (drawing.kind === "box" && drawing.w >= 4 && drawing.h >= 4) {
				const id = crypto.randomUUID();
				const z = maxNodeZRef.current + 1;
				maxNodeZRef.current = z;
				insertOcifNode(activeWorkspace.id, id, {
					x: drawing.x,
					y: drawing.y,
					z,
					width: drawing.w,
					height: drawing.h,
					rect: {},
				});
				setSelectedNodeId(id);
				setTool("pointer");
			} else if (drawing.kind === "pen") {
				// Doc was already mutated live during the gesture
				// (see `onMove`). Just finalize selection + drop
				// back to pointer mode. A click without any drag
				// (no nodeId set) leaves nothing in the doc.
				if (drawing.nodeId) {
					setSelectedNodeId(drawing.nodeId);
					setTool("pointer");
				}
			} else if (drawing.kind === "arrow") {
				const dx = drawing.endX - drawing.startX;
				const dy = drawing.endY - drawing.startY;
				// Self-loops (start and end on the same box) are
				// degenerate — treat as no attachment for the dup
				// end. Either-end-anchored counts as connected so a
				// quick click on a box with tiny drag still creates
				// a meaningful arrow.
				const startNodeId = drawing.startNodeId;
				const endNodeId =
					drawing.endNodeId && drawing.endNodeId !== startNodeId
						? drawing.endNodeId
						: null;
				const anyAnchor = !!(startNodeId || endNodeId);
				if (anyAnchor || Math.hypot(dx, dy) >= 8) {
					const id = crypto.randomUUID();
					const z = maxNodeZRef.current + 1;
					maxNodeZRef.current = z;
					const arrow = {
						start_x: drawing.startX,
						start_y: drawing.startY,
						end_x: drawing.endX,
						end_y: drawing.endY,
					};
					const edge =
						startNodeId || endNodeId
							? {
									...(startNodeId ? { start: startNodeId } : {}),
									...(endNodeId ? { end: endNodeId } : {}),
									directed: true,
								}
							: undefined;
					insertOcifNode(activeWorkspace.id, id, {
						x: Math.min(drawing.startX, drawing.endX),
						y: Math.min(drawing.startY, drawing.endY),
						z,
						width: Math.abs(dx),
						height: Math.abs(dy),
						arrow,
						...(edge ? { edge } : {}),
					});
					setSelectedNodeId(id);
					setTool("pointer");
				}
			}
			setDrawing(null);
		};
		window.addEventListener("pointermove", onMove);
		window.addEventListener("pointerup", onUp);
		return () => {
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};
	}, [drawing, activeWorkspace]);

	/** Pointer-down on an existing OCIF node: select it, then drag
	 *  to move. For arrows, dragging the body translates both
	 *  endpoints; for boxes, it shifts position. */
	const handleNodePointerDown = useCallback(
		(id: string, e: React.PointerEvent) => {
			if (!activeWorkspace) return;
			e.preventDefault();
			setSelectedNodeId(id);
			const toCanvas = pageToCanvasRef.current;
			if (!toCanvas) return;
			const node = ocifNodes.get(id);
			if (!node) return;
			const startPoint = toCanvas(e.clientX, e.clientY);
			const wid = activeWorkspace.id;

			// Connected arrow: select-only, no body drag — geometry
			// follows the connected boxes' bounds, dragging the line
			// itself would just produce a stale free-floating arrow.
			if (node.edge) return;
			let onMove: (ev: PointerEvent) => void;
			if (node.arrow) {
				const baseSX = node.arrow.start_x;
				const baseSY = node.arrow.start_y;
				const baseEX = node.arrow.end_x;
				const baseEY = node.arrow.end_y;
				onMove = (ev) => {
					const t = pageToCanvasRef.current;
					if (!t) return;
					const p = t(ev.clientX, ev.clientY);
					const dx = p.x - startPoint.x;
					const dy = p.y - startPoint.y;
					setOcifArrowEndpoints(
						wid,
						id,
						baseSX + dx,
						baseSY + dy,
						baseEX + dx,
						baseEY + dy,
					);
				};
			} else {
				const offsetX = startPoint.x - node.x;
				const offsetY = startPoint.y - node.y;
				onMove = (ev) => {
					const t = pageToCanvasRef.current;
					if (!t) return;
					const p = t(ev.clientX, ev.clientY);
					setOcifNodePosition(wid, id, p.x - offsetX, p.y - offsetY);
				};
			}
			const onUp = () => {
				window.removeEventListener("pointermove", onMove);
				window.removeEventListener("pointerup", onUp);
			};
			window.addEventListener("pointermove", onMove);
			window.addEventListener("pointerup", onUp);
		},
		[activeWorkspace, ocifNodes],
	);

	/** Pointer-down on an arrow's start or end handle. The endpoint
	 *  always follows the cursor during the drag — even if it was
	 *  previously connected, the very first pointermove writes a
	 *  free anchor to the doc so the visual handle leaves the box.
	 *  Local `arrowDrag` state tracks the box currently under the
	 *  cursor so we can render a "drop here to connect" outline.
	 *  On release, if there's a drop target, the doc commits the
	 *  attachment to that node. */
	const handleArrowEndpointDown = useCallback(
		(id: string, end: "start" | "end", e: React.PointerEvent) => {
			if (!activeWorkspace) return;
			e.preventDefault();
			const node = ocifNodes.get(id);
			if (!node?.arrow) return;
			const wid = activeWorkspace.id;
			const otherEndNodeId =
				end === "start" ? node.edge?.end : node.edge?.start;
			let pendingDropNodeId: string | null = null;
			setArrowDrag({ arrowId: id, end, dropTargetNodeId: null });
			const onMove = (ev: PointerEvent) => {
				const t = pageToCanvasRef.current;
				if (!t) return;
				const p = t(ev.clientX, ev.clientY);
				// Free-anchor unconditionally so the handle visually
				// follows the cursor regardless of whether the user
				// is hovering a box.
				setOcifArrowAnchor(wid, id, end, {
					kind: "free",
					x: p.x,
					y: p.y,
				});
				// Walk the elements at the cursor (top-to-bottom)
				// rather than just the topmost — the dragging handle
				// itself is right under the cursor for the start
				// endpoint (perfect-arrows applies `padEnd` only,
				// not `padStart`), so `elementFromPoint` would see
				// the circle instead of the box behind it.
				const overAttachable = document
					.elementsFromPoint(ev.clientX, ev.clientY)
					.map((el) =>
						(el as Element).closest<HTMLElement>(
							"[data-ocif-attachable]",
						),
					)
					.find((el): el is HTMLElement => !!el);
				const overNodeId = overAttachable?.dataset.ocifAttachable ?? null;
				// Self-loops aren't useful — if hovering the box the
				// other end is anchored to, treat as no drop target.
				const dropTargetNodeId =
					overNodeId && overNodeId !== otherEndNodeId ? overNodeId : null;
				pendingDropNodeId = dropTargetNodeId;
				setArrowDrag({ arrowId: id, end, dropTargetNodeId });
			};
			const onUp = () => {
				window.removeEventListener("pointermove", onMove);
				window.removeEventListener("pointerup", onUp);
				if (pendingDropNodeId) {
					setOcifArrowAnchor(wid, id, end, {
						kind: "node",
						nodeId: pendingDropNodeId,
					});
				}
				setArrowDrag(null);
			};
			window.addEventListener("pointermove", onMove);
			window.addEventListener("pointerup", onUp);
		},
		[activeWorkspace, ocifNodes],
	);

	// Delete / Backspace deletes the selected box. Enter on a
	// selected box enters text-edit mode. Esc clears selection +
	// exits draw mode (text-edit Esc is handled inside the editor
	// via stopPropagation).
	useEffect(() => {
		const onKey = (e: KeyboardEvent) => {
			const target = e.target as HTMLElement | null;
			// Don't intercept while the user is typing into an input.
			if (
				target &&
				(target.tagName === "INPUT" ||
					target.tagName === "TEXTAREA" ||
					target.isContentEditable)
			) {
				return;
			}
			if (e.key === "Escape") {
				setSelectedNodeId(null);
				setEditingNodeId(null);
				setTool("pointer");
				return;
			}
			if (e.key === "Enter" && selectedNodeId && !editingNodeId) {
				e.preventDefault();
				setEditingNodeId(selectedNodeId);
				return;
			}
			if (
				(e.key === "Delete" || e.key === "Backspace") &&
				selectedNodeId &&
				activeWorkspace
			) {
				deleteOcifNode(activeWorkspace.id, selectedNodeId);
				setSelectedNodeId(null);
				setEditingNodeId(null);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [selectedNodeId, editingNodeId, activeWorkspace]);

	const handleChangeText = useCallback(
		(id: string, text: string) => {
			if (!activeWorkspace) return;
			setOcifNodeText(activeWorkspace.id, id, text);
		},
		[activeWorkspace],
	);

	const handleExitEdit = useCallback(() => {
		// Empty plain-text nodes have no chrome to indicate they
		// exist — leaving them in the doc just adds invisible
		// clutter. Drop on blur if the node has no shape ext, no
		// markdown chrome, and no content. Markdown notes always
		// have a header + tinted background, so an empty one is a
		// legitimate scratch note the user can return to.
		if (activeWorkspace && editingNodeId) {
			const node = ocifNodes.get(editingNodeId);
			if (
				node &&
				!node.rect &&
				!node.arrow &&
				node.text_mime_type !== "text/markdown" &&
				!(node.text && node.text.length > 0)
			) {
				deleteOcifNode(activeWorkspace.id, editingNodeId);
				setSelectedNodeId(null);
			}
		}
		setEditingNodeId(null);
	}, [activeWorkspace, editingNodeId, ocifNodes]);

	/** Pointer-down on a text node's corner scale handle. Solves
	 *  for the font size that lands the dragged corner closest to
	 *  the cursor (closed form, see `solveFontSizeForCornerTarget`).
	 *  The OPPOSITE corner stays anchored throughout the drag, so
	 *  the cursor's projection along the handle's diagonal axis
	 *  tracks 1:1 — perpendicular movement is absorbed by the
	 *  least-squares fit since text aspect ratio is text-bound. */
	const handleTextScaleHandleDown = useCallback(
		(id: string, corner: TextCorner, e: React.PointerEvent) => {
			if (!activeWorkspace) return;
			e.preventDefault();
			const node = ocifNodes.get(id);
			if (!node || node.rect || node.arrow) return;
			const wid = activeWorkspace.id;
			const signX: 1 | -1 =
				corner === "ne" || corner === "se" ? 1 : -1;
			const signY: 1 | -1 =
				corner === "se" || corner === "sw" ? 1 : -1;
			// Anchor = the corner OPPOSITE the one being dragged.
			// Stays fixed in canvas coords across the gesture so
			// the drag doesn't drift the node.
			const anchorX = signX > 0 ? node.x : node.x + node.width;
			const anchorY = signY > 0 ? node.y : node.y + node.height;
			// Capture text + style at gesture start. The text doesn't
			// change during a resize (handles aren't shown in edit
			// mode); pretext measures against this snapshot.
			const startText = node.text ?? "";
			const startStyle = node.text_style;
			const onMove = (ev: PointerEvent) => {
				const t = pageToCanvasRef.current;
				if (!t) return;
				const p = t(ev.clientX, ev.clientY);
				const targetFont = solveFontSizeForCornerTarget({
					text: startText,
					textStyle: startStyle,
					signX,
					signY,
					anchorX,
					anchorY,
					cursorX: p.x,
					cursorY: p.y,
				});
				const newFont = Math.max(
					FONT_MIN,
					Math.min(FONT_MAX, Math.round(targetFont)),
				);
				setOcifNodeFontSize(wid, id, newFont, {
					x: anchorX,
					y: anchorY,
					gripX: signX,
					gripY: signY,
				});
			};
			const onUp = () => {
				window.removeEventListener("pointermove", onMove);
				window.removeEventListener("pointerup", onUp);
			};
			window.addEventListener("pointermove", onMove);
			window.addEventListener("pointerup", onUp);
		},
		[activeWorkspace, ocifNodes],
	);

	/** Pointer-down on one of a box's resize handles. Computes new
	 *  bounds from the drag delta and ships a single `setBounds`
	 *  doc mutation per pointermove (one sync message rather than
	 *  separate position + size). */
	const handleResizeHandleDown = useCallback(
		(id: string, handle: ResizeHandle, e: React.PointerEvent) => {
			if (!activeWorkspace) return;
			e.preventDefault();
			const node = ocifNodes.get(id);
			const toCanvas = pageToCanvasRef.current;
			if (!node || !toCanvas) return;
			const start = toCanvas(e.clientX, e.clientY);
			const baseX = node.x;
			const baseY = node.y;
			const baseW = node.width;
			const baseH = node.height;
			const MIN = 16;
			const onMove = (ev: PointerEvent) => {
				const t = pageToCanvasRef.current;
				if (!t) return;
				const p = t(ev.clientX, ev.clientY);
				const dx = p.x - start.x;
				const dy = p.y - start.y;
				let nx = baseX;
				let ny = baseY;
				let nw = baseW;
				let nh = baseH;
				if (handle.includes("w")) {
					nx = baseX + dx;
					nw = baseW - dx;
				}
				if (handle.includes("e")) {
					nw = baseW + dx;
				}
				if (handle.includes("n")) {
					ny = baseY + dy;
					nh = baseH - dy;
				}
				if (handle.includes("s")) {
					nh = baseH + dy;
				}
				// Clamp to MIN, anchoring the opposite side so the
				// box doesn't drift when the cursor crosses the
				// minimum threshold.
				if (nw < MIN) {
					if (handle.includes("w")) nx = baseX + baseW - MIN;
					nw = MIN;
				}
				if (nh < MIN) {
					if (handle.includes("n")) ny = baseY + baseH - MIN;
					nh = MIN;
				}
				setOcifNodeBounds(activeWorkspace.id, id, nx, ny, nw, nh);
			};
			const onUp = () => {
				window.removeEventListener("pointermove", onMove);
				window.removeEventListener("pointerup", onUp);
			};
			window.addEventListener("pointermove", onMove);
			window.addEventListener("pointerup", onUp);
		},
		[activeWorkspace, ocifNodes],
	);

	const handleMove = useCallback(
		(windowId: string, x: number, y: number) => {
			// Top-level windows live as `OcifNode`s in the workspace
			// doc — moving one is just an `setOcifNodePosition`. Pop-
			// ups (overrideRedirect) don't get a node; their X-server
			// placement is authoritative and they ignore drags.
			if (!activeWorkspace) return;
			const win = windowsCollection.state.get(windowId);
			if (win?.overrideRedirect) return;
			setOcifNodePosition(activeWorkspace.id, windowId, x, y);
		},
		[activeWorkspace],
	);

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

	const handleFocus = useCallback(
		(windowId: string) => {
			if (activeWorkspace) {
				raiseOcifNode(activeWorkspace.id, windowId);
				unminimizeWindow(windowId);
			}
			// Set focus locally too. For X11 this is redundant — the X
			// server forwards a `WindowUpdate::Focused` after it
			// processes the click — but the macOS sidecar doesn't emit
			// focus events, so without setting it client-side the
			// global menu bar would never light up for macOS windows.
			setFocusedWindow(windowId);
		},
		[activeWorkspace],
	);

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
		[send],
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

	/** Restore a window from maximized/fullscreen/minimized to normal.
	 *  Pre-maximize node position is in `WindowRow.savedPosition` (the
	 *  per-tab local memory stashed at maximize time). */
	const handleRestore = useCallback(
		(windowId: string, sidecarId: string) => {
			if (!activeWorkspace) return;
			const win = windowsCollection.state.get(windowId);
			patchWindow(windowId, { wmState: "normal" });
			if (win?.savedPosition) {
				setOcifNodePosition(
					activeWorkspace.id,
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
		[activeWorkspace, send],
	);

	/** Focus follows mouse: focus window on mouse enter. */
	const handleMouseEnterWindow = useCallback(
		(windowId: string) => {
			if (focusPolicy !== "focus-follows-mouse") return;
			if (activeWorkspace) raiseOcifNode(activeWorkspace.id, windowId);
			setFocusedWindow(windowId);
		},
		[focusPolicy, activeWorkspace],
	);

	// Deduplicate windows by process for the dock -- one entry per (sidecarId, pid)
	// Include minimized windows so they appear in the dock
	const dockProcesses = useMemo(() => {
		const seen = new Set<string>();
		const result: DockProcess[] = [];
		for (const w of windows) {
			if (w.pid === 0) continue;
			const key = `${w.sidecarId}:${w.pid}`;
			if (!seen.has(key)) {
				seen.add(key);
				const proc = processes.find(
					(p) => p.sidecar_id === w.sidecarId && p.pid === w.pid,
				);
				result.push({
					sidecarId: w.sidecarId,
					pid: w.pid,
					title: proc ? proc.command : w.title,
					color: w.color,
				});
			}
		}
		return result;
	}, [windows, processes]);

	// Windows that haven't been placed on the canvas yet. Surface
	// them as draggable thumbnails inside the dock's spawn popover.
	const unattachedDockWindows = useMemo(
		() => windows.filter((w) => !attachedWindowIds.has(w.windowId)),
		[windows, attachedWindowIds],
	);

	// Right-click on a dock app icon. Position is in viewport
	// coords; the menu portal renders absolutely.
	const [dockContextMenu, setDockContextMenu] = useState<{
		sidecarId: string;
		pid: number;
		x: number;
		y: number;
	} | null>(null);

	const focusedWindow = windows.find((w) => w.focused) ?? null;
	const focusedTitle = focusedWindow?.title ?? null;
	const focusedMenu =
		focusedWindow && focusedWindow.menu.length > 0 ? focusedWindow.menu : null;

	const handleMenuActivate = useCallback(
		(action: MenuAction) => {
			if (!focusedWindow) return;
			send({
				type: "InputEvent",
				sidecar_id: focusedWindow.sidecarId,
				window_id: focusedWindow.windowId,
				event: { kind: "MenuActivate", action },
			});
		},
		[focusedWindow, send],
	);

	const focusedAppContextMenuItems =
		focusedWindow && focusedWindow.pid > 0
			? getAppContextMenuItems(focusedWindow.sidecarId, focusedWindow.pid, {
					onClose: handleCloseProcess,
				})
			: null;

	// Top-level windows render off the OcifNode map — `node.x/y/z`
	// are the canvas-collaborative geometry; the WindowRow lookup
	// supplies sidecar-driven props (title, focus, wmState, menu).
	// Pop-ups (override_redirect) never get a node and are picked
	// up directly from `windows` using the descriptor's X-server
	// placement.
	const visibleWindows = useMemo(
		() =>
			windows.filter(
				(w) =>
					!closedWindowIds.has(w.windowId) &&
					(w.overrideRedirect || attachedWindowIds.has(w.windowId)),
			),
		[windows, closedWindowIds, attachedWindowIds],
	);

	// Block rendering until the backend has bound this session to a
	// workspace. The frontend asks for one (by URL hash, or fresh) on
	// WS open, so this gate sits up just for the request/response
	// roundtrip — but everything below (canvas, dock, window frames)
	// needs a workspace context.
	if (!activeWorkspace) {
		return (
			<div
				style={{
					position: "fixed",
					inset: 0,
					display: "flex",
					alignItems: "center",
					justifyContent: "center",
					color: "rgba(255, 255, 255, 0.6)",
					font: "13px system-ui, sans-serif",
					background: "#1a1a1a",
				}}
			>
				{connected ? "Loading workspace…" : "Connecting to backend…"}
			</div>
		);
	}

	return (
		<>
			<GlobalMenuBar
				focusedTitle={focusedTitle}
				workspaceName={workspaceName}
				onRenameWorkspace={(name) => {
					if (activeWorkspace) setWorkspaceName(activeWorkspace.id, name);
				}}
				menu={focusedMenu}
				onActivate={handleMenuActivate}
				appContextMenuItems={focusedAppContextMenuItems}
			/>
			<InfiniteCanvas
				pageToCanvasRef={pageToCanvasRef}
				onCanvasPointerDown={handleCanvasPointerDown}
				onCanvasDrop={(point, event) => {
					const windowId = event.dataTransfer.getData(DOCK_WINDOW_DRAG_MIME);
					if (!windowId || !activeWorkspace) return;
					const win = windowsCollection.state.get(windowId);
					if (!win) return;
					// Create the workspace's OcifNode for this window
					// at the drop point. Width/height seeded from the
					// sidecar-reported dimensions; the backend will
					// keep the node's size in lockstep on subsequent
					// `WindowConfigured` events.
					attachWindowToWorkspace(
						activeWorkspace.id,
						windowId,
						win.sidecarId,
						point.x,
						point.y,
						win.width,
						win.height,
					);
				}}
			>
				{[...ocifNodes.entries()].map(([id, node]) => {
					// Window nodes are rendered by the `visibleWindows`
					// loop below — they need the WindowRow lookup for
					// title / focus / pixels, so we skip them here.
					if (node.window) return null;
					if (node.path) {
						return (
							<OcifPath
								key={id}
								id={id}
								node={node}
								selected={selectedNodeId === id}
								interactive={tool === "pointer"}
								onPointerDown={handleNodePointerDown}
							/>
						);
					}
					if (node.arrow) {
						return (
							<OcifArrow
								key={id}
								id={id}
								node={node}
								selected={selectedNodeId === id}
								interactive={tool === "pointer"}
								nodes={ocifNodes}
								draggingEnd={
									arrowDrag?.arrowId === id ? arrowDrag.end : null
								}
								onPointerDown={handleNodePointerDown}
								onEndpointPointerDown={handleArrowEndpointDown}
							/>
						);
					}
					const dropTarget =
						arrowDrag?.dropTargetNodeId === id ||
						(drawing?.kind === "arrow" && drawing.endNodeId === id);
					if (node.rect) {
						return (
							<OcifBox
								key={id}
								id={id}
								node={node}
								selected={selectedNodeId === id}
								editing={editingNodeId === id}
								interactive={tool === "pointer"}
								dropTarget={dropTarget}
								onPointerDown={handleNodePointerDown}
								onResizeHandleDown={handleResizeHandleDown}
								onChangeText={handleChangeText}
								onExitEdit={handleExitEdit}
							/>
						);
					}
					if (node.text_mime_type === "text/markdown") {
						return (
							<OcifMarkdown
								key={id}
								id={id}
								node={node}
								selected={selectedNodeId === id}
								interactive={tool === "pointer"}
								dropTarget={dropTarget}
								onPointerDown={handleNodePointerDown}
								onResizeHandleDown={handleResizeHandleDown}
								onChangeText={handleChangeText}
							/>
						);
					}
					return (
						<OcifText
							key={id}
							id={id}
							node={node}
							selected={selectedNodeId === id}
							editing={editingNodeId === id}
							interactive={tool === "pointer"}
							dropTarget={dropTarget}
							onPointerDown={handleNodePointerDown}
							onScaleHandleDown={handleTextScaleHandleDown}
							onChangeText={handleChangeText}
							onExitEdit={handleExitEdit}
						/>
					);
				})}
				{drawing?.kind === "box" && (
					<div
						style={{
							position: "absolute",
							left: drawing.x,
							top: drawing.y,
							width: drawing.w,
							height: drawing.h,
							background: "rgba(0, 122, 255, 0.08)",
							outline: "2px dashed #007aff",
							outlineOffset: "-2px",
							pointerEvents: "none",
							zIndex: 9999,
						}}
					/>
				)}
				{drawing?.kind === "arrow" && (
					<svg
						style={{
							position: "absolute",
							left: Math.min(drawing.startX, drawing.endX) - 24,
							top: Math.min(drawing.startY, drawing.endY) - 24,
							width: Math.abs(drawing.endX - drawing.startX) + 48,
							height: Math.abs(drawing.endY - drawing.startY) + 48,
							pointerEvents: "none",
							zIndex: 9999,
							overflow: "visible",
						}}
					>
						<line
							x1={drawing.startX - (Math.min(drawing.startX, drawing.endX) - 24)}
							y1={drawing.startY - (Math.min(drawing.startY, drawing.endY) - 24)}
							x2={drawing.endX - (Math.min(drawing.startX, drawing.endX) - 24)}
							y2={drawing.endY - (Math.min(drawing.startY, drawing.endY) - 24)}
							stroke="#007aff"
							strokeWidth={2}
							strokeDasharray="4 4"
						/>
					</svg>
				)}
				{/* No local pen preview — the in-progress stroke is
				 * mutated into the doc on every pointermove and
				 * rendered through the same OcifPath component as
				 * a finalised stroke. Sibling tabs see it grow
				 * live with no extra plumbing. */}
				{visibleWindows.map((win) => {
					// For top-level windows pull canvas geometry from
					// the matching OcifNode (collaborative); for pop-
					// ups the descriptor's `(x, y)` is the X server's
					// authoritative placement. zIndex defaults to 0
					// for popups so they layer above the topmost node
					// — popup parents trigger them and the user
					// expects them on top.
					const node = win.overrideRedirect
						? null
						: ocifNodes.get(win.windowId);
					if (!win.overrideRedirect && !node) return null;
					const x = node ? node.x : win.x;
					const y = node ? node.y : win.y;
					const zIndex = node ? Math.round(node.z) : 1_000_000;
					// Lazy-create the renderer so a window appearing in
					// the authoritative list shows up immediately, before
					// the first PutImage arrives over the DC. Sync
					// the back buffer to the descriptor's authoritative
					// size on every render — `pushPutImage`'s built-in
					// resize is grow-only, so a shrinking window (e.g.
					// Calculator going from Scientific back to Basic)
					// otherwise leaves the canvas at the previous larger
					// size with stale pixels in the unused region.
					let renderer = renderersRef.current.get(win.windowId);
					if (!renderer) {
						renderer = new ClientRenderer(win.width || 1, win.height || 1);
						renderersRef.current.set(win.windowId, renderer);
					} else if (
						renderer.width !== win.width ||
						renderer.height !== win.height
					) {
						renderer.resize(win.width || 1, win.height || 1);
					}
					return (
						<div
							key={win.windowId}
							onMouseEnter={() => handleMouseEnterWindow(win.windowId)}
						>
							<WindowFrame
								clientId={win.windowId}
								title={win.title}
								x={x}
								y={y}
								zIndex={zIndex}
								color={win.color}
								renderer={renderer}
								overrideRedirect={win.overrideRedirect}
								resizable={win.resizable}
								wmState={win.wmState}
								onClose={() => handleCloseWindow(win.windowId, win.sidecarId)}
								onMove={(nx, ny) => handleMove(win.windowId, nx, ny)}
								onResize={(nw, nh) =>
									handleResize(win.windowId, win.sidecarId, nw, nh)
								}
								onInput={(event) =>
									handleInput(win.windowId, win.sidecarId, event)
								}
								onFocus={() => handleFocus(win.windowId)}
								onMinimize={() => handleMinimize(win.windowId, win.sidecarId)}
								onMaximize={() => handleMaximize(win.windowId, win.sidecarId)}
								onRestore={() => handleRestore(win.windowId, win.sidecarId)}
								borderWidth={win.borderWidth}
								borderPixel={win.borderPixel}
							/>
						</div>
					);
				})}
			</InfiniteCanvas>
			<CanvasToolbar tool={tool} onSelect={setTool} />
			<Dock
				connected={connected}
				sidecars={sidecars}
				processes={dockProcesses}
				windows={unattachedDockWindows}
				thumbnails={thumbnails}
				onSpawn={handleSpawn}
				onFocusWindow={(sidecarId, pid) => {
					if (!activeWorkspace) return;
					for (const wid of windowsForProcess(sidecarId, pid)) {
						raiseOcifNode(activeWorkspace.id, wid);
						unminimizeWindow(wid);
					}
				}}
				onProcessContextMenu={(sidecarId, pid, x, y) =>
					setDockContextMenu({ sidecarId, pid, x, y })
				}
			/>
			{dockContextMenu && (
				<AppContextMenu
					items={getAppContextMenuItems(
						dockContextMenu.sidecarId,
						dockContextMenu.pid,
						{ onClose: handleCloseProcess },
					)}
					x={dockContextMenu.x}
					y={dockContextMenu.y}
					openUpwards
					onClose={() => setDockContextMenu(null)}
				/>
			)}
			<DiagnosticsPanel
				diagnostics={diagnostics}
				onDismiss={dismissDiagnostic}
				onClear={clearDiagnostics}
			/>
			<SettingsPanel
				focusPolicy={focusPolicy}
				onFocusPolicyChange={setFocusPolicy}
			/>
		</>
	);
}

export default App;
