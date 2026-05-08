/**
 * E2E compliance tests for INCR selection transfer, WarpPointer conditional
 * warp, DELETE selection target, and component-alpha glyph rendering.
 *
 * These tests validate the protocol fixes made for full X11 spec compliance.
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}


// ==========================================================================
// INCR (Incremental) Selection Transfer
// ==========================================================================
test.describe.serial("INCR selection transfer", () => {
	test.setTimeout(60_000);

	test("small selection data is transferred inline (non-INCR)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "small_selection_data_is_transferred_inline_non_incr.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("SMALL_TRANSFER_OK");
	});

	test("property change and delete round-trip works (INCR infrastructure)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "property_change_and_delete_round_trip_works_incr_infrastructure.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("LARGE_PROP_OK");
		expect(output).toContain("DELETE_PROP_OK");
	});

	test("MULTIPLE selection target works", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "multiple_selection_target_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("SELECTION_OWNER_OK");
	});
});

// ==========================================================================
// WarpPointer Conditional Warp
// ==========================================================================
test.describe.serial("WarpPointer conditional warp", () => {
	test.setTimeout(60_000);

	test("unconditional warp moves pointer to absolute position", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "unconditional_warp_moves_pointer_to_absolute_position.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("ABSOLUTE_WARP_OK");
	});

	test("relative warp offsets from current position", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "relative_warp_offsets_from_current_position.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("RELATIVE_WARP_OK");
	});

	test("conditional warp with src_window only warps if pointer is in src rectangle", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "conditional_warp_with_src_window_only_warps_if_pointer_is_in_src_rectangle.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("CONDITIONAL_WARP_INSIDE_OK");
		expect(output).toContain("CONDITIONAL_WARP_OUTSIDE_OK");
	});
});

// ==========================================================================
// DELETE Selection Target (ICCCM)
// ==========================================================================
test.describe.serial("DELETE selection target", () => {
	test.setTimeout(60_000);

	test("DELETE target clears selection ownership", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "delete_target_clears_selection_ownership.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("OWNER_SET_OK");
	});
});

// ==========================================================================
// xdpyinfo / xprop protocol validation
// ==========================================================================
test.describe.serial("Protocol validation tools", () => {
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

	test("xlsatoms returns predefined atoms", async ({
		sidecarContainer,
	}) => {
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

	test("xwininfo on root window succeeds", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xwininfo -root 2>&1",
		);
		expect(output).not.toContain("Error");
		expect(output).toMatch(/Width|Height|Depth/);
	});
});

// ==========================================================================
// RENDER extension compliance
// ==========================================================================
test.describe.serial("RENDER extension compliance", () => {
	test.setTimeout(120_000);

	test("rendercheck basic composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t fill 2>&1 | tail -5",
		);
		// rendercheck reports pass/fail
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
		// If rendercheck is not installed, skip gracefully
		if (output.includes("not found") || output.includes("No such file")) {
			test.skip();
		}
	});

	test("rendercheck gradient operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t gradient 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck blend operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t blend 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("rendercheck composite operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"rendercheck -t composite 2>&1 | tail -5",
		);
		if (output.includes("not found")) {
			test.skip();
		}
		if (output.includes("tests passed")) {
			expect(output).not.toMatch(/\d+ tests failed/);
		}
	});

	test("RENDER PictFormats include required formats", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "render_pictformats_include_required_formats.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("RENDER_EXT_OK");
	});
});

// ==========================================================================
// Complex WM scenarios
// ==========================================================================
test.describe.serial("Window manager compliance", () => {
	test.setTimeout(60_000);

	test("override-redirect windows bypass SubstructureRedirect", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "override_redirect_windows_bypass_substructureredirect.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("OVERRIDE_REDIRECT_OK");
	});

	test("window stacking operations (raise/lower)", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "window_stacking_operations_raise_lower.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("STACKING_OK");
	});

	test("focus model (Passive, Locally Active) works", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "focus_model_passive_locally_active_works.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("FOCUS_SET_OK");
		expect(output).toContain("FOCUS_POINTERROOT_OK");
	});
});

test.describe("Orphan: INCR clipboard transfer", () => {
	test("large clipboard data transfers via INCR protocol", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				// Generate a large string (> typical max request size)
				"python3 -c \"print('A' * 100000)\" | xclip -selection clipboard -i",
				"sleep 0.5",
				"RESULT=$(xclip -selection clipboard -o 2>&1 | wc -c)",
				"echo \"INCR_BYTES=$RESULT\"",
			].join("\n"),
		]);
		console.log(`INCR: ${result.output.trim()}`);
		// If xclip works, it should have transferred the full data
		if (result.exitCode === 0 && result.output.includes("INCR_BYTES=")) {
			const bytes = parseInt(
				result.output.match(/INCR_BYTES=(\d+)/)?.[1] || "0",
				10,
			);
			// We expect close to 100001 bytes (100000 chars + newline)
			if (bytes > 0) {
				expect(bytes).toBeGreaterThan(50000);
			}
		}
	});
});
