import type { Meta, StoryObj } from "@storybook/react-vite";
import { Polaroid, PolaroidStack } from "./Polaroid.tsx";

// 5×4 SVGs encoded inline so the stories don't need network /
// filesystem access. Two solid colours so the fan reads.
const SAMPLE_IMAGES = [
	`data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 250 200'><rect width='250' height='200' fill='%236699cc'/><circle cx='125' cy='100' r='40' fill='%23fff'/></svg>`,
	)}`,
	`data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 250 200'><rect width='250' height='200' fill='%23cc6677'/><polygon points='125,40 90,160 160,160' fill='%23fff'/></svg>`,
	)}`,
	`data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 250 200'><rect width='250' height='200' fill='%2388aa66'/><rect x='90' y='60' width='70' height='80' fill='%23fff'/></svg>`,
	)}`,
];

const meta: Meta<typeof Polaroid> = {
	title: "Polaroid",
	component: Polaroid,
	parameters: {
		backgrounds: {
			options: {
				canvas: { name: "Canvas", value: "#1a1a1a" },
			},
		},
	},
	globals: {
		backgrounds: { value: "canvas" },
	},
};

export default meta;
type Story = StoryObj<typeof Polaroid>;

export const Single: Story = {
	args: { src: SAMPLE_IMAGES[0], caption: "Lake Como" },
};

export const Placeholder: Story = {
	args: { src: undefined, caption: "Loading…" },
};

/** A fan of polaroids inside `<PolaroidStack>` — the tilt and
 *  vertical-offset cycles are driven by `:nth-child`, so consecutive
 *  cards never look identical and a single hover lifts one out of
 *  the deck. */
export const Stack: StoryObj<typeof PolaroidStack> = {
	render: () => (
		<PolaroidStack>
			<Polaroid src={SAMPLE_IMAGES[0]} caption="Lake Como" />
			<Polaroid src={SAMPLE_IMAGES[1]} caption="Sunset peak" />
			<Polaroid src={SAMPLE_IMAGES[2]} caption="Cottage" />
			<Polaroid src={undefined} caption="Loading…" />
			<Polaroid src={SAMPLE_IMAGES[0]} caption="Lake Como II" />
		</PolaroidStack>
	),
};
