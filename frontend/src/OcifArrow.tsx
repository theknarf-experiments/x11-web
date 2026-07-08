import { getBoxToBoxArrow } from "perfect-arrows";
import { useMemo } from "react";
import s from "./OcifArrow.module.css";
import type { OcifNode } from "./workspaceSync";

interface OcifArrowProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	/** When true (pointer mode) the arrow's hit path intercepts
	 *  pointerdown for select / drag-to-move. When false (box /
	 *  arrow draw mode) the event bubbles so a drag gesture can
	 *  start through it. */
	interactive: boolean;
	/** Connected-arrow lookup: when `node.edge.start` or
	 *  `node.edge.end` is set, the renderer pulls the connected
	 *  node's bounds from this map and computes geometry against
	 *  them (so the connection follows the boxes). */
	nodes: Map<string, OcifNode>;
	/** Which endpoint, if any, is currently being drag-relocated.
	 *  Renders that handle in a distinctive "dragging" color so the
	 *  user has visual feedback that they're moving it. */
	draggingEnd: "start" | "end" | null;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	onEndpointPointerDown: (
		id: string,
		end: "start" | "end",
		e: React.PointerEvent,
	) => void;
}

const HEAD_SIZE = 12;
/** Invisible-but-clickable padding around the line so users don't
 *  have to hit the 2px stroke pixel-perfect. */
const HIT_STROKE_WIDTH = 14;
/** Outer padding from arrow geometry to SVG container — must be
 *  larger than the bow + arrowhead so the curve never gets
 *  clipped by the SVG viewport. */
const SVG_MARGIN = HEAD_SIZE + 24;

function endpointClass(dragging: boolean, attached: boolean): string {
	if (dragging) return s.endpointDragging;
	return attached ? s.endpointAttached : s.endpoint;
}

/** One arrow node. Each endpoint independently follows either a
 *  connected box (via `node.edge.start` / `node.edge.end`) or a
 *  cached canvas coord (`node.arrow.start_x/y` / `end_x/y`). The
 *  geometry funnels through a single `getBoxToBoxArrow` call by
 *  treating free endpoints as 1×1 point boxes — keeps the path
 *  uniform regardless of whether 0, 1, or 2 ends are anchored. */
export function OcifArrow({
	id,
	node,
	selected,
	interactive,
	nodes,
	draggingEnd,
	onPointerDown,
	onEndpointPointerDown,
}: OcifArrowProps) {
	const layout = useMemo(() => {
		const arrow = node.arrow;
		if (!arrow) return null;
		const startNode = node.edge?.start ? nodes.get(node.edge.start) : undefined;
		const endNode = node.edge?.end ? nodes.get(node.edge.end) : undefined;
		// Determine effective bounding box per endpoint. Free
		// endpoints become 1×1 point boxes at the cached coord.
		const sBox = startNode
			? {
					x: startNode.x,
					y: startNode.y,
					w: startNode.width,
					h: startNode.height,
				}
			: {
					x: arrow.start_x - 0.5,
					y: arrow.start_y - 0.5,
					w: 1,
					h: 1,
				};
		const eBox = endNode
			? {
					x: endNode.x,
					y: endNode.y,
					w: endNode.width,
					h: endNode.height,
				}
			: {
					x: arrow.end_x - 0.5,
					y: arrow.end_y - 0.5,
					w: 1,
					h: 1,
				};
		// SVG container covers the union of both boxes plus margin.
		const minX = Math.min(sBox.x, eBox.x) - SVG_MARGIN;
		const minY = Math.min(sBox.y, eBox.y) - SVG_MARGIN;
		const maxX = Math.max(sBox.x + sBox.w, eBox.x + eBox.w) + SVG_MARGIN;
		const maxY = Math.max(sBox.y + sBox.h, eBox.y + eBox.h) + SVG_MARGIN;
		const width = maxX - minX;
		const height = maxY - minY;
		const [sx, sy, cx, cy, ex, ey] = getBoxToBoxArrow(
			sBox.x - minX,
			sBox.y - minY,
			sBox.w,
			sBox.h,
			eBox.x - minX,
			eBox.y - minY,
			eBox.w,
			eBox.h,
			{ bow: 0.2, stretch: 0.5, padEnd: HEAD_SIZE },
		);
		return {
			minX,
			minY,
			width,
			height,
			path: `M${sx},${sy} Q${cx},${cy} ${ex},${ey}`,
			endX: ex,
			endY: ey,
			startX: sx,
			startY: sy,
			startAttached: !!startNode,
			endAttached: !!endNode,
		};
	}, [node.arrow, node.edge, nodes]);

	if (!node.arrow || !layout) return null;

	return (
		<div
			data-testid="ocif-arrow"
			data-node-id={id}
			className={s.container}
			style={{
				left: layout.minX,
				top: layout.minY,
				width: layout.width,
				height: layout.height,
				zIndex: Math.round(node.z),
			}}
		>
			<svg
				className={s.svg}
				width={layout.width}
				height={layout.height}
				viewBox={`0 0 ${layout.width} ${layout.height}`}
			>
				{/* Wide invisible hit path along the (GL-painted) stroke
				 *  so the user can click anywhere near the line. The
				 *  visible curve and arrowheads render in the GL shape
				 *  layer (see shapeLayer.ts). */}
				<path
					d={layout.path}
					fill="none"
					stroke="transparent"
					strokeWidth={HIT_STROKE_WIDTH}
					className={s.hit}
					style={interactive ? undefined : { pointerEvents: "none" }}
					onPointerDown={(e) => {
						if (!interactive) return;
						e.stopPropagation();
						onPointerDown(id, e);
					}}
				/>
				{selected && interactive && (
					<>
						<circle
							cx={layout.startX}
							cy={layout.startY}
							r={6}
							className={endpointClass(
								draggingEnd === "start",
								layout.startAttached,
							)}
							onPointerDown={(e) => {
								e.stopPropagation();
								onEndpointPointerDown(id, "start", e);
							}}
						/>
						<circle
							cx={layout.endX}
							cy={layout.endY}
							r={6}
							className={endpointClass(
								draggingEnd === "end",
								layout.endAttached,
							)}
							onPointerDown={(e) => {
								e.stopPropagation();
								onEndpointPointerDown(id, "end", e);
							}}
						/>
					</>
				)}
			</svg>
		</div>
	);
}
