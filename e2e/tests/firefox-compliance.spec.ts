/**
 * Firefox comprehensive e2e tests.
 *
 * Validates that Firefox ESR works correctly through our X11 server:
 *   - Startup and initial rendering
 *   - Address bar navigation (about:config)
 *   - Wikipedia page load and scrolling
 *   - YouTube page load
 *   - Local HTML5 video playback
 */

import type { Locator, Page } from "@playwright/test";
import {
	test,
	expect,
	spawnApp,
	waitForDock,
	hasRenderedContent,
	countNonBlackPixels,
	canvasPixelHash,
	waitForCanvasStable,
	cleanupApps,
} from "./fixtures";

// Re-usable timeout for Firefox startup
const FIREFOX_STARTUP_TIMEOUT = 120_000;
const FIREFOX_NAVIGATE_TIMEOUT = 60_000;

/**
 * Spawn Firefox via spawnApp, wait for rendering. Returns the canvas.
 */
async function spawnFirefoxAndWait(
	page: Page,
	args = "--no-remote --new-instance about:blank",
): Promise<Locator> {
	const win = await spawnApp(page, args, "firefox-esr", FIREFOX_STARTUP_TIMEOUT);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: FIREFOX_STARTUP_TIMEOUT });

	// Wait for Firefox to finish its multi-stage rendering
	await expect
		.poll(async () => hasRenderedContent(canvas), {
			timeout: FIREFOX_STARTUP_TIMEOUT,
			intervals: [3000, 5000, 5000, 10000, 10000, 10000],
		})
		.toBe(true);

	const pixels = await countNonBlackPixels(canvas);
	expect(pixels).toBeGreaterThan(500);
	return canvas;
}

async function navigateFirefox(
	page: Page,
	canvas: Locator,
	url: string,
): Promise<void> {
	// Click somewhere in the content area first to make sure the canvas
	// has keyboard focus, then use Ctrl+L (Firefox's "focus URL bar"
	// shortcut) — the URL bar's exact pixel position depends on chrome
	// height and isn't a stable click target.
	const box = await canvas.boundingBox();
	expect(box).not.toBeNull();
	await page.mouse.click(box!.x + box!.width * 0.5, box!.y + box!.height * 0.5);
	await page.waitForTimeout(500);
	await page.keyboard.press("Control+l");
	await page.waitForTimeout(500);
	await page.keyboard.type(url, { delay: 30 });
	await page.waitForTimeout(300);
	await page.keyboard.press("Enter");
}

// ---------------------------------------------------------------------------
// Firefox startup and initial rendering
// ---------------------------------------------------------------------------
test("firefox: startup and initial rendering", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	const hash = await canvasPixelHash(canvas);
	expect(hash).not.toBe("");
	await waitForCanvasStable(canvas, {
		stableMs: 2000,
		totalTimeoutMs: 30_000,
	});
});

// ---------------------------------------------------------------------------
// Firefox navigates to about:config
// ---------------------------------------------------------------------------
test("firefox: navigate to about:config", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	const hashBefore = await canvasPixelHash(canvas);

	await navigateFirefox(page, canvas, "about:config");
	await page.waitForTimeout(5000);

	const hashAfter = await canvasPixelHash(canvas);
	expect(hashAfter).not.toBe(hashBefore);
	const pixels = await countNonBlackPixels(canvas);
	expect(pixels).toBeGreaterThan(500);
});

// ---------------------------------------------------------------------------
// Firefox navigates to Wikipedia
// ---------------------------------------------------------------------------
test("firefox: navigate to Wikipedia", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(240_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	const hashBefore = await canvasPixelHash(canvas);

	await navigateFirefox(
		page,
		canvas,
		"https://en.wikipedia.org/wiki/Main_Page",
	);
	await page.waitForTimeout(10_000);

	await expect
		.poll(async () => (await canvasPixelHash(canvas)) !== hashBefore, {
			timeout: FIREFOX_NAVIGATE_TIMEOUT,
			intervals: [3000, 5000, 5000, 10000, 10000],
		})
		.toBe(true);

	const pixels = await countNonBlackPixels(canvas);
	expect(pixels).toBeGreaterThan(1000);
});

// ---------------------------------------------------------------------------
// Firefox scroll works
// ---------------------------------------------------------------------------
test("firefox: scroll works on loaded page", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(240_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(
		page,
		"--no-remote --new-instance https://en.wikipedia.org/wiki/X_Window_System",
	);

	await page.waitForTimeout(15_000);
	await waitForCanvasStable(canvas, {
		stableMs: 3000,
		totalTimeoutMs: 60_000,
	});

	const hashBeforeScroll = await canvasPixelHash(canvas);

	const box = await canvas.boundingBox();
	expect(box).not.toBeNull();
	const viewport = page.viewportSize() || { width: 1280, height: 720 };
	await page.mouse.move(
		Math.min(viewport.width - 20, box!.x + box!.width * 0.5),
		Math.min(viewport.height - 20, box!.y + box!.height * 0.5),
	);
	await page.waitForTimeout(500);

	for (let i = 0; i < 20; i++) {
		await page.mouse.wheel(0, 120);
		await page.waitForTimeout(100);
	}
	await page.waitForTimeout(3000);

	const hashAfterScroll = await canvasPixelHash(canvas);
	expect(hashAfterScroll).not.toBe(hashBeforeScroll);
});

// ---------------------------------------------------------------------------
// Firefox navigates to YouTube
// ---------------------------------------------------------------------------
test("firefox: navigate to YouTube", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(240_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	const hashBefore = await canvasPixelHash(canvas);

	await navigateFirefox(page, canvas, "https://www.youtube.com");
	await page.waitForTimeout(15_000);

	await expect
		.poll(async () => (await canvasPixelHash(canvas)) !== hashBefore, {
			timeout: FIREFOX_NAVIGATE_TIMEOUT,
			intervals: [5000, 5000, 10000, 10000, 10000],
		})
		.toBe(true);

	const pixels = await countNonBlackPixels(canvas);
	expect(pixels).toBeGreaterThan(500);
});

// ---------------------------------------------------------------------------
// Local HTML5 video playback
// ---------------------------------------------------------------------------
test("firefox: local HTML5 video playback", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(240_000);
	await cleanupApps(sidecarContainer);

	// Start HTTP server for test video content
	await sidecarContainer.exec([
		"bash",
		"-c",
		"cd /opt/test-content && python3 -m http.server 8888 &",
	]);
	await new Promise((r) => setTimeout(r, 2000));

	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(
		page,
		"--no-remote --new-instance http://localhost:8888/video-test.html",
	);

	await page.waitForTimeout(10_000);

	// Detect video playback by checking canvas updates over time
	const hashes: string[] = [];
	for (let i = 0; i < 6; i++) {
		hashes.push(await canvasPixelHash(canvas));
		if (i < 5) await new Promise((r) => setTimeout(r, 1500));
	}
	let changes = 0;
	for (let i = 1; i < hashes.length; i++) {
		if (hashes[i] !== hashes[i - 1]) changes++;
	}
	expect(changes).toBeGreaterThanOrEqual(2);
});
