import type { Meta, StoryObj } from "@storybook/react";
import { Button } from "./Button.tsx";

const meta: Meta<typeof Button> = {
	title: "Button",
	component: Button,
	argTypes: {
		variant: {
			control: "select",
			options: ["primary", "secondary"],
		},
	},
};

export default meta;
type Story = StoryObj<typeof Button>;

export const Primary: Story = {
	args: {
		label: "Primary Button",
		variant: "primary",
	},
};

export const Secondary: Story = {
	args: {
		label: "Secondary Button",
		variant: "secondary",
	},
};
