import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    w1 = root.create_window(0, 0, 200, 200, 0, 24, Xlib.X.InputOutput)
    w2 = root.create_window(100, 100, 200, 200, 0, 24, Xlib.X.InputOutput)
    w1.map()
    w2.map()
    d.sync()
    # SetInputFocus to w1 with RevertToParent
    d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
    d.sync()
    focus = d.get_input_focus()
    if focus.focus.id == w1.id:
        passed += 1; print(f"PASS: focus on w1={w1.id:#x}")
    else:
        failed += 1; print(f"FAIL: focus={focus.focus.id:#x} expected={w1.id:#x}")
    if focus.revert_to == Xlib.X.RevertToParent:
        passed += 1; print("PASS: revert_to=RevertToParent")
    else:
        failed += 1; print(f"FAIL: revert_to={focus.revert_to}")
    # Switch focus to w2 with RevertToPointerRoot
    d.set_input_focus(w2, Xlib.X.RevertToPointerRoot, Xlib.X.CurrentTime)
    d.sync()
    focus = d.get_input_focus()
    if focus.focus.id == w2.id:
        passed += 1; print(f"PASS: focus on w2={w2.id:#x}")
    else:
        failed += 1; print(f"FAIL: focus={focus.focus.id:#x} expected={w2.id:#x}")
    if focus.revert_to == Xlib.X.RevertToPointerRoot:
        passed += 1; print("PASS: revert_to=RevertToPointerRoot")
    else:
        failed += 1; print(f"FAIL: revert_to={focus.revert_to}")
    # SetInputFocus to PointerRoot
    d.set_input_focus(Xlib.X.PointerRoot, Xlib.X.RevertToNone, Xlib.X.CurrentTime)
    d.sync()
    focus = d.get_input_focus()
    if focus.focus.id == 1:
        passed += 1; print("PASS: focus=PointerRoot")
    else:
        failed += 1; print(f"FAIL: focus={focus.focus.id} expected PointerRoot")
    w1.destroy()
    w2.destroy()
    d.sync()
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"focus-model: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
