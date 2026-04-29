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

### Various `rendercheck` extended-suite tests — flaky timeouts

Tests under `advanced-compliance.spec.ts:3570` (`rendercheck composite
operations pass`) and `x11-web.spec.ts:2989` repeatedly hit Playwright's
60-second timeout when running `rendercheck -t composite` over the full
PictOp matrix. Tests pass when given enough time (we've seen >60s runs
succeed). Pre-existing — the rendercheck binary is just slow over our
software pipeline.
