import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// `vite-plugin-wasm` + `vite-plugin-top-level-await` are only
// needed by stories/tests that pull in `@automerge/automerge`
// (Storybook adds them via `.storybook/main.ts`'s `viteFinal`).
// The library build only exports plain React components from
// `src/index.ts`; bundling them in here would force rolldown's
// esbuild minifier to transform destructuring out of the WASM
// loader for older targets, which it can't do.
export default defineConfig({
	plugins: [react()],
	build: {
		lib: {
			entry: "src/index.ts",
			formats: ["es"],
			fileName: "index",
		},
		rollupOptions: {
			external: ["react", "react-dom", "react/jsx-runtime"],
		},
	},
});
