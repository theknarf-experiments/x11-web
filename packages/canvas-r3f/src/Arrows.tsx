import { useThree } from "@react-three/fiber";
import { useEffect, useMemo } from "react";
import * as THREE from "three";
import { parseColor } from "./color.ts";
import type { ShapeArrow } from "./types.ts";

/** Arrow strokes as triangulated ribbons around the sampled
 *  centerline (round caps + bevel-ish joins from overlapping round
 *  join discs), arrowheads as plain triangles. Everything is exact
 *  world-space geometry, so it scales crisply like the SVG it
 *  replaces — without the addon LineMaterial's screen-space
 *  resolution plumbing. */

const JOIN_SEGMENTS = 6;

/** Triangulate a stroked polyline: a quad per segment plus a fan of
 *  `JOIN_SEGMENTS` triangles at every vertex, which doubles as round
 *  joins and round end caps. */
function ribbonGeometry(
	polyline: Array<readonly [number, number]>,
	width: number,
): THREE.BufferGeometry | null {
	if (polyline.length < 2) return null;
	const hw = width / 2;
	const positions: number[] = [];

	const pushTri = (
		ax: number,
		ay: number,
		bx: number,
		by: number,
		cx: number,
		cy: number,
	) => {
		positions.push(ax, ay, 0, bx, by, 0, cx, cy, 0);
	};

	for (let i = 0; i + 1 < polyline.length; i++) {
		const [ax, ay] = polyline[i];
		const [bx, by] = polyline[i + 1];
		const len = Math.hypot(bx - ax, by - ay);
		if (len < 1e-6) continue;
		// Unit normal to the segment.
		const nx = -(by - ay) / len;
		const ny = (bx - ax) / len;
		pushTri(
			ax + nx * hw,
			ay + ny * hw,
			ax - nx * hw,
			ay - ny * hw,
			bx + nx * hw,
			by + ny * hw,
		);
		pushTri(
			ax - nx * hw,
			ay - ny * hw,
			bx - nx * hw,
			by - ny * hw,
			bx + nx * hw,
			by + ny * hw,
		);
	}

	// Round discs at every vertex: caps at the ends, joins between
	// segments. Overdraw between a disc and its segments is invisible
	// for opaque strokes (and negligible for translucent ones).
	for (const [cx, cy] of polyline) {
		for (let s = 0; s < JOIN_SEGMENTS; s++) {
			const a0 = (s / JOIN_SEGMENTS) * Math.PI * 2;
			const a1 = ((s + 1) / JOIN_SEGMENTS) * Math.PI * 2;
			pushTri(
				cx,
				cy,
				cx + Math.cos(a0) * hw,
				cy + Math.sin(a0) * hw,
				cx + Math.cos(a1) * hw,
				cy + Math.sin(a1) * hw,
			);
		}
	}

	const geo = new THREE.BufferGeometry();
	geo.setAttribute(
		"position",
		new THREE.BufferAttribute(new Float32Array(positions), 3),
	);
	return geo;
}

function headGeometry(heads: ShapeArrow["heads"]): THREE.BufferGeometry | null {
	if (heads.length === 0) return null;
	const positions: number[] = [];
	for (const head of heads) {
		const cos = Math.cos(head.angle);
		const sin = Math.sin(head.angle);
		// Local triangle: base at the anchor, apex `size` forward —
		// matches the SVG polygon `0,-s/2 s,0 0,s/2`.
		const local: Array<[number, number]> = [
			[0, -head.size / 2],
			[head.size, 0],
			[0, head.size / 2],
		];
		for (const [lx, ly] of local) {
			positions.push(
				head.x + lx * cos - ly * sin,
				head.y + lx * sin + ly * cos,
				0,
			);
		}
	}
	const geo = new THREE.BufferGeometry();
	geo.setAttribute(
		"position",
		new THREE.BufferAttribute(new Float32Array(positions), 3),
	);
	return geo;
}

function ArrowMesh({ arrow }: { arrow: ShapeArrow }) {
	const invalidate = useThree((s) => s.invalidate);

	const ribbon = useMemo(
		() => ribbonGeometry(arrow.polyline, arrow.strokeWidth),
		[arrow.polyline, arrow.strokeWidth],
	);
	const heads = useMemo(() => headGeometry(arrow.heads), [arrow.heads]);
	const material = useMemo(() => {
		const c = parseColor(arrow.stroke);
		const mat = new THREE.MeshBasicMaterial({
			transparent: true,
			opacity: c.a,
			side: THREE.DoubleSide,
			toneMapped: false,
		});
		mat.color.setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);
		return mat;
	}, [arrow.stroke]);

	useEffect(() => {
		invalidate();
		return () => {
			ribbon?.dispose();
			heads?.dispose();
			material.dispose();
		};
	}, [ribbon, heads, material, invalidate]);

	return (
		<group position={[0, 0, arrow.z]} renderOrder={arrow.z}>
			{ribbon && (
				<mesh geometry={ribbon} material={material} frustumCulled={false} />
			)}
			{heads && (
				<mesh geometry={heads} material={material} frustumCulled={false} />
			)}
		</group>
	);
}

export function Arrows({ arrows }: { arrows: ShapeArrow[] }) {
	return (
		<>
			{arrows.map((a) => (
				<ArrowMesh key={a.id} arrow={a} />
			))}
		</>
	);
}
