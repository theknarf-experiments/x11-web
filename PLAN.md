# X11 Server Full Spec Compliance Plan

## Current Status

- 120/120 core X11 opcodes implemented
- 22 extensions (SHAPE, MIT-SHM, XFIXES, RANDR, SYNC, XInput2, XKB, XTEST, RENDER, Composite, DAMAGE, Present, GLX, XVideo, DBE, DPMS, ScreenSaver, Record, Xinerama, etc.)
- GLX software rendering works via DRISW/llvmpipe (glxinfo shows OpenGL 4.5)
- 70+ deep-conformance tests passing

## Known Issues

### Firefox/GTK3 GL apps segfault in DRISW
Firefox and GTK3 apps that use OpenGL crash in Mesa's DRISW software renderer.
glxinfo works (simple context creation + queries), but Firefox's GPU process
crashes during more complex GL initialization. Likely cause: our GLX single
opcode dispatch table doesn't match Mesa's glxproto.h spec (opcodes 111-150
are mismatched), or we mishandle specific GLX render commands that Firefox's
compositor sends during startup.

### GLX single opcode table wrong
The dispatch table in context.rs maps opcodes 111-127 to wrong handlers
(e.g., opcode 112 dispatches GetFloatv instead of GetBooleanv). Since DRISW
renders locally these aren't hit by glxinfo, but Firefox's multi-process
architecture or pure indirect rendering would be affected.

## Remaining Work

### Fix GLX for Firefox/GTK3 (highest priority)
- [ ] Correct GLX single opcode dispatch table to match Mesa glxproto.h
- [ ] Fix single_query.rs reply field positions (size at [12..16] per xGLXSingleReply spec)
- [ ] Investigate Firefox GPU process crash — capture what X11/GLX messages it sends
- [ ] Get Firefox ESR to create a window without segfault
- [ ] Get gtk3-demo to run without segfault
- [ ] Un-skip Firefox and GTK3 tests

### Broader GLX Testing
- [ ] E2e: glxgears renders frames without crash
- [ ] E2e: mesa-utils GL queries succeed
- [ ] Verify OSMesa indirect rendering path with glxinfo -i

### XTS Test Suite Hardening
- [ ] Run full XTS Xproto suite, fix any failures
- [ ] Run full XTS Xlib suite, fix any failures

### Heavy Application Testing
- [ ] E2e: Chromium/Chrome launch and basic navigation
- [ ] E2e: Java AWT/Swing application
- [ ] E2e: Multi-monitor simulation via RANDR

### Protocol Edge Cases
- [ ] Rapid reconnect stress test (100+ sequential connections)
- [ ] Large property data round-trip (>256KB)
- [ ] Concurrent clipboard operations
- [ ] Deep window hierarchy (100+ nested)
