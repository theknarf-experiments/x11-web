# X11 Server Full Spec Compliance Plan

## Current State (Phase 17 Complete)
- ~70K LOC Rust X11 server implementation
- 684 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 26 extensions fully implemented with modular registry
- E2e tests: 188 passing, 1 failing (glxinfo), 7 conditional skips
- All X11 apps work: xeyes, xterm, xclock, xdpyinfo, xdotool, SDL2, Firefox, GIMP, etc.
- WebRTC transport + PulseAudio audio streaming complete

## Completed Phases

### Phase 9-12: Event Constants, XID Recycling, Setup Reply, App Compatibility ✓

### Phase 13: Critical Protocol Fixes ✓

### Phase 14: Modular Extension Architecture ✓

### Phase 15: Performance & Robustness ✓

### Phase 16: Advanced Features ✓

### Phase 17: WebRTC Transport & Audio ✓

## Remaining Phases

### Phase 18: GLX Compliance & Protocol Hardening

#### 18a: Fix GLX/glxinfo
- [ ] Fix GetVisualConfigs visual class value (1=GrayScale → 4=TrueColor)
- [ ] Add `libgl1-mesa-dri` to Dockerfile.sidecar for swrast DRI driver
- [ ] Verify glxinfo runs successfully and reports GLX version
- [ ] E2e: glxinfo test passes
- [ ] `git fetch` and rebase against origin/main, then force push

#### 18b: GrabServer Protocol Fix
- [ ] Investigate and fix GrabServer serialization (currently hardcoded test.skip)
- [ ] Unskip the GrabServer serialization e2e test
- [ ] Verify multi-client grab ordering
- [ ] `git fetch` and rebase against origin/main, then force push

#### 18c: Broader GLX Testing
- [ ] E2e: glxgears renders frames without crash
- [ ] E2e: mesa-utils GL queries succeed
- [ ] Verify OSMesa indirect rendering path with glxinfo -i
- [ ] `git fetch` and rebase against origin/main, then force push

#### 18d: Test & Commit
- [ ] All existing e2e tests pass (0 failures)
- [ ] 684+ unit tests passing
- [ ] git commit
- [ ] `git fetch` and rebase against origin/main, then force push

#### 18e: Fix xterm font rendering

- [ ] `xterm-canvas-darwin.png` is correct while `xterm-canvas-linux.png` clearly has fucked up font rendering. Delete the linux png and copy and rename the darwin one. Then use the correct snapshot to guide you to fix it

#### 18e: Fix firefox

- [ ] Firefox either doesn't start or crashes under startup, ensure that it works and have e2e tests with snapshots that cover it

### Phase 19: Deep XTS Conformance & App Stress Testing

#### 19a: XTS Test Suite Hardening
- [ ] Run full XTS Xproto suite, fix any failures
- [ ] Run full XTS Xlib suite, fix any failures
- [ ] Add more XTS categories to e2e test matrix
- [ ] `git fetch` and rebase against origin/main, then force push

#### 19b: Heavy Application Testing
- [ ] E2e: Chromium/Chrome launch and basic navigation
- [ ] E2e: Inkscape SVG editing (if installable in container)
- [ ] E2e: Java AWT/Swing application (xterm + java)
- [ ] E2e: Multi-monitor simulation via XINERAMA/RANDR
- [ ] `git fetch` and rebase against origin/main, then force push

#### 19c: Protocol Edge Cases
- [ ] E2e: Rapid reconnect stress test (100+ sequential connections)
- [ ] E2e: Large property data round-trip (>256KB)
- [ ] E2e: Concurrent clipboard operations
- [ ] E2e: Deep window hierarchy (100+ nested)
- [ ] `git fetch` and rebase against origin/main, then force push
