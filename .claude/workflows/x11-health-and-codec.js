export const meta = {
	name: 'x11-health-and-codec',
	description:
		'Fix the deterministic X11 e2e failures, kill the parallel-run flakiness, make the WebP codec content-adaptive, and close the Wayland sidecar follow-ups',
	whenToUse:
		'Run to drive the post-Wayland-merge cleanup: green e2e suite, adaptive pixel codec, Wayland gaps. Resumable via resumeFromRunId.',
	phases: [
		{ title: 'Triage', detail: 'read-only: hypotheses for the deterministic failures, the flakiness, and the codec' },
		{ title: 'Baseline', detail: 'one authoritative measurement of what fails today, and why' },
		{ title: 'DetFix', detail: 'fix the deterministic failures, one cluster at a time' },
		{ title: 'Codec', detail: 'content-adaptive lossless/lossy selection across all three sidecars' },
		{ title: 'Flake', detail: 'attack the contention-only failures' },
		{ title: 'Wayland', detail: 'pointer catch-up, bounded channel, and the remaining gaps' },
		{ title: 'Gate', detail: 'full suite twice + wayland suite, compared against the baseline' },
	],
}

// ---------------------------------------------------------------------------
// Ground truth established by hand before this workflow ran. Every agent gets
// it so nobody re-derives it or contradicts it by accident.
// ---------------------------------------------------------------------------

const REPO = '/Users/knarf/projects/theknarf-experiments/x11-web'

