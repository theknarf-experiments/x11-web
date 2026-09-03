export const meta = {
	name: 'wayland-sidecar',
	description:
		'Clone EVV1E/waylandcraft and derive a Wayland sidecar crate for x11-web, end-to-end, verified by a Docker build and a new Playwright e2e test',
	whenToUse:
		'Run to (re)drive the full waylandcraft-derived Wayland sidecar implementation in this repo. Resumable via resumeFromRunId.',
	phases: [
		{ title: 'Recon', detail: 'clone waylandcraft; map the sidecar contract, the upstream compositor, and the build/test infra' },
		{ title: 'Design', detail: 'synthesise one concrete file-by-file implementation plan' },
		{ title: 'Scaffold', detail: 'crates, workspace wiring, SidecarKind::Wayland, Linux cfg-gating, docker build harness' },
		{ title: 'Compositor', detail: 'smithay state, xdg-shell, shm buffers, surface -> DisplayUpdate::PutImage' },
		{ title: 'Input', detail: 'seat, xkb keyboard, pointer, focus, resize — InputEvent -> wl_seat' },
		{ title: 'Sidecar', detail: 'binary: process manager, QUIC link to backend, telemetry' },
		{ title: 'Package', detail: 'Dockerfile.sidecar-wayland, compose service, e2e fixture support' },
		{ title: 'Integrate', detail: 'make the whole workspace + image build green' },
		{ title: 'E2E', detail: 'new wayland e2e spec must pass; existing suite must not regress' },
		{ title: 'Review', detail: 'parallel review lenses over the diff' },
		{ title: 'Polish', detail: 'apply confirmed review findings, re-verify, commit' },
	],
}

// ---------------------------------------------------------------------------
// Shared context — verified facts about this repo, handed to every agent so
// they don't each re-derive the same ground truth.
// ---------------------------------------------------------------------------

const REPO = '/Users/knarf/projects/theknarf-experiments/x11-web'
const UPSTREAM_URL = 'https://github.com/EVV1E/waylandcraft'
const UPSTREAM_DIR = `${REPO}/tools/waylandcraft-upstream`

const CONTEXT = `
REPO: ${REPO}  (git branch: wayland-sidecar — already created, work directly on it)
Host is macOS (darwin/arm64). Docker is available (server 29.4.0). Package managers: pnpm + cargo + turbo.
Project rules (CLAUDE.md): use pnpm (never npm/npx), cargo for Rust, turbo for cross-project tasks.
"Commit intermittently as stuff works, ensure that stuff works and never commit anything that breaks e2e tests."

WHAT WE ARE BUILDING
A **Wayland sidecar**: a new sidecar binary that is to Wayland clients what the existing
X11 sidecar (crates/sidecar + crates/x11-server) is to X11 clients. It hosts an embedded
headless Wayland compositor (derived from ${UPSTREAM_URL}, a smithay-based compositor),
streams every toplevel surface to the backend as DisplayUpdate::PutImage, and injects
input events coming back from the browser.

EXISTING ARCHITECTURE (verified)
  Frontend (React canvas) <-WS-> Backend (axum, crates/backend) <-QUIC/capnp-> Sidecar
  - crates/wire      : QUIC + Cap'n Proto transport. dial(addr, server_name, fingerprint,
                       bearer_token, sidecar_name, SidecarKind) -> DialedConnection{reader,writer}.
                       SidecarKind lives in crates/wire/src/conn.rs (Unknown/X11/Macos) and in
                       the schema crates/wire/schema/wire.capnp (enum SidecarKind { unknown @0;
                       x11 @1; macos @2; }). Messages: SidecarToBackend / BackendToSidecar.
  - crates/protocol  : DisplayUpdate + InputEvent types (crates/protocol/src/lib.rs, ~496 lines).
  - crates/x11-server: embeddable X11 server library. Emits TaggedDisplayUpdate = (client_id, DisplayUpdate)
                       into an mpsc channel; WindowRouter routes input/resize back per window_id.
  - crates/sidecar   : the X11 sidecar binary (crates/sidecar/src/main.rs, ~840 lines). THIS IS THE
                       TEMPLATE for the new binary: ProcessManager (spawns children with DISPLAY set),
                       dbus session, fingerprint/QUIC dial + reconnect loop, run_session() with a recv
                       loop and an events loop, encode_for_wire() which WebP-lossless-encodes PutImage
                       via x11-web-pixel-codec, telemetry.rs, heartbeat.
  - crates/sidecar-macos: a SECOND sidecar. Read its Cargo.toml — it is the pattern for a sidecar whose
                       real code only compiles on one OS ([target.'cfg(target_os = "macos")'.dependencies]
                       + a stub lib/bin elsewhere) so \`cargo check --workspace\` still passes on other hosts.
  - crates/backend/src/main.rs lines ~1135 and ~1285 branch on SidecarKind (X11|Unknown => auto-stream
                       every window). The Wayland sidecar auto-streams, so it must be treated like X11 there.
  - Dockerfile.sidecar: multi-stage; builder copies ONLY the crates the sidecar depends on and stubs the
                       rest of the workspace members so the workspace manifest resolves.
  - compose.yml       : \`sidecar\` service; BACKEND_QUIC_ADDR=host.docker.internal:3002, fingerprint
                       bind-mounted from \${HOME}/.x11web-fingerprint.
  - e2e/              : Playwright + testcontainers. e2e/tests/fixtures.ts starts a per-worker
                       (mock-oidc, backend, sidecar) trio on a private docker network and builds the
                       frontend once in global-setup.ts. Tests live in e2e/tests/**.spec.ts.

UPSTREAM (waylandcraft, GPLv3)
  ${UPSTREAM_URL} — cloned to ${UPSTREAM_DIR} (gitignored reference checkout).
  native/ is a Rust cdylib crate "waylandcraft" (edition 2024) using smithay (git rev 89e58f7,
  default-features off, features wayland_frontend + backend_drm), jni, xkbcommon, libc, rustix.
  native/src: lib.rs (WaylandCraft state + delegate_* wiring + calloop event loop + ListeningSocketSource),
  bridge.rs (2024 lines: JNI surface — buffer attach for shm / single-pixel / dmabuf, toplevel+popup
  enumeration, surface tree walk, pointer/keyboard entrypoints, output sizing, titles/app_ids, resize),
  seat.rs (1067 lines: xkb keymap, pointer/keyboard/touch focus + serials), ddm.rs (734: wl_data_device,
  clipboard + DnD), egl.rs (358: EGL/dmabuf import), xdg_spec.rs (213: .desktop entries), satellite.rs
  (285: spawning xwayland-satellite), output.rs, process.rs, svg.rs, utils.rs, java_types.rs.
  The Minecraft/JNI half is NOT reusable. The reusable half is the compositor: smithay state wiring,
  xdg-shell handling, buffer attach (shm path especially), seat/input/xkb, and clipboard.

LICENSING (decided, do not re-litigate)
  waylandcraft is GPLv3. x11-web currently ships no LICENSE file. Any code derived from waylandcraft
  is a derivative work: keep every derived file in the new crate, put a GPLv3 attribution header at the
  top of each derived file naming the upstream project, commit hash and license, and add
  crates/wayland-server/NOTICE recording the provenance. Do NOT copy the upstream code into unrelated
  crates. Do not add a repo-wide LICENSE file (that is the repo owner's call) — the NOTICE is enough.

HARD CONSTRAINTS
  1. \`cargo check --workspace\` MUST still pass on this macOS host. smithay/wayland only build on Linux,
     so the new crates must gate all real dependencies + code behind cfg(target_os = "linux") exactly like
     crates/sidecar-macos gates macOS. On macOS they must compile to a stub that exits with a message.
  2. The real Linux build/typecheck happens in Docker. Use the helper the Scaffold phase creates
     (tools/wayland-build.sh) — it runs cargo inside rust:1-bookworm with persistent named volumes for
     the cargo registry and the target dir, so rebuilds are incremental. NEVER cargo-build smithay on the host.
  3. Do NOT break anything that exists. No behaviour change to the X11 sidecar, x11-server, frontend or
     backend beyond the additive SidecarKind::Wayland plumbing.
  4. Existing e2e tests must still pass. New e2e work goes in a new spec file.
  5. NEVER ask the user a question and never stop to request confirmation. Decide, justify in a comment,
     and continue. If something is impossible, implement the closest workable thing and record the
     deviation in your structured output.
  6. Long commands: Bash tool timeout is 10 minutes max. Docker builds and cargo builds of smithay WILL
     exceed that — run them with run_in_background:true and poll the output, or split them into stages.
`

