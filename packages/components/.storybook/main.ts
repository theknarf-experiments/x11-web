import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import type { StorybookConfig } from "@storybook/react-vite";
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";

const config: StorybookConfig = {
	stories: ["../src/**/*.stories.@(ts|tsx)"],
	addons: [
		getAbsolutePath("@storybook/addon-docs"),
		getAbsolutePath("@storybook/addon-vitest"),
		getAbsolutePath("@storybook/addon-a11y"),
	],
	framework: {
		name: getAbsolutePath("@storybook/react-vite"),
		options: {},
	},
	// Stories that import `@automerge/automerge` need WASM + top-
	// level-await support; merge those plugins into Storybook's
	// underlying Vite config.
	viteFinal: async (vite) => {
		vite.plugins = [...(vite.plugins ?? []), wasm(), topLevelAwait()];
		// The default build target (es2020/chrome87/…) can't represent
		// top-level await, so the static `storybook build` fails while
		// the dev server (no bundling) works. Target modern engines.
		vite.build = { ...vite.build, target: "esnext" };
		return vite;
	},
};

export default config;

/** Resolve an addon/framework to its on-disk directory — with pnpm's
 *  isolated node_modules, bare package names aren't always resolvable
 *  from Storybook's own location. */
function getAbsolutePath(value: string): string {
	return dirname(fileURLToPath(import.meta.resolve(`${value}/package.json`)));
}
