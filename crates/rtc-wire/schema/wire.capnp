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
