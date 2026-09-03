/**
 * REPRODUCER for the long-standing "Firefox is deaf to input" bug
 * (`x11-web.spec.ts:832`, which carries `test.fail`). Opt-in, because it
 * is a diagnostic with no assertions — it prints a colour timeline:
 *
 *     X11WEB_FIREFOX_DIAG=1 pnpm --filter x11-web-e2e exec playwright test \
 *       --workers=1 --repeat-each=4 tests/firefox-input-diag.spec.ts
 *
 * THE FINDING: only the FIRST Firefox launch in a fresh container is
 * deaf. Every later launch in the same container works instantly. That
 * is 100% consistent across repeats and takes ~8 minutes, replacing the
 * old belief that this was an unreproducible intermittency.
 *
 * Iteration 1: hover/click/key all 0.000 for 25-55s.
 * Iterations 2+: `HOVER WORKED after ~0s`, then click and key.
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
 * So the warming factor is in-memory, not on disk. The two live
 * candidates are (a) lazily-initialised state inside the sidecar's X
 * server, which is the same process across iterations — the OSMesa/GLX
 * path is the obvious suspect since Firefox probes GLX at startup — and
 * (b) a startup-speed race: run 1 pays cold page-cache costs and starts
 * far slower (the one screencast-confirmed success had its window mapped
 * 1.85s after spawn). Next step is to log Firefox's spawn->window
 * duration per iteration and dump the sidecar log around the first vs
 * second launch, to tell those apart.
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

	await page.goto(frontendUrl);
	await waitForDock(page);

	const frame = await spawnApp(
		page,
		"--no-remote --new-instance file:///opt/test-content/input-probe.html",
		"firefox-esr",
		180_000,
	);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 180_000 });

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
