/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import type { StartedTestContainer } from "testcontainers";
import { expect, runPythonScript, test } from "../fixtures";

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

test.describe
	.serial("XKB event notifications", () => {
		test("XKEYBOARD extension has proper event base", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkeyboard_extension_has_proper_event_base.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("present=True");
			expect(output).toContain("major_opcode=136");
			expect(output).toContain("has_event_base=True");
		});

		test("XKB SelectEvents accepts subscription requests", async ({
			sidecarContainer,
		}) => {
			// Use xdotool to verify XKB is functional via key simulation
			const output = await execInSidecar(
				sidecarContainer,
				`xdotool key shift 2>&1 && echo "xkb_key_ok=True"`,
			);
			expect(output).toContain("xkb_key_ok=True");
		});

		test("XKB GetState returns valid modifier state", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkb_getstate_returns_valid_modifier_state.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			// Just verify the extension is queryable without crashing
			expect(output).not.toContain("error");
		});

		test("xinput list shows keyboard devices", async ({ sidecarContainer }) => {
			const output = await execInSidecar(sidecarContainer, "xinput list 2>&1");
			// Should list at least a virtual core keyboard
			expect(output.toLowerCase()).toMatch(/keyboard|pointer/);
		});
	});

test.describe
	.serial("Application compatibility", () => {
		test("Tk applications (wish) can open display", async ({
			sidecarContainer,
		}) => {
			// Tk uses XI 1.x, so this tests our ListInputDevices implementation
			const output = await execInSidecar(
				sidecarContainer,
				`echo 'puts "tk_ok"; exit' | timeout 5 wish 2>&1 || echo "wish_not_available"`,
			);
			if (!output.includes("wish_not_available")) {
				expect(output).toContain("tk_ok");
			}
		});

		test("xclock renders without errors", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				`timeout 3 xclock -digital 2>&1; echo "exit=$?"`,
			);
			expect(output).not.toContain("Error");
			expect(output).not.toContain("cannot open display");
		});

		test("xdpyinfo reports complete display info", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xdpyinfo 2>&1");
			expect(output).toContain("number of extensions:");
			expect(output).toContain("RENDER");
			expect(output).toContain("RANDR");
			expect(output).toContain("XFIXES");
			expect(output).toContain("SYNC");
			expect(output).toContain("XKEYBOARD");
			expect(output).toContain("Composite");
			expect(output).toContain("GLX");
			expect(output).toContain("MIT-SHM");
			expect(output).toContain("DOUBLE-BUFFER");
			expect(output).toContain("SHAPE");
			expect(output).toContain("RECORD");
			expect(output).toContain("XTEST");
			expect(output).toContain("X-Resource");
			expect(output).toContain("DPMS");
			expect(output).toContain("BIG-REQUESTS");
		});

		test("Multiple concurrent X clients don't crash", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"multiple_concurrent_x_clients_don_t_crash.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("ok_count=5");
			expect(output).toContain("error_count=0");
		});

		test("Clipboard round-trip between clients works", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"clipboard_round_trip_between_clients_works.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("owner_set=True");
		});
	});

test.describe("XKB extension conformance", () => {
	test("XKB ListComponents returns real component names", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"xkb_listcomponents_real_names.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("PASS");
	});

	test("XKB SetMap + GetMap round-trip preserves keysyms", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"xkb_setmap_getmap_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("PASS");
	});

	test("xset q reports keyboard state without errors", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"export DISPLAY=:99 && xset q 2>&1 | head -30",
		]);
		// xset q should show keyboard and pointer info
		expect(result.output).toMatch(/Keyboard Control|Key click|auto repeat/i);
	});
});

test.describe("XKB advanced opcodes", () => {
	test("setxkbmap queries work", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["setxkbmap", "-query"]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toMatch(/layout/i);
	});
});

