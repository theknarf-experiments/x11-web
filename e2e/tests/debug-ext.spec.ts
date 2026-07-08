// Diagnostic toolkit for X11 input debugging — NOT part of the
// regular suite. Run with X11_INPUT_DEBUG=1, e.g.:
//   X11_INPUT_DEBUG=1 SIDECAR_RUST_LOG=info,x11_web_x11_server=debug \
//     pnpm exec playwright test debug-ext.spec.ts -g "<test>"
// Tools: extension dump, xev via XTEST + via frontend, GDK event log,
// full xtrace of Firefox (ours vs Xvfb baseline), request-stream
// diff, gtk3-demo hover probe, WM_DELETE liveness.
import {
	canvasPixelHash,
	colorFraction,
	expect,
	spawnApp,
	test,
	waitForDock,
} from "./fixtures";

test.skip(
	!process.env.X11_INPUT_DEBUG,
	"diagnostics — set X11_INPUT_DEBUG=1 to run",
);

/** The decisive client-side split: gtkprobe logs every raw XEvent
 *  GDK pulls off the socket AND every widget-level event GTK
 *  dispatches. Raw-missing => Xlib/xcb never surfaces our events;
 *  raw-present-widget-missing => gdk_event_translate drops them. */
test("gtkprobe raw vs widget events", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(240_000);
	const run = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const coreMode = !!process.env.GTKPROBE_CORE;
	const frame = coreMode
		? await spawnApp(
				page,
				"GDK_CORE_DEVICE_EVENTS=1 xtrace -n -o /tmp/xp99.log gtkprobe",
				"env",
				60_000,
			)
		: await spawnApp(page, "", "gtkprobe", 60_000);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 60_000 });
	await page.waitForTimeout(3000);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, {
		steps: 6,
	});
	await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
	await page.waitForTimeout(500);
	await page.keyboard.press("g");
	await page.waitForTimeout(2000);

	const logs = await sidecarContainer.logs();
	const chunks: string[] = [];
	await new Promise<void>((resolve) => {
		logs.on("data", (c: unknown) => chunks.push(String(c)));
		logs.on("end", resolve);
		setTimeout(resolve, 5000);
	});
	const probeLines = chunks
		.join("")
		.split("\n")
		.filter((l) => l.includes("[gtkprobe:") || l.includes("[env:"))
		.map((l) => l.replace(/^.*:stdout\] /, ""));
	console.log("GTKPROBE OURS:\n" + probeLines.slice(-80).join("\n"));
	const serverLines = chunks
		.join("")
		.split("\n")
		.filter((l) => /frontend-input|crossing:|core-event|xi-dispatch/.test(l))
		.map((l) => l.replace(/^.*(frontend-input|crossing:)/, "$1"));
	console.log("SERVER INPUT LINES:\n" + serverLines.slice(-40).join("\n"));

	const wire = await run(
		"grep -E 'Event|Reply|Request' /tmp/xp99.log 2>/dev/null | tail -50",
	);
	console.log("CLIENT WIRE VIEW:\n" + wire);

	// Baseline: same probe on Xvfb, synthetic input via xdotool.
	await run("Xvfb :78 -screen 0 1024x768x24 2>/dev/null & sleep 2");
	await run("DISPLAY=:78 nohup gtkprobe > /tmp/probe78.log 2>&1 & sleep 3");
	await run("DISPLAY=:78 xdotool mousemove 200 150 && sleep 1");
	await run("DISPLAY=:78 xdotool click 1 && sleep 1");
	await run("DISPLAY=:78 xdotool key g && sleep 1");
	const baseline = await run("cat /tmp/probe78.log | tail -60");
	console.log("GTKPROBE XVFB:\n" + baseline);
	await run("pkill gtkprobe 2>/dev/null; pkill Xvfb 2>/dev/null; true");
	expect(probeLines.length + baseline.length).toBeGreaterThan(0);
});

/** Is the input deafness Firefox-specific or all of GTK3? A stock
 *  GTK3 demo's buttons prelight on hover and depress on click —
 *  pixel-level changes measurable without app-specific hooks. */
