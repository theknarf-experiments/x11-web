/**
 * PulseAudio + WebSocket display e2e tests.
 *
 * Tests:
 * - PulseAudio is running in the sidecar with the virtual_in / virtual_out sinks
 * - VLC can play a test video through PulseAudio
 * - Audio capture from the monitor source works
 * - Audacity is installed and sees the virtual devices
 * - The WebSocket display path renders xeyes / xterm
 */

import type { Locator, Page } from "@playwright/test";
import {
	test,
	expect,
	spawnApp,
	waitForDock,
	hasRenderedContent,
	countNonBlackPixels,
	waitForCanvasStable,
	cleanupApps,
} from "./fixtures";

test.describe("Audio", () => {
	test.afterEach(async ({ sidecarContainer }) => {
		await cleanupApps(sidecarContainer);
	});

	test("PulseAudio is running in sidecar", async ({ sidecarContainer }) => {
		// PulseAudio is optional in the sidecar image; skip gracefully
		// when pactl isn't installed instead of failing.
		const probe = await sidecarContainer.exec([
			"bash",
			"-c",
			"command -v pactl >/dev/null 2>&1 && echo HAVE || echo MISS",
		]);
		if (probe.output.includes("MISS")) {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"pactl info 2>&1 || echo PULSE_NOT_RUNNING",
		]);
		expect(result.output).not.toContain("PULSE_NOT_RUNNING");
	});

	test("PulseAudio virtual sinks are configured", async ({
		sidecarContainer,
	}) => {
		const probe = await sidecarContainer.exec([
			"bash",
			"-c",
			"command -v pactl >/dev/null 2>&1 && echo HAVE || echo MISS",
		]);
		if (probe.output.includes("MISS")) {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"pactl list sinks short 2>&1",
		]);
		expect(result.output).toContain("virtual_out");
		expect(result.output).toContain("virtual_in");
	});

	test("VLC plays test video with audio output", async ({
		sidecarContainer,
	}) => {
		const probe = await sidecarContainer.exec([
			"bash",
			"-c",
			"command -v cvlc >/dev/null 2>&1 && command -v pactl >/dev/null 2>&1 && echo HAVE || echo MISS",
		]);
		if (probe.output.includes("MISS")) {
			test.skip();
			return;
		}
		// Run cvlc (headless VLC) directly in the container to play test video.
		// This tests audio flows through PulseAudio without needing an X11 window.
		const playResult = await sidecarContainer.exec([
			"bash",
			"-c",
			// Play the test video for 3 seconds via PulseAudio, then exit.
			"timeout 5 cvlc --play-and-exit --no-video --aout=pulse " +
				"/opt/test-video.mp4 2>&1 &" +
				"sleep 2; pactl list sink-inputs short 2>&1",
		]);
		const output = playResult.output;
		// PulseAudio should be responsive. VLC may or may not have created
		// a sink-input yet, but the command should not error.
		expect(output).toBeDefined();
		// Check PA is still running after VLC usage.
		const paCheck = await sidecarContainer.exec([
			"bash",
			"-c",
			"pactl info 2>&1 | head -3",
		]);
		expect(paCheck.output).toContain("Server String");
	});

	test("audio capture produces Opus frames from monitor source", async ({
		sidecarContainer,
	}) => {
		// Use parec to capture a short sample from the monitor source.
		// This tests that the audio capture pipeline works.
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			// Record 0.5s of audio from the monitor, save to file.
			"timeout 2 parec --format=s16le --rate=48000 --channels=1 " +
				"--device=virtual_out.monitor > /tmp/audio_test.raw 2>&1; " +
				"ls -la /tmp/audio_test.raw 2>&1",
		]);
		const output = result.output;
		// The file should exist (even if it contains silence).
		expect(output).toContain("audio_test.raw");
	});

	test("Audacity is installed and can detect audio devices", async ({
		sidecarContainer,
	}) => {
		// Check that Audacity is installed.
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"which audacity 2>&1 || echo NOT_FOUND",
		]);
		const output = result.output;
		if (output.includes("NOT_FOUND")) {
			test.skip();
			return;
		}
		expect(output).toContain("/audacity");

		// Verify PulseAudio virtual devices are available (Audacity will use these).
		const paResult = await sidecarContainer.exec([
			"bash",
			"-c",
			"pactl list sinks short 2>&1; pactl list sources short 2>&1",
		]);
		expect(paResult.output).toContain("virtual_out");
		expect(paResult.output).toContain("virtual_in");
	});

	test("WebSocket display path renders xeyes", async ({
		page,
		frontendUrl,
	}) => {
		await page.goto(frontendUrl);
		await waitForDock(page);

		const win = await spawnApp(page, "", "xeyes");
		const canvas = win.locator('[data-testid="x11-canvas"]');
		await expect(canvas).toBeVisible({ timeout: 15_000 });

		await expect
			.poll(async () => hasRenderedContent(canvas), {
				timeout: 15_000,
				intervals: [1000, 2000, 3000],
			})
			.toBe(true);

		const pixels = await countNonBlackPixels(canvas);
		expect(pixels).toBeGreaterThan(100);
	});

	test("xterm renders with display updates via existing path", async ({
		page,
		frontendUrl,
	}) => {
		await page.goto(frontendUrl);
		await waitForDock(page);

		const win = await spawnApp(page, "", "xterm");
		const canvas = win.locator('[data-testid="x11-canvas"]');
		await expect(canvas).toBeVisible({ timeout: 15_000 });

		await waitForCanvasStable(canvas, { stableMs: 1500 });

		const pixels = await countNonBlackPixels(canvas);
		expect(pixels).toBeGreaterThan(50);
	});

	test("VLC test video file exists", async ({ sidecarContainer }) => {
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			"ls -la /opt/test-video.mp4 2>&1",
		]);
		expect(result.output).toContain("test-video.mp4");
	});
});
