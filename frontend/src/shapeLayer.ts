import { sampleQuadratic } from "@x11-web/canvas-core";
import type { ShapeArrow, ShapeBox, ShapePath } from "@x11-web/canvas-r3f";
import { getBoxToBoxArrow } from "perfect-arrows";
import type { OcifNode } from "./workspaceSync";
import { outlineFromPoints } from "./workspaceSync";

/** Defaults mirrored from the DOM components the GL layer replaces
 *  (OcifBox / OcifPath / OcifArrow) — the doc omits these, so both
 *  hit-testing DOM and painting GL must agree on them. */
const BOX_FILL = "transparent";
const BOX_STROKE = "#ffffff";
const BOX_STROKE_WIDTH = 2;
const BOX_RADIUS = 6;
const PATH_FILL = "#ffffff";
const ARROW_STROKE = "#ffffff";
const ARROW_STROKE_WIDTH = 2;
const HEAD_SIZE = 12;

export interface ShapeLayerData {
	boxes: ShapeBox[];
	paths: ShapePath[];
	arrows: ShapeArrow[];
}

/** Endpoint bounds for the perfect-arrows solver: the connected
 *  node's box, or a 1×1 point box at the cached coord for free
 *  endpoints — same treatment as OcifArrow, but in absolute canvas
 *  coords (the GL layer has no per-shape container to offset). */
function endpointBox(
	connected: OcifNode | undefined,
	x: number,
	y: number,
): { x: number; y: number; w: number; h: number } {
	return connected
		? {
				x: connected.x,
				y: connected.y,
				w: connected.width,
				h: connected.height,
			}
		: { x: x - 0.5, y: y - 0.5, w: 1, h: 1 };
}

/** Map the OCIF node set to GL shape-layer data. Window, text and
 *  markdown nodes stay DOM-rendered; the painted visuals of rects,
 *  freehand paths and arrows move to the GL layer while their DOM
 *  counterparts keep hit areas, selection chrome and text. */
export function buildShapeLayerData(
	nodes: Map<string, OcifNode>,
): ShapeLayerData {
	const boxes: ShapeBox[] = [];
	const paths: ShapePath[] = [];
	const arrows: ShapeArrow[] = [];

	for (const [id, node] of nodes) {
		if (node.window) continue;

		if (node.path) {
			const outline = outlineFromPoints(node.path.points);
			if (!outline) continue;
			paths.push({
				id,
				z: node.z,
				outline: outline.map(([x, y]) => [x + node.x, y + node.y] as const),
				fill: node.path.fill_color ?? PATH_FILL,
			});
			continue;
		}

		if (node.arrow) {
			const arrow = node.arrow;
			const startNode = node.edge?.start
				? nodes.get(node.edge.start)
				: undefined;
			const endNode = node.edge?.end ? nodes.get(node.edge.end) : undefined;
			const sBox = endpointBox(startNode, arrow.start_x, arrow.start_y);
			const eBox = endpointBox(endNode, arrow.end_x, arrow.end_y);
			const [sx, sy, cx, cy, ex, ey, ae] = getBoxToBoxArrow(
				sBox.x,
				sBox.y,
				sBox.w,
				sBox.h,
				eBox.x,
				eBox.y,
				eBox.w,
				eBox.h,
				{ bow: 0.2, stretch: 0.5, padEnd: HEAD_SIZE },
			);
			const heads: ShapeArrow["heads"] = [];
			if ((arrow.end_marker ?? "arrowhead") === "arrowhead") {
				heads.push({ x: ex, y: ey, angle: ae, size: HEAD_SIZE });
			}
			if ((arrow.start_marker ?? "none") === "arrowhead") {
				heads.push({
					x: sx,
					y: sy,
					angle: Math.atan2(sy - cy, sx - cx),
					size: HEAD_SIZE,
				});
			}
			arrows.push({
				id,
				z: node.z,
				polyline: sampleQuadratic(sx, sy, cx, cy, ex, ey),
				stroke: arrow.stroke_color ?? ARROW_STROKE,
				strokeWidth: arrow.stroke_width ?? ARROW_STROKE_WIDTH,
				heads,
			});
			continue;
		}

		if (node.rect) {
			boxes.push({
				id,
				x: node.x,
				y: node.y,
				width: node.width,
				height: node.height,
				z: node.z,
				fill: node.rect.fill_color ?? BOX_FILL,
				stroke: node.rect.stroke_color ?? BOX_STROKE,
				strokeWidth: node.rect.stroke_width ?? BOX_STROKE_WIDTH,
				radius: BOX_RADIUS,
			});
		}
	}

	return { boxes, paths, arrows };
}
