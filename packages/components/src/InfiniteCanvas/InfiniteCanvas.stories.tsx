import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";
import { InfiniteCanvas } from "./InfiniteCanvas.tsx";

const meta: Meta<typeof InfiniteCanvas> = {
	title: "InfiniteCanvas",
	component: InfiniteCanvas,
	args: {
		onCanvasDrop: fn(),
		onCanvasPointerDown: fn(),
	},
	parameters: {
		// The viewport is `position: fixed; inset: 0` and uses a
		// dark background; remove Storybook's padding chrome.
		layout: "fullscreen",
	},
};

export default meta;
type Story = StoryObj<typeof InfiniteCanvas>;

/** A handful of rectangles laid out in canvas space — pan with
 *  scroll, zoom with ⌘ / ctrl + scroll. */
export const WithShapes: Story = {
	render: (args) => (
		<InfiniteCanvas {...args}>
			{[
				{ x: 100, y: 100, color: "#cc6677" },
				{ x: 320, y: 220, color: "#6699cc" },
				{ x: 600, y: 80, color: "#88aa66" },
				{ x: 800, y: 400, color: "#cc99cc" },
			].map((box, i) => (
				<div
					// biome-ignore lint/suspicious/noArrayIndexKey: static demo content
					key={i}
					style={{
						position: "absolute",
						left: box.x,
						top: box.y,
						width: 160,
						height: 100,
						background: box.color,
						border: "1px solid rgba(255,255,255,0.2)",
						borderRadius: 6,
						color: "white",
						display: "flex",
						alignItems: "center",
						justifyContent: "center",
						fontFamily: "system-ui, sans-serif",
					}}
				>
					Shape {i + 1}
				</div>
			))}
		</InfiniteCanvas>
	),
};

/** Empty canvas — useful for verifying the zoom indicator and
 *  background colour without distractions. */
export const Empty: Story = {
	render: (args) => <InfiniteCanvas {...args}>{null}</InfiniteCanvas>,
};
