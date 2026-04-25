import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
s = d.screen()

# Test AllocColor on default colormap
try:
    reply = s.default_colormap.alloc_color(65535, 0, 0)  # red
    if reply.pixel > 0 or reply.red == 65535:
        passed += 1; print(f"PASS: alloc red pixel={reply.pixel:#x}")
    else:
        failed += 1; print(f"FAIL: unexpected alloc reply")
except Exception as e:
    failed += 1; print(f"FAIL: AllocColor: {e}")

# Test AllocNamedColor
try:
    reply = s.default_colormap.alloc_named_color("blue")
    if reply.pixel > 0 or (reply.exact_blue > 0):
        passed += 1; print(f"PASS: alloc named blue pixel={reply.pixel:#x}")
    else:
        failed += 1; print(f"FAIL: unexpected named alloc reply")
except Exception as e:
    failed += 1; print(f"FAIL: AllocNamedColor: {e}")

# Test QueryColors
try:
    colors = s.default_colormap.query_colors([reply.pixel])
    if len(colors) == 1:
        passed += 1; print(f"PASS: query color r={colors[0].red} g={colors[0].green} b={colors[0].blue}")
    else:
        failed += 1; print(f"FAIL: expected 1 color, got {len(colors)}")
except Exception as e:
    failed += 1; print(f"FAIL: QueryColors: {e}")

# Test LookupColor
try:
    reply = s.default_colormap.lookup_color("green")
    if reply.exact_green > 0:
        passed += 1; print(f"PASS: lookup green exact=({reply.exact_red},{reply.exact_green},{reply.exact_blue})")
    else:
        failed += 1; print(f"FAIL: unexpected lookup reply")
except Exception as e:
    failed += 1; print(f"FAIL: LookupColor: {e}")

d.close()
print(f"colormap: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
