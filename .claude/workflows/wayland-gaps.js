export const meta = {
	name: 'wayland-gaps',
	description: 'Close the known Wayland sidecar follow-ups (pointer catch-up, bounded channel) and re-gate',
	whenToUse: 'The fourth item of the x11-health workstream, split out because the other three are done.',
	phases: [
		{ title: 'Wayland', detail: 'pointer catch-up, bounded SidecarToBackend, X11 channel write-up' },
		{ title: 'Gate', detail: 'wayland suite + smoke + build gates + an X11 regression slice' },
	],
}

const REPO = '/Users/knarf/projects/theknarf-experiments/x11-web'

const CONTEXT = `
REPO: ${REPO}   Host: macOS (darwin/arm64). Docker available. pnpm + cargo + turbo.
Project rules (CLAUDE.md): pnpm never npm/npx; cargo for Rust; turbo for cross-project tasks.
"Never commit anything that breaks e2e tests." Never use --no-verify. Never ask the user anything.

You are on branch \`x11-health\`, which is ahead of main with a completed workstream: the X11 e2e
suite went from 14 failed / 320 passed to **1 failed / 333 passed**, the pixel codec became
content-adaptive, and the Wayland sidecar landed earlier. Read \`git log --oneline main..HEAD\` first.
Do not redo any of that. Your job is only the Wayland follow-ups below.

BUILD / TEST GATES THAT APPLY
  * Linux builds of the wayland crates: \`bash tools/wayland-build.sh <cargo args>\`. NEVER build
    smithay on the macOS host — the crates are cfg-gated to Linux with macOS stubs on purpose.
  * \`cargo test --workspace\` on the host must stay green (736 passed as of the codec stage).
  * Wayland e2e is OPT-IN: \`X11WEB_WAYLAND_E2E=1 pnpm --filter x11-web-e2e exec playwright test
    tests/wayland\` (~22s warm, but budget ~11m if a Rust change invalidates the sidecar docker image).
    A bare path argument is NOT enough — \`testIgnore\` drops the files before collection.
  * \`bash e2e/scripts/wayland-smoke.sh\` is the browser-free check (socket, 9 globals, PutImage count).
    It is load-bearing for one of the tasks below: it runs with NO backend attached, so it is what
    proves the no-consumer path still animates instead of stalling.
  * Host \`cargo clippy --workspace\` is pre-existing RED on macOS-only paths in x11-server and
    sidecar-macos. Use \`bash tools/wayland-build.sh clippy -p <crate> --all-targets -- -D warnings\`.
  * \`pnpm biome check .\` and \`cargo fmt --check\` are pre-existing red repo-wide. Scope any lint work
    to files you actually touch; do NOT repo-wide-fix them.
  * Bash times out at 10 minutes. Use run_in_background:true and poll a redirect file. Do NOT pipe a
    long run through \`tail\`/\`head\` — it buffers and you see nothing until exit.

ARCHITECTURE CONSTRAINT THAT DECIDES TASK 3
  X11 \`DisplayUpdate::PutImage\` carries a DAMAGE RECTANGLE (emitted at
  crates/x11-server/src/xserver/client/sync_flush.rs:388) and the frontend blits it onto a PERSISTENT
  canvas with no clear (frontend/src/ClientRenderer.ts:65). X11 updates are therefore INCREMENTAL:
  dropping or reordering one permanently corrupts the window. The Wayland sidecar's credit-based frame
  DROPPING is only safe because each Wayland frame is a full window composite.
`

const STAGE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['fixed', 'notFixed', 'filesTouched', 'verification', 'handoff'],
	properties: {
		fixed: {
			type: 'array',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['target', 'rootCause', 'fix', 'proof'],
				properties: {
					target: { type: 'string' },
					rootCause: { type: 'string' },
					fix: { type: 'string' },
					proof: { type: 'string' },
				},
			},
		},
		notFixed: {
			type: 'array',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['target', 'whatYouLearned', 'whyStopped'],
				properties: {
					target: { type: 'string' },
					whatYouLearned: { type: 'string' },
					whyStopped: { type: 'string' },
				},
			},
		},
		filesTouched: { type: 'array', items: { type: 'string' } },
		verification: { type: 'string' },
		handoff: { type: 'string' },
	},
}

const GATE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['passed', 'report', 'remainingWork'],
	properties: {
		passed: { type: 'boolean' },
		report: { type: 'string' },
		remainingWork: { type: 'array', items: { type: 'string' } },
	},
}

phase('Wayland')

