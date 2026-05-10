/**
 * DRI3 was deliberately removed from the server in commit 60b4bd3.
 * The tests here exist as regression guards: they assert that DRI3
 * is not advertised, so an accidental re-introduction (e.g. by
 * registering the extension without a real handler) trips the suite.
 */

import { test, expect } from "../fixtures";

test.describe("DRI3 is not advertised", () => {
	test("xdpyinfo does not list DRI3", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo", "-queryExtensions"]);
		expect(result.exitCode).toBe(0);
		expect(result.output).not.toContain("DRI3");
	});

	test("QueryExtension reports DRI3 as missing", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				// python-xlib's query_extension returns None for missing
				// extensions; print the literal so we can assert below.
				"python3 -c \"import Xlib.display as d; print('absent' if d.Display().query_extension('DRI3') is None else 'present')\"",
			].join("\n"),
		]);
		expect(result.output).toContain("absent");
	});
});
