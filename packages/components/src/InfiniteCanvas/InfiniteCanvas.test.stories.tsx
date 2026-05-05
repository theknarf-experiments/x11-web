import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef } from "react";
import { expect, fn, waitFor, within } from "storybook/test";
import { InfiniteCanvas } from "./InfiniteCanvas.tsx";

interface HostProps {
	onCanvasPointerDown?: (
		point: { x: number; y: number },
		event: React.PointerEvent,
	) => void;
}

function Host(props: HostProps) {
	type PageToCanvas = (
		clientX: number,
		clientY: number,
	) => { x: number; y: number };
	const pageToCanvasRef = useRef<PageToCanvas | null>(null);
	return (
		<div data-testid="host">
			<InfiniteCanvas
				onCanvasPointerDown={props.onCanvasPointerDown}
				pageToCanvasRef={pageToCanvasRef}
			>
				<div
					data-testid="payload"
					style={{
						position: "absolute",
						left: 200,
						top: 200,
						padding: 16,
						background: "white",
					}}
				>
					Payload
				</div>
			</InfiniteCanvas>
			<button
				type="button"
				data-testid="probe-pagetocanvas"
				onClick={() => {
					const fnRef = pageToCanvasRef.current;
					if (!fnRef) return;
					// Probe at a known viewport-relative point. The
					// result is written into a sibling DOM node so
					// the play function can read it back.
					const rect = (
						document.querySelector(
							"[data-testid='infinite-canvas']",
						) as HTMLElement
					).getBoundingClientRect();
					const probe = fnRef(rect.left + 80, rect.top + 90);
					document.querySelector(
						"[data-testid='probe-result']",
					)!.textContent = `${probe.x}|${probe.y}`;
				}}
			>
				probe
			</button>
			<pre data-testid="probe-result"></pre>
		</div>
	);
}

const meta: Meta<typeof Host> = {
	title: "InfiniteCanvas/tests",
	component: Host,
	tags: ["!autodocs"],
	parameters: { layout: "fullscreen" },
};
export default meta;
type Story = StoryObj<typeof Host>;

/** Children render inside the canvas. */
export const RendersChildren: Story = {
	args: {},
	play: async ({ canvasElement }) => {
		await waitFor(() =>
			expect(
				within(canvasElement).getByTestId("payload"),
			).toBeInTheDocument(),
		);
	},
};

/** `pageToCanvasRef` translates page-space coords into
 *  canvas-space. At the initial camera (0, 0, 1) the result
 *  equals the offset from the viewport's top-left. */
export const PageToCanvasReturnsTranslatedCoords: Story = {
	args: {},
	play: async ({ canvasElement }) => {
		const probeBtn = within(canvasElement).getByTestId("probe-pagetocanvas");
		probeBtn.click();
		const result = within(canvasElement).getByTestId("probe-result");
		await waitFor(() =>
			expect(result.textContent).toBe("80|90"),
		);
	},
};

/** `onCanvasPointerDown` reports the click in canvas coords.
 *  Initial camera is `{x:0, y:0, scale:1}`, so canvas coords
 *  equal viewport-relative offsets. */
export const PointerDownFiresWithCanvasCoords: Story = {
	args: { onCanvasPointerDown: fn() },
	play: async ({ canvasElement, args }) => {
		const viewport = within(canvasElement).getByTestId("infinite-canvas");
		const rect = viewport.getBoundingClientRect();
		viewport.dispatchEvent(
			new PointerEvent("pointerdown", {
				bubbles: true,
				clientX: rect.left + 50,
				clientY: rect.top + 60,
			}),
		);
		await waitFor(() =>
			expect(args.onCanvasPointerDown).toHaveBeenCalledWith(
				{ x: 50, y: 60 },
				expect.anything(),
			),
		);
	},
};

/** Clicking the zoom indicator opens a preset menu; picking a
 *  level snaps the camera to that exact scale. Useful when scroll
 *  zoom has drifted off a round number like 100%. */
export const ZoomIndicatorPresetSnaps: Story = {
	args: {},
	play: async ({ canvasElement }) => {
		const viewport = within(canvasElement).getByTestId("infinite-canvas");
		const transformLayer = viewport.querySelector(
			"[data-canvas-scale]",
		) as HTMLElement;

		// Drift away from 100% with a couple of ctrl+wheel events.
		viewport.dispatchEvent(
			new WheelEvent("wheel", {
				bubbles: true,
				cancelable: true,
				ctrlKey: true,
				deltaY: -100,
				clientX: 100,
				clientY: 100,
			}),
		);
		await waitFor(() =>
			expect(
				parseFloat(
					transformLayer.getAttribute("data-canvas-scale") ?? "1",
				),
			).not.toBe(1),
		);

		// Open menu via the indicator, pick 200%.
		within(canvasElement).getByTestId("zoom-indicator").click();
		const menu = await waitFor(() =>
			within(canvasElement).getByTestId("zoom-menu"),
		);
		within(menu).getByText("200%").click();

		await waitFor(() =>
			expect(transformLayer.getAttribute("data-canvas-scale")).toBe("2"),
		);
	},
};

/** `ctrl + wheel` zooms the canvas around the cursor. The inner
 *  transform layer exposes the new scale via `data-canvas-scale`
 *  for inspection. */
export const CtrlWheelZooms: Story = {
	args: {},
	play: async ({ canvasElement }) => {
		const viewport = within(canvasElement).getByTestId("infinite-canvas");
		const transformLayer = viewport.querySelector(
			"[data-canvas-scale]",
		) as HTMLElement;
		expect(transformLayer.getAttribute("data-canvas-scale")).toBe("1");

		viewport.dispatchEvent(
			new WheelEvent("wheel", {
				bubbles: true,
				cancelable: true,
				ctrlKey: true,
				deltaY: -100,
				clientX: 100,
				clientY: 100,
			}),
		);

		await waitFor(() => {
			const scale = parseFloat(
				transformLayer.getAttribute("data-canvas-scale") ?? "1",
			);
			expect(scale).toBeGreaterThan(1);
		});
	},
};
