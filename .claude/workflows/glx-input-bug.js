export const meta = {
	name: 'glx-input-bug',
	description:
		'Root-cause and fix the GLX input bug: advertising GLX leaves the GTK3 client that runs GDK visual probe unable to dispatch input',
	whenToUse: 'The blocker for shipping: GLX must be on by default AND Firefox/GTK3 must take input.',
	phases: [
		{ title: 'Diff', detail: 'find the divergence between the known-good and known-bad X servers' },
		{ title: 'Fix', detail: 'implement the fix and prove it on the reproducers' },
		{ title: 'Gate', detail: 'GLX on by default, full suites green' },
	],
}

const CONTEXT = `
REPO: /Users/knarf/projects/theknarf-experiments/x11-web  (branch main, tree clean, work here)
macOS host, Docker available. pnpm / cargo / turbo. Never npm or npx.
NEVER ask the user anything. Decide and proceed. VERIFY BEFORE YOU CLAIM: paste the real command and
its real output; a claim without a green command is a failed stage. Bash times out at 10 minutes —
use run_in_background:true and poll a redirect file, and NEVER pipe a long run through tail/head
(it buffers and you see nothing until exit).

============================ THE BUG ============================
When our X server advertises GLX, the GTK3 client that performs GDK's GLX-based visual probe becomes
unable to dispatch input — no hover, no click, no keys, permanently. GDK caches that probe's result in
a \`GDK_VISUALS\` property on the ROOT window, so every LATER GTK3 client on the same display skips the
probe and works fine. That is why it long looked like "only the first client is deaf" and like a
Firefox-specific bug. It is neither. Firefox is just the app users notice.

Because of this, GLX is currently DISABLED by default in
crates/x11-server/src/xserver/mod.rs (search X11WEB_ENABLE_GLX). THE GOAL IS TO DELETE THAT WORKAROUND:
GLX on by default, and Firefox + GTK3 still take input.

============================ REPRODUCERS ============================
Fast, ~2 min, no Firefox — the primary loop:
  X11_INPUT_DEBUG=1 X11WEB_ENABLE_GLX=1 pnpm --filter x11-web-e2e exec playwright test \\
    --workers=1 --repeat-each=2 -g "gtk3-demo reacts to hover"
  BROKEN: run 1 "HOVER changed=false / PRESS changed=false", run 2 changed=true.
  FIXED:  both runs changed=true.

Full, ~5 min, with a colour timeline and instruments:
  X11WEB_FIREFOX_DIAG=1 X11WEB_ENABLE_GLX=1 [DIAG_XTRACE=1] [DIAG_FF_ENV="env FOO=1"] \\
    pnpm --filter x11-web-e2e exec playwright test --workers=1 tests/firefox-input-diag.spec.ts
  BROKEN: hover/click/key all 0.000. FIXED: "DIAG HOVER WORKED" then CLICK and KEY.
  Read e2e/tests/firefox-input-diag.spec.ts's header first — it documents every instrument.

THE KEY ASSET — a known-good and known-bad X server side by side, same container, same Firefox
binary, same Mesa/llvmpipe stack:
  docker run -d --name X -e X11WEB_ENABLE_GLX=1 x11-web-sidecar-test:latest   # our server on :99
  docker exec -d X sh -c 'Xvfb :78 -screen 0 1024x768x24 >/tmp/xvfb.log 2>&1' # real server on :78
Firefox on :78 WORKS (its probe page goes blue on hover, confirmed by
\`import -window root /tmp/p.png && convert /tmp/p.png -resize 1x1 txt:\` showing #2929D7).
Firefox on :99 with GLX enabled is DEAF. Use \`xtrace -n -o /tmp/t.log <cmd>\` on both and diff.
Use \`docker exec -d\` to launch anything that must outlive the exec — \`nohup ... &\` inside a
non-detached exec dies with it. NOTE \`import\` returns a blank 2-colour image against :99, so pixel
capture only works on :78; on :99 measure via the e2e colour timeline or xtrace instead.

============================ ALREADY MEASURED — DO NOT RE-RUN ============================
* WE DELIVER CORRECTLY. A deaf Firefox still receives 2 EnterNotify, 1 LeaveNotify, 13 MotionNotify,
  with counts AND target windows byte-identical to the working GLX-off run. The client drops them.
* Firefox only QUERIES GLX (QueryServerString, QueryVersion, GetFBConfigs, GetVisualConfigs,
  ClientInfo). It never creates a context. No protocol errors anywhere in the trace.
* The deaf run does uniformly LESS startup work than the working one — QueryPointer 0 vs 6,
  GetWindowAttributes 3 vs 6, ConfigureNotify 6 vs 12, MapNotify 2 vs 4, no UnmapNotify, PropertyNotify
  164 vs 213. It is bailing out of an init path, not being starved of events. Xvfb answers 5
  QueryPointer in its working run. THIS IS THE SHARPEST UNEXPLAINED SIGNAL.
* Ruled out, each by experiment: page readiness; anything motion-specific (click and key equally
  dead); the GTK a11y bridge (NO_AT_BRIDGE=1 is already set image-wide); window tree, visual, depth,
  colormap and selected event masks (all identical between working and broken); the Firefox profile
  and the whole home directory; a startup-speed race (spawn->window 2.08s deaf vs 1.85s working); the
  advertised GLX extension list (3 -> 22 did not help and broke a glx.spec.ts assertion); the config
  replies (zero visual configs + zero FBConfigs did not help); direct rendering (generating the full
  FBConfig permutation set DID flip \`direct rendering\` No -> Yes matching Xvfb, Firefox stayed deaf and
  glxgears began segfaulting, reverted); Firefox's GL compositor (MOZ_ACCELERATED=0 MOZ_WEBRENDER=0
  still deaf, and gtk3-demo has no WebRender yet fails identically); the target-vs-deepest window
  mismatch in the browser input path (present in the working run too).
* One field-level difference WAS found this way and fixed in 6a9a87a: the crossing \`focus\` flag
  tested the tree backwards (ancestor-of-focus instead of the spec's inferior-of-focus). It did not
  fix this bug but it is the proof the method works — keep diffing FIELDS, not just message counts.

============================ CONSTRAINTS ============================
* Do not "fix" this by disabling GLX, by special-casing Firefox, or by shipping app-specific env vars
  or prefs. The product is "install the sidecar, remote-control ANY app, it just works".
* crates/x11-server is the X server. The browser input path is crates/x11-server/src/xserver/connection/mod.rs;
  crossing/event construction is crates/x11-server/src/xserver/input.rs; GLX is
  crates/x11-server/src/xserver/handlers/glx/ (~7.5k lines, genuinely functional — glxinfo reports a
  real llvmpipe context, OpenGL 4.5 compat).
* Must not regress: core suite is currently 332 passed / 0 failed
  (\`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core\`, ~15 min), and
  the GLX suite is 39/39 (\`X11WEB_GLX_E2E=1 X11WEB_ENABLE_GLX=1 ... tests/extensions/glx.spec.ts\`).
* Commit as you go, repo style (lowercase scope prefix). Never commit red. Never --no-verify.
`

