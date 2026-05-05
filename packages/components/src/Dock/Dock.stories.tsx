import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";
import { Dock } from "./Dock.tsx";

const meta: Meta<typeof Dock> = {
	title: "Dock",
	component: Dock,
	args: {
		connected: true,
		thumbnails: new Map(),
		onSpawn: fn(),
		onFocusWindow: fn(),
		onProcessContextMenu: fn(),
	},
	// The dock is translucent + has a `backdrop-filter: blur(20px)`;
	// against Storybook's default white canvas the chrome reads as
	// washed out. Pin a dark backdrop matching the app's canvas
	// color so it looks like production. Storybook 10's backgrounds
	// take a `parameters.backgrounds.options` map plus a `globals`
	// pick — the old `default` / `values` shape from <=v8 is gone.
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
type Story = StoryObj<typeof Dock>;

export const Empty: Story = {
	args: {
		sidecars: [{ id: "x11", name: "X11" }],
		processes: [],
		windows: [],
	},
};

export const WithProcesses: Story = {
	args: {
		sidecars: [{ id: "x11", name: "X11" }],
		processes: [
			{ sidecarId: "x11", pid: 101, title: "xeyes", color: "#cc6677" },
			{ sidecarId: "x11", pid: 102, title: "xterm", color: "#6699cc" },
		],
		windows: [],
	},
};

export const Disconnected: Story = {
	args: {
		connected: false,
		sidecars: [{ id: "x11", name: "X11" }],
		processes: [],
		windows: [],
	},
};
