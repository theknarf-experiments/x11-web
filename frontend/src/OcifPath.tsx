import { useCallback, useMemo } from "react";
import s from "./OcifPath.module.css";
import { pathBoundsFromPoints, svgPathFromPoints } from "./workspaceSync";
import type { OcifNode } from "./workspaceSync";

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

const DEFAULT_FILL = "#ffffff";

/** One `@ocif/path` node — a freehand stroke. The doc carries the
 *  raw input samples (flat `[x, y, p, ...]` triples in node-local
 *  coords); we run perfect-freehand here at render time to get the
 *  smoothed polygon outline and the matching SVG path string.
 *
 *  Bounds also come from the smoothed polygon — `node.x/y` is the
 *  anchor (the first sampled canvas point), but the polygon may
 *  extend in any direction from there, so the SVG container is
 *  positioned at `(node.x + bounds.minX, node.y + bounds.minY)`. */
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
	const fill = node.path.fill_color ?? DEFAULT_FILL;
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
					fill={fill}
					// Transparent wider stroke = invisible hit area
					// around the visible ink so thin strokes stay
					// selectable without pixel-perfect aim. `all`
					// pointer-events catches both the visible fill
					// and the transparent stroke.
					stroke={interactive ? "transparent" : "none"}
					strokeWidth={interactive ? 16 : 0}
					strokeLinecap="round"
					strokeLinejoin="round"
					className={interactive ? s.hitPath : undefined}
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
