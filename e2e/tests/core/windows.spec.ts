/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, runPythonScript, test } from "../fixtures";

test.describe
	.serial("Advanced event delivery", () => {
		test("Enter/Leave events generated on pointer warp", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"enter_leave_events_generated_on_pointer_warp.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("got_enter=True");
		});

		test("FocusIn/FocusOut events on SetInputFocus", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"focusin_focusout_events_on_setinputfocus.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("focus_w1=True");
			expect(output).toContain("focus_w2=True");
			expect(output).toContain("got_focus_in=True");
		});

		test("ConfigureNotify on sibling stacking change", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"configurenotify_on_sibling_stacking_change.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("got_configure_notify=True");
		});
	});

test.describe("Bounds checking", () => {
	test("CreateWindow rejects zero dimensions", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"createwindow_rejects_zero_dimensions.py",
			{ env: { DISPLAY: ":99" } },
		);
		console.log(`Bounds checking: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});

test.describe
	.serial("SubstructureRedirect compliance", () => {
		test.setTimeout(60_000);

		test("SubstructureRedirectMask can be set on non-root parent", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"substructureredirectmask_can_be_set_on_non_root_parent.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("CHILD_MAP_OK");
		});

		test("Override redirect window bypasses SubstructureRedirect", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"override_redirect_window_bypasses_substructureredirect.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("OR_MAP_OK");
		});

		test("ConfigureWindow works on override-redirect windows", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"configurewindow_works_on_override_redirect_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("OR_CONFIGURE_OK");
		});
	});

test.describe
	.serial("Window hierarchy events", () => {
		test.setTimeout(60_000);

		test("StructureNotifyMask delivers MapNotify", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"structurenotifymask_delivers_mapnotify.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("MAP_NOTIFY_OK");
		});

		test("SubstructureNotifyMask delivers CreateNotify", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"substructurenotifymask_delivers_createnotify.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("CREATE_NOTIFY_OK");
		});

		test("DestroyNotify delivered when window destroyed", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"destroynotify_delivered_when_window_destroyed.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("DESTROY_NOTIFY_OK");
		});

		test("ConfigureNotify sent after ConfigureWindow", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"configurenotify_sent_after_configurewindow.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("CONFIGURE_NOTIFY_OK");
		});

		test("ReparentNotify sent on reparent", async ({ sidecarContainer }) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"reparentnotify_sent_on_reparent.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("REPARENT_NOTIFY_OK");
		});
	});

test.describe("Window geometry", () => {
	test("GetGeometry and TranslateCoordinates round-trip", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3",
			"-c",
			[
				"import Xlib, Xlib.display, Xlib.X",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"root = screen.root",
				"# Create parent at (50, 50) size 200x200",
				"parent = root.create_window(50, 50, 200, 200, 0, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"# Create child at (10, 10) relative to parent",
				"child = parent.create_window(10, 10, 50, 50, 2, screen.root_depth,",
				"    Xlib.X.InputOutput, Xlib.X.CopyFromParent)",
				"parent.map()",
				"child.map()",
				"d.sync()",
				"# GetGeometry on child",
				"geo = child.get_geometry()",
				"assert geo.x == 10, f'Child x={geo.x}'",
				"assert geo.y == 10, f'Child y={geo.y}'",
				"assert geo.width == 50, f'Child width={geo.width}'",
				"assert geo.height == 50, f'Child height={geo.height}'",
				"assert geo.border_width == 2, f'Child border={geo.border_width}'",
				"# TranslateCoordinates: child (0,0) -> root coords",
				"tc = d.screen().root.translate_coords(child, 0, 0)",
				"# Should be approximately (50+10+2, 50+10+2) = (62, 62)",
				"# (border_width adds to the offset)",
				"print(f'translate=({tc.x},{tc.y})')",
				"assert tc.x >= 50, f'Translated x too small: {tc.x}'",
				"assert tc.y >= 50, f'Translated y too small: {tc.y}'",
				"child.destroy()",
				"parent.destroy()",
				"d.sync()",
				"print('GEOMETRY_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("GEOMETRY_PASS");
	});
});

test.describe
	.serial("Input handling edge cases", () => {
		test("QueryPointer returns valid coordinates", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"querypointer_returns_valid_coordinates.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("pointer_ok=True");
		});

		test("TranslateCoordinates works between windows", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"translatecoordinates_works_between_windows.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("translate_ok=True");
		});
	});
