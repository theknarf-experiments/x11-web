/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import type { StartedTestContainer } from "testcontainers";
import { expect, runPythonScript, test } from "../fixtures";

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe
	.serial("SendEvent propagation compliance", () => {
		test("SendEvent delivers synthetic ClientMessage", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"sendevent_delivers_synthetic_clientmessage.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("send_event_ok=True");
		});

		test("SendEvent with propagate walks ancestor tree", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"sendevent_with_propagate_walks_ancestor_tree.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("propagation_setup=ok");
		});
	});

test.describe
	.serial("ConfigureNotify delivery (Phase 7)", () => {
		test("Window receives ConfigureNotify on resize when StructureNotifyMask set", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"window_receives_configurenotify_on_resize_when_structurenotifymask_set.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("configure_notify_received=True");
		});

		test("MapNotify delivered only with StructureNotifyMask", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"mapnotify_delivered_only_with_structurenotifymask.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("map_notify_with_mask=True");
		});
	});

test.describe("Crossing event detail conformance", () => {
	test("EnterNotify/LeaveNotify detail fields are correct per hierarchy", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"enternotify_leavenotify_detail_hierarchy.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(/crossing-detail: pass=(\d+) fail=(\d+)/);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Crossing detail: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});

	test("Nonlinear crossing between sibling windows", async ({
		sidecarContainer,
	}) => {
		test.setTimeout(30_000);
		const result = await runPythonScript(
			sidecarContainer,
			"nonlinear_crossing_sibling_windows.py",
			{ env: { DISPLAY: ":99" } },
		);
		const match = result.output.match(
			/crossing-nonlinear: pass=(\d+) fail=(\d+)/,
		);
		expect(match).toBeTruthy();
		const passed = Number.parseInt(match![1], 10);
		const failed = Number.parseInt(match![2], 10);
		console.log(`Crossing nonlinear: ${passed} passed, ${failed} failed`);
		expect(failed).toBe(0);
		expect(passed).toBeGreaterThanOrEqual(2);
	});
});

test.describe
	.serial("Extension event bases", () => {
		test.setTimeout(60_000);

		test("all extensions report valid non-overlapping event bases", async ({
			sidecarContainer,
		}) => {
			const output = await probe(
				sidecarContainer,
				"extension_event_bases_unique.py",
			);
			expect(output).toContain("EVENT_BASES_UNIQUE_OK");
		});
	});

test.describe
	.serial("Window management edge cases", () => {
		test.setTimeout(60_000);

		test("Expose event on newly mapped window with ExposureMask", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"expose_event_on_newly_mapped_window_with_exposuremask.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("EXPOSE_ON_MAP_OK");
		});

		test("UnmapNotify sent when window unmapped", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"unmapnotify_sent_when_window_unmapped.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("UNMAP_NOTIFY_OK");
		});
	});

test.describe
	.serial("VisibilityNotify on geometry changes", () => {
		test.setTimeout(60_000);

		test("VisibilityNotify sent when window is moved to reveal sibling", async ({
			sidecarContainer,
		}) => {
			const output = await probe(
				sidecarContainer,
				"visibilitynotify_on_geometry_move.py",
			);
			expect(output).toContain("PASS");
		});
	});

test.describe
	.serial("SendEvent event_type validation", () => {
		test.setTimeout(60_000);

		test("valid synthetic events are delivered", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"valid_synthetic_events_are_delivered.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("SEND_EVENT_OK");
		});
	});