test("gtk3-demo reacts to hover", async ({ page, frontendUrl }) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const frame = await spawnApp(
		page,
		"GDK_CORE_DEVICE_EVENTS=1 gtk3-demo",
		"env",
		60_000,
	);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 60_000 });
	await page.waitForTimeout(4000);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	// Park the pointer outside, hash, hover a button region, re-hash.
	await page.mouse.move(box.x - 60, box.y - 60);
	await page.waitForTimeout(1000);
	const before = await canvasPixelHash(canvas);
	await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, {
		steps: 8,
	});
	await page.waitForTimeout(1500);
	const after = await canvasPixelHash(canvas);
	console.log(
		`GTK3 HOVER: before=${before} after=${after} changed=${before !== after}`,
	);

	// And a click for good measure.
	await page.mouse.down();
	await page.waitForTimeout(400);
	const pressed = await canvasPixelHash(canvas);
	await page.mouse.up();
	console.log(`GTK3 PRESS: pressed=${pressed} changed=${after !== pressed}`);
	expect(before.length).toBeGreaterThan(0);
});

test("dump advertised X extensions", async ({ sidecarContainer }) => {
	const result = await sidecarContainer.exec([
		"sh",
		"-c",
		"xdpyinfo -display :99 | sed -n '/number of extensions/,/^[^ ]/p'",
	]);
	console.log("EXTENSIONS DUMP:\n" + result.output);
	expect(result.output.length).toBeGreaterThan(0);
});

/** What does a core client actually receive when XTEST synthesizes a
 *  click on it? xev prints every event field — malformed timestamps,
 *  coords, state or missing enter/motion events show up here. */
test("xev sees xdotool click", async ({ sidecarContainer }) => {
	test.setTimeout(120_000);
	const run = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);

	await run(
		"DISPLAY=:99 nohup xev -geometry 300x300+200+200 -name xevprobe > /tmp/xev.log 2>&1 &",
	);
	await new Promise((r) => setTimeout(r, 3000));

	// Synthesize: move into the window, then click.
	await run("DISPLAY=:99 xdotool mousemove 350 350");
	await new Promise((r) => setTimeout(r, 500));
	await run("DISPLAY=:99 xdotool click 1");
	await new Promise((r) => setTimeout(r, 1500));
	await run("DISPLAY=:99 xdotool key g");
	await new Promise((r) => setTimeout(r, 1500));

	const log = await run("cat /tmp/xev.log");
	console.log("XEV LOG:\n" + log.slice(-6000));
	expect(log.length).toBeGreaterThan(0);
});

/** What does GDK itself see? Launch Firefox with GDK_DEBUG=events
 *  (stderr → file, since the sidecar never drains child pipes),
 *  click it through the real frontend pipeline, and read GDK's
 *  translation log. Zero button entries ⇒ GDK discarded our events
 *  at translate time (device mismatch etc.); entries present but no
 *  page reaction ⇒ Gecko-level drop. */
test("gdk event log under frontend click", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);

	// `env` as the spawn command injects GDK_DEBUG for firefox; the
	// sidecar drains child stderr into its own log, which we read
	// back through docker logs.
	const firefoxFrame = await spawnApp(
		page,
		"GDK_DEBUG=events firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html",
		"env",
		120_000,
	);
	const canvas = firefoxFrame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 120_000 });
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 120_000,
			intervals: [3000, 3000, 5000, 5000, 10000],
		})
		.toBeGreaterThan(0.5);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.6, {
		steps: 5,
	});
	await page.mouse.click(box.x + box.width / 2, box.y + box.height * 0.6);
	await page.waitForTimeout(1000);
	await page.keyboard.press("g");
	await page.waitForTimeout(2000);

	const logs = await sidecarContainer.logs();
	const chunks: string[] = [];
	await new Promise<void>((resolve) => {
		logs.on("data", (c: unknown) => chunks.push(String(c)));
		logs.on("end", resolve);
		setTimeout(resolve, 5000);
	});
	const gdkLines = chunks
		.join("")
		.split("\n")
		.filter((l) => l.includes(":stderr]") || l.includes(":stdout]"));
	console.log("CHILD LOG LINES: " + gdkLines.length);
	const eventLines = gdkLines.filter((l) =>
		/button|motion|enter|leave|key |focus/i.test(l),
	);
	console.log("GDK EVENT LINES:\n" + eventLines.slice(-60).join("\n"));
	const serverLines = chunks
		.join("")
		.split("\n")
		.filter((l) => /XISelectEvents|crossing:/.test(l));
	console.log(
		"SERVER XI LINES (" +
			serverLines.length +
			"):\n" +
			serverLines.slice(-80).join("\n"),
	);
	expect(gdkLines.length).toBeGreaterThan(0);
});

