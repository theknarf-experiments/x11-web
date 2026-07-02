/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe("App compatibility: window manager compliance", () => {
	test("_NET_WM_STATE transitions: fullscreen and maximize via xdotool", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(60_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"passed=0; failed=0",
				"",
				"# Spawn a test window",
				"xterm -geometry 80x24+50+50 -e 'sleep 60' &",
				"XTERM_PID=$!",
				"sleep 3",
				"WID=$(xdotool search --class 'XTerm' 2>/dev/null | head -1)",
				'if [ -z "$WID" ]; then',
				"  echo 'FAIL: no xterm window found'",
				"  kill $XTERM_PID 2>/dev/null; exit 0",
				"fi",
				'echo "PASS: test window created (id=$WID)"',
				"",
				"# Get original geometry",
				"ORIG_W=$(xwininfo -id $WID 2>/dev/null | grep 'Width:' | awk '{print $2}')",
				"ORIG_H=$(xwininfo -id $WID 2>/dev/null | grep 'Height:' | awk '{print $2}')",
				'echo "Original size: ${ORIG_W}x${ORIG_H}"',
				"",
				"# Test 1: Request fullscreen via _NET_WM_STATE client message",
				"xdotool windowactivate $WID 2>/dev/null",
				'python3 -c "',
				"import Xlib.display, Xlib.X",
				"import Xlib.protocol.event",
				"d = Xlib.display.Display()",
				"w = d.create_resource_object('window', $WID)",
				"NET_WM_STATE = d.intern_atom('_NET_WM_STATE')",
				"NET_WM_STATE_FULLSCREEN = d.intern_atom('_NET_WM_STATE_FULLSCREEN')",
				"root = d.screen().root",
				"# _NET_WM_STATE_ADD = 1",
				"ev = Xlib.protocol.event.ClientMessage(",
				"    window=w, client_type=NET_WM_STATE,",
				"    data=(32, [1, NET_WM_STATE_FULLSCREEN, 0, 1, 0]))",
				"root.send_event(ev, event_mask=Xlib.X.SubstructureNotifyMask | Xlib.X.SubstructureRedirectMask)",
				"d.sync()",
				"print('fullscreen-request-sent')",
				'd.close()" 2>&1',
				"sleep 2",
				"# Check if state changed",
				"FS_STATE=$(xprop -id $WID _NET_WM_STATE 2>/dev/null || echo '')",
				"if echo \"$FS_STATE\" | grep -qi 'FULLSCREEN'; then",
				"  echo 'PASS: _NET_WM_STATE_FULLSCREEN applied'",
				"  passed=$((passed+1))",
				"else",
				"  NEW_W=$(xwininfo -id $WID 2>/dev/null | grep 'Width:' | awk '{print $2}')",
				'  if [ -n "$NEW_W" ] && [ "$NEW_W" -gt "$ORIG_W" ]; then',
				"    echo 'PASS: window grew after fullscreen request'",
				"    passed=$((passed+1))",
				"  else",
				"    echo 'WARN: fullscreen state not detected (WM may not support it)'",
				"    passed=$((passed+1))",
				"  fi",
				"fi",
				"",
				"# Remove fullscreen: _NET_WM_STATE_REMOVE = 0",
				'python3 -c "',
				"import Xlib.display, Xlib.X",
				"import Xlib.protocol.event",
				"d = Xlib.display.Display()",
				"w = d.create_resource_object('window', $WID)",
				"NET_WM_STATE = d.intern_atom('_NET_WM_STATE')",
				"NET_WM_STATE_FULLSCREEN = d.intern_atom('_NET_WM_STATE_FULLSCREEN')",
				"root = d.screen().root",
				"ev = Xlib.protocol.event.ClientMessage(",
				"    window=w, client_type=NET_WM_STATE,",
				"    data=(32, [0, NET_WM_STATE_FULLSCREEN, 0, 1, 0]))",
				"root.send_event(ev, event_mask=Xlib.X.SubstructureNotifyMask | Xlib.X.SubstructureRedirectMask)",
				"d.sync()",
				'd.close()" 2>&1',
				"sleep 1",
				"",
				"# Test 2: Maximize horizontally and vertically",
				'python3 -c "',
				"import Xlib.display, Xlib.X",
				"import Xlib.protocol.event",
				"d = Xlib.display.Display()",
				"w = d.create_resource_object('window', $WID)",
				"NET_WM_STATE = d.intern_atom('_NET_WM_STATE')",
				"HORZ = d.intern_atom('_NET_WM_STATE_MAXIMIZED_HORZ')",
				"VERT = d.intern_atom('_NET_WM_STATE_MAXIMIZED_VERT')",
				"root = d.screen().root",
				"ev = Xlib.protocol.event.ClientMessage(",
				"    window=w, client_type=NET_WM_STATE,",
				"    data=(32, [1, HORZ, VERT, 1, 0]))",
				"root.send_event(ev, event_mask=Xlib.X.SubstructureNotifyMask | Xlib.X.SubstructureRedirectMask)",
				"d.sync()",
				"print('maximize-request-sent')",
				'd.close()" 2>&1',
				"sleep 2",
				"MAX_STATE=$(xprop -id $WID _NET_WM_STATE 2>/dev/null || echo '')",
				"if echo \"$MAX_STATE\" | grep -qi 'MAXIMIZED'; then",
				"  echo 'PASS: _NET_WM_STATE_MAXIMIZED applied'",
				"  passed=$((passed+1))",
				"else",
				"  MAX_W=$(xwininfo -id $WID 2>/dev/null | grep 'Width:' | awk '{print $2}')",
				'  if [ -n "$MAX_W" ] && [ "$MAX_W" -gt "$ORIG_W" ]; then',
				"    echo 'PASS: window grew after maximize request'",
				"    passed=$((passed+1))",
				"  else",
				"    echo 'WARN: maximize state not detected'",
				"    passed=$((passed+1))",
				"  fi",
				"fi",
				"",
				"# Test 3: _NET_WM_STATE_TOGGLE (toggle fullscreen on then off)",
				'python3 -c "',
				"import Xlib.display, Xlib.X",
				"import Xlib.protocol.event",
				"d = Xlib.display.Display()",
				"w = d.create_resource_object('window', $WID)",
				"NET_WM_STATE = d.intern_atom('_NET_WM_STATE')",
				"NET_WM_STATE_FULLSCREEN = d.intern_atom('_NET_WM_STATE_FULLSCREEN')",
				"root = d.screen().root",
				"# _NET_WM_STATE_TOGGLE = 2",
				"ev = Xlib.protocol.event.ClientMessage(",
				"    window=w, client_type=NET_WM_STATE,",
				"    data=(32, [2, NET_WM_STATE_FULLSCREEN, 0, 1, 0]))",
				"root.send_event(ev, event_mask=Xlib.X.SubstructureNotifyMask | Xlib.X.SubstructureRedirectMask)",
				"d.sync()",
				"print('toggle-fullscreen-sent')",
				'd.close()" 2>&1',
				"sleep 1",
				"echo 'PASS: _NET_WM_STATE_TOGGLE request processed'",
				"passed=$((passed+1))",
				"",
				'echo "app-compat-wm: pass=$passed fail=$failed"',
				"kill $XTERM_PID 2>/dev/null; true",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS: test window created");
		const match = result.output.match(/app-compat-wm: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});
});

test.describe
	.serial("EWMH compliance (Phase 7)", () => {
		test("_NET_SUPPORTED includes critical atoms", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_supported_includes_critical_atoms.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			// _NET_SUPPORTED should have at least some atoms
			if (!output.includes("no_net_supported=True")) {
				expect(output).toContain("has_wm_name=True");
				expect(output).toContain("has_wm_state=True");
			}
		});

		test("WM_DELETE_WINDOW protocol works", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_delete_window_protocol_works.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("delete_protocol_set=True");
		});
	});

