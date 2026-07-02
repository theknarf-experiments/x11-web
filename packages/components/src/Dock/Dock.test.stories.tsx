import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, waitFor, within } from "storybook/test";
import { userEvent } from "vitest/browser";
import { Dock, type DockProcess, type DockSidecar } from "./Dock.tsx";

interface HostProps {
	connected: boolean;
	sidecars: DockSidecar[];
	processes: DockProcess[];
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onFocusWindow: (sidecarId: string, pid: number) => void;
	onProcessContextMenu: (
		sidecarId: string,
		pid: number,
		x: number,
		y: number,
	) => void;
}

function Host(props: HostProps) {
	return (
		<div data-testid="host" style={{ height: 200 }}>
			<Dock
				connected={props.connected}
				sidecars={props.sidecars}
				processes={props.processes}
				windows={[]}
				thumbnails={new Map()}
				onSpawn={props.onSpawn}
				onFocusWindow={props.onFocusWindow}
				onProcessContextMenu={props.onProcessContextMenu}
			/>
		</div>
	);
}

const meta: Meta<typeof Host> = {
	title: "Dock/tests",
	component: Host,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Host>;

const defaultArgs: HostProps = {
	connected: true,
	sidecars: [{ id: "x11", name: "X11 sidecar" }],
	processes: [
		{ sidecarId: "x11", pid: 101, title: "xeyes", color: "#cc6677" },
		{ sidecarId: "x11", pid: 102, title: "xterm", color: "#6699cc" },
	],
	onSpawn: fn(),
	onFocusWindow: fn(),
	onProcessContextMenu: fn(),
};

/** A click on a process icon focuses the matching window. */
export const ClickProcessIconFocuses: Story = {
	args: { ...defaultArgs, onFocusWindow: fn() },
	play: async ({ canvasElement, args }) => {
		const icons = within(canvasElement).getAllByTestId("process-icon");
		expect(icons).toHaveLength(2);
		await userEvent.click(icons[0]!);
		expect(args.onFocusWindow).toHaveBeenCalledWith("x11", 101);
	},
};

/** Right-click bubbles up via `onProcessContextMenu` so the host
 *  can render an app menu (close, etc.). The Dock itself doesn't
 *  render the menu — keeps it pure / app-agnostic. */
export const RightClickFiresContextMenu: Story = {
	args: { ...defaultArgs, onProcessContextMenu: fn() },
	play: async ({ canvasElement, args }) => {
		const icons = within(canvasElement).getAllByTestId("process-icon");
		// `vitest/browser`'s userEvent doesn't expose `pointer()` —
		// dispatch the contextmenu DOM event directly. React picks it
		// up as `onContextMenu`.
		icons[1]!.dispatchEvent(
			new MouseEvent("contextmenu", {
				bubbles: true,
				cancelable: true,
				clientX: 123,
				clientY: 456,
			}),
		);
		expect(args.onProcessContextMenu).toHaveBeenCalledWith(
			"x11",
			102,
			123,
			456,
		);
	},
};

/** Spawn flow — open the per-sidecar popover, type a command,
 *  click Spawn, verify the callback fires with the parsed args. */
export const SpawnPopoverFlow: Story = {
	args: { ...defaultArgs, onSpawn: fn() },
	play: async ({ canvasElement, args }) => {
		const spawnBtn = within(canvasElement).getByTestId("spawn-button");
		await userEvent.click(spawnBtn);

		const cmd = await waitFor(() =>
			within(canvasElement).getByTestId("spawn-command"),
		);
		const argsInput = within(canvasElement).getByTestId("spawn-args");

		// Inputs are seeded with "xeyes" (the dock's default
		// example command) — clear first so we control the value.
		await userEvent.clear(cmd);
		await userEvent.type(cmd, "xterm");
		await userEvent.type(argsInput, "-bg black");

		await userEvent.click(within(canvasElement).getByTestId("spawn-submit"));
		expect(args.onSpawn).toHaveBeenCalledWith("x11", "xterm", ["-bg", "black"]);
	},
};