/** Full protocol trace of Firefox: xtrace proxies the display and
 *  decodes every request/event. Answers what GDK selected and what
 *  it received around a click. */
test("xtrace firefox click", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(300_000);
	const run = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);
	page.on("console", (m) => {
		if (m.text().includes("[input-debug]")) console.log(m.text());
	});
	await page.goto(frontendUrl);
	await waitForDock(page);

	const firefoxFrame = await spawnApp(
		page,
		"-o /tmp/xt.log firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html",
		"xtrace",
		120_000,
	);
	const canvas = firefoxFrame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 120_000 });
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 120_000,
			intervals: [3000, 3000, 5000, 5000, 10000],
		})
		.toBeGreaterThan(0.5);

	await run("echo '==== CLICK MARKER ====' >> /tmp/xt.log || true");
	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.6, {
		steps: 5,
	});
	await page.mouse.click(box.x + box.width / 2, box.y + box.height * 0.6);
	await page.waitForTimeout(2000);

	const selections = await run(
		"grep -iE '131,[0-9]' /tmp/xt.log | sed -E 's/unparsed-data.*//' | head -40",
	);
	console.log("XTRACE XI REQUESTS:\n" + selections);
	const creates = await run(
		"grep -E 'CreateWindow|ChangeWindowAttributes' /tmp/xt.log | grep -E 'event-mask|0040002a|0040002b|00400047' | head -40",
	);
	console.log("WINDOW EVENT MASKS:\n" + creates);
	const afterClick = await run(
		"sed -n '/CLICK MARKER/,$p' /tmp/xt.log | grep -E 'Event' | grep -vE 'NoExposure' | head -60",
	);
	console.log("XTRACE EVENTS AFTER CLICK:\n" + afterClick);

	// Raw frontend-input as the server received it (debug log added
	// to the connection input path).
	const logs = await sidecarContainer.logs();
	const chunks: string[] = [];
	await new Promise<void>((resolve) => {
		logs.on("data", (c: unknown) => chunks.push(String(c)));
		logs.on("end", resolve);
		setTimeout(resolve, 5000);
	});
	const rawLines = chunks
		.join("")
		.split("\n")
		.filter((l) =>
			/frontend-input|xi-dispatch|crossing:|XISelectEvents/.test(l),
		)
		.map((l) =>
			l.replace(
				/^.*(frontend-input|xi-dispatch|crossing:|XISelectEvents)/,
				"$1",
			),
		);
	console.log("SERVER INPUT (last 40):\n" + rawLines.slice(-40).join("\n"));
	expect(selections.length + afterClick.length).toBeGreaterThan(0);
});

/** Differential trace: the same firefox + probe page + click, but on
 *  a REAL X server (Xvfb) inside the same container. Compare its XI
 *  startup and click-time event stream against ours. */