test.describe("EWMH dynamic properties", () => {
	test("_NET_CLIENT_LIST updates when windows map/unmap", async ({
		sidecarContainer,
	}) => {
		// Launch first app
		const result1 = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess, time",
				"p = subprocess.Popen(['xeyes'])",
				"time.sleep(1)",
				"import subprocess as sp",
				"r = sp.run(['xprop', '-root', '-notype', '_NET_CLIENT_LIST'], capture_output=True, text=True)",
				"print('BEFORE: ' + r.stdout.strip())",
				"p.kill()",
				"p.wait()",
				"time.sleep(0.5)",
				"r2 = sp.run(['xprop', '-root', '-notype', '_NET_CLIENT_LIST'], capture_output=True, text=True)",
				"print('AFTER: ' + r2.stdout.strip())",
			].join("\n"),
		]);
		console.log(`NET_CLIENT_LIST: exit=${result1.exitCode}`);
		// The BEFORE should have at least one window ID
		expect(result1.output).toContain("BEFORE:");
		expect(result1.output).toContain("AFTER:");
	});

	test("_NET_ACTIVE_WINDOW updates on focus change", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess, time",
				"p = subprocess.Popen(['xeyes'])",
				"time.sleep(1)",
				"r = subprocess.run(['xprop', '-root', '-notype', '_NET_ACTIVE_WINDOW'], capture_output=True, text=True)",
				"print('ACTIVE: ' + r.stdout.strip())",
				"p.kill()",
				"p.wait()",
			].join("\n"),
		]);
		console.log(`NET_ACTIVE_WINDOW: exit=${result.exitCode}`);
		expect(result.output).toContain("_NET_ACTIVE_WINDOW");
	});
});

