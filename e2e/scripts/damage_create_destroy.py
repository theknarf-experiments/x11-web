import Xlib.display, Xlib.X, Xlib.ext, sys, struct, socket
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    # Query DAMAGE extension presence
    ext_info = d.query_extension("DAMAGE")
    if ext_info is None or ext_info.major_opcode == 0:
        failed += 1; print("FAIL: DAMAGE extension not available")
    else:
        passed += 1; print(f"PASS: DAMAGE ext opcode={ext_info.major_opcode}")
        # Create a simple window for damage tracking
        w = root.create_window(0, 0, 100, 100, 0, 24, Xlib.X.InputOutput)
        w.map()
        d.sync()
        passed += 1; print("PASS: window created for damage tracking")
        w.destroy()
        d.sync()
        passed += 1; print("PASS: damage window destroyed cleanly")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"damage-basic: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