test("xtrace firefox on Xvfb baseline", async ({ sidecarContainer }) => {
	test.setTimeout(300_000);
	const run = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);

	await run("Xvfb :77 -screen 0 1280x800x24 2>/dev/null & sleep 2");
	await run(
		"DISPLAY=:77 XAUTHORITY= nohup xtrace -n -o /tmp/xt77.log firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html > /tmp/ff77.log 2>&1 & sleep 25",
	);
	// Click the content area and check the probe page reacted by
	// screenshotting the root with xwd → count magenta pixels via
	// imagemagick.
	await run("DISPLAY=:77 xdotool mousemove 640 500 && sleep 1");
	await run("DISPLAY=:77 xdotool click 1 && sleep 2");
	const magenta = await run(
		"DISPLAY=:77 import -window root /tmp/s77.png 2>/dev/null; convert /tmp/s77.png -format %c histogram:info:- 2>/dev/null | grep -i 'FF00FF' | head -2",
	);
	console.log("XVFB MAGENTA PIXELS:\n" + magenta);
	const xi = await run(
		"grep -icE 'Request\\(131\\)|XIQueryVersion|XISelectEvents' /tmp/xt77.log",
	);
	console.log("XVFB XI REQUEST LINES: " + xi);
	const clickEvents = await run(
		"grep -E 'Event' /tmp/xt77.log | grep -vE 'NoExposure|Expose|Property|Configure|Reparent|Map|Visibility|Create|Client' | tail -40",
	);
	console.log("XVFB INPUT EVENTS:\n" + clickEvents);
	const diag = await run(
		"ls -la /tmp/xt77.log /tmp/s77.png 2>&1; echo ---; tail -c 1500 /tmp/ff77.log 2>&1; echo ---; DISPLAY=:77 xdpyinfo 2>&1 | head -3",
	);
	console.log("XVFB DIAG:\n" + diag);
	await run("pkill -f 'firefox-esr' 2>/dev/null; pkill Xvfb 2>/dev/null; true");
	expect(xi.length).toBeGreaterThan(0);
});

/** A/B: firefox against our server (:99, via xtrace, dock-spawned)
 *  vs firefox against Xvfb (:77) — histogram of request names and
 *  the first ~120 requests, to find where behavior diverges. */
test("diff firefox request streams ours vs Xvfb", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(400_000);
	const run = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);

	// --- ours (:99) through the dock, traced ---
	await page.goto(frontendUrl);
	await waitForDock(page);
	const firefoxFrame = await spawnApp(
		page,
		"-o /tmp/xt99.log firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html",
		"xtrace",
		120_000,
	);
	await expect(firefoxFrame.locator('[data-testid="x11-canvas"]')).toBeVisible({
		timeout: 120_000,
	});
	await page.waitForTimeout(15_000);

	// --- Xvfb baseline (:77) ---
	await run("Xvfb :77 -screen 0 1280x800x24 2>/dev/null & sleep 2");
	await run(
		"DISPLAY=:77 XAUTHORITY= nohup xtrace -n -o /tmp/xt77.log firefox-esr --no-remote --new-instance -P xvfbprof /opt/test-content/input-probe.html > /tmp/ff77.log 2>&1 & sleep 20",
	);

	const hist = (f: string) =>
		run(
			`grep -oE '(: |-)Request\\([0-9]+(,[0-9]+)?\\): [A-Za-z]+' ${f} | sed 's/.*: //' | sort | uniq -c | sort -rn | head -40`,
		);
	console.log("REQUEST HISTOGRAM OURS:\n" + (await hist("/tmp/xt99.log")));
	console.log("REQUEST HISTOGRAM XVFB:\n" + (await hist("/tmp/xt77.log")));

	const firstReqs = (f: string) =>
		run(
			`grep -oE 'Request\\([0-9]+(,[0-9]+)?\\): [A-Za-z]+' ${f} | head -120 | awk '{print $2}' | paste -sd, -`,
		);
	console.log("FIRST 120 OURS:\n" + (await firstReqs("/tmp/xt99.log")));
	console.log("FIRST 120 XVFB:\n" + (await firstReqs("/tmp/xt77.log")));

	// Errors either side
	console.log(
		"ERRORS OURS:\n" + (await run("grep -iE 'Error' /tmp/xt99.log | head -20")),
	);
	console.log(
		"ERRORS XVFB:\n" + (await run("grep -iE 'Error' /tmp/xt77.log | head -20")),
	);
	await run("pkill Xvfb 2>/dev/null; true");
	expect(1).toBe(1);
});

/** Decisive: Firefox with XInputExtension disabled via kill switch.
 *  If it falls back to core and :hover(blue)+click(magenta) work,
 *  the fault is XI-specific and gating XI2 fixes Firefox. */
