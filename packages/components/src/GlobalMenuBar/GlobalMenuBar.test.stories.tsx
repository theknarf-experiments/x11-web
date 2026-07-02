import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, waitFor, within } from "storybook/test";
import { userEvent } from "vitest/browser";
import {
	GlobalMenuBar,
	type MenuAction,
	type MenuItem,
} from "./GlobalMenuBar.tsx";

interface HostProps {
	focusedTitle: string | null;
	workspaceName: string | null;
	menu: MenuItem[] | null;
	onActivate: (a: MenuAction) => void;
	onRenameWorkspace: (n: string) => void;
}

function Host(props: HostProps) {
	return (
		<div data-testid="host">
			<GlobalMenuBar
				focusedTitle={props.focusedTitle}
				workspaceName={props.workspaceName}
				onRenameWorkspace={props.onRenameWorkspace}
				menu={props.menu}
				onActivate={props.onActivate}
				appContextMenuItems={null}
			/>
		</div>
	);
}

const sampleMenu: MenuItem[] = [
	{
		id: "file",
		label: "File",
		kind: "submenu",
		children: [
			{
				id: "new",
				label: "New",
				kind: "normal",
				action: { name: "file.new" },
			},
			{ id: "sep", kind: "separator" },
			{
				id: "quit",
				label: "Quit",
				kind: "normal",
				action: { name: "file.quit" },
			},
		],
	},
	{
		id: "edit",
		label: "Edit",
		kind: "submenu",
		children: [
			{
				id: "undo",
				label: "Undo",
				kind: "normal",
				action: { name: "edit.undo" },
			},
		],
	},
];

const meta: Meta<typeof Host> = {
	title: "GlobalMenuBar/tests",
	component: Host,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Host>;

const baseArgs: HostProps = {
	focusedTitle: "App",
	workspaceName: null,
	menu: sampleMenu,
	onActivate: fn(),
	onRenameWorkspace: fn(),
};

/** Click a top-level item, then a leaf inside its dropdown — the
 *  leaf's `action` flows out through `onActivate`. */
export const ClickLeafFiresAction: Story = {
	args: { ...baseArgs, onActivate: fn() },
	play: async ({ canvasElement, args }) => {
		const tops = within(canvasElement).getAllByTestId("global-menu-top-item");
		await userEvent.click(tops[0]!); // File
		const dropdown = await waitFor(() =>
			within(canvasElement).getByTestId("global-menu-dropdown"),
		);
		const items = within(dropdown).getAllByTestId("global-menu-item");
		// items[0] = New, items[1] = Quit (separator skipped)
		await userEvent.click(items[1]!);
		expect(args.onActivate).toHaveBeenCalledWith({ name: "file.quit" });
	},
};

/** macOS-style menu cruise — once a top-level menu is open,
 *  hovering a sibling switches to it without an extra click. */
export const HoverCruisesBetweenTopMenus: Story = {
	args: baseArgs,
	play: async ({ canvasElement }) => {
		const tops = within(canvasElement).getAllByTestId("global-menu-top-item");
		await userEvent.click(tops[0]!); // open File
		// File's dropdown shows "New" + "Quit"
		await waitFor(() =>
			expect(within(canvasElement).getByText("New")).toBeInTheDocument(),
		);

		// Hover Edit — should switch the open dropdown.
		await userEvent.hover(tops[1]!);
		await waitFor(() =>
			expect(within(canvasElement).getByText("Undo")).toBeInTheDocument(),
		);
		// And File's "New" is no longer rendered.
		expect(within(canvasElement).queryByText("New")).toBeNull();
	},
};

/** When no window is focused, the bar shows the workspace name as
 *  an inline-editable input. Renaming + Enter commits via
 *  `onRenameWorkspace`. */
export const RenameWorkspaceCommitsOnEnter: Story = {
	args: {
		...baseArgs,
		focusedTitle: null,
		workspaceName: "Old name",
		onRenameWorkspace: fn(),
	},
	play: async ({ canvasElement, args }) => {
		const input = within(canvasElement).getByTestId(
			"global-menu-bar-title",
		) as HTMLInputElement;
		await userEvent.click(input);
		// `onFocus` selects all → next keystroke replaces.
		await userEvent.keyboard("New name{Enter}");
		expect(args.onRenameWorkspace).toHaveBeenCalledWith("New name");
	},
};
