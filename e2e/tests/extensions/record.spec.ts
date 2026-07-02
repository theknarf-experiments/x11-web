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
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
	return result.output.trim();
}

test.describe
	.serial("RECORD extension compliance", () => {
		test("RECORD extension is present", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				`xdpyinfo 2>/dev/null | grep RECORD`,
			);
			expect(output).toContain("RECORD");
		});

		test("RECORD context create and free", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"record_context_create_and_free.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("record_present=true");
		});
	});

test.describe("RECORD extension", () => {
	test("RECORD extension is advertised", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"xdpyinfo",
			"-queryExtensions",
		]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("RECORD");
	});
});

test.describe("RECORD cross-client interception", () => {
	test("RECORD CreateContext and EnableContext work", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"record_createcontext_enablecontext.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`RECORD cross-client: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});
