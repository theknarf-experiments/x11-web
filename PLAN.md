# X11 Server — Remaining Work Plan

## Current State (Phase 13 Complete)
- ~70K LOC Rust X11 server implementation
- 677 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 22+ extensions fully implemented
- E2e tests passing: protocol-compliance (80/80), app-compatibility (19/19), advanced-compliance (86/86)
- All X11 apps work: xeyes, xterm, xclock, xdpyinfo, xdotool, SDL2, etc.

## Phase 14: Modular Extension Architecture

Make extensions toggleable at runtime via a central registry instead of hardcoded dispatch.

- [ ] Create `ExtensionRegistry` struct in `crates/sidecar/src/xserver/extensions/registry.rs`
  - Maps extension name → `ExtensionInfo { major_opcode, first_event, first_error, enabled, handler }`
  - Default: all extensions enabled
  - Runtime toggle: `registry.set_enabled("RENDER", false)`
- [ ] Create `ExtensionId` enum with all 26 extensions (replaces magic opcode constants)
- [ ] Refactor `dispatch.rs` to use registry lookup instead of hardcoded match
- [ ] Refactor `query.rs` QueryExtension/ListExtensions to use registry
- [ ] Add Cargo feature flags for extension groups:
  - `ext-render` (RENDER, Composite, DAMAGE, PRESENT)
  - `ext-input` (XINPUT, XTEST, XKB)
  - `ext-glx` (GLX, DRI3)
  - `ext-media` (XVideo, DBE)
  - `ext-compat` (XINERAMA, VidMode, DPMS, ScreenSaver, RECORD, SECURITY)
  - `ext-core` (SHAPE, SHM, BIG-REQUESTS, SYNC, GE, XFIXES, RANDR, XC-MISC, XResource)
  - `all-extensions` (default, enables everything)
- [ ] Add e2e test: verify disabled extension returns `present=0` from QueryExtension
- [ ] Verify all existing e2e tests still pass

## Phase 15: Multi-Architecture Support

- [ ] Update Dockerfile.sidecar with multi-arch build support (arm64 + amd64)
- [ ] Ensure no architecture-specific assumptions in byte-order handling
- [ ] Add CI note for cross-compilation testing

## Phase 16: Performance & Robustness
- [ ] Optimize hot paths (event delivery, drawing)
- [ ] Add proper resource limits
- [ ] Replace panic!() in production code with error returns
