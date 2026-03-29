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

/** Spawn an app and return the new window frame locator.
 *  Relies on afterEach killing all app processes so there are no
 *  accumulated windows from previous tests. */
async function spawnApp(
	page: Page,
	args = "",
	command = "xeyes",
): Promise<Locator> {
	const windowFrames = page.locator('[data-testid="window-frame"]');
	const countBefore = await windowFrames.count();

	await page.locator('[data-testid="spawn-button"]').click();
	if (command !== "xeyes") {
		await page.locator('input[placeholder="command"]').fill(command);
	}
	if (args) {
		await page.locator('input[placeholder="args"]').fill(args);
	}
	await expect(
		page.locator("button", { hasText: "Spawn" }),
	).toBeEnabled({ timeout: 30_000 });
	await page.locator("button", { hasText: "Spawn" }).click();

	await expect(windowFrames).toHaveCount(countBefore + 1, {
		timeout: 15_000,
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
						.withHostname("x11web")
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
			// Kill all spawned X11 app processes so the next test starts clean.
			// This prevents accumulated windows from interfering with spawnApp.
			await sidecarContainer
				?.exec([
					"bash",
					"-c",
					"pkill -9 -f 'xeyes|xterm|xlogo|xclock|xmessage|zenity|firefox|vim|gimp' 2>/dev/null; true",
				])
				.catch(() => {});
			// Wait for process cleanup to propagate through the system
			await new Promise((r) => setTimeout(r, 2000));

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

		test("xeyes canvas has rendered content", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 200x150+50+50");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			await expect
				.poll(async () => hasRenderedContent(canvas), {
					timeout: 15_000,
					intervals: [1000, 2000, 2000, 2000],
				})
				.toBe(true);
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
			await expect(win).toBeVisible();
			// Wait a moment for the window to stabilize
			await page.waitForTimeout(2000);

			await win.locator('[data-testid="window-close"]').click();
			await expect(windowFrames).toHaveCount(countBefore, {
				timeout: 10_000,
			});
		});

		test("closing one app does not affect other apps", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const windowFrames = page.locator('[data-testid="window-frame"]');

			// Spawn two different apps
			await spawnApp(page, "-geometry 200x150");
			await page.waitForTimeout(3000);

			await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			await page.waitForTimeout(5000);

			// Both should be visible
			await expect(windowFrames).toHaveCount(2, { timeout: 5_000 });

			// Close the first window (xeyes)
			await windowFrames.first().locator('[data-testid="window-close"]').click();

			// Should have 1 window remaining
			await expect(windowFrames).toHaveCount(1, { timeout: 10_000 });

			// The remaining window should still have rendered content
			const canvas = windowFrames.first().locator('[data-testid="x11-canvas"]');
			expect(await hasRenderedContent(canvas)).toBe(true);
		});

		test("multiple instances of same app get separate dock entries", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn three xeyes
			await spawnApp(page, "-geometry 100x80");
			await spawnApp(page, "-geometry 100x80");
			await spawnApp(page, "-geometry 100x80");
			await page.waitForTimeout(2000);

			// Dock should have 3 entries (one per process)
			const dockButtons = page.locator(
				'[data-testid="dock"] button:not([data-testid="spawn-button"])',
			);
			await expect(dockButtons).toHaveCount(3, { timeout: 5_000 });

			// Window frames should have 3 entries
			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames).toHaveCount(3, { timeout: 5_000 });
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

		test("resizing one window does not affect other windows", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn two windows and separate them so they don't overlap
			const win1 = await spawnApp(page, "-geometry 200x150+10+10");
			const canvas1 = win1.locator('[data-testid="x11-canvas"]');
			await expect(canvas1).toBeVisible();

			const win2 = await spawnApp(page, "-geometry 200x150+10+10");
			const canvas2 = win2.locator('[data-testid="x11-canvas"]');
			await expect(canvas2).toBeVisible();
			await page.waitForTimeout(3000);

			// Drag win2 out of the way so win1's resize handle is accessible
			const titleBar2 = win2.locator('[class*="header"]');
			const tb2Box = await titleBar2.boundingBox();
			if (tb2Box) {
				await page.mouse.move(tb2Box.x + 50, tb2Box.y + 10);
				await page.mouse.down();
				await page.mouse.move(tb2Box.x + 400, tb2Box.y + 10, { steps: 5 });
				await page.mouse.up();
			}
			await page.waitForTimeout(1000);

			// Record both canvas sizes
			const size1Before = await canvas1.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));
			const size2Before = await canvas2.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));

			// Resize only win1 via its SE drag handle
			const box1 = await win1.boundingBox();
			if (!box1) throw new Error("Window 1 has no bounding box");
			const startX = box1.x + box1.width - 5;
			const startY = box1.y + box1.height - 5;
			await page.mouse.move(startX, startY);
			await page.mouse.down();
			await page.mouse.move(startX + 100, startY + 80, { steps: 10 });
			await page.mouse.up();
			await page.waitForTimeout(3000);

			// Win1 should have grown
			const size1After = await canvas1.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));
			expect(size1After.width).toBeGreaterThan(size1Before.width);
			expect(size1After.height).toBeGreaterThan(size1Before.height);

			// Win2 should be unchanged
			const size2After = await canvas2.evaluate((el: HTMLCanvasElement) => ({
				width: el.width,
				height: el.height,
			}));
			expect(size2After.width).toBe(size2Before.width);
			expect(size2After.height).toBe(size2Before.height);
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

		test("dock icon click brings window to front", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn xeyes first, then xterm on top
			await spawnApp(page, "-geometry 200x150+50+50");
			const win2 = await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			await page.waitForTimeout(3000);

			// xterm (win2) is on top
			const z2Before = await win2.evaluate((el) =>
				Number.parseInt(el.style.zIndex || "0"),
			);

			// Click the first dock icon (xeyes) to bring it to front
			const dockButtons = page.locator('[data-testid="dock"] button');
			await dockButtons.first().click();
			await page.waitForTimeout(500);

			// xeyes window should now have a higher z-index than xterm
			const allFrames = page.locator('[data-testid="window-frame"]');
			const frame1Z = await allFrames
				.first()
				.evaluate((el) => Number.parseInt(el.style.zIndex || "0"));
			expect(frame1Z).toBeGreaterThan(z2Before);
		});

		test("keyboard input follows canvas focus between windows", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn two xterms
			const win1 = await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			const canvas1 = win1.locator('[data-testid="x11-canvas"]');
			await expect(canvas1).toBeVisible();
			await page.waitForTimeout(5000);

			const win2 = await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			const canvas2 = win2.locator('[data-testid="x11-canvas"]');
			await expect(canvas2).toBeVisible();
			await page.waitForTimeout(5000);

			// Move win2 so both canvases are accessible
			const tb2 = win2.locator('[class*="header"]');
			const tb2Box = await tb2.boundingBox();
			if (tb2Box) {
				await page.mouse.move(tb2Box.x + 50, tb2Box.y + 10);
				await page.mouse.down();
				await page.mouse.move(tb2Box.x + 400, tb2Box.y + 10, { steps: 5 });
				await page.mouse.up();
			}
			await page.waitForTimeout(1000);

			// Type in xterm 1
			await canvas1.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("echo AAA", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(2000);

			// Screenshot xterm 1 after typing AAA
			await expect(canvas1).toHaveScreenshot("xterm1-after-aaa.png", {
				maxDiffPixelRatio: 0.1,
			});

			// Switch to xterm 2 and type
			await canvas2.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("echo BBB", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(2000);

			// Screenshot xterm 2 after typing BBB
			await expect(canvas2).toHaveScreenshot("xterm2-after-bbb.png", {
				maxDiffPixelRatio: 0.1,
			});

			// Switch BACK to xterm 1 and type more
			await canvas1.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("echo CCC", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(2000);

			// Screenshot xterm 1 after typing CCC — should show both AAA and CCC
			await expect(canvas1).toHaveScreenshot("xterm1-after-ccc.png", {
				maxDiffPixelRatio: 0.1,
			});

			// xterm 2 should still only show BBB (not CCC)
			await expect(canvas2).toHaveScreenshot("xterm2-unchanged.png", {
				maxDiffPixelRatio: 0.1,
			});
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
				maxDiffPixelRatio: 0.1,
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
				maxDiffPixelRatio: 0.1,
			});
		});

		test("xterm renders text on the canvas", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			await expect(canvas).toHaveScreenshot("xterm-canvas.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		test("xterm accepts keyboard input", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			await canvas.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("echo hello", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			await expect(canvas).toHaveScreenshot("xterm-keyboard.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		test("window content survives page refresh", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 40x10", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			// Verify content is rendered
			expect(await hasRenderedContent(canvas)).toBe(true);

			// Refresh the page
			await page.reload();
			await waitForDock(page);

			// The window should reappear with content
			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames.first()).toBeVisible({ timeout: 10_000 });
			const restoredCanvas = windowFrames
				.first()
				.locator('[data-testid="x11-canvas"]');
			await page.waitForTimeout(5000);
			expect(await hasRenderedContent(restoredCanvas)).toBe(true);
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

			await expect(canvas).toHaveScreenshot("zenity-canvas.png", {
				maxDiffPixelRatio: 0.1,
				timeout: 15_000,
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

			await expect(canvas).toHaveScreenshot("zenity-question.png", {
				maxDiffPixelRatio: 0.1,
				timeout: 15_000,
			});
		});

		test("vim workflow: insert, save, quit, cat", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			await expect
				.poll(async () => hasRenderedContent(canvas), {
					timeout: 15_000,
					intervals: [500, 1000, 2000, 2000],
				})
				.toBe(true);

			await canvas.click();
			await page.waitForTimeout(500);

			await page.keyboard.type("vim /tmp/test.txt", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			await expect(canvas).toHaveScreenshot("vim-opened.png", {
				maxDiffPixelRatio: 0.05,
			});

			await page.keyboard.press("i");
			await page.waitForTimeout(1000);
			await page.keyboard.type("Hello from x11-web!", { delay: 30 });
			await page.waitForTimeout(2000);

			await expect(canvas).toHaveScreenshot("vim-insert.png", {
				maxDiffPixelRatio: 0.05,
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
				maxDiffPixelRatio: 0.05,
			});
		});

		test("firefox starts without crashing the sidecar", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("firefox-esr");
			await page.locator('input[placeholder="args"]').fill("--no-remote about:blank");
			await expect(
				page.locator("button", { hasText: "Spawn" }),
			).toBeEnabled({ timeout: 30_000 });
			await page.locator("button", { hasText: "Spawn" }).click();

			// Wait for Firefox to start and create windows
			await page.waitForTimeout(15000);

			// At least one window frame should have been created
			const windowFrames = page.locator('[data-testid="window-frame"]');
			expect(await windowFrames.count()).toBeGreaterThan(0);

			// The afterEach health check verifies the sidecar survived
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
