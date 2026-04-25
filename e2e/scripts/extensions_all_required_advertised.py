import Xlib.display, sys
d = Xlib.display.Display()
passed = 0; failed = 0

required_extensions = [
    "RENDER", "RANDR", "SHAPE", "MIT-SHM", "SYNC",
    "COMPOSITE", "DAMAGE", "XFIXES", "XKEYBOARD",
    "DOUBLE-BUFFER", "RECORD", "GLX", "PRESENT",
    "DRI3", "Generic Event Extension", "X-Resource",
    "XTEST", "SECURITY", "XINERAMA",
]

for ext_name in required_extensions:
    ext = d.query_extension(ext_name)
    if ext and ext.present:
        passed += 1; print(f"PASS: {ext_name} (opcode={ext.major_opcode})")
    else:
        failed += 1; print(f"FAIL: {ext_name} not present")

d.close()
print(f"extensions: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
