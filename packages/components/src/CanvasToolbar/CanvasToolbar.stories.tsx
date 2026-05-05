import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";
import { CanvasToolbar } from "./CanvasToolbar.tsx";

const meta: Meta<typeof CanvasToolbar> = {
	title: "CanvasToolbar",
	component: CanvasToolbar,
	args: { onSelect: fn() },
	argTypes: {
		tool: {
			control: "select",
			options: ["pointer", "box", "arrow", "text", "pen", "markdown"],
		},
	},
	// The toolbar is translucent + has a `backdrop-filter: blur(20px)`;
	// pin a dark backdrop so the chrome reads like production.
	parameters: {
		backgrounds: {
			options: {
				canvas: { name: "Canvas", value: "#1a1a1a" },
			},
		},
	},
	globals: {
		backgrounds: { value: "canvas" },
	},
};

export default meta;
type Story = StoryObj<typeof CanvasToolbar>;

export const Pointer: Story = { args: { tool: "pointer" } };
export const Box: Story = { args: { tool: "box" } };
export const Arrow: Story = { args: { tool: "arrow" } };
