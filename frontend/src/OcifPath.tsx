import { useCallback, useMemo } from "react";
import s from "./OcifPath.module.css";
import type { OcifNode } from "./workspaceSync";
import { pathBoundsFromPoints, svgPathFromPoints } from "./workspaceSync";

interface OcifPathProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	/** When true (pointer mode) the path intercepts pointerdown for
	 *  select / drag-to-move. When false (any draw mode) the event
	 *  bubbles so a drag-create gesture can start through it. */
	interactive: boolean;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
}

/** One `@ocif/path` node's interactive hit area. The visible ink is
 *  painted by the GL shape layer (see `shapeLayer.ts`); this SVG
 *  carries only the invisible fat hit stroke for select/drag, which
 *  doubles as the hover/selection halo by tinting on those states.
 *
 *  Bounds come from the smoothed polygon — `node.x/y` is the anchor
 *  (the first sampled canvas point), but the polygon may extend in
 *  any direction from there, so the SVG container is positioned at
 *  `(node.x + bounds.minX, node.y + bounds.minY)`. */
export function OcifPath({
	id,
	node,
	selected,
	interactive,
	onPointerDown,
}: OcifPathProps) {
	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (!interactive) return;
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown, interactive],
	);
	const points = node.path?.points;
	const pathStr = useMemo(
		() => (points ? svgPathFromPoints(points) : null),
		[points],
	);
	const bounds = useMemo(
		() => (points ? pathBoundsFromPoints(points) : null),
		[points],
	);
	if (!node.path || !pathStr || !bounds) return null;
	const className = selected ? s.selected : s.path;
	return (
		<div
			data-testid="ocif-path"
			data-node-id={id}
			className={className}
			style={{
				position: "absolute",
				left: node.x + bounds.minX,
				top: node.y + bounds.minY,
				width: bounds.width,
				height: bounds.height,
				zIndex: Math.round(node.z),
			}}
		>
			<svg
				width={bounds.width}
				height={bounds.height}
				viewBox={`${bounds.minX} ${bounds.minY} ${bounds.width} ${bounds.height}`}
				preserveAspectRatio="none"
				// SVG itself ignores hits — only the path catches
				// events. Otherwise the SVG's bounding rect (often
				// much larger than the visible ink) would intercept
				// clicks on transparent pixels.
				style={{ pointerEvents: "none", overflow: "visible" }}
			>
				<path
					d={pathStr}
					fill="transparent"
					// Transparent wider stroke = invisible hit area
					// around the (GL-painted) ink so thin strokes stay
					// selectable without pixel-perfect aim. `all`
					// pointer-events catches both the fill region and
					// the stroke band. Hover/selected tint this stroke
					// (see the module CSS) as the halo affordance.
					stroke="transparent"
					strokeWidth={16}
					strokeLinecap="round"
					strokeLinejoin="round"
					className={s.hitPath}
					style={{
						pointerEvents: interactive ? "all" : "none",
						cursor: interactive ? "move" : undefined,
					}}
					onPointerDown={handlePointerDown}
				/>
			</svg>
		</div>
	);
}
