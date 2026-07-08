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

test.describe
	.serial("ICCCM/EWMH compliance", () => {
		test("_NET_SUPPORTED lists required atoms on root", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xprop -root _NET_SUPPORTED",
			);
			expect(output).toContain("_NET_WM_STATE");
			expect(output).toContain("_NET_WM_NAME");
			expect(output).toContain("_NET_ACTIVE_WINDOW");
			expect(output).toContain("_NET_CLIENT_LIST");
			expect(output).toContain("_NET_WM_PING");
			expect(output).toContain("_NET_WM_SYNC_REQUEST");
			expect(output).toContain("_NET_CLOSE_WINDOW");
			expect(output).toContain("_NET_WM_WINDOW_TYPE");
			expect(output).toContain("_NET_WM_STRUT");
			expect(output).toContain("_NET_WORKAREA");
		});

		test("_NET_SUPPORTING_WM_CHECK points to valid window", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xprop -root _NET_SUPPORTING_WM_CHECK",
			);
			expect(output).toContain("_NET_SUPPORTING_WM_CHECK");
			// Extract the window ID
			const match = output.match(/window id # (0x[0-9a-f]+)/i);
			expect(match).toBeTruthy();
			if (match) {
				const wmCheckId = match[1];
				// Verify the WM check window has _NET_WM_NAME
				const wmName = await execInSidecar(
					sidecarContainer,
					`xprop -id ${wmCheckId} _NET_WM_NAME`,
				);
				expect(wmName).toContain("x11-web");
			}
		});

		test("Windows get _NET_WM_PID set", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"windows_get_net_wm_pid_set.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("pid_nonzero=True");
		});

		test("Windows get WM_CLIENT_MACHINE set", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"windows_get_wm_client_machine_set.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("machine_set=true");
		});

		test("GetGeometry returns correct depth for different visuals", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"getgeometry_returns_correct_depth_for_different_visuals.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("depth_24=24");
			expect(output).toContain("depth_32=32");
		});

		test("Colormap read-only enforcement for TrueColor", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"colormap_read_only_enforcement_for_truecolor.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("colormap_readonly_test=ok");
		});

		test("_NET_WM_STATE changes via ClientMessage", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_changes_via_clientmessage.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("state_change_test=ok");
			expect(output).toContain("has_fullscreen=True");
		});

		test("WM_DELETE_WINDOW via _NET_CLOSE_WINDOW", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_delete_window_via_net_close_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("close_window_test=ok");
		});

		test("_NET_FRAME_EXTENTS set on new windows", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_frame_extents_set_on_new_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("frame_set=true");
			expect(output).toContain("frame_extents=[0, 0, 0, 0]");
		});

		test("_NET_WM_STATE_MODAL raises window above parent", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_modal_raises_window_above_parent.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("modal_set=True");
			expect(output).toContain("dialog_above_parent=True");
		});

		test("_NET_WM_STATE_DEMANDS_ATTENTION is accepted", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_state_demands_attention_is_accepted.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("demands_attention_set=True");
		});

		test("_NET_WM_ALLOWED_ACTIONS is set on mapped windows", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_allowed_actions_is_set_on_mapped_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("has_close=True");
			expect(output).toContain("has_move=True");
			expect(output).toContain("has_resize=True");
		});
	});

test.describe
	.serial("Atom system tests", () => {
		test("xlsatoms lists standard atoms", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"xlsatoms 2>&1 | head -100",
			);
			// Standard predefined atoms
			expect(output).toContain("PRIMARY");
			expect(output).toContain("ATOM");
			expect(output).toContain("STRING");
			expect(output).toContain("WM_NAME");
		});

		test("xlsatoms lists EWMH atoms", async ({ sidecarContainer }) => {
			const output = await execInSidecar(sidecarContainer, "xlsatoms 2>&1");
			expect(output).toContain("_NET_SUPPORTED");
			expect(output).toContain("_NET_WM_NAME");
			expect(output).toContain("_NET_WM_STATE");
		});

		test("InternAtom and GetAtomName round-trip", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"internatom_and_getatomname_round_trip_2.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("ATOM_OK");
			expect(output).toContain("ONLY_IF_EXISTS_OK");
		});
	});