const SCOPE = `
SCOPE OF THE VERTICAL SLICE (deliberately bounded — build this, and only this, well)
  IN:  wl_compositor, wl_shm (software buffers), wl_subcompositor if cheap, xdg_shell (toplevel + popup),
       wl_seat (keyboard via xkbcommon + pointer), wl_output, viewporter, single-pixel-buffer,
       xdg_decoration (server-side / no decoration), surface commit -> damage -> RGBA readback ->
       DisplayUpdate::PutImage, toplevel map/unmap/title/app_id -> the window lifecycle DisplayUpdate
       variants the frontend already understands, resize via xdg_toplevel configure.
  OUT: dmabuf / EGL / GPU import (shm only — the sidecar runs headless in a container),
       xwayland-satellite / XWayland, drag-and-drop, the .desktop app launcher, SVG icons, touch,
       tablet, Minecraft/JNI anything. Clipboard is optional; skip it unless it falls out for free.
  The success criterion is a Wayland client rendering pixels into the browser canvas and reacting to
  a click and a keystroke — not protocol completeness.
`

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

const RECON_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['summary', 'facts', 'risks'],
	properties: {
		summary: { type: 'string', description: 'Dense prose brief for the designer, <= 500 words' },
		facts: {
			type: 'array',
			description: 'Concrete, verified facts: exact file:line, type name, signature, env var, command',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['fact', 'evidence'],
				properties: {
					fact: { type: 'string' },
					evidence: { type: 'string', description: 'file:line or command output that proves it' },
				},
			},
		},
		risks: { type: 'array', items: { type: 'string' } },
	},
}

const PLAN_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['overview', 'files', 'dependencies', 'verification', 'openDecisions'],
	properties: {
		overview: { type: 'string' },
		files: {
			type: 'array',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['path', 'purpose', 'phase'],
				properties: {
					path: { type: 'string' },
					purpose: { type: 'string' },
					phase: {
						type: 'string',
						enum: ['Scaffold', 'Compositor', 'Input', 'Sidecar', 'Package'],
					},
					derivedFrom: { type: 'string', description: 'upstream file it is derived from, or "" if original' },
				},
			},
		},
		dependencies: {
			type: 'array',
			description: 'Exact crate + version/rev to put in Cargo.toml, with why',
			items: { type: 'string' },
		},
		verification: { type: 'array', items: { type: 'string' }, description: 'Exact commands that prove each phase works' },
		openDecisions: {
			type: 'array',
			description: 'Decisions the designer made unilaterally (there is no user to ask)',
			items: { type: 'string' },
		},
	},
}

const STAGE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['done', 'filesTouched', 'verification', 'handoff', 'deviations'],
	properties: {
		done: { type: 'boolean', description: 'true only if the stage compiles/verifies as specified' },
		filesTouched: { type: 'array', items: { type: 'string' } },
		verification: { type: 'string', description: 'the exact command(s) run and their real outcome — never claim a command you did not run' },
		handoff: { type: 'string', description: 'What the next stage needs to know: type names, signatures, TODOs left, gotchas' },
		deviations: { type: 'array', items: { type: 'string' } },
	},
}

const GATE_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['passed', 'report', 'remainingWork'],
	properties: {
		passed: { type: 'boolean' },
		report: { type: 'string', description: 'Commands run, verbatim key output, what failed and why' },
		remainingWork: { type: 'array', items: { type: 'string' } },
	},
}

