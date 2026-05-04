@0xb1d4c5a2e7f31a09;

# Wire protocol between the backend and a sidecar.
#
# Single bidirectional QUIC stream carries a sequence of Cap'n Proto
# messages. The first exchange is `Hello` (sidecar → backend) and
# `HelloAck` (backend → sidecar); after that each side writes the
# union message defined for its direction.
#
# Field numbers are explicit and append-only — never renumber, never
# reuse. To remove a field, leave its slot reserved (Cap'n Proto
# doesn't enforce this syntactically; we enforce by review).
#
# Versioning: `Hello.protocolVersion`. Backend rejects connections
# whose major version doesn't match. Minor / patch additions are
# expressed by adding new union variants here, which older peers
# safely ignore (Cap'n Proto unions surface unknown discriminants
# as the default `Void` case).

# ---------- Handshake ----------

struct Hello {
    protocolVersion @0 :UInt32;
    # Opaque bytes: lets us swap text tokens (v0) for cryptographic
    # nonce signatures or pre-shared keys (v1) without a schema bump.
    bearerToken @1 :Data;
    sidecarName @2 :Text;
    # Distinguishes X11 (auto-stream every window) from macOS
    # (stream on demand when the user drags a polaroid out of the
    # picker into the canvas). Old sidecars that don't set this
    # default to `unknown` — backend treats them as X11 for
    # backwards compatibility.
    sidecarKind @3 :SidecarKind;
}

enum SidecarKind {
    unknown @0;
    x11 @1;
    macos @2;
}

struct HelloAck {
    union {
        ok @0 :Ok;
        rejected @1 :Rejected;
    }

    struct Ok {
        sidecarId @0 :Text;
        # Backend tells the sidecar what protocol version it agreed
        # to. Sidecar may close the connection if it doesn't like
        # the answer.
        agreedProtocolVersion @1 :UInt32;
    }

    struct Rejected {
        # Human-readable. Wire protocol error codes can be added
        # later if we need machine-routable rejection reasons.
        message @0 :Text;
    }
}

# ---------- Sidecar → Backend ----------

struct FromSidecar {
    union {
        heartbeat @0 :Void;

        # Lifecycle of a process the sidecar has spawned or
        # discovered (the macOS sidecar synthesizes
        # `processConnected` from window enumeration since macOS
        # apps don't connect to a server like X11 clients do).
        processConnected @1 :ProcessConnected;
        processExited @2 :ProcessExited;

        # Display surface updates — windows appearing / moving /
        # gaining pixels. The largest variant by far is `putImage`
        # which carries an encoded frame.
        display @3 :DisplayUpdate;

        # Replies to spawn / kill / list-processes requests.
        processSpawned @4 :ProcessSpawnedReply;
        processKilled @5 :ProcessKilledReply;
        processList @6 :ProcessListReply;

        # Generic error response, used by the X11 sidecar to surface
        # spawn/kill failures back to the requesting frontend.
        errorReply @7 :ErrorReply;
    }
}

struct ProcessConnected {
    pid @0 :UInt32;
    clientId @1 :Text;
    command @2 :Text;
}

struct ProcessExited {
    pid @0 :UInt32;
    exitStatus :union {
        # Normal exit; carries the exit status code.
        code @1 :Int32;
        # Process was terminated by a signal — no meaningful code.
        # `KillProcess` ack also uses this branch since we don't
        # wait() for the real status before replying.
        killedBySignal @2 :Void;
    }
}

struct ProcessSpawnedReply {
    requestId @0 :Text;
    pid @1 :UInt32;
}

struct ProcessKilledReply {
    requestId @0 :Text;
    pid @1 :UInt32;
}

struct ProcessListReply {
    requestId @0 :Text;
    processes @1 :List(ProcessInfo);
}

struct ProcessInfo {
    pid @0 :UInt32;
    command @1 :Text;
}

