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

test.describe.serial("XIM protocol compliance", () => {
	test("XIM server window exists on display", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "xim_server_window_exists_on_display.py", { env: { DISPLAY: ":99" } })).output.trim();
		// XIM server should be advertised
		expect(output).toContain("xim_server_found=True");
	});

	test("xterm launches without XIM errors", async ({
		sidecarContainer,
	}) => {
		// Launch xterm briefly and verify it doesn't crash from XIM issues
		const output = await execInSidecar(
			sidecarContainer,
			`timeout 3 xterm -e "echo xterm_started && sleep 1" 2>&1; echo "exit_code=$?"`,
		);
		// Should not see "Cannot open input method" or similar errors
		expect(output).not.toContain("Cannot open input method");
	});
});

test.describe("XIM protocol", () => {
	test("XIM_SERVERS property is set on root window", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xprop -root 2>&1 | grep -i 'XIM_SERVERS' && echo 'xim-servers-ok' || echo 'xim-servers-missing'",
			].join("\n"),
		]);
		expect(result.output).toContain("xim-servers-ok");
	});

	test("XIM server window exists and has LOCALES property", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xim_server_window_locales.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("xim-server-found");
	});
});

test.describe("XIM input method protocol", () => {
	test("XIM server is reachable and accepts connections", async ({ sidecarContainer }) => {
		const result = await runPythonScript(sidecarContainer, "xim_server_reachable.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("PASS:");
	});
});
