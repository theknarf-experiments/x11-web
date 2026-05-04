import { useMemo } from "react";
import { getArrow, getBoxToBoxArrow } from "perfect-arrows";
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
	/** Connected-arrow lookup: if `node.edge` is set, the renderer
	 *  pulls the start/end node bounds from this map and computes
	 *  geometry via `getBoxToBoxArrow` so the arrow follows its
	 *  endpoints when they move or resize. */
	nodes: Map<string, OcifNode>;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	onEndpointPointerDown: (
		id: string,
		end: "start" | "end",
		e: React.PointerEvent,
	) => void;
}

const DEFAULT_STROKE = "#ffffff";
const DEFAULT_STROKE_WIDTH = 2;
const HEAD_SIZE = 12;
/** Invisible-but-clickable padding around the line so users don't
 *  have to hit the 2px stroke pixel-perfect. */
const HIT_STROKE_WIDTH = 14;
/** Outer padding from arrow geometry to SVG container — must be
 *  larger than the bow + arrowhead so the curve never gets
 *  clipped by the SVG viewport. */
const SVG_MARGIN = HEAD_SIZE + 24;

/** One arrow node. Geometry varies by which extensions are set:
 *  - `edge` ext: connection between two nodes; uses
 *    `getBoxToBoxArrow` against their bounds and re-renders when
 *    those bounds change (the connection follows the boxes).
 *  - `arrow` ext only: free-floating; uses `getArrow` against the
 *    stored start/end coords.
 *
 *  In both cases the SVG covers a bounding rect around the
 *  endpoints plus margin for the arrowhead and curve. */
export function OcifArrow({
	id,
	node,
	selected,
	interactive,
	nodes,
	onPointerDown,
	onEndpointPointerDown,
}: OcifArrowProps) {
	const layout = useMemo(() => {
		const arrow = node.arrow;
		const edge = node.edge;
		if (!arrow) return null;

		// Resolve connected-edge endpoints from the boxes' bounds.
		// If a referenced box has gone missing (was deleted before
		// us — `deleteOcifNode` cascade should have caught it but
		// inbound sync timing may briefly leave us dangling), fall
		// back to the cached arrow coords.
		let path: string;
		let headX: number;
		let headY: number;
		let headDeg: number;
		let visibleStartX: number;
		let visibleStartY: number;
		let visibleEndX: number;
		let visibleEndY: number;

		if (edge) {
			const startNode = nodes.get(edge.start);
			const endNode = nodes.get(edge.end);
			if (!startNode || !endNode) return null;
			const margin =
				SVG_MARGIN +
				Math.max(startNode.width, startNode.height, endNode.width, endNode.height) /
					2;
			const minX =
				Math.min(startNode.x, endNode.x) - margin;
			const minY =
				Math.min(startNode.y, endNode.y) - margin;
			const maxX =
				Math.max(
					startNode.x + startNode.width,
					endNode.x + endNode.width,
				) + margin;
			const maxY =
				Math.max(
					startNode.y + startNode.height,
					endNode.y + endNode.height,
				) + margin;
			const width = maxX - minX;
			const height = maxY - minY;
			const [sx, sy, cx, cy, ex, ey, ae] = getBoxToBoxArrow(
				startNode.x - minX,
				startNode.y - minY,
				startNode.width,
				startNode.height,
				endNode.x - minX,
				endNode.y - minY,
				endNode.width,
				endNode.height,
				{ bow: 0.2, stretch: 0.5, padEnd: HEAD_SIZE },
			);
			path = `M${sx},${sy} Q${cx},${cy} ${ex},${ey}`;
			headX = ex;
			headY = ey;
			headDeg = ae * (180 / Math.PI);
			visibleStartX = sx;
			visibleStartY = sy;
			visibleEndX = ex;
			visibleEndY = ey;
			return {
				minX,
				minY,
				width,
				height,
				path,
				headX,
				headY,
				headDeg,
				startX: visibleStartX,
				startY: visibleStartY,
				endX: visibleEndX,
				endY: visibleEndY,
				connected: true,
			};
		}

		// Free-floating arrow.
		const minX = Math.min(arrow.start_x, arrow.end_x) - SVG_MARGIN;
		const minY = Math.min(arrow.start_y, arrow.end_y) - SVG_MARGIN;
		const maxX = Math.max(arrow.start_x, arrow.end_x) + SVG_MARGIN;
		const maxY = Math.max(arrow.start_y, arrow.end_y) + SVG_MARGIN;
		const width = maxX - minX;
		const height = maxY - minY;
		const sx0 = arrow.start_x - minX;
		const sy0 = arrow.start_y - minY;
		const ex0 = arrow.end_x - minX;
		const ey0 = arrow.end_y - minY;
		const [sx, sy, cx, cy, ex, ey, ae] = getArrow(sx0, sy0, ex0, ey0, {
			bow: 0.2,
			stretch: 0.5,
			padEnd: HEAD_SIZE,
		});
		path = `M${sx},${sy} Q${cx},${cy} ${ex},${ey}`;
		headX = ex;
		headY = ey;
		headDeg = ae * (180 / Math.PI);
		visibleStartX = sx0;
		visibleStartY = sy0;
		visibleEndX = ex0;
		visibleEndY = ey0;
		return {
			minX,
			minY,
			width,
			height,
			path,
			headX,
			headY,
			headDeg,
			startX: visibleStartX,
			startY: visibleStartY,
			endX: visibleEndX,
			endY: visibleEndY,
			connected: false,
		};
	}, [node.arrow, node.edge, nodes]);

	if (!node.arrow || !layout) return null;

	const stroke = node.arrow.stroke_color ?? DEFAULT_STROKE;
	const strokeWidth = node.arrow.stroke_width ?? DEFAULT_STROKE_WIDTH;

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
				{/* Wide invisible hit path under the visible stroke so
				 *  the user can click anywhere near the line. */}
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
				<path
					d={layout.path}
					fill="none"
					stroke={stroke}
					strokeWidth={strokeWidth}
					strokeLinecap="round"
					pointerEvents="none"
				/>
				<polygon
					points={`0,-${HEAD_SIZE / 2} ${HEAD_SIZE},0 0,${HEAD_SIZE / 2}`}
					fill={stroke}
					transform={`translate(${layout.headX},${layout.headY}) rotate(${layout.headDeg})`}
					pointerEvents="none"
				/>
				{selected && interactive && !layout.connected && (
					<>
						<circle
							cx={layout.startX}
							cy={layout.startY}
							r={6}
							className={s.endpoint}
							onPointerDown={(e) => {
								e.stopPropagation();
								onEndpointPointerDown(id, "start", e);
							}}
						/>
						<circle
							cx={layout.endX}
							cy={layout.endY}
							r={6}
							className={s.endpoint}
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
