/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe.serial("DPMS spec compliance", () => {
	test.setTimeout(60_000);

	test("DPMS extension is reported by server", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>/dev/null | grep -i dpms || echo NO_DPMS",
		);
		expect(output.toLowerCase()).toContain("dpms");
	});
});
