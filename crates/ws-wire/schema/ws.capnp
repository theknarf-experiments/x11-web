@0xc0c0fee1ba5eba11;

# Browser ↔ Backend WebSocket binary wire format. Replaces the
# previous JSON-over-text encoding so both sides share one schema
# (this file) instead of duplicating types in
# `crates/protocol/src/lib.rs` and `frontend/src/types.ts`.
#
# Field numbers are explicit and append-only — never renumber,
# never reuse. Renaming a field is fine; changing its ordinal is
# not. Adding a new union variant is the canonical way to extend.
#
# `traceparent` rides as a top-level field (rather than a magic
# JSON envelope key like the old `_traceparent`) so OTel context
# propagation across the WS keeps working — see
# `crates/backend/src/main.rs::handle_frontend_ws` for the parent
# context adoption.

# ============================================================
# Frontend → Backend
# ============================================================

struct FrontendMsg {
    # W3C Trace Context. Empty string when OTel is disabled or
    # when there is no active span on the sender.
    traceparent @0 :Text;

    payload :union {
        # Default for unknown / unset variants — gives readers a
        # safe fallback when a future schema variant lands. Cap'n
        # Proto unions need at least two members.
        noVariant @1 :Void;

        openWorkspace      @2 :OpenWorkspace;
        spawnProcess       @3 :SpawnProcess;
        killProcess        @4 :KillProcess;
        inputEvent         @5 :InputEventCmd;
        resizeWindow       @6 :ResizeWindowCmd;
        rtcOffer           @7 :RtcSdp;
        rtcIceCandidate    @8 :RtcIceCandidate;
    }
}

# `id` empty → backend creates a fresh workspace and returns the
# new id in `BackendMsg.workspace`.
struct OpenWorkspace {
    id @0 :Text;
}

struct SpawnProcess {
    requestId   @0 :Text;
    sidecarId   @1 :Text;
    workspaceId @2 :Text;
    command     @3 :Text;
    args        @4 :List(Text);
}

struct KillProcess {
    requestId @0 :Text;
    sidecarId @1 :Text;
    pid       @2 :UInt32;
}

struct InputEventCmd {
    sidecarId @0 :Text;
    windowId  @1 :Text;
    event     @2 :InputEvent;
}

struct ResizeWindowCmd {
    sidecarId @0 :Text;
    windowId  @1 :Text;
    width     @2 :UInt16;
    height    @3 :UInt16;
}

struct RtcSdp {
    sdp @0 :Text;
}

# Trickled ICE candidate. `sdpMid` empty + `sdpMlineIndexHas`=false
# means absent, matching the JS shape where both fields are
# optional but at least one is conventionally set.
struct RtcIceCandidate {
    candidate          @0 :Text;
    sdpMid             @1 :Text;
    sdpMlineIndexHas   @2 :Bool;
    sdpMlineIndex      @3 :UInt16;
}

# ============================================================
# Backend → Frontend
# ============================================================

struct BackendMsg {
    traceparent @0 :Text;

    payload :union {
        noVariant       @1 :Void;
        sidecarList     @2 :SidecarListMsg;
        workspace       @3 :WorkspaceMsg;
        commandResult   @4 :CommandResult;
        processList     @5 :ProcessListMsg;
        windowUpdate    @6 :WindowUpdateMsg;
        windowList      @7 :WindowListMsg;
        bell            @8 :Bell;
        rtcAnswer       @9 :RtcSdp;
        rtcIceCandidate @10 :RtcIceCandidate;
    }
}

struct SidecarListMsg {
    sidecars @0 :List(SidecarInfo);
}

struct SidecarInfo {
    id   @0 :Text;
    name @1 :Text;
}

struct WorkspaceMsg {
    workspace @0 :Workspace;
}

struct Workspace {
    id   @0 :Text;
    name @1 :Text;
}

struct CommandResult {
    requestId @0 :Text;
    success   @1 :Bool;
    message   @2 :Text;
}

struct ProcessListMsg {
    sidecarId @0 :Text;
    processes @1 :List(ProcessInfo);
}

struct ProcessInfo {
    pid      @0 :UInt32;
    clientId @1 :Text;
    command  @2 :Text;
}

struct WindowUpdateMsg {
    update @0 :WindowUpdate;
}

struct WindowUpdate {
    union {
        noVariant      @0 :Void;
        titleChanged   @1 :TitleChanged;
        stateChanged   @2 :StateChanged;
        focused        @3 :Focused;
        menuStructure  @4 :MenuStructure;
    }
}

struct TitleChanged {
    windowId @0 :Text;
    title    @1 :Text;
}

struct StateChanged {
    windowId @0 :Text;
    state    @1 :WindowWmState;
}

# `windowId` unset = focus cleared (revert to root). Text fields
# expose `has_window_id()` automatically, so no separate flag.
struct Focused {
    windowId @0 :Text;
}