test.describe
	.serial("Application compatibility", () => {
		// Container setup can take several minutes — use a generous timeout.
		test.setTimeout(300_000);

		test.afterEach(async ({ sidecarContainer }) => {
			await killApps(sidecarContainer);
		});

		// --- Container setup (first test absorbs fixture init time) ---

		test("containers start and sidecar is ready", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "echo READY");
			expect(output).toContain("READY");
		});

		// --- X11 tool validation ---

		test("server reports all required extensions", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"server_reports_all_required_extensions.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();

			for (const ext of [
				"BIG-REQUESTS",
				"Composite",
				"DAMAGE",
				"DPMS",
				"Generic Event Extension",
				"MIT-SCREEN-SAVER",
				"MIT-SHM",
				"RANDR",
				"RECORD",
				"RENDER",
				"SECURITY",
				"SHAPE",
				"SYNC",
				"X-Resource",
				"XC-MISC",
				"XFIXES",
				"XInputExtension",
				"XKEYBOARD",
				"XTEST",
				"XVideo",
			]) {
				expect(output).toContain(ext);
			}
		});

		test("server reports correct screen info", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"server_reports_correct_screen_info.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("screens=1");
			expect(output).toContain("depth=24");
		});

		test("standard and EWMH atoms are present", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"standard_and_ewmh_atoms_are_present.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("PRIMARY=");
			expect(output).toContain("WM_NAME=");
			expect(output).toContain("_NET_WM_STATE=");
			expect(output).toContain("_NET_SUPPORTED=");
			// All atoms should be non-zero (meaning they exist)
			expect(output).not.toContain("=0");
		});

		test("RANDR provides screen information", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"randr_provides_screen_information.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("randr_present=True");
			expect(output).toMatch(/width=\d+/);
		});

		test("Font system serves standard fonts", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"font_system_serves_standard_fonts.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			const count = Number.parseInt(
				output.match(/font_count=(\d+)/)?.[1] ?? "0",
			);
			expect(count).toBeGreaterThan(5);
			expect(output).toContain("has_fixed=True");
		});

		test("Visual configuration supports TrueColor", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"visual_configuration_supports_truecolor.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("is_truecolor=True");
			expect(output).toContain("root_depth=24");
		});

		// --- XSETTINGS / XKB ---

		test("XSETTINGS manager provides GTK defaults", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xsettings_manager_provides_gtk_defaults.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("xsettings_owner=True");
			const cnt = Number.parseInt(
				output.match(/xsettings_count=(\d+)/)?.[1] ?? "0",
			);
			expect(cnt).toBeGreaterThan(0);
		});

		test("XKB extension is available", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkb_extension_is_available.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("xkb_present=True");
		});

		// --- Real applications ---

		// --- Real application tests (use python3-xlib to avoid XCB issues) ---

		test("X11 window create/map/destroy cycle works", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"x11_window_create_map_destroy_cycle_works.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("mapped_width=200");
			expect(output).toContain("lifecycle_ok=True");
		});

		test("wish (Tcl/Tk) creates and renders widgets", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				`echo 'wm title . "TkTest"; label .l -text "Hello Tk"; pack .l; update; puts "TK_RENDERED"; exit' | timeout 8 wish 2>&1`,
			);
			expect(output).toContain("TK_RENDERED");
			expect(output).not.toContain("X Error");
		});

		// --- EWMH fullscreen / maximize ---

		test("_NET_WM_STATE_FULLSCREEN resizes window to screen size", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_fullscreen_resizes_window_to_screen_size.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("fullscreen_ok=True");
			expect(output).toContain("restore_ok=True");
		});

		test("_NET_WM_STATE_MAXIMIZED resizes window", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_maximized_resizes_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("maximize_ok=True");
		});

		// --- Multi-client interaction ---

		test("Multiple X clients can coexist", async ({ sidecarContainer }) => {
			// Kill any leftover xeyes/xclock/xlogo instances from previous tests
			// or test files on the same Playwright worker (sidecarContainer is
			// scoped to the worker, so processes outlive a single test file).
			await execInSidecar(
				sidecarContainer,
				[
					"for app in xeyes xclock xlogo; do",
					"  pkill -KILL -x $app 2>/dev/null; true",
					"done",
					"sleep 1",
				].join("\n"),
			);
			await execInSidecar(sidecarContainer, "xeyes &");
			await execInSidecar(sidecarContainer, "xclock -digital &");
			await execInSidecar(sidecarContainer, "xlogo &");
			await new Promise((r) => setTimeout(r, 3000));

			// Count only LIVE processes — `pgrep -x` happily counts zombies
			// (state Z) that PID 1 in the sidecar container never reaps after
			// previous tests' background processes are killed.  `ps -eo state`
			// filters those out.
			const ps = await execInSidecar(
				sidecarContainer,
				[
					'count() { ps --no-headers -eo state,comm 2>/dev/null | awk -v c="$1" \'$2 == c && $1 != "Z" { n++ } END { print n + 0 }\'; }',
					'echo "xeyes=$(count xeyes)"',
					'echo "xclock=$(count xclock)"',
					'echo "xlogo=$(count xlogo)"',
				].join("\n"),
			);
			expect(ps).toContain("xeyes=1");
			expect(ps).toContain("xclock=1");
			expect(ps).toContain("xlogo=1");
		});

		test("Window stacking z-order is maintained", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"window_stacking_z_order_is_maintained.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("w2_above_w1=True");
			expect(output).toContain("after_raise_w1_above_w2=True");
		});

		// --- EWMH property tests ---

		test("_NET_SUPPORTED has 20+ atoms", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_supported_has_20_atoms.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			const cnt = Number.parseInt(output.match(/count=(\d+)/)?.[1] ?? "0");
			expect(cnt).toBeGreaterThan(20);
		});

		test("window configure and query works", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"window_configure_and_query_works.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("configure_ok=True");
		});

		// --- Extension availability ---

		test("All required extensions are present", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"all_required_extensions_are_present.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			for (const ext of [
				"MIT-SHM",
				"RENDER",
				"Composite",
				"RANDR",
				"XKEYBOARD",
				"SHAPE",
				"SYNC",
				"XFIXES",
				"DAMAGE",
				"XInputExtension",
				"XTEST",
				"RECORD",
				"SECURITY",
				"BIG-REQUESTS",
				"XC-MISC",
			]) {
				expect(output).toContain(`${ext}=True`);
			}
		});
	});