const FINDINGS_SCHEMA = {
	type: 'object',
	additionalProperties: false,
	required: ['findings'],
	properties: {
		findings: {
			type: 'array',
			items: {
				type: 'object',
				additionalProperties: false,
				required: ['file', 'summary', 'severity', 'fix'],
				properties: {
					file: { type: 'string' },
					line: { type: 'number' },
					summary: { type: 'string' },
					severity: { type: 'string', enum: ['blocker', 'major', 'minor'] },
					fix: { type: 'string' },
				},
			},
		},
	},
}

// ---------------------------------------------------------------------------
// Phase 1 — Recon (parallel, read-only)
// ---------------------------------------------------------------------------

phase('Recon')
log('Cloning waylandcraft and mapping three territories in parallel')

const recon = await parallel([
	() =>
		agent(
			`${CONTEXT}

YOUR JOB (read-only recon — write NO source files):
Map the **sidecar contract** of x11-web precisely enough that someone can write a brand-new sidecar
from scratch without reading the repo again.

Read and report on:
  - crates/sidecar/src/main.rs end to end: startup order, fingerprint sourcing, dial + reconnect loop,
    run_session's recv/events loops, every channel and what flows through it, heartbeat, telemetry init.
  - crates/protocol/src/lib.rs: EVERY DisplayUpdate variant and EVERY InputEvent variant, with fields
    and exact semantics (coordinate spaces, id types, what the frontend does with each). Say precisely
    which variants a new sidecar MUST emit for a window to appear, be titled, be resized and be closed.
  - crates/wire/src/{lib,conn,bridge,types}.rs: SidecarToBackend / BackendToSidecar variants, dial()
    signature, SidecarKind, and the exact edit needed to add a \`wayland\` kind (capnp schema + Rust
    enum + both translation match arms + backend/src/main.rs:1143 and :1286 branches).
  - crates/x11-server/src/lib.rs public surface: X11Server::new signature, WindowRouter, MenuTracker,
    TaggedDisplayUpdate — i.e. the shape a "server library" exposes to a sidecar binary, which the new
    wayland-server crate should mirror.
  - crates/pixel-codec: encode_rgba_lossless signature and the pixel byte order it expects
    (the repo notes the framebuffer is RGBA storage while the X11 wire is BGRA — state which one
    encode_rgba_lossless wants).
  - crates/telemetry + crates/sidecar/src/telemetry.rs: what a new binary must copy.

Output the structured brief. Facts must carry file:line evidence. Be exhaustive on the protocol
variants — the implementers will code straight off your list.`,
			{ label: 'recon:sidecar-contract', phase: 'Recon', schema: RECON_SCHEMA },
		),

	() =>
		agent(
			`${CONTEXT}
${SCOPE}

YOUR JOB (recon):
FIRST, clone the upstream reference:
  git clone --depth 1 ${UPSTREAM_URL} ${UPSTREAM_DIR}
  (if the directory already exists, \`git -C ${UPSTREAM_DIR} pull --ff-only\` instead)
Then record the resolved commit: \`git -C ${UPSTREAM_DIR} rev-parse HEAD\` — later phases need it for
the GPLv3 attribution headers. Add \`tools/waylandcraft-upstream/\` to ${REPO}/.gitignore (this is the
ONLY file you may write) so the reference checkout is never committed.

Then map **waylandcraft's compositor** and report what is reusable for a headless, software-rendered,
container-hosted compositor:
  - native/Cargo.toml: exact smithay git rev + feature flags, and every other dependency with version.
    Judge which of those features we actually need with dmabuf/EGL out of scope, and whether
    edition 2024 is required (this workspace is edition 2021 — say whether the new crate can be 2021).
  - native/src/lib.rs: the full state struct, every delegate_* macro, the calloop event loop,
    ListeningSocketSource + how WAYLAND_DISPLAY is derived, client data, and the dispatch cadence.
  - native/src/bridge.rs: try_attach_shm / try_attach_single_pixel / try_attach_buffer /
    update_surface_data / update_surface_tree — how a committed buffer becomes pixels, what format and
    stride, and how the surface tree (subsurfaces, popups) is walked. Ignore all the JNI marshalling;
    extract the compositor logic underneath. Also: toplevel/popup enumeration, titles, app_ids,
    xdg geometry, resize/maximize/fullscreen request handling, output sizing.
  - native/src/seat.rs: xkb keymap creation, keyboard focus + enter/leave, key serials, pointer
    motion/button/axis, and what a caller must feed it. Note the xkbcommon crate version and the
    system libs it needs (libxkbcommon + xkbcli per the README).
  - Anything Minecraft/JNI-specific that must be stripped, and anything subtly load-bearing that
    would break if naively stripped.

For each reusable piece, say: port-as-is / port-with-changes / rewrite-from-smithay-docs — and why.
Flag any place where upstream depends on a GPU/EGL/DRM path we cannot have in a headless container.`,
			{ label: 'recon:upstream-compositor', phase: 'Recon', schema: RECON_SCHEMA },
		),

	() =>
		agent(
			`${CONTEXT}
${SCOPE}

YOUR JOB (read-only recon — write NO files):
Map the **build, container and test infrastructure** so the new sidecar slots in without friction.

Read and report on:
  - Dockerfile.sidecar in full: every stage, why the workspace members get stubbed, the runtime image's
    apt packages, the entrypoint, and exactly what a sibling Dockerfile.sidecar-wayland must change.
    List the Debian bookworm packages a headless Wayland compositor host needs (libxkbcommon, xkbcli /
    xkb-data, libwayland, seatd? etc.) and — importantly — which Wayland TEST CLIENTS are installable in
    bookworm. Investigate concretely (run \`docker run --rm debian:bookworm-slim apt-get ... \` style
    probes, or \`apt-cache\` inside a container) and report which of these actually exist and which
    binaries they ship: weston (weston-simple-shm / weston-info / weston-terminal), gtk-3-examples
    (gtk3-demo), foot, wayland-utils (wayland-info), qtwayland5. Recommend ONE minimal pure-shm client
    for the first e2e assertion and ONE richer client (a GTK app) as the stretch case.
  - compose.yml and compose.dev.yml: the sidecar service shape, env vars, fingerprint mount, extra_hosts.
  - e2e/tests/fixtures.ts in full: how containers are started per worker, network aliases, .withReuse
    keys, the exported test fixtures, page/canvas helpers, how a test spawns a process on the sidecar
    and asserts pixels. Say exactly what it takes to add a SECOND sidecar image (a wayland one) to the
    harness with the least intrusion — ideally a separate opt-in fixture or spec-level container start,
    NOT a change to the default trio (which must keep working unchanged).
  - e2e/tests/x11-web.spec.ts (and one apps/ spec): the idioms for asserting a window appeared and that
    pixels are non-blank; the exact helper functions to reuse.
  - turbo.json, package.json, .hooks/pre-commit, biome.json: what CI-ish gates run on commit and what
    the new code must satisfy (biome formatting of any new TS!).

Output the structured brief with file:line evidence and the verified list of available test clients.`,
			{ label: 'recon:infra-and-e2e', phase: 'Recon', schema: RECON_SCHEMA },
		),
])

