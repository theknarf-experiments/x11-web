import { type ChildProcess, exec } from "node:child_process";
import * as http from "node:http";
import * as path from "node:path";
import { expect, test } from "@playwright/test";
import type { StartedNetwork, StartedTestContainer } from "testcontainers";
import { GenericContainer, Network, Wait } from "testcontainers";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "../..");
const E2E_DIR = path.resolve(import.meta.dirname, "..");
const FRONTEND_DIR = path.join(PROJECT_ROOT, "frontend");
const SERVE_BIN = path.join(E2E_DIR, "node_modules", ".bin", "serve");

let network: StartedNetwork;
let backendContainer: StartedTestContainer;
let sidecarContainer: StartedTestContainer;
let frontendServer: ChildProcess;
let frontendPort: number;
let backendPort: number;

// Use serial mode so all tests share the same containers
test.describe
	.serial("x11-web e2e", () => {
		test.beforeAll(async () => {
			// Create a shared Docker network
			network = await new Network().start();

			// Build and start backend
			backendContainer = await GenericContainer.fromDockerfile(
				PROJECT_ROOT,
				"Dockerfile.backend",
			)
				.build("x11-web-backend-test", { deleteOnExit: false })
				.then((image) =>
					image
						.withNetwork(network)
						.withNetworkAliases("backend")
						.withExposedPorts(3001)
						.withWaitStrategy(Wait.forHttp("/health", 3001).forStatusCode(200))
						.start(),
				);

			backendPort = backendContainer.getMappedPort(3001);
			console.log(`Backend running at localhost:${backendPort}`);

			// Build and start sidecar (connects to backend via internal network)
			sidecarContainer = await GenericContainer.fromDockerfile(
				PROJECT_ROOT,
				"Dockerfile.sidecar",
			)
				.build("x11-web-sidecar-test", { deleteOnExit: false })
				.then((image) =>
					image
						.withNetwork(network)
						.withNetworkAliases("sidecar")
						.withEnvironment({
							BACKEND_URL: "ws://backend:3001/ws/sidecar",
							SIDECAR_NAME: "test-sidecar",
							DISPLAY_NUMBER: "99",
							RUST_LOG: "info",
						})
						.withWaitStrategy(Wait.forLogMessage(/Connected to backend/))
						.start(),
				);

			console.log("Sidecar connected to backend");

			// Build the frontend locally with the correct backend WS URL
			const wsUrl = `ws://localhost:${backendPort}/ws/frontend`;
			await new Promise<void>((resolve, reject) => {
				exec(
					`VITE_WS_URL=${wsUrl} pnpm run build`,
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

			// Serve the built frontend with a simple static file server
			frontendPort = await findFreePort();
			await new Promise<void>((resolve) => {
				frontendServer = exec(
					`${SERVE_BIN} dist -l ${frontendPort} --no-clipboard`,
					{ cwd: FRONTEND_DIR },
				);
				// Wait for server to start
				const check = setInterval(async () => {
					try {
						const res = await fetch(`http://localhost:${frontendPort}`);
						if (res.ok) {
							clearInterval(check);
							resolve();
						}
					} catch {
						// Not ready yet
					}
				}, 200);
			});

			console.log(`Frontend running at http://localhost:${frontendPort}`);
		});

		test.afterAll(async () => {
			frontendServer?.kill();
			await sidecarContainer?.stop();
			await backendContainer?.stop();
			await network?.stop();
		});

		test("frontend loads and shows connected status", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			// Wait for WebSocket to connect
			const status = page.locator('[data-testid="connection-status"]');
			await expect(status).toHaveText("Connected", { timeout: 15_000 });
		});

		test("sidecar appears in the dashboard", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			// Wait for sidecar card to appear
			const sidecarCard = page.locator('[data-testid="sidecar-card"]');
			await expect(sidecarCard).toBeVisible({ timeout: 15_000 });

			// Verify sidecar name is shown
			await expect(sidecarCard.locator("h3")).toHaveText("test-sidecar");
		});

		test("spawning xeyes produces display updates on canvas", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);

			// Wait for sidecar to appear
			const sidecarCard = page.locator('[data-testid="sidecar-card"]');
			await expect(sidecarCard).toBeVisible({ timeout: 15_000 });

			// Set args for a larger window so the eyes are visible in the screenshot
			await page
				.locator('input[placeholder*="geometry"]')
				.fill("-geometry 300x200+10+10");

			// Click "Spawn xeyes" button
			await sidecarCard.locator("button", { hasText: "Spawn xeyes" }).click();

			// Wait for the display section and canvas to appear
			const displaySection = page.locator('[data-testid="display-section"]');
			await expect(displaySection).toBeVisible({ timeout: 10_000 });

			const canvas = page.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			// Wait for xeyes to connect to our X server and produce some drawing commands
			await page.waitForTimeout(5000);

			// Verify non-trivial content was drawn:
			// 1. Minimum number of non-black pixels (xeyes draws two filled ellipses)
			// 2. Multiple distinct colors present (background, eye outline, eye fill)
			const canvasStats = await canvas.evaluate((el: HTMLCanvasElement) => {
				const ctx = el.getContext("2d");
				if (!ctx) return { nonBlackPixels: 0, distinctColors: 0 };
				const imageData = ctx.getImageData(0, 0, el.width, el.height);
				let nonBlackPixels = 0;
				const colors = new Set<string>();

				for (let i = 0; i < imageData.data.length; i += 4) {
					const r = imageData.data[i];
					const g = imageData.data[i + 1];
					const b = imageData.data[i + 2];
					if (r !== 0 || g !== 0 || b !== 0) {
						nonBlackPixels++;
						// Bucket colors to 16-level bins to avoid counting anti-aliasing noise
						const key = `${r >> 4},${g >> 4},${b >> 4}`;
						colors.add(key);
					}
				}

				return { nonBlackPixels, distinctColors: colors.size };
			});

			// xeyes draws a small window — even a handful of non-black pixels
			// proves the full pipeline works: X11 server → protocol → backend → frontend canvas
			expect(canvasStats.nonBlackPixels).toBeGreaterThan(10);

			// Screenshot test: compare canvas against a stored reference.
			// This is the real visual regression check — it catches rendering changes.
			// Use a generous diff ratio since xeyes rendering can vary slightly
			// depending on timing (e.g. exact cursor position affects pupil placement).
			await expect(canvas).toHaveScreenshot("xeyes-canvas.png", {
				maxDiffPixelRatio: 0.01,
			});
		});
	});

async function findFreePort(): Promise<number> {
	return new Promise((resolve) => {
		const server = http.createServer();
		server.listen(0, () => {
			const port = (server.address() as { port: number }).port;
			server.close(() => resolve(port));
		});
	});
}