struct ErrorReply {
    # `requestId` is logically nullable — some errors aren't tied
    # to a specific request. Use the auto-generated `hasRequestId()`
    # accessor; null pointer means "general error".
    requestId @0 :Text;
    message @1 :Text;
}

struct DisplayUpdate {
    clientId @0 :Text;
    payload @1 :DisplayPayload;
}

struct DisplayPayload {
    union {
        windowCreated @0 :WindowCreated;
        windowDestroyed @1 :WindowDestroyed;
        windowMapped @2 :WindowMapped;
        windowUnmapped @3 :WindowUnmapped;
        windowConfigured @4 :WindowConfigured;
        titleChanged @5 :TitleChanged;
        putImage @6 :PutImage;

        # Ordinals @7 / @8 / @9 are reserved — they previously held
        # cursorChanged / cursorBitmap / cursorAnimated. Cursor
        # plumbing was never wired all the way through to a usable
        # browser cursor, so the wire variants were removed; the
        # X11 sidecar still parses cursor resources internally.
        # Don't reuse these ordinals — pick @16 onwards if cursor
        # delivery is ever wired back.
        reservedCursor7 @7 :Void;
        reservedCursor8 @8 :Void;
        reservedCursor9 @9 :Void;

        # Window manager state, focus, and z-order signals the
        # frontend needs to render correctly.
        windowFocused @10 :WindowFocused;
        windowRaised @11 :WindowRaised;
        windowStateChanged @12 :WindowStateChanged;

        # Audio cue — frontend plays a bell sound at `percent`
        # volume.
        bell @13 :Bell;

        # AppMenu mirroring (GTK / Qt apps via DBus). The X11
        # sidecar's MenuTracker emits the full tree on first
        # discovery; deltas (state changes per item) are not yet
        # implemented — sidecar re-emits the full tree on change.
        menuStructure @14 :MenuStructure;

        # Low-resolution preview of a window, refreshed at low rate
        # (~1 Hz) so the frontend can render thumbnails in the
        # spawn-popover before deciding to attach the window to the
        # canvas. Pre-encoded as WebP — frontend decodes via
        # `createImageBitmap`.
        windowThumbnail @15 :WindowThumbnail;
    }
}

struct WindowCreated {
    windowId @0 :Text;
    x @1 :Int16;
    y @2 :Int16;
    width @3 :UInt16;
    height @4 :UInt16;
    isTopLevel @5 :Bool;
    overrideRedirect @6 :Bool;
    borderWidth @7 :UInt16;
    borderPixel @8 :UInt32;
    # Whether the user can resize the window. macOS apps like
    # Calculator have fixed-size windows; setting AXSize on them
    # silently no-ops. The frontend hides the resize handles when
    # this is false.
    resizable @9 :Bool;
}

struct WindowDestroyed {
    windowId @0 :Text;
}

struct WindowMapped {
    windowId @0 :Text;
    isTopLevel @1 :Bool;
    overrideRedirect @2 :Bool;
}

struct WindowUnmapped {
    windowId @0 :Text;
}

struct WindowConfigured {
    windowId @0 :Text;
    x @1 :Int16;
    y @2 :Int16;
    width @3 :UInt16;
    height @4 :UInt16;
    borderWidth @5 :UInt16;
    borderPixel @6 :UInt32;
    # See `WindowCreated.resizable`. Re-emitted on every
    # configure so apps that flip the constraint at runtime
    # (rare) reach the frontend.
    resizable @7 :Bool;
}

struct TitleChanged {
    windowId @0 :Text;
    title @1 :Text;
}

struct PutImage {
    windowId @0 :Text;
    x @1 :Int16;
    y @2 :Int16;
    width @3 :UInt16;
    height @4 :UInt16;
    encoding @5 :ImageEncoding;
    data @6 :Data;
}

enum ImageEncoding {
    rawRgba @0;
    jpeg @1;
    png @2;
}

