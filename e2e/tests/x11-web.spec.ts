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

test.describe
	.serial("x11-web e2e", () => {
		test.beforeAll(async () => {
			network = await new Network().start();

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

			frontendPort = await findFreePort();
			await new Promise<void>((resolve) => {
				frontendServer = exec(
					`${SERVE_BIN} dist -l ${frontendPort} --no-clipboard`,
					{ cwd: FRONTEND_DIR },
				);
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

		test("dock shows sidecar as connected", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			const dock = page.locator('[data-testid="dock"]');
			await expect(dock).toBeVisible({ timeout: 15_000 });
			await expect(dock).toContainText("test-sidecar");
		});

		test("spawning xeyes creates a window on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			const dock = page.locator('[data-testid="dock"]');
			await expect(dock).toContainText("test-sidecar", { timeout: 15_000 });

			// Set args for a larger window
			await page
				.locator('input[placeholder="args"]')
				.fill("-geometry 300x200+10+10");

			// Spawn
			await page.locator('[data-testid="spawn-button"]').click();

			// A window frame should appear on the canvas
			const windowFrame = page.locator('[data-testid="window-frame"]');
			await expect(windowFrame).toBeVisible({ timeout: 10_000 });

			// The canvas inside it should render
			const canvas = windowFrame.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			await page.waitForTimeout(5000);

			// Verify content was drawn
			const nonBlackPixels = await canvas.evaluate((el: HTMLCanvasElement) => {
				const ctx = el.getContext("2d");
				if (!ctx) return 0;
				const imageData = ctx.getImageData(0, 0, el.width, el.height);
				let count = 0;
				for (let i = 0; i < imageData.data.length; i += 4) {
					if (
						imageData.data[i] !== 0 ||
						imageData.data[i + 1] !== 0 ||
						imageData.data[i + 2] !== 0
					) {
						count++;
					}
				}
				return count;
			});

			expect(nonBlackPixels).toBeGreaterThan(10);

			await expect(canvas).toHaveScreenshot("xeyes-canvas.png", {
				maxDiffPixelRatio: 0.01,
			});
		});

		test("multiple processes create multiple windows", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			const dock = page.locator('[data-testid="dock"]');
			await expect(dock).toContainText("test-sidecar", { timeout: 15_000 });

			await page
				.locator('input[placeholder="args"]')
				.fill("-geometry 200x150+10+10");

			// Spawn first
			await page.locator('[data-testid="spawn-button"]').click();
			const windows = page.locator('[data-testid="window-frame"]');
			await expect(windows.first()).toBeVisible({ timeout: 10_000 });

			// Spawn second
			await page.locator('[data-testid="spawn-button"]').click();
			await expect(windows).toHaveCount(2, { timeout: 10_000 });
		});

		test("xeyes pupils follow the cursor", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);

			const dock = page.locator('[data-testid="dock"]');
			await expect(dock).toContainText("test-sidecar", { timeout: 15_000 });

			await page
				.locator('input[placeholder="args"]')
				.fill("-geometry 300x200+10+10");
			await page.locator('[data-testid="spawn-button"]').click();

			const canvas = page.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible({ timeout: 10_000 });

			await page.waitForTimeout(3000);

			const box = await canvas.boundingBox();
			if (!box) throw new Error("Canvas has no bounding box");

			// Move to center
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await page.waitForTimeout(2000);
			await expect(canvas).toHaveScreenshot("xeyes-looking-center.png", {
				maxDiffPixelRatio: 0.01,
			});

			// Move to top-right
			await page.mouse.move(box.x + box.width - 10, box.y + 10);
			await page.waitForTimeout(2000);
			await expect(canvas).toHaveScreenshot("xeyes-looking-top-right.png", {
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
