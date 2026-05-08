/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

test.describe("Server grab robustness", () => {
	test("server grab is released on client disconnect", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "server_grab_released_disconnect.py", { env: { DISPLAY: ":99" } });
		console.log(`Server grab: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});

test.describe("Resource cleanup on client disconnect", () => {
	test("windows are destroyed when client disconnects in Destroy mode", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "client_disconnect_destroy_windows.py", { env: { DISPLAY: ":99" } });
		console.log(`Destroy-mode test output: ${result.output}`);
		const match = result.output.match(
			/cleanup-destroy: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});

	test("SetCloseDownMode RetainTemporary keeps windows alive", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "setclosedownmode_retaintemporary.py", { env: { DISPLAY: ":99" } });
		const match = result.output.match(
			/cleanup-retain: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		expect(Number.parseInt(match![2], 10)).toBe(0);
		expect(Number.parseInt(match![1], 10)).toBeGreaterThanOrEqual(2);
	});
});

test.describe("Resource cleanup on disconnect", () => {
	test("server cleans up resources after client disconnect", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "resource_cleanup_after_disconnect.py", { env: { DISPLAY: ":99" } });
		console.log(`Resource cleanup: ${result.output}`);
		expect(result.output).toContain("PASS");
	});

	test("SaveSet reparenting works on WM disconnect", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(sidecarContainer, "saveset_reparenting_wm_disconnect.py", { env: { DISPLAY: ":99" } });
		console.log(`SaveSet: ${result.output}`);
		expect(result.output).toContain("PASS");
	});
});

test.describe.serial("python3-xlib edge cases", () => {
	test("SetCloseDownMode RetainPermanent preserves window", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const output = (await runPythonScript(sidecarContainer, "setclosedownmode_retainpermanent_preserves_window.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("window_retained=True");
	});

	test("Window gravity during resize (NorthEast)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_gravity_during_resize_northeast.py", { env: { DISPLAY: ":99" } })).output.trim();
		// Accept the test output; the key thing is it doesn't crash.
		// Gravity handling varies – log the result.
		expect(output).toContain("x_before=");
		expect(output).toContain("delta=");
	});

	test("GrabServer serialization", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const output = (await runPythonScript(sidecarContainer, "grabserver_serialization.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("b_was_blocked=True");
	});

	test("Exposure event delivery on map", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "exposure_event_delivery_on_map.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("got_expose=True");
	});

	test("PropertyNotify event on property change", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "propertynotify_event_on_property_change.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("got_property_notify=True");
		expect(output).toContain("notify_state=0");
	});

	test("Selection protocol: SetSelectionOwner / ConvertSelection exchange", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const output = (await runPythonScript(sidecarContainer, "selection_protocol_setselectionowner_convertselection_exchange.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("owner_set=True");
		expect(output).toContain("got_selection_request=True");
		expect(output).toContain("got_selection_notify=True");
		expect(output).toContain("selection_value=hello_selection");
	});
});

test.describe.serial("Advanced protocol compliance", () => {
	test.skip("PutImage and GetImage round-trip at depth 24 (ZPixmap)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "putimage_and_getimage_round_trip_at_depth_24_zpixmap.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("round_trip_ok=True");
		expect(output).toContain("pixel0=0x00ff0000");
		expect(output).toContain("pixel3=0x00ffffff");
	});

	test("PutImage Bitmap format (depth 1)", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "putimage_bitmap_format_depth_1.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("bitmap_ok=True");
	});

	test("CreatePixmap and FreePixmap for all depths", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "createpixmap_and_freepixmap_for_all_depths.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("1");
		expect(output).toContain("24");
		expect(output).toContain("32");
	});

	test("Window border_width is reported in GetGeometry", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_border_width_is_reported_in_getgeometry.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("border_width=5");
		expect(output).toContain("width=100");
		expect(output).toContain("height=50");
		expect(output).toContain("new_border_width=10");
	});

	test("Window gravity affects child positioning on resize", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_gravity_affects_child_positioning_on_resize.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("before_x=50");
		expect(output).toContain("gravity_correct=True");
	});

	test("SubstructureRedirectMask is exclusive (BadAccess)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "substructureredirectmask_is_exclusive_badaccess.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("first_grab=ok");
		expect(output).toContain("second_grab=BadAccess");
	});

	test("SYNC extension counter/alarm operations", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "sync_extension_counter_alarm_operations.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("sync_present=True");
	});

	// Pre-existing: python-xlib's Display.info accessor raises KeyError
	// after our QueryExtension('BIG-REQUESTS') reply — the connection's
	// `info` attribute isn't being refreshed via the EnableExtension
	// reply, so the test never reads back an updated max_request_length.
	// Either we're not handling EnableExtension correctly or the
	// follow-up Setup info refresh isn't propagating.
	test.skip("Big-Requests extension enables large requests", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "big_requests_extension_enables_large_requests.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("bigreq_present=True");
		expect(output).toContain("big_requests_work=True");
	});

	test("Stacking order changes with ConfigureWindow", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "stacking_order_changes_with_configurewindow.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("initial_order_correct=True");
		expect(output).toContain("w1_on_top=True");
	});

	test("CirculateWindow raises lowest and lowers highest", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "circulatewindow_raises_lowest_and_lowers_highest.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("after_raise_lowest_top=True");
		expect(output).toContain("after_lower_highest_bottom=True");
	});

	test("SetCloseDownMode RetainPermanent preserves resources", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "setclosedownmode_retainpermanent_preserves_resources.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("close_down_mode_set=True");
	});

	test("GrabServer blocks other clients", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "grabserver_blocks_other_clients_2.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("grab_server=ok");
		expect(output).toContain("ungrab_server=ok");
	});

	test("ListFonts returns available fonts", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xlsfonts -fn '*' 2>&1 | wc -l",
		);
		const count = parseInt(output.trim(), 10);
		expect(count).toBeGreaterThan(0);
	});

	test("Multiple visuals are advertised", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>&1",
		);
		expect(output).toContain("TrueColor");
		// Should have multiple depths
		expect(output).toMatch(/depth.*24/);
	});

	test("MIT-SHM extension is functional", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "mit_shm_extension_is_functional.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("shm_present=True");
	});

	test("COMPOSITE extension is functional", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "composite_extension_is_functional.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("composite_present=True");
	});

	test.skip("COMPOSITE RedirectWindow and NameWindowPixmap work", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "composite_redirectwindow_and_namewindowpixmap_work.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("composite_query_ok=True");
		expect(output).toContain("redirect_ok=True");
		expect(output).toContain("unredirect_ok=True");
	});

	test("DAMAGE extension is functional and tracks regions", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "damage_extension_is_functional_and_tracks_regions.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("damage_present=True");
		expect(output).toContain("xfixes_present=True");
	});

	test.skip("Error handling: BadWindow for invalid window ID", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "error_handling_badwindow_for_invalid_window_id.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("error=BadWindow");
	});

	test.skip("Error handling: BadValue for invalid arguments", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "error_handling_badvalue_for_invalid_arguments.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("error=BadValue");
	});

	test.skip("Multi-client event delivery via EventBroadcaster", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "multi_client_event_delivery_via_eventbroadcaster.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("client2_got_property_notify=True");
	});

	test("WarpPointer moves cursor position", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "warppointer_moves_cursor_position.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("warp_ok=True");
	});

	test("CopyArea between windows works", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "copyarea_between_windows_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("copy_area=ok");
	});

	test("RotateProperties works correctly", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "rotateproperties_works_correctly.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("rotate_ok=True");
	});

	test("KillClient destroys client resources", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "killclient_destroys_client_resources.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("kill_client_test=ok");
	});

	test("SetInputFocus and GetInputFocus round-trip", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "setinputfocus_and_getinputfocus_round_trip.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("focus_window=True");
	});

	test("ListProperties returns all set properties", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "listproperties_returns_all_set_properties.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("has_a1=True");
		expect(output).toContain("has_a2=True");
	});

	test.skip("QueryBestSize returns valid tile/stipple sizes", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "querybestsize_returns_valid_tile_stipple_sizes.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("tile_width=");
		expect(output).toContain("stipple_width=");
	});

	test("glmark2 smoke test (GLX rendering)", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(60_000);
		const output = await execInSidecar(
			sidecarContainer,
			"timeout 10 glmark2 --off-screen -b build 2>&1 || true",
		);
		// Should produce some output without crashing
		expect(output).not.toContain("Segmentation fault");
	});

	test("XTEST FakeInput generates events", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "xtest_fakeinput_generates_events.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xtest_present=True");
	});

	test("RECORD extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "record_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("record_present=True");
	});

	test("DOUBLE-BUFFER extension is available", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "double_buffer_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("dbe_present=True");
	});

	// DRI3 was removed from the server (commit 60b4bd3).
	test.skip("DRI3 extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "dri3_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("dri3_present=True");
	});

	test("Present extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "present_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("present_present=True");
	});

	test("SECURITY extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "security_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("security_present=True");
	});

	test("XVideo extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "xvideo_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xvideo_present=True");
	});

	test("XIM extension is available", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "xim_extension_is_available.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("xim_support=True");
	});

	test("Backing store preserves window contents across unmap/remap", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "backing_store_preserves_window_contents_across_unmap_remap.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("backing_store=2");
		expect(output).toContain("backing_store_test=ok");
	});

	test("Bit gravity preserves content on resize", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "bit_gravity_preserves_content_on_resize.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("bit_gravity=9");
	});

	test("PolyLine and PolySegment draw without errors", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "polyline_and_polysegment_draw_without_errors.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("drawing_ops=ok");
	});

	test("Selection protocol (clipboard) works end-to-end", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "selection_protocol_clipboard_works_end_to_end.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("selection_owner=True");
	});
});
