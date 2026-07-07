import { Canvas } from "@react-three/fiber";
import type { CameraStore } from "@x11-web/canvas-core";
import type { CSSProperties } from "react";
import { Boxes } from "./Boxes.tsx";
import { CAMERA_Z, CameraRig } from "./CameraRig.tsx";
import type { ShapeArrow, ShapeBox, ShapePath } from "./types.ts";

export interface CanvasShapeLayerProps {
	/** Shared camera store (canvas-core) — the same store the DOM
	 *  transform layer follows, so both layers move in lockstep. */
	camera: CameraStore;
	boxes?: ShapeBox[];
	paths?: ShapePath[];
	arrows?: ShapeArrow[];
	style?: CSSProperties;
	className?: string;
}

/** GL renderer for canvas vector shapes. Purely visual: the canvas
 *  ignores pointer events — hit-testing stays with the consumer
 *  (DOM hit areas or canvas-core geometric tests). Render-on-demand:
 *  frames are produced only on camera movement or data changes. */
export function CanvasShapeLayer({
	camera,
	boxes,
	paths,
	arrows,
	style,
	className,
}: CanvasShapeLayerProps) {
	return (
		<Canvas
			orthographic
			camera={{
				manual: true,
				position: [0, 0, CAMERA_Z],
				near: 0,
				far: CAMERA_Z * 2,
			}}
			frameloop="demand"
			flat
			dpr={[1, 2]}
			gl={{ antialias: true, alpha: true }}
			style={{ pointerEvents: "none", ...style }}
			className={className}
		>
			<CameraRig store={camera} />
			{boxes && boxes.length > 0 && <Boxes boxes={boxes} camera={camera} />}
			{/* paths / arrows land in the next slice */}
			{paths?.length ? null : null}
			{arrows?.length ? null : null}
		</Canvas>
	);
}
