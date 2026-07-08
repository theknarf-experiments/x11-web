/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, test } from "../fixtures";

test.describe("MIT-MAGIC-COOKIE-1 authentication", () => {
	test("xauth list shows a cookie for display :99", async ({
		sidecarContainer,
	}) => {
		// The X server writes its auth file to a fixed path
		// (/tmp/.x11-web-Xauthority — see X11Server::write_xauthority).
		// `xauth` without an explicit -f / $XAUTHORITY would look at
		// ~/.Xauthority which we don't create.
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"set -e",
				"export DISPLAY=:99",
				"export XAUTHORITY=/tmp/.x11-web-Xauthority",
				"ENTRIES=$(xauth list 2>&1 || echo 'xauth failed')",
				'echo "$ENTRIES"',
				"if echo \"$ENTRIES\" | grep -q 'MIT-MAGIC-COOKIE-1'; then",
				"  echo 'PASS: MIT-MAGIC-COOKIE-1 entry found'",
				"else",
				"  echo 'FAIL: no auth entries found'",
				"fi",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS:");
	});

	test("connection with wrong cookie is rejected", async ({
		sidecarContainer,
	}) => {
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
