import type { Meta, StoryObj } from "@storybook/react-vite";
import { userEvent } from "vitest/browser";
import { expect, fn, within } from "storybook/test";
import { useState } from "react";
import { CanvasToolbar, type CanvasTool } from "./CanvasToolbar.tsx";

interface HostProps {
	initial: CanvasTool;
	onSelect?: (tool: CanvasTool) => void;
}

function Host({ initial, onSelect }: HostProps) {
	const [tool, setTool] = useState<CanvasTool>(initial);
	return (
		<div data-testid="host" style={{ height: 400 }}>
			<CanvasToolbar
				tool={tool}
				onSelect={(t) => {
					setTool(t);
					onSelect?.(t);
				}}
			/>
			<pre data-testid="active-tool">{tool}</pre>
		</div>
	);
}

const meta: Meta<typeof Host> = {
	title: "CanvasToolbar/tests",
	component: Host,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Host>;

/** Clicking a tool button reports the selection through `onSelect`
 *  and the host's controlled state flips to the new tool. */
export const ClickSelectsTool: Story = {
	args: { initial: "pointer", onSelect: fn() },
	play: async ({ canvasElement, args }) => {
		const boxBtn = within(canvasElement).getByRole("button", {
			name: /Box/,
		});
		await userEvent.click(boxBtn);
		expect(args.onSelect).toHaveBeenCalledWith("box");
		expect(within(canvasElement).getByTestId("active-tool").textContent).toBe(
			"box",
		);
	},
};

/** Active tool exposes the standard a11y signal (`aria-pressed`)
 *  so screen readers announce the toggle state. */
export const ActiveToolHasAriaPressed: Story = {
	args: { initial: "arrow" },
	play: async ({ canvasElement }) => {
		const arrowBtn = within(canvasElement).getByRole("button", {
			name: /Arrow/,
		});
		const boxBtn = within(canvasElement).getByRole("button", {
			name: /Box/,
		});
		expect(arrowBtn.getAttribute("aria-pressed")).toBe("true");
		expect(boxBtn.getAttribute("aria-pressed")).toBe("false");
	},
};

/** Each tool button advertises its hotkey via `aria-keyshortcuts`,
 *  so `App.tsx`'s global `useHotkey` registrations and the
 *  toolbar's tooltip stay in lockstep through `TOOL_HOTKEYS`. */
export const ButtonsAdvertiseHotkeys: Story = {
	args: { initial: "pointer" },
	play: async ({ canvasElement }) => {
		const pointerBtn = within(canvasElement).getByRole("button", {
			name: /Pointer/,
		});
		expect(pointerBtn.getAttribute("aria-keyshortcuts")).toBe("V");

		const markdownBtn = within(canvasElement).getByRole("button", {
			name: /Markdown/,
		});
		expect(markdownBtn.getAttribute("aria-keyshortcuts")).toBe("M");
	},
};
