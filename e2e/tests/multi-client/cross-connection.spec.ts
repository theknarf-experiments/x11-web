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
	const result = await container.exec([
		"bash",
		"-c",
		`export DISPLAY=:99; ${cmd}`,
	]);
	return result.output.trim();
}

test.describe
	.serial("INCR selection transfer", () => {
		test("Large clipboard data can be transferred between clients", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"large_clipboard_data_can_be_transferred_between_clients.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("owns_clipboard=True");
			expect(output).toContain("data_matches=True");
		});

		test("Selection conversion between two clients works", async ({
			sidecarContainer,
		}) => {
			// Use xclip + xsel to test cross-client selection conversion
			// which avoids python3-xlib hanging issues with multi-display scripts.
			const output = await execInSidecar(
				sidecarContainer,
				[
					// Set PRIMARY selection via xclip (client 1)
					`echo -n "test_data" | xclip -selection primary -i 2>/dev/null`,
					"&&",
					// Read it back via xsel (client 2 = different process)
					`result=$(timeout 5 xsel --primary --output 2>/dev/null || echo "TIMEOUT")`,
					"&&",
					`echo "selection_data=$result"`,
					"&&",
					// Also verify with xclip -o
					`result2=$(timeout 5 xclip -selection primary -o 2>/dev/null || echo "TIMEOUT")`,
					"&&",
					`echo "xclip_readback=$result2"`,
				].join(" "),
			);
			// xclip writes, xsel or xclip reads back
			expect(output).toMatch(
				/selection_data=test_data|xclip_readback=test_data/,
			);
		});
	});

test.describe
	.serial("Multi-client interaction", () => {
		test.setTimeout(60_000);

		test("Two clients can set and read properties", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"two_clients_can_set_and_read_properties.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("MULTI_CLIENT_OK");
		});

		test("Selection transfer between two clients", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"selection_transfer_between_two_clients.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("SELECTION_OWNER_OK");
		});

		test("Event delivery to multiple clients watching same window", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"event_delivery_to_multiple_clients_watching_same_window.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("MULTI_EVENT_OK");
		});
	});

test.describe
	.serial("Cross-connection event delivery", () => {
		test("ClientMessage delivered across connections", async ({
			sidecarContainer,
		}) => {
			const output = (
				await runPythonScript(
					sidecarContainer,
					"clientmessage_delivered_across_connections.py",
					{ env: { DISPLAY: ":99" } },
				)
			).output.trim();
			expect(output).toContain("cross_conn_test=done");
		});
	});
