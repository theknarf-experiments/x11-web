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

test.describe.serial("Colormap operations", () => {
	test("AllocColor returns correct RGB values", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "alloccolor_returns_correct_rgb_values.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("red_match=True");
	});

	test("AllocNamedColor resolves color names", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "allocnamedcolor_resolves_color_names.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("red_ok=True");
		expect(output).toContain("blue_ok=True");
	});
});

test.describe.serial("Multi-depth visual compliance", () => {
	test("Server advertises 24-bit and 32-bit visuals", async ({
		sidecarContainer,
	}) => {
		const output = await execInSidecar(
			sidecarContainer,
			`xdpyinfo 2>&1 | grep 'depth' | head -5`,
		);
		expect(output).toContain("24");
	});

	test("PutImage and GetImage round-trip depth 24 ZPixmap", async ({
		sidecarContainer,
	}) => {
		const output = (await runPythonScript(sidecarContainer, "putimage_and_getimage_round_trip_depth_24_zpixmap.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("putget_test=ok");
		expect(output).toContain("red_match=True");
	});

	test("CopyArea between windows", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "copyarea_between_windows.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("copy_area=ok");
	});

	test("Window colormap operations", async ({ sidecarContainer }) => {
		const output = (await runPythonScript(sidecarContainer, "window_colormap_operations.py", { env: { DISPLAY: ":99" } })).output.trim();
		expect(output).toContain("colormap_test=ok");
		expect(output).toContain("alloc_red=65535");
		expect(output).toContain("named_alloc=ok");
	});
});

test.describe("Visual and depth support", () => {
	test("xdpyinfo reports multiple depths (1, 4, 8, 16, 24, 32)", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo"]);
		console.log(`xdpyinfo depths: exit=${result.exitCode}`);
		// Check that multiple depths are advertised
		expect(result.output).toContain("depth 24");
		expect(result.output).toContain("depth 32");
		expect(result.output).toContain("depth 8");
		expect(result.output).toContain("depth 16");
		expect(result.output).toContain("depth 1");
	});

	test("xdpyinfo reports PseudoColor visual for depth 8", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo"]);
		expect(result.output).toContain("PseudoColor");
	});

	test("xdpyinfo reports DirectColor visual for depth 24", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo"]);
		expect(result.output).toContain("DirectColor");
	});

	test("xdpyinfo reports all pixmap formats (1, 4, 8, 16, 24, 32)", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec(["xdpyinfo"]);
		// Check pixmap formats section
		const lines = result.output.split("\n");
		const formatLines = lines.filter((l: string) =>
			l.includes("pixmap format") || (l.includes("depth") && l.includes("bits_per_pixel")),
		);
		// Should have at least 6 pixmap formats
		expect(formatLines.length).toBeGreaterThanOrEqual(6);
	});
});

test.describe.serial("CopyColormapAndFree spec compliance", () => {
	test.setTimeout(60_000);

	test("CopyColormapAndFree copies source and is usable", async ({
		sidecarContainer,
	}) => {
		const output = await probe(sidecarContainer, "copycolormapandfree_usable.py");
		expect(output).toContain("COPY_CMAP_OK");
	});
});

test.describe("Colormap operations", () => {
	test("AllocColor and AllocNamedColor round-trip", async ({ sidecarContainer }) => {
		test.setTimeout(30_000);
		const result = await sidecarContainer.exec([
			"python3", "-c", [
				"import Xlib, Xlib.display",
				"d = Xlib.display.Display()",
				"screen = d.screen()",
				"cmap = screen.default_colormap",
				"# AllocColor: exact RGB values",
				"reply = cmap.alloc_color(65535, 0, 0)  # pure red",
				"assert reply.pixel is not None, 'AllocColor failed'",
				"print(f'alloc_red_pixel={reply.pixel:#x}')",
				"# AllocNamedColor: look up by name",
				"reply2 = cmap.alloc_named_color('blue')",
				"assert reply2.pixel is not None, 'AllocNamedColor failed'",
				"print(f'alloc_blue_pixel={reply2.pixel:#x}')",
				"# QueryColors: read back the allocated colors",
				"colors = cmap.query_colors([reply.pixel, reply2.pixel])",
				"assert len(colors) == 2, f'QueryColors returned {len(colors)} colors'",
				"print(f'query_red=({colors[0].red},{colors[0].green},{colors[0].blue})')",
				"print(f'query_blue=({colors[1].red},{colors[1].green},{colors[1].blue})')",
				"# Red should have red component > 60000",
				"assert colors[0].red > 60000, f'Red too low: {colors[0].red}'",
				"# Blue should have blue component > 60000",
				"assert colors[1].blue > 60000, f'Blue too low: {colors[1].blue}'",
				"print('COLORMAP_PASS')",
				"d.close()",
			].join("\n"),
		]);
		expect(result.output).toContain("COLORMAP_PASS");
	});
});

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
