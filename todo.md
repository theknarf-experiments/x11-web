# todo

Things we've deferred. Each entry should explain *why* it's deferred so future
us doesn't burn another hour rediscovering the reason.

## Skipped e2e suites

### `e2e/tests/firefox-compliance.spec.ts` — all 6 tests `test.skip`-ed

Symptom: `spawnApp(... "firefox-esr" ...)` never sees a new
`[data-testid="window-frame"]` appear. The `expect(windowFrames)
.toHaveCount(countBefore + 1)` assertion times out at the 120-second mark
for every test in the suite.

Pre-existing on `origin/main` (verified by checking out `0371d07` before
the rebase and reproducing the same failure). The fault is somewhere
between `firefox-esr` startup inside the sidecar container and the
frontend learning about the new top-level — could be:

- Firefox crashing before MapWindow (no `WM_CLASS` lands).
- Our `_NET_WM_*` property handling not flushing the window-list update
  to the frontend.
- A profile / first-run flow that needs `--no-remote --new-instance`
  to be combined with something we don't pass yet.

Re-enable by removing the `test.skip(` wrappers and getting at least
"firefox: startup and initial rendering" green again.

### `tests/x11-web.spec.ts:1157` — `xkbcomp dumps a parseable XKB keymap`

`xkbcomp -xkb :99 -` exits with status 1 and produces 6 lines of error
output instead of a keymap dump. Pre-existing on origin/main. Likely
related to our XKB SetMap / GetMap wire format not surviving xkbcomp's
strict validation. Not skipped yet — left unmarked because the rest
of `x11-web.spec.ts` still needs sweeping.

### `tests/advanced-compliance.spec.ts:2888` — `XVideo QueryAdaptors and ListImageFormats return formats`

`xvinfo` aborts with `[xcb] Extra reply data still left in queue`. Our
`XvListImageFormats` reply has a length-field mismatch somewhere — xcb
believes there is leftover payload after parsing. Pre-existing on
`origin/main`.

### `tests/phase7-compliance.spec.ts` — 4 tests skipped

- **`GetControls reports correct repeat delay and interval`**: Asserts on
  `xkbcomp :99 -` output. xkbcomp currently exits 1; same fix as the
  `xkbcomp dumps` failure above will unblock this.
- **`xterm renders CJK characters via xdotool`**: After typing "你好世界"
  the canvas hash is unchanged, meaning no CJK glyphs land on screen.
  Need to confirm whether xterm-with-`-fn fixed` actually requests CJK
  glyphs and whether our font path serves them. The runtime image has
  `fonts-noto-cjk`; we may not be advertising the right XLFDs for
  xterm to pick them up.
- **`GTK text entry (zenity --entry) launches`**: Same spawnApp /
  window-frame timeout root cause as the firefox suite.
- **`window stacking order via xdotool windowraise`**: After spawning
  xeyes + xclock, only the first window appears in the frontend's
  window list. Same spawnApp issue scoped to a second concurrent
  spawn.
- **`window resize via xdotool windowsize`**: `xdotool windowsize`
  sends ConfigureWindow against xeyes, but the canvas size doesn't
  change. matchbox-WM should be redirecting via
  SubstructureRedirectMask and re-issuing the configure on the inner
  window — somewhere in that chain the resize is dropped. Investigate
  whether our ConfigureRequest delivery to the WM is correct, and
  whether the WM's response actually maps to a canvas-size update on
  the frontend.

### `tests/app-compatibility.spec.ts` — 5 tests skipped

- **`clipboard data persists after source app exits`**: We need an
  in-server clipboard manager that takes over CLIPBOARD ownership when
  the original owner disconnects. Right now the data is lost the moment
  xclip exits.
- **`GTK3 app can query XSETTINGS for theme`**: We don't advertise an
  `_XSETTINGS_S0` selection owner with a settings property, so GTK3
  apps fall back to "no theme" and gtk3-demo flakes around startup.
- **`editres starts without crash`**: editres expects extensive Xt /
  resource introspection; it dies during startup in our environment
  before the test's pkill detects it.
- **`xterm with Athena scrollbar renders`**: `xterm -sb -rightbar`
  doesn't show up in `xwininfo -root -tree`. Our server probably isn't
  tracking the scrollbar child window correctly under the Athena
  toolkit's wider window tree.
- **`xdotool sends keystrokes to a specific window`**: After
  `xdotool windowfocus + xdotool type`, the typed text never lands in
  the focused xterm. Likely an XTEST / SetInputFocus interaction bug.

### `tests/deep-conformance.spec.ts:213` — `x11perf window operations` skipped

`x11perf -create -map -unmap -destroy -resize -move` runs past
Playwright's 5-minute test timeout (the other x11perf benchmarks in
the same suite finish well under the limit). One of those sub-tests
is hanging. Likely candidates: our `MapSubwindows` /
`UnmapSubwindows` traversal, or the resize-then-immediately-destroy
pattern racing against expose-event delivery.

### Various `rendercheck` extended-suite tests — flaky timeouts

Tests under `advanced-compliance.spec.ts:3570` (`rendercheck composite
operations pass`) and `x11-web.spec.ts:2989` repeatedly hit Playwright's
60-second timeout when running `rendercheck -t composite` over the full
PictOp matrix. Tests pass when given enough time (we've seen >60s runs
succeed). Pre-existing — the rendercheck binary is just slow over our
software pipeline.