# Low-rate window preview. Distinct from `PutImage` so the backend
# doesn't have to disambiguate "tile of a live frame" from "thumbnail
# for the picker". Always WebP-encoded; frontend renders directly via
# `createImageBitmap`.
struct WindowThumbnail {
    windowId @0 :Text;
    width @1 :UInt16;
    height @2 :UInt16;
    data @3 :Data;
}

# Window state.
struct WindowFocused {
    # `windowId` is logically nullable (focus cleared = "no
    # window"). Use the auto-generated `hasWindowId()` accessor;
    # null pointer means "no focus".
    windowId @0 :Text;
}

struct WindowRaised {
    windowId @0 :Text;
}

struct WindowStateChanged {
    windowId @0 :Text;
    state @1 :WindowWmState;
}

enum WindowWmState {
    normal @0;
    minimized @1;
    maximized @2;
    fullscreen @3;
    close @4;
}

struct Bell {
    percent @0 :UInt8;
}

# AppMenu mirroring. Sidecar emits the full tree on discovery and
# again whenever menu state changes (no per-item delta yet).
struct MenuStructure {
    windowId @0 :Text;
    menu @1 :List(MenuItem);
}

# Recursive — submenus contain their own MenuItem children. Cap'n
# Proto handles this via List(MenuItem) on the children field.
#
# Pointer fields (`label`, `accelerator`, `icon`, `action`) are
# logically optional; null pointer ↔ `None`, accessed via the
# auto-generated `has*()` getters.
struct MenuItem {
    id @0 :Text;
    label @1 :Text;
    kind @2 :MenuItemKind;
    enabled @3 :Bool;
    visible @4 :Bool;
    # `Option<Bool>` mapped to a tri-state enum: `notApplicable` for
    # separators / normal items, `unchecked` / `checked` for
    # checkboxes and radios.
    checked @5 :CheckState;
    accelerator @6 :Text;
    icon @7 :Text;
    action @8 :MenuAction;
    children @9 :List(MenuItem);
}

enum MenuItemKind {
    normal @0;
    submenu @1;
    separator @2;
    checkbox @3;
    radio @4;
}

enum CheckState {
    notApplicable @0;
    unchecked @1;
    checked @2;
}

struct MenuAction {
    name @0 :Text;
    # Typed union mirroring `protocol::MenuActionTarget`. Pointer
    # field — null (auto `hasTarget()`) means the action is
    # parameterless.
    target @1 :MenuActionTarget;
}

struct MenuActionTarget {
    union {
        string @0 :Text;
        boolean @1 :Bool;
        int32 @2 :Int32;
        uInt32 @3 :UInt32;
        int64 @4 :Int64;
        float64 @5 :Float64;
    }
}

# ---------- Backend → Sidecar ----------

struct ToSidecar {
    union {
        # Forward an input event coming from the frontend.
        inputEvent @0 :InputEventEnvelope;

        # The frontend's WindowFrame got resized; ask the sidecar
        # to resize the corresponding window if its windowing
        # model permits.
        resizeWindow @1 :ResizeWindow;

        # Process control. macOS sidecar doesn't implement these
        # yet; the schema slot exists for parity with X11.
        spawnProcess @2 :SpawnProcess;
        killProcess @3 :KillProcess;

        # Process listing — request, the reply comes back as
        # `processList` on the FromSidecar union.
        listProcesses @4 :ListProcessesReq;

        # Begin / end live capture of a specific window. macOS
        # sidecar enumerates every window into the picker via
        # thumbnails by default; the live SCStream only spins up
        # when the backend asks via `startWindowCapture` (and stops
        # when no workspace has it attached anymore).
        # X11 sidecar streams unconditionally; ignores these.
        startWindowCapture @5 :WindowCaptureReq;
        stopWindowCapture @6 :WindowCaptureReq;
    }
}

struct WindowCaptureReq {
    windowId @0 :Text;
}

