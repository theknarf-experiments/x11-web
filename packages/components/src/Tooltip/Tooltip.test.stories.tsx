import type { Meta, StoryObj } from "@storybook/react-vite";
import { userEvent } from "vitest/browser";
import { expect, waitFor, within } from "storybook/test";
import { Tooltip } from "./Tooltip.tsx";

function Host({ label, hotkey }: { label: string; hotkey?: string }) {
	return (
		<div data-testid="host" style={{ padding: 60 }}>
			<Tooltip label={label} hotkey={hotkey} side="top">
				<button type="button" aria-label={label}>
					trigger
				</button>
			</Tooltip>
		</div>
	);
}

const meta: Meta<typeof Host> = {
	title: "Tooltip/tests",
	component: Host,
	tags: ["!autodocs"],
};

export default meta;
type Story = StoryObj<typeof Host>;

/** Hovering the trigger fades the bubble in. We assert against
 *  computed `opacity` because the visibility is CSS-driven —
 *  there's no React state toggle to query. Vitest browser mode
 *  runs real Chromium, so `:hover` actually flips the rule. */
export const HoverShowsBubble: Story = {
	args: { label: "Save", hotkey: "⌘S" },
	play: async ({ canvasElement }) => {
		const trigger = within(canvasElement).getByRole("button", {
			name: "Save",
		});
		const bubble = canvasElement.querySelector(
			"[role='presentation']",
		) as HTMLElement;
		expect(bubble).toBeTruthy();
		// Initial state — hidden.
		expect(getComputedStyle(bubble).opacity).toBe("0");

		await userEvent.hover(trigger);
		await waitFor(() =>
			expect(getComputedStyle(bubble).opacity).toBe("1"),
		);

		// Bubble content matches the props.
		expect(bubble.textContent).toContain("Save");
		expect(bubble.textContent).toContain("⌘S");

		await userEvent.unhover(trigger);
		await waitFor(() =>
			expect(getComputedStyle(bubble).opacity).toBe("0"),
		);
	},
};