const reconBrief = recon
	.filter(Boolean)
	.map((r, i) => `--- RECON ${i + 1} ---\n${r.summary}\n\nFACTS:\n${r.facts.map((f) => `- ${f.fact}  [${f.evidence}]`).join('\n')}\n\nRISKS:\n${r.risks.map((x) => `- ${x}`).join('\n')}`)
	.join('\n\n')

if (recon.filter(Boolean).length < 3) log(`WARNING: only ${recon.filter(Boolean).length}/3 recon agents returned`)

// ---------------------------------------------------------------------------
// Phase 2 — Design
// ---------------------------------------------------------------------------

phase('Design')

const plan = await agent(
	`${CONTEXT}
${SCOPE}

RECON BRIEFS FROM THREE AGENTS:
${reconBrief}

YOUR JOB (design — write NO source files; produce the plan the implementers will follow):
Produce ONE concrete, file-by-file implementation plan for the Wayland sidecar. You may read the repo
and ${UPSTREAM_DIR} to check anything the briefs left ambiguous.

Decide and pin down:
  - Crate split. Default: \`crates/wayland-server\` (library: the headless compositor, mirroring how
    crates/x11-server serves crates/sidecar) + \`crates/sidecar-wayland\` (binary, mirroring
    crates/sidecar/src/main.rs). Confirm or change with reasons.
  - The library's public API: the exact \`WaylandServer::new(...)\` signature, the channels it takes
    (display updates as TaggedDisplayUpdate, client-connected, screen size watch), the router type for
    input/resize, \`wayland_display_name()\`, and \`run()\`. It must be a drop-in analogue of X11Server
    so the binary reads like crates/sidecar/src/main.rs.
  - Exact Cargo.toml contents for both crates, with the Linux cfg-gating pattern from
    crates/sidecar-macos, and the stub-on-macOS shape. Pin smithay to the SAME git rev upstream uses
    (89e58f7) unless the recon proves a crates.io release works — a pinned rev is safer.
  - How a committed wl_surface buffer becomes DisplayUpdate::PutImage: format conversion (shm formats
    Argb8888/Xrgb8888 are little-endian BGRA in memory — state the exact swizzle to the RGBA the
    pixel-codec wants), damage tracking, throttling/frame callbacks, and per-toplevel window ids.
  - How BackendToSidecar::InputEvent maps onto wl_seat pointer/keyboard calls, including keycode
    translation (the frontend sends what? — check crates/protocol) to Linux evdev keycodes + 8 offset,
    modifier state, and serial handling.
  - The threading model: smithay is single-threaded calloop, the sidecar is tokio. Specify exactly how
    they meet (dedicated OS thread running calloop + std mpsc/tokio unbounded channels across the
    boundary), and which types must be Send.
  - The Docker + compose + e2e delta, naming the ONE test client the e2e spec will spawn.
  - The verification command for each phase, runnable as-is.

Assign every file to exactly one phase: Scaffold, Compositor, Input, Sidecar, Package.
There is no user to ask: make every call yourself and list them in openDecisions.`,
	{ label: 'design:plan', phase: 'Design', effort: 'high', schema: PLAN_SCHEMA },
)

if (!plan) throw new Error('Design phase produced no plan; aborting')

const PLAN_TEXT = `
IMPLEMENTATION PLAN (authoritative — follow it; deviate only with a recorded reason)

${plan.overview}

FILES:
${plan.files.map((f) => `- [${f.phase}] ${f.path} — ${f.purpose}${f.derivedFrom ? `  (derived from ${f.derivedFrom})` : ''}`).join('\n')}

DEPENDENCIES:
${plan.dependencies.map((d) => `- ${d}`).join('\n')}

VERIFICATION:
${plan.verification.map((v) => `- ${v}`).join('\n')}

DECISIONS ALREADY MADE (do not revisit):
${plan.openDecisions.map((d) => `- ${d}`).join('\n')}
`

log(`Plan covers ${plan.files.length} files across 5 implementation phases`)

// ---------------------------------------------------------------------------
// Phases 3-7 — Implementation. Strictly sequential: every agent edits the same
// working tree, so a barrier between each is the point, not an accident.
// ---------------------------------------------------------------------------

