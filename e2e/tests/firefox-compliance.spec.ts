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

test.skip("DIAG: GIMP click receives input", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/sidecar.log"]);
	// Spawn GIMP interactively (no batch) so the UI window appears.
	await sidecarContainer.exec(["bash", "-c", "export DISPLAY=:99; gimp --no-data --no-fonts > /tmp/gimp.log 2>&1 &"]);
	await page.waitForTimeout(20000);

	const canvases = page.locator('[data-testid="x11-canvas"]');
	const count = await canvases.count();
	console.log(`Canvases visible: ${count}`);
	if (count === 0) { console.log("no canvas, abort"); return; }
	const canvas = canvases.nth(count - 1);
	await canvas.screenshot({ path: "test-results/diag-gimp-before.png" });

	const box = await canvas.boundingBox();
	if (!box) { console.log("no box"); return; }
	const cx = Math.round(box.x + box.width * 0.5);
	const cy = Math.round(box.y + box.height * 0.5);
	console.log(`Click GIMP at (${cx}, ${cy})`);
	await page.mouse.click(cx, cy);
	await page.waitForTimeout(500);
	await page.keyboard.type("hello", { delay: 50 });
	await page.waitForTimeout(800);
	await canvas.screenshot({ path: "test-results/diag-gimp-after.png" });

	const log = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH' /tmp/sidecar.log | tail -8"]);
	console.log("---DISPATCH---\n" + log.output);
});

test.skip("DIAG: synthetic firefox-like client receives events", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	// Drop a python script that creates a parent window 921x691 with
	// Firefox-like event_mask, plus a child window of the same size.
	// Both select for events and the child logs received events.
	const pyScript = [
		"import os, time, sys",
		"import Xlib.display, Xlib.X",
		"d = Xlib.display.Display()",
		"screen = d.screen()",
		"# Parent: KeyPress|KeyRelease|ButtonPress|ButtonRelease|EnterWindow|LeaveWindow|PointerMotion|KeymapState|Exposure|StructureNotify|FocusChange|PropertyChange",
		"PARENT_MASK = 0x63a07f",
		"# Child: ButtonPress|ButtonRelease|Enter|Leave|Motion|Exposure|VisibilityChange|StructureNotify|PropertyChange",
		"CHILD_MASK = 0x43807c",
		"parent = screen.root.create_window(0, 0, 921, 691, 0, screen.root_depth, event_mask=PARENT_MASK)",
		"child = parent.create_window(0, 0, 921, 691, 0, screen.root_depth, event_mask=CHILD_MASK)",
		"parent.set_wm_class('FFTest', 'FFTest')",
		"parent.set_wm_name('Synthetic FFTest')",
		"parent.map()",
		"child.map()",
		"d.sync()",
		"sys.stderr.write(f'PARENT={hex(parent.id)} CHILD={hex(child.id)}\\n')",
		"sys.stderr.flush()",
		"start = time.time()",
		"while time.time() - start < 30:",
		"    while d.pending_events() > 0:",
		"        ev = d.next_event()",
		"        sys.stderr.write(f'EV {ev.type} {ev}\\n')",
		"        sys.stderr.flush()",
		"    time.sleep(0.05)",
	].join("\n");
	const b64 = Buffer.from(pyScript).toString("base64");
	await sidecarContainer.exec(["bash", "-c", `printf '%s' '${b64}' | base64 -d > /tmp/fftest.py`]);
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/fftest.log; export DISPLAY=:99; (python3 /tmp/fftest.py > /tmp/fftest.out 2> /tmp/fftest.log &) ; sleep 2"]);
	await page.waitForTimeout(3000);

	// Now the synthetic window should be on the canvas — find its locator.
	const canvases = page.locator('[data-testid="x11-canvas"]');
	const count = await canvases.count();
	console.log(`Canvases visible: ${count}`);
	const lastCanvas = canvases.nth(count - 1);
	const box = await lastCanvas.boundingBox();
	console.log(`Last canvas box: ${JSON.stringify(box)}`);
	if (!box) {
		console.log("No box - aborting");
		return;
	}
	const cx = Math.round(box.x + box.width * 0.5);
	const cy = Math.round(box.y + box.height * 0.5);
	console.log(`Clicking at viewport (${cx}, ${cy})`);
	await page.mouse.click(cx, cy);
	await page.waitForTimeout(300);
	await page.keyboard.press("a");
	await page.waitForTimeout(300);

	const log = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH' /tmp/sidecar.log 2>/dev/null | tail -10"]);
	console.log("---DISPATCH---\n" + log.output);
	const fft = await sidecarContainer.exec(["bash", "-c", "head -30 /tmp/fftest.log"]);
	console.log("---FFTEST events received---\n" + fft.output);
});

