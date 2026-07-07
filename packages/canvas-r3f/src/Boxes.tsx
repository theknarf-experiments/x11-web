import { useThree } from "@react-three/fiber";
import type { CameraStore } from "@x11-web/canvas-core";
import { useEffect, useMemo } from "react";
import * as THREE from "three";
import { parseColor } from "./color.ts";
import type { ShapeBox } from "./types.ts";

/** All boxes render as ONE instanced quad with a rounded-rect SDF
 *  fragment shader: analytically anti-aliased, so strokes and
 *  corners stay crisp at any zoom — including mid-gesture, where
 *  DOM/SVG shows the compositor's stale raster. Per-instance
 *  attributes carry rect, depth, fill/stroke RGBA, stroke width and
 *  corner radius. */

const VERT = /* glsl */ `
attribute vec4 iRect;      // x, y, width, height (canvas coords)
attribute float iZ;
attribute vec4 iFill;
attribute vec4 iStroke;
attribute vec2 iParams;    // strokeWidth, cornerRadius
uniform float uPixel;      // canvas units per screen pixel (1/scale)
varying vec2 vLocal;
varying vec2 vHalf;
varying vec4 vFill;
varying vec4 vStroke;
varying vec2 vParams;

void main() {
	vHalf = iRect.zw * 0.5;
	vFill = iFill;
	vStroke = iStroke;
	vParams = iParams;
	// Pad the quad ~2 screen px past the rect so the outer-edge
	// anti-aliasing band never gets clipped by the quad boundary.
	float pad = uPixel * 2.0;
	vec2 quad = position.xy * (iRect.zw + pad * 2.0);
	vLocal = quad;
	vec2 world = iRect.xy + vHalf + quad;
	gl_Position = projectionMatrix * modelViewMatrix * vec4(world, iZ, 1.0);
}
`;

const FRAG = /* glsl */ `
precision highp float;
varying vec2 vLocal;
varying vec2 vHalf;
varying vec4 vFill;
varying vec4 vStroke;
varying vec2 vParams;

float sdRoundBox(vec2 p, vec2 b, float r) {
	vec2 q = abs(p) - b + r;
	return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
	float r = min(vParams.y, min(vHalf.x, vHalf.y));
	// d < 0 inside the outer rounded bounds; the stroke occupies the
	// band -strokeWidth..0 (outer edge on the bounds — same geometry
	// as the DOM renderer), fill everything further in. Offsetting
	// an SDF by +strokeWidth is the exact inset rounded rect.
	float d = sdRoundBox(vLocal, vHalf, r);
	float aa = max(fwidth(d), 1e-5);
	float outer = 1.0 - smoothstep(-aa * 0.5, aa * 0.5, d);
	float inner = 1.0 - smoothstep(-aa * 0.5, aa * 0.5, d + vParams.x);
	float strokeA = (outer - inner) * vStroke.a;
	float fillA = inner * vFill.a;
	float a = strokeA + fillA;
	// Discard non-ink fragments so they neither write depth (which
	// would occlude shapes behind transparent interiors) nor blend.
	if (a < 0.003) discard;
	vec3 rgb = (vStroke.rgb * strokeA + vFill.rgb * fillA) / a;
	gl_FragColor = vec4(rgb, a);
}
`;

interface InstanceBuffers {
	capacity: number;
	rect: THREE.InstancedBufferAttribute;
	z: THREE.InstancedBufferAttribute;
	fill: THREE.InstancedBufferAttribute;
	stroke: THREE.InstancedBufferAttribute;
	params: THREE.InstancedBufferAttribute;
}

function allocate(
	geometry: THREE.InstancedBufferGeometry,
	capacity: number,
): InstanceBuffers {
	const make = (itemSize: number) => {
		const attr = new THREE.InstancedBufferAttribute(
			new Float32Array(capacity * itemSize),
			itemSize,
		);
		attr.setUsage(THREE.DynamicDrawUsage);
		return attr;
	};
	const buffers: InstanceBuffers = {
		capacity,
		rect: make(4),
		z: make(1),
		fill: make(4),
		stroke: make(4),
		params: make(2),
	};
	geometry.setAttribute("iRect", buffers.rect);
	geometry.setAttribute("iZ", buffers.z);
	geometry.setAttribute("iFill", buffers.fill);
	geometry.setAttribute("iStroke", buffers.stroke);
	geometry.setAttribute("iParams", buffers.params);
	return buffers;
}

export function Boxes({
	boxes,
	camera,
}: {
	boxes: ShapeBox[];
	camera: CameraStore;
}) {
	const invalidate = useThree((s) => s.invalidate);

	const { geometry, material, state } = useMemo(() => {
		const geo = new THREE.InstancedBufferGeometry();
		const quad = new THREE.PlaneGeometry(1, 1);
		geo.index = quad.index;
		geo.setAttribute("position", quad.getAttribute("position"));
		const mat = new THREE.ShaderMaterial({
			vertexShader: VERT,
			fragmentShader: FRAG,
			transparent: true,
			side: THREE.DoubleSide,
			uniforms: { uPixel: { value: 1 } },
		});
		return {
			geometry: geo,
			material: mat,
			state: { buffers: null as InstanceBuffers | null },
		};
	}, []);

	useEffect(
		() => () => {
			geometry.dispose();
			material.dispose();
		},
		[geometry, material],
	);

	// Track the zoom for constant-screen-size AA padding.
	useEffect(() => {
		const apply = () => {
			material.uniforms.uPixel.value = 1 / camera.get().scale;
		};
		apply();
		return camera.subscribe(apply);
	}, [camera, material]);

	useEffect(() => {
		// Back-to-front instance order so semi-transparent boxes blend
		// correctly against each other (depth handles the rest).
		const sorted = [...boxes].sort((a, b) => a.z - b.z);
		if (!state.buffers || state.buffers.capacity < sorted.length) {
			let capacity = Math.max(64, state.buffers?.capacity ?? 0);
			while (capacity < sorted.length) capacity *= 2;
			state.buffers = allocate(geometry, capacity);
		}
		const b = state.buffers;
		for (let i = 0; i < sorted.length; i++) {
			const box = sorted[i];
			b.rect.array.set([box.x, box.y, box.width, box.height], i * 4);
			(b.z.array as Float32Array)[i] = box.z;
			const fill = parseColor(box.fill);
			b.fill.array.set([fill.r, fill.g, fill.b, fill.a], i * 4);
			const stroke = parseColor(box.stroke);
			b.stroke.array.set([stroke.r, stroke.g, stroke.b, stroke.a], i * 4);
			b.params.array.set([box.strokeWidth, box.radius], i * 2);
		}
		for (const attr of [b.rect, b.z, b.fill, b.stroke, b.params]) {
			attr.needsUpdate = true;
		}
		geometry.instanceCount = sorted.length;
		invalidate();
	}, [boxes, geometry, state, invalidate]);

	return <mesh geometry={geometry} material={material} frustumCulled={false} />;
}
