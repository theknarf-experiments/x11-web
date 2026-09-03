/**
 * REGRESSION TEST + instruments for the GLX input bug (FIXED).
 *
 *     X11WEB_FIREFOX_DIAG=1 [DIAG_XTRACE=1] \
 *       pnpm --filter x11-web-e2e exec playwright test --workers=1 \
 *       tests/firefox-input-diag.spec.ts
 *
 * THE BUG THAT WAS: when the server advertised GLX, the GTK3 client that
 * performed GDK's GLX-based visual probe became unable to dispatch input —
 * no hover, no click, no keys, forever. GDK caches that probe's result in a
 * `GDK_VISUALS` property on the root window, so every LATER GTK3 client on
 * the same display skipped the probe and worked fine. That is why it looked
 * like "only the first client is deaf", and why any prior GTK3 client
 * "warmed" the display. Firefox was just the app people noticed.
 *
 * THE CAUSE was one field: we answered QueryExtension("GLX") with
 * first_event = 0. libGL registers via libXext's XextAddDisplay with
 * __GLX_NUMBER_EVENTS = 17, which installs __glXWireToEvent into
 * dpy->event_vec[first_event + 0..16] — i.e. straight over Xlib's own
 * handlers for KeyPress(2) .. CreateNotify(16). __glXWireToEvent returns
 * False for those, and Xlib discards any event whose hook returns False.
 * GLX now reports a real base of 66; see
 * crates/x11-server/src/xserver/extensions.rs.
 *
 * A SHORTER REGRESSION CHECK, ~2 min, no Firefox:
 *     X11_INPUT_DEBUG=1 pnpm --filter x11-web-e2e exec \
 *       playwright test --workers=1 --repeat-each=2 -g "gtk3-demo reacts to hover"
 *     Both runs must report HOVER changed=true. Before the fix, run 1 was
 *     changed=false and run 2 changed=true.
 *
 * THE KEY ASSET: the same Firefox, in the same container, on the same
 * Mesa/llvmpipe stack, WORKS against Xvfb on :78 — its probe page goes blue
 * on hover — and is deaf against ours on :99. Start Xvfb inside the sidecar
 * container and you have a known-good and known-bad server side by side.
 *
 * WHAT THE TRACES SHOW (DIAG_XTRACE=1 dumps a profile of everything the
 * server sends, plus the delivered input events):
 *  - We DELIVER correctly. A deaf Firefox still receives 2 EnterNotify,
 *    1 LeaveNotify and 13 MotionNotify, with counts and target windows
 *    byte-identical to the working GLX-off run. The client drops them.
 *  - The deaf run does uniformly LESS startup work: QueryPointer 0 vs 6,
 *    GetWindowAttributes 3 vs 6, ConfigureNotify 6 vs 12, MapNotify 2 vs 4,
 *    no UnmapNotify at all. It is bailing out of an init path, not being
 *    starved of events. Xvfb answers 5 QueryPointer in its working run.
 *  - Firefox only QUERIES GLX (QueryServerString, QueryVersion,
 *    GetFBConfigs, GetVisualConfigs, ClientInfo). It never creates a
 *    context and gets no protocol errors.
 *
 * RULED OUT BY EXPERIMENT — do not re-run these:
 *  - Page readiness: white=0.966 from the first sample, steady 55s, blue 0.
 *  - Motion-specific: click and key are equally dead.
 *  - The GTK a11y bridge: NO_AT_BRIDGE=1 is already set image-wide.
 *  - Window tree, visual, depth, colormap, and selected event masks: all
 *    identical between the working and broken runs.
 *  - The Firefox profile and the entire home dir (DIAG_WIPE_PROFILE=1).
 *  - A startup-speed race: spawn->window 2.08s deaf vs 1.85s working.
 *  - The advertised GLX extension list (3 -> 22 did not help; it also broke
 *    a glx.spec.ts assertion).
 *  - The config replies: zero visual configs + zero FBConfigs did not help.
 *  - Direct rendering: generating the full FBConfig permutation set DID flip
 *    `direct rendering` No -> Yes, matching Xvfb. Firefox stayed deaf and
 *    glxgears began segfaulting, so it was reverted.
 *  - Firefox's GL compositor: MOZ_ACCELERATED=0 MOZ_WEBRENDER=0 stays deaf,
 *    and gtk3-demo has no WebRender yet is affected identically.
 *  - The target_wid vs deepest-window mismatch in the browser input path: it
 *    is present in the working run too.
 *
 * NEXT: diff the full :78 and :99 traces around window mapping and the
 * property/focus traffic, not just the input events — that is where the
 * "does less startup work" divergence begins. The one field-level difference
 * found that way so far was the crossing `focus` flag, fixed in 6a9a87a.
 *
 * Incidental, worth fixing separately: the per-test X reset kills the parent
 * firefox-esr but ORPHANS its children (Socket Process, RDD, WebExtensions,
 * Web Content x3, crashhelper), reparented to pid 1.
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
				`-n -o /tmp/ff-trace.log ${process.env.DIAG_FF_ENV ?? ""} firefox-esr --no-remote --new-instance file:///opt/test-content/input-probe.html`.replace(
					/\s+/g,
					" ",
				),
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
					'echo "--- profile of everything the server sends:"; ' +
					'sed -E "s/0x[0-9a-f]+/H/g; s/=[0-9]+/=N/g" /tmp/ff-trace.log | grep -oE "^[0-9]+:>:[^ ]* (Event [A-Za-z]+|Reply to [A-Za-z]+|Error [A-Za-z]+)" | sed -E "s/^[0-9]+:>:[^ ]* //" | sort | uniq -c | sort -rn | head -32',
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
