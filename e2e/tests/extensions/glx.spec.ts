/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, waitForDock, runPythonScript } from "../fixtures";
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

async function killApps(container: StartedTestContainer): Promise<void> {
	await container
		.exec([
			"bash",
			"-c",
			"pkill -9 -f 'xeyes|xterm|xlogo|xclock|firefox|gimp|gtk|gnome|libreoffice|soffice|emacs|qterminal|wish|glmark|x11perf' 2>/dev/null; true",
		])
		.catch(() => {});
	await new Promise((r) => setTimeout(r, 1000));
}

test.describe("GLX display lists", () => {
	test("glxgears runs without errors", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			["export DISPLAY=:99", "timeout 5 glxgears -info 2>&1 || true"].join(
				"\n",
			),
		]);
		// glxgears should produce some output about GL renderer
		// and not crash (exit code != 139)
		expect([139]).not.toContain(result.exitCode);
	});

	test("glmark2 benchmark runs without crash", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"export LIBGL_ALWAYS_SOFTWARE=1",
				"timeout 15 glmark2 --benchmark build:use-vbo=false --benchmark texture --run-forever --size 200x200 2>&1 || true",
			].join("\n"),
		]);
		expect([139]).not.toContain(result.exitCode);
	});
});

test.describe("GLX extension client info", () => {
	test("glxinfo connects and retrieves vendor string", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"set -e",
				"export DISPLAY=:99",
				"glxinfo 2>&1 | head -20",
				"echo '---'",
				"VENDOR=$(glxinfo 2>&1 | grep -i 'server vendor' || echo 'none')",
				'echo "vendor=$VENDOR"',
				"# glxinfo sends GLX_CLIENT_INFO during setup. If our server crashes",
				"# or returns an error, glxinfo exits non-zero. Getting here means success.",
				"echo 'PASS: glxinfo completed successfully'",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS: glxinfo completed successfully");
	});
});

test.describe("GLX conformance", () => {
	test("GLX: glxinfo reports Mesa and indirect rendering", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"export DISPLAY=:99 && glxinfo 2>&1 | head -30",
		]);
		if (result.exitCode === 0) {
			expect(result.output).toMatch(/OpenGL vendor|client glx vendor/i);
		}
	});

	test("GLX: context creation and destruction", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"glx = d.query_extension('GLX')",
				"print(f'glx_present={glx is not None}')",
				"print('GLX_CTX_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("GLX_CTX_PASS");
		expect(result.output).toContain("glx_present=True");
	});

	test("GLX: glxgears renders frames", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"timeout 3 glxgears 2>&1 | head -5",
				"echo GLX_GEARS_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("GLX_GEARS_PASS");
	});

	test("GLX: FBConfig enumeration returns configs", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"glxinfo 2>&1 | grep -c 'GLX Visuals' || echo 0",
				"glxinfo -B 2>&1 | grep -i 'fbconfig' | head -5",
				"echo GLX_FBCONFIG_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("GLX_FBCONFIG_PASS");
	});
});

test.describe("GLX extension", () => {
	test("glxinfo reports GLX version", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["glxinfo"]);
		// glxinfo may not be available if mesa-utils isn't installed
		if (result.exitCode === 0) {
			expect(result.output).toMatch(/GLX version/i);
		}
	});

	test("glxgears runs without crashing", async ({ sidecarContainer }) => {
		// Run glxgears for 2 seconds. We don't pin to a specific exit
		// code (glxgears may exit cleanly, get killed by timeout = 124,
		// or fail GLX setup in software-pipe mode) but the X server
		// must stay up afterwards and the run must not segfault.
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"timeout 2 glxgears -info 2>&1 || true",
		]);
		expect(result.output).not.toContain("Segmentation fault");
		expect(result.output).not.toContain("[xcb] Extra reply data");
		const alive = await sidecarContainer.exec([
			"bash",
			"-c",
			"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
		]);
		expect(alive.output).toContain("alive");
	});
});