const IMPL_RULES = `
RULES FOR THIS IMPLEMENTATION STAGE
  - Write real, complete code. No \`todo!()\` in a path the e2e test will hit, no placeholder modules,
    no "left as an exercise". If you must stub something out of scope, stub it deliberately and say so.
  - Match the surrounding code's style: this repo writes dense explanatory block comments above
    non-obvious code explaining WHY. Copy that register — read a neighbouring file first.
  - Every file derived from waylandcraft gets a header comment: upstream URL, upstream commit,
    "GPLv3 — see crates/wayland-server/NOTICE", and a one-line note on what was changed.
  - VERIFY BEFORE YOU CLAIM. Run the build. Paste real output into \`verification\`. A stage that
    reports done:true without a green command is a failed stage.
  - Host builds: \`cargo check --workspace\` on macOS must stay green (stub path).
    Linux builds: \`bash tools/wayland-build.sh <cargo args>\` — run it with run_in_background:true and
    poll, because a cold smithay build takes far longer than the 10-minute Bash timeout.
  - Commit when your stage is green: \`git add -A && git commit\` with a message in this repo's style
    (lowercase scope prefix, e.g. \`wayland-server: shm buffer readback -> PutImage\`). Never commit red.
  - Never ask the user anything. Decide and proceed.
`

phase('Scaffold')

const scaffold = await agent(
	`${CONTEXT}
${SCOPE}
${PLAN_TEXT}
${IMPL_RULES}

YOUR STAGE — Scaffold. Deliver, in this order:

1. \`tools/wayland-build.sh\` — the Linux build harness every later stage depends on. It must:
   - run cargo inside \`rust:1-bookworm\` with the repo bind-mounted at /app,
   - install the build deps the compositor needs (libxkbcommon-dev, libwayland-dev, capnproto, cmake,
     pkg-config, libudev-dev/libinput-dev/libseat-dev only if smithay's chosen features need them),
     doing so in a **prebuilt image** (write \`Dockerfile.wayland-builder\` and have the script
     \`docker build\` it once, tagged \`x11web-wayland-builder\`) rather than apt-getting on every run,
   - mount persistent named volumes for \`/usr/local/cargo/registry\`, \`/usr/local/cargo/git\` and a
     Linux-only target dir (do NOT share the host's ./target — different platform, it will thrash),
   - handle the x11rb fork setup that Dockerfile.sidecar does (tools/setup-x11rb-fork.sh) since the
     workspace's [patch.crates-io] needs it,
   - pass "\$@" straight through to cargo, and be executable (chmod +x).
   Prove it works: \`bash tools/wayland-build.sh check -p x11-web-protocol\` must go green before you
   move on.

2. Both new crates, compiling but empty of logic:
   - \`crates/wayland-server/\` — lib crate, Linux-gated deps, a macOS stub, Cargo.toml per the plan,
     src/lib.rs exposing the planned public API with the real type signatures and \`unimplemented\`
     bodies ONLY where the next stage will fill them in (mark each with a \`// STAGE: Compositor\` /
     \`// STAGE: Input\` comment so the next agents can find them).
   - \`crates/sidecar-wayland/\` — bin crate, same gating, main.rs that compiles and exits with a clear
     message on non-Linux.
   - Add both to the workspace members in Cargo.toml, in the right place.
   - \`crates/wayland-server/NOTICE\` with the GPLv3 provenance (upstream URL + the exact commit hash
     recorded during recon).

3. The SidecarKind::Wayland plumbing, additive and complete:
   - \`wayland @3\` in crates/wire/schema/wire.capnp's enum,
   - \`Wayland\` in the Rust \`SidecarKind\` (crates/wire/src/conn.rs) plus BOTH translation match arms,
   - crates/backend/src/main.rs: the two \`matches!(kind, SidecarKind::X11 | SidecarKind::Unknown)\`
     branches must also accept Wayland (it auto-streams like X11). Read the surrounding code first and
     update the comments that explain the branch.
   - Anywhere else the enum is matched exhaustively.

4. Gates, all green, pasted into \`verification\`:
   - \`cargo check --workspace\` on the macOS host,
   - \`bash tools/wayland-build.sh check --workspace\` (background + poll),
   - \`git commit\`.

In \`handoff\`, give the next agent the exact public API you scaffolded (signatures verbatim) and where
the STAGE markers are.`,
	{ label: 'impl:scaffold', phase: 'Scaffold', effort: 'high', schema: STAGE_SCHEMA },
)

const h1 = scaffold ? `\nHANDOFF FROM Scaffold:\n${scaffold.handoff}\nDeviations: ${scaffold.deviations.join('; ') || 'none'}\n` : '\n(Scaffold stage returned nothing — inspect the tree yourself before starting.)\n'

phase('Compositor')

const compositor = await agent(
	`${CONTEXT}
${SCOPE}
${PLAN_TEXT}
${IMPL_RULES}
${h1}

YOUR STAGE — Compositor. Fill in \`crates/wayland-server\` so a Wayland client can connect, map a
toplevel, and have its pixels arrive as DisplayUpdate::PutImage. Port from ${UPSTREAM_DIR}/native/src
(lib.rs + the compositor logic buried in bridge.rs) — read the upstream code properly, it is your
best documentation for this smithay revision.

Deliver:
  - The compositor state struct + every needed delegate_* (compositor, shm, xdg_shell, seat placeholder,
    output, viewporter, single-pixel-buffer), the calloop event loop, ListeningSocketSource, and a
    \`wayland_display_name()\` (e.g. \`wayland-1\`) plus WAYLAND_DISPLAY / XDG_RUNTIME_DIR handling.
  - The calloop-thread <-> tokio bridge exactly as the plan specifies.
  - Surface commit handling: buffer attach (shm; keep single-pixel as a cheap freebie), damage
    accumulation, the surface tree walk for subsurfaces, and readback into RGBA. Get the byte order
    right — verify it against crates/pixel-codec's expectations rather than guessing, and leave a
    comment recording the reasoning.
  - Window lifecycle -> the DisplayUpdate variants the frontend already handles: window created with
    geometry, title / app_id changes, unmap/destroy. Use the recon brief's variant list; do not invent
    new protocol messages, and do NOT change crates/protocol.
  - Frame callbacks so clients keep drawing, and enough throttling that a busy client doesn't flood
    the channel.
  - xdg_toplevel configure plumbing so a resize request from the router reaches the client.
  - Unit tests where they're cheap and meaningful (buffer swizzle, damage merge, id mapping) —
    \`#[cfg(all(test, target_os = "linux"))]\`.

Gate: \`bash tools/wayland-build.sh test -p x11-web-wayland-server\` green (background + poll), plus
\`cargo check --workspace\` still green on the host. Then commit.

In \`handoff\`, tell the Input stage the exact seat/router hooks you left open.`,
	{ label: 'impl:compositor', phase: 'Compositor', effort: 'high', schema: STAGE_SCHEMA },
)

