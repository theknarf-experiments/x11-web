import { type ChildProcess, exec } from "node:child_process";
import * as http from "node:http";
import * as path from "node:path";
import { expect, type Page, test } from "@playwright/test";
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

/** Open the spawn popover, fill in args, and click Spawn */
async function spawnApp(page: Page, args = "") {
	const spawnBtn = page.locator('[data-testid="spawn-button"]');
	await spawnBtn.click();
	if (args) {
		await page.locator('input[placeholder="args"]').fill(args);
	}
	// Click the Spawn button inside the popover
	await page.locator("button", { hasText: "Spawn" }).click();
}

/** Wait for the dock to be ready (visible with status indicator) */
async function waitForDock(page: Page) {
	const dock = page.locator('[data-testid="dock"]');
	await expect(dock).toBeVisible({ timeout: 15_000 });
	// Wait for the connection status dot to be green
	await expect(page.locator('[data-testid="spawn-button"]')).toBeVisible({
		timeout: 15_000,
	});
}

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

		test("dock is visible", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);
		});

		test("spawning xeyes creates a window on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await spawnApp(page, "-geometry 300x200+10+10");

			const windowFrame = page.locator('[data-testid="window-frame"]');
			await expect(windowFrame).toBeVisible({ timeout: 10_000 });

			const canvas = windowFrame.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			await page.waitForTimeout(5000);

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
			await waitForDock(page);

			await spawnApp(page, "-geometry 200x150+10+10");
			const windows = page.locator('[data-testid="window-frame"]');
			await expect(windows.first()).toBeVisible({ timeout: 10_000 });

			await spawnApp(page, "-geometry 200x150+10+10");
			await expect(windows).toHaveCount(2, { timeout: 10_000 });
		});

		test("closing a window removes it", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await spawnApp(page, "-geometry 200x150+10+10");

			const windowFrame = page.locator('[data-testid="window-frame"]');
			await expect(windowFrame).toBeVisible({ timeout: 10_000 });

			const canvas = windowFrame.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(3000);

			const pixelsBefore = await canvas.evaluate((el: HTMLCanvasElement) => {
				const ctx = el.getContext("2d");
				if (!ctx) return 0;
				const d = ctx.getImageData(0, 0, el.width, el.height);
				let n = 0;
				for (let i = 0; i < d.data.length; i += 4) {
					if (d.data[i] || d.data[i + 1] || d.data[i + 2]) n++;
				}
				return n;
			});
			expect(pixelsBefore).toBeGreaterThan(10);

			await windowFrame.locator('[data-testid="window-close"]').click();
			await expect(windowFrame).toHaveCount(0, { timeout: 10_000 });
		});

		test("resizing a window changes the canvas dimensions", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await spawnApp(page, "-geometry 300x200+10+10");

			const windowFrame = page.locator('[data-testid="window-frame"]');
			await expect(windowFrame).toBeVisible({ timeout: 10_000 });

			const canvas = windowFrame.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(3000);

			const initialSize = await canvas.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));

			const handleBox = await windowFrame.boundingBox();
			if (!handleBox) throw new Error("Window has no bounding box");

			const startX = handleBox.x + handleBox.width - 5;
			const startY = handleBox.y + handleBox.height - 5;
			await page.mouse.move(startX, startY);
			await page.mouse.down();
			await page.mouse.move(startX + 100, startY + 80, { steps: 5 });
			await page.mouse.up();

			await page.waitForTimeout(2000);

			const newSize = await canvas.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));

			expect(newSize.width).toBeGreaterThan(initialSize.width);
			expect(newSize.height).toBeGreaterThan(initialSize.height);
		});

		test("xeyes pupils follow the cursor", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await spawnApp(page, "-geometry 300x200+10+10");

			const canvas = page.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible({ timeout: 10_000 });

			await page.waitForTimeout(3000);

			const box = await canvas.boundingBox();
			if (!box) throw new Error("Canvas has no bounding box");

			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await page.waitForTimeout(2000);
			await expect(canvas).toHaveScreenshot("xeyes-looking-center.png", {
				maxDiffPixelRatio: 0.01,
			});

			await page.mouse.move(box.x + box.width - 10, box.y + 10);
			await page.waitForTimeout(2000);
			await expect(canvas).toHaveScreenshot("xeyes-looking-top-right.png", {
				maxDiffPixelRatio: 0.01,
			});
		});

		test("xlogo renders on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Change command to xlogo
			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("xlogo");
			await page.locator('input[placeholder="args"]').fill("-geometry 100x100");
			await page.locator("button", { hasText: "Spawn" }).click();

			const canvas = page.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible({ timeout: 10_000 });
			await page.waitForTimeout(5000);

			const nonBlackPixels = await canvas.evaluate((el: HTMLCanvasElement) => {
				const ctx = el.getContext("2d");
				if (!ctx) return 0;
				const d = ctx.getImageData(0, 0, el.width, el.height);
				let n = 0;
				for (let i = 0; i < d.data.length; i += 4) {
					if (d.data[i] || d.data[i + 1] || d.data[i + 2]) n++;
				}
				return n;
			});
			expect(nonBlackPixels).toBeGreaterThan(10);
		});

		test("xclock renders on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("xclock");
			await page.locator('input[placeholder="args"]').fill("-update 1");
			await page.locator("button", { hasText: "Spawn" }).click();

			const canvas = page.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible({ timeout: 10_000 });
			await page.waitForTimeout(5000);

			const nonBlackPixels = await canvas.evaluate((el: HTMLCanvasElement) => {
				const ctx = el.getContext("2d");
				if (!ctx) return 0;
				const d = ctx.getImageData(0, 0, el.width, el.height);
				let n = 0;
				for (let i = 0; i < d.data.length; i += 4) {
					if (d.data[i] || d.data[i + 1] || d.data[i + 2]) n++;
				}
				return n;
			});
			expect(nonBlackPixels).toBeGreaterThan(10);
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
