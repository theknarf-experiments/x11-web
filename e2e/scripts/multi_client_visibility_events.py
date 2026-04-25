import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
# Open two independent connections
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
root = d1.screen().root
# Client 1 creates a window
w1 = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w1.map()
d1.sync()
# Test 1: Client 2 can see client 1 window via QueryTree
tree = d2.screen().root.query_tree()
c2_children = [c.id for c in tree.children]
if w1.id in c2_children:
    passed += 1
else:
    print(f"FAIL: client 2 cannot see client 1 window in QueryTree")
    failed += 1
# Test 2: Client 2 can read properties set by client 1
TEST_ATOM = d1.intern_atom("_X11WEB_MULTI_TEST")
w1.change_property(TEST_ATOM, Xlib.Xatom.STRING, 8, b"cross-client")
d1.sync()
time.sleep(0.1)
# Client 2 reads the property
win2 = d2.create_resource_object("window", w1.id)
TEST_ATOM2 = d2.intern_atom("_X11WEB_MULTI_TEST")
prop = win2.get_full_property(TEST_ATOM2, Xlib.Xatom.STRING)
if prop and prop.value == b"cross-client":
    passed += 1
else:
    print(f"FAIL: cross-client property read failed")
    failed += 1
# Test 3: Client 2 can get window geometry of client 1 window
geom = win2.get_geometry()
if geom.width == 100 and geom.height == 100:
    passed += 1
else:
    print(f"FAIL: geometry mismatch: {geom.width}x{geom.height}")
    failed += 1
w1.destroy()
d1.close()
d2.close()
print(f"multi-client: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
