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

        # Diagnostics: an InputEvent arrived for a window UUID the
        # sidecar's router has no entry for. Lets the frontend
        # surface "input dropped" instead of silently swallowing.
        inputDropped @4 :InputDropped;
    }
}

struct ProcessConnected {
    pid @0 :UInt32;
    clientId @1 :Text;
    command @2 :Text;
}

struct ProcessExited {
    pid @0 :UInt32;
    # `exitCode` is logically nullable. Cap'n Proto has no Option
    # type — encode "no exit code" as `hasExitCode = false`.
    hasExitCode @1 :Bool;
    exitCode @2 :Int32;
}

struct InputDropped {
    windowId @0 :Text;
    reason @1 :Text;
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

# ---------- Backend → Sidecar ----------

struct ToSidecar {
    union {
        # Forward an input event coming from the frontend.
        inputEvent @0 :InputEventEnvelope;

        # Force a redraw — sidecar should re-emit the latest frame
        # for that window. Used when a frontend reconnects.
        requestRedraw @1 :RequestRedraw;

        # The frontend's WindowFrame got resized; ask the sidecar
        # to resize the corresponding window if its windowing
        # model permits.
        resizeWindow @2 :ResizeWindow;

        # Process control. macOS sidecar doesn't implement these
        # yet; the schema slot exists for parity with X11.
        spawnProcess @3 :SpawnProcess;
        killProcess @4 :KillProcess;
    }
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

struct RequestRedraw {
    windowId @0 :Text;
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
