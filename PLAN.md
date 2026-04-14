# X11 Server Full Spec Compliance Plan

## Current State (Phase 14 Complete)
- ~70K LOC Rust X11 server implementation
- 684 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 26 extensions fully implemented with modular registry
- E2e tests passing: protocol-compliance (80/80), app-compatibility (19/19), advanced-compliance (86/86)
- All X11 apps work: xeyes, xterm, xclock, xdpyinfo, xdotool, SDL2, etc.

## Completed Phases

### Phase 9-12: Event Constants, XID Recycling, Setup Reply, App Compatibility ✓

### Phase 13: Critical Protocol Fixes ✓
- Fixed X11 event sequence number patching
- GrabServer setup-phase blocking
- GLX GetFBConfigs numAttribs fix
- SDL2 application compatibility verified

### Phase 14: Modular Extension Architecture ✓
- [x] Created `ExtensionRegistry` as single source of truth for all extension metadata
- [x] Created `ExtensionId` enum replacing all magic opcode constants
- [x] Refactored `dispatch.rs` to use registry lookup instead of hardcoded match
- [x] Refactored `query.rs` QueryExtension/ListExtensions to use registry
- [x] Added Cargo feature flags for extension groups:
  - `ext-core`: SHAPE, MIT-SHM, BIG-REQUESTS, SYNC, GE, XFIXES, RANDR, XC-MISC, X-Resource
  - `ext-input`: XInputExtension, XTEST, XKEYBOARD
  - `ext-render`: RENDER, Composite, DAMAGE, Present
  - `ext-glx`: GLX, DRI3
  - `ext-media`: XVideo, DOUBLE-BUFFER
  - `ext-compat`: DPMS, MIT-SCREEN-SAVER, VidMode, RECORD, SECURITY, XINERAMA
  - `all-extensions` (default): enables everything
- [x] Runtime extension toggling via `ExtensionRegistry::set_enabled()`
- [x] Multi-arch Dockerfile support (arm64 + amd64)
- [x] 684 unit tests passing, 178 e2e tests passing

## Remaining Phases

### Phase 15: Performance & Robustness
- [x] Add per-client resource limits (windows, pixmaps, GCs, colormaps, cursors)
- [x] Bound pending_events queue per client
- [x] Fix mutex poison handling in menus.rs (use unwrap_or_else with logging)
- [x] Fix polygon rendering safety (.unwrap on .min/.max)
- [x] Use VecDeque for frozen events in grab.rs instead of Vec with remove(0)
- [x] Add motion_history size limit via configurable ResourceLimits
- [x] Add error logging for silent failures (compression, etc.)
- [x] E2e test: resource limits enforcement (5 robustness tests)
- [ ] Git fetch, rebase against origin/main, fix any merge conflicts and push up

### Phase 16: Advanced Features
- [ ] XIM completion for CJK input
- [ ] XVideo actual video decode
- [ ] GLX direct rendering improvements
- [ ] Ensure that there are e2e tests covering testing with Firefox (it should start up, be possible to navigate to wikipedia, scroll should work, navigate to youtube, playing a youtube video should work)
- [ ] Git fetch, rebase against origin/main, fix any merge conflicts and push up

### Phase 17: WebRTC

- [ ] Replace WebSocket's with a custom binary protocol over WebRTC
- [ ] Add support for audio streaming
- [ ] e2e tests with VLC streaming a test video with audio
- [ ] Add support for microphone streaming from the browser
- [ ] e2e test with Audacity testing recording
- [ ] git commit and push
