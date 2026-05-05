import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";
import { GlobalMenuBar, type MenuItem } from "./GlobalMenuBar.tsx";

const sampleMenu: MenuItem[] = [
	{
		id: "file",
		label: "File",
		kind: "submenu",
		children: [
			{ id: "new", label: "New", kind: "normal", accelerator: "⌘N" },
			{ id: "open", label: "Open…", kind: "normal", accelerator: "⌘O" },
			{ id: "sep1", kind: "separator" },
			{
				id: "recent",
				label: "Open Recent",
				kind: "submenu",
				children: [
					{ id: "rec1", label: "report.txt", kind: "normal" },
					{ id: "rec2", label: "notes.md", kind: "normal" },
				],
			},
			{ id: "sep2", kind: "separator" },
			{ id: "quit", label: "Quit", kind: "normal", accelerator: "⌘Q" },
		],
	},
	{
		id: "edit",
		label: "Edit",
		kind: "submenu",
		children: [
			{ id: "undo", label: "Undo", kind: "normal", accelerator: "⌘Z" },
			{
				id: "redo",
				label: "Redo",
				kind: "normal",
				accelerator: "⇧⌘Z",
				enabled: false,
			},
			{ id: "sep3", kind: "separator" },
			{ id: "wrap", label: "Word Wrap", kind: "checkbox", checked: true },
		],
	},
	{ id: "view", label: "View", kind: "submenu", children: [] },
];

const meta: Meta<typeof GlobalMenuBar> = {
	title: "GlobalMenuBar",
	component: GlobalMenuBar,
	args: {
		onActivate: fn(),
		onRenameWorkspace: fn(),
	},
	parameters: {
		backgrounds: {
			options: {
				canvas: { name: "Canvas", value: "#1a1a1a" },
			},
		},
		layout: "fullscreen",
	},
	globals: {
		backgrounds: { value: "canvas" },
	},
};

export default meta;
type Story = StoryObj<typeof GlobalMenuBar>;

export const FocusedApp: Story = {
	args: {
		focusedTitle: "TextEditor",
		workspaceName: "My Workspace",
		menu: sampleMenu,
		appContextMenuItems: [
			{ label: "Close", destructive: true, onSelect: () => {} },
		],
	},
};

export const NoFocusedApp: Story = {
	args: {
		focusedTitle: null,
		workspaceName: "My Workspace",
		menu: null,
		appContextMenuItems: null,
	},
};

export const DisabledItems: Story = {
	args: {
		focusedTitle: "TextEditor",
		workspaceName: "My Workspace",
		menu: [
			{
				id: "file",
				label: "File",
				kind: "submenu",
				children: [{ id: "new", label: "New", kind: "normal" }],
			},
			{
				id: "edit",
				label: "Edit",
				kind: "submenu",
				enabled: false,
				children: [],
			},
		],
		appContextMenuItems: null,
	},
};
