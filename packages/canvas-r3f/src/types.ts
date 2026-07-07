/** Renderer-facing shape data, all in canvas coordinates (y-down).
 *  Geometry is pre-computed by the consumer (smoothed freehand
 *  outlines, sampled arrow curves) so this package stays free of
 *  app-specific geometry libraries. */

export interface ShapeBox {
	id: string;
	x: number;
	y: number;
	width: number;
	height: number;
	/** Stacking position — mapped to depth, matching CSS z-index
	 *  semantics across all shape types. */
	z: number;
	fill: string;
	stroke: string;
	strokeWidth: number;
	/** Corner radius of the outer edge. */
	radius: number;
}

export interface ShapePath {
	id: string;
	z: number;
	/** Closed outline polygon (canvas coords) to fill. */
	outline: Array<readonly [number, number]>;
	fill: string;
}

export interface ShapeArrow {
	id: string;
	z: number;
	/** Sampled centerline (canvas coords). */
	polyline: Array<readonly [number, number]>;
	stroke: string;
	strokeWidth: number;
	/** Arrowhead triangles: tip position + direction in radians. */
	heads: Array<{ x: number; y: number; angle: number; size: number }>;
}