const h2 = compositor ? `\nHANDOFF FROM Compositor:\n${compositor.handoff}\nDeviations: ${compositor.deviations.join('; ') || 'none'}\n` : '\n(Compositor stage returned nothing — inspect the tree yourself.)\n'

phase('Input')

const input = await agent(
	`${CONTEXT}
${SCOPE}
${PLAN_TEXT}
${IMPL_RULES}
${h1}${h2}

YOUR STAGE — Input. Make the compositor accept input from the browser. Port from
${UPSTREAM_DIR}/native/src/seat.rs (1067 lines — read it; it encodes hard-won serial/focus details)
and the pointer/keyboard entrypoints in bridge.rs.

Deliver:
  - wl_seat with keyboard + pointer capabilities, an xkbcommon keymap built at startup (us layout
    default, overridable by env), and the keymap fd handed to clients.
  - A \`WaylandRouter\` (analogue of x11-server's WindowRouter) that the sidecar binary calls with
    \`send_input(window_id, InputEvent)\` and \`send_resize(window_id, w, h)\`, delivering into the
    calloop thread.
  - Full mapping of every InputEvent variant in crates/protocol to wl_pointer / wl_keyboard:
    motion (surface-local coordinates — get the transform from window origin right), button
    (frontend button ids -> BTN_LEFT/RIGHT/MIDDLE 0x110/0x111/0x112), axis/scroll (including
    discrete/value120 if the smithay rev supports it), key press/release with keycode + 8 evdev offset,
    modifier state updates, and enter/leave as pointer focus moves between toplevels.
  - Keyboard focus follows the frontend's focus signal; send wl_keyboard enter/leave and correct serials.
  - Unit tests for the keycode and button mappings.

Gate: \`bash tools/wayland-build.sh test -p x11-web-wayland-server\` green, host \`cargo check --workspace\`
green, commit.`,
	{ label: 'impl:input', phase: 'Input', effort: 'high', schema: STAGE_SCHEMA },
)

const h3 = input ? `\nHANDOFF FROM Input:\n${input.handoff}\nDeviations: ${input.deviations.join('; ') || 'none'}\n` : '\n(Input stage returned nothing — inspect the tree yourself.)\n'

phase('Sidecar')

const sidecarStage = await agent(
	`${CONTEXT}
${SCOPE}
${PLAN_TEXT}
${IMPL_RULES}
${h1}${h2}${h3}

YOUR STAGE — the sidecar binary, \`crates/sidecar-wayland\`.

Read crates/sidecar/src/main.rs line by line and write the Wayland analogue. Keep the structure
recognisably parallel — a reviewer should be able to diff them mentally. Specifically:
  - ProcessManager that spawns children with WAYLAND_DISPLAY (+ XDG_RUNTIME_DIR, GDK_BACKEND=wayland,
    QT_QPA_PLATFORM=wayland, MOZ_ENABLE_WAYLAND=1, SDL_VIDEODRIVER=wayland — waylandcraft's bridge.rs
    sets a similar env set near line 1888, use it as the reference), draining child stdout/stderr into
    the log exactly like the X11 sidecar does.
  - The same fingerprint sourcing, DNS-resolving dial loop, reconnect delay, heartbeat and
    \`SidecarKind::Wayland\` handshake.
  - run_session with the recv loop / events loop split, handling every BackendToSidecar variant
    (SpawnProcess, KillProcess, ListProcesses, InputEvent, ResizeWindow, Start/StopWindowCapture —
    the capture ones are no-ops for an auto-streaming sidecar, mirror the X11 comment).
  - encode_for_wire via x11-web-pixel-codec, and the same OTel span/attribute plumbing
    (crates/sidecar-wayland/src/telemetry.rs modelled on crates/sidecar/src/telemetry.rs).
  - Process-to-client attribution: the X11 sidecar walks /proc PPid from the connecting peer's pid.
    Wayland gives you the client's credentials too — implement the equivalent so spawned processes are
    attributed to their surfaces, or record clearly why you couldn't and what you did instead.
  - Dropping the dbus session is fine if nothing needs it; say so.

Gate: \`bash tools/wayland-build.sh build --bin x11-web-sidecar-wayland\` green (background + poll),
host \`cargo check --workspace\` green, commit.`,
	{ label: 'impl:sidecar-binary', phase: 'Sidecar', effort: 'high', schema: STAGE_SCHEMA },
)

const h4 = sidecarStage ? `\nHANDOFF FROM Sidecar:\n${sidecarStage.handoff}\nDeviations: ${sidecarStage.deviations.join('; ') || 'none'}\n` : '\n(Sidecar stage returned nothing — inspect the tree yourself.)\n'

phase('Package')