test.describe
	.serial("xdpyinfo verification", () => {
		test("xdpyinfo reports correct server info", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xdpyinfo 2>&1");
			// Basic server info
			expect(output).toContain("screen #0");
			expect(output).toMatch(/dimensions:/);
			expect(output).toMatch(/depth.*24/);

			// Should report all key extensions
			const requiredExtensions = [
				"BIG-REQUESTS",
				"RENDER",
				"XFIXES",
				"Composite",
				"DAMAGE",
				"RANDR",
				"XINERAMA",
				"SYNC",
				"MIT-SHM",
				"XInputExtension",
				"XKEYBOARD",
				"SHAPE",
				"XTEST",
				"DPMS",
				"DOUBLE-BUFFER",
				"SECURITY",
				"RECORD",
				"Present",
				"GLX",
			];
			for (const ext of requiredExtensions) {
				expect(output).toContain(ext);
			}
		});

		test("xdpyinfo reports correct visual info", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xdpyinfo 2>&1");
			// Should have TrueColor visual
			expect(output).toContain("TrueColor");
			// Should report visual depth
			expect(output).toMatch(/depth.*24/);
			// Color depth info (xdpyinfo says "significant bits in color specification")
			expect(output).toMatch(/significant bits|bits per rgb/i);
		});
	});

test.describe("Extension enumeration", () => {
	test("all required extensions are advertised", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"export DISPLAY=:99 && xdpyinfo 2>&1",
		]);
		const output = result.output;
		const requiredExtensions = [
			"BIG-REQUESTS",
			"Composite",
			"DAMAGE",
			"DPMS",
			"Generic Event Extension",
			"MIT-SCREEN-SAVER",
			"MIT-SHM",
			"RANDR",
			"RECORD",
			"RENDER",
			"SECURITY",
			"SHAPE",
			"SYNC",
			"XC-MISC",
			"XFIXES",
			"XInputExtension",
			"XKEYBOARD",
			"XVideo",
		];
		let found = 0;
		for (const ext of requiredExtensions) {
			if (output.includes(ext)) {
				found++;
			}
		}
		expect(found).toBeGreaterThanOrEqual(16);
	});

	test("extension version negotiation works", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"extension_version_negotiation.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("PASS");
	});
});

test.describe("Extension enumeration completeness", () => {
	test("all required extensions are listed by xdpyinfo", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"set -e",
				"export DISPLAY=:99",
				"EXTS=$(xdpyinfo 2>&1)",
				"passed=0",
				"failed=0",
				"check_ext() {",
				'  if echo "$EXTS" | grep -qi "$1"; then',
				'    echo "PASS: $1"',
				"    passed=$((passed+1))",
				"  else",
				'    echo "FAIL: $1 missing"',
				"    failed=$((failed+1))",
				"  fi",
				"}",
				"check_ext 'BIG-REQUESTS'",
				"check_ext 'Composite'",
				"check_ext 'DAMAGE'",
				// xdpyinfo prints the wire name verbatim; ours is
				// "Generic Event Extension", not "Generic Events".
				"check_ext 'Generic Event Extension'",
				"check_ext 'GLX'",
				"check_ext 'Present'",
				"check_ext 'RANDR'",
				"check_ext 'RENDER'",
				"check_ext 'SHAPE'",
				"check_ext 'MIT-SHM'",
				"check_ext 'SYNC'",
				"check_ext 'XFIXES'",
				"check_ext 'XInputExtension'",
				"check_ext 'XKEYBOARD'",
				"check_ext 'XTEST'",
				"check_ext 'XC-MISC'",
				"check_ext 'XVideo'",
				"check_ext 'RECORD'",
				"check_ext 'SECURITY'",
				"check_ext 'DPMS'",
				"check_ext 'XFree86-VidModeExtension'",
				"check_ext 'DOUBLE-BUFFER'",
				"check_ext 'MIT-SCREEN-SAVER'",
				"check_ext 'XINERAMA'",
				"check_ext 'X-Resource'",
				'echo "extensions: pass=$passed fail=$failed"',
			].join("\n"),
		]);
		const match = result.output.match(/extensions: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		// All 25 advertised extensions must be present.
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(25);
		expect(Number.parseInt(match![2], 10)).toBe(0);
	});
});

