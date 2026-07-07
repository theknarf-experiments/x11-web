import type { Meta, StoryObj } from "@storybook/react-vite";
import { createCameraStore } from "@x11-web/canvas-core";
import { CanvasShapeLayer, type ShapeBox } from "@x11-web/canvas-r3f";
import { useMemo } from "react";
import { InfiniteCanvas } from "./InfiniteCanvas.tsx";

/** Integration of the GL shape layer (`@x11-web/canvas-r3f`) as an
 *  InfiniteCanvas underlay: one canvas-core camera store drives the
 *  DOM transform layer and the ortho camera in lockstep. Zoom in
 *  (⌘/ctrl + scroll) — SDF strokes stay crisp even mid-gesture. */
const meta: Meta = {
	title: "InfiniteCanvas/ShapeLayer",
	parameters: { layout: "fullscreen" },
};
export default meta;

const BOXES: ShapeBox[] = [
	// A spread of stroke widths / radii / fills, plus a DOM reference
	// box rendered at the same spot in the overlay for comparison.
	{
		id: "plain",
		x: 100,
		y: 80,
		width: 180,
		height: 110,
		z: 1,
		fill: "transparent",
		stroke: "#ffffff",
		strokeWidth: 2,
		radius: 6,
	},
	{
		id: "fat",
		x: 320,
		y: 80,
		width: 180,
		height: 110,
		z: 2,
		fill: "transparent",
		stroke: "#cc6677",
		strokeWidth: 6,
		radius: 6,
	},
	{
		id: "filled",
		x: 540,
		y: 80,
		width: 180,
		height: 110,
		z: 3,
		fill: "#1f4e7a",
		stroke: "#88bbee",
		strokeWidth: 2,
		radius: 6,
	},
	{
		id: "translucent-overlap",
		x: 200,
		y: 240,
		width: 220,
		height: 130,
		z: 5,
		fill: "rgba(52, 199, 89, 0.35)",
		stroke: "#34c759",
		strokeWidth: 2,
		radius: 12,
	},
	{
		id: "under-overlap",
		x: 320,
		y: 300,
		width: 220,
		height: 130,
		z: 4,
		fill: "#5b3a5b",
		stroke: "#ffffff",
		strokeWidth: 1,
		radius: 24,
	},
	{
		id: "hairline",
		x: 600,
		y: 260,
		width: 140,
		height: 90,
		z: 6,
		fill: "transparent",
		stroke: "#ffffff",
		strokeWidth: 1,
		radius: 6,
	},
];

function Host() {
	const store = useMemo(() => createCameraStore(), []);
	return (
		<InfiniteCanvas
			cameraStore={store}
			underlay={<CanvasShapeLayer camera={store} boxes={BOXES} />}
		>
			{/* DOM reference: same geometry as the "plain" GL box, drawn
			    with the border technique — should sit exactly on top of
			    its GL twin, dashed so both remain visible. */}
			<div
				style={{
					position: "absolute",
					left: 100,
					top: 80,
					width: 180,
					height: 110,
					boxSizing: "border-box",
					border: "2px dashed rgba(255, 255, 0, 0.6)",
					borderRadius: 6,
				}}
			/>
			<div
				style={{
					position: "absolute",
					left: 100,
					top: 30,
					color: "#888",
					fontFamily: "system-ui, sans-serif",
					fontSize: 13,
				}}
			>
				GL boxes below — dashed yellow DOM box should align with the first one.
				⌘/ctrl+scroll to zoom.
			</div>
		</InfiniteCanvas>
	);
}

export const Boxes: StoryObj = {
	render: () => <Host />,
};
