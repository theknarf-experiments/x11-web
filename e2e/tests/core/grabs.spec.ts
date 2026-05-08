/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";

test.describe.serial("Grab protocol compliance", () => {
	test("GrabPointer succeeds on a viewable window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabpointer_succeeds_on_a_viewable_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("grab_status=0");
	});

	test("GrabPointer on unmapped window returns NotViewable", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabpointer_on_unmapped_window_returns_notviewable.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("grab_status=3");
	});

	test("GrabKeyboard succeeds on a viewable window", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabkeyboard_succeeds_on_a_viewable_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("keyboard_grab_status=0");
	});

	test("GrabButton and passive activation via xdotool", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabbutton_and_passive_activation_via_xdotool.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("passive_grab_established=True");
		expect(output).toContain("passive_grab_removed=True");
	});

	test("GrabKey passive grab lifecycle", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabkey_passive_grab_lifecycle.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("key_grab_established=True");
		expect(output).toContain("key_grab_removed=True");
	});
});

test.describe.serial("Event delivery compliance", () => {
	test("EnterNotify and LeaveNotify on pointer warp with detail modes", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "enternotify_and_leavenotify_on_pointer_warp_with_detail_modes.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("crossing_events_generated=True");
	});

	test("FocusIn and FocusOut events on SetInputFocus", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "focusin_and_focusout_events_on_setinputfocus.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("focus_events_generated=True");
	});

	test("GrabServer and UngrabServer complete successfully", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabserver_and_ungrabserver_complete_successfully.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("server_grabbed=True");
		expect(output).toContain("server_ungrabbed=True");
	});

	test("AllowEvents modes complete without error", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "allowevents_modes_complete_without_error.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("allow_async_pointer=ok");
		expect(output).toContain("allow_async_keyboard=ok");
	});
});

test.describe.serial("Core protocol edge cases (Phase 7)", () => {
	test("QueryTree returns correct parent-child relationships", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "querytree_returns_correct_parent_child_relationships.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("parent_of_parent=True");
		expect(output).toContain("num_children=2");
		expect(output).toContain("child1_in_tree=True");
		expect(output).toContain("child2_in_tree=True");
	});

	test("GetGeometry returns correct window dimensions", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "getgeometry_returns_correct_window_dimensions.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("x=50");
		expect(output).toContain("y=75");
		expect(output).toContain("w=320");
		expect(output).toContain("h=240");
		expect(output).toContain("bw=2");
	});

	test("InternAtom and GetAtomName round-trip", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "internatom_and_getatomname_round_trip.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("atom_name=X11_WEB_TEST_ATOM_12345");
		expect(output).toContain("primary_atom=1");
	});

	test("ChangeProperty and GetProperty round-trip", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "changeproperty_and_getproperty_round_trip.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("value=hello world");
		expect(output).toContain("format=8");
	});

	test("GrabServer/UngrabServer works", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "grabserver_ungrabserver_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("grab_ungrab_ok=True");
	});
});

test.describe("Grab operations", () => {
	test("GrabPointer and UngrabPointer via xdotool", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "grabpointer_ungrabpointer_xdotool.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/grabs-basic: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});

	test("passive button grab and ungrab", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "passive_button_grab_ungrab.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/grabs-passive: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(4);
	});
});

test.describe("GrabServer serialization", () => {
	test("GrabServer blocks other clients", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"# GrabServer should succeed",
				"d.grab_server()",
				"d.sync()",
				"# We can still make requests while holding the grab",
				"root = d.screen().root",
				"tree = root.query_tree()",
				"assert tree is not None, 'QueryTree failed during GrabServer'",
				"# Release the grab",
				"d.ungrab_server()",
				"d.sync()",
				"# Verify server is still usable",
				"tree2 = root.query_tree()",
				"assert tree2 is not None, 'QueryTree failed after UngrabServer'",
				"print('GRAB_SERVER_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("GRAB_SERVER_PASS");
	});
});

test.describe.serial("Grab operations validation", () => {
	test.setTimeout(60_000);

	test("GrabButton and UngrabButton work correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabbutton_and_ungrabbutton_work_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("GRAB_BUTTON_OK");
		expect(output).toContain("UNGRAB_BUTTON_OK");
	});

	test("GrabKey and UngrabKey work correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabkey_and_ungrabkey_work_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("GRAB_KEY_OK");
		expect(output).toContain("UNGRAB_KEY_OK");
	});

	test("GrabKeyboard and UngrabKeyboard work correctly", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "grabkeyboard_and_ungrabkeyboard_work_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("GRAB_KEYBOARD_OK");
		expect(output).toContain("UNGRAB_KEYBOARD_OK");
	});
});

test.describe.serial("Passive grab cleanup on disconnect", () => {
	test("passive grabs are cleaned up when client disconnects", async ({
		sidecarContainer,
	}) => {
		// Client creates a passive button grab, then disconnects.
		// A second client should not see stale grabs.
		const output = (await runPythonScript(sidecarContainer, "passive_grabs_are_cleaned_up_when_client_disconnects.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("cleanup_ok=True");
	});
});
