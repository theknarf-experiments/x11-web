import type { Preview } from "@storybook/react";

const preview: Preview = {
	parameters: {
		controls: {
			matchers: {
				color: /(background|color)$/i,
				date: /Date$/i,
			},
		},
		// `@storybook/addon-a11y` runs axe-core against each story
		// and surfaces violations in the panel. Setting `test: "error"`
		// promotes axe violations to test failures so the vitest run
		// catches a11y regressions in CI alongside the component
		// behaviour assertions.
		a11y: {
			test: "error",
		},
	},
};

export default preview;
