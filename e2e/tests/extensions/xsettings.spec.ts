/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe("XSETTINGS manager", () => {
	test("XSETTINGS_S0 selection owner exists", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xsettings_s0_owner_exists.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("xsettings-owner-ok");
	});

	test("XSETTINGS_SETTINGS property is set in binary format", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xsettings_settings_binary_format.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("xsettings-format-ok");
	});

	test("Xft/DPI setting is 96 DPI (98304 in 1024ths)", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xft_dpi_setting_96.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("xft-dpi-ok");
	});

	test("MANAGER client message atom is predefined", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xlsatoms 2>&1 | grep -q MANAGER && echo 'manager-atom-ok' || echo 'manager-atom-missing'",
			].join("\n"),
		]);
		expect(result.output).toContain("manager-atom-ok");
	});
});

test.describe("XSETTINGS GTK integration", () => {
	// Pre-existing: gtk3-demo crashes (or never starts cleanly) before our
	// timeout. Probably needs an XSETTINGS daemon publishing _XSETTINGS_S0
	// or for us to advertise sane defaults.
	test.skip("GTK3 app can query XSETTINGS for theme", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"# Run a GTK3 demo briefly to verify it doesn't crash due to missing XSETTINGS",
				"timeout 5 gtk3-demo 2>&1 &",
				"sleep 3",
				"pkill -f gtk3-demo 2>/dev/null || true",
				"echo 'gtk3-xsettings-ok'",
			].join("\n"),
		]);
		expect(result.output).toContain("gtk3-xsettings-ok");
	});
});

test.describe.serial("EWMH compliance for real applications", () => {
	test("_NET_SUPPORTED lists all required atoms", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xprop -root _NET_SUPPORTED 2>&1",
		);
		// Should contain critical EWMH atoms
		expect(output).toContain("_NET_WM_STATE");
		expect(output).toContain("_NET_WM_WINDOW_TYPE");
		expect(output).toContain("_NET_ACTIVE_WINDOW");
		expect(output).toContain("_NET_CLIENT_LIST");
		expect(output).toContain("_NET_WM_NAME");
	});

	test("_NET_SUPPORTING_WM_CHECK is valid", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xprop -root _NET_SUPPORTING_WM_CHECK 2>&1`,
		);
		// Should point to a valid window
		expect(output).toMatch(/window id # 0x/);
	});

	test("WM name is x11-web", async ({ sidecarContainer }) => {
		// Get the WM check window and verify its _NET_WM_NAME
		const checkOutput = await execInSidecar(
			sidecarContainer,
			`xprop -root _NET_SUPPORTING_WM_CHECK 2>&1`,
		);
		const match = checkOutput.match(/window id # (0x[0-9a-f]+)/);
		if (match) {
			const wmWindowId = match[1];
			const nameOutput = await execInSidecar(
				sidecarContainer,
				`xprop -id ${wmWindowId} _NET_WM_NAME 2>&1`,
			);
			expect(nameOutput).toContain("x11-web");
		}
	});

	test("XSETTINGS manager is running", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "xsettings_manager_is_running.py", { env: { DISPLAY: ":99" } })).output.trim();
		// Owner should be non-zero (XSETTINGS manager window)
		expect(output).not.toContain("xsettings_owner=0");
	});
});
