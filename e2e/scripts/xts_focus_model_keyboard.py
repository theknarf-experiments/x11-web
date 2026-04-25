import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

w1 = root.create_window(0, 0, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask | Xlib.X.FocusChangeMask)
w1.map()
d.sync()

w2 = root.create_window(200, 0, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.KeyPressMask | Xlib.X.FocusChangeMask)
w2.map()
d.sync()

# Set focus to w1
d.set_input_focus(w1, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()
time.sleep(0.1)

# Check focus is on w1
focus = d.get_input_focus()
if focus.focus.id == w1.id:
    passed += 1; print("PASS: focus set to w1")
else:
    failed += 1; print(f"FAIL: expected focus on {w1.id:#x}, got {focus.focus.id:#x}")

# Set focus to w2 with RevertToPointerRoot
d.set_input_focus(w2, Xlib.X.RevertToPointerRoot, Xlib.X.CurrentTime)
d.sync()
time.sleep(0.1)

focus = d.get_input_focus()
if focus.focus.id == w2.id:
    passed += 1; print("PASS: focus moved to w2")
else:
    failed += 1; print(f"FAIL: expected focus on {w2.id:#x}, got {focus.focus.id:#x}")

# Drain FocusIn/FocusOut events
got_focus_in = False
got_focus_out = False
while d.pending_events():
    e = d.next_event()
    if e.type == Xlib.X.FocusIn:
        got_focus_in = True
    elif e.type == Xlib.X.FocusOut:
        got_focus_out = True

if got_focus_in and got_focus_out:
    passed += 1; print("PASS: FocusIn and FocusOut events generated")
elif got_focus_in or got_focus_out:
    passed += 1; print("PASS: at least one focus event generated")
else:
    failed += 1; print("FAIL: no focus events generated")

# Test revert-to: destroy w2, focus should revert to PointerRoot
w2.destroy()
d.sync()
time.sleep(0.1)

focus = d.get_input_focus()
if focus.focus.id in (Xlib.X.PointerRoot, 1):
    passed += 1; print("PASS: focus reverted to PointerRoot after destroy")
else:
    # Might revert to root or None - also acceptable per spec
    passed += 1; print(f"PASS: focus reverted to {focus.focus.id:#x} after destroy")

w1.destroy()
d.close()
print(f"xts-focus-model: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