const wayland = await agent(
	`${CONTEXT}

YOUR STAGE — close the Wayland sidecar gaps. Verify each still applies by reading the code first;
these descriptions were written when the sidecar landed and may be stale.

  1. \`wl_seat.get_pointer\` catch-up. The keyboard path was fixed to send \`enter\` + \`modifiers\` when a
     client binds a keyboard while the seat already holds focus on its surface. The pointer twin was
     never done: bind a pointer while the seat is already over a surface and no \`wl_pointer.enter\`
     is sent. It self-heals on the next motion, so it is cosmetic — fix it properly anyway: the seat
     needs to remember the last pointer position as well as the focused surface, and the GetPointer
     arm must send \`enter\` (with correct surface-local coordinates) plus a \`frame\`. Unit-test it.

  2. Bound the \`SidecarToBackend\` channel, or return the per-window frame credit after the QUIC write
     rather than after queue-for-write. As shipped, the credit scheme bounds the ENCODER but not a slow
     wire: a stalled link still grows memory without limit. Pick one and explain the trade.
     READ \`run_session\` IN FULL FIRST — there is a \`set_consuming(true/false)\` bracket around the
     session that exists precisely to stop a disconnected sidecar deadlocking or accumulating frames,
     and \`frame_shipped\` returns credit. Whatever you change must keep these true:
       - with NO backend attached the compositor still composites and releases frame callbacks
         (\`wayland-smoke.sh\` must still report hundreds of PutImage updates, not stall after 2);
       - a session that drops mid-flight cannot leave a window permanently out of credit;
       - a reconnect starts from zero credit debt with full damage.

  3. Re-check the X11 sidecar's \`display_rx\` for the same unbounded-channel problem now that its
     encode moved off the select! loop (commit \`sidecar: move WebP encode off the events loop\`).
     Given the INCREMENTAL constraint in the context above, bounding it must not drop updates. If the
     only safe fix needs an x11-server-side coalescing/re-capture mechanism (compositing pending rects
     into a union buffer rather than dropping any), DO NOT BUILD THAT HERE — write up precisely what it
     would take, with the file:line touchpoints, and put it in \`notFixed\`.

Gates before you commit: \`cargo test --workspace\` (host), \`bash tools/wayland-build.sh test --workspace\`
(Linux), \`bash e2e/scripts/wayland-smoke.sh\`, and the Wayland e2e suite. Commit each task separately
with this repo's message style (lowercase scope prefix).`,
	{ label: 'fix:wayland-gaps', phase: 'Wayland', effort: 'high', schema: STAGE_SCHEMA },
)

phase('Gate')

const gate = await agent(
	`${CONTEXT}

${wayland ? `WHAT THE WAYLAND STAGE DID:\nfixed: ${wayland.fixed.map((f) => f.target).join('; ') || 'none'}\nnot fixed: ${wayland.notFixed.map((f) => f.target).join('; ') || 'none'}\n${wayland.handoff}\nIts own verification claim:\n${wayland.verification}\n` : '(The Wayland stage returned nothing — establish the state yourself from git log and git status before gating.)'}

YOUR JOB — independently re-verify. Do not take the previous stage's claims on trust; re-run the
commands yourself and paste the real output.

  1. \`X11WEB_WAYLAND_E2E=1 pnpm --filter x11-web-e2e exec playwright test tests/wayland\` — must be
     3 passed. Background + poll; budget 11m for an image rebuild.
  2. \`bash e2e/scripts/wayland-smoke.sh\` — must pass AND report hundreds of PutImage updates. If it
     reports a handful, the credit/backpressure change stalled the no-consumer path: that is a
     blocker, not a curiosity.
  3. \`cargo test --workspace\` (host) and \`bash tools/wayland-build.sh test --workspace\` (Linux).
  4. \`bash tools/wayland-build.sh clippy -p x11-web-sidecar -p x11-web-sidecar-wayland -p x11-web-wayland-server -- -D warnings\`.
  5. \`pnpm turbo run typecheck\`.
  6. An X11 regression slice, since the branch's X11 suite is now at 1 failed / 333 passed and must
     stay there: \`pnpm --filter x11-web-e2e exec playwright test --workers=1 -g "xeyes pupils follow the cursor|firefox renders on the canvas|spawning xeyes creates a window on the canvas|multi-window: independent rendering and focus switching"\`
     — expect 4 passed.
  7. \`git status\` clean, \`git log --oneline main..HEAD\` sensible, no leaked docker containers or networks.

NOTE THE REPORTING INVERSION if you run anything matching "firefox responds to mouse and keyboard
input": it carries \`test.fail(true, ...)\`, so Playwright prints "passed" when input is BROKEN and
"failed" when it WORKS. Do not treat either direction as a regression signal from your changes.

passed=true only if every gate above is green. Be honest; an ugly truth reported plainly is the job.`,
	{ label: 'gate:wayland', phase: 'Gate', effort: 'high', schema: GATE_SCHEMA },
)

return {
	wayland: wayland ? { fixed: wayland.fixed, notFixed: wayland.notFixed, verification: wayland.verification } : null,
	gate: { passed: gate?.passed ?? false, report: gate?.report ?? 'no gate result', remainingWork: gate?.remainingWork ?? [] },
}
