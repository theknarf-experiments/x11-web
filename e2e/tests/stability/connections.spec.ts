/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe("XCB protocol round-trip", () => {
	test("window lifecycle round-trip", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await runPythonScript(sidecarContainer, "xcb_window_lifecycle_roundtrip.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("xcb-lifecycle-ok");
	});

	test("multi-client concurrent connections", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await runPythonScript(sidecarContainer, "xcb_multi_client_concurrent_connections.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("multi-client-ok");
	});

	test("protocol error responses are correct", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await runPythonScript(sidecarContainer, "xcb_protocol_error_responses.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toContain("protocol-errors: pass=2 fail=0");
	});
});
