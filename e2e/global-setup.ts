/**
 * Playwright global setup — runs once before any worker starts.
 *
 * Builds the frontend bundle so every worker can spawn its own `serve`
 * process against the shared `frontend/dist` output (no rebuild races).
 * The runtime WS URL is supplied per-page via a `?ws=` query param so the
 * single bundle works regardless of which worker / backend port a test
 * happens to land on.
 */

import { exec } from "node:child_process";
import * as path from "node:path";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");

export default async function globalSetup() {
	await new Promise<void>((resolve, reject) => {
		exec(
			"pnpm run build",
			{ cwd: FRONTEND_DIR },
			(error, _stdout, stderr) => {
				if (error) {
					console.error("Frontend build failed:", stderr);
					reject(error);
				} else {
					resolve();
				}
			},
		);
	});
}