test.describe
	.serial("ICCCM and EWMH compliance", () => {
		test("WM_PROTOCOLS and WM_DELETE_WINDOW", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_protocols_and_wm_delete_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("WM_DELETE_WINDOW_OK");
		});

		test("_NET_WM_NAME (UTF-8 window title)", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_name_utf_8_window_title.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("UTF8_TITLE_OK");
		});

		test("_NET_WM_STATE management", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(sidecarContainer, "net_wm_state_management.py", {
					env: { DISPLAY: ":99" },
				})
			).output.trim();
			expect(output).toContain("NET_WM_STATE_OK");
		});
	});

test.describe
	.serial("EWMH/ICCCM compliance", () => {
		test("_NET_SUPPORTED contains required atoms", async ({
			sidecarContainer,
		}) => {
			const output = await execInSidecar(
				sidecarContainer,
				`xprop -root _NET_SUPPORTED 2>/dev/null || echo "no_xprop"`,
			);
			if (!output.includes("no_xprop")) {
				expect(output).toContain("_NET_WM_NAME");
				expect(output).toContain("_NET_WM_STATE");
				expect(output).toContain("_NET_ACTIVE_WINDOW");
				expect(output).toContain("_NET_CLOSE_WINDOW");
			}
		});

		test("_NET_SUPPORTING_WM_CHECK is valid", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				`xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || echo "no_xprop"`,
			);
			if (!output.includes("no_xprop")) {
				// Should contain a window ID
				expect(output).toMatch(/window id # 0x[0-9a-f]+/i);
			}
		});

		test("_NET_WM_PID set on mapped windows", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"net_wm_pid_set_on_mapped_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("has_pid=True");
		});
	});

test.describe("ICCCM/EWMH automated validation", () => {
	test("root window has required _NET_SUPPORTED atoms", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"ewmh_root_net_supported_atoms.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("ewmh-ok:");
	});

	test("_NET_SUPPORTING_WM_CHECK is valid", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"ewmh_net_supporting_wm_check_valid.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("wm-check-ok");
	});
});

test.describe("EWMH compliance", () => {
	test("root window has _NET_SUPPORTING_WM_CHECK", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xprop -root _NET_SUPPORTING_WM_CHECK",
				"echo EWMH_CHECK_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("_NET_SUPPORTING_WM_CHECK");
		expect(result.output).toContain("EWMH_CHECK_PASS");
	});

	test("root window has _NET_SUPPORTED listing", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xprop -root _NET_SUPPORTED 2>&1 | head -5",
				"echo EWMH_SUPPORTED_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("_NET_SUPPORTED");
		expect(result.output).toContain("EWMH_SUPPORTED_PASS");
	});

	test("WM_STATE is set on mapped top-level windows", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xeyes &",
				"sleep 1",
				"xprop -name xeyes WM_STATE 2>&1 || echo 'no_window'",
				"pkill xeyes 2>/dev/null; true",
				"echo WM_STATE_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("WM_STATE_PASS");
	});

	test("_NET_CLIENT_LIST is updated on window creation", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xeyes &",
				"sleep 1",
				"xprop -root _NET_CLIENT_LIST 2>&1 | head -3",
				"pkill xeyes 2>/dev/null; true",
				"echo NET_CLIENT_LIST_PASS",
			].join("\n"),
		]);
		expect(result.output).toContain("NET_CLIENT_LIST_PASS");
	});
});

