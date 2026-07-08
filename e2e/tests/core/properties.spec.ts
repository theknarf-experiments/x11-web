/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, runPythonScript, test } from "../fixtures";

test.describe
	.serial("Event delivery edge cases", () => {
		test("PropertyNotify events delivered on property changes", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"propertynotify_events_delivered_on_property_changes.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			// Should have at least 1 PropertyNotify
			const count = Number.parseInt(
				output.match(/property_notify_count=(\d+)/)?.[1] ?? "0",
			);
			expect(count).toBeGreaterThanOrEqual(1);
		});

		test("SubstructureRedirectMask generates ConfigureRequest", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"substructureredirectmask_generates_configurerequest.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("got_map_request=True");
		});

		test("Focus revert to parent on destroy", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"focus_revert_to_parent_on_destroy.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("focus_before=");
			// Focus should revert to parent (or root if parent got cleaned up)
		});
	});

test.describe("Protocol compliance: xprop", () => {
	test("xprop can set and retrieve a custom property", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"# Get root window ID",
				"ROOT=$(xdpyinfo 2>/dev/null | grep 'root window id:' | awk '{print $NF}')",
				'if [ -z "$ROOT" ]; then ROOT=0x1; fi',
				"# Set a custom property on root",
				"xprop -root -f _X11WEB_TEST 8s -set _X11WEB_TEST 'hello_from_e2e' 2>&1",
				"# Read it back",
				"VALUE=$(xprop -root _X11WEB_TEST 2>&1)",
				"if echo \"$VALUE\" | grep -q 'hello_from_e2e'; then",
				"  echo 'XPROP_PASS: round-trip successful'",
				"else",
				"  echo \"XPROP_FAIL: got '$VALUE'\"",
				"fi",
				"# Clean up",
				"xprop -root -remove _X11WEB_TEST 2>/dev/null || true",
			].join("\n"),
		]);
		expect(result.output).toContain("XPROP_PASS");
	});
});

test.describe("Property operations", () => {
	test("ChangeProperty + GetProperty + RotateProperties", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display, Xlib.X, Xlib.Xatom",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"w = root.create_window(0, 0, 50, 50, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"# Set two custom properties",
				"a1 = d.intern_atom('_TEST_PROP_A')",
				"a2 = d.intern_atom('_TEST_PROP_B')",
				"w.change_property(a1, Xlib.Xatom.STRING, 8, b'hello')",
				"w.change_property(a2, Xlib.Xatom.STRING, 8, b'world')",
				"d.sync()",
				"# Read them back",
				"p1 = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"p2 = w.get_property(a2, Xlib.Xatom.STRING, 0, 100)",
				"assert bytes(p1.value) == b'hello', f'Prop A mismatch: {p1.value}'",
				"assert bytes(p2.value) == b'world', f'Prop B mismatch: {p2.value}'",
				"# ListProperties",
				"props = w.list_properties()",
				"assert a1 in props, 'Missing _TEST_PROP_A in ListProperties'",
				"assert a2 in props, 'Missing _TEST_PROP_B in ListProperties'",
				"# RotateProperties",
				"w.rotate_properties([a1, a2], 1)",
				"d.sync()",
				"p1_after = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"p2_after = w.get_property(a2, Xlib.Xatom.STRING, 0, 100)",
				"# After rotating by 1, a1 should have the value that was in a2",
				"assert bytes(p1_after.value) == b'world', f'After rotate, A={p1_after.value}'",
				"assert bytes(p2_after.value) == b'hello', f'After rotate, B={p2_after.value}'",
				"# DeleteProperty",
				"w.delete_property(a1)",
				"d.sync()",
				"p1_del = w.get_property(a1, Xlib.Xatom.STRING, 0, 100)",
				"assert p1_del is None or p1_del.property_type == 0, 'Property not deleted'",
				"w.destroy()",
				"d.sync()",
				"print('PROPERTY_OPS_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("PROPERTY_OPS_PASS");
	});
});

test.describe("Orphan: python3-xlib smoke tests", () => {
	test("python3-xlib can connect and query the server", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"python_xlib_connect_query.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`python-xlib: ${result.output.trim()}`);
		expect(result.output).toContain("PYTHON_XLIB_OK");
		expect(result.output).toContain("1024x768");
	});

	test("python3-xlib can create and destroy windows", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"python_xlib_window_lifecycle.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`python-xlib window: ${result.output.trim()}`);
		expect(result.output).toContain("WINDOW_LIFECYCLE_OK");
		expect(result.output).toContain("100x100");
	});

	test("python3-xlib can get/set properties", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"python_xlib_get_set_properties.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`python-xlib property: ${result.output.trim()}`);
		expect(result.output).toContain("PROPERTY_OK");
		expect(result.output).toContain("hello world");
	});

	test("python3-xlib can query extensions", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"python_xlib_query_extensions.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`python-xlib extensions: exit=${result.exitCode}`);
		expect(result.output).toContain("EXTENSIONS_OK");
	});
});