const FINDING_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['divergence', 'evidence', 'proposedFix', 'confidence'],
	properties: {
		divergence: {
			type: 'string',
			description: 'The precise first point where the two servers differ, with the actual trace lines from each',
		},
		evidence: { type: 'array', items: { type: 'string' } },
		proposedFix: { type: 'string', description: 'What to change in crates/x11-server, and why that explains the symptom' },
		confidence: { type: 'string', enum: ['high', 'medium', 'low'] },
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

phase('Diff')

const angles = [
	{
		key: 'startup',
		prompt: `Diff the STARTUP traffic. Capture a full xtrace of the same GTK3 client against :78 (works)
and :99 with GLX enabled (deaf), in one container, and find the FIRST point where the two diverge —
before any input is injected. The deaf run does measurably less startup work (see the counts in the
brief), so there is a divergence to find and it happens during initialisation.
Work at the level of individual requests and REPLY FIELD VALUES, not message counts: the one real find
so far was a single wrong flag bit. Pay attention to anything GDK does around visual/FBConfig
selection, window creation, property writes (GDK_VISUALS especially) and the first XQueryPointer.
Use gtk3-demo rather than Firefox — it reproduces identically and starts far faster.`,
	},
	{
		key: 'gdk-source',
		prompt: `Work out from GDK's SOURCE what the probe does and which server answer could break it.
GTK3's GDK X11 backend is what goes wrong: it probes visuals through GLX, writes GDK_VISUALS on the
root, and thereafter the client cannot dispatch input. Find the actual GTK3/GDK code path (the
container has the distro's GTK3; source may not be present, so reason from the protocol trace plus
your knowledge of gdk_x11_screen_init_visuals / gdk_x11_screen_get_gl_visual / GdkX11GLContext, and
from Mesa's glx client code). Identify which specific reply value would make GDK take a path where it
stops translating X events into GDK events for that display — for instance a visual it selects for
its windows that then mismatches, an fbconfig with no matching visual, or a screen/depth combination
it cannot resolve. Then verify your candidate against the actual replies our server sends (capture
them with xtrace) and say precisely which of our values is wrong and what a real server answers.`,
	},
]

const findings = await parallel(
	angles.map((a) => () =>
		agent(`${CONTEXT}\n\nYOUR ANGLE (${a.key}):\n${a.prompt}\n\nDo not change product code in this phase; this is diagnosis. You MAY write throwaway scripts outside the repo.`, {
			label: `diff:${a.key}`,
			phase: 'Diff',
			effort: 'high',
			schema: FINDING_SCHEMA,
		}),
	),
)

const brief = findings
	.filter(Boolean)
	.map((f, i) => `--- ${angles[i].key} (confidence ${f.confidence}) ---\nDIVERGENCE: ${f.divergence}\nPROPOSED FIX: ${f.proposedFix}\nEVIDENCE:\n${f.evidence.map((e) => `  - ${e}`).join('\n')}`)
	.join('\n\n')

log(`Diff phase returned ${findings.filter(Boolean).length}/2 findings`)

phase('Fix')

let fix = null
let fixReport = 'not run'
for (let attempt = 1; attempt <= 3; attempt++) {
	const r = await agent(
		`${CONTEXT}

DIAGNOSIS FROM THE DIFF PHASE:
${brief || '(no findings returned — you must diagnose it yourself, using the method in the brief)'}

${attempt > 1 ? `PREVIOUS FIX ATTEMPT FAILED:\n${fixReport}\nDo not repeat it. Either fix the real cause or find a better diagnosis first.\n` : ''}

YOUR JOB — attempt ${attempt} of 3. Make GLX work with input.

  1. Implement the fix in crates/x11-server. If the diagnosis above is wrong or thin, diagnose it
     yourself first — the trace-diff method against :78 is documented in the brief and it works.
  2. Prove it on the fast reproducer, which must go from changed=false/changed=true to BOTH runs true:
       X11_INPUT_DEBUG=1 X11WEB_ENABLE_GLX=1 pnpm --filter x11-web-e2e exec playwright test \\
         --workers=1 --repeat-each=2 -g "gtk3-demo reacts to hover"
  3. Then prove it on Firefox:
       X11WEB_FIREFOX_DIAG=1 X11WEB_ENABLE_GLX=1 pnpm --filter x11-web-e2e exec playwright test \\
         --workers=1 tests/firefox-input-diag.spec.ts
     must print DIAG HOVER WORKED, DIAG CLICK WORKED, DIAG KEY WORKED.
  4. Only once both pass, remove the workaround: delete the X11WEB_ENABLE_GLX gate in
     crates/x11-server/src/xserver/mod.rs so GLX is advertised by default again, and undo the
     opt-in plumbing that exists only because of it — the X11WEB_GLX_E2E testIgnore entry in
     e2e/playwright.config.ts, the GLX_ENABLED conditionals and the two test.skip lines in
     e2e/tests/x11-web.spec.ts (restore "GLX" to the expected-extension lists and the count to 25),
     and the X11WEB_ENABLE_GLX pass-through in e2e/tests/fixtures.ts.
  5. Re-run both reproducers with NO env vars at all. They must still pass.
  6. Commit.

Set passed=true only if both reproducers pass with GLX on by default.`,
		{ label: `fix:attempt-${attempt}`, phase: 'Fix', effort: 'high', schema: GATE_SCHEMA },
	)
	if (!r) {
		fixReport = 'agent returned nothing'
		continue
	}
	fixReport = r.report
	fix = r
	if (r.passed) {
		log(`Fix green on attempt ${attempt}`)
		break
	}
	log(`Fix attempt ${attempt} failed: ${r.remainingWork.slice(0, 2).join('; ')}`)
}

phase('Gate')

const gate = await agent(
	`${CONTEXT}

FIX PHASE REPORT:
${fixReport}

YOUR JOB — independently re-verify. Re-run everything yourself; do not trust the report above.

  1. Both reproducers, with NO env vars (GLX must be on by default):
       X11_INPUT_DEBUG=1 ... -g "gtk3-demo reacts to hover"   -> both runs changed=true
       X11WEB_FIREFOX_DIAG=1 ... tests/firefox-input-diag.spec.ts -> HOVER/CLICK/KEY all WORKED
  2. \`grep -rn X11WEB_ENABLE_GLX crates/ e2e/\` — the workaround must be gone.
  3. GL still renders: run glxgears in a sidecar container and confirm frames-per-second output,
     not "glXCreateContext failed".
  4. The GLX suite: \`pnpm --filter x11-web-e2e exec playwright test tests/extensions/glx.spec.ts\`
     (it should no longer need X11WEB_GLX_E2E if the fix landed) — 39 passed.
  5. The core suite: \`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core\`
     — must be 332 passed / 0 failed, i.e. no regression. Background + poll; it takes ~15 minutes.
  6. \`cargo test --workspace\` and \`bash tools/wayland-build.sh test --workspace\`.
  7. \`git status\` clean, \`git log --oneline\` sensible.

passed=true only if every one of those is green. If the fix did not land, say so plainly and report
exactly what the Fix phase learned so the next session starts ahead of where this one did.`,
	{ label: 'gate', phase: 'Gate', effort: 'high', schema: GATE_SCHEMA },
)

return {
	diagnosis: brief || 'none',
	fix: fix ? { passed: fix.passed, report: fix.report } : null,
	gate: { passed: gate?.passed ?? false, report: gate?.report ?? 'no gate result', remainingWork: gate?.remainingWork ?? [] },
}
