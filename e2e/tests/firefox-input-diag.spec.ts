/**
 * REPRODUCER for the long-standing "Firefox is deaf to input" bug
 * (`x11-web.spec.ts:832`, which carries `test.fail`). Opt-in, because it
 * is a diagnostic with no assertions — it prints a colour timeline:
 *
 *     X11WEB_FIREFOX_DIAG=1 pnpm --filter x11-web-e2e exec playwright test \
 *       --workers=1 --repeat-each=4 tests/firefox-input-diag.spec.ts
 *
 * THE FINDING: only the FIRST GTK3 client in a fresh container is deaf.
 * Every later one works instantly. This is NOT Firefox-specific, and it
 * is 100% consistent across repeats, which replaces the old belief that
 * this was an unreproducible intermittency.
 *
 * Iteration 1: hover/click/key all 0.000 for 25-55s.
 * Iterations 2+: `HOVER WORKED after ~0s`, then click and key.
 *
 * The SHORTEST reproducer is not this file — it is the existing
 * gtk3-demo probe, ~2 minutes, no Firefox involved:
 *
 *     X11_INPUT_DEBUG=1 pnpm --filter x11-web-e2e exec playwright test \
 *       --workers=1 --repeat-each=2 -g "gtk3-demo reacts to hover"
 *     run 1: GTK3 HOVER changed=false   GTK3 PRESS changed=false
 *     run 2: GTK3 HOVER changed=true    GTK3 PRESS changed=true
 *
 * That also explains why the earlier investigation concluded "GTK3 input
 * is FIXED, Firefox is specially broken": gtkprobe was never the first
 * GTK3 client in its container. The conclusion was a test-ordering
 * artifact. Note the gtk3-demo probe runs with GDK_CORE_DEVICE_EVENTS=1,
 * i.e. XI2 bypassed client-side, and is STILL deaf when first — so this
 * is not simply an XI2 story either.
 *
 * WHAT WARMS IT (DIAG_PREWARM=<cmd>, run in an otherwise fresh container
 * before Firefox):
 *     none       -> deaf
 *     xterm      -> deaf     (a plain core-events client is not enough)
 *     glxgears   -> deaf     (it does reach our GLX: glXCreateContext failed)
 *     gtk3-demo  -> WORKS    (any GTK3 client warms it)
 *
 * AND THE DECISIVE ONE — DIAG_PREWARM_XVFB=1 runs gtk3-demo against a
 * throwaway Xvfb on :78, so every container-level side effect of "a GTK3
 * app has run here" happens (dbus activation, fontconfig, GTK caches,
 * libraries paged in) while our server on :99 never sees a GTK3 client.
 * Result: STILL DEAF. So the warming is not environmental — it is state
 * inside OUR X SERVER PROCESS, created when the first GTK3 client
 * connects to it. The bug is ours.
 *
 * NEXT STEP: xtrace the first and second GTK3 client against :99 and
 * diff them. The difference in what the server answers — most likely an
 * atom, a property, or a device/extension query that only resolves once
 * some other client has interned or created it — should name the bug.
 *
 * RULED OUT, each by measurement rather than argument:
 *  - Page readiness. `white=0.966` from the first sample and steady for
 *    55s while blue stays 0.000, so the probe page is fully rendered and
 *    simply deaf. (The white gate IS still too weak to distinguish the
 *    probe page from Firefox's blank page — both #ffffff — but that is
 *    not what is happening here.)
 *  - Anything motion-specific: click and key are equally dead.
 *  - The GTK a11y bridge: NO_AT_BRIDGE=1 is already set image-wide.
 *  - The X window tree and focus: dumps from a deaf and a working run
 *    are structurally identical — same 921x691 Navigator, same four
 *    200x200 windows, focus on the same 1x1 child in both.
 *  - The Firefox profile, and in fact the whole home directory:
 *    DIAG_WIPE_PROFILE=1 deletes ~/.mozilla, ~/.cache and ~/Downloads
 *    before every launch, `ls /root` confirms only .bashrc/.profile
 *    remain each time, and iterations 2+ STILL work.
 *
 *  - A startup-speed race. `DIAG SPAWN->WINDOW` is 2.08s in a DEAF run
 *    versus 1.85s in the screencast-confirmed working one, so run 1 is
 *    not meaningfully slower and cold page cache is not the trigger.
 *  - GLX / OSMesa lazy init: a glxgears pre-warm does not help.
 *
 * Incidental find, worth fixing separately: the per-test X reset kills
 * the parent firefox-esr but ORPHANS its children (Socket Process, RDD,
 * WebExtensions, Web Content x3, crashhelper), which survive reparented
 * to pid 1.
 */
