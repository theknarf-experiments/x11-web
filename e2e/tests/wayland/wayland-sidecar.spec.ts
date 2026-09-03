/**
 * End-to-end coverage for the Wayland sidecar.
 *
 * The X11 suite's equivalent assertions ride on X11 apps (xeyes,
 * firefox). Here the two clients are:
 *
 *   - `weston-simple-shm` — a real third-party Wayland client, part of
 *     the weston reference stack. It exercises wl_compositor + wl_shm +
 *     xdg_shell and nothing else, and it animates continuously, so
 *     "pixels arrive and keep arriving" is not timing-dependent.
 *   - `wl-input-probe` — our own probe (built into the image from
 *     crates/wayland-server/test-tools/wl-input-probe.c). It starts
 *     white and can ONLY turn magenta by receiving a real
 *     wl_pointer.button and green by receiving a real wl_keyboard.key
 *     for `g`. That makes an exact-colour pixel fraction a sound
 *     assertion for input delivery — the same trick the Firefox
 *     input-probe page plays on the X11 side, and immune to the
 *     animation/repaint timing that makes pixel hashes lie.
 *
 * Everything runs against this suite's own backend + sidecar pair (see
 * ./fixtures.ts) so the X11 specs are untouched.
 */

import {
	cleanupWaylandApps,
	colorFraction,
	countNonBlackPixels,
	expect,
	hasRenderedContent,
	spawnApp,
	test,
	waitForDock,
} from "./fixtures";

test.afterEach(async ({ waylandSidecarContainer }) => {
	await cleanupWaylandApps(waylandSidecarContainer);
});

/** Container-only smoke: the compositor is up and advertises exactly
 *  the globals the vertical slice promises. No browser involved, so a
 *  failure here localises the problem to the sidecar immediately. */
test("compositor advertises the vertical slice's globals", async ({
	waylandSidecarContainer,
}) => {
	// The socket name is *derived*, not hardcoded to wayland-1:
	// `ListeningSocketSource::new_auto` takes the first free name in
	// wayland-1..wayland-32, and a stale lock file left behind in a
	// `.withReuse()`d container moves it to wayland-2 — at which point a
	// literal would fail with a connect error rather than with anything
	// that names the real problem. Same expression as
	// e2e/scripts/wayland-smoke.sh.
	const result = await waylandSidecarContainer.exec([
		"bash",
		"-c",
		"export XDG_RUNTIME_DIR=/run/user/0; " +
			"WAYLAND_DISPLAY=$(ls /run/user/0 | grep -E '^wayland-[0-9]+$' | head -1) wayland-info",
	]);
	expect(result.exitCode).toBe(0);
	for (const iface of [
		"wl_compositor",
		"wl_subcompositor",
		"wl_shm",
		"xdg_wm_base",
		"wl_seat",
		"wl_output",
		"wp_viewporter",
		"zxdg_decoration_manager_v1",
	]) {
		expect(result.output, `missing global ${iface}`).toContain(
			`interface: '${iface}'`,
		);
	}
});

test("a wayland client's window appears with its own title and real pixels", async ({
	page,
	frontendUrl,
}) => {
	await page.goto(frontendUrl);
	await waitForDock(page);

	const frame = await spawnApp(page, "", "weston-simple-shm", 30_000);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 30_000 });

	// The title bar must show the xdg_toplevel's OWN title, not the
	// command we spawned — `set_title("simple-shm")` vs the command
	// `weston-simple-shm`. `exact: true` is what makes the two
	// distinguishable, and proves TitleChanged really came from the
	// Wayland client.
	await expect(frame.getByText("simple-shm", { exact: true })).toBeVisible({
		timeout: 30_000,
	});

	// Pixels: the shm buffer must have made it through commit ->
	// BGRA/RGBA swizzle -> composite -> PutImage -> WebP -> canvas.
	await expect
		.poll(() => hasRenderedContent(canvas), {
			timeout: 30_000,
			intervals: [500, 500, 1000, 2000],
		})
		.toBe(true);
	expect(await countNonBlackPixels(canvas)).toBeGreaterThan(200);
});

/** Deterministic input check — the gating assertion for the whole
 *  browser -> backend -> sidecar -> wl_seat -> client path. */
test("a wayland client reacts to a click and a keystroke", async ({
	page,
	frontendUrl,
	waylandSidecarContainer,
}) => {
	await page.goto(frontendUrl);
	await waitForDock(page);

	const frame = await spawnApp(page, "", "wl-input-probe", 30_000);
	const canvas = frame.locator('[data-testid="x11-canvas"]');
	await expect(canvas).toBeVisible({ timeout: 30_000 });

	// Idle state: a solid white 400x300 surface.
	await expect
		.poll(() => colorFraction(canvas, [255, 255, 255]), {
			timeout: 30_000,
			intervals: [500, 500, 1000, 2000],
		})
		.toBeGreaterThan(0.9);

	const box = await canvas.boundingBox();
	if (!box) throw new Error("probe canvas has no bounding box");

	// Click: only a real wl_pointer.button can produce magenta.
	await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
	await expect
		.poll(() => colorFraction(canvas, [255, 0, 255]), { timeout: 20_000 })
		.toBeGreaterThan(0.9);

	// Keystroke: the click above focused the canvas, so `g` must route
	// through the seat's keyboard focus to the client. Only a real
	// wl_keyboard.key whose keysym resolves to `g` produces green —
	// which also proves the compositor's xkb keymap and the X11
	// keycode -> evdev translation agree with the client's.
	await page.keyboard.press("g");
	await expect
		.poll(() => colorFraction(canvas, [0, 204, 0]), { timeout: 20_000 })
		.toBeGreaterThan(0.9);

	// Sidecar-side corroboration: the probe prints every event it
	// receives and the sidecar drains a child's stdout into its own
	// log, so the log independently confirms what the pixels claim.
	const logs = await waylandSidecarContainer.logs();
	const text = await new Promise<string>((resolve) => {
		let buf = "";
		logs.on("data", (chunk) => {
			buf += chunk.toString();
		});
		setTimeout(() => resolve(buf), 2000);
	});
	expect(text).toContain("PROBE pointer.button");
	expect(text).toContain("PROBE kb.key");
});
