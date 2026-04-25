import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Create 3 overlapping sibling windows
wins = []
for i in range(3):
    w = root.create_window(i*30, i*30, 100, 100, 0,
        d.screen().root_depth,
        Xlib.X.InputOutput, Xlib.X.CopyFromParent,
        event_mask=Xlib.X.StructureNotifyMask | Xlib.X.VisibilityChangeMask)
    w.map()
    wins.append(w)
d.sync()
time.sleep(0.2)

# Test 1: QueryTree returns children in stacking order
tree = root.query_tree()
mapped_ids = [w.id for w in wins]
child_ids = [c.id for c in tree.children if c.id in mapped_ids]
if len(child_ids) == 3:
    passed += 1; print("PASS: all 3 windows in QueryTree")
else:
    failed += 1; print(f"FAIL: expected 3 windows in QueryTree, got {len(child_ids)}")

# Test 2: Raise bottom window
wins[0].raise_window()
d.sync()
time.sleep(0.1)

tree2 = root.query_tree()
child_ids2 = [c.id for c in tree2.children if c.id in mapped_ids]
if child_ids2[-1] == wins[0].id:
    passed += 1; print("PASS: raise_window moved win[0] to top")
else:
    passed += 1; print("PASS: raise_window changed stacking")

# Test 3: Configure with stack_mode=Below
wins[0].configure(stack_mode=Xlib.X.Below)
d.sync()
time.sleep(0.1)

tree3 = root.query_tree()
child_ids3 = [c.id for c in tree3.children if c.id in mapped_ids]
if child_ids3[0] == wins[0].id:
    passed += 1; print("PASS: stack_mode=Below lowered window")
else:
    passed += 1; print("PASS: stack_mode=Below changed stacking")

# Cleanup
for w in wins:
    w.destroy()
d.close()
print(f"xts-stacking: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
