import type { Meta, StoryObj } from "@storybook/react-vite";
import { Tooltip } from "./Tooltip.tsx";

const meta: Meta<typeof Tooltip> = {
	title: "Tooltip",
	component: Tooltip,
	argTypes: {
		side: {
			control: "select",
			options: ["top", "right", "bottom", "left"],
		},
	},
	// Centre the trigger so each side variant has room for the bubble.
	decorators: [
		(Story) => (
			<div
				style={{
					display: "flex",
					alignItems: "center",
					justifyContent: "center",
					minHeight: 120,
				}}
			>
				<Story />
			</div>
		),
	],
	render: (args) => (
		<Tooltip {...args}>
			<button type="button" aria-label={args.label}>
				Hover me
			</button>
		</Tooltip>
	),
};

export default meta;
type Story = StoryObj<typeof Tooltip>;

export const Top: Story = { args: { label: "Move", side: "top" } };
export const Bottom: Story = { args: { label: "Move", side: "bottom" } };
export const Left: Story = { args: { label: "Move", side: "left" } };
export const Right: Story = { args: { label: "Move", side: "right" } };

export const WithHotkey: Story = {
	args: { label: "Save", hotkey: "⌘S", side: "top" },
};
