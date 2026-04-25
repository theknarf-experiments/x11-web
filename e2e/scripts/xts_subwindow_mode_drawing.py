import Xlib.display, Xlib.X, sys, time
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

parent = root.create_window(0, 0, 200, 200, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=0x000000,
    event_mask=Xlib.X.ExposureMask)
parent.map()
d.sync()
time.sleep(0.1)

child = parent.create_window(50, 50, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    background_pixel=0xFF0000)
child.map()
d.sync()
time.sleep(0.1)

# Test 1: Default GC (ClipByChildren) - drawing on parent clips around child
gc_clip = parent.create_gc(
    foreground=0x00FF00,
    subwindow_mode=Xlib.X.ClipByChildren)
parent.fill_rectangle(gc_clip, 0, 0, 200, 200)
d.sync()
passed += 1; print("PASS: ClipByChildren fill accepted")

# Test 2: IncludeInferiors GC - drawing overlaps children
gc_incl = parent.create_gc(
    foreground=0x0000FF,
    subwindow_mode=Xlib.X.IncludeInferiors)
parent.fill_rectangle(gc_incl, 0, 0, 200, 200)
d.sync()
passed += 1; print("PASS: IncludeInferiors fill accepted")

# Test 3: CopyGC copies subwindow_mode
gc_copy = parent.create_gc()
gc_copy.copy(gc_incl, Xlib.X.GCSubwindowMode)
d.sync()
passed += 1; print("PASS: CopyGC with GCSubwindowMode")

gc_clip.free()
gc_incl.free()
gc_copy.free()
child.destroy()
parent.destroy()
d.close()
print(f"xts-subwindow-mode: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
