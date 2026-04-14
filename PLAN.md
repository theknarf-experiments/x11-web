# X11 Server Full Spec Compliance Plan

## Current State (Phase 13 Complete)
- ~70K LOC Rust X11 server implementation
- 677 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 22+ extensions fully implemented
- E2e tests passing: protocol-compliance (80/80), app-compatibility (19/19), advanced-compliance (86/86)
- All X11 apps work: xeyes, xterm, xclock, xdpyinfo, xdotool, SDL2, etc.

## Completed Phases

### Phase 9-12: Event Constants, XID Recycling, Setup Reply, App Compatibility ✓
- All event constants, XID recycling for 19 resource types
- Setup reply validation, XSETTINGS format tests
- Fixed _NET_WM_STATE fullscreen/maximize to actually resize windows
- Added _NET_WM_STATE_FOCUSED, saved_geometry for state transitions
- Comprehensive app-compatibility and xts-compliance e2e tests
- Sidecar Dockerfile working with XTS, dbusmenu, and full app suite

### Phase 13: Critical Protocol Fixes ✓
- [x] Fixed X11 event sequence number patching (root cause of all xcb aborts)
  - Events must have sequence of "last request processed" when delivered
  - Cross-connection events need sequence patching for receiving client
  - MANAGER events during setup need correct byte order
  - Added patch_event_sequences() at all pending_events drain sites
- [x] Added GrabServer setup-phase blocking (new connections blocked)
- [x] Fixed e2e test python3-xlib error handler patterns
- [x] Fixed python3-xlib visual class lookup
- [x] Increased playwright timeout to 300s for Docker image builds
- [x] Fixed cargo check / cargo fmt
- [x] Fixed GLX GetFBConfigs numAttribs (was doubling attribute count)
- [x] Tested Firefox (headless screenshot), emacs (batch), LibreOffice (headless), zenity
- [x] Fixed GrabServer cross-connection unblocking
  - Switched from tokio::sync::Mutex to std::sync::Mutex (no try_lock spin)
  - Proper tokio::sync::Notify usage for immediate wakeup
- [x] Fixed remaining advanced-compliance failures (4→0)
  - AllocNamedColor test attribute fix
  - ListFontsWithInfo python3-xlib crash workaround
  - Selection conversion using xclip/xsel for reliability
  - Clipboard round-trip comparison fix
- [x] Verified pnpm dev starts cleanly
- [x] SDL2 application compatibility verified with e2e test

## Remaining Phases

### Phase 14: Performance & Robustness
- [ ] Optimize hot paths (event delivery, drawing)
- [ ] Add proper resource limits
- [ ] Improve error messages

### Phase 15: Advanced Features
- [ ] XIM completion for CJK input
- [ ] XVideo actual video decode
- [ ] GLX direct rendering improvements