// Diagnostic — attach xev to Firefox's content child by XID, then click
// the canvas. xev runs on a separate connection so it receives the same
// X11 stream a Firefox-internal listener would.
// Diagnostic — capture the actual bytes our X server writes to Firefox's
// X11 socket on canvas click + key press, so we can compare with what an
// X11 client expects.
test.skip("DIAG: Firefox click delivery target", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/sidecar.log"]);

	// Find Firefox's "Mozilla Firefox" (Navigator) window id and its child.
	const findIds = await sidecarContainer.exec([
		"bash",
		"-c",
		[
			"export DISPLAY=:99",
			"TOP=$(xwininfo -root -tree | grep 'Mozilla Firefox.*Navigator' | grep -oE '0x[0-9a-f]+' | head -1)",
			'if [ -z "$TOP" ]; then echo NOT_FOUND; exit 1; fi',
			'echo "TOP=$TOP"',
			'CHILD=$(xwininfo -id "$TOP" -tree | grep -oE "0x[0-9a-f]+" | grep -v "^$TOP\\$" | head -1)',
			'echo "CHILD=$CHILD"',
		].join("\n"),
	]);
	console.log("---FIREFOX IDS---\n" + findIds.output);
	const m = findIds.output.match(/TOP=(0x[0-9a-f]+)\s+CHILD=(0x[0-9a-f]+)/);
	if (!m) { console.log("could not parse"); return; }
	const topId = m[1];
	const childId = m[2];
	console.log(`Top=${topId} Child=${childId}`);

	// Attach xev to the child so xev sees the same events Firefox sees.
	await sidecarContainer.exec([
		"bash",
		"-c",
		`true > /tmp/xev_top.log; true > /tmp/xev_child.log
		export DISPLAY=:99
		xev -id ${topId} > /tmp/xev_top.log 2>&1 &
		xev -id ${childId} > /tmp/xev_child.log 2>&1 &
		`,
	]);
	await page.waitForTimeout(1500);

	const box = await canvas.boundingBox();
	const cx = Math.round(box!.x + box!.width * 0.5);
	const cy = Math.round(box!.y + box!.height * 0.5);
	console.log(`Clicking at viewport (${cx}, ${cy})`);
	await page.mouse.click(cx, cy);
	await page.waitForTimeout(500);
	// Send Ctrl+L which Firefox interprets as "focus URL bar".
	await page.keyboard.down("Control");
	await page.waitForTimeout(100);
	await page.keyboard.press("l");
	await page.waitForTimeout(100);
	await page.keyboard.up("Control");
	await page.waitForTimeout(500);
	// Type a few chars to verify URL bar is focused (would appear in URL).
	await page.keyboard.type("xyz", { delay: 50 });
	await page.waitForTimeout(500);
	await canvas.screenshot({ path: "test-results/diag-after-ctrl-l-type.png" });

	// Sidecar dispatch log + byte writes (full)
	const log = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH|WRITE_INPUT' /tmp/sidecar.log"]);
	console.log("---DISPATCH/WRITE---\n" + log.output);

	// Map peer pid → process name to see which app got the bytes
	const procs = await sidecarContainer.exec(["bash", "-c", "ps axo pid,comm | grep -iE 'firefox|xterm|xev' | head"]);
	console.log("---PROCS---\n" + procs.output);

	// Full window tree to see what overlays / popups are active
	const tree = await sidecarContainer.exec(["bash", "-c", "export DISPLAY=:99; xwininfo -root -tree 2>&1 | head -80"]);
	console.log("---WINDOW TREE---\n" + tree.output);
});

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
// Firefox-side input does NOT actually work. The pixel-hash assertion is
// a false-positive farm — cursor blink and minor UI animation make
// hashAfter !== hashBefore even when the URL bar never receives focus.
// See test-results/ff-{before,after}-navigate.png from a recent run:
// the URL bar still shows the "Search or enter address" placeholder
// and the active tab is still "New Tab", proving input never reached
// Firefox. Tracked as a separate workstream.
test.skip("firefox: navigate to about:config", async ({
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
test.skip("firefox: navigate to Wikipedia", async ({
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
test.skip("firefox: scroll works on loaded page", async ({
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
test.skip("firefox: navigate to YouTube", async ({
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
test.skip("firefox: local HTML5 video playback", async ({
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
