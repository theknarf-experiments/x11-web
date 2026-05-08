import Xlib.display, Xlib.X, Xlib.error
d = Xlib.display.Display()
root = d.screen().root
w = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent)
w.map()
d.sync()

# Try invalid stack mode (5) — should cause a BadValue error
try:
    # Low-level protocol: send ConfigureWindow with stack_mode=5
    # The python-xlib library might not expose raw stack_mode values,
    # so we use a raw protocol request.
    import struct
    # ConfigureWindow opcode=12, length=4+1=5 words, mask=0x40 (stack-mode)
    mask = 0x40  # CWStackMode
    req = struct.pack('=BBHIHxx', 12, 0, 5, w.id, mask)
    req += struct.pack('=I', 5)  # invalid stack_mode = 5
    d.display.send_request(req, 0)
    d.sync()
    print("NO_ERROR")
except Xlib.error.BadValue:
    print("BAD_VALUE_ERROR")
except Exception as e:
    # Any error is acceptable — the key is the server doesn't crash
    print(f"OTHER_ERROR:{type(e).__name__}")

w.destroy()
d.sync()
