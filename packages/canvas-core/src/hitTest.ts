/** Geometric hit-testing in canvas coordinates — renderer-agnostic,
 *  so shapes drawn by a GL layer (which has no DOM events) hit-test
 *  identically to their old DOM/SVG counterparts. */

export type Vec2 = readonly [number, number];

/** Point-in-rounded-rect. `radius` is clamped to half the smaller
 *  side, matching CSS/SVG corner behavior. */
export function pointInRoundedRect(
	px: number,
	py: number,
	x: number,
	y: number,
	width: number,
	height: number,
	radius = 0,
): boolean {
	if (px < x || py < y || px > x + width || py > y + height) return false;
	const r = Math.min(radius, width / 2, height / 2);
	if (r <= 0) return true;
	// Inside the straight bands → hit; otherwise test the corner disc.
	const dx = Math.max(x + r - px, px - (x + width - r), 0);
	const dy = Math.max(y + r - py, py - (y + height - r), 0);
	return dx * dx + dy * dy <= r * r;
}

/** Distance from a point to a line segment. */
export function distToSegment(
	px: number,
	py: number,
	ax: number,
	ay: number,
	bx: number,
	by: number,
): number {
	const abx = bx - ax;
	const aby = by - ay;
	const lenSq = abx * abx + aby * aby;
	const t =
		lenSq === 0
			? 0
			: Math.max(0, Math.min(1, ((px - ax) * abx + (py - ay) * aby) / lenSq));
	return Math.hypot(px - (ax + t * abx), py - (ay + t * aby));
}

/** Distance from a point to an open polyline. Infinity for fewer
 *  than two points. */
export function distToPolyline(px: number, py: number, pts: Vec2[]): number {
	let best = Number.POSITIVE_INFINITY;
	for (let i = 0; i + 1 < pts.length; i++) {
		const d = distToSegment(
			px,
			py,
			pts[i][0],
			pts[i][1],
			pts[i + 1][0],
			pts[i + 1][1],
		);
		if (d < best) best = d;
	}
	return best;
}

/** Even-odd point-in-polygon test over a closed polygon (last point
 *  implicitly connects to the first). */
export function pointInPolygon(px: number, py: number, poly: Vec2[]): boolean {
	let inside = false;
	for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
		const [xi, yi] = poly[i];
		const [xj, yj] = poly[j];
		if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) {
			inside = !inside;
		}
	}
	return inside;
}

/** Sample a quadratic Bézier (the shape `perfect-arrows` produces)
 *  into a polyline for stroke hit-testing and GL rendering. */
export function sampleQuadratic(
	sx: number,
	sy: number,
	cx: number,
	cy: number,
	ex: number,
	ey: number,
	segments = 24,
): Vec2[] {
	const pts: Vec2[] = [];
	for (let i = 0; i <= segments; i++) {
		const t = i / segments;
		const mt = 1 - t;
		pts.push([
			mt * mt * sx + 2 * mt * t * cx + t * t * ex,
			mt * mt * sy + 2 * mt * t * cy + t * t * ey,
		]);
	}
	return pts;
}