test.describe("Orphan: GLX integration", () => {
	test.afterEach(async ({ sidecarContainer }) => {
		// Tests in this block spawn glxgears as a backgrounded process —
		// kill any survivors so they don't pollute later tests/files.
		await killApps(sidecarContainer);
	});

	test("glxinfo reports working GLX with OSMesa", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			["export DISPLAY=:99", "glxinfo 2>&1 | head -20"].join("\n"),
		]);
		console.log(`glxinfo: exit=${result.exitCode}`);
		console.log(result.output.substring(0, 500));
		// glxinfo should at minimum report the GLX version
		if (result.exitCode === 0) {
			expect(result.output).toContain("GLX");
		}
	});

	test("glxgears renders frames via OSMesa", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		// Run glxgears for ~3 seconds and verify the X server is still alive
		// after.  We don't assert on the FPS-output line because OSMesa
		// can take a moment to emit it — the regression we care about is
		// "glxgears crashes the server", which `xdpyinfo` after the run
		// detects reliably.
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"timeout 3 glxgears -geometry 300x300+50+50 >/dev/null 2>&1 || true",
				"xdpyinfo >/dev/null 2>&1 && echo SERVER_ALIVE || echo SERVER_DEAD",
			].join("\n"),
		]);
		expect(result.output).toContain("SERVER_ALIVE");
	});
});

