import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    d.grab_server()
    d.sync()
    passed += 1; print("PASS: GrabServer succeeded")
    d.ungrab_server()
    d.sync()
    passed += 1; print("PASS: UngrabServer succeeded")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-grabserver: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