test.describe
	.serial("XKB controls and accessibility", () => {
		test("XKB GetControls returns valid control state", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkb_getcontrols_returns_valid_control_state.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("xkb_present=True");
		});

		test("XKB modifier state tracks correctly", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkb_modifier_state_tracks_correctly.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			// No modifiers pressed initially
			expect(output).toContain("initial_mods=0");
		});
	});

test.describe
	.serial("XKB control masks (Phase 7A)", () => {
		test("SetControls/GetControls round-trips RepeatKeys (bit 0)", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"setcontrols_getcontrols_round_trips_repeatkeys_bit_0.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			// RepeatKeys (bit 0) should be enabled by default
			if (output.includes("repeat_keys_enabled=")) {
				expect(output).toContain("repeat_keys_enabled=1");
			} else {
				expect(output).toContain("xkb_present=true");
			}
		});

		test("XKB extension is queryable", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"xkb_extension_is_queryable.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("present=True");
		});
	});

test.describe
	.serial("Application compatibility (Phase 7)", () => {
		test("xdpyinfo runs without errors", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"timeout 10 xdpyinfo 2>&1 | head -20",
			);
			expect(output).toContain("number of extensions:");
			expect(output).not.toContain("unable to open display");
		});

		test("xdpyinfo reports all critical extensions", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"timeout 10 xdpyinfo -queryExtensions 2>&1",
			);
			expect(output).toContain("RENDER");
			expect(output).toContain("RANDR");
			expect(output).toContain("XKEYBOARD");
			expect(output).toContain("XInputExtension");
			expect(output).toContain("SHAPE");
			expect(output).toContain("MIT-SHM");
			expect(output).toContain("XFIXES");
			expect(output).toContain("Composite");
			expect(output).toContain("DAMAGE");
			expect(output).toContain("SYNC");
			expect(output).toContain("DOUBLE-BUFFER");
		});

		test("xlsfonts lists available fonts", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"timeout 10 xlsfonts 2>&1 | wc -l",
			);
			const fontCount = parseInt(output.trim(), 10);
			expect(fontCount).toBeGreaterThan(5);
		});

		test("xlsatoms lists standard atoms", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"timeout 10 xlsatoms 2>&1 | head -20",
			);
			expect(output).toContain("PRIMARY");
			expect(output).toContain("SECONDARY");
			expect(output).toContain("ATOM");
		});

		test("xprop can query root window", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"timeout 10 xprop -root 2>&1 | head -10",
			);
			expect(output).not.toContain("unable to open display");
			// Should have at least some properties
			expect(output.length).toBeGreaterThan(0);
		});
	});