const CONTEXT = `
REPO: ${REPO}   Host: macOS (darwin/arm64). Docker available. pnpm + cargo + turbo.
Project rules (CLAUDE.md): pnpm never npm/npx; cargo for Rust; turbo for cross-project tasks.
"Commit intermittently as stuff works, ensure that stuff works and never commit anything that breaks e2e tests."

RECENT HISTORY
\`main\` just absorbed the Wayland sidecar (8 commits, merged + pushed, HEAD 8baa40b): \`crates/wayland-server\`
(headless smithay compositor, shm-only, derived from waylandcraft, GPLv3, see its NOTICE) and
\`crates/sidecar-wayland\`. Its own e2e suite is green. Nothing in this workflow should regress it.

E2E GROUND TRUTH (measured, do not re-litigate — but DO re-measure if a number looks stale)
  * The suite is \`e2e/\`: Playwright + testcontainers, 2 workers by default, each worker gets its own
    (mock-oidc, backend, sidecar) container trio on a private docker network.
  * \`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core\` currently gives
    roughly **13-15 failed / 315-321 passed / 8 skipped**, and takes 17-30 MINUTES.
  * The failing SET is unstable: only ~6/13 repeat between back-to-back runs. Re-running the failures with
    \`--workers=1\` passed 13/13 — they are contention-sensitive, not (all) broken assertions.
  * SIX failures reproduce on \`main\` on EVERY run. These are the deterministic ones and the priority:
      - tests/core/pointer.spec.ts:68   WarpPointer moves pointer to target coordinates
      - tests/x11-web.spec.ts:25        global menu bar tracks the focused window
      - tests/x11-web.spec.ts:80        global menu bar mirrors a GTK app's exported menus
      - tests/x11-web.spec.ts:136       global menu bar mirrors an app via dbusmenu
      - tests/x11-web.spec.ts:171       spawning xeyes creates a window on the canvas
      - tests/x11-web.spec.ts:4032      xeyes renders at higher frame rate with 16ms timer
  * The rotating contention-only cast seen so far: resizing one window, vim :q, scroll-wheel xterm,
    firefox mouse+keyboard, firefox renders, dock entries, keyboard focus, emacs, xeyes pupils,
    closing a window, xdotool mousemove, multi-window rendering, clipboard round-trip.
  * A run can also report "N did not run" — that means a worker crashed. Treat it as a failure signal.
  * CHEAPEST WAY TO PROVE "not my change": \`git checkout main -- frontend packages\` (or the crates you
    did not touch), re-run, compare. Only the variable you moved changes; docker layers still cache-hit.
  * The Wayland suite is OPT-IN and separate: \`X11WEB_WAYLAND_E2E=1 pnpm --filter x11-web-e2e exec
    playwright test tests/wayland\` (~22s warm). A bare path argument is NOT enough — \`testIgnore\` drops
    the files before collection. It stands up its own backend+sidecar pair.
  * \`bash e2e/scripts/wayland-smoke.sh\` is the browser-free Wayland check.

PIXEL CODEC GROUND TRUTH (measured with a real benchmark against crates/pixel-codec, release build)
  content                     lossless            lossy q90
  UI/text 1024x768            36 ms,  25 KB       91 ms, 274 KB
  photo   1024x768           391 ms, 743 KB      153 ms, 256 KB
  UI/text 200x100 (damage)   2.4 ms, 870 B      4.1 ms, 7.6 KB
  photo   200x100 (damage)  27.7 ms,  21 KB     4.4 ms, 7.3 KB
  So: lossless wins decisively on flat UI/text; lossy wins decisively on photographic content, where
  lossless is CATASTROPHIC (391 ms for one full repaint, 28 ms for even a small damage rect).
  Today: \`crates/sidecar\` and \`crates/sidecar-wayland\` hardcode \`encode_rgba_lossless\`;
  \`crates/sidecar-macos\` hardcodes \`encode_rgba_lossy(.., 90.0)\`. The docstring on \`encode_rgba_lossy\`
  in crates/pixel-codec claims it is "5-10x faster than lossless and smaller" — that is TRUE ONLY for
  photographic input and BACKWARDS for flat UI. Fix the docstring as part of this work.

ARCHITECTURE FACTS THAT CONSTRAIN THE WORK
  * X11 \`DisplayUpdate::PutImage\` carries a DAMAGE RECTANGLE (x, y, w, h) — emitted at
    crates/x11-server/src/xserver/client/sync_flush.rs:388 — and the frontend blits it onto a
    PERSISTENT canvas with no clear (frontend/src/ClientRenderer.ts:65). Updates are therefore
    INCREMENTAL: dropping or reordering one permanently corrupts the window. This is why the Wayland
    sidecar's credit-based frame-dropping is NOT portable to X11, and why the X11 display loop must
    stay sequential. Any coalescing scheme must composite pending rects into a union buffer, not drop.
  * \`crates/sidecar/src/main.rs\` was just changed (UNCOMMITTED or freshly committed — check \`git log\`
    and \`git status\` first): display updates moved out of the \`events_loop\` select! arm into a sibling
    \`display_loop\` future with the encode on \`spawn_blocking\`. \`crates/sidecar-wayland/src/linux.rs\`
    already had this shape. If the change is still uncommitted, your Baseline phase decides its fate.
  * Host \`cargo clippy --workspace -- -D warnings\` is RED on macOS-only paths in \`crates/x11-server\`
    (pre-existing). The gate that matters for the sidecars is the Linux one:
    \`bash tools/wayland-build.sh clippy -p <crate> -- -D warnings\`.
  * \`pnpm biome check .\` and \`cargo fmt --check\` are BOTH pre-existing red repo-wide. Do NOT
    repo-wide-fix them again; scope any lint work to files you actually touch.
  * \`git clone\` over HTTPS FAILS inside docker containers here (a proxy 401s git's smart-HTTP).
    apt-get, plain HTTPS, and cargo's own libgit2 fetch all work fine. Never put \`git clone\` in a
    Dockerfile and never set CARGO_NET_GIT_FETCH_WITH_CLI=true.
  * Linux builds of the wayland crates: \`bash tools/wayland-build.sh <cargo args>\`. Never build
    smithay on the macOS host.

RULES FOR EVERY AGENT
  * NEVER ask the user anything. Decide, justify in a comment or in your structured output, continue.
  * VERIFY BEFORE YOU CLAIM. Paste the real command and its real output. A claim without a green
    command is a failed stage. Never describe a command you did not run.
  * E2E RUNS ARE THE SCARCE RESOURCE. A full suite run is 17-30 minutes and the machine flakes above
    2 workers. Do NOT run the full suite when a \`-g\` filtered subset answers your question. Never run
    two suites concurrently. Bash times out at 10 minutes — use run_in_background:true and poll, and
    do not pipe through \`tail\`/\`head\` (it buffers and you will see nothing until it exits); redirect
    to a file and poll the file instead.
  * Fix causes, not symptoms. Weakening or deleting an assertion to make a test pass is a failed stage
    unless you prove the assertion itself was wrong, and say so explicitly.
  * Commit when your stage is green, with this repo's message style (lowercase scope prefix, e.g.
    \`x11-server: fix WarpPointer target coordinate space\`). Never commit red. Never use --no-verify.
  * Work on a branch, not main: the Baseline phase creates \`x11-health\` and everything else builds on it.
`

