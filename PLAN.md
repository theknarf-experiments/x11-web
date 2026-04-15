# X11 Server Full Spec Compliance Plan

## Current State (Phase 18 In Progress)
- ~70K LOC Rust X11 server implementation
- 685 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 26 extensions fully implemented with modular registry
- WebRTC transport + PulseAudio audio streaming complete

## Completed Phases

### Phase 9-12: Event Constants, XID Recycling, Setup Reply, App Compatibility ✓

### Phase 13: Critical Protocol Fixes ✓

### Phase 14: Modular Extension Architecture ✓

### Phase 15: Performance & Robustness ✓

### Phase 16: Advanced Features ✓

### Phase 17: WebRTC Transport & Audio ✓

### Phase 18a: Fix GLX/glxinfo ✓
- [x] Fix GetVisualConfigs visual class value (1=GrayScale → 4=TrueColor)
- [x] Add `libgl1-mesa-dri` to Dockerfile.sidecar for swrast DRI driver
- [x] Fix GLX QueryExtensionsString/QueryServerString reply layout

### Phase 18b: GrabServer Protocol Fix ✓
- [x] Fix GrabServer wait timing (moved after handshake, 5ms polling)

### Phase 18e-1: Fix xterm font rendering ✓
- [x] Fix PCF font bitmap row padding (glyph_pad was computed but ignored)
- [x] Regenerated Linux snapshots with correct font rendering
- [x] Unit test for PCF 4-byte padding repack (685 total unit tests)

### Phase 18e-2: Fix COMPOSITE redirect ✓
- [x] Forward redirected windows to frontend (we ARE the final display)
- [x] Firefox/GTK apps using CompositeRedirectWindow now render correctly

### Phase 18e-3: Container environment cleanup ✓
- [x] Remove DRI3 extension (no GPU/DRM in Docker containers)

### Phase 18f: Fix GLX reply format ✓
- [x] Root cause: GLX GetString handler had `.max(1)` on reply_length, adding 4
      spurious bytes for empty strings. Mesa's indirect GLX calls _XReply(extra=0)
      leaving unconsumed data that crashes the next _XReply call.
- [x] Fix: Remove `.max(1)` — empty strings correctly return reply_length=0
- [x] Fix IsDirect reply field offset (is_direct at byte 1, not byte 8)
- [x] glxinfo and glxgears now work without crash
- [x] Remove app-specific env vars (MOZ_*, MOZ_USE_XINPUT2, MOZ_X11_EGL)
- [ ] Firefox non-headless still segfaults in Mesa's indirect GLX path
      (unhandled render opcode 32768 or FBConfig matching issue)

## Remaining Phases

### Phase 18g: Fix Firefox non-headless GLX segfault
- [ ] Debug Mesa indirect GLX segfault (unrelated to xcb reply data bug)
- [ ] Handle GLX render opcode 32768 (currently returns BAD_REQUEST error)
- [ ] Investigate FBConfig matching so swrast driver loads successfully

### Phase 18c: Broader GLX Testing
- [ ] E2e: glxgears renders frames without crash
- [ ] E2e: mesa-utils GL queries succeed
- [ ] Verify OSMesa indirect rendering path with glxinfo -i

### Phase 18d: Test & Commit
- [ ] All existing e2e tests pass (0 failures)
- [ ] 685+ unit tests passing
- [ ] git commit

### Phase 19: Deep XTS Conformance & App Stress Testing

#### 19a: XTS Test Suite Hardening
- [ ] Run full XTS Xproto suite, fix any failures
- [ ] Run full XTS Xlib suite, fix any failures
- [ ] Add more XTS categories to e2e test matrix

#### 19b: Heavy Application Testing
- [ ] E2e: Chromium/Chrome launch and basic navigation
- [ ] E2e: Inkscape SVG editing (if installable in container)
- [ ] E2e: Java AWT/Swing application (xterm + java)
- [ ] E2e: Multi-monitor simulation via XINERAMA/RANDR

#### 19c: Protocol Edge Cases
- [ ] E2e: Rapid reconnect stress test (100+ sequential connections)
- [ ] E2e: Large property data round-trip (>256KB)
- [ ] E2e: Concurrent clipboard operations
- [ ] E2e: Deep window hierarchy (100+ nested)