struct InputEventEnvelope {
    windowId @0 :Text;
    event @1 :InputEvent;
}

struct InputEvent {
    union {
        keyPress @0 :KeyPress;
        keyRelease @1 :KeyRelease;
        buttonPress @2 :ButtonPress;
        buttonRelease @3 :ButtonRelease;
        motionNotify @4 :MotionNotify;

        # AppMenu / window-manage actions originated by the
        # frontend's chrome (clicking a menu item, the close
        # button, etc.).
        menuActivate @5 :MenuActivateEvent;
        windowManage @6 :WindowManageEvent;

        # Browser → X11 drag-and-drop bridge.
        dndBridge @7 :DndBridgeEvent;

        # Touch + gesture from a touchscreen / trackpad pinch.
        touchBegin @8 :TouchEvent;
        touchUpdate @9 :TouchEvent;
        touchEnd @10 :TouchEvent;
        gestureSwipe @11 :GestureSwipeEvent;
        gesturePinch @12 :GesturePinchEvent;

        # IME / dead-key composition for CJK + multilingual input.
        compositionEvent @13 :CompositionEvent;
    }
}

struct KeyPress {
    keycode @0 :UInt32;
    state @1 :UInt16;
}

struct KeyRelease {
    keycode @0 :UInt32;
    state @1 :UInt16;
}

struct ButtonPress {
    button @0 :UInt8;
    x @1 :Int16;
    y @2 :Int16;
    state @3 :UInt16;
}

struct ButtonRelease {
    button @0 :UInt8;
    x @1 :Int16;
    y @2 :Int16;
    state @3 :UInt16;
}

struct MotionNotify {
    x @0 :Int16;
    y @1 :Int16;
    state @2 :UInt16;
}

struct ResizeWindow {
    windowId @0 :Text;
    width @1 :UInt16;
    height @2 :UInt16;
}

struct SpawnProcess {
    requestId @0 :Text;
    command @1 :Text;
    args @2 :List(Text);
}

struct KillProcess {
    requestId @0 :Text;
    pid @1 :UInt32;
}

struct ListProcessesReq {
    requestId @0 :Text;
}

# AppMenu activation. `action.name` is the namespaced action name
# (e.g. `app.quit`); `target` is JSON-encoded GVariant.
struct MenuActivateEvent {
    action @0 :MenuAction;
}

struct WindowManageEvent {
    action @0 :WindowWmState;
}

struct DndBridgeEvent {
    event @0 :DndEventKind;
}

# Drag-and-drop event payloads from the browser into the X11
# `XdndDrop` protocol. Mirrors the protocol crate's `DndEventKind`.
struct DndEventKind {
    union {
        enter @0 :DndEnter;
        position @1 :DndPosition;
        drop @2 :DndDrop;
        leave @3 :Void;
    }
}

struct DndEnter {
    mimeTypes @0 :List(Text);
}

struct DndPosition {
    x @0 :Int16;
    y @1 :Int16;
}

struct DndDrop {
    mimeType @0 :Text;
    data @1 :Data;
}

struct TouchEvent {
    touchId @0 :UInt32;
    x @1 :Int16;
    y @2 :Int16;
    state @3 :UInt16;
}

struct GestureSwipeEvent {
    phase @0 :GesturePhase;
    fingers @1 :UInt8;
    dx @2 :Float32;
    dy @3 :Float32;
}

struct GesturePinchEvent {
    phase @0 :GesturePhase;
    fingers @1 :UInt8;
    dx @2 :Float32;
    dy @3 :Float32;
    scale @4 :Float32;
    rotation @5 :Float32;
}

enum GesturePhase {
    begin @0;
    update @1;
    end @2;
}

struct CompositionEvent {
    # Phase: "start" / "update" / "end" — text is dead-key /
    # IME accumulator state.
    phase @0 :Text;
    text @1 :Text;
}
