/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { test, expect, runPythonScript } from "../fixtures";
import type { StartedTestContainer } from "testcontainers";

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
	.serial("WarpPointer conditional warp", () => {
		test.setTimeout(60_000);

		test("unconditional warp moves pointer to absolute position", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"unconditional_warp_moves_pointer_to_absolute_position.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("ABSOLUTE_WARP_OK");
		});

		test("relative warp offsets from current position", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"relative_warp_offsets_from_current_position.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("RELATIVE_WARP_OK");
		});

		test("conditional warp with src_window only warps if pointer is in src rectangle", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"conditional_warp_with_src_window_only_warps_if_pointer_is_in_src_rectangle.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("CONDITIONAL_WARP_INSIDE_OK");
			expect(output).toContain("CONDITIONAL_WARP_OUTSIDE_OK");
		});
	});

test.describe
	.serial("WarpPointer spec compliance", () => {
		test.setTimeout(60_000);

		test("WarpPointer moves pointer to target coordinates", async ({
			sidecarContainer,
		}) => {
			const output = await probe(
				sidecarContainer,
				"warppointer_target_coords.py",
			);
			expect(output).toContain("WARP_OK");
		});
	});

test.describe
	.serial("Pointer mapping", () => {
		test.setTimeout(60_000);

		test("GetPointerMapping returns at least 5 buttons", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"getpointermapping_returns_at_least_5_buttons.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("MAPPING_COUNT_OK");
			expect(output).toContain("MAPPING_IDENTITY_OK");
		});

		test("SetPointerMapping can remap buttons", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"setpointermapping_can_remap_buttons.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("REMAP_OK");
		});
	});
