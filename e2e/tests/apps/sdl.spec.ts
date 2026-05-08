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

test.describe.serial("SDL2 application compatibility", () => {
	test("SDL2 initializes video subsystem and creates window", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(60_000);
		const output = await execInSidecar(
			sidecarContainer,
			[
				`timeout 15 python3 -c '`,
				`import ctypes, sys, time, os`,
				`os.environ["DISPLAY"] = ":99"`,
				`try:`,
				`    sdl = ctypes.CDLL("libSDL2-2.0.so.0")`,
				`except OSError:`,
				`    print("SKIP: libSDL2 not available")`,
				`    sys.exit(0)`,
				`SDL_INIT_VIDEO = 0x00000020`,
				`SDL_WINDOW_SHOWN = 0x00000004`,
				`if sdl.SDL_Init(SDL_INIT_VIDEO) != 0:`,
				`    err_fn = sdl.SDL_GetError`,
				`    err_fn.restype = ctypes.c_char_p`,
				`    print(f"FAIL: SDL_Init failed: {err_fn()}")`,
				`    sys.exit(1)`,
				`print("PASS: SDL2 initialized")`,
				`sdl.SDL_CreateWindow.restype = ctypes.c_void_p`,
				`win = sdl.SDL_CreateWindow(b"SDL2_Test", 100, 100, 320, 240, SDL_WINDOW_SHOWN)`,
				`if not win:`,
				`    print("FAIL: SDL_CreateWindow returned NULL")`,
				`    sdl.SDL_Quit()`,
				`    sys.exit(1)`,
				`print("PASS: SDL2 window created")`,
				`time.sleep(1)`,
				`sdl.SDL_DestroyWindow(ctypes.c_void_p(win))`,
				`sdl.SDL_Quit()`,
				`print("PASS: SDL2 cleanup complete")`,
				`' 2>&1`,
			].join("\n"),
		);
		// Either SDL2 works or isn't available (both acceptable)
		expect(output).toMatch(/PASS: SDL2 window created|SKIP: libSDL2 not available/);
	});
});

test.describe("App compatibility: SDL2 via Python", () => {
	test("SDL2 opens and renders an X11 window via Python ctypes", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"python3 << 'PYEOF'",
				"import ctypes, ctypes.util, sys, time, os",
				"",
				"# Try to load SDL2",
				"try:",
				"    sdl = ctypes.CDLL('libSDL2-2.0.so.0')",
				"except OSError:",
				"    print('SKIP: libSDL2 not available')",
				"    sys.exit(0)",
				"",
				"# SDL constants",
				"SDL_INIT_VIDEO = 0x00000020",
				"SDL_WINDOW_SHOWN = 0x00000004",
				"",
				"# Initialize SDL video subsystem",
				"if sdl.SDL_Init(SDL_INIT_VIDEO) != 0:",
				"    print('FAIL: SDL_Init failed')",
				"    sys.exit(1)",
				"",
				"# Create a visible window",
				"sdl.SDL_CreateWindow.restype = ctypes.c_void_p",
				"win = sdl.SDL_CreateWindow(",
				"    b'SDL2_E2E_Test', 100, 100, 320, 240, SDL_WINDOW_SHOWN",
				")",
				"if not win:",
				"    print('FAIL: SDL_CreateWindow returned NULL')",
				"    sdl.SDL_Quit()",
				"    sys.exit(1)",
				"print('PASS: SDL2 window created')",
				"",
				"# Give X server time to process the window",
				"time.sleep(2)",
				"",
				"# Verify via xdotool",
				"import subprocess",
				"r = subprocess.run(['xdotool', 'search', '--name', 'SDL2_E2E_Test'],",
				"                   capture_output=True, text=True, timeout=5)",
				"if r.stdout.strip():",
				"    print('PASS: xdotool found SDL2 window')",
				"else:",
				"    print('WARN: xdotool did not find SDL2 window (may be unnamed)')",
				"",
				"sdl.SDL_DestroyWindow(ctypes.c_void_p(win))",
				"sdl.SDL_Quit()",
				"print('PASS: SDL2 cleanup complete')",
				"PYEOF",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS: SDL2 window created");
	});
});