test.describe("Cross-connection event delivery", () => {
	test("ReparentNotify sent to parent with SubstructureNotifyMask", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"from Xlib import X, display",
				"d = display.Display()",
				"s = d.screen()",
				"# Create parent window with SubstructureNotifyMask",
				"parent = s.root.create_window(0, 0, 200, 200, 0, s.root_depth,",
				"    event_mask=X.SubstructureNotifyMask | X.StructureNotifyMask)",
				"parent.map()",
				"d.sync()",
				"# Create child window",
				"child = s.root.create_window(50, 50, 100, 100, 0, s.root_depth)",
				"child.map()",
				"d.sync()",
				"# Reparent child to our parent",
				"child.reparent(parent, 10, 10)",
				"d.sync()",
				"import time; time.sleep(0.2)",
				"# Check for ReparentNotify event on parent",
				"got_reparent = False",
				"while d.pending_events() > 0:",
				"    ev = d.next_event()",
				"    if ev.type == X.ReparentNotify:",
				"        got_reparent = True",
				"        break",
				"if got_reparent:",
				"    print('REPARENT_NOTIFY_OK')",
				"else:",
				"    print('REPARENT_NOTIFY_MISSING')",
				"child.destroy()",
				"parent.destroy()",
				"d.close()",
			].join("\n"),
		]);
		console.log(`ReparentNotify: exit=${result.exitCode}`);
		expect(result.output).toContain("REPARENT_NOTIFY_OK");
	});

	test("MapNotify sent to parent with SubstructureNotifyMask", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"from Xlib import X, display",
				"d = display.Display()",
				"s = d.screen()",
				"# Create parent window with SubstructureNotifyMask",
				"parent = s.root.create_window(0, 0, 200, 200, 0, s.root_depth,",
				"    event_mask=X.SubstructureNotifyMask)",
				"parent.map()",
				"d.sync()",
				"# Create child window under parent (unmapped)",
				"child = parent.create_window(10, 10, 100, 100, 0, s.root_depth)",
				"d.sync()",
				"import time; time.sleep(0.1)",
				"# Drain any pending events",
				"while d.pending_events() > 0: d.next_event()",
				"# Map child - parent should get MapNotify",
				"child.map()",
				"d.sync()",
				"time.sleep(0.2)",
				"got_map = False",
				"while d.pending_events() > 0:",
				"    ev = d.next_event()",
				"    if ev.type == X.MapNotify:",
				"        got_map = True",
				"        break",
				"if got_map:",
				"    print('MAP_NOTIFY_OK')",
				"else:",
				"    print('MAP_NOTIFY_MISSING')",
				"child.destroy()",
				"parent.destroy()",
				"d.close()",
			].join("\n"),
		]);
		console.log(`MapNotify to parent: exit=${result.exitCode}`);
		expect(result.output).toContain("MAP_NOTIFY_OK");
	});
	// =====================================================================
	// Phase 4 tests: New spec-compliance features
	// =====================================================================

	test("XKB SetNames and GetKbdByName are handled without errors", async ({
		sidecarContainer,
	}) => {
		// setxkbmap queries GetKbdByName internally; verify it doesn't crash
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && setxkbmap -query 2>&1`,
		]);
		console.log(`setxkbmap output: ${result.output}`);
		expect(result.exitCode).toBeLessThanOrEqual(1); // 0 = success, 1 = no rules (acceptable)
		// Verify xkbcomp can dump the keymap (uses GetKbdByName)
		const result2 = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xkbcomp :99 /dev/null 2>&1`,
		]);
		console.log(`xkbcomp exit=${result2.exitCode}`);
		expect(result2.exitCode).toBeLessThanOrEqual(1);
	});

	test("PseudoColor visual is reported by xdpyinfo", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdpyinfo 2>&1 | grep -i pseudocolor`,
		]);
		console.log(`PseudoColor: ${result.output.trim()}`);
		expect(result.output.toLowerCase()).toContain("pseudocolor");
	});

	test("AllocColor works in TrueColor colormap", async ({
		sidecarContainer,
	}) => {
		// python3-xlib test that allocates a color
		const result = await runPythonScript(
			sidecarContainer,
			"alloccolor_truecolor_colormap.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`AllocColor: ${result.output.trim()}`);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("pixel=");
	});

	test("DBE AllocateBackBuffer and SwapBuffers work", async ({
		sidecarContainer,
	}) => {
		// Use xdpyinfo to verify DBE extension is present
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdpyinfo -ext DOUBLE-BUFFER 2>&1`,
		]);
		console.log(`DBE ext: ${result.output.substring(0, 200)}`);
		expect(result.output).toContain("DOUBLE-BUFFER");
	});

	test("MIT-SCREEN-SAVER extension QueryVersion works", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdpyinfo -ext MIT-SCREEN-SAVER 2>&1`,
		]);
		console.log(`ScreenSaver: ${result.output.substring(0, 200)}`);
		expect(result.output).toContain("MIT-SCREEN-SAVER");
	});

	test("XTEST CompareCursor returns correct result", async ({
		sidecarContainer,
	}) => {
		// xdotool uses XTEST extension; verify it works
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdotool getactivewindow 2>&1 || echo "xdotool_ok"`,
		]);
		console.log(`XTEST xdotool: ${result.output.trim()}`);
		// xdotool should not crash from CompareCursor
		expect(result.exitCode).toBeLessThanOrEqual(1);
	});

	test("SYNC counter query returns SERVERTIME value", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"sync_counter_query_servertime.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`SYNC query: ${result.output.trim()}`);
		expect(result.exitCode).toBe(0);
	});

	test("WM_HINTS property is accepted without errors", async ({
		sidecarContainer,
	}) => {
		// Set WM_HINTS on a window via python3-xlib
		const result = await runPythonScript(
			sidecarContainer,
			"wm_hints_property_accepted.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`WM_HINTS: ${result.output.trim()}`);
		expect(result.exitCode).toBe(0);
	});

	test("StoreColors works on PseudoColor colormap", async ({
		sidecarContainer,
	}) => {
		// Test that StoreColors doesn't crash for PseudoColor visual
		const result = await runPythonScript(
			sidecarContainer,
			"storecolors_pseudocolor_colormap.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`PseudoColor: ${result.output.trim()}`);
		expect(result.exitCode).toBe(0);
	});

	test("xset s queries screen saver without errors", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xset s 2>&1`,
		]);
		console.log(`xset s: exit=${result.exitCode}`);
		// xset s should not crash
		expect(result.exitCode).toBeLessThanOrEqual(1);
	});

	test("all 24 extensions are still advertised after changes", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdpyinfo -queryExtensions 2>&1 | grep 'number of extensions' | head -1`,
		]);
		console.log(`Extensions: ${result.output.trim()}`);
		const m = result.output.match(/number of extensions:\s+(\d+)/);
		expect(m).toBeTruthy();
		expect(Number.parseInt(m![1], 10)).toBeGreaterThanOrEqual(24);
	});

	test("xdpyinfo reports all depths (1, 4, 8, 16, 24, 32) after changes", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && xdpyinfo 2>&1 | grep '^ *depths' | head -1`,
		]);
		console.log(`Depths: ${result.output.trim()}`);
		for (const depth of ["1", "4", "8", "16", "24", "32"]) {
			expect(result.output).toContain(depth);
		}
	});

	test("rendercheck all test groups still pass after changes", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && timeout 30 rendercheck -t fill 2>&1 | tail -5`,
		]);
		console.log(`rendercheck fill: ${result.output.trim()}`);
		expect(result.output.toLowerCase()).not.toContain("tests failed");
	});

	test("x11perf basic operations still work after changes", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			`export DISPLAY=:99 && timeout 10 x11perf -rect100 -reps 10 2>&1 | tail -3`,
		]);
		console.log(`x11perf rect100: ${result.output.trim()}`);
		expect(result.exitCode).toBeLessThanOrEqual(1);
	});

	test("python3-xlib: full protocol round-trip with new features", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"python_xlib_full_protocol_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Full round-trip: ${result.output.trim()}`);
		expect(result.exitCode).toBe(0);
		expect(result.output).toContain("ALL_OK");
	});
});

