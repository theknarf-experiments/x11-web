import {
	type CameraStore,
	createCameraStore,
	PinchTracker,
	panBy,
	viewportToCanvas,
	wheelIntent,
	zoomAt,
} from "@x11-web/canvas-core";
import {
	type ReactNode,
	useEffect,
	useMemo,
	useRef,
	useState,
	useSyncExternalStore,
} from "react";
import { Popover } from "../Popover/Popover.tsx";
import s from "./InfiniteCanvas.module.css";

interface InfiniteCanvasProps {
	children: ReactNode;
	/// Rendered beneath the transform layer, filling the viewport —
	/// e.g. a GL shape layer following the same camera via the shared
	/// store. Pointer events pass through to the canvas.
	underlay?: ReactNode;
	/// External camera store (`createCameraStore()` from
	/// `@x11-web/canvas-core`). Pass one when another renderer needs
	/// to follow the same camera; omitted, the canvas owns a private
	/// store internally.
	cameraStore?: CameraStore;
	/// Called on drop with the drop point already translated into
	/// canvas coordinates (camera-aware). Used to land dragged
	/// polaroids onto the canvas at the cursor.
	onCanvasDrop?: (
		point: { x: number; y: number },
		event: React.DragEvent,
	) => void;
	/// Called when the user pointer-downs on empty canvas (not on a
	/// window or any other child). Caller gets canvas-space coords
	/// and the original event so it can attach window-level
	/// pointermove / pointerup listeners for the drag-to-draw
	/// gesture.
	onCanvasPointerDown?: (
		point: { x: number; y: number },
		event: React.PointerEvent,
	) => void;
	/// Helper: convert page-space pointer coords (e.g. from a
	/// window-level pointermove) into the canvas's local coords.
	/// Provided as a ref so the parent can grab the latest function
	/// without re-rendering on every camera update.
	pageToCanvasRef?: React.MutableRefObject<
		((clientX: number, clientY: number) => { x: number; y: number }) | null
	>;
}

const ZOOM_PRESETS = [0.25, 0.5, 0.75, 1, 1.5, 2];

