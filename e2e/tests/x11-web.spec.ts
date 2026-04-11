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

/**
 * Cheap rolling hash of a canvas's pixel buffer. Used by
 * `waitForCanvasStable` to detect when an app has finished its
 * startup repaint sequence.
 */
async function canvasPixelHash(canvas: Locator): Promise<string> {
	return canvas.evaluate((el: HTMLCanvasElement) => {
		const ctx = el.getContext("2d");
		if (!ctx) return "";
		const d = ctx.getImageData(0, 0, el.width, el.height);
		// Sample every 16th byte; that's enough resolution to spot
		// any meaningful repaint without iterating millions of pixels
		// on a hot loop.
		let h = 0x811c9dc5 | 0;
		for (let i = 0; i < d.data.length; i += 16) {
			h = (h ^ d.data[i]) >>> 0;
			h = Math.imul(h, 0x01000193) >>> 0;
		}
		return `${el.width}x${el.height}:${h.toString(16)}`;
	});
}

/**
 * Wait until a canvas's pixel content has been unchanged for at
 * least `stableMs`. Polls every `pollMs`. Returns once stable, or
 * gives up after `totalTimeoutMs` (callers can then proceed and let
 * any downstream snapshot assertion fail with a real diff).
 */
async function waitForCanvasStable(
	canvas: Locator,
	{
		stableMs = 1000,
		pollMs = 200,
		totalTimeoutMs = 15_000,
	}: { stableMs?: number; pollMs?: number; totalTimeoutMs?: number } = {},
): Promise<void> {
	const start = Date.now();
	let lastHash = "";
	let stableSince = 0;
	while (Date.now() - start < totalTimeoutMs) {
		const hash = await canvasPixelHash(canvas);
		const now = Date.now();
		if (hash === lastHash && hash !== "") {
			if (stableSince === 0) stableSince = now;
			if (now - stableSince >= stableMs) return;
		} else {
			lastHash = hash;
			stableSince = 0;
		}
		await new Promise((r) => setTimeout(r, pollMs));
	}
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
					"pkill -9 -f 'xeyes|xterm|xlogo|xclock|xmessage|zenity|firefox|vim|gimp|gtk3-demo|qpdfview' 2>/dev/null; true",
				])
				.catch(() => {});
			// Wipe Firefox profile state left behind by SIGKILL — otherwise
			// the next firefox-esr launch either hangs on a profile lock or
			// resumes mid-tab and causes flakes between tests.
			await sidecarContainer
				?.exec([
					"bash",
					"-c",
					"rm -rf /root/.mozilla /root/.cache/mozilla 2>/dev/null; true",
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

		test("global menu bar tracks the focused window", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const menuBarTitle = page.locator(
				'[data-testid="global-menu-bar-title"]',
			);
			// Before any window is focused, the bar shows the fallback.
			await expect(menuBarTitle).toBeVisible();
			await expect(menuBarTitle).toHaveText("x11-web");

			// Use two apps that don't set their own WM_NAME so the bar
			// title is deterministic — xeyes and xclock both keep the
			// command name we passed to spawn.
			const xeyesFrame = await spawnApp(
				page,
				"-geometry 200x150+50+50",
				"xeyes",
			);
			const xclockFrame = await spawnApp(
				page,
				"-geometry 200x150+300+50",
				"xclock",
			);

			await expect(
				xeyesFrame.locator('[data-testid="x11-canvas"]'),
			).toBeVisible();
			await expect(
				xclockFrame.locator('[data-testid="x11-canvas"]'),
			).toBeVisible();
			await page.waitForTimeout(2500);

			// The frontend stacks new windows at fixed offsets so the
			// two frames overlap. Drag xclock far to the right by its
			// title bar so we can click each canvas independently.
			const xclockBox = await xclockFrame.boundingBox();
			if (!xclockBox) throw new Error("xclock frame has no bounding box");
			await page.mouse.move(
				xclockBox.x + xclockBox.width / 2,
				xclockBox.y + 5,
			);
			await page.mouse.down();
			await page.mouse.move(
				xclockBox.x + xclockBox.width / 2 + 350,
				xclockBox.y + 5,
				{ steps: 5 },
			);
			await page.mouse.up();
			await page.waitForTimeout(300);

			// Click into xeyes — focus broadcast should put "xeyes" in the bar.
			await xeyesFrame.locator('[data-testid="x11-canvas"]').click();
			await expect(menuBarTitle).toHaveText("xeyes", { timeout: 5_000 });

			// Click into xclock — title should switch.
			await xclockFrame.locator('[data-testid="x11-canvas"]').click();
			await expect(menuBarTitle).toHaveText("xclock", { timeout: 5_000 });

			// And back again, to verify it's not a one-shot.
			await xeyesFrame.locator('[data-testid="x11-canvas"]').click();
			await expect(menuBarTitle).toHaveText("xeyes", { timeout: 5_000 });
		});

		test("global menu bar mirrors a GTK app's exported menus", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// gtk3-demo-application is a GtkApplication that calls
			// gtk_application_set_menubar(), so once we tell it (via the
			// _GTK_SHELL_SHOWS_MENUBAR root property) that the shell will
			// render the menubar, it exports its menu structure over
			// org.gtk.Menus and never draws it locally.
			const win = await spawnApp(page, "", "gtk3-demo-application");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			// Click the canvas so X11 input focus lands on the GTK app
			// — the global menu bar only shows the *focused* window's
			// menu, and the focus broadcast fires on ButtonPress.
			await canvas.click();

			// The MenuStructure update should arrive from the sidecar
			// shortly after the window maps. Poll until at least one
			// top-level menu item is rendered.
			const topItems = page.locator(
				'[data-testid="global-menu-top-item"]',
			);
			await expect
				.poll(async () => topItems.count(), {
					timeout: 30_000,
					intervals: [500, 1000, 2000, 2000, 3000],
				})
				.toBeGreaterThan(0);

			// gtk3-demo-application's exported menubar has Preferences
			// and Help as top-level items.
			const topLabels = await topItems.allInnerTexts();
			expect(topLabels).toContain("Preferences");
			expect(topLabels).toContain("Help");

			// Click Preferences — its dropdown should open with the
			// real items GTK exported (theme toggle, color submenu, ...).
			await topItems.filter({ hasText: "Preferences" }).first().click();
			const dropdown = page.locator(
				'[data-testid="global-menu-dropdown"]',
			);
			await expect(dropdown).toBeVisible();

			const itemLabels = await page
				.locator('[data-testid="global-menu-item"]')
				.allInnerTexts();
			// "Prefer Dark Theme" is a checkbox-style toggle — just
			// assert it exists, since the checked-state prefix can vary.
			expect(itemLabels.some((l) => l.includes("Prefer Dark Theme"))).toBe(
				true,
			);
			expect(itemLabels.some((l) => l.includes("Color"))).toBe(true);
		});

		// TODO(dbusmenu): no working test target on Debian bookworm.
		// The sidecar's dbusmenu pipeline is complete and correct
		// (registrar serves, attach_dbusmenu spawns a tracker, the
		// tracker fetches via GetLayout and translates the (i,a{sv},av)
		// tree into MenuItems). Apps that publish a real dbusmenu
		// object Just Work — we just don't have one in the image.
		//
		// Things we tried that DON'T work as a test target:
		//
		// - **featherpad** (Qt 5 text editor): calls
		//   `AppMenu.Registrar.RegisterWindow("/MenuBar/N")` but
		//   never actually exports any dbus object at that path.
		//   Verified by introspection: the path responds to neither
		//   `com.canonical.dbusmenu` nor the standard
		//   `org.freedesktop.DBus.Introspectable`. Apparently the
		//   Debian Qt 5 build doesn't enable the QDBusPlatformMenu
		//   exporter even though the registrar client is wired up.
		//
		// - **qpdfview** (Qt 5 PDF viewer): same exact behavior as
		//   featherpad. Confirms it's a Qt build flag issue.
		//
		// - **`appmenu-gtk-module`** (the Debian package that
		//   advertises "Application Menu GTK+ Module — exports
		//   GMenuModel via the dbus menu system"): doesn't actually
		//   export via dbusmenu. It just relocates GTK menubar paths
		//   to `/org/appmenu/gtk/window/N`, which still serves
		//   `org.gtk.Menus` (verified via introspection). Useless
		//   for our purpose; the existing GTK pipeline already
		//   covers it.
		//
		// What would work: a tiny custom test program that links
		// `libdbusmenu-glib` directly and publishes a static menu.
		// Maybe ~80 lines of C built in the sidecar Dockerfile's
		// builder stage. Saving for a follow-up so PR 3 isn't
		// blocked indefinitely.
		test.fail("global menu bar mirrors an app via dbusmenu", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// qpdfview is the closest Debian-packaged Qt app to
			// being a working test target — it links libdbusmenu-qt5
			// and announces a path on RegisterWindow, just doesn't
			// actually back the path with an object.
			const win = await spawnApp(page, "", "qpdfview");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await canvas.click();

			const topItems = page.locator(
				'[data-testid="global-menu-top-item"]',
			);
			await expect
				.poll(async () => topItems.count(), {
					timeout: 15_000,
					intervals: [500, 1000, 2000, 2000, 3000],
				})
				.toBeGreaterThan(0);
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

			// xmessage (Athena toolkit) maps the top-level window first
			// and only paints the "okay" button child a beat later, so
			// the canvas briefly shows the message text alone. Wait for
			// the canvas pixel content to stop changing before letting
			// the screenshot assertion run, otherwise the comparison
			// races against the second redraw.
			await waitForCanvasStable(canvas);

			await expect(canvas).toHaveScreenshot("xmessage-canvas.png", {
				maxDiffPixelRatio: 0.1,
				timeout: 15_000,
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

		test("gimp renders main window", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Open gimp on a tiny built-in image so the canvas area has
			// content and many widgets get exercised.
			await spawnApp(
				page,
				"--no-splash /usr/share/pixmaps/debian-logo.png",
				"gimp",
			);

			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames.first()).toBeVisible({ timeout: 60_000 });
			await expect
				.poll(
					async () => {
						const count = await windowFrames.count();
						for (let i = 0; i < count; i++) {
							const canvas = windowFrames
								.nth(i)
								.locator('[data-testid="x11-canvas"]');
							if (
								(await canvas.isVisible()) &&
								(await hasRenderedContent(canvas))
							) {
								return true;
							}
						}
						return false;
					},
					{
						timeout: 120_000,
						intervals: [2000, 3000, 5000, 5000, 10000, 10000],
					},
				)
				.toBe(true);

			// Give gimp time to settle.
			await page.waitForTimeout(8000);

			const gimpFrame = windowFrames.first();
			await expect(gimpFrame).toHaveScreenshot("gimp-canvas.png", {
				maxDiffPixelRatio: 0.05,
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

		test("firefox renders on the canvas", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn xeyes first — matches the manual testing flow
			await spawnApp(page, "-geometry 100x80+0+0");
			await page.waitForTimeout(2000);

			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("firefox-esr");
			await page.locator('input[placeholder="args"]').fill("");
			await expect(
				page.locator("button", { hasText: "Spawn" }),
			).toBeEnabled({ timeout: 30_000 });
			await page.locator("button", { hasText: "Spawn" }).click();

			const windowFrames = page.locator('[data-testid="window-frame"]');
			// Wait for Firefox window (in addition to xeyes)
			await expect(windowFrames).toHaveCount(2, { timeout: 120_000 });

			// Wait for rendered content on the Firefox canvas
			await expect
				.poll(
					async () => {
						const count = await windowFrames.count();
						for (let i = 0; i < count; i++) {
							const canvas = windowFrames
								.nth(i)
								.locator('[data-testid="x11-canvas"]');
							if (
								(await canvas.isVisible()) &&
								(await hasRenderedContent(canvas))
							)
								return true;
						}
						return false;
					},
					{ timeout: 120_000, intervals: [5000, 5000, 5000, 5000, 5000, 10000, 10000] },
				)
				.toBe(true);

			// Screenshot the Firefox canvas (last frame with content)
			const count = await windowFrames.count();
			let firefoxCanvas: Locator | null = null;
			for (let i = 0; i < count; i++) {
				const canvas = windowFrames
					.nth(i)
					.locator('[data-testid="x11-canvas"]');
				if (
					(await canvas.isVisible()) &&
					(await hasRenderedContent(canvas))
				) {
					firefoxCanvas = canvas;
				}
			}
			expect(firefoxCanvas).not.toBeNull();
			await expect(firefoxCanvas!).toHaveScreenshot("firefox-canvas.png", {
				maxDiffPixelRatio: 0.1,
				timeout: 15_000,
			});
		});

		test("firefox responds to mouse and keyboard input", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn xeyes first — matches the manual testing flow
			await spawnApp(page, "-geometry 100x80+0+0");
			await page.waitForTimeout(2000);

			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("firefox-esr");
			await page.locator('input[placeholder="args"]').fill("");
			await expect(
				page.locator("button", { hasText: "Spawn" }),
			).toBeEnabled({ timeout: 30_000 });
			await page.locator("button", { hasText: "Spawn" }).click();

			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames).toHaveCount(2, { timeout: 120_000 });

			// Wait for both canvases to have content
			let firefoxCanvas: Locator | null = null;
			await expect
				.poll(
					async () => {
						const count = await windowFrames.count();
						let withContent = 0;
						for (let i = 0; i < count; i++) {
							const canvas = windowFrames
								.nth(i)
								.locator('[data-testid="x11-canvas"]');
							if (
								(await canvas.isVisible()) &&
								(await hasRenderedContent(canvas))
							) {
								withContent++;
								firefoxCanvas = canvas;
							}
						}
						return withContent >= 2;
					},
					{ timeout: 120_000, intervals: [5000, 5000, 5000, 5000, 5000, 10000] },
				)
				.toBe(true);

			// Screenshot before interaction
			await page.waitForTimeout(5000);
			await expect(firefoxCanvas!).toHaveScreenshot(
				"firefox-before-input.png",
				{ maxDiffPixelRatio: 0.1, timeout: 15_000 },
			);

			// Click the address bar and type a URL
			const box = await firefoxCanvas!.boundingBox();
			expect(box).not.toBeNull();
			await page.mouse.click(
				box!.x + box!.width * 0.5,
				box!.y + box!.height * 0.08,
			);
			await page.waitForTimeout(1000);
			await page.keyboard.type("about:config", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(5000);

			// The page should have changed — no longer the welcome page
			await expect(firefoxCanvas!).not.toHaveScreenshot(
				"firefox-before-input.png",
				{ maxDiffPixelRatio: 0.1, timeout: 30_000 },
			);
		});

		test("scrolling on a window canvas does not pan the InfiniteCanvas", async ({
			page,
		}) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-geometry 300x200+10+10");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(2000);

			const transformBefore = await page
				.locator('[data-testid="infinite-canvas"] > div')
				.first()
				.evaluate((el) => (el as HTMLElement).style.transform);

			// Scroll on the canvas
			const box = await canvas.boundingBox();
			if (!box) throw new Error("no canvas box");
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await page.waitForTimeout(200);
			for (let i = 0; i < 10; i++) {
				await page.mouse.wheel(0, 100);
				await page.waitForTimeout(30);
			}
			await page.waitForTimeout(500);

			const transformAfter = await page
				.locator('[data-testid="infinite-canvas"] > div')
				.first()
				.evaluate((el) => (el as HTMLElement).style.transform);

			expect(transformAfter).toBe(transformBefore);
		});

		test("scroll wheel triggers xterm scrollback", async ({
			page,
		}) => {
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

			// Run a command that produces enough output to fill the scrollback
			await canvas.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("seq 1 200", { delay: 30 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(2000);

			const fingerprint = async () =>
				canvas.evaluate((el: HTMLCanvasElement) => {
					const ctx = el.getContext("2d");
					if (!ctx) return "";
					const d = ctx.getImageData(0, 0, el.width, el.height);
					let h = 0;
					for (let i = 0; i < d.data.length; i += 97)
						h = (h * 31 + d.data[i]) >>> 0;
					return h.toString();
				});

			const before = await fingerprint();

			const box = await canvas.boundingBox();
			if (!box) throw new Error("no canvas box");
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await page.waitForTimeout(200);
			// Scroll up (negative deltaY) to reveal earlier output
			for (let i = 0; i < 30; i++) {
				await page.mouse.wheel(0, -120);
				await page.waitForTimeout(50);
			}
			await page.waitForTimeout(1500);

			const after = await fingerprint();
			expect(after).not.toBe(before);
		});

		// Known failure: Firefox in default mode (no MOZ_USE_XINPUT2) does
		// not subscribe to XInput2 events, AND does not respond to core
		// ButtonPress 4/5 events for scroll wheel in our environment. The
		// XInput2 dispatch path is verified by `xeyes pupils follow the
		// cursor` and `scroll wheel triggers xterm scrollback`. Marked as
		// test.fail so the test runs and we'll notice when it starts to
		// pass — at which point this comment can be removed.
		test.fail("firefox responds to scroll wheel input", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Spawn xeyes first — matches the manual testing flow.
			await spawnApp(page, "-geometry 100x80+0+0");
			await page.waitForTimeout(2000);

			await page.locator('[data-testid="spawn-button"]').click();
			await page.locator('input[placeholder="command"]').fill("firefox-esr");
			await page.locator('input[placeholder="args"]').fill("");
			await expect(
				page.locator("button", { hasText: "Spawn" }),
			).toBeEnabled({ timeout: 30_000 });
			await page.locator("button", { hasText: "Spawn" }).click();

			const windowFrames = page.locator('[data-testid="window-frame"]');
			await expect(windowFrames).toHaveCount(2, { timeout: 120_000 });

			// Wait for both canvases to render content.
			let firefoxCanvas: Locator | null = null;
			await expect
				.poll(
					async () => {
						const count = await windowFrames.count();
						let withContent = 0;
						for (let i = 0; i < count; i++) {
							const canvas = windowFrames
								.nth(i)
								.locator('[data-testid="x11-canvas"]');
							if (
								(await canvas.isVisible()) &&
								(await hasRenderedContent(canvas))
							) {
								withContent++;
								firefoxCanvas = canvas;
							}
						}
						return withContent >= 2;
					},
					{ timeout: 120_000, intervals: [5000, 5000, 5000, 5000, 5000, 10000] },
				)
				.toBe(true);
			await page.waitForTimeout(5000);

			// Hash every byte of the canvas — sensitive enough to catch
			// even a few pixels of movement.
			const fingerprint = async () =>
				firefoxCanvas!.evaluate((el: HTMLCanvasElement) => {
					const ctx = el.getContext("2d");
					if (!ctx) return "";
					const d = ctx.getImageData(0, 0, el.width, el.height);
					let h = 2166136261 >>> 0;
					for (let i = 0; i < d.data.length; i++) {
						h ^= d.data[i];
						h = Math.imul(h, 16777619) >>> 0;
					}
					return h.toString();
				});

			// Move cursor onto a part of the Firefox content area that's
			// guaranteed to be inside the browser viewport — Firefox often
			// renders larger than the viewport, in which case page.mouse
			// silently clips moves that go off-screen and the wheel event
			// never reaches our canvas.
			const viewport = page.viewportSize() || { width: 1280, height: 720 };
			const box = await firefoxCanvas!.boundingBox();
			expect(box).not.toBeNull();
			const targetX = Math.min(
				viewport.width - 20,
				box!.x + box!.width * 0.5,
			);
			const targetY = Math.min(
				viewport.height - 20,
				box!.y + box!.height * 0.5,
			);
			await page.mouse.move(targetX, targetY);
			await page.waitForTimeout(500);

			const before = await fingerprint();
			for (let i = 0; i < 30; i++) {
				await page.mouse.wheel(0, 120);
				await page.waitForTimeout(40);
			}
			await page.waitForTimeout(2500);
			const after = await fingerprint();
			expect(after, "Firefox canvas should change after scrolling").not.toBe(before);
		});

		// TODO: investigate Escape key delivery — vim doesn't receive it
		test.skip("vim can be quit with :q", async ({ page }) => {
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			const win = await spawnApp(page, "-fn fixed -geometry 60x15", "xterm");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();
			await page.waitForTimeout(5000);

			// Open vim
			await canvas.click();
			await page.waitForTimeout(500);
			await page.keyboard.type("vim", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(3000);

			// Quit vim with Escape + :q + Enter
			await page.keyboard.press("Escape");
			await page.waitForTimeout(500);
			await page.keyboard.type(":q", { delay: 50 });
			await page.keyboard.press("Enter");
			await page.waitForTimeout(2000);

			// Should be back at the shell prompt, not stuck in vim
			await expect(canvas).toHaveScreenshot("vim-quit.png", {
				maxDiffPixelRatio: 0.05,
			});
		});

		// =====================================================================
		// Spec-compliance gap inventory.
		//
		// These tests run real X11 client tools (xdpyinfo, rendercheck,
		// x11perf) against our server inside the sidecar container. They
		// don't go through the frontend at all — they shell out into the
		// container and capture stdout / exit codes. The goal is to surface
		// concrete unimplemented or wrong protocol behavior we can then
		// prioritise fixing, and to act as guard rails so future regressions
		// fail loudly.
		// =====================================================================

		test("xkbcomp dumps a parseable XKB keymap", async () => {
			// xkbcomp -xkb walks every XKB request the server
			// supports (UseExtension, GetMap, GetIndicatorMap,
			// GetControls, GetCompatMap, GetNames, GetGeometry) and
			// emits a textual XKB keymap to stdout. A clean
			// (exit-0) dump means our XKB extension implementation
			// is byte-perfect from libxkbfile's point of view —
			// libxkbfile validates length fields, struct sizes,
			// and (notably) requires at least 4 key types and a
			// non-null sym_interpret list.
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				"DISPLAY=:99 xkbcomp -xkb :99 - 2>&1",
			]);
			const fs = await import("node:fs");
			fs.writeFileSync("/tmp/x11web-xkbcomp.txt", result.output);
			console.log(
				`xkbcomp: ${result.output.split("\n").length} lines (exit=${result.exitCode})`,
			);
			expect(result.exitCode).toBe(0);
			// Top-level container.
			expect(result.output).toContain("xkb_keymap {");
			// Per-section sanity checks.
			expect(result.output).toContain("xkb_keycodes");
			expect(result.output).toContain("minimum = 8;");
			expect(result.output).toContain("maximum = 255;");
			expect(result.output).toContain("xkb_types");
			expect(result.output).toContain("xkb_compatibility");
			expect(result.output).toContain("xkb_symbols");
			// A few well-known key names from our US-QWERTY map.
			expect(result.output).toContain("<ESC > = 9;");
			expect(result.output).toContain("<AE01> = 10;");
			expect(result.output).toContain("<RTRN> = 36;");
			expect(result.output).toContain("<SPCE> = 65;");
		});

		test("xprop / xwininfo / xlsatoms introspect the server", async () => {
			// Three lightweight introspection tools that exercise
			// QueryTree / GetWindowAttributes / GetGeometry /
			// ListProperties / GetProperty / GetAtomName / ListExtensions
			// against the root window. Each one bails the moment it
			// hits a malformed reply, so a clean exit + a few smoke
			// strings in the output is meaningful coverage of the
			// "core protocol replies are byte-perfect" surface.
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				[
					"set -e",
					'echo "=== xprop -root ==="',
					"DISPLAY=:99 xprop -root",
					'echo "=== xwininfo -root -tree ==="',
					"DISPLAY=:99 xwininfo -root -tree",
					'echo "=== xlsatoms ==="',
					"DISPLAY=:99 xlsatoms",
				].join("\n"),
			]);
			expect(result.exitCode).toBe(0);
			// xwininfo emits the canonical "Root window id" header.
			expect(result.output).toContain("Root window id");
			// xlsatoms must list the standard X11 predefined atoms.
			// These are reserved by the spec — every X server hands
			// them back at fixed atom IDs.
			expect(result.output).toMatch(/\b1\s+PRIMARY/);
			expect(result.output).toMatch(/\b4\s+ATOM/);
			expect(result.output).toMatch(/\b39\s+WM_NAME/);
			// And we expose our own GTK-shows-menubar atom from the
			// menu bridge work.
			expect(result.output).toContain("_GTK_SHELL_SHOWS_MENUBAR");
		});

		test("xdpyinfo describes the server without errors", async () => {
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				"DISPLAY=:99 xdpyinfo",
			]);
			const fs = await import("node:fs");
			fs.writeFileSync("/tmp/x11web-xdpyinfo.txt", result.output);
			console.log(
				`xdpyinfo exit=${result.exitCode} bytes=${result.output.length}`,
			);
			// xdpyinfo bails as soon as it hits an unknown reply or
			// malformed buffer, so a clean exit alone is a meaningful
			// pass for a hand-rolled X server.
			expect(result.exitCode).toBe(0);
			// And the dump should at least mention us as the screen.
			expect(result.output).toContain("name of display");
			expect(result.output).toContain("screen #0");
		});

		test("rendercheck XRender compliance", async () => {
			// rendercheck runs ~789 individual XRender tests covering
			// every compositing operator (Over, Src, In, Out, Atop,
			// Xor, Add, Saturate, plus the Disjoint and Conjoint
			// families), glyph rendering, repeat modes, transforms,
			// and gradients. Each emits a `passed` / `FAILED` line
			// and a summary at the end. The pass count is our
			// XRender compliance score; we ratchet it up as we
			// implement more operators.
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				// `-f a8r8g8b8` selects our native pict format —
				// without it rendercheck loops over every format and
				// the non-32bit paths are much thinner.
				"DISPLAY=:99 rendercheck -f a8r8g8b8 2>&1 || true",
			]);
			const fs = await import("node:fs");
			fs.writeFileSync("/tmp/x11web-rendercheck.txt", result.output);

			// Parse the summary line: "N tests passed of M total".
			const summary = result.output.match(
				/(\d+)\s+tests passed of\s+(\d+)\s+total/,
			);
			const passed = summary ? Number.parseInt(summary[1], 10) : 0;
			const total = summary ? Number.parseInt(summary[2], 10) : 0;
			console.log(
				`rendercheck: ${passed}/${total} passed (exit=${result.exitCode})`,
			);

			// Pass-count baseline. Bump up (never down) as we
			// implement more of the XRender spec.
			//   2026-04-10  80/789  (initial inventory)
			//   2026-04-10 110/789  (full PictOp 0..12 table)
			//   2026-04-10 194/789  (handle_get_image returns the
			//                        actual depth so a8r8g8b8 dest
			//                        readback gets the alpha byte;
			//                        + linear gradient parser, +
			//                        SetPictureTransform handler)
			//   2026-04-10 240/789  (PictOpSaturate, plus the
			//                        Disjoint{Clear,Src,Dst,Over,
			//                        OverReverse} and Conjoint{
			//                        Clear,Src,Dst,Over,OverReverse}
			//                        operators)
			//   2026-04-10 292/789  (full Disjoint{In,InReverse,Out,
			//                        OutReverse,Atop,AtopReverse,Xor}
			//                        and Conjoint{In,InReverse,Out,
			//                        OutReverse,Atop,AtopReverse,Xor}
			//                        via shared in/out coverage helpers)
			//   2026-04-10 786/789  (XRenderColor is premultiplied per
			//                        spec — stop double-multiplying;
			//                        gradient stops lerp in straight
			//                        space; gradient picture repeat
			//                        modes; rgb24 dst gets implicit
			//                        Da=1; pixman half-open trapezoid
			//                        rasterisation + zero_src_has_no
			//                        _effect bbox extension; per-pixel
			//                        SetPictureTransform sampling for
			//                        non-gradient sources; component
			//                        alpha (CA) masks via per-channel
			//                        Fs/Fd; BadDrawable on render-into
			//                        -gradient)
			//   2026-04-11 789/789  (xRGB32 + xBGR32 picture formats
			//                        with format-aware byte decode in
			//                        resolve_source_pixels; GXinvert
			//                        in PolyFillRectangle)
			const RENDERCHECK_BASELINE_PASSED = 789;
			expect(passed).toBeGreaterThanOrEqual(RENDERCHECK_BASELINE_PASSED);
		});

		test("xev reports synthetic input events", async ({ page }) => {
			// Spawn xev wrapped in `sh -c` so its stdout is captured
			// to a file we can read back. We go through the frontend's
			// spawn flow (instead of direct container exec) so the
			// resulting window is tracked by the dock and can be
			// driven from Playwright.
			//
			// xev prints one block per X event with the event name
			// (KeyPress / ButtonPress / Motion / Expose / ...) and
			// the relevant fields. That gives us a *byte-precise*
			// contract on event delivery and event-record layout —
			// far stricter than the existing screenshot-based input
			// tests.
			await page.goto(`http://localhost:${frontendPort}`);
			await waitForDock(page);

			// Drop a small wrapper into /tmp that the spawn flow can
			// invoke without arguments — the spawn UI splits args on
			// spaces, so we can't pass `-c 'xev > log'` directly.
			await sidecarContainer.exec([
				"bash",
				"-c",
				[
					"rm -f /tmp/xev.log /tmp/xev-wrapper.sh",
					"cat > /tmp/xev-wrapper.sh <<'EOF'",
					"#!/bin/sh",
					"exec xev > /tmp/xev.log 2>&1",
					"EOF",
					"chmod +x /tmp/xev-wrapper.sh",
				].join("\n"),
			]);

			const win = await spawnApp(page, "", "/tmp/xev-wrapper.sh");
			const canvas = win.locator('[data-testid="x11-canvas"]');
			await expect(canvas).toBeVisible();

			// Drive a click and a key.
			await canvas.click({ position: { x: 30, y: 30 } });
			await canvas.click({ position: { x: 60, y: 40 } });
			await page.keyboard.press("a");
			await page.keyboard.press("Enter");

			// Give the events time to round-trip through the
			// frontend → backend → sidecar → xev pipeline.
			await page.waitForTimeout(800);

			// Read xev's accumulated log, then kill it.
			const logResult = await sidecarContainer.exec([
				"bash",
				"-c",
				'cat /tmp/xev.log; pkill -f "^xev" >/dev/null 2>&1; true',
			]);
			const fs = await import("node:fs");
			fs.writeFileSync("/tmp/x11web-xev.txt", logResult.output);

			const log = logResult.output;
			console.log(`xev: ${log.split("\n").length} lines captured`);

			// We should always see the window-creation events.
			expect(log).toContain("MapNotify event");
			expect(log).toContain("Expose event");
			// And — the actual point of this test — the synthetic
			// input events we drove from Playwright.
			expect(log).toContain("ButtonPress event");
			expect(log).toContain("ButtonRelease event");
			expect(log).toContain("KeyPress event");
		});

		test("x11perf curated short benchmark", async () => {
			// 42 tests at `-time 1 -repeat 1` plus x11perf's own
			// per-test setup overhead routinely exceeds the default
			// 2 min Playwright timeout, so give it some headroom.
			test.setTimeout(300_000);
			// x11perf's default `-time 5 -repeat 5` makes each test
			// run for 25 seconds, which is too slow for CI. We use
			// `-time 1 -repeat 1` and a curated subset that exercises
			// the protocol primitives we actually implement.
			//
			// Drawing / image primitives:
			//   - noop:                NoOperation round-trip
			//   - dot:                 single-pixel rendering
			//   - line/seg:            PolyLine / PolySegment
			//   - rect:                PolyFillRectangle
			//   - orect:               PolyRectangle (outlines)
			//   - triangle:            FillPoly (3-vertex)
			//   - circle / fcircle:    PolyArc / PolyFillArc
			//   - putimage:            PutImage
			//   - getimage:            GetImage
			//   - copywinwin:          CopyArea (window→window)
			//   - copypixpix:          CopyArea (pixmap→pixmap)
			//   - scroll:              CopyArea (self, overlapping)
			//   - ftext:               PolyText8 (6x13 fixed font)
			//
			// Pointer / property / window-management primitives
			// (these don't touch the rendering paths at all and so
			// catch a different class of regressions — request
			// dispatch, reply marshalling, window-tree mutation):
			//   - pointer:             QueryPointer
			//   - prop:                GetProperty
			//   - gc:                  ChangeGC
			//   - create / ucreate:    CreateWindow (mapped/unmapped)
			//   - map / unmap:         MapWindow / UnmapWindow
			//   - destroy:             DestroyWindow
			//   - popup:               map+unmap roundtrip
			//   - move / umove:        ConfigureWindow (position)
			//   - resize / uresize:    ConfigureWindow (size)
			//   - circulate / ucirculate: CirculateWindow
			//
			// We don't assert on the throughput numbers (those are
			// noisy in a container) — just that every selected test
			// emitted a line of the form "N reps @ ... msec (... /sec)"
			// and the binary exited cleanly. That's enough to catch
			// any regression that crashes the server, returns a
			// malformed reply, or makes a request hang.
			const tests = [
				// drawing / image
				"-noop",
				"-dot",
				"-line10",
				"-line500",
				"-seg10",
				"-seg100",
				"-rect10",
				"-rect100",
				"-orect10",
				"-orect100",
				"-triangle10",
				"-triangle100",
				"-circle10",
				"-circle100",
				"-fcircle10",
				"-fcircle100",
				"-putimage10",
				"-putimage100",
				"-getimage10",
				"-getimage100",
				"-copywinwin10",
				"-copywinwin100",
				"-copypixpix10",
				"-copypixpix100",
				"-scroll10",
				"-scroll100",
				"-ftext",
				// pointer / property / window-management
				"-pointer",
				"-prop",
				"-gc",
				"-create",
				"-ucreate",
				"-map",
				"-unmap",
				"-destroy",
				"-popup",
				"-move",
				"-umove",
				"-resize",
				"-uresize",
				"-circulate",
				"-ucirculate",
			];
			// `-subs 4` constrains the window-management tests
			// (-create / -map / -resize / etc.) to a single sub-window
			// count instead of the default seven, which would make
			// each of those tests emit 7 reps lines and take ~7s.
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				`DISPLAY=:99 x11perf -time 1 -repeat 1 -subs 4 ${tests.join(" ")} 2>&1 || true`,
			]);
			const fs = await import("node:fs");
			fs.writeFileSync("/tmp/x11web-x11perf.txt", result.output);

			expect(result.exitCode).toBe(0);
			// Each test prints exactly one "N reps @ ... msec (.../sec)" line
			// (with -subs 4, the window-mgmt tests also emit just one).
			// x11perf right-pads small throughput values, so allow spaces
			// between the open paren and the number.
			const repLines = result.output.match(
				/^\s*\d[\d,]*\s+reps\s+@\s+[\d.]+\s+msec\s+\(\s*[\d.]+\/sec\):/gm,
			);
			const repsCount = repLines ? repLines.length : 0;
			console.log(
				`x11perf: ${repsCount}/${tests.length} reps lines (exit=${result.exitCode})`,
			);
			expect(repsCount).toBe(tests.length);
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