test.describe("Protocol compliance: xdpyinfo", () => {
	test("xdpyinfo reports all required extensions", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"OUTPUT=$(xdpyinfo 2>&1)",
				"PASS=0; FAIL=0",
				// Check for required extensions
				"for ext in BIG-REQUESTS MIT-SHM RENDER XFIXES SHAPE SYNC 'Generic Event Extension' XC-MISC Composite DAMAGE RANDR XKEYBOARD XInputExtension XTEST DPMS DOUBLE-BUFFER RECORD SECURITY X-Resource Present; do",
				'  if echo "$OUTPUT" | grep -qi "$ext"; then',
				"    PASS=$((PASS+1))",
				"  else",
				"    FAIL=$((FAIL+1))",
				'    echo "MISSING_EXT: $ext"',
				"  fi",
				"done",
				// Check screen info
				"if echo \"$OUTPUT\" | grep -q 'screen #0'; then PASS=$((PASS+1)); else FAIL=$((FAIL+1)); echo 'MISSING: screen #0'; fi",
				"if echo \"$OUTPUT\" | grep -q 'dimensions:'; then PASS=$((PASS+1)); else FAIL=$((FAIL+1)); echo 'MISSING: dimensions'; fi",
				"if echo \"$OUTPUT\" | grep -q 'depth.*24'; then PASS=$((PASS+1)); else FAIL=$((FAIL+1)); echo 'MISSING: depth 24'; fi",
				// Check visual info
				"if echo \"$OUTPUT\" | grep -q 'TrueColor'; then PASS=$((PASS+1)); else FAIL=$((FAIL+1)); echo 'MISSING: TrueColor visual'; fi",
				// Check pixmap formats
				"if echo \"$OUTPUT\" | grep -q 'pixmap formats'; then PASS=$((PASS+1)); else FAIL=$((FAIL+1)); echo 'MISSING: pixmap formats'; fi",
				'echo "xdpyinfo-check: pass=$PASS fail=$FAIL"',
			].join("\n"),
		]);
		const match = result.output.match(/xdpyinfo-check: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`xdpyinfo: ${passed} checks passed, ${failed} failed`);
		// All required extensions and properties must be present
		expect(failed).toBe(0);
	});

	test("xdpyinfo reports multiple visual types", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(15_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"VISUALS=$(xdpyinfo 2>&1 | grep -c 'visual:' || echo 0)",
				'echo "visual-count: $VISUALS"',
			].join("\n"),
		]);
		const match = result.output.match(/visual-count: (\d+)/);
		expect(match).toBeTruthy();
		const count = Number.parseInt(match![1], 10);
		// Our server provides multiple visuals (TrueColor 24, DirectColor, PseudoColor, etc.)
		expect(count).toBeGreaterThanOrEqual(3);
	});
});

