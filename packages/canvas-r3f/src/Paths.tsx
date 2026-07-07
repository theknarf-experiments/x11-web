import { useThree } from "@react-three/fiber";
import { useEffect, useMemo } from "react";
import * as THREE from "three";
import { parseColor } from "./color.ts";
import type { ShapePath } from "./types.ts";

/** A freehand stroke arrives as a closed outline polygon (the
 *  consumer runs perfect-freehand); filling it is plain earcut
 *  triangulation via ShapeGeometry. Exact geometry at every zoom —
 *  no rasterization to go stale. */
function PathMesh({ path }: { path: ShapePath }) {
	const invalidate = useThree((s) => s.invalidate);

	const geometry = useMemo(() => {
		const shape = new THREE.Shape();
		const pts = path.outline;
		if (pts.length < 3) return null;
		shape.moveTo(pts[0][0], pts[0][1]);
		for (let i = 1; i < pts.length; i++) {
			shape.lineTo(pts[i][0], pts[i][1]);
		}
		shape.closePath();
		return new THREE.ShapeGeometry(shape);
	}, [path.outline]);

	const material = useMemo(() => {
		const c = parseColor(path.fill);
		const mat = new THREE.MeshBasicMaterial({
			transparent: true,
			opacity: c.a,
			side: THREE.DoubleSide,
			toneMapped: false,
		});
		mat.color.setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);
		return mat;
	}, [path.fill]);

	useEffect(() => {
		invalidate();
		return () => {
			geometry?.dispose();
			material.dispose();
		};
	}, [geometry, material, invalidate]);

	if (!geometry) return null;
	return (
		<mesh
			geometry={geometry}
			material={material}
			position={[0, 0, path.z]}
			renderOrder={path.z}
			frustumCulled={false}
		/>
	);
}

export function Paths({ paths }: { paths: ShapePath[] }) {
	return (
		<>
			{paths.map((p) => (
				<PathMesh key={p.id} path={p} />
			))}
		</>
	);
}
