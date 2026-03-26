import { type ChildProcess, exec } from "node:child_process";
import * as http from "node:http";
import * as path from "node:path";
import { expect, type Locator, type Page, test } from "@playwright/test";
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

/** Spawn an app and return the new window frame locator */
async function spawnApp(
	page: Page,
	args = "",
	command = "xeyes",
): Promise<Locator> {
	const windowFrames = page.locator('[data-testid="window-frame"]');
	// Wait for existing windows to stabilize (from replay on reconnect)
	await page.waitForTimeout(500);
	const countBefore = await windowFrames.count();

	await page.locator('[data-testid="spawn-button"]').click();
	if (command !== "xeyes") {
		await page.locator('input[placeholder="command"]').fill(command);
	}
	if (args) {
		await page.locator('input[placeholder="args"]').fill(args);
	}
	// Wait for the Spawn button to be enabled (sidecar must be connected)
	await expect(
		page.locator("button", { hasText: "Spawn" }),
	).toBeEnabled({ timeout: 30_000 });
	await page.locator("button", { hasText: "Spawn" }).click();

	await expect(windowFrames).toHaveCount(countBefore + 1, {
		timeout: 10_000,
	});
	return windowFrames.nth(countBefore);
}

async function waitForDock(page: Page) {
	const dock = page.locator('[data-testid="dock"]');
	await expect(dock).toBeVisible({ timeout: 15_000 });
	await expect(page.locator('[data-testid="spawn-button"]')).toBeVisible({
		timeout: 15_000,
	});
}

async function countNonBlackPixels(canvas: Locator): Promise<number> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return 0;
		const d = ctx.getImageData(0, 0, el.width, el.height);
		let n = 0;
		for (let i = 0; i < d.data.length; i += 4) {
			if (d.data[i] || d.data[i + 1] || d.data[i + 2]) n++;
		}
		return n;
	});
}

