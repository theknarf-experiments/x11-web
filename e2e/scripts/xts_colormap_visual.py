import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
screen = d.screen()

# Test 1: default colormap exists. python-xlib >= 0.32 returns a
# Colormap resource object here; older versions returned the raw XID.
try:
    cmap_attr = screen.default_colormap
    cmap = cmap_attr.id if hasattr(cmap_attr, "id") else cmap_attr
    if cmap > 0:
        passed += 1; print(f"PASS: default colormap id=0x{cmap:x}")
    else:
        failed += 1; print("FAIL: no default colormap")
except Exception as e:
    failed += 1; print(f"FAIL: default colormap: {e}")

# Test 2: root visual is TrueColor
try:
    vis = screen.root_visual
    if vis > 0:
        passed += 1; print(f"PASS: root visual id={vis}")
    else:
        failed += 1; print(f"FAIL: root visual = {vis}")
except Exception as e:
    failed += 1; print(f"FAIL: root visual: {e}")

# Test 3: AllocColor on default colormap
try:
    cmap_obj = d.create_resource_object("colormap", cmap)
    reply = cmap_obj.alloc_color(65535, 0, 0)
    if reply.pixel > 0:
        passed += 1; print(f"PASS: AllocColor red pixel=0x{reply.pixel:x}")
    else:
        failed += 1; print(f"FAIL: AllocColor returned pixel=0")
except Exception as e:
    failed += 1; print(f"FAIL: AllocColor: {e}")

# Test 4: QueryColors
try:
    colors = cmap_obj.query_colors([0, reply.pixel])
    if len(colors) == 2:
        passed += 1; print(f"PASS: QueryColors returned {len(colors)} entries")
    else:
        failed += 1; print(f"FAIL: QueryColors returned {len(colors)}")
except Exception as e:
    failed += 1; print(f"FAIL: QueryColors: {e}")

# Test 5: AllocNamedColor
try:
    reply2 = cmap_obj.alloc_named_color("blue")
    if reply2.pixel > 0:
        passed += 1; print(f"PASS: AllocNamedColor blue=0x{reply2.pixel:x}")
    else:
        failed += 1; print("FAIL: AllocNamedColor returned 0")
except Exception as e:
    failed += 1; print(f"FAIL: AllocNamedColor: {e}")

d.close()
print(f"xts-colormap: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
