/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import type { StartedTestContainer } from "testcontainers";
import { expect, runPythonScript, test } from "../fixtures";

async function execInSidecar(
	container: StartedTestContainer,
	cmd: string,
	_timeoutMs = 30_000,
): Promise<string> {
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
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

test.describe
	.serial("Keyboard and input", () => {
		test("GetKeyboardMapping returns valid mappings", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"getkeyboardmapping_returns_valid_mappings.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("KEYMAP_OK");
		});

		test("GetModifierMapping returns valid modifiers", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"getmodifiermapping_returns_valid_modifiers.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("MODMAP_OK");
		});

		test("xkbcomp can query keyboard layout", async ({ sidecarContainer }) => {
			const output = await execInSidecar(
				sidecarContainer,
				"setxkbmap -query 2>&1",
			);
			// Should not error
			expect(output).not.toContain("Error");
			// Should report a layout
			expect(output).toMatch(/layout|rules/);
		});
	});

test.describe
	.serial("Dynamic keymap support", () => {
		test("ChangeKeyboardMapping stores and retrieves custom keysyms", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"changekeyboardmapping_stores_and_retrieves_custom_keysyms.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("original_keysym=0x61"); // 'a'
			expect(output).toContain("new_keysym=0x7a"); // 'z'
		});

		test("GetKeyboardMapping returns correct keysyms for common keys", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"getkeyboardmapping_returns_correct_keysyms_for_common_keys.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("escape=0xff1b");
			expect(output).toContain("return=0xff0d");
			expect(output).toContain("space=0x20");
		});
	});

test.describe
	.serial("ChangeKeyboardControl spec compliance", () => {
		test.setTimeout(60_000);

		test("GetKeyboardControl returns valid auto_repeats bitmap", async ({
			sidecarContainer,
		}) => {
			const output = await probe(
				sidecarContainer,
				"getkeyboardcontrol_auto_repeats.py",
			);
			// Either all-on or partial is fine (some modifier keys may be excluded).
			expect(output).toMatch(/AUTO_REPEATS_(ALL_ON|PARTIAL)/);
		});

		test("ChangeKeyboardControl modifies bell settings", async ({
			sidecarContainer,
		}) => {
			const output = await probe(
				sidecarContainer,
				"changekeyboardcontrol_bell.py",
			);
			expect(output).toContain("BELL_PERCENT_OK");
			expect(output).toContain("BELL_PITCH_OK");
		});
	});