struct MenuStructure {
    windowId @0 :Text;
    items    @1 :List(MenuItem);
}

enum WindowWmState {
    normal     @0;
    minimized  @1;
    maximized  @2;
    fullscreen @3;
    close      @4;
}

struct WindowListMsg {
    windows @0 :List(WindowDescriptor);
}

struct WindowDescriptor {
    windowId         @0  :Text;
    sidecarId        @1  :Text;
    pid              @2  :UInt32;
    command          @3  :Text;
    x                @4  :Float64;
    y                @5  :Float64;
    width            @6  :UInt16;
    height           @7  :UInt16;
    borderWidth      @8  :UInt16;
    borderPixel      @9  :UInt32;
    overrideRedirect @10 :Bool;
    resizable        @11 :Bool;
}

struct Bell {
    percent @0 :UInt8;
}

# ============================================================
# Menus (recursive — `MenuItem.children` is List(MenuItem))
# ============================================================

# Text + struct fields auto-expose `has_*()` for absence checks,
# so `Option<String>` and `Option<MenuAction>` need no extra flag.
# `checked` is the only `Option<primitive>` here — encoded via a
# 3-state enum (NotApplicable / Unchecked / Checked) so the reader
# can distinguish "explicitly false" from "absent".
struct MenuItem {
    id          @0  :Text;
    label       @1  :Text;
    kind        @2  :MenuItemKind;
    enabled     @3  :Bool;
    visible     @4  :Bool;
    checked     @5  :CheckState;
    accelerator @6  :Text;
    icon        @7  :Text;
    action      @8  :MenuAction;
    children    @9  :List(MenuItem);
}

enum MenuItemKind {
    normal    @0;
    submenu   @1;
    separator @2;
    checkbox  @3;
    radio     @4;
}

enum CheckState {
    notApplicable @0;
    unchecked     @1;
    checked       @2;
}

struct MenuAction {
    name   @0 :Text;
    target @1 :MenuActionTarget;  # absent via `has_target()`
}

struct MenuActionTarget {
    union {
        string  @0 :Text;
        boolean @1 :Bool;
        int32   @2 :Int32;
        uInt32  @3 :UInt32;
        int64   @4 :Int64;
        float64 @5 :Float64;
    }
}

# ============================================================
# Input events (frontend → sidecar via backend)
# ============================================================

struct InputEvent {
    payload :union {
        noVariant         @0  :Void;
        keyPress          @1  :KeyEvent;
        keyRelease        @2  :KeyEvent;
        buttonPress       @3  :ButtonEvent;
        buttonRelease     @4  :ButtonEvent;
        motionNotify      @5  :MotionEvent;
        menuActivate      @6  :MenuActivateEvt;
        windowManage      @7  :WindowManageEvt;
        dndBridge         @8  :DndBridgeEvt;
        touchBegin        @9  :TouchEvent;
        touchUpdate       @10 :TouchEvent;
        touchEnd          @11 :TouchEvent;
        gestureSwipe      @12 :GestureSwipeEvt;
        gesturePinch      @13 :GesturePinchEvt;
        compositionEvent  @14 :CompositionEvt;
    }
}

struct KeyEvent {
    keycode @0 :UInt32;
    state   @1 :UInt16;
}

struct ButtonEvent {
    button @0 :UInt8;
    x      @1 :Int16;
    y      @2 :Int16;
    state  @3 :UInt16;
}

struct MotionEvent {
    x     @0 :Int16;
    y     @1 :Int16;
    state @2 :UInt16;
}

struct MenuActivateEvt {
    action @0 :MenuAction;
}

struct WindowManageEvt {
    action @0 :WindowWmState;
}

struct DndBridgeEvt {
    event @0 :DndEvent;
}

struct DndEvent {
    payload :union {
        noVariant @0 :Void;
        enter     @1 :DndEnter;
        position  @2 :DndPosition;
        drop      @3 :DndDrop;
        leave     @4 :Void;
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
    data     @1 :Data;
}

struct TouchEvent {
    touchId @0 :UInt32;
    x       @1 :Int16;
    y       @2 :Int16;
    state   @3 :UInt16;
}

enum GesturePhase {
    begin  @0;
    update @1;
    end    @2;
}

struct GestureSwipeEvt {
    phase   @0 :GesturePhase;
    fingers @1 :UInt8;
    dx      @2 :Float32;
    dy      @3 :Float32;
}

struct GesturePinchEvt {
    phase    @0 :GesturePhase;
    fingers  @1 :UInt8;
    dx       @2 :Float32;
    dy       @3 :Float32;
    scale    @4 :Float32;
    rotation @5 :Float32;
}

struct CompositionEvt {
    phase @0 :Text;
    text  @1 :Text;
}