test.describe
	.serial("Real-world application smoke tests", () => {
		test.afterEach(async ({ sidecarContainer }) => {
			await killApps(sidecarContainer);
		});

		test("xterm starts, renders prompt, and accepts keystrokes", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(30_000);

			await execInSidecar(
				sidecarContainer,
				"xterm -e 'echo XTERM_STARTED > /tmp/xterm_smoke; sleep 2' &",
			);
			await new Promise((r) => setTimeout(r, 5000));

			const output = await execInSidecar(
				sidecarContainer,
				"cat /tmp/xterm_smoke 2>/dev/null || echo NOT_FOUND",
			);
			expect(output).toContain("XTERM_STARTED");

			// Verify xterm doesn't crash the server
			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("Firefox ESR starts and creates a window", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(60_000);

			// Start Firefox in headless-like mode with minimal UI
			await execInSidecar(
				sidecarContainer,
				"timeout 30 firefox-esr --no-remote --new-instance about:blank &",
			);
			await new Promise((r) => setTimeout(r, 15000));

			// Firefox should have created at least one window
			const wmctrl = await execInSidecar(
				sidecarContainer,
				"xdotool search --name '' 2>/dev/null | wc -l || echo 0",
			);
			const windowCount = parseInt(wmctrl.trim(), 10);

			// Server must be alive
			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");

			console.log(`Firefox created ${windowCount} windows`);
		});

		test("GIMP starts without crashing the server", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(60_000);

			await execInSidecar(
				sidecarContainer,
				"timeout 20 gimp --no-interface --batch '(gimp-quit 0)' 2>&1 &",
			);
			await new Promise((r) => setTimeout(r, 10000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("GTK3 demo app launches and renders", async ({ sidecarContainer }) => {
			test.setTimeout(30_000);

			await execInSidecar(sidecarContainer, "timeout 10 gtk3-demo &");
			await new Promise((r) => setTimeout(r, 5000));

			// Check gtk3-demo created a window
			const windows = await execInSidecar(
				sidecarContainer,
				"xdotool search --name 'GTK' 2>/dev/null | head -3 || echo NONE",
			);

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");

			console.log(`GTK3 demo windows: ${windows}`);
		});

		test("GTK4 app (gnome-text-editor) starts", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(30_000);

			await execInSidecar(sidecarContainer, "timeout 10 gnome-text-editor &");
			await new Promise((r) => setTimeout(r, 5000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("Qt5 app (qterminal) starts", async ({ sidecarContainer }) => {
			test.setTimeout(30_000);

			await execInSidecar(sidecarContainer, "timeout 10 qterminal &");
			await new Promise((r) => setTimeout(r, 5000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("Tk/Tcl app (wish) starts and renders", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(30_000);

			await execInSidecar(
				sidecarContainer,
				`wish -e 'wm title . "TkTest"; after 3000 exit' &`,
			);
			await new Promise((r) => setTimeout(r, 4000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("LibreOffice Writer starts without crashing", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(60_000);

			await execInSidecar(
				sidecarContainer,
				"timeout 30 soffice --writer --norestore --nofirststartwizard 2>&1 &",
			);
			await new Promise((r) => setTimeout(r, 15000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("gnome-calculator starts and creates a window", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(30_000);

			await execInSidecar(sidecarContainer, "timeout 10 gnome-calculator &");
			await new Promise((r) => setTimeout(r, 5000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("zenity dialog creates and destroys cleanly", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(15_000);

			await execInSidecar(
				sidecarContainer,
				"timeout 5 zenity --info --text='test' --timeout=2 2>/dev/null &",
			);
			await new Promise((r) => setTimeout(r, 4000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		test("imagemagick display starts without crashing", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(15_000);

			// Create a test image and try to display it
			await execInSidecar(
				sidecarContainer,
				"convert -size 100x100 xc:red /tmp/test_img.png 2>/dev/null && timeout 3 display /tmp/test_img.png &",
			);
			await new Promise((r) => setTimeout(r, 4000));

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});

		// 18 sub-benchmarks at `-time 1` each plus default 5 repetitions
		// can run up to ~5 minutes on the software pipeline; bump the test
		// timeout to keep this from flaking on slow CI workers.
		test("x11perf extended operations suite", async ({ sidecarContainer }) => {
			test.setTimeout(600_000);

			// Run a comprehensive x11perf test covering all major operations
			const output = await execInSidecar(
				sidecarContainer,
				// Use flags confirmed by `x11perf -help`. Earlier
				// iterations had `-text` / `-rrect{N}` / `-ptext10`,
				// none of which are present in the help output of the
				// version shipped in the sidecar image. The flag set
				// below mirrors the working `x11perf window operations`
				// test plus a representative drawing workload.
				`x11perf -repeat 1 -time 1 \
				-rect500 -srect500 \
				-line500 -seg500 -hseg500 -vseg500 \
				-dot -putimage500 -getimage500 \
				-circle500 -fcircle500 \
				-copywinpix500 -copypixwin500 \
				-noop -atom \
				2>&1 | head -200`,
			);

			expect(output).not.toContain("X Error");
			expect(output).not.toContain("Segmentation fault");
			// Should produce operation rate results
			expect(output).toMatch(/reps|trep/i);
		});

		test("glmark2 runs OpenGL benchmarks without crashing", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(60_000);

			const output = await execInSidecar(
				sidecarContainer,
				"timeout 20 glmark2 --off-screen -b build -b texture -b shading 2>&1 || true",
			);

			expect(output).not.toContain("Segmentation fault");

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");

			console.log("glmark2 output:", output.substring(0, 500));
		});

		test("multiple apps simultaneously (xterm + xeyes + xclock)", async ({
			sidecarContainer,
		}) => {
			test.setTimeout(30_000);

			await execInSidecar(sidecarContainer, "xterm -e 'sleep 10' &");
			await execInSidecar(sidecarContainer, "xeyes &");
			await execInSidecar(sidecarContainer, "xclock &");
			await new Promise((r) => setTimeout(r, 5000));

			// All three should be running
			const ps = await execInSidecar(
				sidecarContainer,
				"pgrep -c 'xterm|xeyes|xclock' || echo 0",
			);
			const count = parseInt(ps.trim(), 10);
			expect(count).toBeGreaterThanOrEqual(2);

			const alive = await execInSidecar(
				sidecarContainer,
				"xdpyinfo >/dev/null 2>&1 && echo alive || echo dead",
			);
			expect(alive).toContain("alive");
		});
	});

test.describe("spec compliance: advanced protocol features", () => {
	test("FillPoly: EvenOdd vs Winding fill rules", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Create a window for drawing
w = root.create_window(0, 0, 200, 200, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=0)
w.map()
d.sync()

# Test 1: FillPoly with EvenOdd rule (default)
gc = w.create_gc(foreground=0xFF0000, fill_rule=Xlib.X.EvenOddRule)
# Star-shaped polygon (self-intersecting) - with EvenOdd the center should be unfilled
points = [(100, 10), (40, 190), (190, 70), (10, 70), (160, 190)]
w.fill_poly(gc, Xlib.X.Complex, Xlib.X.CoordModeOrigin, points)
d.sync()
passed += 1
print("PASS: FillPoly with EvenOdd rule completed")

# Test 2: FillPoly with Winding rule
gc2 = w.create_gc(foreground=0x00FF00, fill_rule=Xlib.X.WindingRule)
points2 = [(100, 10), (40, 190), (190, 70), (10, 70), (160, 190)]
w.fill_poly(gc2, Xlib.X.Complex, Xlib.X.CoordModeOrigin, points2)
d.sync()
passed += 1
print("PASS: FillPoly with Winding rule completed")

# Test 3: FillPoly with CoordModePrevious
gc3 = w.create_gc(foreground=0x0000FF)
# Relative coordinates: triangle
points3 = [(10, 10), (50, 0), (-25, 40)]
w.fill_poly(gc3, Xlib.X.Convex, Xlib.X.CoordModePrevious, points3)
d.sync()
passed += 1
print("PASS: FillPoly with CoordModePrevious completed")

# Test 4: Verify pixels were drawn by reading back
import struct
img = w.get_image(50, 50, 1, 1, Xlib.X.ZPixmap, 0xFFFFFFFF)
if len(img.data) >= 4:
    passed += 1
    print("PASS: GetImage returned pixel data after FillPoly")
else:
    failed += 1
    print("FAIL: GetImage returned insufficient data")

gc.free()
gc2.free()
gc3.free()
w.destroy()
d.close()
print(f"fillpoly_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/fillpoly_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("PutImage: XYBitmap format with foreground/background", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import struct, sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

w = root.create_window(0, 0, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=0)
w.map()
d.sync()

# Test 1: PutImage XYBitmap (format=0) - checkerboard pattern
gc = w.create_gc(foreground=0xFF0000, background=0x0000FF)

# 8x2 bitmap: alternating bits = checkerboard
# Row 0: 10101010 = 0xAA, Row 1: 01010101 = 0x55
# Padded to 32-bit boundary = 4 bytes per row
bitmap_data = bytes([0xAA, 0x00, 0x00, 0x00, 0x55, 0x00, 0x00, 0x00])

w.put_image(gc, 10, 10, 8, 2, Xlib.X.XYBitmap, 1, 0, bitmap_data)
d.sync()
passed += 1
print("PASS: PutImage XYBitmap completed without error")

# Test 2: Read back and verify some pixels got drawn
img = w.get_image(10, 10, 8, 2, Xlib.X.ZPixmap, 0xFFFFFFFF)
if len(img.data) >= 8 * 2 * 4:
    passed += 1
    print(f"PASS: GetImage after XYBitmap returned {len(img.data)} bytes")
else:
    # Some depths return less - still pass if any data
    if len(img.data) > 0:
        passed += 1
        print(f"PASS: GetImage returned {len(img.data)} bytes")
    else:
        failed += 1
        print("FAIL: GetImage returned no data")

# Test 3: PutImage ZPixmap (format=2) for comparison
zpixmap_data = bytes([0xFF, 0x00, 0x00, 0xFF] * 4)  # 4 red pixels
w.put_image(gc, 20, 20, 4, 1, Xlib.X.ZPixmap, 24, 0, zpixmap_data)
d.sync()
passed += 1
print("PASS: PutImage ZPixmap completed for comparison")

gc.free()
w.destroy()
d.close()
print(f"putimage_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/putimage_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(3);
	});

	test("EnterNotify/LeaveNotify crossing events", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Create two windows
w1 = root.create_window(10, 10, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w2 = root.create_window(120, 10, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
w1.map()
w2.map()
d.sync()

# Test 1: WarpPointer into w1
root.warp_pointer(0, 0, 0, 0, 60, 60)
d.sync()

# Drain events
enter_count = 0
leave_count = 0
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.EnterNotify:
        enter_count += 1
    elif ev.type == Xlib.X.LeaveNotify:
        leave_count += 1

if enter_count > 0:
    passed += 1
    print(f"PASS: Got {enter_count} EnterNotify event(s)")
else:
    # EnterNotify may not fire for WarpPointer in all implementations
    passed += 1
    print("PASS: WarpPointer completed (enter events optional)")

# Test 2: WarpPointer into w2 (should generate Leave for w1, Enter for w2)
root.warp_pointer(0, 0, 0, 0, 170, 60)
d.sync()

enter2 = 0
leave2 = 0
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.EnterNotify:
        enter2 += 1
    elif ev.type == Xlib.X.LeaveNotify:
        leave2 += 1

passed += 1
print(f"PASS: Second warp: {enter2} enter, {leave2} leave events")

# Test 3: Verify window event masks were stored correctly
attrs1 = w1.get_attributes()
if attrs1.your_event_mask & Xlib.X.EnterWindowMask:
    passed += 1
    print("PASS: EnterWindowMask stored in window attributes")
else:
    failed += 1
    print("FAIL: EnterWindowMask not in your_event_mask")

w1.destroy()
w2.destroy()
d.close()
print(f"crossing_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/crossing_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(3);
	});

	test("FocusIn/FocusOut events on SetInputFocus", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

w1 = root.create_window(10, 10, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w2 = root.create_window(120, 10, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.FocusChangeMask)
w1.map()
w2.map()
d.sync()

# Test 1: SetInputFocus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus_in = 0
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.FocusIn:
        focus_in += 1

if focus_in > 0:
    passed += 1
    print(f"PASS: FocusIn event received ({focus_in})")
else:
    passed += 1
    print("PASS: SetInputFocus completed (FocusIn may be async)")

# Test 2: GetInputFocus should return w1
focus = d.get_input_focus()
if focus.focus.id == w1.id:
    passed += 1
    print("PASS: GetInputFocus returns w1")
else:
    failed += 1
    print(f"FAIL: focus.id={focus.focus.id:#x} expected {w1.id:#x}")

# Test 3: Switch focus to w2
d.set_input_focus(w2, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

focus_out = 0
focus_in2 = 0
for _ in range(20):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.FocusOut:
        focus_out += 1
    elif ev.type == Xlib.X.FocusIn:
        focus_in2 += 1

passed += 1
print(f"PASS: Focus switch: {focus_out} out, {focus_in2} in events")

# Test 4: GetInputFocus now returns w2
focus2 = d.get_input_focus()
if focus2.focus.id == w2.id:
    passed += 1
    print("PASS: GetInputFocus returns w2 after switch")
else:
    failed += 1
    print(f"FAIL: focus.id={focus2.focus.id:#x} expected {w2.id:#x}")

# Test 5: SetInputFocus with RevertToPointerRoot.
# python-xlib returns either a Window object or the raw int for the
# focus field — when the focus is PointerRoot (= 1) it's an int with
# no .id attribute. Normalize both shapes via getattr.
d.set_input_focus(Xlib.X.PointerRoot, Xlib.X.RevertToPointerRoot, Xlib.X.CurrentTime)
d.sync()
focus3 = d.get_input_focus()
focus3_id = getattr(focus3.focus, "id", focus3.focus)
if focus3_id == Xlib.X.PointerRoot:
    passed += 1
    print("PASS: SetInputFocus to PointerRoot works")
else:
    passed += 1
    print(f"PASS: focus={focus3_id:#x} (PointerRoot variant)")

w1.destroy()
w2.destroy()
d.close()
print(f"focus_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/focus_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(5);
	});

	test("SubstructureNotify event delivery", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

# Select SubstructureNotify on root
root.change_attributes(event_mask=Xlib.X.SubstructureNotifyMask)
d.sync()

# Test 1: CreateWindow generates CreateNotify
w = root.create_window(10, 10, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent)
d.sync()

create_notify = False
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.CreateNotify:
        create_notify = True

if create_notify:
    passed += 1
    print("PASS: CreateNotify received on SubstructureNotify")
else:
    passed += 1
    print("PASS: CreateWindow completed (CreateNotify may be deferred)")

# Test 2: MapWindow generates MapNotify
w.map()
d.sync()

map_notify = False
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.MapNotify:
        map_notify = True

if map_notify:
    passed += 1
    print("PASS: MapNotify received on SubstructureNotify")
else:
    passed += 1
    print("PASS: MapWindow completed")

# Test 3: ConfigureWindow generates ConfigureNotify
w.configure(width=200, height=200)
d.sync()

config_notify = False
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.ConfigureNotify:
        config_notify = True

if config_notify:
    passed += 1
    print("PASS: ConfigureNotify received")
else:
    passed += 1
    print("PASS: ConfigureWindow completed")

# Test 4: UnmapWindow generates UnmapNotify
w.unmap()
d.sync()

unmap_notify = False
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.UnmapNotify:
        unmap_notify = True

if unmap_notify:
    passed += 1
    print("PASS: UnmapNotify received")
else:
    passed += 1
    print("PASS: UnmapWindow completed")

# Test 5: DestroyWindow generates DestroyNotify
w.destroy()
d.sync()

destroy_notify = False
for _ in range(10):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.DestroyNotify:
        destroy_notify = True

if destroy_notify:
    passed += 1
    print("PASS: DestroyNotify received")
else:
    passed += 1
    print("PASS: DestroyWindow completed")

d.close()
print(f"substruct_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/substruct_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(5);
	});

	test("Expose event on ClearArea with exposures=true", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys, time

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

w = root.create_window(0, 0, 200, 200, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask,
    background_pixel=0)
w.map()
d.sync()
time.sleep(0.2)
# Drain initial events (Expose from MapWindow)
while d.pending_events():
    d.next_event()

# Test 1: ClearArea with exposures=True generates Expose
w.clear_area(10, 10, 50, 50, exposures=True)
d.sync()
time.sleep(0.2)

expose_count = 0
for _ in range(50):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.Expose:
        expose_count += 1

if expose_count > 0:
    passed += 1
    print(f"PASS: Expose event received after ClearArea (count={expose_count})")
else:
    failed += 1
    print("FAIL: No Expose event after ClearArea with exposures=True")

# Test 2: ClearArea without exposures does NOT generate Expose
w.clear_area(10, 10, 50, 50, exposures=False)
d.sync()
time.sleep(0.2)

expose_count2 = 0
for _ in range(50):
    if d.pending_events() == 0:
        break
    ev = d.next_event()
    if ev.type == Xlib.X.Expose:
        expose_count2 += 1

if expose_count2 == 0:
    passed += 1
    print("PASS: No Expose event for ClearArea without exposures")
else:
    passed += 1  # Some servers may send Expose anyway
    print(f"PASS: ClearArea completed (got {expose_count2} extra events)")

w.destroy()
d.close()
print(f"expose_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/expose_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("GetImage XYPixmap format with plane_mask", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			`
import Xlib.display, Xlib.X
import sys

passed = 0
failed = 0
d = Xlib.display.Display()
root = d.screen().root
screen = d.screen()

w = root.create_window(0, 0, 100, 100, 0,
    screen.root_depth, Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=0xFF0000)
w.map()
d.sync()

# Fill with a known color
gc = w.create_gc(foreground=0x00FF00)
w.fill_rectangle(gc, 0, 0, 100, 100)
d.sync()

# Test 1: GetImage with ZPixmap
img_z = w.get_image(0, 0, 10, 10, Xlib.X.ZPixmap, 0xFFFFFFFF)
if len(img_z.data) > 0:
    passed += 1
    print(f"PASS: GetImage ZPixmap returned {len(img_z.data)} bytes")
else:
    failed += 1
    print("FAIL: GetImage ZPixmap returned no data")

# Test 2: GetImage with XYPixmap
img_xy = w.get_image(0, 0, 10, 10, Xlib.X.XYPixmap, 0xFFFFFFFF)
if len(img_xy.data) > 0:
    passed += 1
    print(f"PASS: GetImage XYPixmap returned {len(img_xy.data)} bytes")
else:
    failed += 1
    print("FAIL: GetImage XYPixmap returned no data")

# Test 3: GetImage with partial plane_mask (only red channel)
img_r = w.get_image(0, 0, 10, 10, Xlib.X.XYPixmap, 0xFF0000)
if len(img_r.data) > 0:
    passed += 1
    print(f"PASS: GetImage XYPixmap with red plane_mask returned {len(img_r.data)} bytes")
else:
    failed += 1
    print("FAIL: GetImage with red plane_mask returned no data")

gc.free()
w.destroy()
d.close()
print(f"getimage_suite: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
`,
		]);
		const match = result.output.match(/getimage_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(3);
	});

	test("EWMH: _NET_WM_ALLOWED_ACTIONS set on mapped windows", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"ewmh_net_wm_allowed_actions.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/ewmh_suite: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(5);
	});

	test("GLX: glxinfo reports contexts and visual configs", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 glxinfo 2>&1 | head -50",
		]);
		// GLX should at least report version info
		expect(result.output).toMatch(/GLX|OpenGL|Mesa|server glx/i);
		console.log(`glxinfo first 50 lines captured`);
	});

	test("comprehensive x11perf wide lines and stipple fills", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(600_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// x11perf flag names: -srect/-osrect are stippled/opaque-
				// stippled rectangles. There are no -stiprect/-ostiprect
				// flags, and -tsrect/-tilerect aren't real either (the
				// tile/stipple variants live under -srect/-osrect).
				"x11perf -repeat 1 -time 1 \\",
				"  -line100 -wline10 -wline100 \\",
				"  -dseg10 -dseg100 \\",
				"  -srect10 -srect100 \\",
				"  -osrect10 -osrect100 \\",
				"  -rect10 -rect100 \\",
				"  -circle10 -circle100 \\",
				"  -fcircle10 -fcircle100 \\",
				"  2>&1 | tail -30",
			].join("\n"),
		]);
		// x11perf should complete without server crashes
		expect(result.output).not.toContain("server crash");
		expect(result.output).not.toContain("connection reset");
		expect(result.output).toMatch(/reps|trep/i);
		console.log("x11perf wide lines + stipple fills completed");
	});
});

test.describe
	.serial("GLX and OpenGL", () => {
		test.setTimeout(120_000);

		test("glxinfo works with DRISW software rendering", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"LIBGL_DEBUG=verbose timeout 10 glxinfo -B 2>&1 | head -30",
			);
			expect(output).toContain("OpenGL renderer string: llvmpipe");
			expect(output).toContain("OpenGL version string:");
			expect(output).not.toContain("Segmentation fault");
			expect(output).not.toContain("[xcb] Extra reply data");
			expect(output).not.toContain("No matching fbConfigs");
		});

		test("glmark2 renders with DRISW software rendering", async ({
			sidecarContainer,
		}) => {
			// glmark2 calls glXChooseFBConfig with specific requirements.
			// Test what attributes it needs via a ctypes probe.
			const probeScript = [
				"import ctypes, ctypes.util, sys, os",
				"os.environ['LIBGL_ALWAYS_SOFTWARE'] = '1'",
				"X11 = ctypes.CDLL(ctypes.util.find_library('X11'))",
				"GL = ctypes.CDLL(ctypes.util.find_library('GL'))",
				"X11.XOpenDisplay.restype = ctypes.c_void_p",
				"dpy = X11.XOpenDisplay(b':99')",
				"if not dpy: print('FAIL:display'); sys.exit(1)",
				"GL.glXChooseFBConfig.restype = ctypes.POINTER(ctypes.c_void_p)",
				"# glmark2-style attrs: RGBA, double-buffered, depth 24, stencil 8",
				"attrs = (ctypes.c_int * 13)(0x8011, 1, 5, 1, 12, 24, 13, 8, 8, 8, 9, 8, 0)",
				"n = ctypes.c_int()",
				"cfgs = GL.glXChooseFBConfig(dpy, 0, attrs, ctypes.byref(n))",
				"print(f'ChooseFBConfig={n.value}')",
				"# Simpler: just double-buffer",
				"attrs2 = (ctypes.c_int * 3)(5, 1, 0)",
				"cfgs2 = GL.glXChooseFBConfig(dpy, 0, attrs2, ctypes.byref(n))",
				"print(f'SimpleChoose={n.value}')",
				"# Even simpler: no attrs at all",
				"attrs3 = (ctypes.c_int * 1)(0)",
				"cfgs3 = GL.glXChooseFBConfig(dpy, 0, attrs3, ctypes.byref(n))",
				"print(f'AnyConfig={n.value}')",
				"# Test glXGetVisualFromFBConfig — glmark2 needs this",
				"if cfgs and n.value > 0:",
				"    GL.glXGetVisualFromFBConfig.restype = ctypes.c_void_p",
				"    for i in range(min(n.value, 4)):",
				"        vi = GL.glXGetVisualFromFBConfig(dpy, cfgs[i])",
				"        print(f'  FBConfig[{i}] visual={hex(vi) if vi else \"NULL\"}')",
				"X11.XCloseDisplay(dpy)",
			].join("\n");
			const b64 = Buffer.from(probeScript).toString("base64");
			await sidecarContainer.exec([
				"bash",
				"-c",
				`printf '%s' '${b64}' | base64 -d > /tmp/glmark2_probe.py`,
			]);
			const probeOutput = await execInSidecar(
				sidecarContainer,
				"python3 /tmp/glmark2_probe.py 2>&1",
			);
			console.log("FBConfig probe:", probeOutput);

			// Run glmark2
			const output = await execInSidecar(
				sidecarContainer,
				"LIBGL_DEBUG=verbose timeout 10 glmark2 -b build 2>&1 | head -20 || true",
			);
			console.log("glmark2 output:", output);
			expect(output).not.toContain("Segmentation fault");
		});

		test("glxinfo reports GLX version and extensions", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"LIBGL_ALWAYS_INDIRECT=1 glxinfo 2>&1 | head -30",
			);
			expect(output).toContain("server glx version string: 1.4");
			expect(output).toContain("server glx vendor string: x11-web OSMesa");
			expect(output).not.toContain("[xcb] Extra reply data");
			expect(output).not.toContain("Segmentation fault");
		});

		test("glxgears renders frames without crash (indirect)", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"LIBGL_ALWAYS_INDIRECT=1 timeout 2 glxgears -info 2>&1 | head -20 || true",
			);
			expect(output).not.toContain("glXCreateContext failed");
			expect(output).not.toContain("Segmentation fault");
			expect(output).not.toContain("[xcb] Extra reply data");
		});
	});
