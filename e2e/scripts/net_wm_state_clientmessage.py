import Xlib.display, Xlib.X, Xlib.protocol.event, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
NET_WM_STATE = d.intern_atom("_NET_WM_STATE")
NET_WM_STATE_FULLSCREEN = d.intern_atom("_NET_WM_STATE_FULLSCREEN")
try:
    w = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    w.map()
    d.sync()
    # Send _NET_WM_STATE ClientMessage to root (EWMH spec)
    ev = Xlib.protocol.event.ClientMessage(
        window=w, client_type=NET_WM_STATE,
        data=(32, [1, NET_WM_STATE_FULLSCREEN, 0, 1, 0]))
    root.send_event(ev, event_mask=Xlib.X.SubstructureRedirectMask|Xlib.X.SubstructureNotifyMask)
    d.sync()
    passed += 1; print("PASS: sent _NET_WM_STATE ClientMessage to root")
    # Verify the state was applied
    import time; time.sleep(0.2)
    prop = w.get_full_property(NET_WM_STATE, Xlib.X.Atom("ATOM", d))
    if prop is not None:
        atoms = list(prop.value)
        if NET_WM_STATE_FULLSCREEN in atoms:
            passed += 1; print("PASS: fullscreen state set via ClientMessage")
        else:
            failed += 1; print(f"FAIL: fullscreen not in state {atoms}")
    else:
        failed += 1; print("FAIL: _NET_WM_STATE not found after ClientMessage")
    w.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"ewmh-cm: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
