/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, runPythonScript, test } from "../fixtures";

test.describe("RandR output properties", () => {
	test("xrandr lists outputs with properties", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xrandr", "--verbose"]);
		expect(result.exitCode).toBe(0);
		expect(result.output).toMatch(/default connected/i);
	});
});

test.describe("Conformance: X11 protocol unit tests", () => {
	test("QueryPointer returns valid child and coordinates", async ({
		sidecarContainer,
	}) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"xeyes &",
				"PID=$!",
				"sleep 1",
				"python3 -c '",
				"import Xlib.display",
				'd = Xlib.display.Display(":99")',
				"root = d.screen().root",
				"r = root.query_pointer()",
				'print(f"ROOT_X={r.root_x}")',
				'print(f"ROOT_Y={r.root_y}")',
				'print(f"WIN_X={r.win_x}")',
				'print(f"WIN_Y={r.win_y}")',
				'print(f"SAME_SCREEN={r.same_screen}")',
				"# Verify coordinates are within screen bounds",
				'assert 0 <= r.root_x <= 4096, f"root_x out of range: {r.root_x}"',
				'assert 0 <= r.root_y <= 4096, f"root_y out of range: {r.root_y}"',
				'assert r.same_screen == 1, f"same_screen should be 1"',
				'print("QUERY_POINTER_OK")',
				"d.close()",
				"' 2>&1",
				"kill $PID 2>/dev/null",
			].join("\n"),
		]);
		console.log(`QueryPointer: ${result.output}`);
		expect(result.output).toContain("QUERY_POINTER_OK");
	});

	test("InternAtom and GetAtomName round-trip", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"internatom_getatomname_roundtrip.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Atom roundtrip: ${result.output}`);
		expect(result.output).toContain("ATOM_ROUNDTRIP_OK");
	});

	test("CreateWindow, MapWindow, GetWindowAttributes, DestroyWindow", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"createwindow_mapwindow_attributes_destroy.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Window lifecycle: ${result.output}`);
		expect(result.output).toContain("WINDOW_LIFECYCLE_OK");
	});

	test("ChangeProperty, GetProperty, DeleteProperty cycle", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"changeproperty_getproperty_deleteproperty_cycle.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Property cycle: ${result.output}`);
		expect(result.output).toContain("PROPERTY_CYCLE_OK");
	});

	test("GC creation, drawing operations, and GetImage", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"gc_drawing_getimage.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Drawing ops: ${result.output}`);
		expect(result.output).toContain("DRAWING_OPS_OK");
	});

	test("Selection transfer (copy/paste) between two clients", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"selection_transfer_two_clients.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Selection: ${result.output}`);
		expect(result.output).toContain("SELECTION_OWNER_OK");
	});

	test("ConfigureWindow changes geometry and sends ConfigureNotify", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"configurewindow_geometry_notify.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Configure: ${result.output}`);
		expect(result.output).toContain("CONFIGURE_OK");
	});

	test("GrabPointer and UngrabPointer", async ({ sidecarContainer }) => {
		const result = await runPythonScript(
			sidecarContainer,
			"grabpointer_ungrabpointer_protocol.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Grab: ${result.output}`);
		expect(result.output).toContain("GRAB_OK");
	});

	test("FocusIn and FocusOut events are delivered", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"focusin_focusout_delivery.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Focus events: ${result.output}`);
		expect(result.output).toContain("FOCUS_EVENTS_OK");
	});

	test("Colormap operations: AllocColor, QueryColors", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"colormap_alloccolor_querycolors.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Colormap: ${result.output}`);
		expect(result.output).toContain("COLORMAP_OK");
	});

	test("RandR GetScreenResources returns valid data", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"randr_getscreenresources.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`RandR: ${result.output}`);
		expect(result.output).toContain("RANDR_OK");
	});

	test("EWMH _NET_SUPPORTED reports required atoms", async ({
		sidecarContainer,
	}) => {
		const result = await runPythonScript(
			sidecarContainer,
			"ewmh_net_supported_atoms.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`EWMH: ${result.output}`);
		expect(result.output).toContain("EWMH_OK");
	});
});
