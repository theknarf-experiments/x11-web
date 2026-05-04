import { type ReactNode, useEffect, useRef, useState } from "react";
import s from "./InfiniteCanvas.module.css";

interface Camera {
	x: number;
	y: number;
	scale: number;
}

interface InfiniteCanvasProps {
	children: ReactNode;
	/// Called on drop with the drop point already translated into
	/// canvas coordinates (camera-aware). Used to land dragged
	/// polaroids onto the canvas at the cursor.
	onCanvasDrop?: (point: { x: number; y: number }, event: React.DragEvent) => void;
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

const MIN_SCALE = 0.1;
const MAX_SCALE = 3;

export function InfiniteCanvas({
	children,
	onCanvasDrop,
	onCanvasPointerDown,
	pageToCanvasRef,
}: InfiniteCanvasProps) {
	const [camera, setCamera] = useState<Camera>({ x: 0, y: 0, scale: 1 });
	const cameraRef = useRef(camera);
	cameraRef.current = camera;
	const viewportRef = useRef<HTMLDivElement>(null);

	// Wheel: scroll = pan, cmd+scroll or pinch = zoom
	// Use a native event listener to get { passive: false } for preventDefault
	useEffect(() => {
		const el = viewportRef.current;
		if (!el) return;

		const onWheel = (e: WheelEvent) => {
			e.preventDefault();
			const cam = cameraRef.current;

			if (e.ctrlKey || e.metaKey) {
				// Zoom (cmd+scroll or pinch-to-zoom — trackpad pinch fires as ctrlKey+wheel)
				const zoomFactor = e.deltaY > 0 ? 0.95 : 1.05;
				const newScale = Math.min(
					MAX_SCALE,
					Math.max(MIN_SCALE, cam.scale * zoomFactor),
				);

				const rect = el.getBoundingClientRect();
				const cursorX = e.clientX - rect.left;
				const cursorY = e.clientY - rect.top;

				const canvasX = cam.x + cursorX / cam.scale;
				const canvasY = cam.y + cursorY / cam.scale;
				const newX = canvasX - cursorX / newScale;
				const newY = canvasY - cursorY / newScale;

				setCamera({ x: newX, y: newY, scale: newScale });
			} else {
				// Pan (regular scroll)
				setCamera({
					...cam,
					x: cam.x + e.deltaX / cam.scale,
					y: cam.y + e.deltaY / cam.scale,
				});
			}
		};

		el.addEventListener("wheel", onWheel, { passive: false });
		return () => el.removeEventListener("wheel", onWheel);
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
				const cam = cameraRef.current;
				const cursorX = e.clientX - rect.left;
				const cursorY = e.clientY - rect.top;
				const canvasX = cam.x + cursorX / cam.scale;
				const canvasY = cam.y + cursorY / cam.scale;
				onCanvasDrop({ x: canvasX, y: canvasY }, e);
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
			const cam = cameraRef.current;
			return {
				x: cam.x + (clientX - rect.left) / cam.scale,
				y: cam.y + (clientY - rect.top) / cam.scale,
			};
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
				const cam = cameraRef.current;
				const cursorX = e.clientX - rect.left;
				const cursorY = e.clientY - rect.top;
				onCanvasPointerDown(
					{ x: cam.x + cursorX / cam.scale, y: cam.y + cursorY / cam.scale },
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
			<div
				className={s.transform}
				style={{ transform }}
				data-canvas-scale={camera.scale}
			>
				{children}
			</div>
			<div className={s.zoomIndicator}>{zoomPercent}%</div>
		</div>
	);
}

/** Get the current viewport center in canvas coordinates */
export function getViewportCenter(camera: {
	x: number;
	y: number;
	scale: number;
}): { x: number; y: number } {
	const vw = window.innerWidth;
	const vh = window.innerHeight;
	return {
		x: camera.x + vw / (2 * camera.scale),
		y: camera.y + vh / (2 * camera.scale),
	};
}
