import { type ReactNode, useEffect, useRef, useState } from "react";
import s from "./InfiniteCanvas.module.css";

interface Camera {
	x: number;
	y: number;
	scale: number;
}

interface InfiniteCanvasProps {
	children: ReactNode;
}

const MIN_SCALE = 0.1;
const MAX_SCALE = 3;

export function InfiniteCanvas({ children }: InfiniteCanvasProps) {
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

	return (
		<div ref={viewportRef} className={s.viewport} data-testid="infinite-canvas">
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
