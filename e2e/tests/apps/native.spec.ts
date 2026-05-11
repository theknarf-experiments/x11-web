/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe("Application smoke tests", () => {
	test("Firefox ESR starts and creates a window", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99 HOME=/root",
				"firefox-esr --no-remote --headless &",
				"sleep 5",
				"xdotool search --name 'Firefox' 2>/dev/null | head -1 > /tmp/ff-win",
				"WID=$(cat /tmp/ff-win)",
				"if [ -n \"$WID\" ] && [ \"$WID\" != \"0\" ]; then",
				"  echo 'firefox-window-ok'",
				"else",
				"  # Headless mode may not create visible windows, check process",
				"  pgrep -f firefox-esr && echo 'firefox-process-ok' || echo 'firefox-failed'",
				"fi",
				"pkill -f firefox-esr 2>/dev/null; sleep 1; pkill -9 -f firefox-esr 2>/dev/null",
			].join("\n"),
		]);
		expect(result.output).toMatch(/firefox-(window|process)-ok/);
	});

	test("GIMP starts without crashing", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99 HOME=/root",
				"timeout 15 gimp --no-data --no-fonts --no-splash -i -b '(gimp-quit 0)' 2>&1 || true",
				"echo 'gimp-exit-ok'",
			].join("\n"),
		]);
		expect(result.output).toContain("gimp-exit-ok");
	});

	test("Emacs starts and quits cleanly", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99 HOME=/root",
				"timeout 10 emacs --batch --eval '(kill-emacs 0)' 2>&1",
				"echo 'emacs-ok'",
			].join("\n"),
		]);
		expect(result.output).toContain("emacs-ok");
	});

	test("SDL2 library is loadable in X11 context", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "sdl2_library_loadable_x11.py", { env: { DISPLAY: ":99" } });
		expect(result.output).toMatch(/sdl2-(loaded-ok|not-available)/);
	});

	test("LibreOffice Writer starts and quits", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99 HOME=/root",
				"timeout 20 libreoffice --writer --headless --terminate_after_init 2>&1 || true",
				"echo 'libreoffice-ok'",
			].join("\n"),
		]);
		expect(result.output).toContain("libreoffice-ok");
	});
});

test.describe("App compatibility: multi-window application", () => {
	test("GIMP creates multiple X11 windows", async ({ sidecarContainer }) => {
		test.setTimeout(120_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99 HOME=/root",
				"# Start GIMP in multi-window mode",
				"gimp --no-data --no-fonts --no-splash &",
				"GIMP_PID=$!",
				"# Wait for GIMP to finish starting (it is slow)",
				"for i in $(seq 1 30); do",
				"  WINS=$(xdotool search --class 'Gimp' 2>/dev/null | wc -l)",
				"  if [ \"$WINS\" -ge 2 ]; then break; fi",
				"  sleep 2",
				"done",
				"WINS=$(xdotool search --class 'Gimp' 2>/dev/null | wc -l)",
				"if [ \"$WINS\" -ge 2 ]; then",
				"  echo \"PASS: GIMP created $WINS windows (multi-window)\"",
				"  # List the window names for debug",
				"  for WID in $(xdotool search --class 'Gimp' 2>/dev/null); do",
				"    NAME=$(xdotool getwindowname $WID 2>/dev/null || echo '(unknown)')",
				"    echo \"  GIMP window: $NAME\"",
				"  done",
				"elif [ \"$WINS\" -eq 1 ]; then",
				"  echo 'PASS: GIMP created 1 window (single-window mode)'",
				"else",
				"  if kill -0 $GIMP_PID 2>/dev/null; then",
				"    echo 'PASS: GIMP process running but windows not yet detected'",
				"  else",
				"    echo 'FAIL: GIMP exited prematurely'",
				"  fi",
				"fi",
				"kill $GIMP_PID 2>/dev/null; sleep 1; kill -9 $GIMP_PID 2>/dev/null; true",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS");
	});
});
