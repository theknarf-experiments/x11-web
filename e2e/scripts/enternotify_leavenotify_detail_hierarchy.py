import Xlib.display, Xlib.X, Xlib.protocol.event, sys, time
passed = 0; failed = 0
try:
    d = Xlib.display.Display()
    root = d.screen().root
    # Create parent window
    parent = root.create_window(10, 10, 300, 300, 0,
        d.screen().root_depth, Xlib.X.InputOutput,
        Xlib.X.CopyFromParent,
        event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
    # Create child inside parent
    child = parent.create_window(50, 50, 100, 100, 0,
        d.screen().root_depth, Xlib.X.InputOutput,
        Xlib.X.CopyFromParent,
        event_mask=Xlib.X.EnterWindowMask | Xlib.X.LeaveWindowMask)
    parent.map()
    child.map()
    d.sync()
    time.sleep(0.5)
    # Warp pointer into parent (outside child)
    root.warp_pointer(20, 20)
    d.sync()
    time.sleep(0.3)
    # Drain existing events
    while d.pending_events():
        d.next_event()
    # Warp into child
    root.warp_pointer(70, 70)
    d.sync()
    time.sleep(0.3)
    # Check events: parent should get LeaveNotify(detail=Inferior)
    # child should get EnterNotify(detail=Ancestor)
    leave_found = False; enter_found = False
    while d.pending_events():
        ev = d.next_event()
        if hasattr(ev, "detail"):
            if ev.type == Xlib.X.LeaveNotify and ev.window == parent:
                if ev.detail == 2:  # Inferior
                    leave_found = True; passed += 1
                    print("PASS: LeaveNotify detail=Inferior on parent")
                else:
                    failed += 1; print(f"FAIL: LeaveNotify detail={ev.detail}, expected 2 (Inferior)")
            elif ev.type == Xlib.X.EnterNotify and ev.window == child:
                if ev.detail == 0:  # Ancestor
                    enter_found = True; passed += 1
                    print("PASS: EnterNotify detail=Ancestor on child")
                else:
                    failed += 1; print(f"FAIL: EnterNotify detail={ev.detail}, expected 0 (Ancestor)")
    if not leave_found: failed += 1; print("FAIL: no LeaveNotify on parent")
    if not enter_found: failed += 1; print("FAIL: no EnterNotify on child")
    parent.destroy()
    d.close()
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"crossing-detail: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
