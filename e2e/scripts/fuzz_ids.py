import Xlib.display
import Xlib.X
import Xlib.error
import Xlib.xobject.drawable
import sys

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()

# Test 1: GetGeometry on a bogus window ID
bogus_ids = [0, 1, 0xDEADBEEF, 0x7FFFFFFF, 0xFFFFFFFF]
for bogus_id in bogus_ids:
    try:
        bogus_win = Xlib.xobject.drawable.Window(d.display, bogus_id)
        bogus_win.get_geometry()
        d.sync()
        print(f"PASS: GetGeometry(0x{bogus_id:08x}) silently handled")
    except (Xlib.error.BadWindow, Xlib.error.BadDrawable):
        print(f"PASS: GetGeometry(0x{bogus_id:08x}) raised BadWindow/BadDrawable")
    except Exception as e:
        print(f"PASS: GetGeometry(0x{bogus_id:08x}) raised {type(e).__name__}")

# Test 2: GetWindowAttributes on bogus window
for bogus_id in [0xCAFEBABE, 0x12345678]:
    try:
        bogus_win = Xlib.xobject.drawable.Window(d.display, bogus_id)
        bogus_win.get_attributes()
        d.sync()
        print(f"PASS: GetWindowAttributes(0x{bogus_id:08x}) silently handled")
    except (Xlib.error.BadWindow, Xlib.error.BadDrawable):
        print(f"PASS: GetWindowAttributes(0x{bogus_id:08x}) raised error")
    except Exception as e:
        print(f"PASS: GetWindowAttributes(0x{bogus_id:08x}) raised {type(e).__name__}")

# Test 3: FreePixmap on a bogus pixmap ID
for bogus_id in [0xDEAD0001, 0xBEEF0002]:
    try:
        bogus_px = Xlib.xobject.drawable.Pixmap(d.display, bogus_id)
        bogus_px.free()
        d.sync()
        print(f"PASS: FreePixmap(0x{bogus_id:08x}) silently handled")
    except Xlib.error.BadPixmap:
        print(f"PASS: FreePixmap(0x{bogus_id:08x}) raised BadPixmap")
    except Exception as e:
        print(f"PASS: FreePixmap(0x{bogus_id:08x}) raised {type(e).__name__}")

# Test 4: GetAtomName with bogus atom ID
for bogus_atom in [0, 0xFFFFFFFF, 99999999]:
    try:
        name = d.get_atom_name(bogus_atom)
        print(f"PASS: GetAtomName({bogus_atom}) returned {name!r}")
    except Xlib.error.BadAtom:
        print(f"PASS: GetAtomName({bogus_atom}) raised BadAtom")
    except Exception as e:
        print(f"PASS: GetAtomName({bogus_atom}) raised {type(e).__name__}")

# Verify server health
d2 = Xlib.display.Display(':99')
root = d2.screen().root
geom = root.get_geometry()
d2.close()
print(f"PASS: server alive, root={geom.width}x{geom.height}")

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("FUZZING_INVALID_IDS_OK")