test("firefox with xinput disabled", async ({ page, frontendUrl }) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);
	const frame = await spawnApp(
		page,
		"--no-remote --new-instance file:///opt/test-content/input-probe.html",
		"firefox-esr",
		120_000,
	);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 120_000 });
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 120_000,
			intervals: [3000, 3000, 5000, 5000, 10000],
		})
		.toBeGreaterThan(0.5);
	const box = await canvas.boundingBox();
	if (!box) throw new Error("no box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.6, {
		steps: 6,
	});
	await page.waitForTimeout(2000);
	console.log(
		"XI-OFF FIREFOX blue=" + (await colorFraction(canvas, [0, 0, 255])),
	);
	await page.mouse.click(box.x + box.width / 2, box.y + box.height * 0.6);
	await page.waitForTimeout(2000);
	console.log(
		"XI-OFF FIREFOX magenta=" + (await colorFraction(canvas, [255, 0, 255])),
	);
	expect(1).toBe(1);
});

/** Decisive multiprocess check: single-process Firefox
 *  (MOZ_FORCE_DISABLE_E10S) renders content in the chrome process, so
 *  the content window lives on the chrome X connection and is in our
 *  hit-test tree. If :hover(blue) then works, Firefox input death is
 *  the cross-connection content-window problem, not core delivery. */
test("firefox single-process hover", async ({ page, frontendUrl }) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const frame = await spawnApp(
		page,
		"MOZ_FORCE_DISABLE_E10S=1 firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html",
		"env",
		120_000,
	);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 120_000 });
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 120_000,
			intervals: [3000, 3000, 5000, 5000, 10000],
		})
		.toBeGreaterThan(0.5);
	const box = await canvas.boundingBox();
	if (!box) throw new Error("no box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.6, {
		steps: 6,
	});
	await page.waitForTimeout(2000);
	const blue = await colorFraction(canvas, [0, 0, 255]);
	console.log("SINGLE-PROC FIREFOX blue-fraction=" + blue);
	await page.mouse.click(box.x + box.width / 2, box.y + box.height * 0.6);
	await page.waitForTimeout(2000);
	const magenta = await colorFraction(canvas, [255, 0, 255]);
	console.log("SINGLE-PROC FIREFOX magenta-fraction=" + magenta);
	expect(blue + magenta).toBeGreaterThanOrEqual(0);
});

/** Does Firefox's GTK loop consume X events at all? WM_DELETE via
 *  the frame close button exits Firefox iff ClientMessage events are
 *  read and dispatched — isolating "X event consumption dead" from
 *  "pointer events specifically dropped". */
test("firefox reacts to WM_DELETE", async ({ page, frontendUrl }) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const firefoxFrame = await spawnApp(
		page,
		"--no-remote --new-instance file:///opt/test-content/input-probe.html",
		"firefox-esr",
		120_000,
	);
	const canvas = firefoxFrame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 120_000 });
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 120_000,
			intervals: [3000, 3000, 5000, 5000, 10000],
		})
		.toBeGreaterThan(0.5);

	await firefoxFrame.dispatchEvent("pointerdown");
	await page.waitForTimeout(300);
	await firefoxFrame.locator('[data-testid="window-close"]').click();
	await expect(page.locator('[data-testid="window-frame"]')).toHaveCount(0, {
		timeout: 20_000,
	});
});

/** Ground truth for the FRONTEND input path: xev spawned via the
 *  dock prints every core event it receives; the sidecar drains its
 *  stdout into docker logs. */
test("xev sees frontend click", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(240_000);
	await page.goto(frontendUrl);
	await waitForDock(page);

	const xevFrame = await spawnApp(page, "-geometry 300x300", "xev", 60_000);
	const canvas = xevFrame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 60_000 });
	await page.waitForTimeout(2000);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, {
		steps: 5,
	});
	await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
	await page.waitForTimeout(500);
	await page.keyboard.press("g");
	await page.waitForTimeout(2000);

	const logs = await sidecarContainer.logs();
	const chunks: string[] = [];
	await new Promise<void>((resolve) => {
		logs.on("data", (c: unknown) => chunks.push(String(c)));
		logs.on("end", resolve);
		setTimeout(resolve, 5000);
	});
	const xevLines = chunks
		.join("")
		.split("\n")
		.filter((l) => l.includes("[xev:"))
		.map((l) => l.replace(/^.*\[xev:\d+:stdout\] /, ""));
	console.log("XEV VIA FRONTEND:\n" + xevLines.slice(-80).join("\n"));
	expect(xevLines.length).toBeGreaterThan(0);
});
