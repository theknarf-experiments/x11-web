import Xlib.display, sys
d = Xlib.display.Display()
passed = 0; failed = 0

# X11 extension wire names are case-sensitive. Canonical capitalization
# matches what real X servers and Xorg's xextproto register: "Composite"
# and "Present" are mixed case, while older extensions like "RENDER" and
# "DAMAGE" are uppercase.
required_extensions = [
    "RENDER", "RANDR", "SHAPE", "MIT-SHM", "SYNC",
    "Composite", "DAMAGE", "XFIXES", "XKEYBOARD",
    "DOUBLE-BUFFER", "RECORD", "GLX", "Present",
    "Generic Event Extension", "X-Resource",
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