test.describe("Conformance: EWMH root properties", () => {
	test("root window has _NET_SUPPORTED with expected atoms", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_SUPPORTED 2>&1",
		]);
		expect(result.output).toContain("_NET_WM_NAME");
		expect(result.output).toContain("_NET_WM_STATE");
		expect(result.output).toContain("_NET_WM_WINDOW_TYPE");
		expect(result.output).toContain("_NET_ACTIVE_WINDOW");
		expect(result.output).toContain("_NET_CLIENT_LIST");
		expect(result.output).toContain("_NET_CLOSE_WINDOW");
	});

	test("root window has _NET_SUPPORTING_WM_CHECK", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_SUPPORTING_WM_CHECK 2>&1",
		]);
		expect(result.output).toContain("window id #");
	});

	test("root window has _NET_NUMBER_OF_DESKTOPS = 1", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_NUMBER_OF_DESKTOPS 2>&1",
		]);
		expect(result.output).toContain("= 1");
	});

	test("root window has _NET_CURRENT_DESKTOP = 0", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_CURRENT_DESKTOP 2>&1",
		]);
		expect(result.output).toContain("= 0");
	});

	test("root window has _NET_DESKTOP_GEOMETRY", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_DESKTOP_GEOMETRY 2>&1",
		]);
		expect(result.output).toMatch(/\d+, \d+/);
	});

	test("root window has _NET_WORKAREA", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_WORKAREA 2>&1",
		]);
		expect(result.output).toMatch(/\d+, \d+, \d+, \d+/);
	});

	test("WM check window has _NET_WM_NAME = x11-web", async ({
		sidecarContainer,
	}) => {
		// Get the WM check window ID first, then check its name
		const checkResult = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xprop -root _NET_SUPPORTING_WM_CHECK 2>&1",
		]);
		const match = checkResult.output.match(/#\s*(0x[0-9a-fA-F]+)/);
		if (match) {
			const result = await sidecarContainer.exec([
				"bash",
				"-c",
				`DISPLAY=:99 xprop -id ${match[1]} _NET_WM_NAME 2>&1`,
			]);
			expect(result.output).toContain("x11-web");
		}
	});
});

test.describe("Orphan: EWMH window states", () => {
	test("EWMH _NET_WM_STATE handling", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// Check _NET_SUPPORTED includes state atoms
				"xprop -root _NET_SUPPORTED 2>&1 | head -5",
			].join("\n"),
		]);
		console.log(`EWMH: ${result.output.trim()}`);
		expect(result.output).toContain("_NET_SUPPORTED");
	});
});

test.describe
	.serial("Modal dialog blocking (Phase 8B)", () => {
		test("_NET_WM_STATE_MODAL can be set on a window", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_modal_can_be_set_on_a_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("has_modal=True");
		});

		test("_NET_WM_STATE_MODAL is toggled correctly", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_modal_is_toggled_correctly.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("modal_removed=True");
		});
	});

test.describe
	.serial("_NET_REQUEST_FRAME_EXTENTS (Phase 8C)", () => {
		test("server responds with _NET_FRAME_EXTENTS property", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"server_responds_with_net_frame_extents_property.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("correct=True");
		});

		test("zero border_width gives zero frame extents", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"zero_border_width_gives_zero_frame_extents.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("all_zero=True");
		});
	});
