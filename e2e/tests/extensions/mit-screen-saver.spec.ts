/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, test } from "../fixtures";

test.describe("screen saver", () => {
	test("GetScreenSaver returns settings", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"ss = d.get_screen_saver()",
				"print(f'timeout={ss.timeout}')",
				"print(f'interval={ss.interval}')",
				"print(f'prefer_blank={ss.prefer_blanking}')",
				"print(f'allow_exposures={ss.allow_exposures}')",
				"print('SCREEN_SAVER_GET_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SCREEN_SAVER_GET_PASS");
	});

	test("SetScreenSaver round-trips timeout", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"d.set_screen_saver(timeout=300, interval=60, prefer_blank=1, allow_exposures=1)",
				"d.sync()",
				"ss = d.get_screen_saver()",
				"print(f'timeout={ss.timeout}')",
				"assert ss.timeout == 300, f'Expected 300, got {ss.timeout}'",
				"assert ss.interval == 60, f'Expected 60, got {ss.interval}'",
				"# Restore defaults",
				"d.set_screen_saver(timeout=0, interval=0, prefer_blank=0, allow_exposures=0)",
				"print('SCREEN_SAVER_SET_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("SCREEN_SAVER_SET_PASS");
	});

	test("ForceScreenSaver activate/reset works", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"d.force_screen_saver(1)  # Activate",
				"d.sync()",
				"d.force_screen_saver(0)  # Reset",
				"d.sync()",
				"print('FORCE_SCREEN_SAVER_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("FORCE_SCREEN_SAVER_PASS");
	});
});
