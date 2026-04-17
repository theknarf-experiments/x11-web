# X11 Server Full Spec Compliance Plan

## Current Status

- 120/120 core X11 opcodes implemented
- 22 extensions implemented
- GLX software rendering works via DRISW/llvmpipe (glxinfo shows OpenGL 4.5)
- GLX opcode dispatch table corrected to match Mesa's glxproto.h
- GLX reply handlers refactored to use GlxSingleReply struct (no more magic offsets)
- GLX opcode constants module generated from Mesa headers (no more magic numbers in dispatch)
- 71 deep-conformance tests passing, 2 skipped (Firefox/GTK3 GL crash)

## Known Issues

### glxinfo XCB assertion (intermittent)
Docker build caching makes it hard to verify GLX changes. After pruning build
cache and rebuilding, glxinfo sometimes crashes with `xcb_xlib_extra_reply_data_left`.
This may be a stale binary issue or a subtle reply padding bug in one of the GLX
query handlers. The Python protocol test passes all GLX operations correctly.
Need to isolate which specific reply leaves unconsumed data.

### Firefox/GTK3 GL apps segfault in DRISW
Firefox headless works (produces screenshots). Non-headless Firefox and GTK3 GL
apps crash in Mesa's DRISW software renderer before creating any windows. This is
a Mesa/DRISW initialization crash, not a protocol error — all GLX protocol tests
pass. Likely related to SHM buffer setup or DRI screen creation in the container.

### GLX render opcode table needs audit
The render dispatch tables (render_draw.rs, render_state.rs, etc.) have ~300
magic numbers, some of which may be wrong (e.g., glEnable mapped to opcode 69
instead of X_GLrop_Enable=139). Only affects pure indirect rendering (not DRISW).

## Remaining Work

### Fix glxinfo XCB assertion
- [ ] Add protocol-level tracing to identify which reply leaves extra data
- [ ] Compare wire bytes from our server vs real Xorg for same operations
- [ ] Verify QueryExtensionsString/QueryServerString padding consumption

### Fix Firefox/GTK3 DRISW crash
- [ ] Investigate Firefox GPU process crash with strace (install in container)
- [ ] Check if SHM buffer operations work correctly during DRISW init
- [ ] Test with different Mesa debug flags (MESA_DEBUG, LIBGL_DEBUG)
- [ ] Un-skip Firefox and GTK3 tests once fixed

### Audit GLX render opcode table
- [ ] Compare all ~300 render opcodes against Mesa's X_GLrop_* constants
- [ ] Fix mismatched entries (currently only affects indirect rendering)
- [ ] Replace magic numbers with named constants from opcodes.rs

### Broader Testing
- [ ] glxgears renders frames
- [ ] Chromium/Chrome launch
- [ ] XTS Xproto/Xlib full suite
- [ ] Stress testing (rapid reconnects, deep window hierarchies)
