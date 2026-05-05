import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";

// `wasm()` + `topLevelAwait()` are needed by stories/tests that
// pull in `@automerge/automerge` — its core is Rust→WASM and the
// loader uses top-level `await`.
export default defineConfig({
	plugins: [wasm(), topLevelAwait(), react()],
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
