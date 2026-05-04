import { useCallback } from "react";
import s from "./OcifPath.module.css";
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

/** One `@ocif/path` node — a freehand stroke or any other vector
 *  path. The path commands are stored in node-local coords; the
 *  SVG container sits at the node's bounds so the path renders
 *  in place. The freehand pipeline produces a closed filled
 *  polygon (perfect-freehand outputs the stroke OUTLINE), so
 *  `fill_color` carries the drawn color while `stroke_*` are
 *  typically unused. */
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
	const path = node.path;
	if (!path) return null;
	const fill = path.fill_color ?? DEFAULT_FILL;
	const className = selected ? s.selected : s.path;
	return (
		<div
			data-testid="ocif-path"
			data-node-id={id}
			className={className}
			style={{
				position: "absolute",
				left: node.x,
				top: node.y,
				width: node.width,
				height: node.height,
				zIndex: Math.round(node.z),
			}}
		>
			<svg
				width={node.width}
				height={node.height}
				viewBox={`0 0 ${node.width} ${node.height}`}
				preserveAspectRatio="none"
				// SVG itself ignores hits — only the path catches
				// events. Otherwise the SVG's bounding rect (often
				// much larger than the visible ink) would intercept
				// clicks on transparent pixels.
				style={{ pointerEvents: "none" }}
			>
				<path
					d={path.path}
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