const pkg = await agent(
	`${CONTEXT}
${SCOPE}
${PLAN_TEXT}
${IMPL_RULES}
${h1}${h2}${h3}${h4}

YOUR STAGE — Package: make it runnable and testable end to end.

Deliver:
  1. \`Dockerfile.sidecar-wayland\`, modelled on Dockerfile.sidecar (read it fully first):
     builder stage that copies only the crates this binary needs and stubs the other workspace
     members; runtime stage on debian:bookworm-slim with libxkbcommon + xkb-data, the chosen Wayland
     test client(s) installed, a writable XDG_RUNTIME_DIR (mode 0700, correct ownership), and an
     entrypoint mirroring the X11 sidecar's. It must NOT need /dev/dri, a GPU or a seat.
     Verify by actually building it: \`docker build -f Dockerfile.sidecar-wayland -t x11web-sidecar-wayland .\`
     (background + poll — this takes many minutes).
  2. A \`sidecar-wayland\` service in compose.yml alongside \`sidecar\`, same fingerprint mount and
     host.docker.internal wiring, SIDECAR_NAME=dev-wayland-sidecar. Put it behind a profile if that
     keeps the default \`docker compose up sidecar\` behaviour byte-identical — do not perturb the
     existing dev loop.
  3. A smoke check that does not need the browser: run the image, confirm the compositor comes up,
     spawn the chosen test client inside the container against it, and confirm from the logs that a
     surface was mapped and pixels were produced. Script this as
     \`e2e/scripts/wayland-smoke.sh\` so later phases can re-run it cheaply.
  4. The e2e spec: \`e2e/tests/wayland/wayland-sidecar.spec.ts\`, using the least intrusive extension
     of e2e/tests/fixtures.ts that the recon identified. It must:
       - start the wayland sidecar image against the worker's backend,
       - wait for the sidecar to appear in the frontend,
       - spawn the test client,
       - assert a window frame appears with a sane title,
       - assert the canvas for that window is NOT blank (reuse the existing non-blank-pixel helper),
       - click inside the window and type a key, and assert the app reacts in a way that is
         deterministic for the client you chose (e.g. a visible pixel change) — if no deterministic
         visual reaction exists for that client, assert instead on sidecar-side evidence that the
         events were delivered to the client, and say so in a comment.
     Follow the existing specs' idioms and run \`pnpm biome check --write\` on the new TS.
  5. Update Readme.md's component list + project structure, and mprocs.yaml if a wayland pane belongs
     there.

Gate: the docker image builds; \`e2e/scripts/wayland-smoke.sh\` passes; \`pnpm turbo run typecheck\` and
\`pnpm biome check .\` are clean. Commit.`,
	{ label: 'impl:package', phase: 'Package', effort: 'high', schema: STAGE_SCHEMA },
)

const IMPL_SUMMARY = [scaffold, compositor, input, sidecarStage, pkg]
	.map((s, i) => {
		const names = ['Scaffold', 'Compositor', 'Input', 'Sidecar', 'Package']
		if (!s) return `${names[i]}: NO RESULT (agent died or was skipped) — treat as incomplete.`
		return `${names[i]}: done=${s.done}\n  verification: ${s.verification}\n  deviations: ${s.deviations.join('; ') || 'none'}`
	})
	.join('\n')

log('Implementation stages complete; entering verification')

// ---------------------------------------------------------------------------
// Phase 8 — Integrate: loop until the whole thing builds green.
// ---------------------------------------------------------------------------

phase('Integrate')

let integrated = null
let integrateReport = 'not run'
for (let attempt = 1; attempt <= 3; attempt++) {
	const r = await agent(
		`${CONTEXT}
${SCOPE}
${IMPL_RULES}

STATE OF THE IMPLEMENTATION:
${IMPL_SUMMARY}

${attempt > 1 ? `PREVIOUS INTEGRATION ATTEMPT FAILED. Its report:\n${integrateReport}\n\nFix the causes, do not paper over them.\n` : ''}

YOUR JOB — Integration gate, attempt ${attempt} of 3. Make ALL of these actually pass, fixing whatever
is broken (you may edit any file in the repo):
  1. \`cargo check --workspace\` on the macOS host.
  2. \`cargo test --workspace\` on the macOS host (existing tests must not regress).
  3. \`bash tools/wayland-build.sh test --workspace\` (Linux; background + poll).
  4. \`bash tools/wayland-build.sh clippy --workspace -- -D warnings\` if clippy is available in the
     builder image; if it isn't, install it there and then run it. Fix the warnings in NEW code;
     for pre-existing warnings in old code, leave them and say so.
  5. \`docker build -f Dockerfile.sidecar-wayland -t x11web-sidecar-wayland .\` (background + poll).
  6. \`docker build -f Dockerfile.sidecar -t x11web-sidecar-check .\` — proof the X11 sidecar image
     still builds after the workspace changes.
  7. \`pnpm turbo run typecheck\` and \`pnpm biome check .\`.
  8. \`bash e2e/scripts/wayland-smoke.sh\`.
Report each command and its real outcome verbatim in \`report\`. Set passed=true ONLY if every one is
green. Commit any fixes.`,
		{ label: `integrate:attempt-${attempt}`, phase: 'Integrate', effort: 'high', schema: GATE_SCHEMA },
	)
	if (!r) {
		integrateReport = 'agent returned nothing'
		continue
	}
	integrateReport = r.report
	integrated = r
	if (r.passed) {
		log(`Integration green on attempt ${attempt}`)
		break
	}
	log(`Integration attempt ${attempt} failed: ${r.remainingWork.slice(0, 3).join('; ')}`)
}

// ---------------------------------------------------------------------------
// Phase 9 — E2E: the actual proof it works, plus a no-regression run.
// ---------------------------------------------------------------------------

phase('E2E')