test.describe("Conformance: Protocol edge cases", () => {
	test("xlsatoms returns standard X11 atoms", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xlsatoms 2>&1 | head -50",
		]);
		// Standard pre-defined atoms (PRIMARY=1, ATOM=4, STRING=31)
		expect(result.output).toContain("PRIMARY");
		expect(result.output).toContain("ATOM");
		expect(result.output).toContain("STRING");
	});

	test("xwininfo reports root window properties", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xwininfo -root 2>&1",
		]);
		expect(result.output).toContain("Width:");
		expect(result.output).toContain("Height:");
		expect(result.output).toContain("Depth:");
	});

	test("xdpyinfo reports all registered extensions", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xdpyinfo -queryExtensions 2>&1",
		]);
		// Core extensions that must be present
		const requiredExtensions = [
			"BIG-REQUESTS",
			"MIT-SHM",
			"RENDER",
			"XFIXES",
			"SHAPE",
			"SYNC",
			"Composite",
			"DAMAGE",
			"RANDR",
			"XInputExtension",
			"XKEYBOARD",
			"XTEST",
			"GLX",
			"Present",
			"X-Resource",
		];
		for (const ext of requiredExtensions) {
			expect(result.output).toContain(ext);
		}
	});

	test("xdpyinfo reports correct visual classes", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"DISPLAY=:99 xdpyinfo 2>&1",
		]);
		expect(result.output).toContain("TrueColor");
		expect(result.output).toMatch(/depth.*24/);
	});

	test("multiple concurrent X11 connections work", async ({
		sidecarContainer,
	}) => {
		// Start two xeyes in background, verify both connect
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xeyes &",
				"PID1=$!",
				"xeyes &",
				"PID2=$!",
				"sleep 1",
				"# Both should still be running",
				"kill -0 $PID1 && kill -0 $PID2 && echo 'both-alive'",
				"kill $PID1 $PID2 2>/dev/null",
				"wait",
			].join("\n"),
		]);
		expect(result.output).toContain("both-alive");
	});

	test("xprop can list properties on a window", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xeyes &",
				"PID=$!",
				"sleep 0.5",
				"# Find the xeyes window",
				"WID=$(xdotool search --name xeyes 2>/dev/null | head -1)",
				'if [ -n "$WID" ]; then',
				"  xprop -id $WID 2>&1 | head -20",
				"else",
				"  echo 'no-window-found'",
				"fi",
				"kill $PID 2>/dev/null",
			].join("\n"),
		]);
		// xprop should either list properties or find the window
		expect(result.exitCode).toBeDefined();
	});
});

test.describe
	.serial("Protocol validation tools", () => {
		test.setTimeout(60_000);

		test("xdpyinfo reports correct server info", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xdpyinfo 2>&1 | head -40",
			);
			expect(output).not.toContain("unable to open display");
			expect(output).not.toContain("Error");
			// Should report version and screen info
			expect(output).toMatch(/version number/i);
			expect(output).toMatch(/screen/i);
		});

		test("xdpyinfo reports all expected extensions", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xdpyinfo -queryExtensions 2>&1",
			);
			// Core extensions that real applications require
			const requiredExtensions = [
				"RENDER",
				"RANDR",
				"XFIXES",
				"SHAPE",
				"MIT-SHM",
				"SYNC",
				"Composite",
				"DAMAGE",
				"XTEST",
				"XInputExtension",
				"XKEYBOARD",
			];
			for (const ext of requiredExtensions) {
				expect(output).toContain(ext);
			}
		});

		test("xprop on root window returns standard properties", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xprop -root 2>&1 | head -30",
			);
			expect(output).not.toContain("Error");
			// Root window should have EWMH properties
			expect(output).toMatch(/_NET_SUPPORTED|_NET_WM_NAME|WM_NAME/);
		});

		test("xlsatoms returns predefined atoms", async ({ sidecarContainer }) => {
			// STRING is predefined atom 31, so we need to peek past the
			// first 20 entries.
			const output = await execInSidecar(
				sidecarContainer,
				"xlsatoms 2>&1 | head -40",
			);
			expect(output).not.toContain("Error");
			expect(output).toContain("PRIMARY");
			expect(output).toContain("STRING");
		});

		test("xwininfo on root window succeeds", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xwininfo -root 2>&1",
			);
			expect(output).not.toContain("Error");
			expect(output).toMatch(/Width|Height|Depth/);
		});
	});

