import Xlib.display, Xlib.X, Xlib.error, sys
d = Xlib.display.Display()
root = d.screen().root
pass_count = 0; fail_count = 0
# BadWindow: get attributes of non-existent window
try:
    from Xlib.protocol import request
    bad_win = d.create_resource_object("window", 0xDEAD)
    try:
        bad_win.get_attributes()
        d.sync()
        fail_count += 1; print("FAIL: no error for bad window")
    except Xlib.error.BadWindow:
        pass_count += 1; print("PASS: BadWindow raised")
    except Exception as e:
        pass_count += 1; print(f"PASS: error raised ({type(e).__name__})")
except Exception as e: fail_count += 1; print(f"FAIL: {e}")
# BadAtom: get name of non-existent atom
try:
    try:
        d.get_atom_name(0xFFFFFF)
        d.sync()
        fail_count += 1; print("FAIL: no error for bad atom")
    except (Xlib.error.BadAtom, Xlib.error.BadValue):
        pass_count += 1; print("PASS: BadAtom raised")
    except Exception as e:
        pass_count += 1; print(f"PASS: error raised ({type(e).__name__})")
except Exception as e: fail_count += 1; print(f"FAIL: {e}")
d.close()
print(f"protocol-errors: pass={pass_count} fail={fail_count}")
