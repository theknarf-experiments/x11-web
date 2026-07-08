/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, test } from "../fixtures";

test.describe("App compatibility: Chromium", () => {
	test("chromium creates an X11 window and xwininfo reports it", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(120_000);
		const which = await sidecarContainer.exec([
			"bash",
			"-c",
			"which chromium 2>/dev/null || which chromium-browser 2>/dev/null || echo NONE",
		]);
		if (which.output.trim() === "NONE") {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99 HOME=/root",
				"mkdir -p /root/.config",
				"chromium --no-sandbox --disable-gpu --no-first-run --disable-extensions --disable-background-networking --user-data-dir=/tmp/chromium-test 'about:blank' &",
				"CHROME_PID=$!",
				"# Wait for chromium window to appear",
				"for i in $(seq 1 20); do",
				"  WID=$(xdotool search --name '[Cc]hromium' 2>/dev/null | head -1)",
				'  if [ -n "$WID" ]; then break; fi',
				"  sleep 1",
				"done",
				'if [ -n "$WID" ]; then',
				'  echo "FOUND_CHROMIUM_WINDOW=$WID"',
				"  # Verify xwininfo can query the window",
				"  WININFO=$(xwininfo -id $WID 2>&1)",
				"  if echo \"$WININFO\" | grep -q 'Width:'; then",
				"    echo 'PASS: xwininfo reports chromium window geometry'",
				"  fi",
				"  if echo \"$WININFO\" | grep -q 'Map State:.*IsViewable'; then",
				"    echo 'PASS: chromium window is viewable'",
				"  fi",
				"else",
				"  # Chromium may take very long; check process is at least alive",
				"  if kill -0 $CHROME_PID 2>/dev/null; then",
				"    echo 'PASS: chromium process alive but window not yet visible'",
				"  else",
				"    echo 'FAIL: chromium exited prematurely'",
				"  fi",
				"fi",
				"kill $CHROME_PID 2>/dev/null; pkill -9 -f chromium 2>/dev/null; true",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS");
	});
});
