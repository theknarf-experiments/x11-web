import { type ReactNode, useCallback, useRef, useState } from "react";
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
	const isPanning = useRef(false);

	// Pan: pointer down on background starts panning
	const handlePointerDown = useCallback((e: React.PointerEvent) => {
		// Only pan on direct clicks on the viewport (not on children)
		if (e.target !== e.currentTarget) return;
		isPanning.current = true;
		const startX = e.clientX;
		const startY = e.clientY;
		const cam = { ...cameraRef.current };
		const target = e.currentTarget;
		target.setPointerCapture(e.pointerId);

		const onPointerMove = (ev: Event) => {
			if (!isPanning.current) return;
			const { clientX, clientY } = ev as PointerEvent;
			const dx = clientX - startX;
			const dy = clientY - startY;
			setCamera({
				...cam,
				x: cam.x - dx / cam.scale,
				y: cam.y - dy / cam.scale,
			});
		};

		const onPointerUp = () => {
			isPanning.current = false;
			target.removeEventListener("pointermove", onPointerMove);
			target.removeEventListener("pointerup", onPointerUp);
		};

		target.addEventListener("pointermove", onPointerMove);
		target.addEventListener("pointerup", onPointerUp);
	}, []);

	// Zoom: wheel zooms around cursor position
	const handleWheel = useCallback((e: React.WheelEvent) => {
		e.preventDefault();
		const cam = cameraRef.current;
		const zoomFactor = e.deltaY > 0 ? 0.9 : 1.1;
		const newScale = Math.min(
			MAX_SCALE,
			Math.max(MIN_SCALE, cam.scale * zoomFactor),
		);

		// Zoom toward cursor: keep the point under the cursor fixed
		const rect = e.currentTarget.getBoundingClientRect();
		const cursorX = e.clientX - rect.left;
		const cursorY = e.clientY - rect.top;

		// Point in canvas space under cursor before zoom
		const canvasX = cam.x + cursorX / cam.scale;
		const canvasY = cam.y + cursorY / cam.scale;

		// After zoom, the same canvas point should be under the cursor
		const newX = canvasX - cursorX / newScale;
		const newY = canvasY - cursorY / newScale;

		setCamera({ x: newX, y: newY, scale: newScale });
	}, []);

	const transform = `scale(${camera.scale}) translate(${-camera.x}px, ${-camera.y}px)`;

	return (
		<div
			className={s.viewport}
			onPointerDown={handlePointerDown}
			onWheel={handleWheel}
			data-testid="infinite-canvas"
		>
			<div
				className={s.transform}
				style={{ transform }}
				data-canvas-scale={camera.scale}
			>
				{children}
			</div>
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
