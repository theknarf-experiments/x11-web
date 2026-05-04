import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";

// https://vite.dev/config/
export default defineConfig({
	// `wasm()` + `topLevelAwait()` are required for
	// `@automerge/automerge` (workspace CRDTs over the control
	// DataChannel) — its core is compiled Rust→WebAssembly and
	// bundled as a separate `*.wasm` asset whose loader uses
	// top-level `await`. Our build target (default es2020) needs
	// the plugin to rewrite that into a wrapper IIFE.
	plugins: [wasm(), topLevelAwait(), react()],
	worker: {
		format: "es",
		plugins: () => [wasm(), topLevelAwait()],
	},
});
