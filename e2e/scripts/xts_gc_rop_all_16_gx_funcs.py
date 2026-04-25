import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root

# Create a pixmap to test ROP operations
pm = root.create_pixmap(32, 32, d.screen().root_depth)

gx_names = [
    "GXclear", "GXand", "GXandReverse", "GXcopy",
    "GXandInverted", "GXnoop", "GXxor", "GXor",
    "GXnor", "GXequiv", "GXinvert", "GXorReverse",
    "GXcopyInverted", "GXorInverted", "GXnand", "GXset"
]

for gx_func in range(16):
    try:
        gc = root.create_gc(function=gx_func, foreground=0xFFFFFF, background=0x000000)
        pm.fill_rectangle(gc, 0, 0, 32, 32)
        d.sync()
        gc.free()
        passed += 1
    except Exception as e:
        failed += 1; print(f"FAIL: {gx_names[gx_func]}: {e}")

if passed == 16:
    print("PASS: all 16 GX functions accepted")
else:
    print(f"PARTIAL: {passed}/16 GX functions ok")

pm.free()
d.close()
print(f"xts-rop: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
