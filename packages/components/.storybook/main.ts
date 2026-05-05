import type { StorybookConfig } from "@storybook/react-vite";
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";

const config: StorybookConfig = {
	stories: ["../src/**/*.stories.@(ts|tsx)"],
	addons: ["@storybook/addon-vitest"],
	framework: {
		name: "@storybook/react-vite",
		options: {},
	},
	// Stories that import `@automerge/automerge` need WASM + top-
	// level-await support; merge those plugins into Storybook's
	// underlying Vite config.
	viteFinal: async (vite) => {
		vite.plugins = [...(vite.plugins ?? []), wasm(), topLevelAwait()];
		return vite;
	},
};

export default config;
