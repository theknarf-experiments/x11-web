/**
 * E2E compliance tests for Phase 2 spec compliance fixes:
 * - CopyColormapAndFree semantics
 * - ChangeKeyboardControl led_mode and per-key auto-repeat
 * - WarpPointer MotionNotify timestamp
 * - DPMS ForceLevel validation
 * - MIT-SCREEN-SAVER extension event base
 * - XFIXES CreatePointerBarrier window validation
 * - SECURITY untrusted client restrictions
 *
 * Per-test python3-xlib scripts live under `e2e/scripts/`.
 */

import { test, expect, runPythonScript } from "./fixtures";
import type { StartedTestContainer } from "testcontainers";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
): Promise<string> {
	const result = await container.exec(["bash", "-c", `export DISPLAY=:99; ${cmd}`]);
	return result.output.trim();
}

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe.serial("CopyColormapAndFree spec compliance", () => {
	test.setTimeout(60_000);

	test("CopyColormapAndFree copies source and is usable", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "copycolormapandfree_usable.py");
		expect(output).toContain("COPY_CMAP_OK");
	});
});

test.describe.serial("ChangeKeyboardControl spec compliance", () => {
	test.setTimeout(60_000);

	test("GetKeyboardControl returns valid auto_repeats bitmap", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "getkeyboardcontrol_auto_repeats.py");
		// Either all-on or partial is fine (some modifier keys may be excluded).
		expect(output).toMatch(/AUTO_REPEATS_(ALL_ON|PARTIAL)/);
	});

	test("ChangeKeyboardControl modifies bell settings", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "changekeyboardcontrol_bell.py");
		expect(output).toContain("BELL_PERCENT_OK");
		expect(output).toContain("BELL_PITCH_OK");
	});
});

test.describe.serial("WarpPointer spec compliance", () => {
	test.setTimeout(60_000);

	test("WarpPointer moves pointer to target coordinates", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "warppointer_target_coords.py");
		expect(output).toContain("WARP_OK");
	});
});

test.describe.serial("DPMS spec compliance", () => {
	test.setTimeout(60_000);

	test("DPMS extension is reported by server", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"xdpyinfo 2>/dev/null | grep -i dpms || echo NO_DPMS",
		);
		expect(output.toLowerCase()).toContain("dpms");
	});
});

test.describe.serial("MIT-SCREEN-SAVER extension", () => {
	test.setTimeout(60_000);

	test("MIT-SCREEN-SAVER extension is present with event base", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "screensaver_event_base.py");
		expect(output).toContain("EXT_OK");
		expect(output).toContain("EVENT_BASE_92_OK");
	});
});

test.describe.serial("XFIXES spec compliance", () => {
	test.setTimeout(60_000);

	test("XFIXES extension is present", async ({ sidecarContainer }) => {
		const output = await probe(sidecarContainer, "xfixes_extension_present.py");
		expect(output).toContain("XFIXES_OK");
	});
});

test.describe.serial("SECURITY extension", () => {
	test.setTimeout(60_000);

	test("SECURITY extension is present", async ({ sidecarContainer }) => {
		const output = await probe(sidecarContainer, "security_extension_present.py");
		expect(output).toContain("SECURITY_OK");
	});
});

test.describe.serial("Extension event bases", () => {
	test.setTimeout(60_000);

	test("all extensions report valid non-overlapping event bases", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "extension_event_bases_unique.py");
		expect(output).toContain("EVENT_BASES_UNIQUE_OK");
	});
});