/** Count unique non-background colors (detects text/graphics on solid background). */
async function hasRenderedContent(canvas: Locator): Promise<boolean> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return false;
		const d = ctx.getImageData(0, 0, el.width, el.height);
		const colors = new Set<number>();
		for (let i = 0; i < d.data.length; i += 4) {
			const c = (d.data[i] << 16) | (d.data[i + 1] << 8) | d.data[i + 2];
			colors.add(c);
			if (colors.size >= 2) return true; // Multiple colors = has content
		}
		return false;
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
							NO_AT_BRIDGE: "1",
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

		test.afterEach(async () => {
			// Verify neither the backend nor sidecar has crashed
			const backendRunning = await backendContainer
				?.exec(["true"])
				.then(() => true)
				.catch(() => false);
			const sidecarRunning = await sidecarContainer
				?.exec(["true"])
				.then(() => true)
				.catch(() => false);
			expect(
				backendRunning,
				"Backend container crashed during test",
			).toBe(true);
			expect(
				sidecarRunning,
				"Sidecar container crashed during test",
			).toBe(true);
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

			const win = await spawnApp(page, "-geometry 300x200+10+10");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			const pixels = await countNonBlackPixels(canvas);
			expect(pixels).toBeGreaterThan(10);

			await expect(canvas).toHaveScreenshot("xeyes-canvas.png", {
				maxDiffPixelRatio: 0.01,
			});
		});

		test("multiple processes create multiple windows", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const windowFrames = page.locator('[data-testid="window-frame"]');
			const countBefore = await windowFrames.count();

			await spawnApp(page, "-geometry 200x150+10+10");
			await spawnApp(page, "-geometry 200x150+10+10");

			await expect(windowFrames).toHaveCount(countBefore + 2, {
				timeout: 10_000,
			});
		});

		test("closing a window removes it", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const windowFrames = page.locator('[data-testid="window-frame"]');
			const countBefore = await windowFrames.count();

			const win = await spawnApp(page, "-geometry 200x150+10+10");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(3000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(10);

			await win.locator('[data-testid="window-close"]').click();
			await expect(windowFrames).toHaveCount(countBefore, {
				timeout: 10_000,
			});
		});

		test("resizing a window changes the canvas dimensions", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 300x200+10+10");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(3000);

			const initialSize = await canvas.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));

			const handleBox = await win.boundingBox();
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

		test("clicking a window brings it to front", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn two windows
			const win1 = await spawnApp(page, "-geometry 200x150+50+50");
			const win2 = await spawnApp(page, "-geometry 200x150+100+100");
			await expect(win1).toBeVisible();
			await expect(win2).toBeVisible();

			// win2 was spawned second, so it should have higher z-index initially
			const z2Before = await win2.evaluate((el) =>
				Number.parseInt(el.style.zIndex || "0"),
			);
			const z1Before = await win1.evaluate((el) =>
				Number.parseInt(el.style.zIndex || "0"),
			);
			expect(z2Before).toBeGreaterThan(z1Before);

			// Directly trigger pointerdown on win1 to bring it to front
			await win1.dispatchEvent("pointerdown");
			await page.waitForTimeout(300);

			const z1After = await win1.evaluate((el) =>
				Number.parseInt(el.style.zIndex || "0"),
			);
			expect(z1After).toBeGreaterThan(z2Before);
		});

		test("xeyes pupils follow the cursor", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 300x200+10+10");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
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

			const win = await spawnApp(page, "-geometry 100x100", "xlogo");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(100);
			await expect(canvas).toHaveScreenshot("xlogo-canvas.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		test("xclock renders on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "", "xclock");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(100);
			await expect(canvas).toHaveScreenshot("xclock-canvas.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		test("xterm renders text on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 40x10", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(50);
			await expect(canvas).toHaveScreenshot("xterm-canvas.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		test("xterm accepts keyboard input", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			await canvas.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("echo hello", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			// Text should have rendered — at minimum the "echo hello" output
			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(50);
		});

		test("xmessage renders on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(
				page,
				'-center "Hello World"',
				"xmessage",
			);
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(3000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(100);
			await expect(canvas).toHaveScreenshot("xmessage-canvas.png", {
				maxDiffPixelRatio: 0.1,
			});
		});

		test("GTK app renders on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(
				page,
				'--info --text "Hello from GTK" --title "GTK Test"',
				"zenity",
			);
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			// Zenity should render something (GTK dialog with text)
			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(100);

			await expect(canvas).toHaveScreenshot("zenity-canvas.png", {
				maxDiffPixelRatio: 0.15,
			});
		});

		test("zenity question dialog renders", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(
				page,
				'--question --text "Are you sure?" --title "Confirm"',
				"zenity",
			);
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			expect(await countNonBlackPixels(canvas)).toBeGreaterThan(100);
			await expect(canvas).toHaveScreenshot("zenity-question.png", {
				maxDiffPixelRatio: 0.15,
			});
		});

		test("vim workflow: insert, save, quit, cat", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			// Wait until xterm renders text (more than just a solid color)
			await expect
				.poll(async () => hasRenderedContent(canvas), {
					timeout: 15_000,
					intervals: [500, 1000, 1000, 2000, 2000],
				})
				.toBe(true);

			await canvas.click();
			await page.waitForTimeout(500);

			await page.keyboard.type("vim /tmp/test.txt", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			await expect(canvas).toHaveScreenshot("vim-opened.png", {
				maxDiffPixelRatio: 0.15,
			});

			await page.keyboard.press("i");
			await page.waitForTimeout(1000);
			await page.keyboard.type("Hello from x11-web!", { delay: 30 });
			await page.waitForTimeout(2000);

			await expect(canvas).toHaveScreenshot("vim-insert.png", {
				maxDiffPixelRatio: 0.15,
			});

			await page.keyboard.press("Escape");
			await page.waitForTimeout(500);
			await page.keyboard.type(":wq", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			await page.keyboard.type("cat /tmp/test.txt", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			await expect(canvas).toHaveScreenshot("vim-after-save.png", {
				maxDiffPixelRatio: 0.15,
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
