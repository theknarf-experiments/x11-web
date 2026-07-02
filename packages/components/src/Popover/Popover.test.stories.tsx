import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, waitFor, within } from "storybook/test";
import { userEvent } from "vitest/browser";
import { Popover } from "./Popover.tsx";

function Host() {
	return (
		<div data-testid="host" style={{ padding: 80 }}>
			<Popover
				trigger={
					<button type="button" data-testid="trigger">
						open
					</button>
				}
			>
				{({ close }) => (
					<div>
						<span data-testid="content">popover content</span>
						<button type="button" data-testid="close-btn" onClick={close}>
							dismiss
						</button>
					</div>
				)}
			</Popover>
			<button type="button" data-testid="outside">
				outside
			</button>
		</div>
	);
}

const meta: Meta<typeof Host> = {
	title: "Popover/tests",
	component: Host,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Host>;

/** Trigger toggles the panel. Initial state is closed. */
export const TriggerToggles: Story = {
	play: async ({ canvasElement }) => {
		const trigger = within(canvasElement).getByTestId("trigger");
		expect(within(canvasElement).queryByTestId("content")).toBeNull();
		await userEvent.click(trigger);
		await waitFor(() =>
			expect(within(canvasElement).getByTestId("content")).toBeInTheDocument(),
		);
		await userEvent.click(trigger);
		await waitFor(() =>
			expect(within(canvasElement).queryByTestId("content")).toBeNull(),
		);
	},
};

/** Outside-click dismisses the panel. */
export const OutsideClickDismisses: Story = {
	play: async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getByTestId("trigger"));
		await waitFor(() =>
			expect(within(canvasElement).getByTestId("content")).toBeInTheDocument(),
		);
		await userEvent.click(within(canvasElement).getByTestId("outside"));
		await waitFor(() =>
			expect(within(canvasElement).queryByTestId("content")).toBeNull(),
		);
	},
};

/** The render-prop's `close` callback dismisses the panel — the
 *  pattern used by menu items that should auto-close on pick. */
export const RenderPropCloseDismisses: Story = {
	play: async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getByTestId("trigger"));
		const closeBtn = await waitFor(() =>
			within(canvasElement).getByTestId("close-btn"),
		);
		await userEvent.click(closeBtn);
		await waitFor(() =>
			expect(within(canvasElement).queryByTestId("content")).toBeNull(),
		);
	},
};