import { colorFraction, expect, spawnApp, test, waitForDock } from "./fixtures";

test.skip(
	!process.env.X11WEB_FIREFOX_DIAG,
	"diagnostic; set X11WEB_FIREFOX_DIAG=1 to run",
);

const WHITE: [number, number, number] = [255, 255, 255];
const BLUE: [number, number, number] = [0, 0, 255];
const MAGENTA: [number, number, number] = [255, 0, 255];
const GREEN: [number, number, number] = [0, 204, 0];

test("DIAG: firefox input with no timing assumptions", async ({
	page,
	frontendUrl,
	sidecarContainer,
}) => {
	test.setTimeout(600_000);

	// DIAG_WIPE_PROFILE=1 deletes Firefox's profile before each launch,
	// which is the discriminator for "run 1 is deaf because the profile
	// is being created". If every iteration goes deaf with this set, the
	// warming factor is the profile; if iteration 2 still works, it is
	// something else the first run leaves in the container.
	if (process.env.DIAG_WIPE_PROFILE) {
		const out = await sidecarContainer
			.exec([
				"sh",
				"-c",
				"rm -rf /root/.mozilla /root/.cache /root/Downloads $HOME/.mozilla $HOME/.cache 2>&1; echo wiped; ls -a /root 2>&1 | head",
			])
			.then((r) => r.output);
		console.log(`DIAG PROFILE WIPE: ${out.replace(/\n/g, " | ")}`);
	}

	// DIAG_PREWARM=<cmd> runs another X client to completion-ish before
	// Firefox, in an otherwise fresh container. This is the three-way
	// discriminator for what "warms" the container:
	//   glxgears  -> if this fixes run 1, the culprit is lazily
	//                initialised GLX/OSMesa state in OUR X server, which
	//                is the same process across iterations.
	//   gtk3-demo -> if this fixes it but glxgears does not, it is the
	//                GTK/GDK stack (page cache, or GDK-side init).
	//   neither   -> it is specific to Firefox itself.
	// A prior `xterm` is already known NOT to warm it, so a plain X
	// client is not enough.
	if (process.env.DIAG_PREWARM) {
		const out = await sidecarContainer
			.exec([
				"sh",
				"-c",
				`DISPLAY=:99 nohup ${process.env.DIAG_PREWARM} >/tmp/prewarm.log 2>&1 & sleep 10; echo "prewarm rc=$?"; head -5 /tmp/prewarm.log 2>&1`,
			])
			.then((r) => r.output);
		console.log(
			`DIAG PREWARM(${process.env.DIAG_PREWARM}): ${out.replace(/\n/g, " | ")}`,
		);
	}

	// DIAG_PREWARM_XVFB=1 is THE discriminator for where the warming
	// lives. It runs a GTK3 client against a throwaway Xvfb on :78, so
	// every container-level side effect of "a GTK3 app has run here"
	// happens — dbus activation, fontconfig caches, GTK module/immodule
	// caches, shared libraries paged in — while OUR X server on :99
	// never sees a GTK3 client at all.
	//   Firefox then works  -> the warming is container/environment-level
	//                          and our server is innocent.
	//   Firefox still deaf  -> the warming is state inside our X server
	//                          process (interned atoms, some lazily
	//                          initialised per-first-GTK-client path),
	//                          i.e. the bug is ours.
	if (process.env.DIAG_PREWARM_XVFB) {
		const out = await sidecarContainer
			.exec([
				"sh",
				"-c",
				"Xvfb :78 -screen 0 1024x768x24 >/tmp/xvfb.log 2>&1 & sleep 3; " +
					"DISPLAY=:78 nohup gtk3-demo >/tmp/prewarm78.log 2>&1 & sleep 10; " +
					'echo "xvfb-prewarm done"; DISPLAY=:78 xwininfo -root -children 2>&1 | head -6',
			])
			.then((r) => r.output);
		console.log(`DIAG PREWARM_XVFB: ${out.replace(/\n/g, " | ")}`);
	}

	await page.goto(frontendUrl);
	await waitForDock(page);

	// Time the spawn. The competing hypothesis to server-side lazy init
	// is a plain startup-speed race: run 1 pays cold page-cache costs.
	// The one screencast-confirmed success had its window mapped 1.85s
	// after the spawn click, so if run 1 is dramatically slower here that
	// is evidence for the race.
	// DIAG_XTRACE=1 runs Firefox under xtrace so we can count what the
	// server actually DELIVERS to it. That is the delivery-vs-dispatch
	// split: input events present in the trace but no visible reaction
	// means Gecko is dropping them; absent from the trace means we never
	// sent them.
	const spawnStart = performance.now();
	const frame = process.env.DIAG_XTRACE
		? await spawnApp(
				page,
				"-n -o /tmp/ff-trace.log firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html",
				"xtrace",
				180_000,
			)
		: await spawnApp(
				page,
				"--no-remote --new-instance file:///opt/test-content/input-probe.html",
				"firefox-esr",
				180_000,
			);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 180_000 });
	console.log(
		`DIAG SPAWN->WINDOW: ${((performance.now() - spawnStart) / 1000).toFixed(2)}s`,
	);

	const sample = async (label: string) => {
		const [w, b, m, g] = await Promise.all([
			colorFraction(canvas, WHITE),
			colorFraction(canvas, BLUE),
			colorFraction(canvas, MAGENTA),
			colorFraction(canvas, GREEN),
		]);
		console.log(
			`DIAG ${label}: white=${w.toFixed(3)} blue=${b.toFixed(3)} magenta=${m.toFixed(3)} green=${g.toFixed(3)}`,
		);
		return { w, b, m, g };
	};

	// Phase 1 — let the window settle. 15s is plenty: a previous run of
	// this diagnostic showed white pinned at 0.966 from the first sample
	// through 55s, which is what killed the "the page had not loaded
	// yet" theory.
	for (let i = 0; i < 3; i++) {
		await sample(`settle t=${i * 5}s`);
		await page.waitForTimeout(5000);
	}

	// Dump the X state before touching anything. Run 1 in a container is
	// deaf and runs 2+ work, so diffing these two dumps is the whole
	// question: a first-run-only extra window overlapping the probe would
	// make `find_deepest_window` misroute every event, which would look
	// exactly like total deafness.
	const sh = (cmd: string) =>
		sidecarContainer.exec(["sh", "-c", cmd]).then((r) => r.output);
	console.log(
		`DIAG XTREE:\n${await sh("DISPLAY=:99 xwininfo -root -tree 2>&1 | head -60")}`,
	);
	console.log(
		`DIAG XFOCUS:\n${await sh("DISPLAY=:99 xdotool getwindowfocus getwindowname 2>&1; DISPLAY=:99 xdpyinfo | grep -i 'focus' 2>&1")}`,
	);
	console.log(`DIAG PROCS:\n${await sh("ps -eo pid,ppid,comm | head -40")}`);

	// Phase 2 — hover, then sample for 60s. The real test allows 30s.
	const box = await canvas.boundingBox();
	if (!box) throw new Error("no canvas box");
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.6, {
		steps: 5,
	});
	for (let i = 0; i < 5; i++) {
		const s = await sample(`hover t=${i * 5}s`);
		if (s.b > 0.4) {
			console.log(`DIAG HOVER WORKED after ~${i * 5}s`);
			break;
		}
		await page.waitForTimeout(5000);
	}

	// Phase 2b — leave and re-enter. This tests the leading hypothesis for
	// the first-client deafness: XI2 crossings are only emitted when the
	// pointer CHANGES window, and `build_xi_crossing_events_for` reads the
	// per-connection `xi.selections`. If the client's one and only XI_Enter
	// was emitted before it had called XISelectEvents, it missed it — and
	// since `last_entered_window` is now set, no further Enter is generated
	// while the pointer stays inside. GDK will not translate XI_Motion for a
	// window it never saw an XI_Enter for, so the window is deaf forever.
	// A fresh leave/re-enter cycle should deliver an Enter it can actually
	// see. If blue appears after this, the hypothesis is confirmed.
	await page.mouse.move(5, 5, { steps: 3 });
	await page.waitForTimeout(1500);
	await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.5, {
		steps: 8,
	});
	for (let i = 0; i < 4; i++) {
		const s = await sample(`re-enter t=${i * 5}s`);
		if (s.b > 0.4) {
			console.log(`DIAG RE-ENTER WOKE IT after ~${i * 5}s`);
			break;
		}
		await page.waitForTimeout(5000);
	}

	// Dump the server's own routing decisions. `browser input routing` logs
	// the window the frontend addressed vs the deepest window actually under
	// the pointer; if those disagree the toolkit is told the pointer is in
	// one window and handed a button press for another.
	{
		const logs = await sidecarContainer.logs();
		const text = await new Promise<string>((resolve) => {
			let buf = "";
			logs.on("data", (c) => {
				buf += c.toString();
			});
			setTimeout(() => resolve(buf), 2500);
		});
		const routing = text
			.split("\n")
			.filter((l) => l.includes("browser input routing"))
			.slice(-6);
		console.log(
			`DIAG ROUTING (${routing.length} lines):\n${routing.join("\n")}`,
		);
	}

	if (process.env.DIAG_XTRACE) {
		const counts = await sidecarContainer
			.exec([
				"sh",
				"-c",
				'grep -oE "Event (MotionNotify|ButtonPress|ButtonRelease|KeyPress|KeyRelease|EnterNotify|LeaveNotify)" /tmp/ff-trace.log | sort | uniq -c; ' +
					'echo "--- which windows are the crossings/motion addressed to:"; ' +
					'grep -E "Event (MotionNotify|EnterNotify|LeaveNotify|ButtonPress)" /tmp/ff-trace.log | grep -oE "event=0x[0-9a-f]+" | sort | uniq -c; ' +
					'echo "--- the actual crossing/motion event fields:"; ' +
					'grep -E "Event (EnterNotify|LeaveNotify|MotionNotify)" /tmp/ff-trace.log | cut -c1-190 | head -8',
			])
			.then((r) => r.output);
		console.log(`DIAG DELIVERED:\n${counts}`);
	}

	// Phase 3 — click, then sample.
	await page.mouse.click(box.x + box.width / 2, box.y + box.height * 0.6);
	for (let i = 0; i < 3; i++) {
		const s = await sample(`click t=${i * 5}s`);
		if (s.m > 0.4) {
			console.log(`DIAG CLICK WORKED after ~${i * 5}s`);
			break;
		}
		await page.waitForTimeout(5000);
	}

	// Phase 4 — key.
	await page.keyboard.press("g");
	for (let i = 0; i < 3; i++) {
		const s = await sample(`key t=${i * 5}s`);
		if (s.g > 0.4) {
			console.log(`DIAG KEY WORKED after ~${i * 5}s`);
			break;
		}
		await page.waitForTimeout(5000);
	}
});
