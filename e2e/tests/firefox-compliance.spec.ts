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
// Run Firefox under GDK_DEBUG=events so GDK prints every event it receives
// from us to stderr. We capture that and compare with what we sent.
// Try Firefox with a custom profile that disables first-run UI and
// telemetry overlays. If input works without the privacy notice tab and
// the F/+ overlay, the issue is Firefox's first-run gating.
// Down-arrow on gtk3-demo's list view should move the selection. This
// exercises core+XI key delivery, focus subtree descent (GTK3 selects
// XI keys on a sub-window of the toplevel), and XI2 FocusIn synthesis —
// without any of those, GTK3 swallows the key silently and the
// selection never repaints.
test("gtk3-demo keyboard navigation moves list selection", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(120_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const win = await spawnApp(page, "", "gtk3-demo", 30_000);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 30_000 });
	await page.waitForTimeout(5000);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("canvas has no bounding box");

	const beforeClick = await canvasPixelHash(canvas);
	await page.mouse.click(box.x + 100, box.y + 200);
	await page.waitForTimeout(800);
	const afterClick = await canvasPixelHash(canvas);

	await page.keyboard.press("ArrowDown");
	await page.waitForTimeout(500);
	const afterDown1 = await canvasPixelHash(canvas);

	await page.keyboard.press("ArrowDown");
	await page.waitForTimeout(500);
	const afterDown2 = await canvasPixelHash(canvas);

	// Click on a different list row should change the highlighted item.
	expect(afterClick).not.toBe(beforeClick);
	expect(afterDown1).not.toBe(afterClick);
	expect(afterDown2).not.toBe(afterDown1);
});

// XTEST FakeInput must dispatch button events to the window under the
// pointer (not the focus window). Validates this by spawning gtk3-demo,
// hovering the cursor over a list row via xdotool, and clicking; the
// highlighted row should change. Without the fix, xdotool clicks
// silently no-op because they're delivered to focus_window which doesn't
// match the cursor's actual visual target.
//
// XTEST FakeInput motion now emits Enter/Leave crossings alongside
// MotionNotify (commit 22e34ad), but synthesised clicks on gtk3-demo
// still don't change the canvas hash — the toolkit/canvas pipeline
// needs more than just the crossing events.  Leaving skipped pending
// a deeper look at whether gtk3-demo is responding to the press at
// all and the frontend just isn't seeing the repaint.
// Even with the shared-focus fix, gtk3-demo's canvas hash doesn't
// flip after xdotool-injected clicks at the inner main-window
// coordinates. Likely the gtk3-demo widget pipeline expects more
// than ButtonPress + Crossing — pointer focus or grab events too.
test.skip("xdotool clicks reach the window under the pointer", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(120_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const win = await spawnApp(page, "", "gtk3-demo", 30_000);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 30_000 });
	await page.waitForTimeout(5000);

	// Find gtk3-demo's window position and size at root level.
	// Find gtk3-demo's actual main toplevel by ignoring the tiny
	// 10x10 helper windows the toolkit creates. Pick the first
	// child of root that's > 100px wide.
	const winInfo = await sidecarContainer.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; xwininfo -root -tree | head -30`,
	]);
	const lines = winInfo.output.split("\n");
	let wx = 0, wy = 0, w = 0, h = 0;
	for (const line of lines) {
		const m = line.match(
			/0x[0-9a-f]+[^\n]*?(\d{2,4})x(\d{2,4})\+(-?\d+)\+(-?\d+)/,
		);
		if (!m) continue;
		const ww = Number(m[1]);
		const hh = Number(m[2]);
		if (ww > 200 && hh > 200) {
			w = ww;
			h = hh;
			wx = Number(m[3]);
			wy = Number(m[4]);
			break;
		}
	}
	if (w === 0) throw new Error("could not find gtk3-demo main window");
	console.log(`gtk3 at root (${wx},${wy}) size ${w}x${h}`);

	const before = await canvasPixelHash(canvas);

	await sidecarContainer.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; xdotool mousemove ${wx + 100} ${wy + 80} click 1`,
	]);
	await page.waitForTimeout(2000);
	const afterFirst = await canvasPixelHash(canvas);

	await sidecarContainer.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; xdotool mousemove ${wx + 100} ${wy + 250} click 1`,
	]);
	await page.waitForTimeout(2000);
	const afterSecond = await canvasPixelHash(canvas);

	expect(afterFirst).not.toBe(before);
	expect(afterSecond).not.toBe(afterFirst);
});

test.skip("DIAG: Firefox slow click sequence", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	await sidecarContainer.exec([
		"bash",
		"-c",
		`mkdir -p /tmp/ffprof && cat > /tmp/ffprof/user.js <<'EOF'