const TRIAGE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['summary', 'hypotheses', 'evidence'],
	properties: {
		summary: { type: 'string' },
		hypotheses: {
			type: 'array',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['target', 'hypothesis', 'confidence', 'howToTest', 'suspectFiles'],
				properties: {
					target: { type: 'string', description: 'the test or subsystem this is about' },
					hypothesis: { type: 'string', description: 'the specific mechanism you believe is wrong' },
					confidence: { type: 'string', enum: ['high', 'medium', 'low'] },
					howToTest: { type: 'string', description: 'the cheapest command or probe that would confirm or kill it' },
					suspectFiles: { type: 'array', items: { type: 'string' } },
				},
			},
		},
		evidence: { type: 'array', items: { type: 'string' }, description: 'file:line or output backing the above' },
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

const STAGE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['fixed', 'notFixed', 'filesTouched', 'verification', 'handoff'],
	properties: {
		fixed: {
			type: 'array',
			description: 'One entry per test/issue you actually made pass, with the root cause',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['target', 'rootCause', 'fix', 'proof'],
				properties: {
					target: { type: 'string' },
					rootCause: { type: 'string' },
					fix: { type: 'string' },
					proof: { type: 'string', description: 'the command + outcome proving it passes now' },
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

// ---------------------------------------------------------------------------
// Phase 1 — Triage. Read-only and cheap: no e2e runs, so these can be parallel.
// ---------------------------------------------------------------------------

phase('Triage')
log('Three read-only triage agents — no e2e runs, so they can share the machine')

const triage = await parallel([
	() =>
		agent(
			`${CONTEXT}

YOUR JOB — TRIAGE ONLY. Read code and existing artifacts. Do NOT run the e2e suite (it is 30 minutes
and later phases own it) and do NOT edit any file.

Form the best hypothesis for EACH of the six deterministic failures. For each one:
  - Read the test body and every helper it calls (e2e/tests/fixtures.ts and friends).
  - Read the product code it exercises. The menu-bar three go through the DBus AppMenu path
    (crates/x11-server/src/menus.rs, MenuTracker, and the frontend's menu bar); WarpPointer is
    crates/x11-server's XTest/pointer handling plus frontend/src/canvasInputHooks.ts; the two xeyes
    ones are the display/PutImage path and the 16ms timer.
  - Read any leftover artifacts under e2e/test-results/ (screenshots, error-context.md, trace.zip
    listings) — a previous run left some there. \`pnpm exec playwright show-trace\` is interactive, so
    unzip and read the trace's JSON instead.
  - Check \`git log\` for when each test last passed and what changed around it. \`git log -S\` on a
    key symbol is often decisive.

Distinguish clearly between: the product is broken, the test is wrong, and the test is racy but
happens to lose deterministically. Say which for each, and give the CHEAPEST command that would
settle it.`,
			{ label: 'triage:deterministic-six', phase: 'Triage', effort: 'high', schema: TRIAGE_SCHEMA },
		),

	() =>
		agent(
			`${CONTEXT}

YOUR JOB — TRIAGE ONLY. Read code and artifacts; run NO e2e suite; edit NO file.

Explain the parallel-run flakiness — the ~7 rotating failures that pass at --workers=1 and fail at 2.
Read, in depth:
  - e2e/playwright.config.ts (fullyParallel, workers, timeouts) and what fullyParallel means when each
    worker owns one sidecar container: tests in the SAME file now share one X server across parallel
    slots. Work out precisely which shared state that exposes — X focus, the atom table, the dock's
    per-sidecar spawn button, leftover processes from a prior test, window stacking, the clipboard
    selection owner.
  - e2e/tests/fixtures.ts: container lifecycle, .withReuse keys, the per-worker network, and crucially
    whether anything CLEANS UP between tests in the same worker (killing spawned X clients, resetting
    focus). If nothing does, that is likely the whole story.
  - e2e/global-setup.ts / global-teardown.ts: orphan reaping.
  - The failing tests themselves: how many assume they are the only window on the canvas?
Also assess resource contention as a competing explanation (2 workers x 3 containers + a browser on
this machine) and say how you would tell the two apart cheaply.

Deliver a ranked set of hypotheses with the specific mechanism, and for the top one, the exact change
you would make (e.g. a per-test cleanup fixture that kills spawned clients, or test.describe.serial
on the files that share state).`,
			{ label: 'triage:flakiness', phase: 'Triage', effort: 'high', schema: TRIAGE_SCHEMA },
		),

	() =>
		agent(
			`${CONTEXT}

YOUR JOB — TRIAGE ONLY for the codec work. You MAY write and run throwaway benchmarks in a scratch
directory outside the repo, but do NOT edit repo files and do NOT run the e2e suite.

Design content-adaptive codec selection for crates/pixel-codec and its three callers.
  - Read crates/pixel-codec/src/lib.rs and all call sites (crates/sidecar/src/main.rs,
    crates/sidecar-wayland/src/linux.rs, crates/sidecar-macos/src/enumerator.rs).
  - Reproduce and EXTEND the benchmark table in the context above with your own numbers, on realistic
    inputs: solid-colour fills, xterm-like text, a GTK dialog's flat widgets, a gradient, a photo, and
    a mixed window (text over a photo). Include the tiny-rect cases (40x20, 200x100) since those
    dominate X11 traffic by count.
  - Design the SELECTOR: a cheap probe that decides lossless vs lossy per update without encoding
    twice. Candidates: unique-colour count on a subsample, mean absolute difference between adjacent
    pixels, a run-length estimate. Measure the probe's own cost — it must be a small fraction of the
    encode it saves — and measure its accuracy against "which codec actually won" on your corpus.
    Include the degenerate cases: 1x1 rects, single-colour rects, very small rects where the probe
    costs more than either encode.
  - Decide the API shape: a new \`encode_rgba_auto(rgba, w, h) -> Vec<u8>\` in pixel-codec is the
    obvious one, but check the wire/frontend side FIRST — does the frontend need to know which codec
    was used? \`createImageBitmap\` sniffs the WebP header, so probably not, but VERIFY it rather than
    assuming, and say what you verified.
  - Call out the risk: e2e has a screenshot-comparison test for xeyes
    (e2e/tests/x11-web.spec.ts-snapshots/). Say whether lossy could ever be selected for that content
    and what tolerance the comparison uses.

Deliver the design as hypotheses (target = the design decision, hypothesis = your recommendation,
howToTest = how the implementer proves it) plus your benchmark numbers in \`evidence\`.`,
			{ label: 'triage:codec', phase: 'Triage', effort: 'high', schema: TRIAGE_SCHEMA },
		),
])

const triageBrief = triage
	.filter(Boolean)
	.map(
		(t, i) =>
			`--- TRIAGE ${['deterministic-six', 'flakiness', 'codec'][i]} ---\n${t.summary}\n\nHYPOTHESES:\n${t.hypotheses
				.map((h) => `- [${h.confidence}] ${h.target}: ${h.hypothesis}\n    test: ${h.howToTest}\n    files: ${h.suspectFiles.join(', ')}`)
				.join('\n')}\n\nEVIDENCE:\n${t.evidence.map((e) => `- ${e}`).join('\n')}`,
	)
	.join('\n\n')

// ---------------------------------------------------------------------------
// Phase 2 — Baseline. One authoritative measurement. Serial by necessity.
// ---------------------------------------------------------------------------

phase('Baseline')

const baseline = await agent(
	`${CONTEXT}

TRIAGE OUTPUT:
${triageBrief}

YOUR JOB — establish the authoritative baseline everything else is measured against, and resolve the
one open question about the uncommitted sidecar change.

1. Confirm the starting point. You should already be on branch \`x11-health\`, one commit ahead of main
   (\`9b8df73 sidecar: move WebP encode off the events loop\` — the display_loop / spawn_blocking
   change described in the context). Verify with \`git log --oneline main..HEAD\` and \`git status\`.
   That commit was validated only narrowly: the four rendering-sensitive tests passed 4/4 at
   \`--workers=1\`. Your full baseline below is what really tests it. If your runs show a rendering
   regression that \`git stash\`/checkout-main bisection pins on that commit, fix or revert it and say so.
2. Take the baseline. Run the core suite TWICE, serially, redirecting to files:
     \`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core --reporter=list\`
   Record for each run: counts, the exact failing test list, and any "did not run".
   Then run the union of both runs' failures with \`--workers=1 --last-failed\` (or an explicit -g) and
   record which pass solo.
3. Partition the failures into: ALWAYS-FAILS (both runs + fails solo) = real bugs;
   ALWAYS-FAILS-PARALLEL-BUT-PASSES-SOLO = contention; ROTATING = flaky.
   Confirm or correct the six deterministic ones listed in the context — the list may have drifted.
4. Also record a green baseline for the things we must not regress: the Wayland suite
   (\`X11WEB_WAYLAND_E2E=1 ... tests/wayland\`), \`cargo test --workspace\`, and
   \`bash e2e/scripts/wayland-smoke.sh\`.

Write the whole partition into \`report\` in a form later phases can act on directly — an explicit list
under each heading, with file:line. This is the single most valuable artifact of the workflow; be
precise and do not guess. passed=true if you got a clean partition (not "if everything is green").`,
	{ label: 'baseline:measure', phase: 'Baseline', effort: 'high', schema: GATE_SCHEMA },
)

const BASELINE = baseline
	? `\nAUTHORITATIVE BASELINE (from the Baseline phase — trust this over the context's numbers):\n${baseline.report}\n`
	: '\n(Baseline phase returned nothing — re-measure before trusting any number, and create the x11-health branch yourself.)\n'

// ---------------------------------------------------------------------------
// Phases 3-6 — sequential implementation. Every stage owns the whole tree and
// the e2e machine in turn; a barrier between them is the point.
// ---------------------------------------------------------------------------

phase('DetFix')

const detfix = await agent(
	`${CONTEXT}
${BASELINE}

TRIAGE HYPOTHESES:
${triageBrief}

YOUR STAGE — fix the DETERMINISTIC failures (the ones that fail solo, not just in parallel). Work
through them in the Baseline's ALWAYS-FAILS list, cheapest-to-verify first.

For each: reproduce it solo with a \`-g\` filter, find the ROOT CAUSE in the product code, fix it, and
prove it passes solo. Then move on. Commit after each fix that goes green — small commits, one per
root cause, so a later bisect can find them.

Guidance per cluster, from the triage:
  - The three menu-bar tests likely share one cause (the DBus AppMenu path). Fix the shared cause once
    rather than three times, but verify all three.
  - WarpPointer is a coordinate-space question: check what the test asserts against what
    XTest/WarpPointer actually does in crates/x11-server, and which of the two is wrong.
  - The xeyes pair touches the display/PutImage path — the same path the sidecar change in Baseline
    step 1 touched. Be careful to distinguish a real bug from that change.

If a "failure" turns out to be a wrong assertion rather than a product bug, fix the TEST — but say so
explicitly in \`fixed[].rootCause\` and justify it. If you cannot fix one, put it in \`notFixed\` with
everything you learned; a later phase or a human picks it up. Do not leave it half-changed.

Before finishing: re-run the full set of tests you touched, solo, and confirm green.`,
	{ label: 'fix:deterministic', phase: 'DetFix', effort: 'high', schema: STAGE_SCHEMA },
)

const H_DET = detfix
	? `\nFROM DetFix:\nfixed: ${detfix.fixed.map((f) => `${f.target} (${f.rootCause})`).join('; ') || 'none'}\nnot fixed: ${detfix.notFixed.map((f) => f.target).join('; ') || 'none'}\n${detfix.handoff}\n`
	: '\n(DetFix returned nothing.)\n'

phase('Codec')

const codec = await agent(
	`${CONTEXT}
${BASELINE}
${H_DET}

CODEC DESIGN FROM TRIAGE:
${triage[2] ? `${triage[2].summary}\n${triage[2].hypotheses.map((h) => `- ${h.target}: ${h.hypothesis}`).join('\n')}\n\nBenchmarks:\n${triage[2].evidence.join('\n')}` : '(codec triage returned nothing — do the design work yourself, including the benchmarks)'}

YOUR STAGE — make the pixel codec content-adaptive.

  1. Implement the selector and \`encode_rgba_auto\` (or whatever shape the triage justified) in
     crates/pixel-codec, with unit tests covering: flat colour, text-like, photographic, 1x1,
     single-colour, and a rect small enough that the probe should short-circuit.
  2. Add a criterion-style or plain \`#[test]\`-driven benchmark that records the table in the context
     so the numbers live in the repo rather than in a chat log. Keep it out of the default test run if
     it is slow.
  3. Switch \`crates/sidecar\` and \`crates/sidecar-wayland\` to the adaptive entry point. Leave
     \`crates/sidecar-macos\` on lossy ONLY if the triage showed that is right for screen captures —
     otherwise switch it too. Say which you did and why.
  4. Fix the misleading docstring on \`encode_rgba_lossy\`.
  5. VERIFY, and this is the part that matters:
     - \`cargo test -p x11-web-pixel-codec\` green on the host.
     - \`bash tools/wayland-build.sh test -p x11-web-pixel-codec\` green on Linux.
     - The xeyes screenshot-comparison test still passes — run it solo. If adaptive selection changes
       what that test sees, that is a real signal, not an inconvenience: either the selector is wrong
       for that content or the snapshot needs regenerating for a defensible reason. Decide, and justify.
     - The Wayland suite still green: \`X11WEB_WAYLAND_E2E=1 ... tests/wayland\`.
     - A representative slice of rendering tests solo (xeyes pupils, firefox renders, multi-window).
  6. Commit.

Report the real before/after encode cost for at least the UI and photographic full-frame cases.`,
	{ label: 'fix:codec', phase: 'Codec', effort: 'high', schema: STAGE_SCHEMA },
)

const H_CODEC = codec
	? `\nFROM Codec:\nfixed: ${codec.fixed.map((f) => f.target).join('; ') || 'none'}\n${codec.handoff}\n`
	: '\n(Codec stage returned nothing.)\n'

phase('Flake')

const flake = await agent(
	`${CONTEXT}
${BASELINE}
${H_DET}${H_CODEC}

FLAKINESS HYPOTHESES FROM TRIAGE:
${triage[1] ? `${triage[1].summary}\n${triage[1].hypotheses.map((h) => `- [${h.confidence}] ${h.target}: ${h.hypothesis}\n    test: ${h.howToTest}`).join('\n')}` : '(flakiness triage returned nothing — derive it yourself)'}

STATE ON DISK — READ THIS FIRST. A previous attempt at THIS STAGE died mid-flight on a transient API
error (529 Overloaded), after doing real work. You are its continuation, not a fresh start. Before
anything else run \`git log --oneline main..HEAD\` and \`git status\`, and read \`git diff\`.
  * It had already committed its findings up to that point (expect commits touching
    \`e2e/tests/fixtures.ts\`, \`e2e/global-setup.ts\`, and an x11-server WindowDestroyed fix).
  * It left an UNCOMMITTED change to \`e2e/tests/fixtures.ts\`: \`spawnApp\` now waits for a
    \`data-client-id\` that was not in the pre-spawn snapshot, instead of waiting for the window-frame
    count to reach \`idsBefore.size + 1\`. Its recorded evidence: the count condition failed one test in
    each of three consecutive full runs — a different test each time, always the same call log
    ("resolved to 2 elements ... resolved to 1 element", expected 3, received 1) — because the window
    list is backend-authoritative and arrives asynchronously after \`page.goto\`, so the snapshot can be
    taken while rows from the previous test are still draining. Once two drain out, \`2 + 1\` is
    unreachable and a spawn that actually succeeded times out.
  * That reasoning looks sound and the new condition is strictly stronger than the old one, but it was
    NEVER VERIFIED — the agent died before finishing its runs. Your job is to verify it and land it,
    not to re-derive it. If your runs show it is wrong, fix or revert it and say so.
Do not redo analysis that is already committed. Build on it.

YOUR STAGE — make the suite trustworthy under parallel execution.

The deterministic bugs are (mostly) fixed by now, so what remains failing at 2 workers and passing at
1 is the contention/shared-state class. Attack the top-ranked mechanism from the triage.

Likely shapes of the fix, in rough order of preference — pick with evidence, not taste:
  - A per-test cleanup fixture that kills X clients spawned by the test and resets focus, so tests in
    the same worker stop inheriting each other's windows. This is the highest-leverage candidate if
    the triage confirmed nothing cleans up today.
  - \`test.describe.serial\` (or \`fullyParallel: false\` for specific files) where tests genuinely
    cannot share one X server.
  - Making individual assertions robust to a second window existing (scoping locators to the frame
    under test rather than the whole canvas).
  - Only if the evidence really points there: reducing default workers. Treat this as a LAST resort
    and say so — it trades wall-clock for a symptom, it does not fix the sharing.

PROVE IT. The bar is: run the core suite at the default 2 workers THREE times and show the failure
count is stable and materially lower than the Baseline's. One green run proves nothing on a suite this
noisy — and if you cannot get three runs in, say how many you did and what they showed. Report the
per-run failure lists, not just counts.

Do not weaken assertions to buy stability. If a test is fundamentally incompatible with sharing an X
server, serialise it and say why.`,
	{ label: 'fix:flakiness', phase: 'Flake', effort: 'high', schema: STAGE_SCHEMA },
)

const H_FLAKE = flake ? `\nFROM Flake:\n${flake.handoff}\n` : '\n(Flake stage returned nothing.)\n'

phase('Wayland')

const wayland = await agent(
	`${CONTEXT}
${BASELINE}
${H_DET}${H_CODEC}${H_FLAKE}

YOUR STAGE — close the known Wayland sidecar gaps. These were recorded as follow-ups when the sidecar
landed; verify each still applies before working on it (read the code — the descriptions may be stale).

  1. \`wl_seat.get_pointer\` catch-up: the keyboard path was fixed to send \`enter\` + \`modifiers\` when a
     client binds a keyboard while the seat already holds focus on its surface; the pointer twin was
     not. It self-heals on the next motion event, so it is cosmetic — but fix it properly: the seat
     needs to remember the last pointer position as well as the focused surface. Unit-test it.
  2. Bound the \`SidecarToBackend\` channel, or return the per-window frame credit after the QUIC write
     rather than after queue-for-write. As shipped, the credit scheme bounds the encoder but not a slow
     wire, so a stalled link still grows memory without limit. Pick one, implement it, and explain the
     trade — and make sure a disconnected/reconnecting sidecar cannot deadlock (there is a
     \`set_consuming\` bracket in run_session that exists precisely for that hazard; read it first).
  3. Re-check the X11 sidecar for the same unbounded-channel problem now that its encode moved off the
     select! loop. Remember: X11 PutImage is INCREMENTAL, so bounding it must not drop updates —
     if the only safe fix needs an x11-server-side coalescing/re-capture mechanism, do NOT build that
     here; write up precisely what it would take and leave it in \`notFixed\`.

Gates: \`cargo test --workspace\` (host), \`bash tools/wayland-build.sh test --workspace\` (Linux),
\`bash e2e/scripts/wayland-smoke.sh\`, and the Wayland e2e suite. Commit.`,
	{ label: 'fix:wayland-gaps', phase: 'Wayland', effort: 'high', schema: STAGE_SCHEMA },
)

// ---------------------------------------------------------------------------
// Phase 7 — Gate. Loop until the suite is genuinely better than the baseline.
// ---------------------------------------------------------------------------

phase('Gate')

let gate = null
let gateReport = 'not run'
for (let attempt = 1; attempt <= 2; attempt++) {
	const r = await agent(
		`${CONTEXT}
${BASELINE}
${H_DET}${H_CODEC}${H_FLAKE}
${wayland ? `FROM Wayland:\n${wayland.handoff}\n` : ''}
${attempt > 1 ? `PREVIOUS GATE ATTEMPT FAILED:\n${gateReport}\n\nFix the causes and re-gate.\n` : ''}

YOUR JOB — the final gate, attempt ${attempt} of 2. Prove the branch is better than the baseline and
broke nothing.

  1. \`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core --reporter=list\`
     TWICE at default workers. Report both failure lists in full.
  2. \`X11WEB_WAYLAND_E2E=1 pnpm --filter x11-web-e2e exec playwright test tests/wayland\` — must be green.
  3. \`bash e2e/scripts/wayland-smoke.sh\` — green.
  4. \`cargo test --workspace\` (host) and \`bash tools/wayland-build.sh test --workspace\` (Linux) — green.
  5. \`bash tools/wayland-build.sh clippy -p x11-web-sidecar -p x11-web-sidecar-wayland -p x11-web-wayland-server -p x11-web-pixel-codec -- -D warnings\` — green.
  6. \`pnpm turbo run typecheck\` — green. (Do NOT try to make repo-wide \`biome check\` green; it is
     pre-existing red. Check only that the files THIS branch touched are clean.)
  7. Compare against the Baseline partition explicitly: which of the six deterministic failures now
     pass, which contention failures are gone, and whether anything NEW is failing. A new failure is a
     blocker; a surviving old one is a follow-up.
  8. Clean up leaked containers and networks. Confirm \`git status\` is clean and
     \`git log --oneline main..HEAD\` reads sensibly.

passed=true only if: nothing new is failing, the Wayland suite and all build gates are green, and the
core suite's failure count is materially below the Baseline's. Be honest — reporting an ugly truth is
the job here. In \`report\`, give the before/after table and say plainly what is still broken.`,
		{ label: `gate:attempt-${attempt}`, phase: 'Gate', effort: 'high', schema: GATE_SCHEMA },
	)
	if (!r) {
		gateReport = 'agent returned nothing'
		continue
	}
	gateReport = r.report
	gate = r
	if (r.passed) {
		log(`Gate green on attempt ${attempt}`)
		break
	}
	log(`Gate attempt ${attempt} failed: ${r.remainingWork.slice(0, 3).join('; ')}`)
}

return {
	baseline: baseline?.report ?? 'no baseline',
	deterministic: detfix ? { fixed: detfix.fixed, notFixed: detfix.notFixed } : null,
	codec: codec ? { fixed: codec.fixed, notFixed: codec.notFixed, verification: codec.verification } : null,
	flakiness: flake ? { fixed: flake.fixed, notFixed: flake.notFixed, verification: flake.verification } : null,
	wayland: wayland ? { fixed: wayland.fixed, notFixed: wayland.notFixed } : null,
	gate: { passed: gate?.passed ?? false, report: gateReport, remainingWork: gate?.remainingWork ?? [] },
}