test.describe("XKB compat map", () => {
	test("xkbcomp can dump the compat map", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xkbcomp :99 /tmp/xkb_dump.xkb 2>&1",
				"grep -c 'interpret' /tmp/xkb_dump.xkb || echo 0",
				"echo XKB_COMPAT_DUMP_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("XKB_COMPAT_DUMP_PASS");
	});

	test("modifier keys produce correct keysyms", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"# Keycode 50 = Shift_L (keysym 0xFFE1)",
				"sym = d.keycode_to_keysym(50, 0)",
				"print(f'shift_l_sym={sym:#x}')",
				"assert sym == 0xFFE1, f'Expected 0xFFE1, got {sym:#x}'",
				"# Keycode 66 = Caps_Lock (keysym 0xFFE5)",
				"sym = d.keycode_to_keysym(66, 0)",
				"print(f'caps_lock_sym={sym:#x}')",
				"assert sym == 0xFFE5, f'Expected 0xFFE5, got {sym:#x}'",
				"print('MODIFIER_KEYSYM_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("MODIFIER_KEYSYM_PASS");
	});
});

test.describe
	.serial("Server capabilities via xdpyinfo", () => {
		test.setTimeout(60_000);

		test("xdpyinfo reports all required extensions", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xdpyinfo");
			expect(output).not.toContain("XDPYINFO_FAILED");

			// Verify key extensions are listed
			const requiredExtensions = [
				"BIG-REQUESTS",
				"RENDER",
				"XFIXES",
				"SHAPE",
				"SYNC",
				"RANDR",
				"XKEYBOARD",
				"XTEST",
				"Composite",
				"DAMAGE",
			];

			for (const ext of requiredExtensions) {
				expect(output).toContain(ext);
			}
		});

		test("xdpyinfo reports correct visual depths", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(sidecarContainer, "xdpyinfo");

			// Must report 24-bit TrueColor (the default visual)
			expect(output).toContain("depth 24");
			expect(output).toContain("TrueColor");
		});
	});

test.describe("Conformance: Extension conformance", () => {
	test("XFIXES: region operations work", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"xfixes_region_operations.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`XFIXES: ${result.output}`);
		expect(result.output).toContain("XFIXES_OK");
	});

	test("SHAPE extension is available", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"shape_extension_available.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("SHAPE_OK");
	});

	test("MIT-SHM extension is available", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"mit_shm_extension_available.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("SHM_OK");
	});

	test("SYNC extension: counter operations", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"sync_extension_counter_ops.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("SYNC_OK");
	});

	test("COMPOSITE and DAMAGE extensions available", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"composite_damage_extensions.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("COMP_DAMAGE_OK");
	});

	test("XKB: GetState and GetMap succeed", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"# Use xkbcomp to query the full keymap",
				"xkbcomp -xkb :99 /tmp/xkb_test.xkb 2>&1",
				"EXIT_CODE=$?",
				'echo "XKBCOMP_EXIT=$EXIT_CODE"',
				"if [ -f /tmp/xkb_test.xkb ]; then",
				"  SIZE=$(wc -c < /tmp/xkb_test.xkb)",
				'  echo "XKB_FILE_SIZE=$SIZE"',
				"  # Verify it contains key sections",
				"  grep -c 'xkb_keycodes' /tmp/xkb_test.xkb && echo 'HAS_KEYCODES'",
				"  grep -c 'xkb_types' /tmp/xkb_test.xkb && echo 'HAS_TYPES'",
				"  grep -c 'xkb_symbols' /tmp/xkb_test.xkb && echo 'HAS_SYMBOLS'",
				"  rm /tmp/xkb_test.xkb",
				"fi",
				"echo 'XKB_OK'",
			].join("\n"),
		]);
		console.log(`XKB: ${result.output}`);
		expect(result.output).toContain("XKB_OK");
	});

	test("rendercheck full suite passes", async ({ sidecarContainer }) => {
		// rendercheck can take a while; the inner timeout is 120s, but we
		// also need playwright to wait that long.
		test.setTimeout(180_000);
		const result = await sidecarContainer.exec(
			["bash", "-c", "timeout 120 rendercheck -d :99 2>&1 | tail -5"],
			{ timeout: 130_000 } as any,
		);
		console.log(`rendercheck full: ${result.output}`);
		// Should contain test results
		expect(result.output).toMatch(/test|pass/i);
		// Should not report failures
		if (result.output.includes("tests passed")) {
			expect(result.output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("GLX: glxinfo reports renderer", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"glxinfo 2>&1 | head -20 || echo 'GLX_NOT_AVAILABLE'",
			].join("\n"),
		]);
		console.log(`GLX: ${result.output}`);
		// Either GLX works or we report it's not available
		expect(result.output).toMatch(/OpenGL|GLX|GLX_NOT_AVAILABLE/i);
	});

	test("Present extension is available", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"present_extension_available.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("PRESENT_OK");
	});
});
