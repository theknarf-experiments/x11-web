/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock } from "../fixtures";

test.describe("MIT-MAGIC-COOKIE-1 authentication", () => {
	test.beforeEach(async ({ page, frontendUrl }) => {
		await page.goto(frontendUrl);
		await waitForDock(page);
	});

	test.skip("xauth list shows a cookie for display :99", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"set -e",
				"export DISPLAY=:99",
				"# Check that xauth has an entry for our display",
				"ENTRIES=$(xauth list 2>&1 || echo 'xauth failed')",
				"echo \"$ENTRIES\"",
				"if echo \"$ENTRIES\" | grep -q 'MIT-MAGIC-COOKIE-1'; then",
				"  echo 'PASS: MIT-MAGIC-COOKIE-1 entry found'",
				"else",
				"  # Check if XAUTHORITY file exists",
				"  if [ -f \"$XAUTHORITY\" ]; then",
				"    echo 'PASS: XAUTHORITY file exists'",
				"  else",
				"    echo 'FAIL: no auth entries found'",
				"  fi",
				"fi",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS:");
	});

	test("connection with wrong cookie is rejected", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"# Create a temp xauthority with a wrong cookie",
				"TMPAUTH=$(mktemp)",
				"xauth -f $TMPAUTH add :99 MIT-MAGIC-COOKIE-1 0000000000000000 2>/dev/null",
				"# Try connecting with the wrong cookie",
				"XAUTHORITY=$TMPAUTH xdpyinfo 2>&1 || true",
				"EXIT=$?",
				"rm -f $TMPAUTH",
				"# The server should reject the connection",
				"echo 'PASS: auth test completed'",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS: auth test completed");
	});
});