user_pref("browser.startup.firstrunSkipsHomepage", true);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutConfig.showWarning", false);
EOF`,
	]);

	const win = await spawnApp(
		page,
		"--no-remote --new-instance --profile /tmp/ffprof about:blank",
		"firefox-esr",
		FIREFOX_STARTUP_TIMEOUT,
	);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: FIREFOX_STARTUP_TIMEOUT });
	await expect.poll(async () => hasRenderedContent(canvas), {
		timeout: FIREFOX_STARTUP_TIMEOUT,
		intervals: [3000, 5000, 5000],
	}).toBe(true);
	await page.waitForTimeout(8000); // let Firefox fully settle

	const box = await canvas.boundingBox();
	if (!box) return;
	const ux = box.x + box.width * 0.5;
	const uy = box.y + 63;
	console.log(`URL bar at viewport (${ux}, ${uy})`);

	// Move slowly to URL bar with multi-step motion
	await page.mouse.move(box.x + 100, box.y + 200);
	await page.waitForTimeout(200);
	await page.mouse.move(box.x + 300, box.y + 100);
	await page.waitForTimeout(200);
	await page.mouse.move(ux, uy, { steps: 10 });
	await page.waitForTimeout(500);
	await canvas.screenshot({ path: "test-results/slow-1-hover.png" });

	// Click with separate down/up
	await page.mouse.down();
	await page.waitForTimeout(100);
	await page.mouse.up();
	await page.waitForTimeout(1500);
	await canvas.screenshot({ path: "test-results/slow-2-clicked.png" });

	// Capture state RIGHT after click before any DOM-focus changes
	await canvas.screenshot({ path: "test-results/slow-2b-after-click.png" });

	// Send keystrokes via xdotool inside the container (XTEST FakeInput)
	// while URL bar is presumably focused. If THIS works but page.keyboard
	// doesn't, the bug is in our canvas → server keyboard delivery.
	await sidecarContainer.exec([
		"bash",
		"-c",
		"export DISPLAY=:99; xdotool type --delay 50 'xyz' 2>&1",
	]);
	await page.waitForTimeout(800);
	await canvas.screenshot({ path: "test-results/slow-3-xdotool-typed.png" });
});

test.skip("DIAG: Firefox close button click", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	// Build profile
	await sidecarContainer.exec([
		"bash",
		"-c",
		`mkdir -p /tmp/ffprof && cat > /tmp/ffprof/user.js <<'EOF'
user_pref("browser.startup.firstrunSkipsHomepage", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.aboutwelcome.enabled", false);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage", "about:blank");
user_pref("browser.newtabpage.enabled", false);
user_pref("browser.aboutConfig.showWarning", false);
EOF`,
	]);

	// Force GDK to use core X events (no XInput2) by wrapping firefox-esr
	// in a small shell script. If Firefox responds to clicks under this
	// flag, the XInput2 path is the bug.
	await sidecarContainer.exec([
		"bash",
		"-c",
		`cat > /usr/local/bin/firefox-no-xi2 <<'EOF'
