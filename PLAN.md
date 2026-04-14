# X11 Server Full Spec Compliance Plan

## Current State (Phase 15 Complete)
- ~70K LOC Rust X11 server implementation
- 684 unit tests passing
- 121/127 core opcodes (120-126 historically unassigned per X11 spec)
- 26 extensions fully implemented with modular registry
- E2e tests passing: protocol-compliance (80/80), app-compatibility (19/19), advanced-compliance (91/91)
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

### Phase 15: Performance & Robustness ✓
- [x] Add per-client resource limits (windows, pixmaps, GCs, colormaps, cursors)
- [x] Bound pending_events queue per client
- [x] Fix mutex poison handling in menus.rs (use unwrap_or_else with logging)
- [x] Fix polygon rendering safety (.unwrap on .min/.max)
- [x] Use VecDeque for frozen events in grab.rs instead of Vec with remove(0)
- [x] Add motion_history size limit via configurable ResourceLimits
- [x] Add error logging for silent failures (compression, etc.)
- [x] E2e test: resource limits enforcement (5 robustness tests)
- [x] 684 unit tests passing, 91 advanced-compliance e2e tests passing

### Phase 16: Advanced Features ✓
- [x] XIM completion for CJK input: added CJK locales, compose state integration, dead key support, Greek/Cyrillic/Thai keysym mapping
- [x] XVideo: already fully implemented (10 FOURCC formats, YUV→ARGB conversion, BT.601/BT.709)
- [x] GLX: OSMesa software rendering working correctly with proper FBConfig/extension support
- [x] Firefox e2e tests: 6 tests covering startup, about:config navigation, Wikipedia, scroll, YouTube, local HTML5 video playback
- [x] Dockerfile: added ffmpeg/libavcodec-extra for video codecs, fonts-noto-cjk, fc-cache, test video content
- [x] 684 unit tests passing, 91 advanced-compliance e2e tests passing
- [ ] Git fetch, rebase against origin/main, fix any merge conflicts and push up

### Phase 17: WebRTC Transport & Audio ✓

Architecture: Frontend ↔ WebRTC ↔ Sidecar (peer-to-peer), Backend as signaling relay only.
- Data channel: binary msgpack protocol for display updates + input events
- Audio tracks: Opus-encoded PulseAudio output (sidecar→browser) and mic input (browser→sidecar)
- Libraries: `str0m` (Rust WebRTC, Sans-IO), browser native `RTCPeerConnection`

#### 17a: WebRTC Signaling Infrastructure ✓
- [x] Add signaling message types to protocol crate (SDP offer/answer, ICE candidates)
- [x] Add signaling relay to backend (frontend↔sidecar SDP/ICE exchange via existing WS)
- [x] Frontend: send/receive signaling messages through existing WebSocket

#### 17b: Sidecar WebRTC Endpoint ✓
- [x] Add `str0m` dependency to sidecar
- [x] Create `webrtc.rs` module: UDP socket, str0m Rtc agent, ICE handling
- [x] Create data channel for display updates (binary msgpack, no base64)
- [x] Receive input events over data channel
- [x] Keep WebSocket for signaling + process management only

#### 17c: Frontend WebRTC Client ✓
- [x] Create `useWebRTC.ts` hook: RTCPeerConnection, data channel, audio tracks
- [x] Binary msgpack decode for display updates on data channel
- [x] Send input events as binary msgpack on data channel
- [x] Fallback: keep WS path working for process/sidecar management

#### 17d: PulseAudio Integration & Audio Streaming ✓
- [x] Add PulseAudio to sidecar Dockerfile + startup
- [x] Capture audio via PulseAudio monitor source in sidecar
- [x] Encode as Opus, send via WebRTC audio track (str0m media)
- [x] Frontend: play incoming audio track via HTMLAudioElement / AudioContext

#### 17e: Microphone Support (Browser → Sidecar) ✓
- [x] Sidecar: receive audio track, decode Opus, pipe to PulseAudio virtual source
- [x] Update Dockerfiles: add pulseaudio, audacity, VLC packages

#### 17f: Final Integration & Tests ✓
- [x] E2e: WebRTC backward compat — xeyes, xterm via existing WS display path
- [x] E2e: PulseAudio running, virtual sinks configured
- [x] E2e: VLC test video plays with audio output (cvlc headless)
- [x] E2e: Audacity installed, detects PulseAudio virtual devices
- [x] E2e: audio capture pipeline (parec → Opus frames)
- [x] All existing e2e tests pass (81 protocol+compat, 8 webrtc-audio)
- [x] 684 unit tests passing
- [x] git commit
