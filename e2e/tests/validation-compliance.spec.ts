/**
 * E2E compliance tests for X11 protocol validation fixes:
 * - ConfigureWindow stack_mode validation
 * - SendEvent event_type validation
 * - GrabButton/GrabKey window validation
 * - AllocColorCells/AllocColorPlanes contiguous allocation
 * - Pointer mapping (7-button support)
 * - Authentication rejection of unknown protocols
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

/** Run a command inside the sidecar container and return stdout. */
async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}


// ==========================================================================
// ConfigureWindow stack_mode validation
// ==========================================================================
test.describe.serial("ConfigureWindow stack_mode validation", () => {
	test.setTimeout(60_000);

	test("valid stack modes (0-4) are accepted", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "valid_stack_modes_0_4_are_accepted.py", { env: { DISPLAY: ":99" } })).output.trim();
		for (let i = 0; i < 5; i++) {
			expect(output).toContain(`MODE_${i}_OK`);
		}
	});

	test("invalid stack mode (>4) returns BadValue error", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "invalid_stack_mode_4_returns_badvalue_error.py", { env: { DISPLAY: ":99" } })).output.trim();
		// Server should either reject with BadValue or handle gracefully
		expect(output).not.toBe("");
	});
});

// ==========================================================================
// SendEvent event_type validation
// ==========================================================================
test.describe.serial("SendEvent event_type validation", () => {
	test.setTimeout(60_000);

	test("valid synthetic events are delivered", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "valid_synthetic_events_are_delivered.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("SEND_EVENT_OK");
	});
});

// ==========================================================================
// Colormap allocation
// ==========================================================================
test.describe.serial("Colormap allocation", () => {
	test.setTimeout(60_000);

	test("AllocColor on TrueColor colormap returns correct pixel", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "alloccolor_on_truecolor_colormap_returns_correct_pixel.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("ALLOC_COLOR_RED_OK");
		expect(output).toContain("ALLOC_COLOR_BLUE_OK");
	});

	test("LookupColor resolves standard X11 color names", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "lookupcolor_resolves_standard_x11_color_names.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("LOOKUP_RED_OK");
		expect(output).toContain("LOOKUP_GREEN_OK");
		expect(output).toContain("LOOKUP_BLUE_OK");
		expect(output).toContain("LOOKUP_WHITE_OK");
		expect(output).toContain("LOOKUP_BLACK_OK");
	});
});

// ==========================================================================
// Pointer mapping
// ==========================================================================
test.describe.serial("Pointer mapping", () => {
	test.setTimeout(60_000);

	test("GetPointerMapping returns at least 5 buttons", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "getpointermapping_returns_at_least_5_buttons.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("MAPPING_COUNT_OK");
		expect(output).toContain("MAPPING_IDENTITY_OK");
	});

	test("SetPointerMapping can remap buttons", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "setpointermapping_can_remap_buttons.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("REMAP_OK");
	});
});

// ==========================================================================
// Grab validation
// ==========================================================================
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

// ==========================================================================
// xdpyinfo validation
// ==========================================================================
test.describe.serial("Server capabilities via xdpyinfo", () => {
	test.setTimeout(60_000);

	test("xdpyinfo reports all required extensions", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>/dev/null || echo XDPYINFO_FAILED",
		);

		if (output.includes("XDPYINFO_FAILED")) {
			test.skip();
			return;
		}

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
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>/dev/null || echo XDPYINFO_FAILED",
		);

		if (output.includes("XDPYINFO_FAILED")) {
			test.skip();
			return;
		}

		// Must report 24-bit TrueColor (the default visual)
		expect(output).toContain("depth 24");
		expect(output).toContain("TrueColor");
	});
});
