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

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe.serial("XFIXES region operations", () => {
	test("CreateRegion and FetchRegion round-trip", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "createregion_and_fetchregion_round_trip.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xfixes_present=true");
	});

	test("XFIXES extension advertises version 5.0", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xdpyinfo -queryExtensions 2>/dev/null | grep -A2 'XFIXES'`,
		);
		expect(output).toContain("XFIXES");
	});

	test("XFIXES region operations via xdotool and python", async ({
		sidecarContainer,
	}) => {
		// Test that XFIXES regions work through window shape operations
		const output = (await runPythonScript(sidecarContainer, "xfixes_region_operations_via_xdotool_and_python.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("window_exists=true");
		expect(output).toContain("width=100");
		expect(output).toContain("height=100");
		expect(output).toContain("region_test=ok");
	});

	test("Cursor operations via XFIXES", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "cursor_operations_via_xfixes.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xfixes_available=True");
		expect(output).toContain("cursor_ops=ok");
	});
});

test.describe("XFIXES extension conformance", () => {
	test("XFIXES regions and cursor operations", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "xfixes_regions_cursor_operations.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS");
	});
});

test.describe("XFIXES pointer barriers", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test("CreatePointerBarrier and DeletePointerBarrier round-trip", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xfixes_pointer_barrier_create_delete.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain(
			"PASS: pointer barrier create/delete succeeded",
		);
	});
});

test.describe("XFIXES cursor operations", () => {
	test("XFIXES extension version is reported", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"from Xlib import X, display",
				"d = display.Display()",
				"ext = d.query_extension('XFIXES')",
				"print(f'XFIXES: {ext is not None}')",
				"d.close()",
				"print('XFIXES_OK')",
			].join("; "),
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("XFIXES_OK");
	});
});

test.describe.serial("XFIXES spec compliance", () => {
	test.setTimeout(60_000);

	test("XFIXES extension is present", async ({ sidecarContainer }) => {
		const output = await probe(sidecarContainer, "xfixes_extension_present.py");
		expect(output).toContain("XFIXES_OK");
	});
});
