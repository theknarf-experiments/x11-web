import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
try:
    d = Xlib.display.Display()
    root = d.screen().root
    # Two sibling windows
    w1 = root.create_window(10, 10, 100, 100, 0,
        d.screen().root_depth, Xlib.X.InputOutput,
        Xlib.X.CopyFromParent,
        event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
    w2 = root.create_window(200, 10, 100, 100, 0,
        d.screen().root_depth, Xlib.X.InputOutput,
        Xlib.X.CopyFromParent,
        event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
    w1.map(); w2.map()
    d.sync(); time.sleep(0.5)
    # Warp to w1
    root.warp_pointer(50, 50)
    d.sync(); time.sleep(0.3)
    while d.pending_events(): d.next_event()
    # Warp to w2 (sibling = Nonlinear)
    root.warp_pointer(250, 50)
    d.sync(); time.sleep(0.3)
    leave_ok = False; enter_ok = False
    while d.pending_events():
        ev = d.next_event()
        if hasattr(ev, "detail"):
            if ev.type == Xlib.X.LeaveNotify and ev.window == w1:
                if ev.detail == 3:  # Nonlinear
                    leave_ok = True; passed += 1
                    print("PASS: LeaveNotify detail=Nonlinear on sibling w1")
                else:
                    failed += 1; print(f"FAIL: LeaveNotify detail={ev.detail}, expected 3")
            elif ev.type == Xlib.X.EnterNotify and ev.window == w2:
                if ev.detail == 3:  # Nonlinear
                    enter_ok = True; passed += 1
                    print("PASS: EnterNotify detail=Nonlinear on sibling w2")
                else:
                    failed += 1; print(f"FAIL: EnterNotify detail={ev.detail}, expected 3")
    if not leave_ok: failed += 1; print("FAIL: no LeaveNotify on w1")
    if not enter_ok: failed += 1; print("FAIL: no EnterNotify on w2")
    w1.destroy(); w2.destroy()
    d.close()
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"crossing-nonlinear: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
