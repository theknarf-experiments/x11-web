import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";
import { Polaroid } from "./Polaroid.tsx";

const meta: Meta<typeof Polaroid> = {
	title: "Polaroid/tests",
	component: Polaroid,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Polaroid>;

/** When `src` is provided, the photo `<img>` renders and the
 *  hatched placeholder doesn't. */
export const RendersImage: Story = {
	args: {
		src: "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
		caption: "Test photo",
	},
	play: async ({ canvasElement }) => {
		const polaroid = within(canvasElement).getByTestId("polaroid");
		expect(polaroid.querySelector("img")).toBeTruthy();
		expect(
			within(canvasElement).queryByTestId("polaroid-placeholder"),
		).toBeNull();
		expect(polaroid.textContent).toContain("Test photo");
	},
};

/** When `src` is missing, the hatched placeholder renders in the
 *  photo slot. The caption still shows. */
export const RendersPlaceholderWithoutSrc: Story = {
	args: { src: undefined, caption: "No photo yet" },
	play: async ({ canvasElement }) => {
		const polaroid = within(canvasElement).getByTestId("polaroid");
		expect(polaroid.querySelector("img")).toBeNull();
		expect(
			within(canvasElement).getByTestId("polaroid-placeholder"),
		).toBeInTheDocument();
		expect(polaroid.textContent).toContain("No photo yet");
	},
};

/** `draggable` flag flips the HTML5 drag attribute and the
 *  `onDragStart` callback fires with the card `<button>` as
 *  the event target. */
export const DragStartFires: Story = {
	args: {
		src: "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
		caption: "Drag me",
		draggable: true,
		onDragStart: fn(),
	},
	play: async ({ canvasElement, args }) => {
		const polaroid = within(canvasElement).getByTestId("polaroid");
		expect(polaroid.getAttribute("draggable")).toBe("true");
		polaroid.dispatchEvent(
			new DragEvent("dragstart", { bubbles: true, cancelable: true }),
		);
		expect(args.onDragStart).toHaveBeenCalled();
	},
};
