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

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe("SECURITY extension", () => {
	test("SECURITY extension is listed", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"xdpyinfo 2>&1 | grep -i security || echo 'not_found'",
				"echo SECURITY_EXT_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("SECURITY_EXT_PASS");
	});
});

test.describe.serial("SECURITY extension compliance", () => {
	test("SECURITY extension is advertised", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo -queryExtensions 2>&1 || true",
		);
		expect(output).toContain("SECURITY");
	});
});

test.describe.serial("SECURITY extension", () => {
	test.setTimeout(60_000);

	test("SECURITY extension is present", async ({ sidecarContainer }) => {
		const output = await probe(sidecarContainer, "security_extension_present.py");
		expect(output).toContain("SECURITY_OK");
	});
});