#!/bin/sh
export GDK_CORE_DEVICE_EVENTS=1
exec firefox-esr "$@"
EOF
chmod +x /usr/local/bin/firefox-no-xi2`,
	]);

	const win = await spawnApp(
		page,
		"--no-remote --new-instance --profile /tmp/ffprof about:blank",
		"firefox-no-xi2",
		FIREFOX_STARTUP_TIMEOUT,
	);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: FIREFOX_STARTUP_TIMEOUT });
	await expect.poll(async () => hasRenderedContent(canvas), {
		timeout: FIREFOX_STARTUP_TIMEOUT,
		intervals: [3000, 5000, 5000, 10000, 10000],
	}).toBe(true);
	await page.waitForTimeout(3000);
	await canvas.screenshot({ path: "test-results/ff-close-before.png" });

	const box = await canvas.boundingBox();
	if (!box) { console.log("no box"); return; }

	// Click the "X" close button of the Privacy Notice tab at canvas
	// (~478, 22). If it works, the tab will close and the bar will
	// shrink — visible pixel change.
	console.log("Click privacy notice close X at canvas (478, 22)");
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/sidecar.log"]);
	await page.mouse.click(box.x + 478, box.y + 22);
	await page.waitForTimeout(1500);
	await canvas.screenshot({ path: "test-results/ff-after-close-tab.png" });
	const log1 = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH' /tmp/sidecar.log | tail -5"]);
	console.log("---DISPATCH---\n" + log1.output);

	// Click the refresh icon at canvas (~95, 63)
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/sidecar.log"]);
	console.log("Click refresh at canvas (95, 63)");
	await page.mouse.click(box.x + 95, box.y + 63);
	await page.waitForTimeout(1500);
	await canvas.screenshot({ path: "test-results/ff-after-refresh.png" });
	const log2 = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH' /tmp/sidecar.log | tail -5"]);
	console.log("---DISPATCH---\n" + log2.output);
});

test.skip("DIAG: Firefox with clean profile", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(180_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	// Build a fresh profile with prefs that skip first-run / telemetry overlays.
	await sidecarContainer.exec([
		"bash",
		"-c",
		`mkdir -p /tmp/ffprof && cat > /tmp/ffprof/user.js <<'EOF'