let e2e = null
let e2eReport = 'not run'
for (let attempt = 1; attempt <= 3; attempt++) {
	const r = await agent(
		`${CONTEXT}
${IMPL_RULES}

INTEGRATION REPORT:
${integrateReport}

${attempt > 1 ? `PREVIOUS E2E ATTEMPT FAILED. Its report:\n${e2eReport}\n\nDiagnose the real cause — read the sidecar container logs (\`docker logs\`), the compositor's tracing output, and the Playwright trace/screenshot artifacts under e2e/test-results/. Fix the product code when the product is wrong; only relax the test when the assertion itself was wrong, and justify it.\n` : ''}

YOUR JOB — E2E gate, attempt ${attempt} of 3.
  1. Ensure the frontend builds and Playwright's chromium is installed
     (\`cd e2e && pnpm exec playwright install chromium\` if needed).
  2. Run the new spec: \`pnpm --filter x11-web-e2e exec playwright test tests/wayland/\`.
     It must pass for real — a window rendering real pixels from a real Wayland client, and input
     reaching that client. If it fails, fix the cause and re-run. Do not delete or weaken assertions
     to make it pass.
  3. Then prove no regression: run the existing core suite —
     \`pnpm --filter x11-web-e2e exec playwright test tests/x11-web.spec.ts tests/core\`.
     Some tests on this repo are known-flaky; if something fails, re-run just that test to see whether
     it is flaky or genuinely broken by our change, and say which in the report. Anything our change
     broke, fix.
  4. Clean up leaked docker containers/networks afterwards.
Playwright runs exceed the 10-minute Bash timeout — use run_in_background:true and poll.
Report every command and its real outcome. passed=true only if the new spec passes AND nothing we
changed broke the existing suite. Commit any fixes.`,
		{ label: `e2e:attempt-${attempt}`, phase: 'E2E', effort: 'high', schema: GATE_SCHEMA },
	)
	if (!r) {
		e2eReport = 'agent returned nothing'
		continue
	}
	e2eReport = r.report
	e2e = r
	if (r.passed) {
		log(`E2E green on attempt ${attempt}`)
		break
	}
	log(`E2E attempt ${attempt} failed: ${r.remainingWork.slice(0, 3).join('; ')}`)
}

// ---------------------------------------------------------------------------
// Phase 10 — Review (parallel lenses over the finished diff)
// ---------------------------------------------------------------------------

phase('Review')

const LENSES = [
	{
		key: 'correctness',
		prompt: `Review the diff for CORRECTNESS bugs in the Wayland compositor and sidecar: protocol
misuse (serials, focus enter/leave pairing, buffer release timing, frame callbacks), the shm pixel
format/stride/swizzle path, damage rectangles, window-id lifetime, the calloop<->tokio boundary
(blocking a runtime thread, dropped senders, unbounded growth), reconnect handling, and panics that
would take the sidecar down. Prefer few, high-confidence findings you can trace to a concrete failure.`,
	},
	{
		key: 'regression',
		prompt: `Review the diff for REGRESSION RISK to everything that already worked: the X11 sidecar,
crates/x11-server, crates/wire's schema compatibility (does adding \`wayland @3\` break older peers or
any exhaustive match?), crates/backend routing, compose.yml's existing dev loop, Dockerfile.sidecar,
the existing e2e fixtures and specs, turbo/biome gates. Anything that changes behaviour for an existing
component is a finding.`,
	},
	{
		key: 'provenance',
		prompt: `Review the diff for LICENSE PROVENANCE and hygiene: every file derived from waylandcraft
must carry the GPLv3 attribution header with the upstream commit, crates/wayland-server/NOTICE must
exist and be accurate, no upstream code may have leaked into unrelated crates, and the reference
checkout tools/waylandcraft-upstream must be gitignored and NOT committed (check \`git log --stat\`).
Also check: no secrets, no absolute paths from this machine baked into committed files, no stray
scratch files, no giant binaries added.`,
	},
]

const reviews = await parallel(
	LENSES.map((l) => () =>
		agent(
			`${CONTEXT}

Review the work on branch \`wayland-sidecar\`. Get the diff with
\`git diff main...HEAD\` and \`git log --oneline main..HEAD\`; read the new files in full.

${l.prompt}

Report only findings you would defend in review. Include the concrete fix for each. If the code is
sound on your lens, return an empty findings array — do not manufacture findings.`,
			{ label: `review:${l.key}`, phase: 'Review', effort: 'high', schema: FINDINGS_SCHEMA },
		),
	),
)

const findings = reviews.filter(Boolean).flatMap((r) => r.findings)
const blockers = findings.filter((f) => f.severity !== 'minor')
log(`Review: ${findings.length} findings (${blockers.length} blocker/major)`)

// ---------------------------------------------------------------------------
// Phase 11 — Polish: apply what matters, re-verify, land.
// ---------------------------------------------------------------------------

phase('Polish')

const polish = await agent(
	`${CONTEXT}
${IMPL_RULES}

REVIEW FINDINGS (${findings.length} total, ${blockers.length} blocker/major):
${findings.length ? findings.map((f) => `- [${f.severity}] ${f.file}${f.line ? `:${f.line}` : ''} — ${f.summary}\n    fix: ${f.fix}`).join('\n') : '(none)'}

INTEGRATION REPORT:
${integrateReport}

E2E REPORT:
${e2eReport}

YOUR JOB — final pass:
  1. Fix every blocker and major finding. For minors, fix the cheap ones and note the rest.
     If you believe a finding is wrong, say why in the report instead of changing code.
  2. Re-run the gates that your changes could affect: host \`cargo check --workspace\`,
     \`bash tools/wayland-build.sh test --workspace\`, the docker build, and
     \`pnpm --filter x11-web-e2e exec playwright test tests/wayland/\` (background + poll).
  3. Make sure the repo is left clean and honest:
     - Readme.md documents the Wayland sidecar (component description, how to run it, how to test it),
     - crates/wayland-server has a module-level doc comment explaining the architecture and the
       waylandcraft provenance,
     - no uncommitted changes, no leaked docker containers/networks,
     - \`git log --oneline main..HEAD\` reads as a sensible series.
  4. Squash nothing; just make sure the final commit is green.

In \`report\`, give an honest final account: what works end to end (with the evidence), what is stubbed
or out of scope, and what a human should look at first. \`remainingWork\` is the follow-up list.
passed=true only if the wayland e2e spec genuinely passes.`,
	{ label: 'polish:final', phase: 'Polish', effort: 'high', schema: GATE_SCHEMA },
)

return {
	plan: { overview: plan.overview, fileCount: plan.files.length, decisions: plan.openDecisions },
	stages: IMPL_SUMMARY,
	integration: { passed: integrated?.passed ?? false, report: integrateReport },
	e2e: { passed: e2e?.passed ?? false, report: e2eReport },
	review: { total: findings.length, blockersAndMajors: blockers.length, findings },
	final: polish ?? { passed: false, report: 'polish agent returned nothing', remainingWork: [] },
}