export function InfiniteCanvas({
	children,
	underlay,
	cameraStore,
	onCanvasDrop,
	onCanvasPointerDown,
	pageToCanvasRef,
}: InfiniteCanvasProps) {
	const internalStore = useMemo(() => createCameraStore(), []);
	const store = cameraStore ?? internalStore;
	const storeRef = useRef(store);
	storeRef.current = store;
	const camera = useSyncExternalStore(store.subscribe, store.get);
	const viewportRef = useRef<HTMLDivElement>(null);

	// True while a wheel/pinch gesture is in flight. Drives
	// `will-change: transform` on the transform layer: promoted to a
	// compositor layer only *during* the gesture (smooth motion), then
	// demoted once it settles so the browser re-rasterizes vectors and
	// text crisply at the final scale. A permanent `will-change` keeps
	// the whole canvas on one cached raster, which is what made zoomed
	// content blurry.
	const [gestureActive, setGestureActive] = useState(false);

	// Jump the camera to a specific scale, keeping the canvas point
	// currently under the viewport's centre anchored. Used by the
	// preset menu — pure scroll-zoom can't snap precisely to round
	// numbers like 100%.
	const setZoom = (newScale: number) => {
		const el = viewportRef.current;
		if (!el) return;
		const rect = el.getBoundingClientRect();
		const st = storeRef.current;
		st.set(zoomAt(st.get(), rect.width / 2, rect.height / 2, newScale));
	};

	// Wheel: scroll = pan, cmd+scroll or pinch = zoom (trackpad pinch
	// fires as ctrl+wheel). Touch: two-pointer pinch zoom. Native
	// listeners for `{ passive: false }` preventDefault on wheel; the
	// touch listeners only observe, so children keep their events.
	useEffect(() => {
		const el = viewportRef.current;
		if (!el) return;

		let settleTimer: number | null = null;
		const markGesture = () => {
			setGestureActive(true);
			if (settleTimer !== null) clearTimeout(settleTimer);
			settleTimer = window.setTimeout(() => {
				settleTimer = null;
				setGestureActive(false);
			}, 150);
		};

		const onWheel = (e: WheelEvent) => {
			e.preventDefault();
			markGesture();
			const st = storeRef.current;
			const cam = st.get();
			const intent = wheelIntent(e);
			if (intent.type === "zoom") {
				const rect = el.getBoundingClientRect();
				st.set(
					zoomAt(
						cam,
						e.clientX - rect.left,
						e.clientY - rect.top,
						cam.scale * intent.factor,
					),
				);
			} else {
				st.set(panBy(cam, intent.dx, intent.dy));
			}
		};

		const pinch = new PinchTracker();
		const onPointerDown = (e: PointerEvent) => pinch.down(e);
		const onPointerMove = (e: PointerEvent) => {
			const update = pinch.move(e);
			if (!update) return;
			markGesture();
			const rect = el.getBoundingClientRect();
			const st = storeRef.current;
			const cam = st.get();
			st.set(
				zoomAt(
					cam,
					update.midX - rect.left,
					update.midY - rect.top,
					cam.scale * update.factor,
				),
			);
		};
		const onPointerEnd = (e: PointerEvent) => pinch.up(e.pointerId);

		// overflow:hidden still scrolls programmatically (focus()'s
		// scroll-into-view, scrollTo, …), which silently shifts the DOM
		// content out of sync with the camera — and with any GL layer
		// following the same camera. Snap it back immediately.
		const onScroll = () => {
			el.scrollTop = 0;
			el.scrollLeft = 0;
		};
		el.addEventListener("scroll", onScroll);

		el.addEventListener("wheel", onWheel, { passive: false });
		el.addEventListener("pointerdown", onPointerDown);
		// Move/up on window: pinching fingers routinely wander off the
		// viewport mid-gesture.
		window.addEventListener("pointermove", onPointerMove);
		window.addEventListener("pointerup", onPointerEnd);
		window.addEventListener("pointercancel", onPointerEnd);
		return () => {
			el.removeEventListener("scroll", onScroll);
			el.removeEventListener("wheel", onWheel);
			el.removeEventListener("pointerdown", onPointerDown);
			window.removeEventListener("pointermove", onPointerMove);
			window.removeEventListener("pointerup", onPointerEnd);
			window.removeEventListener("pointercancel", onPointerEnd);
			if (settleTimer !== null) clearTimeout(settleTimer);
		};
	}, []);

	const transform = `scale(${camera.scale}) translate(${-camera.x}px, ${-camera.y}px)`;
	const zoomPercent = Math.round(camera.scale * 100);

	const handleDragOver = onCanvasDrop
		? (e: React.DragEvent) => {
				// Allowing drop requires preventDefault on dragover.
				e.preventDefault();
				e.dataTransfer.dropEffect = "copy";
			}
		: undefined;

	const handleDrop = onCanvasDrop
		? (e: React.DragEvent) => {
				e.preventDefault();
				const el = viewportRef.current;
				if (!el) return;
				const rect = el.getBoundingClientRect();
				const point = viewportToCanvas(
					storeRef.current.get(),
					e.clientX - rect.left,
					e.clientY - rect.top,
				);
				onCanvasDrop(point, e);
			}
		: undefined;

	// Compute and expose a page→canvas helper. Updated on every
	// render so callers see the current camera. The ref pattern
	// avoids re-binding pointermove/pointerup listeners as the
	// camera changes mid-drag.
	if (pageToCanvasRef) {
		pageToCanvasRef.current = (clientX, clientY) => {
			const el = viewportRef.current;
			if (!el) return { x: clientX, y: clientY };
			const rect = el.getBoundingClientRect();
			return viewportToCanvas(
				storeRef.current.get(),
				clientX - rect.left,
				clientY - rect.top,
			);
		};
	}

	const handlePointerDown = onCanvasPointerDown
		? (e: React.PointerEvent) => {
				// Fires for any pointerdown that bubbles up to the
				// viewport. Children that want to consume the event
				// (windows, toolbar buttons, OcifBox in pointer mode)
				// stop propagation; everything else falls through to
				// the canvas tool dispatcher (drag-create / deselect).
				const el = viewportRef.current;
				if (!el) return;
				const rect = el.getBoundingClientRect();
				onCanvasPointerDown(
					viewportToCanvas(
						storeRef.current.get(),
						e.clientX - rect.left,
						e.clientY - rect.top,
					),
					e,
				);
			}
		: undefined;

	return (
		<div
			ref={viewportRef}
			className={s.viewport}
			data-testid="infinite-canvas"
			onDragOver={handleDragOver}
			onDrop={handleDrop}
			onPointerDown={handlePointerDown}
		>
			{underlay && <div className={s.underlay}>{underlay}</div>}
			<div
				className={s.transform}
				style={{
					transform,
					willChange: gestureActive ? "transform" : "auto",
				}}
				data-canvas-scale={camera.scale}
			>
				{children}
			</div>
			<div className={s.zoomIndicatorWrap}>
				<Popover
					side="top"
					className={s.zoomMenu}
					trigger={
						<button
							type="button"
							className={s.zoomIndicator}
							data-testid="zoom-indicator"
						>
							{zoomPercent}%
						</button>
					}
				>
					{({ close }) => (
						<div role="menu" data-testid="zoom-menu">
							{ZOOM_PRESETS.map((preset) => (
								<button
									key={preset}
									type="button"
									className={
										camera.scale === preset
											? s.zoomMenuItemActive
											: s.zoomMenuItem
									}
									onClick={() => {
										setZoom(preset);
										close();
									}}
								>
									{Math.round(preset * 100)}%
								</button>
							))}
						</div>
					)}
				</Popover>
			</div>
		</div>
	);
}