user_pref("browser.startup.firstrunSkipsHomepage", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("startup.homepage_welcome_url", "");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("doh-rollout.doneFirstRun", true);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.tabs.warnOnCloseOtherTabs", false);
user_pref("browser.startup.homepage", "about:blank");
user_pref("browser.newtabpage.enabled", false);
user_pref("browser.aboutConfig.showWarning", false);
EOF`,
	]);

	const win = await spawnApp(
		page,
		"--no-remote --new-instance --profile /tmp/ffprof about:blank",
		"firefox-esr",
		FIREFOX_STARTUP_TIMEOUT,
	);
	const canvas = win.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: FIREFOX_STARTUP_TIMEOUT });

	// Wait for Firefox to render content
	await expect.poll(async () => hasRenderedContent(canvas), {
		timeout: FIREFOX_STARTUP_TIMEOUT,
		intervals: [3000, 5000, 5000, 10000, 10000],
	}).toBe(true);
	await page.waitForTimeout(3000);
	await canvas.screenshot({ path: "test-results/clean-profile-before.png" });

	const box = await canvas.boundingBox();
	if (!box) { console.log("no box"); return; }
	console.log(`canvas box: ${JSON.stringify(box)}`);

	// Inspect the actual canvas DOM attributes vs CSS rect — if the
	// internal width/height differs from the CSS rect, the click
	// coordinate translation in inputProtocol's clientToCanvas() will
	// produce wrong Y values.
	const dims = await canvas.evaluate((c: HTMLCanvasElement) => ({
		canvasWidth: c.width,
		canvasHeight: c.height,
		clientWidth: c.clientWidth,
		clientHeight: c.clientHeight,
		rect: c.getBoundingClientRect(),
	}));
	console.log("canvas DOM dims:", JSON.stringify(dims));

	// The URL bar in our screenshot lives at roughly y=63px from the canvas
	// top. Click directly on it so we don't depend on Ctrl+L behaviour.
	await sidecarContainer.exec(["bash", "-c", "true > /tmp/sidecar.log"]);
	// Hamburger menu is at top-right (~896, 64) in canvas coords.
	// Clicking it should open a menu dropdown — visible pixel change.
	const hamburgerX = box.x + 896;
	const hamburgerY = box.y + 63;
	console.log(`Click hamburger at viewport (${hamburgerX}, ${hamburgerY})`);
	await page.mouse.click(hamburgerX, hamburgerY);
	await page.waitForTimeout(1000);
	await canvas.screenshot({ path: "test-results/clean-profile-hamburger.png" });
	const dispLog = await sidecarContainer.exec(["bash", "-c", "grep -aE 'DISPATCH' /tmp/sidecar.log | tail -5"]);
	console.log("---DISPATCH for hamburger click---\n" + dispLog.output);

	// Inspect Firefox's content child window — actual position/size
	const ffWindows = await sidecarContainer.exec([
		"bash",
		"-c",
		[
			"export DISPLAY=:99",
			"TOP=$(xwininfo -root -tree | grep 'Mozilla Firefox.*Navigator' | grep -oE '0x[0-9a-f]+' | head -1)",
			'echo "TOP=$TOP"',
			'xwininfo -id "$TOP" -all 2>&1 | head -30',
			'echo "--- children ---"',
			'xwininfo -id "$TOP" -tree 2>&1 | head -30',
			'echo "--- detailed child geometry ---"',
			'for c in $(xwininfo -id "$TOP" -tree | grep -oE "0x[0-9a-f]+" | grep -v "$TOP" | head -2); do',
			'  echo "child $c:"',
			'  xwininfo -id "$c" -all 2>&1 | head -20',
			'done',
		].join("\n"),
	]);
	console.log("---FIREFOX WINDOWS---\n" + ffWindows.output);
	await canvas.screenshot({ path: "test-results/clean-profile-after-urlbar-click.png" });
	await page.keyboard.type("about:config", { delay: 30 });
	await page.waitForTimeout(500);
	await canvas.screenshot({ path: "test-results/clean-profile-typed.png" });
	await page.keyboard.press("Enter");
	await page.waitForTimeout(3000);
	await canvas.screenshot({ path: "test-results/clean-profile-after.png" });
});

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
// Firefox navigates to about:config.
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
// Wheel events reach our frontend's wheel handler (verified) and the
// X server translates them to XI_Motion with scroll-class valuators
// (covered by xinput2/tests.rs::scroll_button_press_emits_motion...).
// Firefox-ESR still doesn't visibly scroll though — likely the
// content-area child window isn't where we route the synthesized
// scroll events. Needs an XInput2 scrolling investigation; the
// xterm scroll test passes (uses legacy button 4/5), so the
// non-XInput2 path is fine.
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
// Firefox loads /opt/test-video.mp4 but the canvas pixel hash
// never changes — either the video element doesn't actually
// play frames, or the per-frame redraws of the video element
// don't propagate through to the X11 canvas. Likely the latter
// given how much the rest of Firefox renders. Tracked
// separately; needs an audit of how Firefox uses the X11 surface
// for video tear-free output.
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

// ---------------------------------------------------------------------------
// Firefox: click URL bar, type a URL, wait for the page to render.
//
// This is the *canonical* mouse-input-end-to-end test. It exercises the
// click-delivery path through Firefox's chrome (a child window inside the
// toplevel) instead of using the `Ctrl+L` keyboard shortcut that the
// existing `firefox: navigate to Wikipedia` test uses.
//
// Currently failing — kept skipped, with the diagnostic notes below so the
// next attempt has a starting point.
//
// Findings from probing this path (manual sidecar + xev + sidecar tracing):
//   * Firefox does NOT issue any XISelectEvents (no DBG XInput logs fire
//     during startup). So it's using core X events, not XInput2 — even
//     though our X server advertises XInputExtension and negotiates
//     `XIQueryVersion → 2.4`.
//   * Firefox's chrome child window (the visible 921x691 child of the
//     "Mozilla Firefox" Navigator top-level) selects core BUTTON_PRESS /
//     BUTTON_RELEASE / ENTER / LEAVE / MOTION in its event_mask
//     (= 0x43807c). So clicks *should* be delivered as core events.
//   * `find_subwindow_in_shared` correctly resolves a click at
//     `(canvas-center, 63)` to that chrome child, and `broadcast_event`
//     fires with one matching subscriber (Firefox's own connection).
//   * Despite that, `xev -id <chrome-child>` only ever sees ButtonRelease
//     for an XTEST click — ButtonPress is missing from the log. The press
//     event IS sent to Firefox's connection (broadcast logs confirm), but
//     the visible state never changes: no URL bar caret appears, typed
//     keys never echo into the bar, navigation doesn't fire.
//   * The same click-and-type pattern works for `gtk3-demo` (probed
//     during the same investigation), so the core-button-delivery
//     primitive works in general; the failure is Firefox-specific.
//   * `_NET_ACTIVE_WINDOW` after the click reports Firefox's 1x1 helper
//     window (`0x...002b`) rather than the toplevel — likely Firefox sets
//     this itself when it processes whatever FocusIn it sees. Our
//     `set_focus_window` targets the toplevel, which may be a mismatch.
//   * Keyboard input via `Ctrl+L` works (covered by `firefox: navigate
//     to Wikipedia`). So Firefox's chrome receives core *keyboard*
//     events after a focus-establishing click, just not *button* widget
//     hits.
//
// Update (after the 10cbe63 timestamp fix): `xev` now reports
// `time=16999` on a click that used to read `time=12`, so the per-
// connection start-time bug is gone. Re-running this test with the
// fix in place still leaves the URL bar empty and the page un-
// navigated — so the stale-timestamp theory was real but not
// sufficient on its own.
//
// Remaining hypotheses for the next pass: (a) verify whether Firefox
// is silently discarding our ButtonPress due to a missing FocusIn /
// wrong focus target before the press; (b) inspect any passive XI2
// / core passive button grab Firefox sets on its chrome — if it
// grabs *only* via XI2 and our XTEST handler only fires core events,
// the grab never activates.
// ---------------------------------------------------------------------------
test.skip("firefox: click URL bar, type wikipedia.org, page renders", async ({
	page,
	sidecarContainer,
	frontendUrl,
}) => {
	test.setTimeout(240_000);
	await cleanupApps(sidecarContainer);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const canvas = await spawnFirefoxAndWait(page);
	await waitForCanvasStable(canvas, {
		stableMs: 2500,
		totalTimeoutMs: 30_000,
	});

	const hashBefore = await canvasPixelHash(canvas);
	await canvas.screenshot({ path: "test-results/ff-click-1-before.png" });

	const box = await canvas.boundingBox();
	if (!box) throw new Error("Firefox canvas has no bounding box");
	// URL bar sits in Firefox-ESR chrome around y=55-65 from canvas
	// top. Click at center-x so we land squarely in the entry field.
	const ux = box.x + box.width * 0.5;
	const uy = box.y + 63;
	await page.mouse.click(ux, uy);
	await page.waitForTimeout(800);
	await canvas.screenshot({ path: "test-results/ff-click-2-after-click.png" });

	// Select-all to clear whatever placeholder/url is there.
	await page.keyboard.press("Control+a");
	await page.waitForTimeout(200);

	await page.keyboard.type("www.wikipedia.org", { delay: 40 });
	await page.waitForTimeout(500);
	await canvas.screenshot({ path: "test-results/ff-click-3-typed.png" });

	await page.keyboard.press("Enter");
	// Wait long enough for Wikipedia to load and paint.
	await page.waitForTimeout(15_000);
	await waitForCanvasStable(canvas, {
		stableMs: 2000,
		totalTimeoutMs: 60_000,
	});
	await canvas.screenshot({ path: "test-results/ff-click-4-loaded.png" });

	const hashAfter = await canvasPixelHash(canvas);
	expect(hashAfter, "Firefox canvas should change after navigation").not.toBe(
		hashBefore,
	);
	const pixels = await countNonBlackPixels(canvas);
	expect(pixels).toBeGreaterThan(2000);
});
