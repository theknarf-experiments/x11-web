#!/usr/bin/env node
/**
 * Tiny shim — registers `tsx`'s ESM hook so the CLI body in
 * `src/index.ts` runs directly without a build step. Keeps the
 * dev loop one file away from the source: edit, save, re-run.
 */
import { fileURLToPath } from "node:url";
import { register } from "tsx/esm/api";
const unregister = register();
try {
	await import(
		fileURLToPath(new URL("../src/index.ts", import.meta.url))
	);
} finally {
	unregister();
}
