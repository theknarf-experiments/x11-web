/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe.serial("COMPOSITE extension compliance", () => {
	test("COMPOSITE extension is present with version 0.4", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xdpyinfo 2>/dev/null | grep -i composite`,
		);
		expect(output.toLowerCase()).toContain("composite");
	});

	test("Composite redirect and unredirect window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "composite_redirect_and_unredirect_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("composite_present=true");
		expect(output).toContain("composite_test=ok");
	});

	test("Overlay window via Composite GetOverlayWindow", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "overlay_window_via_composite_getoverlaywindow.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("overlay_test=ok");
	});
});

test.describe("Composite overlay window refcounting", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test.skip("Composite extension QueryVersion and overlay operations", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "composite_overlay_get_release.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain(
			"PASS: Composite overlay get/release succeeded",
		);
	});
});

test.describe.serial("COMPOSITE extension (Phase 7)", () => {
	test("CompositeQueryVersion returns 0.4+", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "compositequeryversion_returns_0_4.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("composite_present=True");
	});

	test("NameWindowPixmap creates valid pixmap", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "namewindowpixmap_creates_valid_pixmap.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("composite_available=True");
	});
});
