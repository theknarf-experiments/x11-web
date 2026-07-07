import { useThree } from "@react-three/fiber";
import type { Camera, CameraStore } from "@x11-web/canvas-core";
import { useEffect } from "react";
import type * as THREE from "three";

/** How far the ortho camera sits in front of the z=0 plane. Shape
 *  stacking (`z`) maps directly to world z, so anything within
 *  ±CAMERA_Z is visible and CSS-z-index-like depth ordering falls
 *  out of the depth buffer. */
export const CAMERA_Z = 100_000;

/** Drives the orthographic frustum from the shared camera store so
 *  the GL layer tracks the DOM transform layer exactly. The frustum
 *  is set with `top < bottom` numerically, flipping three's default
 *  y-up into the canvas's y-down coordinate space (materials must
 *  be double-sided — the flip mirrors triangle winding). */
export function CameraRig({ store }: { store: CameraStore }) {
	const camera = useThree((s) => s.camera) as THREE.OrthographicCamera;
	const size = useThree((s) => s.size);
	const invalidate = useThree((s) => s.invalidate);

	useEffect(() => {
		const apply = (cam: Camera) => {
			camera.left = cam.x;
			camera.right = cam.x + size.width / cam.scale;
			camera.top = cam.y;
			camera.bottom = cam.y + size.height / cam.scale;
			camera.updateProjectionMatrix();
			invalidate();
		};
		apply(store.get());
		return store.subscribe(apply);
	}, [store, camera, size, invalidate]);

	return null;
}