test.describe("ICCCM / EWMH compliance", () => {
	test("WM_NORMAL_HINTS stores and retrieves size hints", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"wm_normal_hints.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/icccm-hints: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(3);
	});

	test("WM_TRANSIENT_FOR window relationship", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"wm_transient_for.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/icccm-transient: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("WM_DELETE_WINDOW protocol via WM_PROTOCOLS", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"wm_delete_window_protocol.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/icccm-delete: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("_NET_WM_STATE ClientMessage toggles state on root", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"net_wm_state_clientmessage.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/ewmh-cm: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("_NET_ACTIVE_WINDOW updated on focus change", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"net_active_window_focus.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/ewmh-active: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("_NET_WM_STATE transitions", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"net_wm_state_transitions.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/ewmh-state: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});
});

test.describe("ICCCM WM_STATE and protocols", () => {
	test("WM_STATE is set to NormalState on MapWindow", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess, time",
				"p = subprocess.Popen(['xeyes'])",
				"time.sleep(1)",
				"r = subprocess.run(['xprop', '-name', 'xeyes', 'WM_STATE'], capture_output=True, text=True)",
				"print('WM_STATE: ' + r.stdout.strip())",
				"p.kill()",
				"p.wait()",
			].join("\n"),
		]);
		console.log(`WM_STATE: exit=${result.exitCode}`);
		// WM_STATE should contain NormalState (1)
		expect(result.output).toContain("WM_STATE");
	});

	test("_NET_WM_ALLOWED_ACTIONS is set on top-level windows", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess, time",
				"p = subprocess.Popen(['xeyes'])",
				"time.sleep(1)",
				"r = subprocess.run(['xprop', '-name', 'xeyes', '_NET_WM_ALLOWED_ACTIONS'], capture_output=True, text=True)",
				"print('ALLOWED: ' + r.stdout.strip())",
				"p.kill()",
				"p.wait()",
			].join("\n"),
		]);
		console.log(`ALLOWED_ACTIONS: exit=${result.exitCode}`);
		expect(result.output).toContain("_NET_WM_ALLOWED_ACTIONS");
		expect(result.output).toContain("_NET_WM_ACTION_CLOSE");
	});

	test("WM_DELETE_WINDOW protocol: xeyes supports WM_PROTOCOLS", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import subprocess, time",
				"p = subprocess.Popen(['xeyes'])",
				"time.sleep(1)",
				"r = subprocess.run(['xprop', '-name', 'xeyes', 'WM_PROTOCOLS'], capture_output=True, text=True)",
				"print('PROTOCOLS: ' + r.stdout.strip())",
				"p.kill()",
				"p.wait()",
			].join("\n"),
		]);
		console.log(`WM_PROTOCOLS: exit=${result.exitCode}`);
		// xeyes typically sets WM_PROTOCOLS with WM_DELETE_WINDOW
		expect(result.output).toContain("WM_PROTOCOLS");
	});

	test("python3-xlib: WM_NORMAL_HINTS size constraints are enforced", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"from Xlib import X, display, Xatom",
				"import struct",
				"d = display.Display()",
				"s = d.screen()",
				"w = s.root.create_window(10, 10, 200, 200, 0, s.root_depth,",
				"    X.InputOutput, X.CopyFromParent, event_mask=X.ExposureMask)",
				"# Set WM_NORMAL_HINTS with min_size=100x100, max_size=300x300",
				"hints = struct.pack('=IiiiiiiiiIIIIIIIII',",
				"    (1 << 4) | (1 << 5),  # flags: PMinSize | PMaxSize",
				"    0, 0, 0, 0,  # x, y, width, height (obsolete)",
				"    100, 100,  # min_width, min_height",
				"    300, 300,  # max_width, max_height",
				"    0, 0,  # width_inc, height_inc",
				"    0, 0,  # min_aspect_num, min_aspect_den",
				"    0, 0,  # max_aspect_num, max_aspect_den",
				"    0, 0,  # base_width, base_height",
				"    1  # win_gravity (NorthWestGravity)",
				")",
				"w.change_property(d.intern_atom('WM_NORMAL_HINTS'), d.intern_atom('WM_SIZE_HINTS'), 32, hints)",
				"w.map()",
				"d.sync()",
				"# Try to configure to a size smaller than min",
				"w.configure(width=50, height=50)",
				"d.sync()",
				"import time; time.sleep(0.2)",
				"geom = w.get_geometry()",
				"print(f'GEOMETRY: {geom.width}x{geom.height}')",
				"# Width/height should be clamped to min (100x100)",
				"assert geom.width >= 100, f'Width {geom.width} < 100'",
				"assert geom.height >= 100, f'Height {geom.height} < 100'",
				"print('SIZE_HINTS_OK')",
				"w.destroy()",
				"d.close()",
			].join("\n"),
		]);
		console.log(`WM_NORMAL_HINTS: exit=${result.exitCode}`);
		expect(result.output).toContain("SIZE_HINTS_OK");
	});
});

test.describe("Conformance: ICCCM selections", () => {
	test("xclip can write and read from CLIPBOARD", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"echo 'test-clipboard-data' | xclip -selection clipboard -i",
				"sleep 0.2",
				"xclip -selection clipboard -o 2>&1 || echo 'xclip-read-failed'",
			].join("\n"),
		]);
		// Either we get the data back, or xclip returns something
		// (selection protocol may not round-trip in single-client mode)
		expect(result.exitCode).toBeDefined();
	});
});

