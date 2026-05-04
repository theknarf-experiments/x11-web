@0xfe1d2c3b4a596877;

# Browser ↔ Backend binary wire format, carried over a WebRTC
# DataChannel. Each DataChannel message is one Cap'n Proto serialised
# `Frame`.
#
# Field numbers are explicit and append-only — never renumber, never
# reuse. Renaming a field is fine; changing its ordinal is not.

# Top-level wrapper. Adding a new variant is the canonical way to
# extend the protocol; older clients will see `noVariant()` on
# unknown ordinals.
struct Frame {
    union {
        # Default for unknown / unset variants — gives old clients a
        # safe fallback when they receive a future variant they don't
        # know about. Cap'n Proto unions need at least two members.
        noVariant @0 :Void;

        # Pixel rectangle for a window. Raw RGBA, no compression, no
        # base64 — DataChannel carries arbitrary bytes.
        putImage @1 :PutImage;

        # Low-rate, downscaled preview of a window's pixels. Same
        # shape as PutImage minus the (x, y) offset since thumbnails
        # always represent the full window. WebP-encoded; frontend
        # decodes via createImageBitmap.
        windowThumbnail @2 :WindowThumbnail;

        # Symmetric Automerge sync message for a workspace doc.
        # Carried over the dedicated control DataChannel
        # (ordered+reliable). Same shape both directions — peers
        # exchange these in rounds until they have nothing more to
        # send. The `message` is the raw output of
        # `automerge::sync::Message::encode`; opaque to this layer.
        workspaceSync @3 :WorkspaceSync;
    }
}

struct PutImage {
    windowId @0 :Text;
    x @1 :Int16;
    y @2 :Int16;
    width @3 :UInt16;
    height @4 :UInt16;
    data @5 :Data;
}

struct WindowThumbnail {
    windowId @0 :Text;
    width @1 :UInt16;
    height @2 :UInt16;
    data @3 :Data;
}

struct WorkspaceSync {
    workspaceId @0 :Text;
    message @1 :Data;
}
