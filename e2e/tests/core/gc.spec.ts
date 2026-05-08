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

async function probe(
	container: StartedTestContainer,
	name: string,
): Promise<string> {
	const result = await runPythonScript(container, name, {
		env: { DISPLAY: ":99" },
	});
	return result.output.trim();
}

test.describe.serial("x11perf extended operations", () => {
	test.setTimeout(300_000);

	test("x11perf text operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -noop -prop -gc 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf fill operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -noop -gc -create 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf copy operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -noop -gc -move 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf arc and polygon operations", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -dot -rect100 -srect100 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf window operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -create -map -unmap -destroy -resize -move 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});

	test("x11perf image operations", async ({ sidecarContainer }) => {
		const output = await execInSidecar(
			sidecarContainer,
			"x11perf -repeat 1 -time 1 -putimage10 -putimage100 -putimage500 -shmput10 -shmput100 -shmput500 2>&1 | tail -30",
		);
		expect(output).not.toContain("X Error");
		expect(output).not.toContain("Segmentation fault");
		expect(output).toMatch(/reps|trep/i);
	});
});

test.describe("Conformance: x11perf extended validation", () => {
	test("x11perf drawing operations complete without crashes", async ({ sidecarContainer }) => {
		test.setTimeout(300_000);
		const result = await sidecarContainer.exec([
			"bash", "-c", [
				"export DISPLAY=:99",
				"x11perf -time 1 -repeat 1 -subs 1 \\",
				"  -noop -dot -line10 -rect10 -circle10 -fcircle10 \\",
				"  -seg10 -ftext -putimage10 -scroll10 -copywinwin10 \\",
				"  -prop -gc -create -map -unmap -destroy \\",
				"  2>&1 | tail -40",
			].join("\n"),
		]);
		// Verify we got results lines (reps @ msec format)
		const resultLines = result.output.split("\n").filter((l: string) =>
			l.includes("reps @") || l.includes("/sec")
		);
		expect(resultLines.length).toBeGreaterThanOrEqual(10);
	});
});

test.describe.serial("Wide dashed line rendering", () => {
	test.setTimeout(60_000);

	test("wide dashed horizontal line creates visible gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_dashed_horizontal_line.py");
		expect(output).toContain("PASS");
	});

	test("wide dashed vertical line creates visible gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_dashed_vertical_line.py");
		expect(output).toContain("PASS");
	});

	test("DoubleDash wide line draws background in gaps", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "wide_doubledash_line.py");
		expect(output).toContain("PASS");
	});
});

test.describe.serial("GC raster operations", () => {
	test("GC function modes (copy, xor, invert) work", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "gc_function_modes_copy_xor_invert_work.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("gc_ops_ok=True");
		expect(output).toContain("inverted_pixel=0x00ffff");
	});
});