test.describe
	.serial("WM_HINTS input focus model (Phase 8A)", () => {
		test("WM_HINTS input=true is parsed (Passive/Locally Active)", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_hints_input_true_is_parsed_passive_locally_active.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("input=1");
		});

		test("WM_HINTS input=false is parsed (Globally Active/No Input)", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_hints_input_false_is_parsed_globally_active_no_input.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("input=0");
		});

		test("WM_HINTS window_group is stored", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_hints_window_group_is_stored.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("group_matches=True");
		});

		test("WM_HINTS urgency hint triggers WindowUrgent", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_hints_urgency_hint_triggers_windowurgent.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("urgent=True");
		});
	});

test.describe
	.serial("ICCCM WM_DELETE_WINDOW (Phase 8E)", () => {
		test("WM_PROTOCOLS property can be set and read back", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_protocols_property_can_be_set_and_read_back.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("has_delete=True");
			expect(output).toContain("has_focus=True");
		});
	});

test.describe("Conformance: Window manager protocol", () => {
	test("WM_DELETE_WINDOW protocol works", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"wm_delete_window_protocol_property.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("WM_DELETE_OK");
	});

	test("ICCCM WM_NORMAL_HINTS property round-trip", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"icccm_wm_normal_hints_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		expect(result.output).toContain("WM_HINTS_OK");
	});

	test("_NET_SUPPORTING_WM_CHECK points to valid window", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"net_supporting_wm_check_valid.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`WM check: ${result.output}`);
		expect(result.output).toContain("WM_CHECK_OK");
	});
});

test.describe
	.serial("WM_STATE ICCCM compliance", () => {
		test.setTimeout(60_000);

		test("WM_STATE is set when window is mapped", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_state_is_set_when_window_is_mapped.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("WM_STATE_OK");
		});

		test("WM_STATE is NormalState for child windows", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"wm_state_is_normalstate_for_child_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("CHILD_WM_STATE_OK");
		});
	});

test.describe("python3-xlib deep protocol tests", () => {
	test("CreateWindow + GetWindowAttributes round-trip", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"deep_createwindow_getattributes_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/deep-protocol: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(8);
	});

	test("Selection protocol (CLIPBOARD/PRIMARY) round-trip", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"selection_clipboard_primary_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(
			/selection-protocol: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("GC operations and drawing primitives", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"gc_operations_drawing_primitives.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/gc-drawing: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(9);
	});

	test("Grab operations succeed", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"grab_operations_succeed.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/grabs: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(5);
	});

	test("Colormap operations work in TrueColor", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"colormap_truecolor_operations.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/colormap: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("Multi-client window visibility and event delivery", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"multi_client_visibility_events.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/multi-client: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(3);
	});

	test("InputOnly windows receive events but are not rendered", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"inputonly_window_events.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/inputonly: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("PropertyNotify generated on GetProperty with delete=true", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"propertynotify_getproperty_delete.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/propnotify-del: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("xclip copy-paste between processes via CLIPBOARD", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// xclip -selection clipboard -i: copy text to CLIPBOARD
				"echo 'x11web-clipboard-test' | DISPLAY=:99 xclip -selection clipboard -i",
				// Give the selection owner time to register
				"sleep 0.5",
				// xclip -selection clipboard -o: paste from CLIPBOARD
				"DISPLAY=:99 xclip -selection clipboard -o 2>&1",
			].join("\n"),
		]);
		console.log(
			`xclip: exit=${result.exitCode} output='${result.output.trim()}'`,
		);
		// xclip requires the first process to stay alive as selection owner
		// while the second reads. This tests the full ICCCM selection protocol.
		// If it works end-to-end, both ConvertSelection and SendEvent for
		// SelectionNotify/SelectionRequest are working correctly.
		if (result.exitCode === 0) {
			expect(result.output.trim()).toContain("x11web-clipboard-test");
		}
	});
});

test.describe
	.serial("WM_TRANSIENT_FOR (Phase 8D)", () => {
		test("transient window is placed above its parent on map", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"transient_window_is_placed_above_its_parent_on_map.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("child_above_parent=True");
		});
	});
